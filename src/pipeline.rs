use std::collections::HashMap;

use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::Extent3d,
    },
};
use bevy_ort::{
    models::{
        modnet::{
            modnet_inference,
            Modnet,
            ModnetPlugin
        },
        yolo_v8::{
            yolo_inference,
            BoundingBox,
            Yolo,
            YoloPlugin,
        },
    },
    Onnx,
};
use image::{
    DynamicImage,
    GenericImageView,
    ImageBuffer,
    Luma,
    Rgb,
};
use imageproc::geometric_transformations::{
    rotate_about_center,
    Interpolation,
};
use png::Transformations;
use rayon::prelude::*;

use crate::{
    ffmpeg::FfmpegArgs,
    stream::{
        StreamId,
        StreamDescriptors,
    },
};


pub struct PipelinePlugin;
impl Plugin for PipelinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ModnetPlugin,
            YoloPlugin,
        ));
        app.add_systems(
            Update,
            (
                generate_raw_frames,
                generate_rotated_frames,
                generate_mask_frames,
                generate_yolo_frames,
            )
        );
    }
}


#[derive(Component, Reflect)]
pub struct PipelineConfig {
    pub raw_frames: bool,
    pub rotate_raw_frames: bool,
    pub yolo: bool,                         // https://github.com/ultralytics/ultralytics
    pub repair_frames: bool,                // https://huggingface.co/docs/diffusers/en/optimization/onnx & https://github.com/bnm6900030/swintormer
    pub upsample_frames: bool,              // https://huggingface.co/ssube/stable-diffusion-x4-upscaler-onnx
    pub mask_frames: bool,                  // https://github.com/ZHKKKe/MODNet
    pub light_field_cameras: bool,          // https://github.com/jasonyzhang/RayDiffusion
    pub depth_maps: bool,                   // https://github.com/fabio-sim/Depth-Anything-ONNX
    pub gaussian_cloud: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            raw_frames: true,
            rotate_raw_frames: true,
            yolo: true,
            repair_frames: false,
            upsample_frames: false,
            mask_frames: true,
            light_field_cameras: false,
            depth_maps: false,
            gaussian_cloud: false,
        }
    }
}


#[derive(Bundle, Default, Reflect)]
pub struct StreamSessionBundle {
    pub config: PipelineConfig,
    pub raw_streams: RawStreams,
    pub session: Session,
}

// TODO: use an entity saver to write Session and it's components (e.g. `0/session.ron`)


#[derive(Component, Default, Reflect)]
pub struct Session {
    pub id: usize,
    pub directory: String,
}

impl Session {
    pub fn new(directory: String) -> Self {
        let id = get_next_session_id(&directory);
        let directory = format!("{}/{}", directory, id);
        std::fs::create_dir_all(&directory).unwrap();

        Self { id, directory }
    }

    pub fn from_id(id: usize, directory: String) -> Self {
        let directory = format!("{}/{}", directory, id);

        Self { id, directory }
    }
}


#[derive(Component, Default, Reflect)]
pub struct RawStreams {
    pub streams: Vec<String>,
}

impl RawStreams {
    pub fn load_from_session(session: &Session) -> Self {
        let streams_directory = format!("{}/raw", session.directory);

        let streams = std::fs::read_dir(streams_directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.path().to_str().unwrap().to_string())
            .collect::<Vec<_>>();

        Self {
            streams,
        }
    }
}


// TODO: use the async task pool for all generate systems https://crates.io/crates/bevy-async-task
fn generate_raw_frames(
    mut commands: Commands,
    raw_streams: Query<
        (
            Entity,
            &PipelineConfig,
            &RawStreams,
            &Session,
        ),
        Without<RawFrames>,
    >,
) {
    for (
        entity,
        config,
        raw_streams,
        session,
    ) in raw_streams.iter() {
        if config.raw_frames {
            let run_node = !RawFrames::exists(session);
            let mut raw_frames = RawFrames::load_from_session(session);

            if run_node {
                info!("generating raw frames for session {}", session.id);

                let frame_directory = format!("{}/frames", session.directory);

                raw_streams.streams.par_iter()
                    .for_each(|mp4_path| {
                        let stream_idx = std::path::Path::new(mp4_path).file_stem().unwrap().to_str().unwrap().parse::<usize>().unwrap();
                        let output_directory = format!("{}/{}", frame_directory, stream_idx);
                        std::fs::create_dir_all(&output_directory).unwrap();

                        let _ = FfmpegArgs {
                            mp4_path: mp4_path.clone(),
                            fps: 5,
                            width: 1920,
                            height: 1080,
                            interpolation: "lanczos".to_string(),
                            output_directory,
                        }.run();
                    });

                raw_frames.reload();
            } else {
                info!("raw frames already exist for session {}", session.id);
            }

            commands.entity(entity).insert(raw_frames);
        }
    }
}


fn generate_rotated_frames(
    mut commands: Commands,
    descriptors: Res<StreamDescriptors>,
    raw_frames: Query<
        (
            Entity,
            &PipelineConfig,
            &RawFrames,
            &Session,
        ),
        Without<RotatedFrames>,
    >,
) {
    // TODO: create a caching/loading system wrapper over run_node interior
    for (
        entity,
        config,
        raw_frames,
        session,
    ) in raw_frames.iter() {
        // TODO: get stream descriptor rotation

        if config.rotate_raw_frames {
            let run_node = !RotatedFrames::exists(session);
            let mut rotated_frames = RotatedFrames::load_from_session(session);

            if run_node {
                let rotations: HashMap<StreamId, f32> = descriptors.0.iter()
                    .enumerate()
                    .map(|(id, descriptor)| (StreamId(id), descriptor.rotation.unwrap_or_default()))
                    .collect();

                info!("generating rotated frames for session {}", session.id);

                raw_frames.frames.iter()
                    .for_each(|(stream_id, frames)| {
                        let output_directory = format!("{}/{}", rotated_frames.directory, stream_id.0);
                        std::fs::create_dir_all(&output_directory).unwrap();

                        let frames = frames.par_iter()
                            .map(|frame| {
                                let frame_idx = std::path::Path::new(frame).file_stem().unwrap().to_str().unwrap();
                                let output_path = format!("{}/{}.png", output_directory, frame_idx);

                                rotate_image(
                                    std::path::Path::new(frame),
                                    std::path::Path::new(&output_path),
                                    rotations[stream_id],
                                ).unwrap();

                                output_path
                            })
                            .collect::<Vec<_>>();

                            rotated_frames.frames.insert(*stream_id, frames);
                    });
            } else {
                info!("rotated frames already exist for session {}", session.id);
            }

            commands.entity(entity).insert(rotated_frames);
        }
    }
}


fn generate_mask_frames(
    mut commands: Commands,
    frames: Query<
        (
            Entity,
            &PipelineConfig,
            &RotatedFrames,
            &Session,
        ),
        Without<MaskFrames>,
    >,
    modnet: Res<Modnet>,
    onnx_assets: Res<Assets<Onnx>>,
) {
    for (
        entity,
        config,
        frames,
        session,
    ) in frames.iter() {
        if config.mask_frames {
            if onnx_assets.get(&modnet.onnx).is_none() {
                return;
            }

            let onnx = onnx_assets.get(&modnet.onnx).unwrap();
            let onnx_session_arc = onnx.session.clone();
            let onnx_session_lock = onnx_session_arc.lock().map_err(|e| e.to_string()).unwrap();
            let onnx_session = onnx_session_lock.as_ref().ok_or("failed to get session from ONNX asset").unwrap();

            let run_node = !MaskFrames::exists(session);
            let mut mask_frames = MaskFrames::load_from_session(session);

            if run_node {
                info!("generating mask frames for session {}", session.id);

                frames.frames.keys()
                    .for_each(|stream_id| {
                        let output_directory = format!("{}/{}", mask_frames.directory, stream_id.0);
                        std::fs::create_dir_all(output_directory).unwrap();
                    });

                let mask_images = frames.frames.iter()
                    .map(|(stream_id, frames)| {
                        let frames = frames.iter()
                            .map(|frame| {
                                let mut decoder = png::Decoder::new(std::fs::File::open(frame).unwrap());
                                decoder.set_transformations(Transformations::EXPAND | Transformations::ALPHA);
                                let mut reader = decoder.read_info().unwrap();
                                let mut img_data = vec![0; reader.output_buffer_size()];
                                let _ = reader.next_frame(&mut img_data).unwrap();

                                assert_eq!(reader.info().bytes_per_pixel(), 3);

                                let width = reader.info().width;
                                let height = reader.info().height;

                                // TODO: separate image loading and onnx inference (so the image loading result can be viewed in the pipeline grid view)
                                let image = Image::new(
                                    Extent3d {
                                        width,
                                        height,
                                        depth_or_array_layers: 1,
                                    },
                                    bevy::render::render_resource::TextureDimension::D2,
                                    img_data,
                                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                                    RenderAssetUsages::all(),
                                );

                                let frame_idx = std::path::Path::new(frame).file_stem().unwrap().to_str().unwrap();

                                (
                                    frame_idx,
                                    modnet_inference(
                                        onnx_session,
                                        &[&image],
                                        Some((512, 512)),
                                    ).pop().unwrap(),
                                )
                            })
                            .collect::<Vec<_>>();

                        (stream_id, frames)
                    })
                    .collect::<Vec<_>>();

                mask_images.iter()
                    .for_each(|(stream_id, frames)| {
                        let output_directory = format!("{}/{}", mask_frames.directory, stream_id.0);
                        let mask_paths = frames.iter()
                            .map(|(frame_idx, frame)| {
                                let path = format!("{}/{}.png", output_directory, frame_idx);

                                let buffer = ImageBuffer::<Luma<u8>, Vec<u8>>::from_raw(
                                    frame.width(),
                                    frame.height(),
                                    frame.data.clone(),
                                ).unwrap();

                                let _ = buffer.save(&path);

                                path
                            })
                            .collect::<Vec<_>>();

                        mask_frames.frames.insert(**stream_id, mask_paths);
                    });
            } else {
                info!("mask frames already exist for session {}", session.id);
            }

            commands.entity(entity).insert(mask_frames);
        }
    }
}


fn generate_yolo_frames(
    mut commands: Commands,
    raw_frames: Query<
        (
            Entity,
            &PipelineConfig,
            &RawFrames,
            &Session,
        ),
        Without<YoloFrames>,
    >,
    yolo: Res<Yolo>,
    onnx_assets: Res<Assets<Onnx>>,
) {
    for (
        entity,
        config,
        raw_frames,
        session,
    ) in raw_frames.iter() {
        if config.yolo {
            if onnx_assets.get(&yolo.onnx).is_none() {
                return;
            }

            let onnx = onnx_assets.get(&yolo.onnx).unwrap();
            let onnx_session_arc = onnx.session.clone();
            let onnx_session_lock = onnx_session_arc.lock().map_err(|e| e.to_string()).unwrap();
            let onnx_session = onnx_session_lock.as_ref().ok_or("failed to get session from ONNX asset").unwrap();

            let run_node = !YoloFrames::exists(session);
            let mut yolo_frames = YoloFrames::load_from_session(session);

            if run_node {
                info!("generating yolo frames for session {}", session.id);

                raw_frames.frames.keys()
                    .for_each(|stream_id| {
                        let output_directory = format!("{}/{}", yolo_frames.directory, stream_id.0);
                        std::fs::create_dir_all(output_directory).unwrap();
                    });

                // TODO: support async ort inference (re. progress bars)
                let bounding_box_streams = raw_frames.frames.iter()
                    .map(|(stream_id, frames)| {
                        let frames = frames.iter()
                            .map(|frame| {
                                let mut decoder = png::Decoder::new(std::fs::File::open(frame).unwrap());
                                decoder.set_transformations(Transformations::EXPAND | Transformations::ALPHA);
                                let mut reader = decoder.read_info().unwrap();
                                let mut img_data = vec![0; reader.output_buffer_size()];
                                let _ = reader.next_frame(&mut img_data).unwrap();

                                assert_eq!(reader.info().bytes_per_pixel(), 3);

                                let width = reader.info().width;
                                let height = reader.info().height;

                                // TODO: separate image loading and onnx inference (so the image loading result can be viewed in the pipeline grid view)
                                let image = Image::new(
                                    Extent3d {
                                        width,
                                        height,
                                        depth_or_array_layers: 1,
                                    },
                                    bevy::render::render_resource::TextureDimension::D2,
                                    img_data,
                                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                                    RenderAssetUsages::all(),
                                );

                                let frame_idx = std::path::Path::new(frame).file_stem().unwrap().to_str().unwrap();

                                (
                                    frame_idx,
                                    yolo_inference(
                                        onnx_session,
                                        &image,
                                        0.5,
                                    ),
                                )
                            })
                            .collect::<Vec<_>>();

                        (stream_id, frames)
                    })
                    .collect::<Vec<_>>();

                bounding_box_streams.iter()
                    .for_each(|(stream_id, frames)| {
                        let output_directory = format!("{}/{}", yolo_frames.directory, stream_id.0);
                        let bounding_boxes = frames.iter()
                            .map(|(frame_idx, bounding_boxes)| {
                                let path = format!("{}/{}.json", output_directory, frame_idx);

                                let _ = serde_json::to_writer(std::fs::File::create(path).unwrap(), bounding_boxes);

                                bounding_boxes.clone()
                            })
                            .collect::<Vec<_>>();

                        yolo_frames.frames.insert(**stream_id, bounding_boxes);
                    });
            } else {
                info!("yolo frames already exist for session {}", session.id);
            }

            commands.entity(entity).insert(yolo_frames);
        }
    }
}


// TODO: alphablend frames
#[derive(Component, Default)]
pub struct AlphablendFrames {
    pub frames: HashMap<StreamId, Vec<String>>,
    pub directory: String,
}
impl AlphablendFrames {
    pub fn load_from_session(
        session: &Session,
    ) -> Self {
        let directory = format!("{}/alphablend", session.directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut alphablend_frames = Self {
            frames: HashMap::new(),
            directory,
        };
        alphablend_frames.reload();

        alphablend_frames
    }

    pub fn reload(&mut self) {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|stream_dir| {
                let stream_id = StreamId(stream_dir.path().file_name().unwrap().to_str().unwrap().parse::<usize>().unwrap());

                let frames = std::fs::read_dir(stream_dir.path()).unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("png"))
                    .map(|entry| entry.path().to_str().unwrap().to_string())
                    .collect::<Vec<_>>();

                (stream_id, frames)
            })
            .for_each(|(stream_id, frames)| {
                self.frames.insert(stream_id, frames);
            });
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/alphablend", session.directory);
        std::fs::metadata(output_directory).is_ok()
    }

    pub fn image(&self, _camera: usize, _frame: usize) -> Option<Image> {
        todo!()
    }
}



// TODO: support loading maskframes -> images into a pipeline mask viewer


#[derive(Component, Default)]
pub struct RawFrames {
    pub frames: HashMap<StreamId, Vec<String>>,
    pub directory: String,
}
impl RawFrames {
    pub fn load_from_session(
        session: &Session,
    ) -> Self {
        let directory = format!("{}/frames", session.directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut raw_frames = Self {
            frames: HashMap::new(),
            directory,
        };
        raw_frames.reload();

        raw_frames
    }

    pub fn reload(&mut self) {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|stream_dir| {
                let stream_id = StreamId(stream_dir.path().file_name().unwrap().to_str().unwrap().parse::<usize>().unwrap());

                let frames = std::fs::read_dir(stream_dir.path()).unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("png"))
                    .map(|entry| entry.path().to_str().unwrap().to_string())
                    .collect::<Vec<_>>();

                (stream_id, frames)
            })
            .for_each(|(stream_id, frames)| {
                self.frames.insert(stream_id, frames);
            });
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/frames", session.directory);
        std::fs::metadata(output_directory).is_ok()
    }

    pub fn image(&self, _camera: usize, _frame: usize) -> Option<Image> {
        todo!()
    }
}


#[derive(Component, Default)]
pub struct YoloFrames {
    pub frames: HashMap<StreamId, Vec<Vec<BoundingBox>>>,
    pub directory: String,
}
impl YoloFrames {
    pub fn load_from_session(
        session: &Session,
    ) -> Self {
        let directory = format!("{}/yolo_frames", session.directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut yolo_frames = Self {
            frames: HashMap::new(),
            directory,
        };
        yolo_frames.reload();

        yolo_frames
    }

    pub fn reload(&mut self) {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|stream_dir| {
                let stream_id = StreamId(stream_dir.path().file_name().unwrap().to_str().unwrap().parse::<usize>().unwrap());

                let frames = std::fs::read_dir(stream_dir.path()).unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("json"))
                    .map(|entry| std::fs::File::open(entry.path()).unwrap())
                    .map(|yolo_json_file| {
                        let bounding_boxes: Vec<BoundingBox> = serde_json::from_reader(&yolo_json_file).unwrap();

                        bounding_boxes
                    })
                    .collect::<Vec<_>>();

                // TODO: parse the json at each frame path to get the bounding boxes

                (stream_id, frames)
            })
            .for_each(|(stream_id, frames)| {
                self.frames.insert(stream_id, frames);
            });
    }

    pub fn write(&self) {
        self.frames.iter()
            .for_each(|(stream_id, frames)| {
                let output_directory = format!("{}/{}", self.directory, stream_id.0);
                std::fs::create_dir_all(&output_directory).unwrap();

                frames.iter()
                    .enumerate()
                    .for_each(|(frame_idx, bounding_boxes)| {
                        let path = format!("{}/{}.json", output_directory, frame_idx);
                        let _ = serde_json::to_writer(std::fs::File::create(path).unwrap(), bounding_boxes);
                    });
            });
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/yolo_frames", session.directory);
        std::fs::metadata(output_directory).is_ok()
    }

    pub fn image(&self, _camera: usize, _frame: usize) -> Option<Image> {
        todo!()
    }
}



#[derive(Component, Default)]
pub struct RotatedFrames {
    pub frames: HashMap<StreamId, Vec<String>>,
    pub directory: String,
}
impl RotatedFrames {
    pub fn load_from_session(
        session: &Session,
    ) -> Self {
        let directory = format!("{}/rotated_frames", session.directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut raw_frames = Self {
            frames: HashMap::new(),
            directory,
        };
        raw_frames.reload();

        raw_frames
    }

    pub fn reload(&mut self) {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|stream_dir| {
                let stream_id = StreamId(stream_dir.path().file_name().unwrap().to_str().unwrap().parse::<usize>().unwrap());

                let frames = std::fs::read_dir(stream_dir.path()).unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("png"))
                    .map(|entry| entry.path().to_str().unwrap().to_string())
                    .collect::<Vec<_>>();

                (stream_id, frames)
            })
            .for_each(|(stream_id, frames)| {
                self.frames.insert(stream_id, frames);
            });
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/rotated_frames", session.directory);
        std::fs::metadata(output_directory).is_ok()
    }

    pub fn image(&self, _camera: usize, _frame: usize) -> Option<Image> {
        todo!()
    }
}



#[derive(Component, Default, Reflect)]
pub struct MaskFrames {
    pub frames: HashMap<StreamId, Vec<String>>,
    pub directory: String
}
impl MaskFrames {
    pub fn load_from_session(
        session: &Session,
    ) -> Self {
        let directory = format!("{}/masks", session.directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut mask_frames = Self {
            frames: HashMap::new(),
            directory,
        };
        mask_frames.reload();

        mask_frames
    }

    pub fn reload(&mut self) {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|stream_dir| {
                let stream_id = StreamId(stream_dir.path().file_name().unwrap().to_str().unwrap().parse::<usize>().unwrap());

                let frames = std::fs::read_dir(stream_dir.path()).unwrap()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file() && entry.path().extension().and_then(|s| s.to_str()) == Some("png"))
                    .map(|entry| entry.path().to_str().unwrap().to_string())
                    .collect::<Vec<_>>();

                (stream_id, frames)
            })
            .for_each(|(stream_id, frames)| {
                self.frames.insert(stream_id, frames);
            });
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/masks", session.directory);
        std::fs::metadata(output_directory).is_ok()
    }

    pub fn image(&self, _camera: usize, _frame: usize) -> Option<Image> {
        todo!()
    }
}


#[derive(Default, Clone, Reflect)]
pub struct LightFieldCamera {
    // TODO: intrinsics/extrinsics
}

#[derive(Component, Default, Reflect)]
pub struct LightFieldCameras {
    pub cameras: Vec<LightFieldCamera>,
}



fn get_next_session_id(output_directory: &str) -> usize {
    match std::fs::read_dir(output_directory) {
        Ok(entries) => entries.filter_map(|entry| {
            let entry = entry.ok()?;
                if entry.path().is_dir() {
                    entry.file_name().to_string_lossy().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .max()
            .map_or(0, |max_id| max_id + 1),
        Err(_) => 0,
    }
}


fn rotate_image(
    image_path: &std::path::Path,
    output_path: &std::path::Path,
    angle: f32,
) -> image::ImageResult<()> {
    if angle == 0.0 {
        std::fs::copy(image_path, output_path)?;
        return Ok(());
    }

    let dyn_img = image::open(image_path).unwrap();
    let (w, h) = dyn_img.dimensions();

    let image_bytes = DynamicImage::into_bytes(dyn_img);
    let image_buffer = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_vec(
        w,
        h,
        image_bytes[..].to_vec(),
    ).unwrap();

    let radians = angle.to_radians();

    let rotated_image: ImageBuffer::<Rgb<u8>, Vec<u8>> = rotate_about_center(
        &image_buffer,
        radians,
        Interpolation::Bilinear,
        Rgb([0, 0, 0]),
    );
    rotated_image.save(output_path)?;

    Ok(())
}

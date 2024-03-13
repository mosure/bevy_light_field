use bevy::prelude::*;
use image::DynamicImage;
use rayon::prelude::*;


pub struct PipelinePlugin;
impl Plugin for PipelinePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, generate_annotations);
    }
}


#[derive(Component, Reflect)]
pub struct PipelineConfig {
    pub raw_frames: bool,
    pub subject_refinement: bool,           // https://github.com/onnx/models/tree/main/validated/vision/body_analysis/ultraface
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
            subject_refinement: false,
            repair_frames: false,
            upsample_frames: false,
            mask_frames: false,
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
}


pub trait PipelineNode {
    fn new(session: &Session) -> Self;
    fn exists(session: &Session) -> bool;
}


#[derive(Component, Default, Reflect)]
pub struct RawStreams {
    pub streams: Vec<String>,
}
impl RawStreams {
    pub fn decoders(&self) -> () {
        // TODO: create decoders for each h264 file stream
        todo!()
    }
}


// TODO: add a pipeline config for the session describing which components to run


fn generate_annotations(
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
        let raw_frames = RawFrames::new(session);

        if config.raw_frames {
            let run_node = !RawFrames::exists(session);
            let mut raw_frames = RawFrames::new(session);

            if run_node {
                // TODO: add (identifier == "{}_{}", stream, frame) output to this first stage (forward to mask stage /w image in RAM)
                let frames = raw_streams.streams.par_iter()
                    .map(|stream| {
                        let decoder = todo!();

                        (stream, decoder)
                    })
                    .map(|(stream, decoder)| {
                        let frames: Vec<DynamicImage> = vec![];

                        // TODO: read all frames

                        (stream, frames)
                    })
                    .collect::<Vec<_>>();

                frames.par_iter()
                    .for_each(|(stream, frames)| {
                        frames.iter().enumerate()
                            .for_each(|(i, frame)| {
                                let path = format!(
                                    "{}/frames/{}_{}.png",
                                    session.directory,
                                    stream,
                                    i,
                                );
                                frame.save(path).unwrap();
                            });
                    });
            }
        }

        commands.entity(entity).insert(raw_frames);
    }
}


#[derive(Component, Default, Reflect)]
pub struct RawFrames {
    pub frames: Vec<String>,
}
impl RawFrames {
    // TODO: move new and exists to a trait

    pub fn new(
        session: &Session,
    ) -> Self {
        let output_directory = format!("{}/frames", session.directory);
        std::fs::create_dir_all(&output_directory).unwrap();

        // TODO: load all files that are already in the directory

        Self {
            frames: vec![],
        }
    }

    pub fn exists(
        session: &Session,
    ) -> bool {
        let output_directory = format!("{}/frames", session.directory);
        std::fs::metadata(&output_directory).is_ok()
    }

    pub fn image(&self, camera: usize, frame: usize) -> Option<Image> {
        todo!()
    }
}


#[derive(Component, Default, Reflect)]
pub struct MaskFrames {
    pub frames: Vec<String>,
}
impl MaskFrames {
    pub fn new(
        session: &Session,
    ) -> Self {
        let output_directory = format!("{}/masks", session.directory);
        std::fs::create_dir_all(&output_directory).unwrap();

        Self {
            frames: vec![],
        }
    }

    pub fn image(&self, camera: usize, frame: usize) -> Option<Image> {
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

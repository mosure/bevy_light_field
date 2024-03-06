use async_compat::Compat;
use bevy::{
    prelude::*,
    ecs::system::CommandQueue,
    tasks::{
        block_on,
        futures_lite::future,
        AsyncComputeTaskPool,
        Task,
    },
};
use futures::TryStreamExt;
use openh264::decoder::{
    Decoder,
    DecoderConfig,
};
use retina::{
    client::{
        Credentials,
        Session,
        SessionOptions,
        SetupOptions,
        TcpTransportOptions,
        Transport,
    },
    codec::VideoFrame,
};
use std::sync::Arc;
use url::Url;


const RTSP_URIS: [&str; 2] = [
    "rtsp://rtspstream:17ff228aff57ac78589c5ab00d22435a@zephyr.rtsp.stream/movie",
    "rtsp://rtspstream:827427cceb42214303462d0f4735a6ea@zephyr.rtsp.stream/pattern",
];


struct CapturedFrame {
    index: usize,
    frame: VideoFrame,
}


async fn capture_frame_to_bytes(url: &str, index: usize) -> Result<CapturedFrame, Box<dyn std::error::Error + Send + Sync>> {
    let parsed_url = Url::parse(url)?;

    let username = parsed_url.username();
    let password = parsed_url.password().unwrap_or("");
    let creds = if !username.is_empty() {
        Some(Credentials {
            username: username.into(),
            password: password.into(),
        })
    } else {
        None
    };

    let mut clean_url = parsed_url.clone();
    clean_url.set_username("").unwrap();
    clean_url.set_password(None).unwrap();

    let session_group = Arc::new(retina::client::SessionGroup::default());
    let options = SessionOptions::default()
        .creds(creds)
        .session_group(session_group);

    let mut session = Session::describe(
        clean_url,
        options,
    ).await?;

    let tcp_options = TcpTransportOptions::default();
    let transport = Transport::Tcp(tcp_options);

    let video_stream_index = session.streams().iter().enumerate().find_map(|(i, s)| {
        if s.media() == "video" && s.encoding_name().to_uppercase() == "H264" {
            Some(i)
        } else {
            None
        }
    }).ok_or("No suitable H264 video stream found.")?;

    session.setup(video_stream_index, SetupOptions::default().transport(transport)).await?;

    let described = session.play(retina::client::PlayOptions::default()).await?;
    let mut demuxed = described.demuxed()?;

    if let Some(item) = demuxed.try_next().await? {
        match item {
            retina::codec::CodecItem::VideoFrame(frame) => {
                Ok(CapturedFrame {
                    index,
                    frame,
                })
            },
            _ => Err("Expected a video frame, but got something else.".into()),
        }
    } else {
        Err("No frames were received.".into())
    }
}


#[derive(Component)]
struct ReadRtspFrame(Task<CommandQueue>);


fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, spawn_tasks)
        .add_systems(Update, handle_tasks)
        .run();
}


fn spawn_tasks(
    mut commands: Commands,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    RTSP_URIS.iter()
        .enumerate()
        .for_each(|(index, &url)| {
            let entity = commands.spawn_empty().id();

            let task = thread_pool.spawn(async move {
                let result = futures::executor::block_on(Compat::new(capture_frame_to_bytes(url, index)));
                match result {
                    Ok(captured_frame) => {
                        println!("Captured frame from camera {}:\n{:?}", captured_frame.index, captured_frame.frame);
                    },
                    Err(e) => eprintln!("Failed to capture frame: {}", e),
                }

                let mut command_queue = CommandQueue::default();
                command_queue.push(move |world: &mut World| {
                    world.entity_mut(entity).remove::<ReadRtspFrame>();
                });

                command_queue
            });

            commands.entity(entity).insert(ReadRtspFrame(task));
        });
}


fn handle_tasks(
    mut commands: Commands,
    mut tasks: Query<&mut ReadRtspFrame>,
) {
    for mut task in &mut tasks {
        if let Some(mut commands_queue) = block_on(future::poll_once(&mut task.0)) {
            commands.append(&mut commands_queue);
        }
    }
}



// https://github.com/PortalCloudInc/bevy_video
// #[derive(Component)]
// pub struct VideoDecoder {
//     sender: Mutex<Sender<DecoderMessage>>,
//     next_frame_rgb8: Arc<Mutex<Option<VideoFrame>>>,
//     render_target: Handle<Image>,
// }

// impl VideoDecoder {
//     pub fn create(images: &mut ResMut<Assets<Image>>) -> (Handle<Image>, VideoDecoder) {
//         let render_target = images.add(Self::create_image(12, 12));
//         let (sender, receiver) = channel::<DecoderMessage>();
//         let next_frame_rgb8 = Arc::new(Mutex::new(None));

//         std::thread::spawn({
//             let next_frame_rgb8 = next_frame_rgb8.clone();
//             move || {
//                 let cfg = DecoderConfig::new();
//                 let mut decoder = Decoder::with_config(cfg).expect("Failed to create AVC decoder");
//                 for video_packet in receiver {
//                     let video_packet = match video_packet {
//                         DecoderMessage::Frame(video_packet) => video_packet,
//                         DecoderMessage::Stop => return,
//                     };
//                     let decoded_yuv = decoder.decode(video_packet.as_slice());
//                     let decoded_yuv = match decoded_yuv {
//                         Ok(decoded_yuv) => decoded_yuv,
//                         Err(e) => {
//                             error!("Failed to decode frame: {}", e);
//                             continue;
//                         }
//                     };
//                     let Some(decoded_yuv) = decoded_yuv else { continue };
//                     let (width, height) = decoded_yuv.dimension_rgb();
//                     let mut buffer = vec![0; width * height * 3];

//                     // TODO: Don't convert YUV -> RGB -> BGRA, just make something for YUV -> BGRA
//                     decoded_yuv.write_rgb8(buffer.as_mut_slice());

//                     let frame = VideoFrame {
//                         buffer,
//                         width,
//                         height,
//                     };

//                     next_frame_rgb8.lock().unwrap().replace(frame);
//                 }
//             }
//         });

//         let video_decoder = Self {
//             sender: Mutex::new(sender),
//             next_frame_rgb8,
//             render_target: render_target.clone_weak(),
//         };

//         (render_target, video_decoder)
//     }

//     fn create_image(width: u32, height: u32) -> Image {
//         let size = Extent3d {
//             width,
//             height,
//             ..default()
//         };

//         let mut image = Image {
//             texture_descriptor: TextureDescriptor {
//                 label: Some("Video stream render target"),
//                 size,
//                 dimension: TextureDimension::D2,
//                 format: TextureFormat::Bgra8UnormSrgb,
//                 mip_level_count: 1,
//                 sample_count: 1,
//                 usage: TextureUsages::COPY_DST
//                     | TextureUsages::TEXTURE_BINDING
//                     | TextureUsages::RENDER_ATTACHMENT,
//             },
//             ..default()
//         };
//         image.resize(size);
//         image
//     }

//     pub fn add_video_packet(&self, video_packet: Vec<u8>) {
//         self.sender
//             .lock()
//             .expect("Could not get lock on sender")
//             .send(DecoderMessage::Frame(video_packet))
//             .expect("Could not send packet to decoder thread");
//     }

//     pub(crate) fn take_frame_rgb8(&self) -> Option<VideoFrame> {
//         self.next_frame_rgb8.lock().unwrap().take()
//     }

//     pub fn get_render_target(&self) -> Handle<Image> {
//         self.render_target.clone_weak()
//     }
// }


// pub fn apply_decode(
//     mut commands: Commands,
//     mut images: ResMut<Assets<Image>>,
//     decoders: Query<(Entity, &VideoDecoder)>,
// ) {
//     for (entity, decoder) in decoders.iter() {
//         let frame = decoder.take_frame_rgb8();
//         if let Some(frame) = frame {
//             let VideoFrame {
//                 buffer,
//                 width,
//                 height,
//             } = frame;

//             let image_handle = decoder.get_render_target();
//             let image = match images.get_mut(&image_handle) {
//                 Some(image) => image,
//                 None => {
//                     info!(
//                         "Image gone. Removing video decoder from {:?} and stopping decode thread",
//                         entity
//                     );
//                     commands.entity(entity).remove::<VideoDecoder>();
//                     continue;
//                 }
//             };

//             if image.texture_descriptor.size.width != width as u32
//                 || image.texture_descriptor.size.height != height as u32
//             {
//                 image.resize(Extent3d {
//                     width: width as u32,
//                     height: height as u32,
//                     ..default()
//                 });
//             }

//             for (dest, src) in image.data.chunks_exact_mut(4).zip(buffer.chunks_exact(3)) {
//                 dest.copy_from_slice(&[src[2], src[1], src[0], 255]);
//             }
//         }
//     }
// }

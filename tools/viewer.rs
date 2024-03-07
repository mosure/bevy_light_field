use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Error};
use async_compat::Compat;
use bevy::{
    prelude::*,
    app::AppExit,
    ecs::system::CommandQueue,
    tasks::{
        block_on,
        futures_lite::future,
        AsyncComputeTaskPool,
        Task,
    },
};
use futures::TryStreamExt;
use openh264::{
    decoder::Decoder,
    nal_units,
};
use retina::{
    client::{
        Credentials,
        Demuxed,
        Playing,
        Session,
        SessionOptions,
        SetupOptions,
        TcpTransportOptions,
        Transport,
    },
    codec::VideoFrame,
};
use url::Url;


const RTSP_URIS: [&str; 2] = [
    // "rtsp://localhost:554/lizard",
    // "rtsp://rtspstream:17ff228aff57ac78589c5ab00d22435a@zephyr.rtsp.stream/movie",
    // "rtsp://rtspstream:827427cceb42214303462d0f4735a6ea@zephyr.rtsp.stream/pattern",
    "rtsp://192.168.1.23/user=admin&password=admin123&channel=1&stream=0.sdp?",
    "rtsp://192.168.1.24/user=admin&password=admin123&channel=1&stream=0.sdp?",
];


fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Update, press_esc_close)
        .add_systems(Startup, spawn_tasks)
        .add_systems(Update, task_loop)
        .add_systems(Update, handle_tasks)
        .run();
}


#[derive(Component)]
struct ReadRtspFrame(Task<CommandQueue>);

#[derive(Component, Clone)]
struct RtspStream {
    uri: String,
    index: usize,
    demuxed: Arc<Mutex<Option<Demuxed>>>,
    decoder: Arc<Mutex<Option<Decoder>>>,
}


// TODO: decouple bevy async tasks and the multi-stream RTSP handling
//       a bevy system should query the readiness of a frame for each RTSP stream and update texture as needed
//       the RTSP handling should be a separate async tokio pool that updates the readiness of each RTSP stream /w buffer for transfer to texture


fn spawn_tasks(
    mut commands: Commands,
) {
    RTSP_URIS.iter()
        .enumerate()
        .for_each(|(index, &url)| {
            let entity = commands.spawn_empty().id();

            let api = openh264::OpenH264API::from_source();
            let decoder = Decoder::new(api).unwrap();

            let rtsp_stream = RtspStream {
                uri: url.to_string(),
                index,
                demuxed: Arc::new(Mutex::new(None)),
                decoder: Arc::new(Mutex::new(decoder.into())),
            };
            let demuxed_arc = rtsp_stream.demuxed.clone();

            let session_result = futures::executor::block_on(Compat::new(create_session(&url)));
            match session_result {
                Ok(playing) => {
                    let mut demuxed = demuxed_arc.lock().unwrap();
                    *demuxed = Some(playing.demuxed().unwrap());

                    println!("created demuxer for {}", url);
                },
                Err(e) => panic!("Failed to create session: {}", e),
            }

            queue_rtsp_frame(
                &rtsp_stream,
                &mut commands,
                entity,
            );

            commands.entity(entity).insert(rtsp_stream);
        });
}


fn task_loop(
    mut commands: Commands,
    streams: Query<
        (
            Entity,
            &RtspStream,
        ),
        Without<ReadRtspFrame>,
    >,
) {
    for (entity, rtsp_stream) in &mut streams.iter() {
        queue_rtsp_frame(
            rtsp_stream,
            &mut commands,
            entity,
        );
    }
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


fn convert_h264(data: &mut [u8]) -> Result<(), Error> {
    let mut i = 0;
    while i < data.len() - 3 {
        // Replace each NAL's length with the Annex B start code b"\x00\x00\x00\x01".
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        data[i] = 0;
        data[i + 1] = 0;
        data[i + 2] = 0;
        data[i + 3] = 1;
        i += 4 + len;
        if i > data.len() {
            bail!("partial NAL body");
        }
    }
    if i < data.len() {
        bail!("partial NAL length");
    }
    Ok(())
}

fn queue_rtsp_frame(
    rtsp_stream: &RtspStream,
    commands: &mut Commands,
    entity: Entity,
) {
    let idx = rtsp_stream.index;
    let demuxed_arc = rtsp_stream.demuxed.clone();
    let decoder_arc = rtsp_stream.decoder.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        let result = futures::executor::block_on(Compat::new(capture_frame(demuxed_arc.clone())));

        // TODO: clean this mess
        match result {
            Ok(frame) => {
                let mut decoder_lock = decoder_arc.lock().unwrap();
                let decoder = decoder_lock.as_mut().unwrap();

                let mut data = frame.into_data();
                let annex_b_convert = convert_h264(&mut data);

                match annex_b_convert {
                    Ok(_) => for packet in nal_units(&data) {
                        let result = decoder.decode(packet);

                        match result {
                            Ok(decoded_frame) => {
                                println!("decoded frame from stream {}", idx);
                                // TODO: populate 'latest frame' with new data
                            },
                            Err(e) => println!("failed to decode frame for stream {}: {}", idx, e),
                        }
                    },
                    Err(e) => println!("failed to convert NAL unit to Annex B format: {}", e)
                }
            },
            Err(e) => println!("failed to capture frame for stream {}: {}", idx, e),
        }

        let mut command_queue = CommandQueue::default();
        command_queue.push(move |world: &mut World| {
            world.entity_mut(entity).remove::<ReadRtspFrame>();
        });

        command_queue
    });

    commands.entity(entity).insert(ReadRtspFrame(task));
}

async fn create_session(url: &str) -> Result<Session<Playing>, Box<dyn std::error::Error + Send + Sync>> {
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

    let described = session.play(
        retina::client::PlayOptions::default()
            .enforce_timestamps_with_max_jump_secs(NonZeroU32::new(10).unwrap())
    ).await?;

    Ok(described)
}

async fn capture_frame(demuxed: Arc<Mutex<Option<Demuxed>>>) -> Result<VideoFrame, Box<dyn std::error::Error + Send + Sync>> {
    let mut demux_lock = demuxed.lock().unwrap();
    let demuxed = demux_lock.as_mut().unwrap();
    if let Some(item) = demuxed.try_next().await? {
        match item {
            retina::codec::CodecItem::VideoFrame(frame) => {
                Ok(frame)
            },
            retina::codec::CodecItem::MessageFrame(frame) => {
                println!("Received message frame: {:?}", frame);
                Err("Received message frame.".into())
            },
            _ => Err("Expected a video frame, but got something else".into()),
        }
    } else {
        Err("No frames were received.".into())
    }
}


fn press_esc_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: EventWriter<AppExit>
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.send(AppExit);
    }
}

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Error};
use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{
            Extent3d,
            TextureDescriptor,
            TextureDimension,
            TextureFormat,
            TextureUsages,
        },
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
        UdpTransportOptions,
        Transport,
    },
    codec::VideoFrame,
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    runtime::Handle,
    sync::mpsc,
};
use url::Url;

use crate::{
    mp4::Mp4Writer,
    pipeline::Session as PipelineSession,
};


pub struct RtspStreamPlugin {
    pub stream_config: String,
}

impl Plugin for RtspStreamPlugin {
    fn build(&self, app: &mut App) {
        let config = std::fs::File::open(&self.stream_config).unwrap();
        let stream_uris = serde_json::from_reader::<_, StreamUris>(config).unwrap();

        app
            .insert_resource(stream_uris)
            .init_resource::<RtspStreamManager>()
            .add_systems(PreStartup, create_streams)
            .add_systems(Update, create_streams_from_descriptors)
            .add_systems(Update, apply_decode);
    }
}

fn create_streams(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    stream_uris: Res<StreamUris>,
) {
    stream_uris.0.iter()
        .enumerate()
        .for_each(|(index, descriptor)| {
            let rtsp_stream = RtspStreamHandle::new(
                descriptor.clone(),
                StreamId(index),
                &mut images,
            );

            commands.spawn(rtsp_stream);
        });
}


fn create_streams_from_descriptors(
    mut commands: Commands,
    stream_manager: Res<RtspStreamManager>,
    descriptors: Query<
        (
            Entity,
            &RtspStreamHandle,
        ),
        Without<RtspStreamCreated>,
    >,
) {
    for (entity, descriptor) in descriptors.iter() {
        assert!(!stream_manager.contains(descriptor.id), "stream.id already exists");

        commands.entity(entity).insert(RtspStreamCreated);
        stream_manager.add_stream(RtspStream::new(descriptor.clone()));
    }
}


pub fn apply_decode(
    mut images: ResMut<Assets<Image>>,
    descriptors: Query<&RtspStreamHandle>,
) {
    for descriptor in descriptors.iter() {
        let frame = descriptor.take_frame();
        if let Some(frame) = frame {
            let image_handle = descriptor.get_target();
            let image = images.get_mut(&image_handle).unwrap();

            let Bgra8Frame {
                width,
                height,
                data,
            } = frame;

            if image.texture_descriptor.size.width != u32::from(width)
            || image.texture_descriptor.size.height != u32::from(height)
            {
                image.resize(Extent3d {
                    width: u32::from(width),
                    height: u32::from(height),
                    ..default()
                });
            }

            image.data = data;
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct StreamId(pub usize);

#[derive(Debug)]
pub enum RecordingCommand {
    StartRecording(File),
    StopRecording,
}


#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub enum StreamTransport {
    #[default]
    Tcp,
    Udp,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StreamDescriptor {
    pub uri: String,

    #[serde(default)]
    pub transport: StreamTransport,

    pub visible: Option<bool>,
    pub person_detection: Option<bool>,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct StreamUris(pub Vec<StreamDescriptor>);


#[derive(Component, Clone)]
pub struct RtspStreamHandle {
    pub descriptor: StreamDescriptor,
    pub id: StreamId,
    pub image: bevy::asset::Handle<Image>,
    latest_frame: Arc<Mutex<Option<Bgra8Frame>>>,
    recording_sender: Arc<Mutex<Option<mpsc::Sender<RecordingCommand>>>>,
}

impl RtspStreamHandle {
    pub fn new(
        descriptor: StreamDescriptor,
        id: StreamId,
        images: &mut Assets<Image>,
    ) -> Self {
        let size = Extent3d {
            width: 32,
            height: 32,
            ..default()
        };

        // TODO: use a default 'stream loading' texture

        let mut image = Image {
            asset_usage: RenderAssetUsages::all(),
            texture_descriptor: TextureDescriptor {
                label: None,
                size,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                mip_level_count: 1,
                sample_count: 1,
                usage: TextureUsages::COPY_DST
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[TextureFormat::Rgba8UnormSrgb],
            },
            ..default()
        };
        image.resize(size);

        let image = images.add(image);

        Self {
            descriptor,
            id,
            image,
            latest_frame: Arc::new(Mutex::new(None)),
            recording_sender: Arc::new(Mutex::new(None)),
        }
    }

    fn take_frame(&self) -> Option<Bgra8Frame> {
        self.latest_frame.lock().unwrap().take()
    }

    pub fn get_target(&self) -> bevy::asset::Handle<Image> {
        self.image.clone()
    }
}


#[derive(Component)]
struct RtspStreamCreated;


#[derive(Debug)]
struct Bgra8Frame {
    width: NonZeroU32,
    height: NonZeroU32,
    data: Vec<u8>,
}


#[derive(Resource)]
pub struct RtspStreamManager {
    stream_handles: Arc<Mutex<Vec<RtspStreamHandle>>>,
    handle: Handle,
}

impl FromWorld for RtspStreamManager {
    fn from_world(_world: &mut World) -> Self {

        // TODO: upgrade to [bevy-tokio-tasks](https://github.com/EkardNT/bevy-tokio-tasks) to share tokio runtime between rtsp and inference - waiting on: https://github.com/pykeio/ort/pull/174
        let mut runtime = tokio::runtime::Builder::new_multi_thread();
        runtime.enable_all();
        let runtime = runtime.build().unwrap();
        let handle = runtime.handle().clone();

        std::thread::spawn(move || {
            runtime.block_on(async {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            });
        });

        Self {
            stream_handles: Arc::new(Mutex::new(vec![])),
            handle,
        }
    }
}

impl RtspStreamManager {
    pub fn contains(&self, id: StreamId) -> bool {
        self.stream_handles.lock().unwrap().iter().any(|s: &RtspStreamHandle| s.id == id)
    }

    pub fn add_stream(&self, stream: RtspStream) {
        self.stream_handles.lock().unwrap().push(stream.handle.clone());

        self.handle.spawn(async move {
            let mut stream = stream;
            loop {
                // TODO: print connection errors
                let _ = stream.run().await;
            }
        });
    }

    pub fn start_recording(&self, session: &PipelineSession) {
        let output_directory = format!("{}/raw", session.directory);
        std::fs::create_dir_all(&output_directory).unwrap();

        let stream_handles = self.stream_handles.lock().unwrap();
        for descriptor in stream_handles.iter() {
            let filename = format!("{}.mp4", descriptor.id.0);
            let filepath = format!("{}/{}", output_directory, filename);

            let send_channel = descriptor.recording_sender.lock().unwrap();

            if send_channel.is_none() {
                println!("no recording sender for stream {}", descriptor.id.0);
                continue;
            }

            let sender_clone = send_channel.as_ref().unwrap().clone();

            self.handle.block_on(async move {
                let file = File::create(&filepath).await.unwrap();
                let _ = sender_clone.send(RecordingCommand::StartRecording(file)).await;
            });
        }
    }

    pub fn stop_recording(&self) -> Vec<String> {
        let mut filepaths = vec![];

        let stream_handles = self.stream_handles.lock().unwrap();
        for descriptor in stream_handles.iter() {
            let send_channel = descriptor.recording_sender.lock().unwrap();

            if send_channel.is_none() {
                println!("no recording sender for stream {}", descriptor.id.0);
                continue;
            }

            let sender_clone = send_channel.as_ref().unwrap().clone();

            self.handle.block_on(async move {
                let _ = sender_clone.send(RecordingCommand::StopRecording).await;
            });

            filepaths.push(format!("{}.mp4", descriptor.id.0));
        }

        filepaths
    }
}



pub struct RtspStream {
    pub handle: RtspStreamHandle,
    decoder: Option<Decoder>,
    demuxed: Option<Demuxed>,
    writer: Option<Mp4Writer<File>>,
}

impl RtspStream {
    pub fn new(handle: RtspStreamHandle) -> Self {
        let api = openh264::OpenH264API::from_source();
        let decoder = Decoder::new(api).ok();

        Self {
            handle,
            decoder,
            demuxed: None,
            writer: None,
        }
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>{
        let (session, stream_idx) = create_session(&self.handle.descriptor).await?;
        self.demuxed = session.demuxed()?.into();

        let (sender, mut receiver) = mpsc::channel(1);

        {
            let mut send_channel = self.handle.recording_sender.lock().unwrap();
            *send_channel = sender.into();
        }

        loop {
            let frame = self.capture_frame().await?;

            if let Ok(command) = receiver.try_recv() {
                match command {
                    RecordingCommand::StartRecording(file) => {
                        if let Some(writer) = self.writer.take() {
                            writer.finish().await.ok();
                        }

                        self.writer = Mp4Writer::new(
                            None,
                            true,
                            file,
                        ).await.ok();

                        println!("writing stream {}", self.handle.id.0);
                    },
                    RecordingCommand::StopRecording => {
                        if let Some(writer) = self.writer.take() {
                            println!("stopped recording stream {}", self.handle.id.0);
                            writer.finish().await.ok();
                        }
                    },
                }
            }

            {
                if let Some(writer) = self.writer.as_mut() {
                    writer.video(
                        &self.demuxed.as_mut().unwrap().streams()[stream_idx],
                        &frame,
                    ).await?;
                }
            }

            // TODO: enable/disable decoding based on whether the live frames are being used

            let mut data = frame.into_data();
            convert_h264(&mut data)?;

            for packet in nal_units(&data) {
                let result = self.decoder.as_mut().unwrap().decode(packet);
                let decoded_frame = result?;

                if let Some(frame) = decoded_frame {
                    let image_size = frame.dimension_rgb();

                    {
                        let mut locked_sink = self.handle.latest_frame.lock().unwrap();
                        match *locked_sink {
                            Some(ref mut sink) => {
                                assert_eq!(u32::from(sink.width), image_size.0 as u32, "frame width mismatch - stream size changes are not supported yet.");
                                assert_eq!(u32::from(sink.height), image_size.1 as u32, "frame height mismatch - stream size changes are not supported yet.");

                                let data = sink.data.as_mut();
                                frame.write_rgba8(data);
                            },
                            None => {
                                let mut data = vec![0; image_size.0 * image_size.1 * 4];

                                frame.write_rgba8(&mut data);

                                let bgra = Bgra8Frame {
                                    width: NonZeroU32::new(image_size.0 as u32).unwrap(),
                                    height: NonZeroU32::new(image_size.1 as u32).unwrap(),
                                    data,
                                };

                                // TODO: write streams into a frame texture array (stream, channel, width, height)

                                *locked_sink = Some(bgra);
                            },
                        }
                    }
                }
            }
        }
    }

    async fn capture_frame(&mut self) -> Result<VideoFrame, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(item) = self.demuxed.as_mut().unwrap().try_next().await? {
            match item {
                retina::codec::CodecItem::VideoFrame(frame) => {
                    Ok(frame)
                },
                _ => Err("expected a video frame, but got something else".into()),
            }
        } else {
            Err("no frames were received.".into())
        }
    }
}


async fn create_session(descriptor: &StreamDescriptor) -> Result<
    (Session<Playing>, usize),
    Box<dyn std::error::Error + Send + Sync>
> {
    let parsed_url = Url::parse(&descriptor.uri)?;

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

    let transport = match descriptor.transport {
        StreamTransport::Tcp => Transport::Tcp(TcpTransportOptions::default()),
        StreamTransport::Udp => Transport::Udp(UdpTransportOptions::default()),
    };

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

    Ok((described, video_stream_index))
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

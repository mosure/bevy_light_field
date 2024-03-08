use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Error};
use bevy::{
    prelude::*,
    render::render_resource::Extent3d,
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
use tokio::runtime::{
    Handle,
    Runtime,
};
use url::Url;


pub struct RtspStreamPlugin;
impl Plugin for RtspStreamPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<RtspStreamManager>()
            .add_systems(Update, create_streams_from_descriptors)
            .add_systems(Update, apply_decode);
    }
}


fn create_streams_from_descriptors(
    mut commands: Commands,
    stream_manager: Res<RtspStreamManager>,
    descriptors: Query<
        (
            Entity,
            &RtspStreamDescriptor,
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
    descriptors: Query<&RtspStreamDescriptor>,
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


#[derive(Debug, Clone, Copy, PartialEq, Reflect)]
pub struct StreamId(pub usize);

#[derive(Debug, Component, Clone)]
pub struct RtspStreamDescriptor {
    pub uri: String,
    pub id: StreamId,
    pub image: bevy::asset::Handle<Image>,
    latest_frame: Arc<Mutex<Option<Bgra8Frame>>>,
}

impl RtspStreamDescriptor {
    pub fn new(
        uri: String,
        id: StreamId,
        image: bevy::asset::Handle<Image>,
    ) -> Self {
        Self {
            uri,
            id,
            image,
            latest_frame: Arc::new(Mutex::new(None)),
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
    stream_descriptors: Arc<Mutex<Vec<RtspStreamDescriptor>>>,
    handle: Handle,
}

impl FromWorld for RtspStreamManager {
    fn from_world(_world: &mut World) -> Self {
        let runtime = Runtime::new().unwrap();
        let handle = runtime.handle().clone();

        std::thread::spawn(move || {
            runtime.block_on(async {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            });
        });

        Self {
            stream_descriptors: Arc::new(Mutex::new(vec![])),
            handle,
        }
    }
}

impl RtspStreamManager {
    pub fn contains(&self, id: StreamId) -> bool {
        self.stream_descriptors.lock().unwrap().iter().any(|s: &RtspStreamDescriptor| s.id == id)
    }

    pub fn add_stream(&self, stream: RtspStream) {
        self.stream_descriptors.lock().unwrap().push(stream.descriptor.clone());

        self.handle.spawn(async move {
            let mut stream = stream;
            let _ = stream.run().await;
        });
    }
}



pub struct RtspStream {
    pub descriptor: RtspStreamDescriptor,
    decoder: Option<Decoder>,
    demuxed: Option<Demuxed>,
}

impl RtspStream {
    pub fn new(descriptor: RtspStreamDescriptor) -> Self {
        let api = openh264::OpenH264API::from_source();
        let decoder = Decoder::new(api).ok();

        Self {
            descriptor,
            decoder,
            demuxed: None,
        }
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>{
        let session = create_session(&self.descriptor.uri).await?;
        self.demuxed = session.demuxed()?.into();

        loop {
            let frame = self.capture_frame().await?;

            let mut data = frame.into_data();
            convert_h264(&mut data)?;

            for packet in nal_units(&data) {
                let result = self.decoder.as_mut().unwrap().decode(packet);
                let decoded_frame = result?;

                if let Some(frame) = decoded_frame {
                    let image_size = frame.dimension_rgb();

                    {
                        let mut locked_sink = self.descriptor.latest_frame.lock().unwrap();
                        match *locked_sink {
                            Some(ref mut sink) => {
                                assert_eq!(u32::from(sink.width), image_size.0 as u32, "frame width mismatch - stream size changes are not supported yet.");
                                assert_eq!(u32::from(sink.height), image_size.1 as u32, "frame height mismatch - stream size changes are not supported yet.");

                                let mut data = sink.data.as_mut();
                                frame.write_rgba8(&mut data);
                            },
                            None => {
                                let mut data = vec![0; image_size.0 * image_size.1 * 4];

                                frame.write_rgba8(&mut data);

                                let bgra = Bgra8Frame {
                                    width: NonZeroU32::new(image_size.0 as u32).unwrap(),
                                    height: NonZeroU32::new(image_size.1 as u32).unwrap(),
                                    data,
                                };

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

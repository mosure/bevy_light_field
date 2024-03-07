use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::thread::Thread;

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
use tokio::runtime::Runtime;
use url::Url;


pub struct RtspStreamPlugin;
impl Plugin for RtspStreamPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_systems(Update, create_streams_from_descriptors);
    }
}


#[derive(Debug, Clone, Copy)]
pub struct StreamId(pub usize);

#[derive(Component)]
pub struct RtspStreamDescriptor {
    pub uri: String,
    pub id: StreamId,
}

#[derive(Component)]
struct RtspStreamCreated;


#[derive(Resource)]
pub struct RtspStreamManager {
    runtime: Runtime,
    root: Thread,
}

impl RtspStreamManager {
    pub fn contains(&self, id: StreamId) -> bool {
        false
    }

    pub fn get_latest_frame(&self, id: StreamId) -> Option<u8> {
        None
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
        assert!(stream_manager.contains(descriptor.id), "Stream already exists");


        commands.entity(entity).insert(RtspStreamCreated);


    }
}




pub struct RtspStream {
    pub uri: String,
    pub id: StreamId,
    demuxed: Option<Demuxed>,
    decoder: Option<Decoder>,
}

impl RtspStream {
    pub fn new(uri: &str, id: StreamId) -> Self {
        Self {
            uri: uri.to_string(),
            id,
            demuxed: None,
            decoder: None,
        }
    }
}


#[derive(Default)]
pub struct RtspStreams(pub Vec<RtspStream>);

impl RtspStreams {
    pub fn new(urls: &[&str]) -> Self {
        Self(
            urls.iter()
                .enumerate()
                .map(|(i, &uri)| RtspStream::new(uri, StreamId(i)))
                .collect()
        )
    }
}


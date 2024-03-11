use bevy::prelude::*;

#[cfg(feature = "person_matting")]
pub mod matting;

pub mod materials;
pub mod mp4;
pub mod stream;


pub struct LightFieldPlugin {
    pub stream_config: String,
}

impl Plugin for LightFieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(materials::StreamMaterialsPlugin);
        app.add_plugins(stream::RtspStreamPlugin {
            stream_config: self.stream_config.clone(),
        });
    }
}

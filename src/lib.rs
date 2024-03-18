use bevy::prelude::*;
use bevy_ort::BevyOrtPlugin;

pub mod ffmpeg;
pub mod grid_view;
pub mod materials;
pub mod matting;
pub mod mp4;
pub mod person_detect;
pub mod pipeline;
pub mod stream;
pub mod yolo;


pub struct LightFieldPlugin {
    pub stream_config: String,
}

impl Plugin for LightFieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BevyOrtPlugin);

        app.add_plugins(grid_view::GridViewPlugin);
        app.add_plugins(materials::StreamMaterialsPlugin);
        app.add_plugins(person_detect::PersonDetectPlugin);
        app.add_plugins(pipeline::PipelinePlugin);
        app.add_plugins(stream::RtspStreamPlugin {
            stream_config: self.stream_config.clone(),
        });
        app.add_plugins(yolo::YoloPlugin);
    }
}

use bevy::prelude::*;
use bevy_ort::{
    Onnx,
    models::yolo_v8::Yolo,
};


pub struct YoloPlugin;
impl Plugin for YoloPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Yolo>();
        app.add_systems(Startup, load_yolo);
    }
}

fn load_yolo(
    asset_server: Res<AssetServer>,
    mut modnet: ResMut<Yolo>,
) {
    let modnet_handle: Handle<Onnx> = asset_server.load("models/yolov8n.onnx");
    modnet.onnx = modnet_handle;
}

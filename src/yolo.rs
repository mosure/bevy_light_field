use bevy::prelude::*;
use bevy_ort::Onnx;


pub struct YoloPlugin;
impl Plugin for YoloPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<YoloV8>();
        app.add_systems(Startup, load_yolo_v8);
    }
}


#[derive(Resource, Default)]
pub struct YoloV8 {
    pub onnx: Handle<Onnx>,
}

fn load_yolo_v8(
    asset_server: Res<AssetServer>,
    mut modnet: ResMut<YoloV8>,
) {
    let modnet_handle: Handle<Onnx> = asset_server.load("yolov8n.onnx");
    modnet.onnx = modnet_handle;
}

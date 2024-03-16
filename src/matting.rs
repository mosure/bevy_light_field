use bevy::{
    prelude::*,
    ecs::system::CommandQueue,
    tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task},
};
use bevy_ort::{
    BevyOrtPlugin,
    inputs,
    models::modnet::{
        images_to_modnet_input,
        modnet_output_to_luma_images,
    },
    Onnx,
};

use crate::{
    materials::foreground::ForegroundMaterial,
    stream::StreamId,
};


#[derive(Component, Clone, Debug, Reflect)]
pub struct MattedStream {
    pub stream_id: StreamId,
    pub input: Handle<Image>,
    pub output: Handle<Image>,
    pub material: Handle<ForegroundMaterial>,
}


#[derive(Resource, Default, Clone)]
pub struct InferenceSize(pub (u32, u32));

pub struct MattingPlugin {
    pub max_inference_size: InferenceSize,
}

impl MattingPlugin {
    pub fn new(max_inference_size: (u32, u32)) -> Self {
        MattingPlugin {
            max_inference_size: InferenceSize(max_inference_size),
        }
    }
}

impl Plugin for MattingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BevyOrtPlugin);
        app.register_type::<MattedStream>();
        app.init_resource::<Modnet>();
        app.insert_resource(self.max_inference_size.clone());
        app.add_systems(Startup, load_modnet);
        app.add_systems(Update, matting_inference);
    }
}


#[derive(Resource, Default)]
pub struct Modnet {
    pub onnx: Handle<Onnx>,
}


fn load_modnet(
    asset_server: Res<AssetServer>,
    mut modnet: ResMut<Modnet>,
) {
    let modnet_handle: Handle<Onnx> = asset_server.load("modnet_photographic_portrait_matting.onnx");
    modnet.onnx = modnet_handle;
}


#[derive(Default)]
struct ModnetComputePipeline(Option<Task<CommandQueue>>);


fn matting_inference(
    mut commands: Commands,
    images: Res<Assets<Image>>,
    modnet: Res<Modnet>,
    matted_streams: Query<
        (
            Entity,
            &MattedStream,
        )
    >,
    onnx_assets: Res<Assets<Onnx>>,
    mut pipeline_local: Local<ModnetComputePipeline>,
    inference_size: Res<InferenceSize>,
) {
    if let Some(pipeline) = pipeline_local.0.as_mut() {
        if let Some(mut commands_queue) = block_on(future::poll_once(pipeline)) {
            commands.append(&mut commands_queue);
            pipeline_local.0 = None;
        }

        return;
    }

    if matted_streams.is_empty() {
        return;
    }

    let thread_pool = AsyncComputeTaskPool::get();

    let (inputs, outputs): (Vec<_>, Vec<_>) = matted_streams.iter()
        .map(|(_, matted_stream)| {
            let input = images.get(matted_stream.input.clone()).unwrap();
            let output = (matted_stream.output.clone(), matted_stream.material.clone());
            (input, output)
        })
        .unzip();

    let uninitialized = inputs.iter().any(|image| image.size() == (32, 32).into());
    if uninitialized {
        return;
    }

    let input = images_to_modnet_input(
        inputs.as_slice(),
        inference_size.0.into(),
    );

    if onnx_assets.get(&modnet.onnx).is_none() {
        return;
    }

    let onnx = onnx_assets.get(&modnet.onnx).unwrap();
    let session_arc = onnx.session.clone();

    let task = thread_pool.spawn(async move {
        let mask_images: Result<Vec<Image>, String> = (|| {
            let session_lock = session_arc.lock().map_err(|e| e.to_string())?;
            let session = session_lock.as_ref().ok_or("failed to get session from ONNX asset")?;

            let input_values = inputs!["input" => input.view()].map_err(|e| e.to_string())?;
            let outputs = session.run(input_values).map_err(|e| e.to_string());

            let binding = outputs.ok().unwrap();
            let output_value: &ort::Value = binding.get("output").unwrap();

            Ok(modnet_output_to_luma_images(output_value))
        })();

        match mask_images {
            Ok(mask_images) => {
                let mut command_queue = CommandQueue::default();

                command_queue.push(move |world: &mut World| {
                    world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                        world.resource_scope(|_world, mut foreground_materials: Mut<Assets<ForegroundMaterial>>| {
                            outputs.into_iter().zip(mask_images).for_each(|((mask, material), mask_image)| {
                                images.insert(mask, mask_image);
                                foreground_materials.get_mut(&material).unwrap();
                            });
                        });
                    });
                });

                command_queue
            },
            Err(error) => {
                error!("inference failed: {}", error);
                CommandQueue::default()
            }
        }
    });

    *pipeline_local = ModnetComputePipeline(Some(task));
}

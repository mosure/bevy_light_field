# bevy_light_field 💡🌾📷
[![test](https://github.com/mosure/bevy_light_field/workflows/test/badge.svg)](https://github.com/Mosure/bevy_light_field/actions?query=workflow%3Atest)
[![GitHub License](https://img.shields.io/github/license/mosure/bevy_light_field)](https://raw.githubusercontent.com/mosure/bevy_light_field/main/LICENSE)
[![crates.io](https://img.shields.io/crates/v/bevy_light_field.svg)](https://crates.io/crates/bevy_light_field)

rust bevy light field camera array tooling


## capabilities

- [X] grid view of light field camera array
- [X] stream to files with recording controls
- [X] person segmentation post-process (batch across streams)
- [X] async segmentation model inference
- [X] foreground extraction post-process and visualization mode
- [ ] recording session timeline viewer
- [ ] camera array calibration (extrinsics, intrinsics, color)
- [ ] camera position visualization
- [ ] 3d reconstruction dataset preparation
- [ ] real-time 3d reconstruction viewer


## run the viewer

`cargo run -- --help`

the viewer opens a window and displays the light field camera array, with post-process options

> see execution provider [bevy_ort documentation](https://github.com/mosure/bevy_ort?tab=readme-ov-file#run-the-example-person-segmentation-model-modnet) for better performance

- windows: `cargo run --release --features "ort/cuda"`


### controls

- `r` to start recording
- `s` to stop recording
- `esc` to exit
- [ ] UI controls


## library usage

```rust
use bevy::{
    prelude::*,
    render::render_resource::{
        Extent3d,
        TextureDescriptor,
        TextureDimension,
        TextureFormat,
        TextureUsages,
    },
};

use bevy_light_field::stream::{
    RtspStreamDescriptor,
    RtspStreamPlugin,
    StreamId,
};


const RTSP_URIS: [&str; 1] = [
    "rtsp://localhost:554/lizard",
];


fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            RtspStreamPlugin,
        ))
        .add_systems(Startup, create_streams)
        .run();
}


fn create_streams(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    RTSP_URIS.iter()
        .enumerate()
        .for_each(|(index, &url)| {
            let entity = commands.spawn_empty().id();

            let size = Extent3d {
                width: 640,
                height: 360,
                ..default()
            };

            let mut image = Image {
                texture_descriptor: TextureDescriptor {
                    label: Some(url),
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
            commands.spawn(SpriteBundle {
                sprite: Sprite {
                    custom_size: Some(Vec2::new(640.0, 360.0)),
                    ..default()
                },
                texture: image.clone(),
                ..default()
            });

            let rtsp_stream = RtspStreamDescriptor::new(
                url.to_string(),
                StreamId(index),
                image,
            );

            commands.entity(entity).insert(rtsp_stream);
        });
}
```


## light field camera array

view the [onshape model](https://cad.onshape.com/documents/20d4b522e97cda88fb785536/w/9939c2cecd85477ae7e753f6/e/69f97c604cdee8494e4e46bc?renderMode=0&uiState=65ea51d493f7bd0c772084fa)

- [ ] parts list

![Alt text](docs/light_field_camera_onshape_transparent.webp)


## setup rtsp streaming server

it is useful to test the light field viewer with emulated camera streams

### obs studio

- install https://obsproject.com/
- install rtsp plugin https://github.com/iamscottxu/obs-rtspserver/releases
- tools > rtsp server > start server


## compatible bevy versions

| `bevy_light_field`    | `bevy` |
| :--                   | :--    |
| `0.1.0`               | `0.13` |


## credits
- [bevy_video](https://github.com/PortalCloudInc/bevy_video)
- [gaussian_avatars](https://github.com/ShenhanQian/GaussianAvatars)
- [modnet](https://github.com/ZHKKKe/MODNet)
- [nersemble](https://github.com/tobias-kirschstein/nersemble)
- [paddle_seg_matting](https://github.com/PaddlePaddle/PaddleSeg/blob/release/2.9/Matting/docs/quick_start_en.md)
- [ray diffusion](https://github.com/jasonyzhang/RayDiffusion)

# bevy_light_field 💡🌾📷
[![test](https://github.com/mosure/bevy_light_field/workflows/test/badge.svg)](https://github.com/Mosure/bevy_light_field/actions?query=workflow%3Atest)
[![GitHub License](https://img.shields.io/github/license/mosure/bevy_light_field)](https://raw.githubusercontent.com/mosure/bevy_light_field/main/LICENSE)
[![GitHub Last Commit](https://img.shields.io/github/last-commit/mosure/bevy_light_field)](https://github.com/mosure/bevy_light_field)
[![GitHub Releases](https://img.shields.io/github/v/release/mosure/bevy_light_field?include_prereleases&sort=semver)](https://github.com/mosure/bevy_light_field/releases)
[![GitHub Issues](https://img.shields.io/github/issues/mosure/bevy_light_field)](https://github.com/mosure/bevy_light_field/issues)
[![Average time to resolve an issue](https://isitmaintained.com/badge/resolution/mosure/bevy_light_field.svg)](http://isitmaintained.com/project/mosure/bevy_light_field)
[![crates.io](https://img.shields.io/crates/v/bevy_light_field.svg)](https://crates.io/crates/bevy_light_field)

rust bevy light field camera array tooling


## example

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
        .add_systems(Startup, setup_camera)
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

fn setup_camera(
    mut commands: Commands,
) {
    commands.spawn((
        Camera2dBundle {
            ..default()
        },
    ));
}
```


## run the viewer

`cargo run -- --help`

the viewer opens a window and displays the light field camera array, with post-process options


## capabilities

- [ ] grid view of light field camera array
- [ ] person segmentation post-process (batch across streams)
- [ ] camera array calibration
- [ ] 3d reconstruction dataset preparation
- [ ] real-time 3d reconstruction viewer


## light field camera array

view the [onshape model](https://cad.onshape.com/documents/20d4b522e97cda88fb785536/w/9939c2cecd85477ae7e753f6/e/69f97c604cdee8494e4e46bc?renderMode=0&uiState=65ea51d493f7bd0c772084fa)

![Alt text](docs/light_field_camera_onshape_transparent.webp)

- [ ] parts list


## setup rtsp streaming server

it is useful to test the light field viewer with emulated camera streams

### obs studio

- install https://obsproject.com/
- install rtsp plugin https://github.com/iamscottxu/obs-rtspserver/releases
- Tools > RTSP Server > Start Server


## compatible bevy versions

| `bevy_light_field`    | `bevy` |
| :--                   | :--    |
| `0.1.0`               | `0.13` |


## credits
- [bevy_video](https://github.com/PortalCloudInc/bevy_video)

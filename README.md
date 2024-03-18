# bevy_light_field 💡🌾📷
[![test](https://github.com/mosure/bevy_light_field/workflows/test/badge.svg)](https://github.com/Mosure/bevy_light_field/actions?query=workflow%3Atest)
[![GitHub License](https://img.shields.io/badge/license-MIT%2FGPL%E2%80%933.0-blue.svg)](https://github.com/mosure/bevy_ort#license)
[![crates.io](https://img.shields.io/crates/v/bevy_light_field.svg)](https://crates.io/crates/bevy_light_field)

rust bevy light field camera array tooling


## capabilities

- [X] grid view of light field camera array
- [X] stream to files with recording controls
- [X] person segmentation post-process (batch across streams)
- [X] async segmentation model inference
- [X] foreground extraction post-process and visualization mode
- [X] recording session viewer
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
use bevy::prelude::*;

use bevy_light_field::{
    LightFieldPlugin,
    stream::RtspStreamHandle,
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            LightFieldPlugin {
                stream_config: "assets/streams.json",
            },
        ))
        .add_systems(Startup, setup_ui_gridview)
        .run();
}

fn setup_ui_gridview(
    mut commands: Commands,
    input_streams: Query<(
        Entity,
        &RtspStreamHandle,
    )>,
) {
    let stream = input_streams.single().unwrap();

    commands.spawn(ImageBundle {
        style: Style {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        image: UiImage::new(stream.image.clone()),
        ..default()
    });

    commands.spawn((
        Camera2dBundle {
            ..default()
        },
    ));
}
```


## light field camera array

view the [onshape model](https://cad.onshape.com/documents/20d4b522e97cda88fb785536/w/9939c2cecd85477ae7e753f6/e/69f97c604cdee8494e4e46bc?renderMode=0&uiState=65ea51d493f7bd0c772084fa)

- [ ] parts list

![Alt text](docs/light_field_camera_onshape_transparent.webp)


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
- [pose diffusion](https://github.com/facebookresearch/PoseDiffusion)
- [ray diffusion](https://github.com/jasonyzhang/RayDiffusion)


## license

This software is dual-licensed under the MIT License and the GNU General Public License version 3 (GPL-3.0).

You may choose to use this software under the terms of the MIT License OR the GNU General Public License version 3 (GPL-3.0), except as stipulated below:

The use of the `yolo_v8` feature within this software is specifically governed by the GNU General Public License version 3 (GPL-3.0). By using the `yolo_v8` feature, you agree to comply with the terms and conditions of the GPL-3.0.

For more details on the licenses, please refer to the LICENSE.MIT and LICENSE.GPL-3.0 files included with this software.

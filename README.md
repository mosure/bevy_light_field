# bevy_light_field 💡🌾📷
[![test](https://github.com/mosure/bevy_light_field/workflows/test/badge.svg)](https://github.com/Mosure/bevy_light_field/actions?query=workflow%3Atest)
[![GitHub License](https://img.shields.io/github/license/mosure/bevy_light_field)](https://raw.githubusercontent.com/mosure/bevy_light_field/main/LICENSE)
[![GitHub Last Commit](https://img.shields.io/github/last-commit/mosure/bevy_light_field)](https://github.com/mosure/bevy_light_field)
[![GitHub Releases](https://img.shields.io/github/v/release/mosure/bevy_light_field?include_prereleases&sort=semver)](https://github.com/mosure/bevy_light_field/releases)
[![GitHub Issues](https://img.shields.io/github/issues/mosure/bevy_light_field)](https://github.com/mosure/bevy_light_field/issues)
[![Average time to resolve an issue](https://isitmaintained.com/badge/resolution/mosure/bevy_light_field.svg)](http://isitmaintained.com/project/mosure/bevy_light_field)
[![crates.io](https://img.shields.io/crates/v/bevy_light_field.svg)](https://crates.io/crates/bevy_light_field)

rust bevy light field camera array tooling


## run the viewer

`cargo run -- --help`

the viewer opens a window and displays the light field camera array, with post-process options


## capabilities

- [ ] grid view of light field camera array
- [ ] person segmentation post-process (batch across streams)
- [ ] camera array calibration
- [ ] 3d reconstruction dataset preparation
- [ ] real-time 3d reconstruction viewer


## setup rtsp streaming server

### obs studio

- install https://obsproject.com/
- install rtsp plugin https://github.com/iamscottxu/obs-rtspserver/releases
- Tools > RTSP Server > Start Server


## compatible bevy versions

| `bevy_light_field`    | `bevy` |
| :--                   | :--    |
| `0.1.0`               | `0.13` |

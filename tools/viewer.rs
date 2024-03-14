use bevy::{
    prelude::*,
    app::AppExit,
    diagnostic::{
        DiagnosticsStore,
        FrameTimeDiagnosticsPlugin,
    },
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{
            Extent3d,
            TextureDescriptor,
            TextureDimension,
            TextureFormat,
            TextureUsages,
        },
    },
    time::Stopwatch,
    window::PrimaryWindow,
};
use bevy_args::{
    parse_args,
    BevyArgsPlugin,
    Deserialize,
    Parser,
    Serialize,
};

use bevy_light_field::{
    LightFieldPlugin,
    materials::foreground::ForegroundMaterial,
    person_detect::{
        DetectPersons,
        PersonDetectedEvent,
    },
    pipeline::{
        PipelineConfig,
        StreamSessionBundle,
        Session,
        RawStreams,
    },
    stream::{
        RtspStreamHandle,
        RtspStreamManager,
        StreamId,
        StreamUris,
    },
};

#[cfg(feature = "person_matting")]
use bevy_light_field::matting::{
    MattedStream,
    MattingPlugin,
};


#[derive(
    Default,
    Debug,
    Resource,
    Serialize,
    Deserialize,
    Parser,
)]
#[command(about = "bevy_light_field viewer", version)]
pub struct LightFieldViewer {
    #[arg(long, default_value = "assets/streams.json")]
    pub config: String,

    #[arg(long, default_value = "false")]
    pub show_fps: bool,

    #[arg(long, default_value = "false")]
    pub automatic_recording: bool,

    #[arg(long, default_value = "false")]
    pub fullscreen: bool,

    #[arg(long, default_value = "1920.0")]
    pub width: f32,
    #[arg(long, default_value = "1080.0")]
    pub height: f32,

    #[arg(long, default_value = "1024")]
    pub max_matting_width: u32,
    #[arg(long, default_value = "1024")]
    pub max_matting_height: u32,

    #[arg(long, default_value = "false")]
    pub extract_foreground: bool,
}



fn main() {
    let args = parse_args::<LightFieldViewer>();

    let mode = if args.fullscreen {
        bevy::window::WindowMode::BorderlessFullscreen
    } else {
        bevy::window::WindowMode::Windowed
    };

    let primary_window = Some(Window {
        mode,
        prevent_default_event_handling: false,
        resolution: (args.width, args.height).into(),
        title: "bevy_light_field - rtsp viewer".to_string(),
        present_mode: bevy::window::PresentMode::AutoVsync,
        ..default()
    });

    let mut app = App::new();
    app
        .add_plugins(BevyArgsPlugin::<LightFieldViewer>::default())
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window,
                    ..default()
                }),
            LightFieldPlugin {
                stream_config: args.config.clone(),
            },

            #[cfg(feature = "person_matting")]
            MattingPlugin::new((
                args.max_matting_width,
                args.max_matting_height,
            )),
        ))
        .init_resource::<LiveSession>()
        .add_systems(Startup, create_streams)
        .add_systems(Startup, setup_camera)
        .add_systems(
            Update,
            (
                press_esc_close,
                automatic_recording,
                press_r_start_recording,
                press_s_stop_recording
            )
        );

    if args.show_fps {
        app.add_plugins(FrameTimeDiagnosticsPlugin);
        app.add_systems(Startup, fps_display_setup.after(create_streams));
        app.add_systems(Update, fps_update_system);
    }

    app.run();
}


fn create_streams(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    args: Res<LightFieldViewer>,
    mut foreground_materials: ResMut<Assets<ForegroundMaterial>>,
    stream_uris: Res<StreamUris>,
) {
    let window = primary_window.single();
    let elements = stream_uris.0.len();

    let (
        columns,
        rows,
        _sprite_width,
        _sprite_height,
    ) = calculate_grid_dimensions(
        window.width(),
        window.height(),
        elements,
    );

    let size = Extent3d {
        width: 32,
        height: 32,
        ..default()
    };

    // TODO: support enabling/disabling decoding/matting per stream (e.g. during 'record mode')

    let input_images: Vec<Handle<Image>> = stream_uris.0.iter()
        .enumerate()
        .map(|(index, descriptor)| {
            let entity = commands.spawn_empty().id();

            let mut image = Image {
                asset_usage: RenderAssetUsages::all(),
                texture_descriptor: TextureDescriptor {
                    label: None,
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
            let image_clone = image.clone();

            let rtsp_stream = RtspStreamHandle::new(
                descriptor.clone(),
                StreamId(index),
                image,
            );

            commands.entity(entity).insert(rtsp_stream);

            image_clone
        })
        .collect();

    let mask_images = input_images.iter()
        .enumerate()
        .map(|(index, image)| {
            let mut mask_image = Image {
                asset_usage: RenderAssetUsages::all(),
                texture_descriptor: TextureDescriptor {
                    label: None,
                    size,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::R8Unorm,
                    mip_level_count: 1,
                    sample_count: 1,
                    usage: TextureUsages::COPY_DST
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[TextureFormat::R8Unorm],
                },
                ..default()
            };
            mask_image.resize(size);
            let mask_image = images.add(mask_image);

            let mut material = None;

            #[cfg(feature = "person_matting")]
            if args.extract_foreground || (args.automatic_recording && index == 0) {
                let foreground_mat = foreground_materials.add(ForegroundMaterial {
                    input: image.clone(),
                    mask: mask_image.clone(),
                });

                let mut entity = commands.spawn(MattedStream {
                    stream_id: StreamId(index),
                    input: image.clone(),
                    output: mask_image.clone(),
                    material: foreground_mat.clone(),
                });

                if args.automatic_recording && index == 0 {
                    entity.insert(DetectPersons);
                }

                material = foreground_mat.into();
            }

            (mask_image, material)
        })
        .collect::<Vec<_>>();

    commands.spawn(NodeBundle {
        style: Style {
            display: Display::Grid,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            grid_template_columns: RepeatedGridTrack::flex(columns as u16, 1.0),
            grid_template_rows: RepeatedGridTrack::flex(rows as u16, 1.0),
            ..default()
        },
        background_color: BackgroundColor(Color::BLACK),
        ..default()
    })
    .with_children(|builder| {
        input_images.iter()
            .zip(mask_images.iter())
            .enumerate()
            .for_each(|(index, (input, (_mask, material)))| {
                if args.extract_foreground || (args.automatic_recording && index == 0) {
                    builder.spawn(MaterialNodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        material: material.clone().unwrap(),
                        ..default()
                    });
                } else {
                    builder.spawn(ImageBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        image: UiImage::new(input.clone()),
                        ..default()
                    });
                }
            });
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


fn press_esc_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: EventWriter<AppExit>
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.send(AppExit);
    }
}


fn automatic_recording(
    mut commands: Commands,
    time: Res<Time>,
    mut ev_person: EventReader<PersonDetectedEvent>,
    stream_manager: Res<RtspStreamManager>,
    mut live_session: ResMut<LiveSession>,
    mut person_timeout: Local<Stopwatch>,
) {
    if live_session.0.is_some() {
        if person_timeout.elapsed_secs() > 3.0 {
            person_timeout.reset();

            println!("no person detected for 3 seconds, stopping recording");

            let session_entity = live_session.0.take().unwrap();
            let raw_streams = stream_manager.stop_recording();

            commands.entity(session_entity)
                .insert(RawStreams {
                    streams: raw_streams,
                });
        }

        person_timeout.tick(time.delta());

        for _ev in ev_person.read() {
            person_timeout.reset();
        }

        return;
    }

    for ev in ev_person.read() {
        println!("person detected: {:?}", ev);

        // TODO: deduplicate start recording logic
        let session = Session::new("capture".to_string());

        stream_manager.start_recording(
            &session,
        );

        // TODO: build pipeline config from args
        let entity = commands.spawn(
            StreamSessionBundle {
                session: session,
                raw_streams: RawStreams::default(),
                config: PipelineConfig::default(),
            },
        ).id();
        live_session.0 = Some(entity);

        break;
    }
}


#[derive(Resource, Default)]
pub struct LiveSession(Option<Entity>);

fn press_r_start_recording(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    stream_manager: Res<RtspStreamManager>,
    mut live_session: ResMut<LiveSession>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        if live_session.0.is_some() {
            return;
        }

        let session = Session::new("capture".to_string());

        stream_manager.start_recording(
            &session,
        );

        let entity = commands.spawn(
            StreamSessionBundle {
                session: session,
                raw_streams: RawStreams::default(),
                config: PipelineConfig::default(),
            },
        ).id();
        live_session.0 = Some(entity);
    }
}

fn press_s_stop_recording(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    stream_manager: Res<RtspStreamManager>,
    mut live_session: ResMut<LiveSession>,
) {
    if keys.just_pressed(KeyCode::KeyS) && live_session.0.is_some() {
        let session_entity = live_session.0.take().unwrap();

        let raw_streams = stream_manager.stop_recording();

        commands.entity(session_entity)
            .insert(RawStreams {
                streams: raw_streams,
            });
    }
}


fn calculate_grid_dimensions(window_width: f32, window_height: f32, num_streams: usize) -> (usize, usize, f32, f32) {
    let window_aspect_ratio = window_width / window_height;
    let stream_aspect_ratio: f32 = 16.0 / 9.0;
    let mut best_layout = (1, num_streams);
    let mut best_diff = f32::INFINITY;
    let mut best_sprite_size = (0.0, 0.0);

    for columns in 1..=num_streams {
        let rows = (num_streams as f32 / columns as f32).ceil() as usize;
        let sprite_width = window_width / columns as f32;
        let sprite_height = sprite_width / stream_aspect_ratio;
        let total_height_needed = sprite_height * rows as f32;
        let (final_sprite_width, final_sprite_height) = if total_height_needed > window_height {
            let adjusted_sprite_height = window_height / rows as f32;
            let adjusted_sprite_width = adjusted_sprite_height * stream_aspect_ratio;
            (adjusted_sprite_width, adjusted_sprite_height)
        } else {
            (sprite_width, sprite_height)
        };
        let grid_aspect_ratio = final_sprite_width * columns as f32 / (final_sprite_height * rows as f32);
        let diff = (window_aspect_ratio - grid_aspect_ratio).abs();

        if diff < best_diff {
            best_diff = diff;
            best_layout = (columns, rows);
            best_sprite_size = (final_sprite_width, final_sprite_height);
        }
    }

    (best_layout.0, best_layout.1, best_sprite_size.0, best_sprite_size.1)
}





fn fps_display_setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.spawn((
        TextBundle::from_sections([
            TextSection::new(
                "fps: ",
                TextStyle {
                    font: asset_server.load("fonts/Caveat-Bold.ttf"),
                    font_size: 60.0,
                    color: Color::WHITE,
                },
            ),
            TextSection::from_style(TextStyle {
                font: asset_server.load("fonts/Caveat-Medium.ttf"),
                font_size: 60.0,
                color: Color::GOLD,
            }),
        ]).with_style(Style {
            position_type: PositionType::Absolute,
            width: Val::Px(200.0),
            bottom: Val::Px(5.0),
            right: Val::Px(15.0),
            ..default()
        }),
        FpsText,
    ));
}

#[derive(Component)]
struct FpsText;

fn fps_update_system(
    diagnostics: Res<DiagnosticsStore>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    for mut text in &mut query {
        if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(value) = fps.smoothed() {
                text.sections[1].value = format!("{:.2}", value);
            }
        }
    }
}

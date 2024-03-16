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

    #[arg(long)]
    pub session_id: Option<usize>,
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
        .add_systems(
            Startup,
            (
                #[cfg(feature = "person_matting")]
                create_mask_streams,
                setup_camera,
                select_session_from_args,
            )
        )
        .add_systems(
            PostStartup,
            (
                setup_ui_gridview,
            )
        )
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
        app.add_systems(PostStartup, fps_display_setup.after(setup_ui_gridview));
        app.add_systems(Update, fps_update_system);
    }

    app.run();
}


// TODO: move to MattingPlugin
#[cfg(feature = "person_matting")]
fn create_mask_streams(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut foreground_materials: ResMut<Assets<ForegroundMaterial>>,
    args: Res<LightFieldViewer>,
    input_streams: Query<
        (
            Entity,
            &RtspStreamHandle,
        ),
        Without<MattedStream>
    >,
) {
    let size = Extent3d {
        width: 32,
        height: 32,
        ..default()
    };

    input_streams.iter()
        .for_each(|(entity, stream)| {
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

            if args.automatic_recording && stream.descriptor.person_detection.unwrap_or_default() {
                let foreground_mat = foreground_materials.add(ForegroundMaterial {
                    input: stream.image.clone(),
                    mask: mask_image.clone(),
                });

                commands.entity(entity)
                    .insert(MattedStream {
                        stream_id: stream.id,
                        input: stream.image.clone(),
                        output: mask_image.clone(),
                        material: foreground_mat,
                    })
                    .insert(DetectPersons);
            }
        });
}


fn setup_ui_gridview(
    mut commands: Commands,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    input_streams: Query<(
        Entity,
        &RtspStreamHandle,
    )>,
    person_detection_stream: Query<
        (
            Entity,
            &MattedStream,
        ),
        With<DetectPersons>,
    >,
) {
    let window = primary_window.single();

    let visible_input_streams = input_streams.iter()
        .filter(|(_, stream)| stream.descriptor.visible.unwrap_or_default())
        .collect::<Vec<_>>();

    let visible_streams = visible_input_streams.len() + person_detection_stream.iter().count();

    let (
        columns,
        rows,
        _sprite_width,
        _sprite_height,
    ) = calculate_grid_dimensions(
        window.width(),
        window.height(),
        visible_streams,
    );

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
        visible_input_streams
            .iter()
            .for_each(|(_, input_stream)| {
                builder.spawn(ImageBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    image: UiImage::new(input_stream.image.clone()),
                    ..default()
                });
            });

        person_detection_stream
            .iter()
            .for_each(|(_, matted_stream)| {
                builder.spawn(MaterialNodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    material: matted_stream.material.clone(),
                    ..default()
                });
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


fn select_session_from_args(
    mut commands: Commands,
    args: Res<LightFieldViewer>,
) {
    if args.session_id.is_none() {
        return;
    }

    let session = Session::from_id(
        args.session_id.unwrap(),
        "capture".to_string(),
    );
    let raw_streams = RawStreams::load_from_session(&session);

    commands.spawn(
        StreamSessionBundle {
            session,
            raw_streams,
            config: PipelineConfig::default(),
        },
    );
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

            info!("no person detected for 3 seconds, stop recording");

            let _session_entity = live_session.0.take().unwrap();
            let _raw_streams = stream_manager.stop_recording();

            // TODO: TODO: add a recording finished event when all streams are closed, then execute the following command if pipeline auto-processing is enabled (not ideal for fast recording)

            // commands.entity(session_entity)
            //     .insert(RawStreams {
            //         streams: raw_streams,
            //     })
            //     .insert(PipelineConfig::default());
        }

        person_timeout.tick(time.delta());

        for _ev in ev_person.read() {
            person_timeout.reset();
        }

        return;
    }

    let person_detected = !ev_person.is_empty();
    ev_person.clear();

    if person_detected {
        // TODO: deduplicate start recording logic
        let session = Session::new("capture".to_string());

        stream_manager.start_recording(
            &session,
        );

        // TODO: build pipeline config from args
        let entity = commands.spawn(session).id();
        live_session.0 = Some(entity);
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
                session,
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


// TODO: add pipeline viewer /w left/right arrow keys and UI controls to switch between frames



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

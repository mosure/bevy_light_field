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
};
use bevy_args::{
    parse_args,
    BevyArgsPlugin,
    Deserialize,
    Parser,
    Serialize,
};
use clap::ValueEnum;

use bevy_light_field::{
    grid_view::{
        Element,
        GridView
    },
    materials::foreground::ForegroundMaterial,
    matting::{
        MattedStream,
        MattingPlugin,
    },
    person_detect::{
        DetectPersons,
        PersonDetectedEvent,
    },
    pipeline::{
        load_png,
        AlphablendFrames,
        MaskFrames,
        PipelineConfig,
        RawFrames,
        RawStreams,
        RotatedFrames,
        Session,
        StreamSessionBundle,
    },
    stream::{
        RtspStreamHandle,
        RtspStreamManager,
    },
    LightFieldPlugin,
};


#[derive(
    Debug,
    Default,
    Clone,
    Serialize,
    Deserialize,
    ValueEnum,
)]
pub enum OfflineAnnotation {
    Raw,
    Rotated,
    Mask,
    #[default]
    Alphablend,
    Yolo,
}


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

    #[arg(long, default_value = "true")]
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
    #[arg(long)]
    pub annotation: Option<OfflineAnnotation>,
    #[arg(long)]
    pub frame: Option<usize>,
}



fn main() {
    let args = parse_args::<LightFieldViewer>();

    let online = args.session_id.is_none();

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
            MattingPlugin::new((
                args.max_matting_width,
                args.max_matting_height,
            )),
        ))
        .add_systems(Startup, setup_camera)
        .add_systems(Update, press_esc_close);

    if online {
        app
            .init_resource::<LiveSession>()
            .add_systems(
                Startup,
                (
                    create_mask_streams,
                ),
            )
            .add_systems(
                PostStartup,
                (
                    setup_live_gridview,
                ),
            )
            .add_systems(
                Update,
                (
                    automatic_recording,
                    press_r_start_recording,
                    press_s_stop_recording
                ),
            );
    } else {
        app
            .insert_resource(FrameIndex(args.frame.unwrap_or_default()))
            .add_systems(
                Startup,
                (
                    select_session_from_args,
                ),
            )
            .add_systems(
                Update,
                (
                    offline_viewer,
                    press_arrow_key_frame_navigation,
                ),
            );
    }

    if args.show_fps {
        app.add_plugins(FrameTimeDiagnosticsPlugin);
        app.add_systems(PostStartup, fps_display_setup.after(setup_live_gridview));
        app.add_systems(Update, fps_update_system);
    }

    app.run();
}


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


fn setup_live_gridview(
    mut grid_view: ResMut<GridView>,
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
    let visible_input_streams = input_streams.iter()
        .filter(|(_, stream)| stream.descriptor.visible.unwrap_or_default())
        .collect::<Vec<_>>();

    let grid_elements = visible_input_streams.iter()
        .map(|(_, input_stream)| Element::Image(input_stream.image.clone()))
        .chain(
            person_detection_stream.iter()
                .map(|(_, matted_stream) | Element::Alphablend(matted_stream.material.clone()))
        )
        .collect::<Vec<_>>();

    grid_view.source = grid_elements;
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


#[derive(Resource, Default)]
struct FrameIndex(usize);

fn offline_viewer(
    asset_server: Res<AssetServer>,
    mut grid_view: ResMut<GridView>,
    frame_index: Res<FrameIndex>,
    args: Res<LightFieldViewer>,
    session: Query<
        (
            Entity,
            &PipelineConfig,
            &RawFrames,
            &RotatedFrames,
            &MaskFrames,
            &AlphablendFrames,
            &Session,
        ),
    >,
    mut complete: Local<bool>,
) {
    if session.is_empty() {
        return;
    }

    if !frame_index.is_changed() && *complete {
        return;
    }

    let session = session.iter().next().unwrap();

    let mut frames = match args.annotation.clone().unwrap_or_default() {
        OfflineAnnotation::Raw => &session.2.frames,
        OfflineAnnotation::Rotated => &session.3.frames,
        OfflineAnnotation::Mask => &session.4.frames,
        OfflineAnnotation::Alphablend => &session.5.frames,
        OfflineAnnotation::Yolo => unimplemented!(),
    }.iter()
        .map(|(stream_id, frames)| {
            let mut sorted_frames = frames.clone();
            sorted_frames.sort_by(|a, b| {
                let stem_a = std::path::Path::new(a).file_stem().unwrap().to_str().unwrap();
                let stem_b = std::path::Path::new(b).file_stem().unwrap().to_str().unwrap();

                let a_idx = stem_a.parse::<usize>().unwrap();
                let b_idx = stem_b.parse::<usize>().unwrap();

                a_idx.cmp(&b_idx)
            });

            (stream_id, sorted_frames[frame_index.0].clone())
        })
        .collect::<Vec<_>>();

    frames.sort_by(|a, b| a.0.0.partial_cmp(&b.0.0).unwrap());

    let frames = frames.iter()
        .map(|(_stream_id, frame)| {
            let image = load_png(std::path::Path::new(frame));

            Element::Image(asset_server.add(image))
        })
        .collect::<Vec<_>>();

    grid_view.source = frames;

    *complete = true;
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


fn press_arrow_key_frame_navigation(
    mut frame_index: ResMut<FrameIndex>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if keys.just_pressed(KeyCode::ArrowLeft) {
        frame_index.0 = frame_index.0.saturating_sub(1);
    }

    if keys.just_pressed(KeyCode::ArrowRight) {
        frame_index.0 += 1;
    }
}



fn fps_display_setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let mut bundle = TextBundle::from_sections([
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
    });
    bundle.z_index = ZIndex::Global(10);

    commands.spawn((
        bundle,
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

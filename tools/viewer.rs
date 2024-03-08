use bevy::{
    prelude::*,
    app::AppExit,
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
    window::PrimaryWindow,
};

use bevy_light_field::stream::{
    RtspStreamDescriptor, RtspStreamManager, RtspStreamPlugin, StreamId
};


const RTSP_URIS: [&str; 2] = [
    "rtsp://192.168.1.23/user=admin&password=admin123&channel=1&stream=0.sdp?",
    "rtsp://192.168.1.24/user=admin&password=admin123&channel=1&stream=0.sdp?",
];


// TODO: add bevy_args
fn main() {
    let primary_window = Some(Window {
        mode: bevy::window::WindowMode::Windowed,
        prevent_default_event_handling: false,
        resolution: (1920.0, 1080.0).into(),
        title: "bevy_light_field - rtsp viewer".to_string(),
        present_mode: bevy::window::PresentMode::AutoVsync,
        ..default()
    });

    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window,
                    ..default()
                }),
            RtspStreamPlugin,
        ))
        .add_systems(Startup, create_streams)
        .add_systems(Startup, setup_camera)
        .add_systems(
            Update,
            (
                press_esc_close,
                press_r_start_recording,
                press_s_stop_recording
            )
        )
        .run();
}


fn create_streams(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let window = primary_window.single();
    let (
        columns,
        rows,
        sprite_width,
        sprite_height,
    ) = calculate_grid_dimensions(
        window.width(),
        window.height(),
        RTSP_URIS.len()
    );

    let images: Vec<Handle<Image>> = RTSP_URIS.iter()
        .enumerate()
        .map(|(index, &url)| {
            let entity = commands.spawn_empty().id();

            let size = Extent3d {
                width: 32,
                height: 32,
                ..default()
            };

            let mut image = Image {
                asset_usage: RenderAssetUsages::all(),
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
            let image_clone = image.clone();

            let rtsp_stream = RtspStreamDescriptor::new(
                url.to_string(),
                StreamId(index),
                image,
            );

            commands.entity(entity).insert(rtsp_stream);

            image_clone
        })
        .collect();

    commands.spawn(NodeBundle {
        style: Style {
            display: Display::Grid,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            grid_template_columns: RepeatedGridTrack::flex(columns as u16, 1.0),
            grid_template_rows: RepeatedGridTrack::flex(rows as u16, 1.0),
            ..default()
        },
        background_color: BackgroundColor(Color::DARK_GRAY),
        ..default()
    })
    .with_children(|builder| {
        images.iter()
            .for_each(|image| {
                builder.spawn(ImageBundle {
                    style: Style {
                        width: Val::Px(sprite_width),
                        height: Val::Px(sprite_height),
                        ..default()
                    },
                    image: UiImage::new(image.clone()),
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

fn press_r_start_recording(
    keys: Res<ButtonInput<KeyCode>>,
    stream_manager: Res<RtspStreamManager>
) {
    if keys.just_pressed(KeyCode::KeyR) {
        let output_directory = "capture";
        std::fs::create_dir_all(output_directory).unwrap();

        let base_prefix = "bevy_light_field_";

        let prefix = format!(
            "{}{:03}",
            base_prefix,
            get_next_session_id(output_directory, base_prefix)
        );

        stream_manager.start_recording(
            output_directory,
            &prefix,
        );
    }
}

fn press_s_stop_recording(
    keys: Res<ButtonInput<KeyCode>>,
    stream_manager: Res<RtspStreamManager>
) {
    if keys.just_pressed(KeyCode::KeyS) {
        stream_manager.stop_recording();
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


fn get_next_session_id(output_directory: &str, base_prefix: &str) -> i32 {
    let mut highest_count = -1i32;
    if let Ok(entries) = std::fs::read_dir(output_directory) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.starts_with(base_prefix) {
                    let suffix = stem.trim_start_matches(base_prefix);
                    let numeric_part = suffix.split('_').next().unwrap_or("");
                    if let Ok(num) = numeric_part.parse::<i32>() {
                        highest_count = highest_count.max(num);
                    } else {
                        println!("failed to parse session ID '{}' for file '{}'", numeric_part, stem);
                    }
                }
            }
        }
    }

    highest_count + 1
}

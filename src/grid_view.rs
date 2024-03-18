use bevy::{
    prelude::*,
    window::PrimaryWindow,
};

use crate::materials::foreground::ForegroundMaterial;


pub struct GridViewPlugin;
impl Plugin for GridViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GridView>();
        app.add_systems(Update, draw_grid_view);
    }
}

#[derive(Debug, Clone)]
pub enum Element {
    Image(Handle<Image>),
    Alphablend(Handle<ForegroundMaterial>),
}

#[derive(Resource, Default)]
pub struct GridView {
    pub source: Vec<Element>,
}


#[derive(Component, Default)]
pub struct GridViewParent;


fn draw_grid_view(
    mut commands: Commands,
    primary_window: Query<
        &Window,
        With<PrimaryWindow>
    >,
    grid_view: Res<GridView>,
    grid_view_parent: Query<
        Entity,
        With<GridViewParent>
    >,
) {
    if !grid_view.is_changed() {
        return;
    }

    for entity in grid_view_parent.iter() {
        commands.entity(entity).despawn_recursive();
    }

    let window = primary_window.single();

    let (
        columns,
        rows,
        _width,
        _height,
    ) = calculate_grid_dimensions(
        window.width(),
        window.height(),
        grid_view.source.len(),
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
    .insert(GridViewParent)
    .with_children(|builder| {
        grid_view.source.iter()
            .for_each(|element| {
                match element {
                    Element::Image(image) => {
                        builder.spawn(ImageBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            image: UiImage::new(image.clone()),
                            ..default()
                        });
                    }
                    Element::Alphablend(material) => {
                        builder.spawn(MaterialNodeBundle {
                            style: Style {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            material: material.clone(),
                            ..default()
                        });
                    }
                }
            });
    });
}


fn calculate_grid_dimensions(
    window_width: f32,
    window_height: f32,
    num_streams: usize,
) -> (usize, usize, f32, f32) {
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

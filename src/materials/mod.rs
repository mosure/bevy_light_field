use bevy::prelude::*;

pub mod foreground;


pub struct StreamMaterialsPlugin;
impl Plugin for StreamMaterialsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(foreground::ForegroundPlugin);
    }
}

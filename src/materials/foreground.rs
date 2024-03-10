use bevy::{
    prelude::*,
    asset::load_internal_asset,
    render::render_resource::*,
};


const FOREGROUND_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(5231534123);

pub struct ForegroundPlugin;
impl Plugin for ForegroundPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            FOREGROUND_SHADER_HANDLE,
            "foreground.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins(UiMaterialPlugin::<ForegroundMaterial>::default());
    }
}


#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct ForegroundMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub input: Handle<Image>,

    #[texture(2)]
    #[sampler(3)]
    pub mask: Handle<Image>,
}

impl UiMaterial for ForegroundMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(FOREGROUND_SHADER_HANDLE)
    }
}

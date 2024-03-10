#import bevy_ui::ui_vertex_output::UiVertexOutput


@group(1) @binding(0) var foreground_texture: texture_2d<f32>;
@group(1) @binding(1) var foreground_sampler: sampler;

@group(1) @binding(2) var mask_texture: texture_2d<f32>;
@group(1) @binding(3) var mask_sampler: sampler;


@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(
        foreground_texture,
        foreground_sampler,
        in.uv,
    ) * textureSample(
        mask_texture,
        mask_sampler,
        in.uv,
    ).x;
}

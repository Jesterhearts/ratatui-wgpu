struct VertexOutput {
    @location(0) UV: vec2<f32>,
    @location(1) @interpolate(flat) FgColor: u32,
    @builtin(position) gl_Position: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> ScreenSize: vec4<f32>;

@vertex
fn vs_main(
    @location(0) VertexCoord: vec2<f32>,
    @location(1) UV: vec2<f32>,
    @location(2) FgColor: u32,
) -> VertexOutput {
    let gl_Position = vec4<f32>((2.0 * VertexCoord / ScreenSize.xy - 1.0) * vec2(1.0, -1.0), 0.0, 1.0);
    return VertexOutput(UV, FgColor, gl_Position);
}

struct FragmentOutput {
    @location(0) FragColor: vec4<f32>,
}

@group(1) @binding(0) 
var Atlas: texture_2d<f32>;
@group(1) @binding(1) 
var Sampler: sampler;

@group(1) @binding(2) 
var<uniform> AtlasSize: vec4<f32>;

fn unpack_color(color: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(color >> 24) / 255.0,
        f32((color >> 16) & 0xFF) / 255.0,
        f32((color >> 8) & 0xFF) / 255.0,
        f32(color & 0xFF) / 255.0,
    );
}


@fragment
fn fs_main(@location(0) UV: vec2<f32>, @location(1) @interpolate(flat) FgColor: u32) -> FragmentOutput {
    let fgColorUnpacked = unpack_color(FgColor);
    let textureColor = textureSample(Atlas, Sampler, UV / AtlasSize.xy);
    return FragmentOutput(vec4<f32>(fgColorUnpacked.rgb, textureColor.r * fgColorUnpacked.a));
}
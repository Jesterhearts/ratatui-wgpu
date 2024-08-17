struct VertexOutput {
    @builtin(position) gl_Position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) Index: u32) -> VertexOutput {
    let vertex = vec2(f32((Index << 1) & 2), f32(Index & 2));
    return VertexOutput(vec4(vertex * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0));
}

struct FragmentOutput {
    @location(0) FragColor: vec4<f32>,
}

@group(0) @binding(0) 
var Texture: texture_2d<f32>;
@group(0) @binding(1) 
var Sampler: sampler;

struct Uniforms {
    use_srgb: u32,
    _pad0: vec3<u32>,
}

@group(0) @binding(2)
var<uniform> uniforms: Uniforms;

@fragment
fn fs_main(@builtin(position) gl_Position: vec4<f32>) -> FragmentOutput {
    let redOffset = 0.013;
    let greenOffset = 0.006;
    let blueOffset = -0.09;

    let uv = gl_Position.xy / vec2<f32>(textureDimensions(Texture));
    let factor = select(2.2, 1.0, uniforms.use_srgb == 0);

    let red = textureSample(Texture, Sampler, uv + redOffset).r;
    let green = textureSample(Texture, Sampler, uv + greenOffset).g;
    let blue = textureSample(Texture, Sampler, uv + blueOffset).b;

    return FragmentOutput(pow(vec4(red, green, blue, 1.0), vec4(factor)));
}
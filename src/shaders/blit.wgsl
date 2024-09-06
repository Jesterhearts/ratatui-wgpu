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
    screen_size: vec2<f32>,
    _pad0: vec2<f32>,
    use_srgb: u32,
}

@group(0) @binding(2)
var<uniform> uniforms: Uniforms;

@fragment
fn fs_main(@builtin(position) gl_Position: vec4<f32>) -> FragmentOutput {
    let uv = gl_Position.xy / uniforms.screen_size;
    let factor = select(2.2, 1.0, uniforms.use_srgb == 0);

    return FragmentOutput(pow(textureSample(Texture, Sampler, uv), vec4(vec3(factor), 1.0)));
}
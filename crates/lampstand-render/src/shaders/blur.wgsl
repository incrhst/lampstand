// One direction of a separable Gaussian blur, drawn as a fullscreen triangle.

struct Blur {
    // One texel along the blur direction, in texture coordinates.
    step: vec2<f32>,
    sigma: f32,
    radius: i32,
}

@group(0) @binding(0) var<uniform> blur: Blur;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOut;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    let k = -0.5 / (blur.sigma * blur.sigma);
    for (var i = -blur.radius; i <= blur.radius; i++) {
        let x = f32(i);
        let w = exp(x * x * k);
        sum += w * textureSampleLevel(tex, tex_sampler, in.uv + blur.step * x, 0.0);
        total += w;
    }
    return sum / total;
}

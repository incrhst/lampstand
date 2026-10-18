// Textured quads: images, composited layers, mip generation and, in T27,
// video frames. Textures hold premultiplied sRGB values.

struct Quad {
    // 0: texture times opacity. 1: tint times texture alpha (shadows).
    mode: u32,
    opacity: f32,
    _pad: vec2<f32>,
    // Premultiplied tint for mode 1.
    tint: vec4<f32>,
}

@group(0) @binding(0) var<uniform> quad: Quad;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, tex_sampler, in.uv);
    if quad.mode == 1u {
        return quad.tint * texel.a;
    }
    return texel * quad.opacity;
}

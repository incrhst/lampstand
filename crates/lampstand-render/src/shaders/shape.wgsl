// Filled and stroked paths. Vertices arrive in clip space with the node's
// local coordinates alongside, so gradients are evaluated in local space and
// follow the node's transform. Colours are premultiplied sRGB values.

struct Paint {
    // 0 solid, 1 linear, 2 radial.
    kind: u32,
    // Row of the gradient ramp texture, as a texture coordinate.
    ramp_v: f32,
    opacity: f32,
    _pad: f32,
    color: vec4<f32>,
    // Linear: start.xy, end.xy. Radial: center.xy, radius, unused.
    geom: vec4<f32>,
}

@group(0) @binding(0) var<uniform> paint: Paint;
@group(0) @binding(1) var ramp: texture_2d<f32>;
@group(0) @binding(2) var ramp_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
}

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) local: vec2<f32>) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.local = local;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var t = 0.0;
    if paint.kind == 1u {
        let d = paint.geom.zw - paint.geom.xy;
        t = dot(in.local - paint.geom.xy, d) / max(dot(d, d), 1e-12);
    } else if paint.kind == 2u {
        t = length(in.local - paint.geom.xy) / max(paint.geom.z, 1e-12);
    }
    // 256 texels per ramp; sample texel centres so t=0 and t=1 hit the end stops.
    let u = (clamp(t, 0.0, 1.0) * 255.0 + 0.5) / 256.0;
    let ramp_color = textureSampleLevel(ramp, ramp_sampler, vec2<f32>(u, paint.ramp_v), 0.0);
    let color = select(ramp_color, paint.color, paint.kind == 0u);
    return color * paint.opacity;
}

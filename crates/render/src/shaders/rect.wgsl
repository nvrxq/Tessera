struct Uniforms {
    screen_size: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct InstanceIn {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) corner_radius: f32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) radius: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: InstanceIn) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let c = corners[vi];
    let px = inst.pos + c * inst.size;
    let ndc = vec2(
        (px.x / u.screen_size.x) * 2.0 - 1.0,
        1.0 - (px.y / u.screen_size.y) * 2.0,
    );

    var out: VertexOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.color = inst.color;
    out.local = c * inst.size;
    out.size = inst.size;
    out.radius = inst.corner_radius;
    return out;
}

fn sdf_rounded_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    if (in.radius <= 0.0) {
        return in.color;
    }
    let center = in.size * 0.5;
    let d = sdf_rounded_rect(in.local - center, center, in.radius);
    let a = 1.0 - smoothstep(-0.5, 0.5, d);
    return vec4(in.color.rgb, in.color.a * a);
}

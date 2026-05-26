struct Uniforms {
    screen_size: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct InstanceIn {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv_min: vec2<f32>,
    @location(4) uv_max: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: InstanceIn) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let c = corners[vi];
    let px = inst.pos + c * inst.size;
    let ndc = vec2((px.x / u.screen_size.x) * 2.0 - 1.0, 1.0 - (px.y / u.screen_size.y) * 2.0);

    var out: VertexOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.uv = mix(inst.uv_min, inst.uv_max, c);
    out.color = inst.color;
    return out;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    // Atlas storage convention (see glyph_cache.rs):
    //   RGB = per-subpixel coverage (LCD-style mask from swash::Format::Subpixel)
    //   A   = max(R, G, B) — single-channel fallback / blend gate
    //
    // Output is PREMULTIPLIED alpha:
    //   rgb = color.rgb * cov.rgb * color.a
    //   a   = cov.a            * color.a
    // Paired with pipeline blend (One, OneMinusSrcAlpha) this gives:
    //   out.rgb = color.rgb * cov.rgb * alpha + dst.rgb * (1 - cov.a * alpha)
    // i.e. RGB is masked per-subpixel (the LCD win), the destination is
    // attenuated by the *coarsest* coverage (avoids haloing). For terminal
    // palettes (bright fg on dark bg) the per-channel divergence is small
    // enough that this approximation reads identical to dual-source LCD.
    let cov = textureSample(atlas, atlas_sampler, in.uv);
    let alpha = in.color.a;
    return vec4(in.color.rgb * cov.rgb * alpha, cov.a * alpha);
}

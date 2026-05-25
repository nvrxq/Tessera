//! Headless smoke for the overlay's render path. Exercises the same chain
//! `OverlayApp::redraw` would: GlyphCache → Scene → Renderer → texture.
//! Does NOT open a window — keeps CI happy on headless boxes.

use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
    DEFAULT_ATLAS_SIZE,
};

const FONT: &[u8] = include_bytes!("../../render/assets/GeistMono-Regular.ttf");

#[test]
fn overlay_render_path_works_offscreen() {
    let res = match Resources::new_headless() {
        Ok(r) => r,
        Err(_) => { eprintln!("no GPU; skipping"); return; }
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (512u32, 256u32);
    let target = res.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay-headless"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    let mut renderer = Renderer::new(&res.device, format, DEFAULT_ATLAS_SIZE);
    let mut glyphs = GlyphCache::new(FONT, DEFAULT_ATLAS_SIZE).unwrap();
    let cell = glyphs.cell_metrics(13.0);

    let mut scene = Scene::new();
    scene.push_rect(RectEntry {
        rect: Rect::new(0.0, 0.0, w as f32, h as f32),
        color: Color::rgb(15, 15, 16),
        corner_radius: 0.0,
    });
    let text = "tessera-overlay (headless smoke)";
    let mut x = 16.0_f32;
    let baseline = 20.0 + cell.ascent;
    for ch in text.chars() {
        if let Some(g) = glyphs.get_or_rasterize(ch, 13.0) {
            scene.push_glyph(GlyphEntry {
                rect: Rect::new(x + g.bearing[0], baseline - g.bearing[1], g.size_px[0] as f32, g.size_px[1] as f32),
                color: Color::rgb(232, 232, 230),
                uv_min: g.region.uv_min,
                uv_max: g.region.uv_max,
            });
        }
        x += cell.advance_px;
    }

    renderer.render_with_atlas(
        &res.device, &res.queue, &scene,
        RenderTarget::Texture(&target),
        [w as f32, h as f32],
        &mut glyphs,
    );
}

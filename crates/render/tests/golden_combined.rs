//! Headless integration smoke test: all three pipelines via Renderer in one submit.

use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};

const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

#[test]
fn renderer_draws_combined_scene_in_one_submit() {
    let res = match Resources::new_headless() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("no GPU");
            return;
        }
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (512u32, 256u32);
    let target_tex = res.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let mut renderer = Renderer::new(&res.device, format, 1024);
    let mut cache = GlyphCache::new(FONT, 1024).unwrap();

    let m = cache.get_or_rasterize('M', 48.0).unwrap();
    renderer.glyph.upload_atlas(&res.queue, &cache.atlas_pixels);

    let mut scene = Scene::new();
    scene.push_rect(RectEntry {
        rect: Rect::new(0.0, 0.0, w as f32, h as f32),
        color: Color::rgb(15, 15, 16),
        corner_radius: 0.0,
    });
    scene.push_rect(RectEntry {
        rect: Rect::new(20.0, 20.0, 200.0, 60.0),
        color: Color::rgb(200, 130, 91),
        corner_radius: 6.0,
    });
    scene.push_glyph(GlyphEntry {
        rect: Rect::new(240.0, 80.0, m.size_px[0] as f32, m.size_px[1] as f32),
        color: Color::rgb(232, 232, 230),
        uv_min: m.region.uv_min,
        uv_max: m.region.uv_max,
    });

    renderer.render_to(
        &res.device,
        &res.queue,
        &scene,
        RenderTarget::Texture(&target_tex),
        [w as f32, h as f32],
    );
    // Smoke test: if we got here without panic and queue accepted the submit, we're good.
}

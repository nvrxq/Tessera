//! Frame-time gate. Run with `cargo test -p tessera-render --release -- --ignored perf`.
//!
//! Targets from spec §9:
//!   median ≤ 5 ms, p99 ≤ 16.6 ms.

use std::time::Instant;
use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{Renderer, RenderTarget},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};

const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

#[test]
#[ignore]
fn frame_time_budget() {
    let res = match Resources::new_headless() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("no GPU");
            return;
        }
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (1280u32, 720u32);
    let target = res.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("perf"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut renderer = Renderer::new(&res.device, format, 2048);
    let mut cache = GlyphCache::new(FONT, 2048).unwrap();
    let cell = cache.cell_metrics(13.0);

    // Pre-rasterize printable ASCII so warm-up doesn't pollute timings.
    for ch in (32u8..127).map(|c| c as char) {
        let _ = cache.get_or_rasterize(ch, 13.0);
    }
    renderer.glyph.upload_atlas(&res.queue, &cache.atlas_pixels);

    // 80x24 grid → 1920 glyphs/frame.
    let mut times: Vec<f64> = Vec::with_capacity(1000);
    for frame in 0..1000u32 {
        let mut scene = Scene::new();
        scene.push_rect(RectEntry {
            rect: Rect::new(0.0, 0.0, w as f32, h as f32),
            color: Color::rgb(15, 15, 16),
            corner_radius: 0.0,
        });
        let baseline = cell.ascent;
        for row in 0..24u32 {
            for col in 0..80u32 {
                let ch = (32 + ((frame + col + row) % 95)) as u8 as char;
                if let Some(g) = cache.get_or_rasterize(ch, 13.0) {
                    let x = col as f32 * cell.advance_px;
                    let y = row as f32 * cell.line_height_px + baseline;
                    scene.push_glyph(GlyphEntry {
                        rect: Rect::new(
                            x + g.bearing[0],
                            y - g.bearing[1],
                            g.size_px[0] as f32,
                            g.size_px[1] as f32,
                        ),
                        color: Color::rgb(232, 232, 230),
                        uv_min: g.region.uv_min,
                        uv_max: g.region.uv_max,
                    });
                }
            }
        }

        let t0 = Instant::now();
        renderer.render_to(
            &res.device,
            &res.queue,
            &scene,
            RenderTarget::Texture(&target),
            [w as f32, h as f32],
        );
        // wgpu 29: PollType::wait_indefinitely() replaces Maintain::Wait
        res.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        times.push(ms);
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p99 = times[(times.len() as f64 * 0.99) as usize];
    println!("frame ms: median={:.2} p99={:.2}", median, p99);
    assert!(median <= 5.0, "median frame time {median:.2}ms > 5.0ms");
    assert!(p99 <= 16.6, "p99 frame time {p99:.2}ms > 16.6ms");
}

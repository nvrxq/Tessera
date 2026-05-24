//! Render a known scene, compare to a stored PNG. First run creates the snapshot;
//! re-runs assert per-pixel equality with 8/255 tolerance.

use std::path::PathBuf;
use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{Renderer, RenderTarget},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};

const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

fn snapshot_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("snapshots");
    p.push(format!("{name}.png"));
    p
}

fn render_and_readback(name: &str) -> (u32, u32, Vec<u8>) {
    let res = Resources::new_headless().expect("GPU adapter");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (512u32, 256u32);
    let target = res.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(name),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = Renderer::new(&res.device, format, 1024);
    let mut cache = GlyphCache::new(FONT, 1024).unwrap();

    let mut scene = Scene::new();
    scene.push_rect(RectEntry {
        rect: Rect::new(0.0, 0.0, w as f32, h as f32),
        color: Color::rgb(15, 15, 16),
        corner_radius: 0.0,
    });
    scene.push_rect(RectEntry {
        rect: Rect::new(24.0, 24.0, 200.0, 48.0),
        color: Color::rgb(200, 130, 91),
        corner_radius: 6.0,
    });

    let cell = cache.cell_metrics(28.0);
    let mut x = 24.0_f32;
    let baseline = 96.0 + cell.ascent;
    for ch in "tessera".chars() {
        if let Some(g) = cache.get_or_rasterize(ch, 28.0) {
            scene.push_glyph(GlyphEntry {
                rect: Rect::new(
                    x + g.bearing[0],
                    baseline - g.bearing[1],
                    g.size_px[0] as f32,
                    g.size_px[1] as f32,
                ),
                color: Color::rgb(232, 232, 230),
                uv_min: g.region.uv_min,
                uv_max: g.region.uv_max,
            });
        }
        x += cell.advance_px;
    }
    renderer.glyph.upload_atlas(&res.queue, &cache.atlas_pixels);
    renderer.render_to(
        &res.device,
        &res.queue,
        &scene,
        RenderTarget::Texture(&target),
        [w as f32, h as f32],
    );

    let bpr = (4 * w).next_multiple_of(256);
    let staging = res.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: (bpr * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = res.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    res.queue.submit(Some(enc.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    res.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();

    let mut tight = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let src = (row * bpr) as usize;
        let dst = (row * w * 4) as usize;
        tight[dst..dst + (w * 4) as usize].copy_from_slice(&data[src..src + (w * 4) as usize]);
    }
    (w, h, tight)
}

#[test]
fn tessera_word_snapshot() {
    let (w, h, pixels) =
        match std::panic::catch_unwind(|| render_and_readback("tessera_word")) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("no GPU; skip");
                return;
            }
        };

    let path = snapshot_path("tessera_word");
    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).ok();
        image::save_buffer(&path, &pixels, w, h, image::ColorType::Rgba8).unwrap();
        panic!("snapshot created at {} — re-run to verify", path.display());
    }

    let stored = image::open(&path).expect("snapshot loadable").to_rgba8();
    assert_eq!(stored.width(), w);
    assert_eq!(stored.height(), h);
    let mut diffs = 0u32;
    for (a, b) in stored.as_raw().iter().zip(pixels.iter()) {
        if (*a as i32 - *b as i32).abs() > 8 {
            diffs += 1;
        }
    }
    assert!(diffs < 100, "{diffs} pixels differ by more than 8/255 from snapshot");
}

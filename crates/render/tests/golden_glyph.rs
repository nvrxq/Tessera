//! Headless render-to-texture smoke test for the glyph pipeline.

use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    pipelines::glyph::GlyphPipeline,
    resources::Resources,
    scene::{GlyphEntry, Scene},
};

const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

#[test]
fn glyph_pipeline_draws_m_with_alpha() {
    let res = match Resources::new_headless() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("no GPU; skip");
            return;
        }
    };

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (256u32, 256u32);

    let target = res.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());

    let atlas_size = 1024;
    let mut cache = GlyphCache::new(FONT, atlas_size).expect("font");
    let mut pipeline = GlyphPipeline::new(&res.device, format, atlas_size);

    let g = cache.get_or_rasterize('M', 64.0).expect("rasterizes");
    pipeline.upload_atlas(&res.queue, &cache.atlas_pixels);

    let mut scene = Scene::new();
    scene.push_glyph(GlyphEntry {
        rect: Rect::new(100.0, 100.0, g.size_px[0] as f32, g.size_px[1] as f32),
        color: Color::rgb(232, 232, 230),
        uv_min: g.region.uv_min,
        uv_max: g.region.uv_max,
    });
    let count = pipeline.prepare(&res.device, &res.queue, &scene, [w as f32, h as f32]);

    let mut enc = res.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("glyph pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pipeline.draw(&mut pass, count);
    }

    // wgpu 29: copy_texture_to_buffer uses TexelCopy* types
    let bpr = (4 * w).next_multiple_of(256);
    let staging = res.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("staging"),
        size: (bpr * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
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
    // wgpu 29: PollType::wait_indefinitely() replaces Maintain::Wait
    res.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();

    let mut lit = 0u32;
    for row in 100..100 + g.size_px[1] {
        for col in 100..100 + g.size_px[0] {
            let i = (row * bpr + col * 4) as usize;
            if data[i] > 50 {
                lit += 1;
            }
        }
    }
    assert!(lit > 5, "no lit pixels found inside glyph rect");
}

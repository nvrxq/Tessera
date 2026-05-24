//! Headless render-to-texture smoke test for the rect pipeline.

use tessera_render::{
    geometry::{Color, Rect},
    pipelines::rect::RectPipeline,
    resources::Resources,
    scene::{RectEntry, Scene},
};

#[test]
fn rect_pipeline_draws_solid_pixels() {
    let res = match Resources::new_headless() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("no GPU; skipping");
            return;
        }
    };

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (w, h) = (128u32, 128u32);

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

    let mut pipeline = RectPipeline::new(&res.device, format);
    let mut scene = Scene::new();
    scene.push_rect(RectEntry {
        rect: Rect::new(32.0, 32.0, 64.0, 64.0),
        color: Color::rgb(200, 130, 91),
        corner_radius: 0.0,
    });
    let count = pipeline.prepare(&res.device, &res.queue, &scene, [w as f32, h as f32]);

    let mut enc = res.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rect pass"),
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
    res.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range();

    // Pixel at (64, 64) — centre of the 32..96 rect — should be lit and opaque.
    let i = (64 * bpr + 64 * 4) as usize;
    let px = [data[i], data[i + 1], data[i + 2], data[i + 3]];

    assert!(px[0] > 100, "center pixel not lit: {:?}", px);
    assert!(px[3] == 255, "center pixel not opaque: {:?}", px);
}

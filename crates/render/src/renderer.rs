use crate::pipelines::{glyph::GlyphPipeline, image::ImagePipeline, rect::RectPipeline};
use crate::scene::Scene;

pub enum RenderTarget<'a> {
    Texture(&'a wgpu::Texture),
    Surface(&'a wgpu::SurfaceTexture),
}

pub struct Renderer {
    pub rect: RectPipeline,
    pub glyph: GlyphPipeline,
    pub image: ImagePipeline,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, atlas_size: u32) -> Self {
        Self {
            rect: RectPipeline::new(device, color_format),
            glyph: GlyphPipeline::new(device, color_format, atlas_size),
            image: ImagePipeline::new(device, color_format, 4),
        }
    }

    pub fn render_to(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        target: RenderTarget<'_>,
        screen: [f32; 2],
    ) {
        let rect_n = self.rect.prepare(device, queue, scene, screen);
        let glyph_n = self.glyph.prepare(device, queue, scene, screen);
        let image_n = self.image.prepare(device, queue, scene, screen);

        let view = match target {
            RenderTarget::Texture(t) => t.create_view(&Default::default()),
            RenderTarget::Surface(s) => s.texture.create_view(&Default::default()),
        };

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.06,
                            b: 0.06,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            self.rect.draw(&mut pass, rect_n);
            self.glyph.draw(&mut pass, glyph_n);
            self.image.draw(&mut pass, image_n);
        }
        queue.submit(Some(enc.finish()));
    }
}

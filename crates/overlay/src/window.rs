//! winit `ApplicationHandler` implementation for the overlay window.

use std::sync::Arc;
use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::bounds::{Bounds, OverlayConfig};
use crate::messages::OverlayMessage;

const FONT: &[u8] = include_bytes!("../../render/assets/GeistMono-Regular.ttf");

pub struct OverlayApp {
    pub(crate) config: OverlayConfig,
    pub(crate) bounds: Bounds,
    pub(crate) visible: bool,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    res: Resources,
    renderer: Option<Renderer>,
    glyphs: GlyphCache<'static>,
    cell: tessera_render::glyph_cache::CellMetrics,
}

impl OverlayApp {
    pub fn new(config: OverlayConfig) -> Self {
        let res = Resources::new_headless().expect("GPU adapter required for overlay");
        let glyphs = GlyphCache::new(FONT, config.atlas_size).expect("font load");
        let cell = glyphs.cell_metrics(13.0);
        Self {
            bounds: config.initial,
            visible: config.visible,
            config,
            window: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            res,
            renderer: None,
            glyphs,
            cell,
        }
    }
}

impl ApplicationHandler<OverlayMessage> for OverlayApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let b = self.bounds.nonzero();
        let attrs = Window::default_attributes()
            .with_title("tessera-overlay")
            .with_decorations(false)
            .with_resizable(false)
            .with_position(winit::dpi::PhysicalPosition::new(b.x, b.y))
            .with_inner_size(winit::dpi::PhysicalSize::new(b.w, b.h))
            .with_visible(self.visible);
        let window = Arc::new(el.create_window(attrs).expect("window"));
        let surface: wgpu::Surface<'static> = self
            .res
            .instance
            .create_surface(window.clone())
            .expect("surface");
        let caps = surface.get_capabilities(&self.res.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        surface.configure(
            &self.res.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: b.w,
                height: b.h,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );
        self.surface_format = format;
        self.renderer = Some(Renderer::new(&self.res.device, format, self.config.atlas_size));
        self.surface = Some(surface);
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                self.bounds.w = s.width.max(1);
                self.bounds.h = s.height.max(1);
                if let Some(surface) = &self.surface {
                    surface.configure(
                        &self.res.device,
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: self.surface_format,
                            width: self.bounds.w,
                            height: self.bounds.h,
                            present_mode: wgpu::PresentMode::Fifo,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                            view_formats: vec![],
                            desired_maximum_frame_latency: 2,
                        },
                    );
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: OverlayMessage) {
        match ev {
            OverlayMessage::SetBounds(b) => {
                let b = b.nonzero();
                self.bounds = b;
                if let Some(w) = &self.window {
                    w.set_outer_position(winit::dpi::PhysicalPosition::new(b.x, b.y));
                    let _ = w.request_inner_size(winit::dpi::PhysicalSize::new(b.w, b.h));
                    w.request_redraw();
                }
            }
            OverlayMessage::SetVisible(v) => {
                self.visible = v;
                if let Some(w) = &self.window {
                    w.set_visible(v);
                }
            }
            OverlayMessage::Shutdown => {
                el.exit();
            }
        }
    }
}

impl OverlayApp {
    fn redraw(&mut self) {
        let renderer = match self.renderer.as_mut() {
            Some(r) => r,
            None => return,
        };
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return,
        };
        // wgpu 29: get_current_texture() returns CurrentSurfaceTexture (an enum),
        // not Result<SurfaceTexture, SurfaceError>. Extract the inner SurfaceTexture
        // from Success/Suboptimal; skip the frame on any error variant.
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };

        // Placeholder scene — Plan 4 will replace with Term-driven output.
        let mut scene = Scene::new();
        scene.push_rect(RectEntry {
            rect: Rect::new(0.0, 0.0, self.bounds.w as f32, self.bounds.h as f32),
            color: Color::rgb(15, 15, 16),
            corner_radius: 0.0,
        });
        let text = "tessera-overlay (Plan 3 placeholder)";
        let mut x = 16.0_f32;
        let baseline = 20.0 + self.cell.ascent;
        for ch in text.chars() {
            if let Some(g) = self.glyphs.get_or_rasterize(ch, 13.0) {
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
            x += self.cell.advance_px;
        }

        renderer.render_with_atlas(
            &self.res.device,
            &self.res.queue,
            &scene,
            RenderTarget::Surface(&frame),
            [self.bounds.w as f32, self.bounds.h as f32],
            &mut self.glyphs,
        );
        frame.present();

        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

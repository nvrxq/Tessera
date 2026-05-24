use std::sync::Arc;

use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
    DEFAULT_ATLAS_SIZE,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const FONT: &[u8] = include_bytes!("../assets/GeistMono-Regular.ttf");

struct App {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    res: Resources,
    renderer: Option<Renderer>,
    glyphs: GlyphCache<'static>,
    cell: tessera_render::glyph_cache::CellMetrics,
    sample_text: String,
    size: (u32, u32),
}

impl App {
    fn new() -> Self {
        let res = Resources::new_headless().expect("GPU adapter required for demo");
        let glyphs = GlyphCache::new(FONT, DEFAULT_ATLAS_SIZE).expect("font");
        let cell = glyphs.cell_metrics(26.0);
        Self {
            window: None,
            surface: None,
            surface_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            res,
            renderer: None,
            glyphs,
            cell,
            sample_text: "Tessera GPU terminal — Warp-style scene + 3 pipelines".to_string(),
            size: (960, 480),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("tessera-render demo")
            .with_inner_size(winit::dpi::LogicalSize::new(self.size.0, self.size.1));
        let window = Arc::new(el.create_window(attrs).expect("window"));

        // wgpu 29: create_surface accepts Arc<Window> via From<T: DisplayAndWindowHandle>.
        // The surface lifetime is 'static because Arc<Window>: 'static — the Arc keeps
        // the window alive for the duration of the surface.
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
                width: self.size.0,
                height: self.size.1,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );

        self.surface_format = format;
        self.renderer = Some(Renderer::new(&self.res.device, format, DEFAULT_ATLAS_SIZE));
        self.surface = Some(surface);
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),

            WindowEvent::Resized(s) => {
                self.size = (s.width.max(1), s.height.max(1));
                if let Some(surface) = &self.surface {
                    surface.configure(
                        &self.res.device,
                        &wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: self.surface_format,
                            width: self.size.0,
                            height: self.size.1,
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

                let mut scene = Scene::new();

                // Background fill.
                scene.push_rect(RectEntry {
                    rect: Rect::new(0.0, 0.0, self.size.0 as f32, self.size.1 as f32),
                    color: Color::rgb(15, 15, 16),
                    corner_radius: 0.0,
                });

                // Slightly elevated panel for the text.
                scene.push_rect(RectEntry {
                    rect: Rect::new(40.0, 40.0, (self.size.0 - 80) as f32, 60.0),
                    color: Color::rgb(20, 20, 21),
                    corner_radius: 6.0,
                });

                // Render sample text in Geist Mono.
                let mut x = 56.0_f32;
                let baseline = 80.0_f32 + self.cell.ascent;
                for ch in self.sample_text.chars() {
                    if let Some(g) = self.glyphs.get_or_rasterize(ch, 26.0) {
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

                // Upload atlas if any new glyphs were rasterized this frame.
                if self.glyphs.atlas_dirty {
                    renderer
                        .glyph
                        .upload_atlas(&self.res.queue, &self.glyphs.atlas_pixels);
                    self.glyphs.atlas_dirty = false;
                }

                renderer.render_to(
                    &self.res.device,
                    &self.res.queue,
                    &scene,
                    RenderTarget::Surface(&frame),
                    [self.size.0 as f32, self.size.1 as f32],
                );
                frame.present();

                // Request continuous redraw so the window keeps painting.
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            _ => {}
        }
    }
}

fn main() {
    env_logger::init();
    let el = EventLoop::new().expect("event loop");
    let mut app = App::new();
    el.run_app(&mut app).expect("run");
}

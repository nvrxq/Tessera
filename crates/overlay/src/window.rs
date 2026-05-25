//! winit `ApplicationHandler` implementation for the overlay window.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tessera_term::{palette::ColorPalette, Term};
use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};
use uuid::Uuid;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::bounds::{Bounds, OverlayConfig};
use crate::messages::OverlayMessage;

/// No-op writer for wezterm's `Terminal::new` — keystroke echo is routed
/// via Tauri's `pty_write` command to `Supervisor::write`, not through
/// wezterm's writer. wezterm needs a Write impl to construct; this is
/// the silent sink.
struct DevNull;
impl Write for DevNull {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> { Ok(buf.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

const FONT: &[u8] = include_bytes!("../../render/assets/GeistMono-Regular.ttf");

pub struct OverlayApp {
    config: OverlayConfig,
    bounds: Bounds,
    visible: bool,
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_format: wgpu::TextureFormat,
    surface_alpha_mode: wgpu::CompositeAlphaMode,
    res: Resources,
    renderer: Option<Renderer>,
    glyphs: GlyphCache<'static>,
    cell: tessera_render::glyph_cache::CellMetrics,
    palette: ColorPalette,
    sessions: HashMap<Uuid, Term>,
    active: Option<Uuid>,
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
            surface_alpha_mode: wgpu::CompositeAlphaMode::Auto,
            res,
            renderer: None,
            glyphs,
            cell,
            palette: ColorPalette::tessera_dark(),
            sessions: HashMap::new(),
            active: None,
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
        self.surface_alpha_mode = caps.alpha_modes[0];
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
                            alpha_mode: self.surface_alpha_mode,
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
            OverlayMessage::FeedBytes { session_id, bytes } => {
                let (cols, rows) = self.compute_cell_grid();
                let term = self.sessions.entry(session_id).or_insert_with(|| {
                    Term::new(cols, rows, Box::new(DevNull))
                });
                term.feed(&bytes);
                if Some(session_id) == self.active && self.visible {
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }
            OverlayMessage::ExitSession(id) => {
                self.sessions.remove(&id);
                if Some(id) == self.active {
                    self.active = None;
                    if self.visible {
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                }
            }
            OverlayMessage::SelectSession(id) => {
                self.active = id;
                if self.visible {
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }
            OverlayMessage::ResizeGrid { cols, rows } => {
                if let Some(id) = self.active {
                    if let Some(term) = self.sessions.get_mut(&id) {
                        term.resize(cols, rows);
                    }
                }
            }
            OverlayMessage::Shutdown => { el.exit(); }
        }
    }
}

impl OverlayApp {
    fn compute_cell_grid(&self) -> (u16, u16) {
        let cols = ((self.bounds.w as f32) / self.cell.advance_px).floor().max(1.0) as u16;
        let rows = ((self.bounds.h as f32) / self.cell.line_height_px).floor().max(1.0) as u16;
        (cols, rows)
    }

    fn redraw(&mut self) {
        let renderer = match self.renderer.as_mut() { Some(r) => r, None => return };
        let surface = match self.surface.as_ref() { Some(s) => s, None => return };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };

        let mut scene = Scene::new();

        // Background fill — Tessera --bg.
        scene.push_rect(RectEntry {
            rect: Rect::new(0.0, 0.0, self.bounds.w as f32, self.bounds.h as f32),
            color: Color::rgb(15, 15, 16),
            corner_radius: 0.0,
        });

        // Active session's grid → rect+glyph entries.
        if let Some(active) = self.active {
            if let Some(term) = self.sessions.get(&active) {
                let grid = term.grid(&self.palette);
                let advance = self.cell.advance_px;
                let line_h = self.cell.line_height_px;
                let ascent = self.cell.ascent;
                for (row_idx, row) in grid.rows_iter().enumerate() {
                    let y_top = row_idx as f32 * line_h;
                    let baseline = y_top + ascent;
                    for (col_idx, cell) in row.iter().enumerate() {
                        let x_origin = col_idx as f32 * advance;
                        // Skip cell bg if it matches default — saves ~1900
                        // instances per frame in a typical 80×24 grid.
                        if cell.bg != self.palette.default_bg {
                            scene.push_rect(RectEntry {
                                rect: Rect::new(x_origin, y_top, advance, line_h),
                                color: cell.bg,
                                corner_radius: 0.0,
                            });
                        }
                        // Skip blanks.
                        if cell.ch == ' ' { continue; }
                        if let Some(g) = self.glyphs.get_or_rasterize(cell.ch, 13.0) {
                            scene.push_glyph(GlyphEntry {
                                rect: Rect::new(
                                    x_origin + g.bearing[0],
                                    baseline - g.bearing[1],
                                    g.size_px[0] as f32,
                                    g.size_px[1] as f32,
                                ),
                                color: cell.fg,
                                uv_min: g.region.uv_min,
                                uv_max: g.region.uv_max,
                            });
                        }
                    }
                }

                // Cursor: solid block at cursor position.
                let cur = term.cursor();
                if cur.visible {
                    scene.push_rect(RectEntry {
                        rect: Rect::new(
                            cur.col as f32 * advance,
                            cur.row as f32 * line_h,
                            advance.max(2.0),
                            line_h,
                        ),
                        color: self.palette.default_fg,
                        corner_radius: 0.0,
                    });
                }
            }
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
        // No continuous redraw — redraw is event-driven (FeedBytes / SelectSession etc).
    }
}

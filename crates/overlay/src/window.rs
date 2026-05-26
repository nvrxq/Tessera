//! winit `ApplicationHandler` implementation for the overlay window.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tessera_render::{
    geometry::{Color, Rect},
    glyph_cache::GlyphCache,
    renderer::{RenderTarget, Renderer},
    resources::Resources,
    scene::{GlyphEntry, RectEntry, Scene},
};
use tessera_term::{palette::ColorPalette, Term};
use uuid::Uuid;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, Modifiers, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
#[cfg(all(unix, not(target_os = "macos")))]
use winit::platform::x11::{WindowAttributesExtX11, WindowType};
use winit::window::{Window, WindowId};

use crate::bounds::{Bounds, OverlayConfig};
use crate::messages::OverlayMessage;

/// No-op writer for wezterm's `Terminal::new` — keystroke echo is routed
/// via Tauri's `pty_write` command to `Supervisor::write`, not through
/// wezterm's writer. wezterm needs a Write impl to construct; this is
/// the silent sink.
struct DevNull;
impl Write for DevNull {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const FONT_REGULAR: &[u8] = include_bytes!("../../render/assets/GeistMono-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../render/assets/GeistMono-Bold.ttf");

pub struct OverlayApp {
    config: OverlayConfig,
    bounds: Bounds,
    visible: bool,
    /// CSS→physical pixel ratio. Glyphs rasterize at `base_font_px * scale_factor`
    /// so they read at `base_font_px` CSS pixels on screen.
    scale_factor: f32,
    /// Current logical (CSS) font size — driven by the host UI's persisted
    /// preference. Bounded by `MIN_FONT_PX..=MAX_FONT_PX` on every update.
    base_font_px: f32,
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
    /// Latest keyboard modifier state — winit reports modifiers in a separate
    /// event, so we track them and pair with each KeyEvent at press time.
    modifiers: ModifiersState,
}

/// Default logical (CSS) font size for the terminal text. On a standard-DPI
/// 1440p display 13 px is microscopic; 20 px gives ~12 px cap height,
/// matching Warp/iTerm defaults at "small" zoom. The host UI may override
/// at runtime via `SetFontSize`.
pub const DEFAULT_FONT_PX: f32 = 20.0;
/// Clamp range for the user-adjustable font size.
const MIN_FONT_PX: f32 = 8.0;
const MAX_FONT_PX: f32 = 64.0;

impl OverlayApp {
    pub fn new(config: OverlayConfig) -> Self {
        let res = Resources::new_headless().expect("GPU adapter required for overlay");
        let glyphs = GlyphCache::new_with_bold(FONT_REGULAR, FONT_BOLD, config.atlas_size)
            .expect("font load");
        let cell = glyphs.cell_metrics(DEFAULT_FONT_PX);
        Self {
            bounds: config.initial,
            visible: config.visible,
            scale_factor: 1.0,
            base_font_px: DEFAULT_FONT_PX,
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
            modifiers: ModifiersState::default(),
        }
    }

    fn effective_px(&self) -> f32 {
        self.base_font_px * self.scale_factor.max(1.0)
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
        // `override_redirect=true` makes the X11 server skip WM management
        // entirely: i3 won't tile, focus, or fullscreen this window — even
        // hovering over it leaves keyboard focus on the Tauri main window.
        // The cost is we own stacking: we XRaiseWindow whenever bounds
        // change so the overlay stays above the WebView.
        //
        // `WM_HINTS.input=False` alone is not enough — i3 still binds its
        // shortcuts (alt+f, etc.) to whichever window the pointer is over.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = attrs
            .with_x11_window_type(vec![WindowType::Notification])
            .with_override_redirect(true);
        let window = Arc::new(el.create_window(attrs).expect("window"));
        // Mark the overlay non-focusable on X11. i3 with focus_follows_mouse
        // (the default) would otherwise focus it on hover, and alt+f would
        // fullscreen JUST the overlay instead of the Tauri main window.
        // WM_HINTS.input = False + WM_TRANSIENT_FOR=parent makes WMs treat
        // it as a passive helper that never owns input focus.
        #[cfg(all(unix, not(target_os = "macos")))]
        apply_x11_passive_hints(&window, self.config.parent_window_id);
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
        self.renderer = Some(Renderer::new(
            &self.res.device,
            format,
            self.config.atlas_size,
        ));
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
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
                // Some platforms send a fresh `ModifiersChanged` for plain
                // keypresses too — keep the struct around for the keymap.
                let _ = m as Modifiers;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let (Some(bytes), Some(session)) =
                    (encode_key(&event, self.modifiers), self.active)
                {
                    if let Some(cb) = self.config.on_key.as_ref() {
                        cb(session, bytes);
                    }
                }
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
                    #[cfg(all(unix, not(target_os = "macos")))]
                    x11_raise_window(w);
                    w.request_redraw();
                }
            }
            OverlayMessage::SetScaleFactor(sf) => {
                let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
                if (sf - self.scale_factor).abs() > 0.01 {
                    self.scale_factor = sf;
                    self.cell = self.glyphs.cell_metrics(self.effective_px());
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            OverlayMessage::SetFontSize(px) => {
                let px = if px.is_finite() {
                    px.clamp(MIN_FONT_PX, MAX_FONT_PX)
                } else {
                    DEFAULT_FONT_PX
                };
                if (px - self.base_font_px).abs() > 0.01 {
                    self.base_font_px = px;
                    self.cell = self.glyphs.cell_metrics(self.effective_px());
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            OverlayMessage::SetVisible(v) => {
                self.visible = v;
                if let Some(w) = &self.window {
                    w.set_visible(v);
                    #[cfg(all(unix, not(target_os = "macos")))]
                    if v {
                        x11_raise_window(w);
                    }
                }
            }
            OverlayMessage::FeedBytes { session_id, bytes } => {
                let (cols, rows) = self.compute_cell_grid();
                let term = self
                    .sessions
                    .entry(session_id)
                    .or_insert_with(|| Term::new(cols, rows, Box::new(DevNull)));
                term.feed(&bytes);
                if Some(session_id) == self.active && self.visible {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            OverlayMessage::ExitSession(id) => {
                self.sessions.remove(&id);
                if Some(id) == self.active {
                    self.active = None;
                    if self.visible {
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }
                }
            }
            OverlayMessage::SelectSession(id) => {
                self.active = id;
                if self.visible {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }
            OverlayMessage::ResizeGrid { cols, rows } => {
                if let Some(id) = self.active {
                    if let Some(term) = self.sessions.get_mut(&id) {
                        term.resize(cols, rows);
                    }
                }
            }
            OverlayMessage::Shutdown => {
                el.exit();
            }
        }
    }
}

impl OverlayApp {
    fn compute_cell_grid(&self) -> (u16, u16) {
        let cols = ((self.bounds.w as f32) / self.cell.advance_px)
            .floor()
            .max(1.0) as u16;
        let rows = ((self.bounds.h as f32) / self.cell.line_height_px)
            .floor()
            .max(1.0) as u16;
        (cols, rows)
    }

    fn redraw(&mut self) {
        let renderer = match self.renderer.as_mut() {
            Some(r) => r,
            None => return,
        };
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return,
        };
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

                // Block frames — render BEFORE per-cell content so they sit
                // behind everything except the window background. Each shell
                // command (delimited by Tessera DCS Start/End markers from
                // the integration script) becomes a rounded rect with a
                // subtle darker bg + accent border on the most-recent
                // (active) block.
                //
                // Layout: blocks span the full grid width and cover the
                // [start_row, end_row] (inclusive, viewport rows). An
                // in-progress block (end_row=None) extends to the cursor's
                // current row.
                let blocks = term.blocks();
                if !blocks.is_empty() {
                    // Tessera design tokens (see DESIGN.md):
                    //   --bg-elevated ~ #15151A
                    //   --accent      = #C8825B (terracotta)
                    //   --border-soft ~ #2A2A30
                    let block_bg = Color::rgb(0x15, 0x15, 0x1A);
                    let accent = Color::rgb(0xC8, 0x82, 0x5B);
                    let border_soft = Color::rgb(0x2A, 0x2A, 0x30);
                    let cur_row = term.cursor().row;
                    let active_idx = blocks.len().saturating_sub(1);
                    let block_pad_x = 4.0;
                    let corner_r = 6.0;
                    let border_thickness = 1.0_f32;

                    for (idx, b) in blocks.iter().enumerate() {
                        let end = b.end_row.unwrap_or(cur_row);
                        if end < b.start_row {
                            continue;
                        }
                        let by = b.start_row as f32 * line_h;
                        let bh = ((end - b.start_row + 1) as f32) * line_h;
                        let bw = self.bounds.w as f32 - block_pad_x * 2.0;
                        // Filled background.
                        scene.push_rect(RectEntry {
                            rect: Rect::new(block_pad_x, by, bw, bh),
                            color: block_bg,
                            corner_radius: corner_r,
                        });
                        // Border — drawn as 4 thin rects (top/bottom/L/R).
                        // Cheap to add; matches the Warp visual without a
                        // dedicated stroke pipeline. Active block uses
                        // accent terracotta, otherwise a subtle gray.
                        let border_color = if idx == active_idx {
                            accent
                        } else {
                            border_soft
                        };
                        let bt = border_thickness;
                        // top
                        scene.push_rect(RectEntry {
                            rect: Rect::new(block_pad_x, by, bw, bt),
                            color: border_color,
                            corner_radius: 0.0,
                        });
                        // bottom
                        scene.push_rect(RectEntry {
                            rect: Rect::new(block_pad_x, by + bh - bt, bw, bt),
                            color: border_color,
                            corner_radius: 0.0,
                        });
                        // left
                        scene.push_rect(RectEntry {
                            rect: Rect::new(block_pad_x, by, bt, bh),
                            color: border_color,
                            corner_radius: 0.0,
                        });
                        // right
                        scene.push_rect(RectEntry {
                            rect: Rect::new(block_pad_x + bw - bt, by, bt, bh),
                            color: border_color,
                            corner_radius: 0.0,
                        });
                    }
                }
                // Warp's `UNDERLINE_THICKNESS_SCALE_FACTOR` from
                // warpdotdev/warp app/src/terminal/grid_renderer.rs:50 (MIT).
                const UNDERLINE_THICKNESS_SCALE_FACTOR: f32 = 0.15;
                let underline_thickness =
                    (UNDERLINE_THICKNESS_SCALE_FACTOR * advance.round()).max(1.0);
                // Collect decorations (underlines / strikethroughs) and draw
                // them AFTER the per-row glyphs so a following row's bg cannot
                // cover them. Mirrors Warp's `cell_decorations` Vec in
                // grid_renderer.rs `draw_grid_row`.
                let mut decorations: Vec<RectEntry> = Vec::new();
                for (row_idx, row) in grid.rows_iter().enumerate() {
                    let y_top = row_idx as f32 * line_h;
                    let baseline = y_top + ascent;
                    // Background batching — ported from Warp's
                    // `maybe_draw_background` + `CachedBackgroundColor`
                    // (grid_renderer.rs:1517 / :208). Adjacent cells with the
                    // same non-default bg collapse into a single rect; the
                    // run flushes when the color changes or the row ends.
                    // A long highlighted block in `grep --color` was ~80 rects
                    // per row; now it is one.
                    let mut bg_run: Option<(Color, usize, usize)> = None; // (color, start_col, end_col_excl)
                    let flush_run = |scene: &mut Scene, run: Option<(Color, usize, usize)>| {
                        if let Some((color, start, end)) = run {
                            scene.push_rect(RectEntry {
                                rect: Rect::new(
                                    start as f32 * advance,
                                    y_top,
                                    (end - start) as f32 * advance,
                                    line_h,
                                ),
                                color,
                                corner_radius: 0.0,
                            });
                        }
                    };
                    for (col_idx, cell) in row.iter().enumerate() {
                        let x_origin = col_idx as f32 * advance;
                        // Background batching step.
                        if cell.bg == self.palette.default_bg {
                            flush_run(&mut scene, bg_run);
                            bg_run = None;
                        } else {
                            bg_run = match bg_run {
                                Some((c, s, e)) if c == cell.bg => Some((c, s, e + 1)),
                                other => {
                                    flush_run(&mut scene, other);
                                    Some((cell.bg, col_idx, col_idx + 1))
                                }
                            };
                        }
                        // Decorations (underline / double-underline / strike)
                        // — ported from Warp's `calculate_cell_decorations`
                        // (grid_renderer.rs:2292-2302). Branch priority follows
                        // Warp: double-underline > single-underline > strike.
                        // Origin is always `(0, y - thickness)` in cell-local
                        // coords; here we offset by (x_origin, y_top).
                        let deco = if cell.double_underline {
                            Some((underline_thickness * 2.0, line_h))
                        } else if cell.underline {
                            Some((underline_thickness, line_h))
                        } else if cell.strikethrough {
                            Some((underline_thickness, line_h * 0.5))
                        } else {
                            None
                        };
                        if let Some((thickness, y)) = deco {
                            decorations.push(RectEntry {
                                rect: Rect::new(
                                    x_origin,
                                    y_top + y - thickness,
                                    advance,
                                    thickness,
                                ),
                                color: cell.fg,
                                corner_radius: 0.0,
                            });
                        }
                        // Skip blanks for the glyph step, but decorations were
                        // already collected above (a strikethrough on a blank
                        // cell happens e.g. with `\e[9m   \e[0m`).
                        if cell.ch == ' ' {
                            continue;
                        }
                        let eff = self.base_font_px * self.scale_factor.max(1.0);
                        // Use Bold .ttf for SGR-bold cells so we get real heavy
                        // strokes from the type designer, not algorithmic faux-
                        // bold (which would smear stems and break monospacing).
                        let weight = if cell.bold {
                            tessera_render::glyph_cache::FontWeight::Bold
                        } else {
                            tessera_render::glyph_cache::FontWeight::Regular
                        };
                        if let Some(g) = self.glyphs.get_or_rasterize_weighted(cell.ch, weight, eff)
                        {
                            // Snap glyph origin to integer pixels. Cell
                            // metrics are integer too, but bearings can
                            // come back fractional from the rasterizer and
                            // any sub-pixel offset re-samples the atlas at
                            // different fractional positions per row →
                            // visibly uneven baselines. Round to the nearest
                            // device pixel to keep every row crisp.
                            let gx = (x_origin + g.bearing[0]).round();
                            let gy = (baseline - g.bearing[1]).round();
                            scene.push_glyph(GlyphEntry {
                                rect: Rect::new(gx, gy, g.size_px[0] as f32, g.size_px[1] as f32),
                                color: cell.fg,
                                uv_min: g.region.uv_min,
                                uv_max: g.region.uv_max,
                            });
                        }
                    }
                    // Flush any background run that reached the row's right edge.
                    flush_run(&mut scene, bg_run);
                }
                // Flush decorations — drawn LAST so cell bgs from later rows
                // can't cover an underline. Matches Warp's pattern where
                // decorations are kept out of the bg-merge batch and emitted
                // separately after the row loop.
                for d in decorations {
                    scene.push_rect(d);
                }

                // Cursor variants — ported from Warp's `render_cursor`
                // (warpdotdev/warp app/src/terminal/grid_renderer.rs:2379-2407).
                // Block:     full cell rect.
                // Underline: thin strip at y = line_h - thickness, full width.
                // Bar:       thin strip at x = 0, full height.
                let cur = term.cursor();
                if cur.visible {
                    // Mirror `CURSOR_THICKNESS_SCALE_FACTOR = 0.15` from
                    // grid_renderer.rs:47.
                    const CURSOR_THICKNESS_SCALE_FACTOR: f32 = 0.15;
                    let cursor_thickness =
                        (CURSOR_THICKNESS_SCALE_FACTOR * advance.round()).max(1.0);
                    let cx = cur.col as f32 * advance;
                    let cy = cur.row as f32 * line_h;
                    let rect = match cur.shape {
                        tessera_term::CursorShape::Block => {
                            Rect::new(cx, cy, advance.max(2.0), line_h)
                        }
                        tessera_term::CursorShape::Underline => Rect::new(
                            cx,
                            cy + line_h - cursor_thickness,
                            advance,
                            cursor_thickness,
                        ),
                        tessera_term::CursorShape::Bar => {
                            Rect::new(cx, cy, cursor_thickness, line_h)
                        }
                    };
                    scene.push_rect(RectEntry {
                        rect,
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

/// XRaiseWindow on the overlay — call after every map/resize so the
/// override-redirect window stays above the WebView. Without this any
/// later XMapWindow on the WebView (e.g. focus change) would re-stack the
/// overlay below it.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_raise_window(window: &winit::window::Window) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use x11_dl::xlib;

    let Ok(dh) = window.display_handle() else {
        return;
    };
    let Ok(wh) = window.window_handle() else {
        return;
    };
    let display_ptr = match dh.as_raw() {
        RawDisplayHandle::Xlib(d) => match d.display {
            Some(p) => p.as_ptr() as *mut xlib::Display,
            None => return,
        },
        _ => return,
    };
    let xid = match wh.as_raw() {
        RawWindowHandle::Xlib(w) => w.window,
        RawWindowHandle::Xcb(w) => w.window.get() as u64,
        _ => return,
    };
    let Ok(api) = xlib::Xlib::open() else { return };
    unsafe {
        (api.XRaiseWindow)(display_ptr, xid);
        (api.XSync)(display_ptr, xlib::False);
    }
}

/// Tell the X11 WM that this window does not want input focus and is a
/// transient helper of the Tauri main window. Without these:
///   - i3 with `focus_follows_mouse` (default) focuses the overlay when the
///     pointer enters its area, so `alt+f` fullscreens just the overlay.
///   - The overlay shows up as a separate client in the WM's window list.
///
/// We touch raw Xlib via `x11-dl` because winit 0.30 doesn't expose either
/// hint. Failures are logged and ignored — the overlay still works, the WM
/// just won't treat it as passive.
#[cfg(all(unix, not(target_os = "macos")))]
fn apply_x11_passive_hints(window: &winit::window::Window, parent_xid: Option<u32>) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use x11_dl::xlib;

    let dh = match window.display_handle() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "no X11 display handle for overlay");
            return;
        }
    };
    let wh = match window.window_handle() {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "no X11 window handle for overlay");
            return;
        }
    };
    let display_ptr = match dh.as_raw() {
        RawDisplayHandle::Xlib(d) => match d.display {
            Some(ptr) => ptr.as_ptr() as *mut xlib::Display,
            None => return,
        },
        _ => return,
    };
    let xid = match wh.as_raw() {
        RawWindowHandle::Xlib(w) => w.window,
        RawWindowHandle::Xcb(w) => w.window.get() as u64,
        _ => return,
    };

    let xlib_api = match xlib::Xlib::open() {
        Ok(api) => api,
        Err(e) => {
            tracing::warn!(error = %e, "libX11 dlopen failed");
            return;
        }
    };
    unsafe {
        // WM_HINTS.input = False → "I never want input focus" (ICCCM passive).
        let mut hints: xlib::XWMHints = std::mem::zeroed();
        hints.flags = xlib::InputHint;
        hints.input = xlib::False;
        (xlib_api.XSetWMHints)(display_ptr, xid, &mut hints);

        // WM_TRANSIENT_FOR groups the overlay with its parent so WMs that
        // honor it stack and move them together.
        if let Some(parent) = parent_xid {
            (xlib_api.XSetTransientForHint)(display_ptr, xid, parent as xlib::Window);
        }

        (xlib_api.XSync)(display_ptr, xlib::False);
    }
}

/// Encode a winit KeyEvent into the byte sequence a Unix TTY expects.
/// Mirrors the minimal keymap in `ui/src/Terminal.tsx::encodeKey` so the
/// behavior is identical whether keys reach the PTY via the WebView's
/// `pty_write` invoke or via the overlay's own X11 focus.
fn encode_key(ev: &KeyEvent, mods: ModifiersState) -> Option<Vec<u8>> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    let meta = mods.super_key();

    // Ctrl+letter → control byte (Ctrl-C = 0x03, etc.).
    if ctrl && !alt && !meta {
        if let Key::Character(s) = &ev.logical_key {
            if let Some(c) = s.chars().next() {
                let lc = c.to_ascii_lowercase();
                if lc.is_ascii_alphabetic() {
                    return Some(vec![(lc as u8) - b'a' + 1]);
                }
            }
        }
    }

    if let Key::Named(named) = &ev.logical_key {
        return Some(match named {
            NamedKey::Enter => vec![0x0d],
            NamedKey::Tab => vec![0x09],
            NamedKey::Backspace => vec![0x7f],
            NamedKey::Escape => vec![0x1b],
            NamedKey::ArrowUp => vec![0x1b, b'[', b'A'],
            NamedKey::ArrowDown => vec![0x1b, b'[', b'B'],
            NamedKey::ArrowRight => vec![0x1b, b'[', b'C'],
            NamedKey::ArrowLeft => vec![0x1b, b'[', b'D'],
            NamedKey::Home => vec![0x1b, b'[', b'H'],
            NamedKey::End => vec![0x1b, b'[', b'F'],
            NamedKey::PageUp => vec![0x1b, b'[', b'5', b'~'],
            NamedKey::PageDown => vec![0x1b, b'[', b'6', b'~'],
            NamedKey::Delete => vec![0x1b, b'[', b'3', b'~'],
            NamedKey::Space => vec![b' '],
            _ => return None,
        });
    }

    // Printable text — winit already composed dead keys + shift here.
    if let Some(text) = &ev.text {
        if !text.is_empty() {
            return Some(text.as_bytes().to_vec());
        }
    }
    None
}

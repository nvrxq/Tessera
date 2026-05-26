//! Position + size of the overlay window in screen coordinates.

/// Rectangle in physical screen pixels, origin top-left. Tauri's JS side
/// reports DPR-aware values via `getBoundingClientRect()` and the bridge
/// converts to physical px (`devicePixelRatio` * logical px) before sending.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Bounds {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// Bounds with at least 1×1 size — wgpu surfaces of zero size panic.
    pub fn nonzero(self) -> Self {
        Self {
            w: self.w.max(1),
            h: self.h.max(1),
            ..self
        }
    }
}

/// Construction-time configuration. Mostly defaults for v1.
#[derive(Clone)]
pub struct OverlayConfig {
    /// Initial bounds. Overlay starts hidden if bounds are zero-area.
    pub initial: Bounds,
    /// Glyph atlas size (px). Default: `tessera_render::DEFAULT_ATLAS_SIZE`.
    pub atlas_size: u32,
    /// Initial visibility. Default: false (don't flash on startup).
    pub visible: bool,
    /// X11 parent window ID — when set the overlay is created as an embedded
    /// child of that window (winit `with_embed_parent_window`). The WM then
    /// treats main + overlay as a single client: focus, fullscreen, minimize,
    /// and i3 workspace moves all apply atomically.
    ///
    /// On non-X11 platforms this is ignored.
    pub parent_window_id: Option<u32>,
    /// Callback invoked when the overlay's own window receives a keypress.
    /// Wired by the host app to forward bytes into the active PTY session,
    /// so users can type while the overlay holds X11 focus (clicks on the
    /// terminal area would otherwise steal focus from the WebView).
    #[allow(clippy::type_complexity)] // signature is the public API
    pub on_key: Option<std::sync::Arc<dyn Fn(uuid::Uuid, Vec<u8>) + Send + Sync>>,
}

impl std::fmt::Debug for OverlayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayConfig")
            .field("initial", &self.initial)
            .field("atlas_size", &self.atlas_size)
            .field("visible", &self.visible)
            .field("parent_window_id", &self.parent_window_id)
            .field("on_key", &self.on_key.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            initial: Bounds::default(),
            atlas_size: tessera_render::DEFAULT_ATLAS_SIZE,
            visible: false,
            parent_window_id: None,
            on_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_promotes_zero_dimensions() {
        let b = Bounds::new(10, 20, 0, 0).nonzero();
        assert_eq!(b.w, 1);
        assert_eq!(b.h, 1);
        assert_eq!(b.x, 10);
        assert_eq!(b.y, 20);
    }

    #[test]
    fn default_config_uses_render_atlas_size() {
        let c = OverlayConfig::default();
        assert_eq!(c.atlas_size, tessera_render::DEFAULT_ATLAS_SIZE);
        assert!(!c.visible);
    }
}

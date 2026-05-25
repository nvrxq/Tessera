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
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// Initial bounds. Overlay starts hidden if bounds are zero-area.
    pub initial: Bounds,
    /// Glyph atlas size (px). Default: `tessera_render::DEFAULT_ATLAS_SIZE`.
    pub atlas_size: u32,
    /// Initial visibility. Default: false (don't flash on startup).
    pub visible: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            initial: Bounds::default(),
            atlas_size: tessera_render::DEFAULT_ATLAS_SIZE,
            visible: false,
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

//! Tessera native overlay window — borderless winit + wgpu surface that
//! visually overlays Tauri's WebView terminal-pane region.
//!
//! See `docs/superpowers/specs/2026-05-24-warp-renderer.md` §8.
//!
//! The overlay runs on its own thread; the Tauri main thread interacts
//! with it via [`Handle`] which sends messages through winit's
//! `EventLoopProxy`.

pub fn crate_name() -> &'static str {
    "tessera-overlay"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_tessera_overlay() {
        assert_eq!(crate_name(), "tessera-overlay");
    }
}

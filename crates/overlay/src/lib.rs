//! Tessera native overlay window — borderless winit + wgpu surface that
//! visually overlays Tauri's WebView terminal-pane region.
//!
//! See `docs/superpowers/specs/2026-05-24-warp-renderer.md` §8.
//!
//! The overlay runs on its own thread; the Tauri main thread interacts
//! with it via [`Handle`] which sends messages through winit's
//! `EventLoopProxy`.

pub mod bounds;
pub use bounds::{Bounds, OverlayConfig};

pub mod messages;

pub mod window;

pub mod handle;
pub use handle::Handle;

pub mod thread;
pub use thread::spawn;

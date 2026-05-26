//! Cross-thread events the controlling thread sends to the overlay's
//! winit event loop.

use crate::bounds::Bounds;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum OverlayMessage {
    SetBounds(Bounds),
    /// Logical→physical pixel scale factor (CSS DPR). Glyphs must rasterize at
    /// `base_px * scale_factor` physical pixels so they read as `base_px` CSS
    /// pixels on screen. Default 1.0 if never set.
    SetScaleFactor(f32),
    /// CSS font size in pixels — drives both the renderer (cell metrics +
    /// glyph rasterization) and the grid (cols/rows recompute). Persisted by
    /// the host UI; default applied at overlay startup.
    SetFontSize(f32),
    SetVisible(bool),
    /// PTY output bytes routed to a session-owned Term.
    FeedBytes {
        session_id: Uuid,
        bytes: Vec<u8>,
    },
    /// Session ended (PTY child exited). Drop the Term for this id.
    ExitSession(Uuid),
    /// Switch which session is rendered. `None` clears the display.
    SelectSession(Option<Uuid>),
    /// Resize the active session's grid in cell units.
    ResizeGrid {
        cols: u16,
        rows: u16,
    },
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_clonable() {
        let id = Uuid::nil();
        for m in [
            OverlayMessage::SetBounds(Bounds::new(0, 0, 100, 100)),
            OverlayMessage::SetScaleFactor(2.0),
            OverlayMessage::SetFontSize(24.0),
            OverlayMessage::SetVisible(true),
            OverlayMessage::FeedBytes {
                session_id: id,
                bytes: vec![1, 2, 3],
            },
            OverlayMessage::ExitSession(id),
            OverlayMessage::SelectSession(Some(id)),
            OverlayMessage::SelectSession(None),
            OverlayMessage::ResizeGrid { cols: 80, rows: 24 },
            OverlayMessage::Shutdown,
        ] {
            let _ = m.clone();
        }
    }
}

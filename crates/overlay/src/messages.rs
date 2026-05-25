//! Cross-thread events the controlling thread sends to the overlay's
//! winit event loop.

use crate::bounds::Bounds;

#[derive(Debug, Clone)]
pub enum OverlayMessage {
    SetBounds(Bounds),
    SetVisible(bool),
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_clonable() {
        let m = OverlayMessage::SetBounds(Bounds::new(0, 0, 100, 100));
        let _ = m.clone();
    }
}

//! Thread-safe handle the controller (Tauri) uses to drive the overlay.

use std::sync::Mutex;
use std::thread::JoinHandle;
use winit::event_loop::EventLoopProxy;

use crate::bounds::Bounds;
use crate::messages::OverlayMessage;

pub struct Handle {
    proxy: EventLoopProxy<OverlayMessage>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Handle {
    pub(crate) fn new(proxy: EventLoopProxy<OverlayMessage>, join: JoinHandle<()>) -> Self {
        Self { proxy, join: Mutex::new(Some(join)) }
    }

    pub fn set_bounds(&self, b: Bounds) {
        let _ = self.proxy.send_event(OverlayMessage::SetBounds(b));
    }

    pub fn set_visible(&self, v: bool) {
        let _ = self.proxy.send_event(OverlayMessage::SetVisible(v));
    }

    pub fn feed_bytes(&self, session_id: uuid::Uuid, bytes: Vec<u8>) {
        let _ = self.proxy.send_event(OverlayMessage::FeedBytes { session_id, bytes });
    }

    pub fn exit_session(&self, session_id: uuid::Uuid) {
        let _ = self.proxy.send_event(OverlayMessage::ExitSession(session_id));
    }

    pub fn select_session(&self, session_id: Option<uuid::Uuid>) {
        let _ = self.proxy.send_event(OverlayMessage::SelectSession(session_id));
    }

    pub fn resize_grid(&self, cols: u16, rows: u16) {
        let _ = self.proxy.send_event(OverlayMessage::ResizeGrid { cols, rows });
    }

    /// Send Shutdown and wait for the event loop to finish. Idempotent.
    pub fn shutdown(&self) {
        let _ = self.proxy.send_event(OverlayMessage::Shutdown);
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // Best-effort send; ignore error if proxy is dead.
        let _ = self.proxy.send_event(OverlayMessage::Shutdown);
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }
}

// Compile-time assertion: Handle must be Send + Sync so it can live in
// Arc<Handle> across Tauri's threads (Plan 4 / Task 7).
const _: fn() = || {
    fn _assert_send_sync<T: Send + Sync>() {}
    _assert_send_sync::<Handle>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_module_compiles() {
        fn _accepts_handle(_h: &Handle) {}
    }
}

//! Spawns the overlay event loop on a dedicated thread and returns a [`Handle`].

#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;

use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::bounds::OverlayConfig;
use crate::handle::Handle;
use crate::messages::OverlayMessage;
use crate::window::OverlayApp;

/// Spawn the overlay on a dedicated OS thread and return a [`Handle`] to it.
///
/// The function blocks just long enough to construct the EventLoop on the
/// child thread (a few ms); the actual event loop runs after this returns.
///
/// **Linux-only assumption:** winit's event loop is OK to run off the main
/// thread on X11/Wayland. On macOS this would fail.
pub fn spawn(config: OverlayConfig) -> Handle {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            tracing::info!(
                "overlay: Wayland session detected; positioning may be best-effort (spec §8.1)"
            );
        }
    }

    let (tx, rx) = std::sync::mpsc::sync_channel::<EventLoopProxy<OverlayMessage>>(0);

    let join = std::thread::Builder::new()
        .name("tessera-overlay".into())
        .spawn(move || {
            let mut builder = EventLoop::<OverlayMessage>::with_user_event();
            // On Linux (X11 and Wayland), permit the event loop to run off the main thread.
            #[cfg(target_os = "linux")]
            builder.with_any_thread(true);
            let el: EventLoop<OverlayMessage> = builder.build().expect("event loop");
            let proxy = el.create_proxy();
            tx.send(proxy).expect("send proxy");
            let mut app = OverlayApp::new(config);
            if let Err(e) = el.run_app(&mut app) {
                tracing::error!(error = %e, "overlay event loop ended with error");
            }
        })
        .expect("spawn overlay thread");

    let proxy = rx.recv().expect("recv proxy");
    Handle::new(proxy, join)
}

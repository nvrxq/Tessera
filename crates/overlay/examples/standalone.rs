//! Manual verification harness for the overlay crate.
//!
//! Spawns the overlay window, positions it at (200, 200) sized 640×320,
//! waits 4 seconds (so a screenshot tool can capture it), then shuts down.
//!
//! Run with: `cargo run -p tessera-overlay --example standalone`

use tessera_overlay::{spawn, Bounds, OverlayConfig};
use std::time::Duration;

fn main() {
    let config = OverlayConfig::default();
    let handle = spawn(config);

    handle.set_bounds(Bounds::new(200, 200, 640, 320));
    handle.set_visible(true);

    println!("overlay visible for 4 seconds — take a screenshot if you want one");
    std::thread::sleep(Duration::from_secs(4));

    handle.shutdown();
}

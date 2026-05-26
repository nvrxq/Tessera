//! Manual verification harness for the overlay crate.
//!
//! Spawns the overlay window, selects a fake session, feeds ANSI bytes
//! exercising the Term-driven render path. Waits 6 seconds (so a
//! screenshot tool can capture it), then shuts down.
//!
//! Run with: `cargo run -p tessera-overlay --example standalone`

use std::time::Duration;
use tessera_overlay::{spawn, Bounds, OverlayConfig};
use uuid::Uuid;

fn main() {
    let config = OverlayConfig::default();
    let handle = spawn(config);

    handle.set_bounds(Bounds::new(200, 200, 640, 320));
    handle.set_visible(true);

    let session = Uuid::new_v4();
    handle.select_session(Some(session));

    // Sample ANSI:
    //  - red "Hello", reset
    //  - then plain text
    //  - newline
    //  - second line
    //  - prompt with cursor block
    let payload: Vec<u8> = b"\x1b[31mHello\x1b[0m world from tessera-overlay\r\n\
        Plan 4 wiring smoke test\r\n\
        Cursor \xe2\x86\x92 "
        .to_vec();
    handle.feed_bytes(session, payload);

    println!("overlay visible for 6 seconds — take a screenshot if you want one");
    std::thread::sleep(Duration::from_secs(6));

    handle.shutdown();
}

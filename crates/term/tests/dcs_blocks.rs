//! End-to-end DCS integration tests — feed full Tessera DCS sequences through
//! `Term::feed()` and assert that `take_block_events()` returns the right events.

use std::io::Cursor;
use tessera_term::{BlockEvent, Term};

fn writer() -> Box<dyn std::io::Write + Send> {
    Box::new(Cursor::new(Vec::new()))
}

#[test]
fn dcs_emits_start_block_event() {
    let mut t = Term::new(80, 24, writer());
    // \eP+tessera;v=1;{"event":"start","id":"b1","command":"ls"}\e\\
    let payload = br#"+tessera;v=1;{"event":"start","id":"b1","command":"ls"}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1bP");
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b\\");

    t.feed(&bytes);
    let events = t.take_block_events();
    assert_eq!(events.len(), 1, "got {events:?}");
    match &events[0] {
        BlockEvent::Start { id, command } => {
            assert_eq!(id.0, "b1");
            assert_eq!(command, "ls");
        }
        _ => panic!("expected Start, got {:?}", events[0]),
    }
}

#[test]
fn dcs_emits_end_block_event() {
    let mut t = Term::new(80, 24, writer());
    let payload = br#"+tessera;v=1;{"event":"end","id":"b1","exit_code":0}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1bP");
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b\\");

    t.feed(&bytes);
    let events = t.take_block_events();
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], BlockEvent::End { exit_code: Some(0), .. }),
        "got {:?}",
        events[0]
    );
}

#[test]
fn non_tessera_dcs_does_not_emit_events() {
    let mut t = Term::new(80, 24, writer());
    // A non-Tessera DCS payload — should be ignored.
    t.feed(b"\x1bP+otherdata\x1b\\");
    assert!(t.take_block_events().is_empty());
}

#[test]
fn second_drain_returns_empty() {
    let mut t = Term::new(80, 24, writer());
    let payload = br#"+tessera;v=1;{"event":"start","id":"b1","command":"ls"}"#;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1bP");
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b\\");
    t.feed(&bytes);
    assert_eq!(t.take_block_events().len(), 1);
    assert_eq!(t.take_block_events().len(), 0);
}

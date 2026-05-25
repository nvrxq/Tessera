//! Block model — captures Tessera DCS sequences emitted by the shell
//! integration script and exposes them as a drainable event stream.

use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockEvent {
    Start {
        id: BlockId,
        /// The command line as captured by the shell hook (best-effort).
        command: String,
    },
    End {
        id: BlockId,
        exit_code: Option<i32>,
    },
}

/// Sink shared between the DCS handler and `Term`. We accumulate decoded
/// events in a Mutex<Vec<...>> so `Term::take_block_events()` can drain.
#[derive(Default)]
pub struct BlockSink {
    inner: Arc<Mutex<Vec<BlockEvent>>>,
}

impl BlockSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn handle(&self) -> Arc<Mutex<Vec<BlockEvent>>> {
        Arc::clone(&self.inner)
    }

    pub fn drain(&self) -> Vec<BlockEvent> {
        let mut g = self.inner.lock().expect("blocks mutex");
        std::mem::take(&mut *g)
    }
}

/// Adapter we hand to wezterm-term as the `DeviceControlHandler`.
///
/// T8 implements only `parse_payload` (string → event) and a stub
/// `handle_device_control` (no byte assembly yet). T10 completes the byte
/// buffering between DCS Enter/Data/Exit events.
pub struct BlockHandler {
    inner: Arc<Mutex<Vec<BlockEvent>>>,
}

impl BlockHandler {
    pub fn new(sink: &BlockSink) -> Self {
        Self {
            inner: sink.handle(),
        }
    }

    /// Parse a DCS payload of the form
    ///   `+tessera;v=1;{"event":"start","id":"...","command":"..."}`
    /// (the leading `\eP` and trailing `\e\\` are stripped by wezterm).
    /// Returns true if it was a Tessera block marker (and pushes the event).
    pub(crate) fn parse_payload(&self, payload: &str) -> bool {
        let rest = match payload.strip_prefix("+tessera;v=1;") {
            Some(r) => r,
            None => return false,
        };
        let v: serde_json::Value = match serde_json::from_str(rest) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let event = v.get("event").and_then(|s| s.as_str()).unwrap_or("");
        let id = v
            .get("id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let ev = match event {
            "start" => BlockEvent::Start {
                id: BlockId(id),
                command: v
                    .get("command")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
            },
            "end" => BlockEvent::End {
                id: BlockId(id),
                exit_code: v
                    .get("exit_code")
                    .and_then(|n| n.as_i64())
                    .map(|n| n as i32),
            },
            _ => return false,
        };
        self.inner.lock().expect("blocks mutex").push(ev);
        true
    }
}

// Trait impl is intentionally minimal — T10 fills the body-assembly path.
// For now we just implement the trait so `Terminal::set_device_control_handler`
// accepts our type.
impl wezterm_term::DeviceControlHandler for BlockHandler {
    fn handle_device_control(
        &mut self,
        _control: wezterm_escape_parser::DeviceControlMode,
    ) {
        // Stub: byte-assembly between Enter/Data/Exit lands in T10.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_start_payload() {
        let sink = BlockSink::new();
        let handler = BlockHandler::new(&sink);
        let ok = handler.parse_payload(
            r#"+tessera;v=1;{"event":"start","id":"abc","command":"ls -la"}"#,
        );
        assert!(ok);
        let events = sink.drain();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BlockEvent::Start { id, command } => {
                assert_eq!(id.0, "abc");
                assert_eq!(command, "ls -la");
            }
            _ => panic!("expected Start"),
        }
    }

    #[test]
    fn parse_end_with_exit_code() {
        let sink = BlockSink::new();
        let handler = BlockHandler::new(&sink);
        let ok = handler
            .parse_payload(r#"+tessera;v=1;{"event":"end","id":"abc","exit_code":0}"#);
        assert!(ok);
        let events = sink.drain();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            BlockEvent::End {
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn non_tessera_payload_returns_false() {
        let sink = BlockSink::new();
        let handler = BlockHandler::new(&sink);
        let ok = handler.parse_payload(r#"+other;v=1;{}"#);
        assert!(!ok);
        assert_eq!(sink.drain().len(), 0);
    }
}

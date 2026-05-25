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
    pub(crate) fn handle(&self) -> Arc<Mutex<Vec<BlockEvent>>> {
        Arc::clone(&self.inner)
    }

    pub fn drain(&self) -> Vec<BlockEvent> {
        let mut g = self.inner.lock().expect("blocks mutex");
        std::mem::take(&mut *g)
    }
}

/// Adapter we hand to wezterm-term as the `DeviceControlHandler`.
///
/// T10: buffers body bytes between DCS Enter/Data/Exit, sniffs the Tessera
/// DCS kind via the `+t` intermediate+final pair, and parses on Exit.
pub(crate) struct BlockHandler {
    inner: Arc<Mutex<Vec<BlockEvent>>>,
    /// Buffered DCS body bytes accumulated between Enter and Exit.
    buf: Vec<u8>,
    /// Whether we're currently inside a Tessera DCS sequence.
    /// Set to true when Enter arrives with intermediates=[b'+'] and byte=b't'.
    in_tessera: bool,
}

impl BlockHandler {
    pub(crate) fn new(sink: &BlockSink) -> Self {
        Self {
            inner: sink.handle(),
            buf: Vec::new(),
            in_tessera: false,
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

impl wezterm_term::DeviceControlHandler for BlockHandler {
    fn handle_device_control(
        &mut self,
        control: wezterm_escape_parser::DeviceControlMode,
    ) {
        use wezterm_escape_parser::DeviceControlMode as M;
        match control {
            // Tessera DCS sequences arrive as ESC P + t <data> ST where
            // `+` is the intermediate byte and `t` is the final byte.
            // wezterm-escape-parser fires Enter with intermediates=[b'+'],
            // byte=b't', then Data for each body byte, then Exit.
            M::Enter(ref e) => {
                self.buf.clear();
                self.in_tessera =
                    e.intermediates == [b'+'] && e.byte == b't';
            }
            M::Data(b) => {
                if self.in_tessera {
                    self.buf.push(b);
                }
            }
            M::Exit => {
                if self.in_tessera {
                    // Reconstruct the full payload that parse_payload expects:
                    // "+tessera;v=1;{...}" — the `+t` prefix was the DCS
                    // intermediate+final, and `essera;v=1;{...}` is in buf.
                    let buf = std::mem::take(&mut self.buf);
                    if let Ok(body) = std::str::from_utf8(&buf) {
                        let payload = format!("+t{body}");
                        self.parse_payload(&payload);
                    }
                }
                self.buf.clear();
                self.in_tessera = false;
            }
            // ShortDeviceControl, TmuxEvents, and any future variants ignored.
            _ => {}
        }
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

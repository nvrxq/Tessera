use crate::event::HookEvent;
use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

pub struct Listener {
    pub socket_path: PathBuf,
    pub rx: mpsc::Receiver<HookEvent>,
}

impl Listener {
    /// Bind a Unix socket at `socket_path` and spawn a tokio task that accepts
    /// connections forever, parsing one JSON line per connection into a
    /// HookEvent and forwarding it on the returned channel.
    pub fn bind(socket_path: &Path) -> Result<Listener> {
        // Remove a stale socket if present from a previous run.
        if socket_path.exists() {
            let _ = std::fs::remove_file(socket_path);
        }
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(socket_path)?;
        let (tx, rx) = mpsc::channel::<HookEvent>(256);

        tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "hook accept failed");
                        continue;
                    }
                };
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if let Err(e) = reader.read_line(&mut line).await {
                        tracing::debug!(error = %e, "hook read failed");
                        return;
                    }
                    match serde_json::from_str::<HookEvent>(line.trim()) {
                        Ok(evt) => {
                            let _ = tx.send(evt).await;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, raw = %line.trim(), "bad hook event");
                        }
                    }
                });
            }
        });

        Ok(Listener {
            socket_path: socket_path.to_path_buf(),
            rx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::send_event;
    use crate::event::{HookEvent, HookKind};
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn round_trip_event() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("hooks.sock");
        let mut listener = Listener::bind(&sock).unwrap();
        let ws_id = Uuid::new_v4();

        let sock_for_client = sock.clone();
        std::thread::spawn(move || {
            send_event(
                &sock_for_client,
                &HookEvent {
                    workspace_id: ws_id,
                    kind: HookKind::Stop,
                    payload: serde_json::json!({"hi": 1}),
                },
            )
            .unwrap();
        });

        let evt = tokio::time::timeout(Duration::from_secs(2), listener.rx.recv())
            .await
            .expect("listener timed out")
            .expect("channel closed");
        assert_eq!(evt.workspace_id, ws_id);
        assert_eq!(evt.kind, HookKind::Stop);
        assert_eq!(evt.payload["hi"], 1);
    }

    #[tokio::test]
    async fn missing_socket_is_silent_success() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("nope.sock");
        // No listener bound. Client should not error.
        let result = send_event(
            &sock,
            &HookEvent {
                workspace_id: Uuid::nil(),
                kind: HookKind::Stop,
                payload: serde_json::Value::Null,
            },
        );
        assert!(result.is_ok());
    }
}

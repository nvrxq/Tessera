use crate::session::{PtySession, SessionConfig};
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum PtyEvent {
    Data { session_id: Uuid, bytes: Vec<u8> },
    Exit { session_id: Uuid },
}

pub struct Supervisor {
    sessions: Arc<Mutex<HashMap<Uuid, PtySession>>>,
    tx: broadcast::Sender<PtyEvent>,
}

impl Supervisor {
    pub fn new() -> Self {
        // Capacity is shared across ALL sessions. When claude floods output
        // (large file dumps, alt-screen redraws) faster than the single pump
        // task drains, a too-small buffer overflows and `recv` returns
        // `Lagged`, silently DROPPING parser bytes — which desyncs the grid
        // and shows as on-screen artifacts. 8192 absorbs multi-MB bursts;
        // the buffer sits near-empty in steady state so the memory cost is
        // only paid under backpressure.
        let (tx, _) = broadcast::channel::<PtyEvent>(8192);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PtyEvent> {
        self.tx.subscribe()
    }

    pub fn spawn(&self, cfg: SessionConfig) -> Result<Uuid> {
        let session_id = Uuid::new_v4();
        let (session, rx) = PtySession::spawn(cfg)?;
        self.sessions.lock().unwrap().insert(session_id, session);

        let tx = self.tx.clone();
        let sessions = Arc::clone(&self.sessions);
        thread::spawn(move || {
            // Drain `rx` until the reader thread inside PtySession drops its sender.
            for bytes in rx {
                let _ = tx.send(PtyEvent::Data { session_id, bytes });
            }
            // Channel closed — child has exited or been killed.
            sessions.lock().unwrap().remove(&session_id);
            let _ = tx.send(PtyEvent::Exit { session_id });
        });

        Ok(session_id)
    }

    /// Is this session still backed by a live PTY? The drain thread removes
    /// the session from the map the instant the child exits (see `spawn`), so
    /// a present key is a reliable liveness signal. Used by the workspace
    /// service to make spawning idempotent.
    pub fn is_alive(&self, session_id: Uuid) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&session_id)
    }

    pub fn write(&self, session_id: Uuid, bytes: &[u8]) -> Result<()> {
        // Look up the session, clone out its writer handle, then DROP the
        // sessions-map lock before doing the (potentially blocking)
        // write+flush. Any other session is now free to write concurrently —
        // before this split, a slow PTY pipe on one session would stall the
        // entire app's input dispatch.
        let writer = {
            let map = self.sessions.lock().unwrap();
            let s = map
                .get(&session_id)
                .ok_or_else(|| anyhow::anyhow!("unknown session {session_id}"))?;
            s.writer_handle()
        };
        let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, session_id: Uuid, cols: u16, rows: u16) -> Result<()> {
        let mut map = self.sessions.lock().unwrap();
        let s = map
            .get_mut(&session_id)
            .ok_or_else(|| anyhow::anyhow!("unknown session {session_id}"))?;
        s.resize(cols, rows)
    }

    pub fn kill(&self, session_id: Uuid) -> Result<()> {
        let removed = self.sessions.lock().unwrap().remove(&session_id);
        if let Some(mut s) = removed {
            s.kill()?;
        }
        Ok(())
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionConfig;
    use tokio::time::{timeout, Duration as TokioDuration};

    fn cfg(program: &str, args: &[&str]) -> SessionConfig {
        SessionConfig {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: Vec::new(),
        }
    }

    #[tokio::test]
    async fn spawn_emits_data_event() {
        let sup = Supervisor::new();
        let mut rx = sup.subscribe();
        let _id = sup.spawn(cfg("echo", &["sup-hello"])).unwrap();

        let deadline = TokioDuration::from_secs(3);
        let mut got = Vec::new();
        let result = timeout(deadline, async {
            while let Ok(evt) = rx.recv().await {
                if let PtyEvent::Data { bytes, .. } = evt {
                    got.extend_from_slice(&bytes);
                    if std::str::from_utf8(&got).is_ok_and(|s| s.contains("sup-hello")) {
                        return;
                    }
                }
            }
        })
        .await;
        assert!(
            result.is_ok(),
            "did not see echo output: {:?}",
            String::from_utf8_lossy(&got)
        );
    }

    #[tokio::test]
    async fn kill_removes_session() {
        let sup = Supervisor::new();
        let id = sup.spawn(cfg("sleep", &["30"])).unwrap();
        sup.kill(id).unwrap();
        // Writing after kill should fail because the session was removed.
        assert!(sup.write(id, b"x").is_err());
    }
}

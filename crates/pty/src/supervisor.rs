use crate::session::{PtySession, SessionConfig};
use anyhow::Result;
use std::collections::HashMap;
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
        let (tx, _) = broadcast::channel::<PtyEvent>(1024);
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

    pub fn write(&self, session_id: Uuid, bytes: &[u8]) -> Result<()> {
        let mut map = self.sessions.lock().unwrap();
        let s = map
            .get_mut(&session_id)
            .ok_or_else(|| anyhow::anyhow!("unknown session {session_id}"))?;
        s.write(bytes)
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

use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

pub struct SessionConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    /// Extra environment variables to layer on top of inherited ones. Used by
    /// the shell-integration path to set ZDOTDIR (zsh) and similar.
    pub env: Vec<(String, String)>,
}

/// Owns the writer / resizer / killer for a PTY child. The output stream is
/// returned separately as an `mpsc::Receiver<Vec<u8>>` from `spawn`, so the
/// reader can be consumed without holding any lock on the session.
///
/// `writer` lives behind an `Arc<Mutex<...>>` so `Supervisor::write` can
/// clone the handle out from under the sessions-map lock, drop the outer
/// lock, and only then take the per-writer lock to do the (potentially
/// slow) `write_all + flush`. Other sessions then write concurrently.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child_killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    reader_thread: Option<JoinHandle<()>>,
}

impl PtySession {
    pub fn spawn(cfg: SessionConfig) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let sys = native_pty_system();
        let pair = sys.openpty(PtySize {
            cols: cfg.cols,
            rows: cfg.rows,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&cfg.program);
        for a in &cfg.args {
            cmd.arg(a);
        }
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }
        cmd.cwd(&cfg.cwd);

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let child_killer = child.clone_killer();
        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "pty reader stopped");
                        break;
                    }
                }
            }
            // child is owned by this thread so it gets dropped here
            drop(child);
        });

        Ok((
            PtySession {
                master: pair.master,
                child_killer,
                writer: Arc::new(Mutex::new(writer)),
                reader_thread: Some(handle),
            },
            rx,
        ))
    }

    /// Clone the shared writer handle. `Supervisor::write` uses this so it
    /// can release the global sessions-map mutex BEFORE doing the actual
    /// (potentially blocking) write+flush.
    pub fn writer_handle(&self) -> Arc<Mutex<Box<dyn std::io::Write + Send>>> {
        Arc::clone(&self.writer)
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn kill(&mut self) -> Result<()> {
        self.child_killer.kill()?;
        Ok(())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child_killer.kill();
        if let Some(h) = self.reader_thread.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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

    #[test]
    fn echo_emits_output() {
        let (_session, rx) = PtySession::spawn(cfg("echo", &["hello-tessera"])).unwrap();
        let mut buf = Vec::new();
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(chunk) => {
                    buf.extend_from_slice(&chunk);
                    if std::str::from_utf8(&buf).is_ok_and(|s| s.contains("hello-tessera")) {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        panic!(
            "did not see expected output, got: {:?}",
            String::from_utf8_lossy(&buf)
        );
    }

    #[test]
    fn kill_terminates_long_running_process() {
        let (mut session, rx) = PtySession::spawn(cfg("sleep", &["30"])).unwrap();
        session.kill().unwrap();
        // After kill, the reader thread should drain the child and the channel
        // should disconnect within a couple of seconds.
        let start = std::time::Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                _ if start.elapsed() > Duration::from_secs(3) => {
                    panic!("rx did not disconnect after kill")
                }
                _ => continue,
            }
        }
    }
}

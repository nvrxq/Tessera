use crate::event::HookEvent;
use anyhow::Result;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Send a single event to the GUI listener. Each call is one connection: open,
/// write a single line of JSON terminated by `\n`, close. If the socket
/// doesn't exist (GUI not running) returns Ok silently — the Claude Code
/// hook command must never break the user's session.
pub fn send_event(socket_path: &Path, event: &HookEvent) -> Result<()> {
    if !socket_path.exists() {
        tracing::debug!(path = %socket_path.display(), "hook socket missing — dropping event");
        return Ok(());
    }
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "hook socket connect failed");
            return Ok(());
        }
    };
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;
    Ok(())
}

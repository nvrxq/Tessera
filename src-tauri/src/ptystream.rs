//! Raw-PTY-byte streaming to the frontend's xterm.js renderer.
//!
//! Replaces the old wezterm-term snapshot/delta pipeline (`terminal.rs`). With
//! xterm.js the frontend owns VT parsing + rendering, so the backend's only job
//! is to forward each session's raw PTY output to an attached IPC `Channel`.
//!
//! A small per-session replay ring buffers output so a late-attaching frontend
//! still gets the screen: Tessera spawns the PTY (with the workspace's
//! `--session-id` identity) BEFORE the `<Terminal>` mounts and calls
//! `terminal_attach`, so the first bytes (claude's welcome screen) would
//! otherwise be lost. On attach we replay the ring, then stream live.
//!
//! The byte-Channel streaming approach is copied from Terax
//! (github.com/crynta/terax-ai, Apache-2.0) — see NOTICE.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tauri::ipc::{Channel, Response};
use uuid::Uuid;

/// Cap on the per-session replay ring. Big enough to hold a TUI's initial
/// paint plus a screenful of scrollback; bounded so a chatty session can't
/// grow it without limit.
const RING_CAP: usize = 512 * 1024;

struct SessionStream {
    ring: VecDeque<u8>,
    channel: Option<Channel<Response>>,
}

impl SessionStream {
    fn new() -> Self {
        Self {
            ring: VecDeque::new(),
            channel: None,
        }
    }

    /// Append `bytes` to the replay ring, dropping the oldest data to stay at
    /// or under `RING_CAP`.
    fn push_ring(&mut self, bytes: &[u8]) {
        if bytes.len() >= RING_CAP {
            // The new chunk alone fills (or exceeds) the ring — keep only its tail.
            self.ring.clear();
            self.ring.extend(&bytes[bytes.len() - RING_CAP..]);
            return;
        }
        let overflow = (self.ring.len() + bytes.len()).saturating_sub(RING_CAP);
        for _ in 0..overflow {
            self.ring.pop_front();
        }
        self.ring.extend(bytes);
    }
}

pub struct PtyStreamRegistry {
    inner: Mutex<HashMap<Uuid, SessionStream>>,
}

impl Default for PtyStreamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyStreamRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Forward a chunk of PTY output: buffer it for replay and, if a frontend
    /// channel is attached, send it live. Called from the PTY pump task.
    pub fn feed(&self, sid: Uuid, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.entry(sid).or_insert_with(SessionStream::new);
        st.push_ring(bytes);
        if let Some(ch) = &st.channel {
            // A send failure means the webview/channel went away; the frontend
            // will re-attach (and replay) on next mount, so just drop it.
            let _ = ch.send(Response::new(bytes.to_vec()));
        }
    }

    /// Bind a frontend channel to a session: replay buffered history first
    /// (so xterm rebuilds the current screen), then stream live via `feed`.
    pub fn attach(&self, sid: Uuid, channel: Channel<Response>) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.entry(sid).or_insert_with(SessionStream::new);
        if !st.ring.is_empty() {
            let history: Vec<u8> = st.ring.iter().copied().collect();
            let _ = channel.send(Response::new(history));
        }
        st.channel = Some(channel);
    }

    /// Forget a session (PTY exited or workspace deleted).
    pub fn remove(&self, sid: Uuid) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&sid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_caps_and_keeps_tail() {
        let mut s = SessionStream::new();
        s.push_ring(&[1, 2, 3]);
        assert_eq!(s.ring.len(), 3);
        // A chunk larger than the cap keeps only its tail.
        let big = vec![7u8; RING_CAP + 100];
        s.push_ring(&big);
        assert_eq!(s.ring.len(), RING_CAP);
        assert_eq!(*s.ring.back().unwrap(), 7);
    }

    #[test]
    fn ring_drops_oldest_on_overflow() {
        let mut s = SessionStream::new();
        s.push_ring(&vec![1u8; RING_CAP - 2]);
        s.push_ring(&[2, 2, 2, 2]); // pushes 2 past the cap
        assert_eq!(s.ring.len(), RING_CAP);
        // Oldest two bytes dropped; tail is the new data.
        assert_eq!(*s.ring.back().unwrap(), 2);
    }

    #[test]
    fn feed_and_remove_are_isolated_per_session() {
        let reg = PtyStreamRegistry::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        reg.feed(a, b"hello");
        reg.feed(b, b"world");
        {
            let map = reg.inner.lock().unwrap();
            assert_eq!(map.len(), 2);
        }
        reg.remove(a);
        {
            let map = reg.inner.lock().unwrap();
            assert_eq!(map.len(), 1);
            assert!(map.contains_key(&b));
        }
    }
}

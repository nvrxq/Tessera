//! PTY supervisor for Tessera: owns N sessions, streams output via broadcast.

pub mod session;

pub use session::{PtySession, SessionConfig};

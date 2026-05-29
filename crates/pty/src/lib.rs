//! PTY supervisor for Tessera: owns N sessions, streams output via broadcast.

pub mod da_filter;
pub mod session;
pub mod supervisor;

pub use session::{PtySession, SessionConfig};
pub use supervisor::{PtyEvent, Supervisor};

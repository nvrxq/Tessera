//! Unix-socket pipeline between the `tessera hook` subcommand and the GUI process.

pub mod client;
pub mod event;
pub mod listener;
pub mod worktree_parse;

pub use client::send_event;
pub use event::{HookEvent, HookKind};
pub use listener::Listener;
pub use worktree_parse::{parse_worktree_add, WorktreeAdd};

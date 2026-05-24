//! Unix-socket pipeline between the `tessera hook` subcommand and the GUI process.

pub mod event;

pub use event::{HookEvent, HookKind};

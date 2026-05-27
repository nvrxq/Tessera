//! Domain types shared across the app.

pub mod config;
pub mod error;
pub mod session;
pub mod workspace;

pub use config::{
    config_path, AppearanceConfig, BehaviorConfig, CursorShape, Density, HexColor, TerminalConfig,
    UserConfig, SCHEMA_VERSION,
};
pub use error::CoreError;
pub use session::{AgentSession, AgentStatus};
pub use workspace::{Project, SetupStatus, Workspace};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn workspace_round_trips_json() {
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "feat/login".into(),
            repo_path: PathBuf::from("/tmp/repo"),
            worktree_path: PathBuf::from("/tmp/wt"),
            branch: "feat/login".into(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Pending,
            detected_worktree: None,
            detected_branch: None,
            dangerous_skip_permissions: false,
            has_prior_session: false,
            project_id: None,
            sort_order: 0,
        };
        let j = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, ws.id);
        assert_eq!(back.branch, ws.branch);
    }

    #[test]
    fn setup_status_failed_carries_message() {
        let s = SetupStatus::Failed {
            stderr_tail: "boom\n".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("boom"));
    }

    #[test]
    fn agent_status_serializes_as_tagged() {
        let s = AgentStatus::NeedsInput;
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"needs_input\"");
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupStatus {
    Pending,
    Running,
    Ok,
    Failed { stderr_tail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub created_at: DateTime<Utc>,
    pub setup_status: SetupStatus,
    pub detected_worktree: Option<PathBuf>,
    pub detected_branch: Option<String>,
    pub dangerous_skip_permissions: bool,
    /// True once Claude has been launched in this workspace at least once.
    /// On subsequent launches we pass `--continue` so the conversation resumes.
    pub has_prior_session: bool,
    /// Optional project this workspace belongs to. NULL when ungrouped, and
    /// set NULL by the FK if the parent project is deleted.
    pub project_id: Option<Uuid>,
    /// User-controlled ordering value. List queries sort ascending by this
    /// field, falling back to `created_at` for ties so newly inserted rows
    /// are deterministic.
    pub sort_order: i64,
}

/// A user-defined grouping for workspaces. Workspaces hold a nullable
/// `project_id`; `ON DELETE SET NULL` keeps a workspace alive when its
/// project is removed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    /// Hex color string `"#RRGGBB"` — optional accent shown in the sidebar.
    pub accent: Option<String>,
    pub created_at: DateTime<Utc>,
}

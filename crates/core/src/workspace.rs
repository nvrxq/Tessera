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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    /// Legacy field — always empty in the Plan-5 worktree-less model.
    /// Kept on the struct only because the DB schema still has the column
    /// (and SQLite column drops are painful). Don't read this anywhere new.
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
    /// Claude Code session uuid (the `.jsonl` stem under
    /// `~/.claude/projects/<encoded-cwd>/`). NULL until the first spawn
    /// finishes and we detect the freshest jsonl for the cwd. When set, we
    /// spawn claude with `--resume <id>` instead of `--continue`, which
    /// keeps two workspaces sharing a folder pinned to their own
    /// conversations.
    pub claude_session_id: Option<String>,
    /// Soft-archive timestamp. `None` means active; `Some` means the row
    /// is hidden from the main list but everything (including the pinned
    /// Claude session) stays intact so the user can restore it.
    pub archived_at: Option<DateTime<Utc>>,
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

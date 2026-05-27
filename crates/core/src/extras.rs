//! Per-workspace productivity extras: links, daily tasks, pomodoro timer.
//!
//! These types are persisted by `tessera-store::extras` and exposed to the
//! frontend through `src-tauri/src/commands.rs`. All JSON uses snake_case to
//! match the rest of the workspace surface.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::str::FromStr;
use uuid::Uuid;

/// What kind of URL `WorkspaceLink::url` points at. `Url` is the default; the
/// GitHub variants are set when `workspace_links_add` detects an issue/PR
/// URL so the frontend can render the right icon and a derived
/// `owner/repo#N` label.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Url,
    GithubIssue,
    GithubPr,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Url => "url",
            LinkKind::GithubIssue => "github_issue",
            LinkKind::GithubPr => "github_pr",
        }
    }
}

impl FromStr for LinkKind {
    type Err = Infallible;

    /// Total parse — any unknown string maps to `LinkKind::Url`. The store
    /// only ever writes the three canonical strings, so the catch-all is
    /// just defensive against hand-edited rows.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "github_issue" => LinkKind::GithubIssue,
            "github_pr" => LinkKind::GithubPr,
            _ => LinkKind::Url,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLink {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// Free-text label. For GitHub issues/PRs we auto-fill `owner/repo#N`
    /// at insert time if the caller didn't supply one.
    pub label: Option<String>,
    pub url: String,
    pub kind: LinkKind,
    pub created_at: DateTime<Utc>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTask {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub done: bool,
    pub sort_order: i64,
    /// ISO `YYYY-MM-DD` date or NULL. Kept as a `String` because we don't
    /// need timezone semantics — the user picks a calendar day.
    pub due_date: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Lifecycle of a pomodoro run.
///
/// `Idle` is the resting state — no timer is counting. `Work` and `Break`
/// each have a `started_at` timestamp and a `target_seconds` duration;
/// the frontend renders the live countdown locally. `Paused` freezes the
/// remaining time via `elapsed_seconds_before_pause`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PomodoroMode {
    Idle,
    Work,
    Break,
    Paused,
}

impl PomodoroMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PomodoroMode::Idle => "idle",
            PomodoroMode::Work => "work",
            PomodoroMode::Break => "break",
            PomodoroMode::Paused => "paused",
        }
    }
}

impl FromStr for PomodoroMode {
    type Err = Infallible;

    /// Total parse — unknown strings map to `PomodoroMode::Idle`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "work" => PomodoroMode::Work,
            "break" => PomodoroMode::Break,
            "paused" => PomodoroMode::Paused,
            _ => PomodoroMode::Idle,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PomodoroState {
    pub workspace_id: Uuid,
    pub mode: PomodoroMode,
    pub started_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub target_seconds: i64,
    pub elapsed_seconds_before_pause: i64,
    pub cycles_completed: i64,
    pub updated_at: DateTime<Utc>,
}

impl PomodoroState {
    /// Fresh idle row for a brand-new workspace_id (used when the frontend
    /// asks for state before anything has been started).
    pub fn idle(workspace_id: Uuid) -> Self {
        Self {
            workspace_id,
            mode: PomodoroMode::Idle,
            started_at: None,
            paused_at: None,
            target_seconds: 1500,
            elapsed_seconds_before_pause: 0,
            cycles_completed: 0,
            updated_at: Utc::now(),
        }
    }
}

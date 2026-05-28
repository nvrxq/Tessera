//! Activity log entries — one row per hook event received from Claude.
//! Read by the workspace's "Activity" tab so the user can see what the
//! agent has been doing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    PostToolUse,
    Stop,
    Notification,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityKind::PostToolUse => "post_tool_use",
            ActivityKind::Stop => "stop",
            ActivityKind::Notification => "notification",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "post_tool_use" => Some(ActivityKind::PostToolUse),
            "stop" => Some(ActivityKind::Stop),
            "notification" => Some(ActivityKind::Notification),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: ActivityKind,
    pub summary: String,
    pub payload: String,
    pub created_at: DateTime<Utc>,
}

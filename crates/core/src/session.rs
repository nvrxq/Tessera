use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsInput,
    Done,
    Crashed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub pty_pid: Option<u32>,
    pub status: AgentStatus,
    pub started_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
}

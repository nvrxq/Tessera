use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Stop,
    Notification,
    PostToolUse,
}

impl HookKind {
    pub fn from_cli(s: &str) -> Option<HookKind> {
        match s {
            "stop" => Some(HookKind::Stop),
            "notify" | "notification" => Some(HookKind::Notification),
            "activity" | "post-tool-use" | "posttooluse" => Some(HookKind::PostToolUse),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEvent {
    pub workspace_id: Uuid,
    pub kind: HookKind,
    /// The raw JSON Claude wrote on stdin — kept verbatim so we don't have to
    /// chase changes in Claude's hook payload schema.
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips_snake_case() {
        assert_eq!(serde_json::to_string(&HookKind::Stop).unwrap(), "\"stop\"");
        assert_eq!(
            serde_json::from_str::<HookKind>("\"post_tool_use\"").unwrap(),
            HookKind::PostToolUse
        );
    }

    #[test]
    fn from_cli_accepts_short_names() {
        assert_eq!(HookKind::from_cli("stop"), Some(HookKind::Stop));
        assert_eq!(HookKind::from_cli("notify"), Some(HookKind::Notification));
        assert_eq!(HookKind::from_cli("activity"), Some(HookKind::PostToolUse));
        assert_eq!(HookKind::from_cli("bogus"), None);
    }

    #[test]
    fn event_serialises_with_payload() {
        let evt = HookEvent {
            workspace_id: Uuid::nil(),
            kind: HookKind::Stop,
            payload: serde_json::json!({"session_id": "abc"}),
        };
        let s = serde_json::to_string(&evt).unwrap();
        let back: HookEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(back.kind, HookKind::Stop);
        assert_eq!(back.payload["session_id"], "abc");
    }
}

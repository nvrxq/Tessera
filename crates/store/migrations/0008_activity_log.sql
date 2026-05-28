CREATE TABLE activity_log (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,         -- 'post_tool_use' | 'stop' | 'notification'
    summary      TEXT NOT NULL,
    payload      TEXT NOT NULL,         -- raw JSON Claude sent (truncated upstream)
    created_at   TEXT NOT NULL
);

CREATE INDEX activity_log_ws_ts ON activity_log(workspace_id, created_at DESC);

CREATE TABLE workspaces (
    id            TEXT PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    repo_path     TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    branch        TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    setup_status  TEXT NOT NULL  -- JSON-encoded SetupStatus
);

CREATE TABLE agent_sessions (
    id             TEXT PRIMARY KEY NOT NULL,
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    pty_pid        INTEGER,
    status         TEXT NOT NULL,
    started_at     TEXT NOT NULL,
    last_event_at  TEXT NOT NULL
);

CREATE INDEX idx_sessions_by_workspace ON agent_sessions(workspace_id);

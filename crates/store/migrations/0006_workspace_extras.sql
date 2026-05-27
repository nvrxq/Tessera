CREATE TABLE workspace_links (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    label        TEXT,
    url          TEXT NOT NULL,
    kind         TEXT NOT NULL DEFAULT 'url',           -- 'url' | 'github_issue' | 'github_pr'
    created_at   TEXT NOT NULL,
    sort_order   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_workspace_links_ws ON workspace_links(workspace_id);

CREATE TABLE workspace_tasks (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    done         INTEGER NOT NULL DEFAULT 0,
    sort_order   INTEGER NOT NULL DEFAULT 0,
    due_date     TEXT,                                  -- ISO date or NULL
    created_at   TEXT NOT NULL,
    completed_at TEXT
);
CREATE INDEX idx_workspace_tasks_ws ON workspace_tasks(workspace_id);

CREATE TABLE workspace_pomodoro (
    workspace_id              TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    mode                      TEXT NOT NULL DEFAULT 'idle',  -- 'idle' | 'work' | 'break' | 'paused'
    started_at                TEXT,                          -- ISO timestamp when current run started
    paused_at                 TEXT,                          -- ISO timestamp when paused
    target_seconds            INTEGER NOT NULL DEFAULT 1500, -- 25min
    elapsed_seconds_before_pause INTEGER NOT NULL DEFAULT 0,
    cycles_completed          INTEGER NOT NULL DEFAULT 0,
    updated_at                TEXT NOT NULL
);

CREATE TABLE projects (
    id          TEXT PRIMARY KEY,    -- UUID v4
    name        TEXT NOT NULL,
    accent      TEXT,                -- nullable hex color "#RRGGBB"
    created_at  TEXT NOT NULL        -- RFC3339
);

ALTER TABLE workspaces ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE workspaces ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- Backfill sort_order using rowid order so existing rows have stable initial ordering.
UPDATE workspaces SET sort_order = (SELECT COUNT(*) FROM workspaces w2 WHERE w2.rowid < workspaces.rowid) * 1000;

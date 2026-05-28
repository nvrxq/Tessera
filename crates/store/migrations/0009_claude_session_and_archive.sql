-- Pin each workspace to a specific Claude Code session, and add a
-- soft-archive flag.
--
-- `claude_session_id`: the uuid of a `.jsonl` under
--   `~/.claude/projects/<encoded-cwd>/`. When set, we spawn claude with
--   `--resume <id>` so two workspaces sharing the same folder don't both
--   resume the same conversation via `--continue`. NULL until the first
--   spawn finishes and we detect the freshest session for the cwd.
--
-- `archived_at`: ISO-8601 timestamp. Non-NULL means the row is hidden
--   from the active list but its data (including `claude_session_id`)
--   stays intact so the user can restore it.
ALTER TABLE workspaces ADD COLUMN claude_session_id TEXT;
ALTER TABLE workspaces ADD COLUMN archived_at TEXT;

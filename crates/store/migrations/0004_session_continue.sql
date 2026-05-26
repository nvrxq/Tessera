-- Track whether claude has been launched at least once for this workspace.
-- On subsequent launches we pass `--continue` so Claude Code resumes the
-- saved conversation from disk (`~/.claude/projects/...`).
ALTER TABLE workspaces ADD COLUMN has_prior_session INTEGER NOT NULL DEFAULT 0;

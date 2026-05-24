ALTER TABLE workspaces ADD COLUMN task_prompt TEXT NOT NULL DEFAULT '';
ALTER TABLE workspaces ADD COLUMN detected_worktree TEXT;
ALTER TABLE workspaces ADD COLUMN detected_branch TEXT;

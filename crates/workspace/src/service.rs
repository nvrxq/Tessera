use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tessera_core::Workspace;
use tessera_pty::Supervisor;
use uuid::Uuid;

pub struct WorkspaceService {
    db: Arc<Mutex<Connection>>,
    pub(crate) supervisor: Arc<Supervisor>,
    #[allow(dead_code)]
    worktree_root: PathBuf,
    /// workspace_id -> live pty session_id (transient, cleared on app restart)
    sessions: Mutex<HashMap<Uuid, Uuid>>,
    /// workspace_id -> most recent AgentStatus (transient)
    statuses: Mutex<HashMap<Uuid, tessera_core::AgentStatus>>,
}

impl WorkspaceService {
    pub fn new(
        db: Arc<Mutex<Connection>>,
        supervisor: Arc<Supervisor>,
        worktree_root: PathBuf,
    ) -> Self {
        Self {
            db,
            supervisor,
            worktree_root,
            sessions: Mutex::new(HashMap::new()),
            statuses: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Result<Vec<Workspace>> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::list(&conn)
    }

    pub fn current_session(&self, workspace_id: Uuid) -> Option<Uuid> {
        self.sessions.lock().unwrap().get(&workspace_id).copied()
    }

    /// Create a workspace.
    ///
    /// Tessera no longer pre-creates a git worktree. `folder_path` is the
    /// folder Claude is launched in. If Claude later runs `git worktree add`,
    /// the hook listener will detect it and update `detected_worktree`.
    pub fn create(
        &self,
        folder_path: &std::path::Path,
        name: &str,
        task_prompt: &str,
    ) -> Result<Workspace> {
        use chrono::Utc;
        use tessera_core::SetupStatus;

        anyhow::ensure!(!name.trim().is_empty(), "workspace name is required");

        let id = Uuid::new_v4();
        let ws = Workspace {
            id,
            name: name.to_string(),
            repo_path: folder_path.to_path_buf(),
            worktree_path: folder_path.to_path_buf(),
            branch: String::new(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
            task_prompt: task_prompt.to_string(),
            detected_worktree: None,
            detected_branch: None,
        };

        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::insert(&conn, &ws)?;

        tracing::info!(id = %ws.id, name = %ws.name, "workspace created");
        Ok(ws)
    }

    pub fn delete(&self, workspace_id: Uuid, _force: bool) -> Result<()> {
        let _ = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };

        // Kill any live session for this workspace and forget its status.
        if let Some(session_id) = self.sessions.lock().unwrap().remove(&workspace_id) {
            let _ = self.supervisor.kill(session_id);
        }
        self.statuses.lock().unwrap().remove(&workspace_id);

        // Do NOT touch the on-disk folder. If Claude created a worktree, the
        // user can clean it up via the shell — Tessera is not in the worktree
        // business in Plan 5.
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::delete(&conn, workspace_id)?;
        Ok(())
    }

    /// Spawn the workspace's agent. Defaults to `claude`. The task_prompt on
    /// the workspace row (if any) is piped into the PTY shortly after spawn.
    pub fn spawn_agent(&self, workspace_id: Uuid, cols: u16, rows: u16) -> Result<Uuid> {
        self.spawn_agent_with_program(workspace_id, "claude", &[], cols, rows)
    }

    /// Like `spawn_agent` but lets callers pick the program — used by tests to
    /// substitute `cat` for `claude`.
    pub fn spawn_agent_with_program(
        &self,
        workspace_id: Uuid,
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
    ) -> Result<Uuid> {
        use tessera_pty::session::SessionConfig;

        let workspace = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };

        let cfg = SessionConfig {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: workspace.worktree_path.clone(),
            cols,
            rows,
        };
        let session_id = self.supervisor.spawn(cfg)?;
        self.sessions
            .lock()
            .unwrap()
            .insert(workspace_id, session_id);

        // If the workspace has a task prompt, write it to the PTY a short
        // moment after spawn so the agent's REPL has time to come up.
        if !workspace.task_prompt.is_empty() {
            let supervisor = Arc::clone(&self.supervisor);
            let prompt = workspace.task_prompt.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                let mut bytes = prompt.into_bytes();
                bytes.push(b'\n');
                let _ = supervisor.write(session_id, &bytes);
            });
        }

        Ok(session_id)
    }

    pub fn set_status(&self, workspace_id: Uuid, status: tessera_core::AgentStatus) {
        self.statuses.lock().unwrap().insert(workspace_id, status);
    }

    pub fn status_of(&self, workspace_id: Uuid) -> Option<tessera_core::AgentStatus> {
        self.statuses.lock().unwrap().get(&workspace_id).copied()
    }

    pub fn set_detected_worktree(
        &self,
        workspace_id: Uuid,
        worktree: Option<std::path::PathBuf>,
        branch: Option<String>,
    ) -> Result<()> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_detected_worktree(
            &conn,
            workspace_id,
            worktree.as_deref(),
            branch.as_deref(),
        )
    }

    /// Write `<folder>/.claude/settings.local.json` so Claude Code calls back
    /// into our binary when its lifecycle hooks fire. In Plan 5 this is the
    /// user's source folder (== workspace.worktree_path), not a worktree we
    /// created.
    pub fn install_hooks(
        &self,
        worktree_path: &std::path::Path,
        workspace_id: Uuid,
        tessera_exe: &std::path::Path,
    ) -> Result<()> {
        let dir = worktree_path.join(".claude");
        std::fs::create_dir_all(&dir)?;
        let exe = tessera_exe.display().to_string();
        let id = workspace_id;
        let cmd = |kind: &str| format!("{exe} hook {id} {kind}");
        let config = serde_json::json!({
            "hooks": {
                "Stop": [{
                    "matcher": "",
                    "hooks": [{ "type": "command", "command": cmd("stop") }]
                }],
                "Notification": [{
                    "matcher": "",
                    "hooks": [{ "type": "command", "command": cmd("notify") }]
                }],
                "PostToolUse": [{
                    "matcher": "",
                    "hooks": [{ "type": "command", "command": cmd("activity") }]
                }]
            }
        });
        let path = dir.join("settings.local.json");
        std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
        tracing::info!(path = %path.display(), "wrote claude hooks");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_pty::Supervisor;

    fn make_service() -> (WorkspaceService, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = tessera_store::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        let supervisor = Arc::new(Supervisor::new());
        let svc = WorkspaceService::new(db, supervisor, dir.path().join("worktrees"));
        (svc, dir)
    }

    #[test]
    fn list_empty_db_returns_empty() {
        let (svc, _dir) = make_service();
        assert!(svc.list().unwrap().is_empty());
    }

    #[test]
    fn current_session_returns_none_for_unknown() {
        let (svc, _dir) = make_service();
        assert!(svc.current_session(Uuid::new_v4()).is_none());
    }

    #[test]
    fn create_inserts_row_with_task_prompt() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "Login work", "implement OAuth").unwrap();
        assert_eq!(ws.name, "Login work");
        assert_eq!(ws.task_prompt, "implement OAuth");
        assert_eq!(ws.worktree_path, folder);
        assert_eq!(ws.branch, "");
        assert_eq!(ws.detected_worktree, None);
        assert_eq!(svc.list().unwrap().len(), 1);
    }

    #[test]
    fn create_empty_name_errors() {
        let (svc, dir) = make_service();
        assert!(svc.create(dir.path(), "", "task").is_err());
        assert!(svc.create(dir.path(), "   ", "task").is_err());
        assert!(svc.list().unwrap().is_empty());
    }

    #[test]
    fn status_of_unknown_workspace_is_none() {
        let (svc, _dir) = make_service();
        assert!(svc.status_of(Uuid::new_v4()).is_none());
    }

    #[test]
    fn set_and_read_status() {
        use tessera_core::AgentStatus;
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "P", "").unwrap();
        svc.set_status(ws.id, AgentStatus::Working);
        assert_eq!(svc.status_of(ws.id), Some(AgentStatus::Working));
    }

    #[test]
    fn install_hooks_writes_settings_file() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", "").unwrap();
        let exe = std::path::PathBuf::from("/abs/path/to/tessera");
        svc.install_hooks(&folder, ws.id, &exe).unwrap();
        let path = folder.join(".claude/settings.local.json");
        assert!(path.exists());
        let s = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(parsed["hooks"]["Stop"].is_array());
        let stop_cmd = parsed["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(stop_cmd.contains(&ws.id.to_string()));
        assert!(stop_cmd.contains("/abs/path/to/tessera"));
    }

    #[test]
    fn delete_removes_row_and_leaves_user_folder() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", "").unwrap();
        svc.delete(ws.id, true).unwrap();
        assert!(folder.exists(), "user folder must not be removed");
        assert!(svc.list().unwrap().is_empty());
        assert_eq!(svc.current_session(ws.id), None);
    }

    #[test]
    fn delete_unknown_workspace_errors() {
        let (svc, _dir) = make_service();
        assert!(svc.delete(Uuid::new_v4(), true).is_err());
    }

    #[test]
    fn set_detected_worktree_persists_and_reads_back() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", "task").unwrap();

        svc.set_detected_worktree(
            ws.id,
            Some(std::path::PathBuf::from("/tmp/wt-x")),
            Some("feat/x".to_string()),
        )
        .unwrap();

        let listed = svc.list().unwrap();
        let row = listed.iter().find(|w| w.id == ws.id).unwrap();
        assert_eq!(
            row.detected_worktree,
            Some(std::path::PathBuf::from("/tmp/wt-x"))
        );
        assert_eq!(row.detected_branch, Some("feat/x".to_string()));
    }

    #[test]
    fn spawn_agent_pipes_task_prompt_into_pty() {
        use std::time::{Duration, Instant};
        use tessera_pty::PtyEvent;
        use tokio::sync::broadcast::error::TryRecvError;

        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", "hello from tessera").unwrap();

        // Subscribe to PTY broadcast BEFORE spawning so we don't miss bytes.
        let mut rx = svc.supervisor.subscribe();

        // Use `cat` as the "agent": whatever we pipe in echoes back.
        svc.spawn_agent_with_program(ws.id, "cat", &[], 80, 24).unwrap();

        let mut buf = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match rx.try_recv() {
                Ok(PtyEvent::Data { bytes, .. }) => {
                    buf.extend_from_slice(&bytes);
                    if std::str::from_utf8(&buf)
                        .is_ok_and(|s| s.contains("hello from tessera"))
                    {
                        return;
                    }
                }
                Ok(PtyEvent::Exit { .. }) => break,
                Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        panic!(
            "prompt was not piped; saw: {:?}",
            String::from_utf8_lossy(&buf)
        );
    }
}

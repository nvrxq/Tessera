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
    supervisor: Arc<Supervisor>,
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
    /// - `folder_path` is the source folder. When `branch_name` is set it must
    ///   be a git repo. When `branch_name` is `None`, any folder is accepted
    ///   and the folder is used directly as the agent's working dir (no worktree).
    /// - `name` is a user-supplied label (required, must be non-empty).
    /// - `branch_name` is optional. When `Some`, a git worktree is created at
    ///   `worktree_root/<uuid>/` for that branch.
    pub fn create(
        &self,
        folder_path: &std::path::Path,
        name: &str,
        branch_name: Option<&str>,
    ) -> Result<Workspace> {
        use chrono::Utc;
        use tessera_core::SetupStatus;

        anyhow::ensure!(!name.trim().is_empty(), "workspace name is required");

        let id = Uuid::new_v4();
        let (worktree_path, created_worktree) = match branch_name.filter(|b| !b.is_empty()) {
            Some(branch) => {
                std::fs::create_dir_all(&self.worktree_root)?;
                let wt = self.worktree_root.join(id.to_string());
                tessera_git::worktree::create(folder_path, &wt, branch)?;
                (wt, true)
            }
            None => (folder_path.to_path_buf(), false),
        };

        let ws = Workspace {
            id,
            name: name.to_string(),
            repo_path: folder_path.to_path_buf(),
            worktree_path: worktree_path.clone(),
            branch: branch_name.unwrap_or("").to_string(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
        };

        let conn = self.db.lock().unwrap();
        if let Err(e) = tessera_store::workspaces::insert(&conn, &ws) {
            // Roll back the worktree we just created (if any).
            if created_worktree {
                let _ = tessera_git::worktree::delete(folder_path, &worktree_path, true);
            }
            return Err(e);
        }

        tracing::info!(id = %ws.id, name = %ws.name, branch = %ws.branch, "workspace created");
        Ok(ws)
    }

    pub fn spawn_agent(
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
        Ok(session_id)
    }

    pub fn delete(&self, workspace_id: Uuid, force: bool) -> Result<()> {
        let workspace = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };

        // Kill any live session for this workspace and forget its status.
        if let Some(session_id) = self.sessions.lock().unwrap().remove(&workspace_id) {
            let _ = self.supervisor.kill(session_id);
        }
        self.statuses.lock().unwrap().remove(&workspace_id);

        // Only delete the worktree on disk if we created one (branch was set).
        // If worktree_path == repo_path, the user pointed us at an existing
        // folder and we should leave it alone.
        if workspace.worktree_path != workspace.repo_path {
            tessera_git::worktree::delete(&workspace.repo_path, &workspace.worktree_path, force)?;
        }

        // Delete the DB row last so partial failures still leave something findable.
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::delete(&conn, workspace_id)?;
        Ok(())
    }

    pub fn set_status(&self, workspace_id: Uuid, status: tessera_core::AgentStatus) {
        self.statuses.lock().unwrap().insert(workspace_id, status);
    }

    pub fn status_of(&self, workspace_id: Uuid) -> Option<tessera_core::AgentStatus> {
        self.statuses.lock().unwrap().get(&workspace_id).copied()
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
        let list = svc.list().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn current_session_returns_none_for_unknown() {
        let (svc, _dir) = make_service();
        assert!(svc.current_session(Uuid::new_v4()).is_none());
    }

    use git2::{Repository, Signature};

    /// Build a tiny git repo with one commit, returning its path.
    fn init_repo(parent: &std::path::Path) -> std::path::PathBuf {
        let repo_path = parent.join("src-repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        let repo = Repository::init(&repo_path).unwrap();
        {
            let sig = Signature::now("t", "t@x").unwrap();
            std::fs::write(repo_path.join("README.md"), "hi\n").unwrap();
            let mut idx = repo.index().unwrap();
            idx.add_path(std::path::Path::new("README.md")).unwrap();
            let tree_id = idx.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
            idx.write().unwrap();
        }
        repo_path
    }

    #[test]
    fn create_with_branch_makes_worktree_and_persists_row() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        let ws = svc.create(&repo, "Login work", Some("feat/login")).unwrap();
        assert_eq!(ws.name, "Login work");
        assert_eq!(ws.branch, "feat/login");
        assert!(ws.worktree_path.exists());
        assert_ne!(ws.worktree_path, ws.repo_path);
        let listed = svc.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, ws.id);
    }

    #[test]
    fn create_without_branch_uses_folder_as_worktree() {
        let (svc, dir) = make_service();
        let plain_folder = dir.path().join("plain");
        std::fs::create_dir_all(&plain_folder).unwrap();
        let ws = svc.create(&plain_folder, "Plain work", None).unwrap();
        assert_eq!(ws.name, "Plain work");
        assert_eq!(ws.branch, "");
        assert_eq!(ws.worktree_path, plain_folder);
        assert!(
            plain_folder.exists(),
            "user folder must NOT be moved or removed"
        );
    }

    #[test]
    fn create_empty_name_errors() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        assert!(svc.create(&repo, "", Some("feat/x")).is_err());
        assert!(svc.create(&repo, "   ", Some("feat/x")).is_err());
        assert!(svc.list().unwrap().is_empty());
    }

    #[test]
    fn create_duplicate_branch_errors_and_leaves_no_row() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        svc.create(&repo, "first", Some("dup")).unwrap();
        let err = svc.create(&repo, "second", Some("dup")).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("dup") || msg.to_lowercase().contains("ref"),
            "msg: {msg}"
        );
        let listed = svc.list().unwrap();
        assert_eq!(listed.len(), 1, "rollback should leave only the first row");
    }

    #[test]
    fn spawn_agent_returns_session_id_and_tracks_it() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        let ws = svc.create(&repo, "x", Some("feat/x")).unwrap();
        let sid = svc
            .spawn_agent(ws.id, "bash", &["-c", "echo hi; sleep 0.1"], 80, 24)
            .unwrap();
        assert_eq!(svc.current_session(ws.id), Some(sid));
    }

    #[test]
    fn delete_with_branch_removes_worktree() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        let ws = svc.create(&repo, "x", Some("feat/x")).unwrap();
        let wt_path = ws.worktree_path.clone();
        svc.delete(ws.id, true).unwrap();
        assert!(!wt_path.exists());
        assert!(svc.list().unwrap().is_empty());
        assert_eq!(svc.current_session(ws.id), None);
    }

    #[test]
    fn delete_without_branch_leaves_user_folder() {
        let (svc, dir) = make_service();
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let ws = svc.create(&plain, "Plain", None).unwrap();
        svc.delete(ws.id, true).unwrap();
        assert!(
            plain.exists(),
            "user folder must NOT be removed on workspace delete"
        );
        assert!(svc.list().unwrap().is_empty());
    }

    #[test]
    fn delete_unknown_workspace_errors() {
        let (svc, _dir) = make_service();
        assert!(svc.delete(Uuid::new_v4(), true).is_err());
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
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let ws = svc.create(&plain, "P", None).unwrap();
        svc.set_status(ws.id, AgentStatus::Working);
        assert_eq!(svc.status_of(ws.id), Some(AgentStatus::Working));
        svc.set_status(ws.id, AgentStatus::Done);
        assert_eq!(svc.status_of(ws.id), Some(AgentStatus::Done));
    }
}

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
        }
    }

    pub fn list(&self) -> Result<Vec<Workspace>> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::list(&conn)
    }

    pub fn current_session(&self, workspace_id: Uuid) -> Option<Uuid> {
        self.sessions.lock().unwrap().get(&workspace_id).copied()
    }

    pub fn create(&self, repo_path: &std::path::Path, branch_name: &str) -> Result<Workspace> {
        use chrono::Utc;
        use tessera_core::SetupStatus;

        std::fs::create_dir_all(&self.worktree_root)?;
        let id = Uuid::new_v4();
        let worktree_path = self.worktree_root.join(id.to_string());

        // Create worktree first — if this fails, no row is written.
        tessera_git::worktree::create(repo_path, &worktree_path, branch_name)?;

        let ws = Workspace {
            id,
            name: branch_name.to_string(),
            repo_path: repo_path.to_path_buf(),
            worktree_path: worktree_path.clone(),
            branch: branch_name.to_string(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
        };

        let conn = self.db.lock().unwrap();
        if let Err(e) = tessera_store::workspaces::insert(&conn, &ws) {
            // Roll back the on-disk worktree.
            let _ = tessera_git::worktree::delete(repo_path, &worktree_path, true);
            return Err(e);
        }

        tracing::info!(id = %ws.id, branch = %ws.branch, "workspace created");
        Ok(ws)
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
    fn create_makes_worktree_and_persists_row() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        let ws = svc.create(&repo, "feat/login").unwrap();
        assert_eq!(ws.branch, "feat/login");
        assert!(ws.worktree_path.exists());
        let listed = svc.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, ws.id);
    }

    #[test]
    fn create_duplicate_branch_errors_and_leaves_no_row() {
        let (svc, dir) = make_service();
        let repo = init_repo(dir.path());
        svc.create(&repo, "dup").unwrap();
        let err = svc.create(&repo, "dup").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("dup") || msg.to_lowercase().contains("ref"),
            "msg: {msg}"
        );
        let listed = svc.list().unwrap();
        assert_eq!(listed.len(), 1, "rollback should leave only the first row");
    }
}

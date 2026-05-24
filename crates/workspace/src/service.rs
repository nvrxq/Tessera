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
}

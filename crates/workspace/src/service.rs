use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tessera_core::{Project, Workspace};
use tessera_pty::Supervisor;
use uuid::Uuid;

/// Accept only `"#RRGGBB"` strings. We keep this strict so the frontend can
/// trust the accent value enough to inject it straight into CSS.
fn is_valid_hex_color(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

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

    /// Look up a single workspace by id. Used by the inventory command to
    /// resolve the workspace's folder before scanning Claude's per-project
    /// skill + MCP locations.
    pub fn get(&self, workspace_id: Uuid) -> Result<Option<Workspace>> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::get(&conn, workspace_id)
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
        dangerous_skip_permissions: bool,
        project_id: Option<Uuid>,
    ) -> Result<Workspace> {
        use chrono::Utc;
        use tessera_core::SetupStatus;

        anyhow::ensure!(!name.trim().is_empty(), "workspace name is required");

        let conn = self.db.lock().unwrap();

        // Validate the referenced project exists, so we don't insert a
        // dangling FK that SQLite would later reject.
        if let Some(pid) = project_id {
            anyhow::ensure!(
                tessera_store::projects::get(&conn, pid)?.is_some(),
                "project {pid} not found",
            );
        }

        // New workspaces go to the end of the list so user-curated order is
        // never disturbed by a fresh row jumping to the top.
        let next_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), 0) + 1000 FROM workspaces",
                [],
                |r| r.get(0),
            )
            .unwrap_or(1000);

        let id = Uuid::new_v4();
        let ws = Workspace {
            id,
            name: name.to_string(),
            repo_path: folder_path.to_path_buf(),
            worktree_path: folder_path.to_path_buf(),
            branch: String::new(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
            detected_worktree: None,
            detected_branch: None,
            dangerous_skip_permissions,
            has_prior_session: false,
            project_id,
            sort_order: next_order,
            claude_session_id: None,
            archived_at: None,
        };

        tessera_store::workspaces::insert(&conn, &ws)?;

        tracing::info!(
            id = %ws.id,
            name = %ws.name,
            dangerous = ws.dangerous_skip_permissions,
            project_id = ?ws.project_id,
            sort_order = ws.sort_order,
            "workspace created",
        );
        Ok(ws)
    }

    /// Update the user-visible workspace name. Trimmed and validated to
    /// be non-empty; everything else (project, ordering, sessions) is
    /// untouched.
    pub fn rename(&self, id: Uuid, new_name: &str) -> Result<()> {
        let trimmed = new_name.trim();
        anyhow::ensure!(!trimmed.is_empty(), "workspace name is required");
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_name(&conn, id, trimmed)
    }

    /// Archived workspaces (newest archive first). The active sidebar
    /// query uses `list()`, which excludes these; the archive section in
    /// the UI queries this directly.
    pub fn list_archived(&self) -> Result<Vec<Workspace>> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::list_archived(&conn)
    }

    /// Move a workspace into the archive. Kills the live PTY (same as
    /// delete) so claude isn't left running attached to a hidden row,
    /// but everything else — DB row, projects, activity, the pinned
    /// `claude_session_id` — stays intact so `unarchive` restores the
    /// conversation cleanly.
    pub fn archive(&self, workspace_id: Uuid) -> Result<()> {
        // Existence check up front, same shape as `delete`.
        let _ = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };
        if let Some(session_id) = self.sessions.lock().unwrap().remove(&workspace_id) {
            let _ = self.supervisor.kill(session_id);
        }
        self.statuses.lock().unwrap().remove(&workspace_id);
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_archived_at(&conn, workspace_id, Some(chrono::Utc::now()))
    }

    /// Restore an archived workspace. The pinned `claude_session_id`
    /// (if any) is preserved so the next spawn picks the conversation
    /// back up via `--resume`.
    pub fn unarchive(&self, workspace_id: Uuid) -> Result<()> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_archived_at(&conn, workspace_id, None)
    }

    pub fn delete(&self, workspace_id: Uuid) -> Result<()> {
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

    /// Spawn `claude` directly in the workspace folder. No shell wrapper,
    /// no integration scripts — Tessera is a Claude Code TUI viewer, not a
    /// general terminal.
    ///
    /// **Deterministic conversation identity** (this is what finally fixes the
    /// same-folder session-mixing bug). Tessera *generates* the session uuid
    /// and hands it to claude:
    /// * **first spawn** → generate a uuid, persist it on the row, launch
    ///   `claude --session-id <uuid>` (a fresh conversation with OUR id).
    /// * **subsequent spawns, jsonl present** → `claude --resume <uuid>`.
    /// * **subsequent spawns, jsonl gone** → `claude --session-id <uuid>`
    ///   again (re-create the same stable id; `--resume` hard-errors on a
    ///   missing id, so we must not use it blindly).
    ///
    /// The old scheme pinned by *mtime detection* (`newest_session_id(cwd)`),
    /// which grabbed whichever jsonl in the folder was touched last — i.e. a
    /// sibling workspace's session — and otherwise fell back to `--continue`,
    /// which resumes the folder's newest conversation regardless of which
    /// workspace asked. Both are gone.
    ///
    /// Idempotent: if a live PTY already backs this workspace, reuse it rather
    /// than spawning a second claude (which would orphan the first and litter
    /// the folder with extra jsonls — aggravating the very mixing we fix).
    pub fn spawn_agent(&self, workspace_id: Uuid, cols: u16, rows: u16) -> Result<Uuid> {
        // Reuse a still-live session for this workspace. Guards against the
        // App pre-spawn racing the Terminal-mount spawn past the frontend
        // dedupe, and against a stale frontend session_id triggering a
        // duplicate launch.
        if let Some(existing) = self.current_session(workspace_id) {
            if self.supervisor.is_alive(existing) {
                return Ok(existing);
            }
        }

        let workspace = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };
        let program = "claude".to_string();
        let mut args: Vec<String> = Vec::new();
        if workspace.dangerous_skip_permissions {
            args.push("--dangerously-skip-permissions".to_string());
        }

        // Resolve (and if necessary mint + persist) this workspace's stable
        // session uuid before spawning, so a crash mid-spawn still remembers
        // the identity.
        let session_uuid = match workspace.claude_session_id.clone() {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                let conn = self.db.lock().unwrap();
                // Re-read the pin under the same lock we write it with, so two
                // racing first-spawns can't both mint and clobber each other —
                // if another call already pinned, adopt its uuid.
                match tessera_store::workspaces::get(&conn, workspace_id)? {
                    Some(w) if w.claude_session_id.is_some() => w.claude_session_id.unwrap(),
                    _ => {
                        if let Err(e) = tessera_store::workspaces::update_claude_session_id(
                            &conn,
                            workspace_id,
                            Some(&id),
                        ) {
                            tracing::warn!(error = %e, "persisting new claude_session_id failed");
                        }
                        id
                    }
                }
            }
        };
        if tessera_core::claude_session::session_file_exists_anywhere(&session_uuid) {
            args.push("--resume".to_string());
            args.push(session_uuid);
        } else {
            args.push("--session-id".to_string());
            args.push(session_uuid);
        }

        self.spawn_session_inner(
            workspace_id,
            &workspace.worktree_path,
            &program,
            &args,
            Vec::new(),
            cols,
            rows,
        )
    }

    /// User-facing "Reset session" menu entry: discard the current
    /// conversation and start fresh on the next launch.
    ///
    /// We must end the live PTY (and forget the mapping), not just clear the
    /// pin. Spawning is now idempotent — it hands back a still-live session for
    /// the workspace — so clearing the pin alone would have no visible effect:
    /// the next spawn would return the same running claude on the same
    /// conversation. Killing the PTY makes the frontend show its "session
    /// ended → Restart" overlay; the restart mints a brand-new session uuid
    /// (the pin is now None) and launches `claude --session-id <new>`.
    pub fn reset_claude_session(&self, workspace_id: Uuid) -> Result<()> {
        if let Some(session_id) = self.sessions.lock().unwrap().remove(&workspace_id) {
            let _ = self.supervisor.kill(session_id);
        }
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_claude_session_id(&conn, workspace_id, None)
    }

    /// Like `spawn_agent` but lets callers pick the program — used by tests to
    /// substitute `cat` for the default shell. **No** Tessera integration is
    /// loaded here — direct spawn.
    pub fn spawn_agent_with_program(
        &self,
        workspace_id: Uuid,
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
    ) -> Result<Uuid> {
        let workspace = {
            let conn = self.db.lock().unwrap();
            tessera_store::workspaces::get(&conn, workspace_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace {workspace_id} not found"))?
        };
        self.spawn_session_inner(
            workspace_id,
            &workspace.worktree_path,
            program,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            Vec::new(),
            cols,
            rows,
        )
    }

    // Each parameter has a distinct domain meaning (program, args, env,
    // cwd, dims, workspace_id) and bundling them into a struct just to
    // satisfy clippy would force callers to construct a builder for a
    // private helper. The clarity tradeoff is worth the lint suppression.
    #[allow(clippy::too_many_arguments)]
    fn spawn_session_inner(
        &self,
        workspace_id: Uuid,
        cwd: &std::path::Path,
        program: &str,
        args: &[String],
        env: Vec<(String, String)>,
        cols: u16,
        rows: u16,
    ) -> Result<Uuid> {
        use tessera_pty::session::SessionConfig;
        // Bind workspace identity to the PROCESS, not the on-disk folder.
        // Two workspaces sharing one folder both write their UUID into the
        // shared settings.local.json (last-write-wins), so the baked-in hook
        // argv can't be trusted to identify the caller. Carrying the id in
        // claude's env — which the hook subprocesses inherit — lets the hook
        // command's `${TESSERA_WORKSPACE_ID:-<id>}` expansion resolve to the
        // correct workspace per-process.
        let mut env = env;
        env.push(("TESSERA_WORKSPACE_ID".to_string(), workspace_id.to_string()));
        let cfg = SessionConfig {
            program: program.to_string(),
            args: args.to_vec(),
            cwd: cwd.to_path_buf(),
            cols,
            rows,
            env,
        };
        let session_id = self.supervisor.spawn(cfg)?;
        self.sessions
            .lock()
            .unwrap()
            .insert(workspace_id, session_id);
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

    // ---- Projects ----

    pub fn create_project(&self, name: &str, accent: Option<String>) -> Result<Project> {
        anyhow::ensure!(!name.trim().is_empty(), "project name is required");
        if let Some(ref hex) = accent {
            anyhow::ensure!(is_valid_hex_color(hex), "accent must be \"#RRGGBB\"");
        }
        let project = Project {
            id: Uuid::new_v4(),
            name: name.to_string(),
            accent,
            created_at: chrono::Utc::now(),
        };
        let conn = self.db.lock().unwrap();
        tessera_store::projects::insert(&conn, &project)?;
        tracing::info!(id = %project.id, name = %project.name, "project created");
        Ok(project)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.db.lock().unwrap();
        tessera_store::projects::list(&conn)
    }

    /// Delete a project. Workspaces that pointed at it stay alive — the
    /// FK is `ON DELETE SET NULL`, so they become ungrouped.
    pub fn delete_project(&self, id: Uuid) -> Result<()> {
        let conn = self.db.lock().unwrap();
        tessera_store::projects::delete(&conn, id)
    }

    /// Apply a batch of sort-order updates (UUID -> new sort_order). Runs
    /// in a single SQLite transaction.
    pub fn reorder_workspaces(&self, updates: Vec<(Uuid, i64)>) -> Result<()> {
        let conn = self.db.lock().unwrap();
        tessera_store::workspaces::update_sort_orders(&conn, &updates)
    }

    /// Move a workspace into a project (or out, when `project_id` is `None`).
    pub fn assign_project(&self, workspace_id: Uuid, project_id: Option<Uuid>) -> Result<()> {
        let conn = self.db.lock().unwrap();
        if let Some(pid) = project_id {
            anyhow::ensure!(
                tessera_store::projects::get(&conn, pid)?.is_some(),
                "project {pid} not found",
            );
        }
        tessera_store::workspaces::update_project(&conn, workspace_id, project_id)
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
        // Claude runs hook commands through a shell, so this expands
        // per-process: a spawned claude carries its own workspace id in
        // TESSERA_WORKSPACE_ID (set in spawn_session_inner), and its hook
        // subprocesses inherit it. The `:-{id}` fallback keeps older binaries
        // and any process spawned without the env working with the baked-in
        // UUID. The surrounding double-quotes guard the expansion; the UUID
        // has no shell-special chars, so this is valid JSON-embedded shell.
        let cmd = |kind: &str| format!("{exe} hook \"${{TESSERA_WORKSPACE_ID:-{id}}}\" {kind}");
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
    fn create_inserts_row() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "Login work", false, None).unwrap();
        assert_eq!(ws.name, "Login work");
        assert_eq!(ws.worktree_path, folder);
        assert_eq!(ws.branch, "");
        assert_eq!(ws.detected_worktree, None);
        assert_eq!(ws.project_id, None);
        assert!(ws.sort_order > 0);
        assert_eq!(svc.list().unwrap().len(), 1);
    }

    #[test]
    fn create_empty_name_errors() {
        let (svc, dir) = make_service();
        assert!(svc.create(dir.path(), "", false, None).is_err());
        assert!(svc.create(dir.path(), "   ", false, None).is_err());
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
        let ws = svc.create(&folder, "P", false, None).unwrap();
        svc.set_status(ws.id, AgentStatus::Working);
        assert_eq!(svc.status_of(ws.id), Some(AgentStatus::Working));
    }

    #[test]
    fn install_hooks_writes_settings_file() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", false, None).unwrap();
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
        // Identity is bound to the process via env, with the baked-in UUID
        // kept as a fallback for older binaries / env-less spawns.
        assert!(stop_cmd.contains("TESSERA_WORKSPACE_ID"));
        assert!(stop_cmd.contains(&ws.id.to_string()));
        assert!(stop_cmd.contains("/abs/path/to/tessera"));
    }

    #[test]
    fn delete_removes_row_and_leaves_user_folder() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", false, None).unwrap();
        svc.delete(ws.id).unwrap();
        assert!(folder.exists(), "user folder must not be removed");
        assert!(svc.list().unwrap().is_empty());
        assert_eq!(svc.current_session(ws.id), None);
    }

    #[test]
    fn archive_hides_from_list_and_unarchive_restores() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let active = svc.create(&folder, "active", false, None).unwrap();
        let archive_me = svc.create(&folder, "later", false, None).unwrap();
        assert_eq!(svc.list().unwrap().len(), 2);

        svc.archive(archive_me.id).unwrap();
        let active_list = svc.list().unwrap();
        assert_eq!(active_list.len(), 1);
        assert_eq!(active_list[0].id, active.id);
        let archived_list = svc.list_archived().unwrap();
        assert_eq!(archived_list.len(), 1);
        assert_eq!(archived_list[0].id, archive_me.id);
        assert!(archived_list[0].archived_at.is_some());

        svc.unarchive(archive_me.id).unwrap();
        assert_eq!(svc.list().unwrap().len(), 2);
        assert!(svc.list_archived().unwrap().is_empty());
    }

    #[test]
    fn reset_claude_session_clears_pin() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "w", false, None).unwrap();
        {
            let conn = svc.db.lock().unwrap();
            tessera_store::workspaces::update_claude_session_id(&conn, ws.id, Some("abc-123"))
                .unwrap();
        }
        let row = svc
            .list()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws.id)
            .unwrap();
        assert_eq!(row.claude_session_id.as_deref(), Some("abc-123"));

        svc.reset_claude_session(ws.id).unwrap();
        let row = svc
            .list()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws.id)
            .unwrap();
        assert_eq!(row.claude_session_id, None);
    }

    #[test]
    fn rename_updates_name_and_rejects_empty() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "old", false, None).unwrap();

        svc.rename(ws.id, "  new name  ").unwrap();
        let row = svc
            .list()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws.id)
            .unwrap();
        assert_eq!(row.name, "new name");

        assert!(svc.rename(ws.id, "").is_err());
        assert!(svc.rename(ws.id, "   ").is_err());
    }

    #[test]
    fn delete_unknown_workspace_errors() {
        let (svc, _dir) = make_service();
        assert!(svc.delete(Uuid::new_v4()).is_err());
    }

    #[test]
    fn set_detected_worktree_persists_and_reads_back() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", false, None).unwrap();

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
    fn spawn_agent_with_program_tracks_session() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "x", false, None).unwrap();
        let sid = svc
            .spawn_agent_with_program(ws.id, "cat", &[], 80, 24)
            .unwrap();
        assert_eq!(svc.current_session(ws.id), Some(sid));
    }

    #[test]
    fn create_project_and_list() {
        let (svc, _dir) = make_service();
        let p = svc
            .create_project("Front-end", Some("#C8825B".into()))
            .unwrap();
        assert_eq!(p.name, "Front-end");
        let all = svc.list_projects().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, p.id);
    }

    #[test]
    fn create_project_rejects_bad_hex() {
        let (svc, _dir) = make_service();
        assert!(svc.create_project("x", Some("C8825B".into())).is_err());
        assert!(svc.create_project("x", Some("#ZZZ".into())).is_err());
        assert!(svc.create_project("", None).is_err());
    }

    #[test]
    fn delete_project_unlinks_workspaces_but_keeps_them() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let p = svc.create_project("p", None).unwrap();
        let ws = svc.create(&folder, "w", false, Some(p.id)).unwrap();
        assert_eq!(ws.project_id, Some(p.id));

        svc.delete_project(p.id).unwrap();
        assert!(svc.list_projects().unwrap().is_empty());
        let listed = svc.list().unwrap();
        let row = listed.iter().find(|w| w.id == ws.id).unwrap();
        assert_eq!(row.project_id, None);
    }

    #[test]
    fn reorder_workspaces_persists() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let a = svc.create(&folder, "a", false, None).unwrap();
        let b = svc.create(&folder, "b", false, None).unwrap();
        // Force b ahead of a by lowering its sort_order.
        svc.reorder_workspaces(vec![(a.id, 5_000), (b.id, 100)])
            .unwrap();
        let listed = svc.list().unwrap();
        assert_eq!(listed[0].id, b.id);
        assert_eq!(listed[1].id, a.id);
    }

    #[test]
    fn assign_project_moves_workspace() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let p = svc.create_project("p", None).unwrap();
        let ws = svc.create(&folder, "w", false, None).unwrap();
        assert_eq!(ws.project_id, None);
        svc.assign_project(ws.id, Some(p.id)).unwrap();
        let row = svc
            .list()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws.id)
            .unwrap();
        assert_eq!(row.project_id, Some(p.id));
        svc.assign_project(ws.id, None).unwrap();
        let row = svc
            .list()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws.id)
            .unwrap();
        assert_eq!(row.project_id, None);
    }

    #[test]
    fn assign_project_rejects_unknown_project() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = svc.create(&folder, "w", false, None).unwrap();
        assert!(svc.assign_project(ws.id, Some(Uuid::new_v4())).is_err());
    }

    #[test]
    fn create_workspace_rejects_unknown_project() {
        let (svc, dir) = make_service();
        let folder = dir.path().join("any-folder");
        std::fs::create_dir_all(&folder).unwrap();
        assert!(svc
            .create(&folder, "w", false, Some(Uuid::new_v4()))
            .is_err());
    }
}

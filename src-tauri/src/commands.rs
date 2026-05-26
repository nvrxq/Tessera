use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tessera_pty::session::SessionConfig;
use tessera_pty::Supervisor;
use uuid::Uuid;

pub type SupervisorState = Arc<Supervisor>;

#[derive(Debug, Deserialize)]
pub struct SpawnArgs {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Serialize)]
pub struct SpawnResponse {
    pub session_id: Uuid,
}

#[tauri::command]
pub fn pty_spawn(
    state: State<'_, SupervisorState>,
    args: SpawnArgs,
) -> Result<SpawnResponse, String> {
    let cfg = SessionConfig {
        program: args.program,
        args: args.args,
        cwd: args.cwd,
        cols: args.cols,
        rows: args.rows,
        env: Vec::new(),
    };
    let session_id = state.spawn(cfg).map_err(|e| e.to_string())?;
    Ok(SpawnResponse { session_id })
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, SupervisorState>,
    session_id: Uuid,
    data_b64: String,
) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| e.to_string())?;
    state.write(session_id, &bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, SupervisorState>,
    session_id: Uuid,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .resize(session_id, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_kill(state: State<'_, SupervisorState>, session_id: Uuid) -> Result<(), String> {
    state.kill(session_id).map_err(|e| e.to_string())
}

// ---- Filesystem helpers ----

/// List child directories of `parent` whose name starts with `prefix`. Used by
/// the folder-path autocomplete in NewWorkspaceForm. Returns absolute paths.
/// Caps the result at 16 entries and skips hidden dirs unless prefix starts with '.'.
#[tauri::command]
pub fn list_directories(input: String) -> Result<Vec<String>, String> {
    let expanded = if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped).to_string_lossy().into_owned()
        } else {
            input.clone()
        }
    } else if input == "~" {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(input.clone())
    } else {
        input.clone()
    };

    let path = std::path::PathBuf::from(&expanded);
    // Decide the directory to list and the prefix to match against.
    let (parent, prefix) = if expanded.ends_with('/') || (path.is_dir() && expanded == "/") {
        (path.clone(), String::new())
    } else if path.is_dir() {
        // exact-dir match: also list its children
        (path.clone(), String::new())
    } else {
        let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let prefix = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        (parent, prefix)
    };

    if !parent.is_dir() {
        return Ok(Vec::new());
    }
    let show_hidden = prefix.starts_with('.');

    let mut out: Vec<String> = std::fs::read_dir(&parent)
        .map_err(|e| e.to_string())?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                return None;
            }
            if !prefix.is_empty() && !name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                return None;
            }
            Some(entry.path().to_string_lossy().into_owned())
        })
        .collect();
    out.sort();
    out.truncate(16);
    Ok(out)
}

// ---- Workspace commands ----

use chrono::{DateTime, Utc};
use tessera_core::{SetupStatus, Workspace};
use tessera_workspace::WorkspaceService;

pub type WorkspaceServiceState = Arc<WorkspaceService>;

#[derive(Debug, Serialize)]
pub struct WorkspaceDto {
    pub id: Uuid,
    pub name: String,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    pub setup_status: SetupStatus,
    pub created_at: DateTime<Utc>,
    pub detected_worktree: Option<PathBuf>,
    pub detected_branch: Option<String>,
    pub dangerous_skip_permissions: bool,
    pub session_id: Option<Uuid>,
    pub agent_status: Option<tessera_core::AgentStatus>,
    pub project_id: Option<Uuid>,
    pub sort_order: i64,
}

impl WorkspaceDto {
    fn from_workspace(
        ws: Workspace,
        session_id: Option<Uuid>,
        agent_status: Option<tessera_core::AgentStatus>,
    ) -> Self {
        Self {
            id: ws.id,
            name: ws.name,
            repo_path: ws.repo_path,
            worktree_path: ws.worktree_path,
            setup_status: ws.setup_status,
            created_at: ws.created_at,
            detected_worktree: ws.detected_worktree,
            detected_branch: ws.detected_branch,
            dangerous_skip_permissions: ws.dangerous_skip_permissions,
            session_id,
            agent_status,
            project_id: ws.project_id,
            sort_order: ws.sort_order,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProjectDto {
    pub id: Uuid,
    pub name: String,
    pub accent: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<tessera_core::Project> for ProjectDto {
    fn from(p: tessera_core::Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
            accent: p.accent,
            created_at: p.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceArgs {
    pub folder_path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub dangerous_skip_permissions: bool,
    #[serde(default)]
    pub project_id: Option<Uuid>,
}

/// One row of a workspace_reorder request. We use a named struct (rather
/// than `Vec<(Uuid, i64)>`) so the JSON wire shape is self-describing —
/// `[{"workspace_id": "...", "sort_order": 100}, ...]` — and the
/// TypeScript bindings end up readable.
#[derive(Debug, Deserialize)]
pub struct ReorderEntry {
    pub workspace_id: Uuid,
    pub sort_order: i64,
}

#[tauri::command]
pub fn workspace_create(
    state: State<'_, WorkspaceServiceState>,
    args: CreateWorkspaceArgs,
) -> Result<WorkspaceDto, String> {
    let ws = state
        .create(
            &args.folder_path,
            &args.name,
            args.dangerous_skip_permissions,
            args.project_id,
        )
        .map_err(|e| e.to_string())?;

    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = state.install_hooks(&ws.worktree_path, ws.id, &exe) {
            tracing::warn!(error = %e, "install_hooks failed");
        }
    }

    // Don't spawn the agent here — the frontend Terminal component spawns
    // after measuring its container, so claude starts at the correct PTY
    // size and doesn't have to redraw on first SIGWINCH.
    Ok(WorkspaceDto::from_workspace(ws, None, None))
}

#[tauri::command]
pub fn workspace_list(
    state: State<'_, WorkspaceServiceState>,
) -> Result<Vec<WorkspaceDto>, String> {
    let items = state.list().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|w| {
            let sid = state.current_session(w.id);
            let status = state.status_of(w.id);
            WorkspaceDto::from_workspace(w, sid, status)
        })
        .collect())
}

#[tauri::command]
pub fn workspace_spawn_agent(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
    cols: u16,
    rows: u16,
) -> Result<Uuid, String> {
    state
        .spawn_agent(workspace_id, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_delete(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
    force: bool,
) -> Result<(), String> {
    state.delete(workspace_id, force).map_err(|e| e.to_string())
}

// ---- Projects & ordering ----

#[tauri::command]
pub fn project_create(
    state: State<'_, WorkspaceServiceState>,
    name: String,
    accent: Option<String>,
) -> Result<ProjectDto, String> {
    state
        .create_project(&name, accent)
        .map(ProjectDto::from)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn project_list(
    state: State<'_, WorkspaceServiceState>,
) -> Result<Vec<ProjectDto>, String> {
    state
        .list_projects()
        .map(|v| v.into_iter().map(ProjectDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn project_delete(
    state: State<'_, WorkspaceServiceState>,
    id: Uuid,
) -> Result<(), String> {
    state.delete_project(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_reorder(
    state: State<'_, WorkspaceServiceState>,
    updates: Vec<ReorderEntry>,
) -> Result<(), String> {
    let pairs: Vec<(Uuid, i64)> = updates
        .into_iter()
        .map(|e| (e.workspace_id, e.sort_order))
        .collect();
    state.reorder_workspaces(pairs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_assign_project(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
) -> Result<(), String> {
    state
        .assign_project(workspace_id, project_id)
        .map_err(|e| e.to_string())
}

// ---- Terminal grid (Canvas2D backend) ----

use std::collections::HashMap;
use std::sync::Mutex;

use crate::terminal::TerminalRegistry;

pub type TerminalRegistryState = std::sync::Arc<TerminalRegistry>;
/// Last-known (cols, rows) per session — used by the PTY pump to spawn a
/// Term at the right size on the first byte chunk.
pub type GridSizesState = std::sync::Arc<Mutex<HashMap<Uuid, (u16, u16)>>>;

/// Frontend tells the backend the desired grid size for a session. Resizes
/// both the wezterm-term parser AND remembers the size for any future
/// lazy-spawned Term in the same session id.
///
/// After the resize, immediately emits a fresh `term_snapshot` event with
/// the full grid. This recovers the frontend if it missed the initial
/// snapshot emit (e.g. listener registration raced ahead of the first
/// `claude` stdout chunk on spawn) — the frontend's local grid is
/// guaranteed to be re-synced to backend state every time it (re)connects.
#[tauri::command]
pub fn terminal_resize(
    app: tauri::AppHandle,
    registry: tauri::State<'_, TerminalRegistryState>,
    sizes: tauri::State<'_, GridSizesState>,
    session_id: Uuid,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    use tauri::Emitter;
    sizes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session_id, (cols, rows));
    registry.resize(session_id, cols, rows);
    if let Some(snap) = registry.snapshot(session_id) {
        let _ = app.emit("term_snapshot", snap);
    }
    Ok(())
}

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
    pub task_prompt: String,
    pub detected_worktree: Option<PathBuf>,
    pub detected_branch: Option<String>,
    pub session_id: Option<Uuid>,
    pub agent_status: Option<tessera_core::AgentStatus>,
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
            task_prompt: ws.task_prompt,
            detected_worktree: ws.detected_worktree,
            detected_branch: ws.detected_branch,
            session_id,
            agent_status,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceArgs {
    pub folder_path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub task_prompt: String,
}

#[tauri::command]
pub fn workspace_create(
    state: State<'_, WorkspaceServiceState>,
    args: CreateWorkspaceArgs,
) -> Result<WorkspaceDto, String> {
    let ws = state
        .create(&args.folder_path, &args.name, &args.task_prompt)
        .map_err(|e| e.to_string())?;

    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = state.install_hooks(&ws.worktree_path, ws.id, &exe) {
            tracing::warn!(error = %e, "install_hooks failed");
        }
    }

    let sid = state
        .spawn_agent(ws.id, 80, 24)
        .map_err(|e| e.to_string())?;
    let status = state.status_of(ws.id);
    Ok(WorkspaceDto::from_workspace(ws, Some(sid), status))
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
) -> Result<Uuid, String> {
    state
        .spawn_agent(workspace_id, 80, 24)
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

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

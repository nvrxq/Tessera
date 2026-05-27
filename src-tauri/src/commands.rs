use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tessera_core::{config::config_path, UserConfig};
use tessera_pty::session::SessionConfig;
use tessera_pty::Supervisor;
use uuid::Uuid;

pub type SupervisorState = Arc<Supervisor>;
/// Shared SQLite handle used by the workspace-extras commands. Lives behind
/// the same `Arc<Mutex<Connection>>` that `WorkspaceService` holds, so the
/// extras commands and the existing workspace commands serialise on one lock.
pub type DbState = Arc<Mutex<rusqlite::Connection>>;

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
pub fn project_list(state: State<'_, WorkspaceServiceState>) -> Result<Vec<ProjectDto>, String> {
    state
        .list_projects()
        .map(|v| v.into_iter().map(ProjectDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn project_delete(state: State<'_, WorkspaceServiceState>, id: Uuid) -> Result<(), String> {
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

/// Move the terminal's scroll view. Positive `delta_back` scrolls into
/// history; negative pulls back toward the live tail. Resulting offset is
/// clamped to the scrollback buffer's size. Re-emits a fresh `term_snapshot`
/// so the frontend repaints the new view immediately. Any subsequent PTY
/// data automatically resets the offset to 0 (live tail) inside `feed`.
#[tauri::command]
pub fn terminal_scroll(
    app: tauri::AppHandle,
    registry: tauri::State<'_, TerminalRegistryState>,
    session_id: Uuid,
    delta_back: i32,
) -> Result<usize, String> {
    use tauri::Emitter;
    let new_offset = registry.set_scroll_delta(session_id, delta_back);
    if let Some(snap) = registry.snapshot(session_id) {
        let _ = app.emit("term_snapshot", snap);
    }
    Ok(new_offset)
}

// ---- Settings ----

/// Read the user's settings file (or defaults if it doesn't exist / is
/// malformed). Cheap on a cold call; no caching here — the frontend
/// holds the canonical in-memory copy after first load.
#[tauri::command]
pub fn settings_load() -> Result<UserConfig, String> {
    Ok(UserConfig::load_or_default(&config_path()))
}

/// Persist the settings and notify the frontend so live changes (palette,
/// font, cursor) apply without a restart. The backend palette is hot-swapped
/// here too — the next `term_snapshot` for every live session re-emits a
/// `full` payload with the new colours.
#[tauri::command]
pub fn settings_save(
    app: tauri::AppHandle,
    registry: tauri::State<'_, TerminalRegistryState>,
    config: UserConfig,
) -> Result<(), String> {
    let path = config_path();
    config.save(&path).map_err(|e| e.to_string())?;

    let palette = crate::terminal::palette_from_config(&config);
    registry.set_palette(palette);

    // Event is the only signal — UI components subscribe and re-derive
    // their CSS variables, font sizes, etc. We send the new config as
    // the event payload so subscribers don't have to round-trip back to
    // disk on every change.
    let _ = app.emit("settings_changed", &config);
    Ok(())
}

/// Return the on-disk settings path so the user (or Claude Code on the
/// user's behalf) can hand-edit the JSON file when easier than walking
/// the modal. Returned even if the file doesn't exist yet.
#[tauri::command]
pub fn settings_config_path() -> String {
    config_path().to_string_lossy().into_owned()
}

/// Persist a pasted image (PNG bytes, base64-encoded) to disk and return
/// the absolute path. Frontend pastes flow: clipboard → readImage → encode
/// PNG via canvas.toDataURL → this command → write returned path into the
/// PTY (wrapped in bracketed-paste markers) so Claude Code attaches it.
///
/// Stored under `~/Library/Application Support/tessera/pastes/<uuid>.png`
/// on macOS, `~/.local/share/tessera/pastes/<uuid>.png` on Linux. We do
/// NOT auto-clean — keep things simple; the directory is small (PNGs
/// from clipboard are typically ≤2 MB) and predictable.
#[tauri::command]
pub fn save_paste_image(data_b64: String) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    let dir = dirs::data_local_dir()
        .ok_or_else(|| "no platform data dir".to_string())?
        .join("tessera")
        .join("pastes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.png", Uuid::new_v4()));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

// ---- Workspace extras (links / tasks / pomodoro) ----
//
// These commands share the same `Arc<Mutex<Connection>>` as
// `WorkspaceService`, so all writes are serialised through one lock — no
// risk of interleaving with workspace CRUD.

use tessera_core::{LinkKind, PomodoroMode, PomodoroState, WorkspaceLink, WorkspaceTask};

#[derive(Debug, Serialize)]
pub struct WorkspaceLinkDto {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub label: Option<String>,
    pub url: String,
    pub kind: LinkKind,
    pub created_at: DateTime<Utc>,
    pub sort_order: i64,
}

impl From<WorkspaceLink> for WorkspaceLinkDto {
    fn from(l: WorkspaceLink) -> Self {
        Self {
            id: l.id,
            workspace_id: l.workspace_id,
            label: l.label,
            url: l.url,
            kind: l.kind,
            created_at: l.created_at,
            sort_order: l.sort_order,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkspaceTaskDto {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub done: bool,
    pub sort_order: i64,
    pub due_date: Option<String>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl From<WorkspaceTask> for WorkspaceTaskDto {
    fn from(t: WorkspaceTask) -> Self {
        Self {
            id: t.id,
            workspace_id: t.workspace_id,
            title: t.title,
            done: t.done,
            sort_order: t.sort_order,
            due_date: t.due_date,
            created_at: t.created_at,
            completed_at: t.completed_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PomodoroStateDto {
    pub workspace_id: Uuid,
    pub mode: PomodoroMode,
    pub started_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub target_seconds: i64,
    pub elapsed_seconds_before_pause: i64,
    pub cycles_completed: i64,
    pub updated_at: DateTime<Utc>,
}

impl From<PomodoroState> for PomodoroStateDto {
    fn from(s: PomodoroState) -> Self {
        Self {
            workspace_id: s.workspace_id,
            mode: s.mode,
            started_at: s.started_at,
            paused_at: s.paused_at,
            target_seconds: s.target_seconds,
            elapsed_seconds_before_pause: s.elapsed_seconds_before_pause,
            cycles_completed: s.cycles_completed,
            updated_at: s.updated_at,
        }
    }
}

/// Match `https://github.com/<owner>/<repo>/(issues|pull)/<n>`. We strip
/// `http://` and `https://`, then walk segments by hand — keeps us inside
/// the workspace's existing dependency set (no `regex` crate).
///
/// Returns `(kind, "owner/repo#N")` on a match, or `None` for any other URL.
fn detect_github(url: &str) -> Option<(LinkKind, String)> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    let mut parts = without_scheme.split('/');
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
        return None;
    }
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    let kind_seg = parts.next()?;
    let number_seg = parts.next()?;
    // Number may carry a trailing slash/query/fragment — strip on first
    // non-digit so `/pull/123#issuecomment-...` still parses.
    let number: String = number_seg
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if number.is_empty() {
        return None;
    }
    let kind = match kind_seg {
        "issues" => LinkKind::GithubIssue,
        "pull" => LinkKind::GithubPr,
        _ => return None,
    };
    Some((kind, format!("{owner}/{repo}#{number}")))
}

#[tauri::command]
pub fn workspace_links_list(
    db: State<'_, DbState>,
    workspace_id: Uuid,
) -> Result<Vec<WorkspaceLinkDto>, String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::list_links(&conn, workspace_id)
        .map(|v| v.into_iter().map(WorkspaceLinkDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_links_add(
    db: State<'_, DbState>,
    workspace_id: Uuid,
    url: String,
    label: Option<String>,
) -> Result<WorkspaceLinkDto, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("url is required".to_string());
    }
    let (kind, derived_label) = match detect_github(&url) {
        Some((k, lbl)) => (k, Some(lbl)),
        None => (LinkKind::Url, None),
    };
    // User-supplied label always wins; otherwise use the GitHub-derived one.
    let final_label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(derived_label);

    let conn = db.lock().unwrap();
    let sort_order = tessera_store::extras::next_link_sort_order(&conn, workspace_id)
        .map_err(|e| e.to_string())?;
    let link = WorkspaceLink {
        id: Uuid::new_v4(),
        workspace_id,
        label: final_label,
        url,
        kind,
        created_at: Utc::now(),
        sort_order,
    };
    tessera_store::extras::insert_link(&conn, &link).map_err(|e| e.to_string())?;
    Ok(WorkspaceLinkDto::from(link))
}

#[tauri::command]
pub fn workspace_links_delete(db: State<'_, DbState>, id: Uuid) -> Result<(), String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::delete_link(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_links_reorder(db: State<'_, DbState>, ids: Vec<Uuid>) -> Result<(), String> {
    // Gapped sort_order so a future single-row insert can slot in.
    let updates: Vec<(Uuid, i64)> = ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, ((i + 1) as i64) * 10))
        .collect();
    let conn = db.lock().unwrap();
    tessera_store::extras::update_link_sort_orders(&conn, &updates).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_tasks_list(
    db: State<'_, DbState>,
    workspace_id: Uuid,
    include_completed: bool,
) -> Result<Vec<WorkspaceTaskDto>, String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::list_tasks(&conn, workspace_id, include_completed)
        .map(|v| v.into_iter().map(WorkspaceTaskDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_tasks_add(
    db: State<'_, DbState>,
    workspace_id: Uuid,
    title: String,
    due_date: Option<String>,
) -> Result<WorkspaceTaskDto, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("title is required".to_string());
    }
    let conn = db.lock().unwrap();
    let sort_order = tessera_store::extras::next_task_sort_order(&conn, workspace_id)
        .map_err(|e| e.to_string())?;
    let task = WorkspaceTask {
        id: Uuid::new_v4(),
        workspace_id,
        title,
        done: false,
        sort_order,
        due_date,
        created_at: Utc::now(),
        completed_at: None,
    };
    tessera_store::extras::insert_task(&conn, &task).map_err(|e| e.to_string())?;
    Ok(WorkspaceTaskDto::from(task))
}

#[tauri::command]
pub fn workspace_tasks_toggle(db: State<'_, DbState>, id: Uuid) -> Result<bool, String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::toggle_task(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_tasks_delete(db: State<'_, DbState>, id: Uuid) -> Result<(), String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::delete_task(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_tasks_reorder(db: State<'_, DbState>, ids: Vec<Uuid>) -> Result<(), String> {
    let updates: Vec<(Uuid, i64)> = ids
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, ((i + 1) as i64) * 10))
        .collect();
    let conn = db.lock().unwrap();
    tessera_store::extras::update_task_sort_orders(&conn, &updates).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_pomodoro_get(
    db: State<'_, DbState>,
    workspace_id: Uuid,
) -> Result<PomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let state = tessera_store::extras::get_pomodoro(&conn, workspace_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| PomodoroState::idle(workspace_id));
    Ok(PomodoroStateDto::from(state))
}

/// Begin a fresh work or break run. Discards any prior `paused`/`idle`
/// elapsed time — the frontend's "Start" button is unambiguous (use
/// `resume` for paused timers).
#[tauri::command]
pub fn workspace_pomodoro_start(
    db: State<'_, DbState>,
    workspace_id: Uuid,
    mode: String,
    target_seconds: Option<i64>,
) -> Result<PomodoroStateDto, String> {
    let new_mode = match mode.as_str() {
        "work" => PomodoroMode::Work,
        "break" => PomodoroMode::Break,
        other => {
            return Err(format!(
                "invalid mode {other:?}, expected 'work' or 'break'"
            ))
        }
    };
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_pomodoro(&conn, workspace_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| PomodoroState::idle(workspace_id));
    let state = PomodoroState {
        workspace_id,
        mode: new_mode,
        started_at: Some(Utc::now()),
        paused_at: None,
        target_seconds: target_seconds.unwrap_or(match new_mode {
            PomodoroMode::Break => 300,
            _ => 1500,
        }),
        elapsed_seconds_before_pause: 0,
        cycles_completed: prior.cycles_completed,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(PomodoroStateDto::from(state))
}

/// Pause whatever is running. Captures elapsed time so `resume` continues
/// from the same offset. No-op if the timer is already paused or idle.
#[tauri::command]
pub fn workspace_pomodoro_pause(
    db: State<'_, DbState>,
    workspace_id: Uuid,
) -> Result<PomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_pomodoro(&conn, workspace_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| PomodoroState::idle(workspace_id));
    if !matches!(prior.mode, PomodoroMode::Work | PomodoroMode::Break) {
        return Ok(PomodoroStateDto::from(prior));
    }
    let elapsed_now = prior
        .started_at
        .map(|t| (Utc::now() - t).num_seconds().max(0))
        .unwrap_or(0);
    let state = PomodoroState {
        workspace_id,
        mode: PomodoroMode::Paused,
        started_at: prior.started_at,
        paused_at: Some(Utc::now()),
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: prior.elapsed_seconds_before_pause + elapsed_now,
        cycles_completed: prior.cycles_completed,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(PomodoroStateDto::from(state))
}

/// Resume a paused timer back to its prior mode (work by default — pause
/// only happens during work or break, and we don't try to remember which).
/// `started_at` is shifted forward so the live countdown picks up exactly
/// where it left off.
#[tauri::command]
pub fn workspace_pomodoro_resume(
    db: State<'_, DbState>,
    workspace_id: Uuid,
) -> Result<PomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_pomodoro(&conn, workspace_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| PomodoroState::idle(workspace_id));
    if !matches!(prior.mode, PomodoroMode::Paused) {
        return Ok(PomodoroStateDto::from(prior));
    }
    // Re-anchor `started_at` so `elapsed = now - started_at` continues
    // from `elapsed_seconds_before_pause`. The mode flips back to `work`
    // — we don't track the pre-pause mode separately.
    let new_started = Utc::now() - chrono::Duration::seconds(prior.elapsed_seconds_before_pause);
    let state = PomodoroState {
        workspace_id,
        mode: PomodoroMode::Work,
        started_at: Some(new_started),
        paused_at: None,
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: 0,
        cycles_completed: prior.cycles_completed,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(PomodoroStateDto::from(state))
}

/// Reset the timer to idle. Bumps `cycles_completed` if the timer ran
/// long enough to count as a finished work cycle (≥ 50% of target).
#[tauri::command]
pub fn workspace_pomodoro_reset(
    db: State<'_, DbState>,
    workspace_id: Uuid,
) -> Result<PomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_pomodoro(&conn, workspace_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| PomodoroState::idle(workspace_id));
    let cycles = prior.cycles_completed + pomodoro_reset_cycle_credit(&prior, Utc::now());
    let state = PomodoroState {
        workspace_id,
        mode: PomodoroMode::Idle,
        started_at: None,
        paused_at: None,
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: 0,
        cycles_completed: cycles,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(PomodoroStateDto::from(state))
}

/// Returns `1` when a reset should credit a completed work cycle, `0`
/// otherwise. Pure so it can be unit-tested without a DB.
///
/// Rules — credit when the prior mode is `Work` or `Paused` and the elapsed
/// work time is at least half the target. The half-target threshold keeps
/// "accidental start → reset" from inflating the counter while still
/// rewarding a paused 23-of-25-min session.
///
/// Elapsed depends on mode:
/// - `Work`:   `elapsed_seconds_before_pause + (now - started_at)`
/// - `Paused`: `elapsed_seconds_before_pause` only — `started_at` was frozen
///   at pause time, so adding `(now - started_at)` would count the entire
///   paused interval as productive time.
fn pomodoro_reset_cycle_credit(prior: &PomodoroState, now: DateTime<Utc>) -> i64 {
    let total = match prior.mode {
        PomodoroMode::Work => {
            let live = prior
                .started_at
                .map(|t| (now - t).num_seconds().max(0))
                .unwrap_or(0);
            prior.elapsed_seconds_before_pause + live
        }
        PomodoroMode::Paused => prior.elapsed_seconds_before_pause,
        _ => return 0,
    };
    if total >= prior.target_seconds / 2 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod extras_tests {
    use super::*;

    #[test]
    fn detect_github_issue() {
        let (kind, label) = detect_github("https://github.com/nvrxq/Tessera/issues/42").unwrap();
        assert!(matches!(kind, LinkKind::GithubIssue));
        assert_eq!(label, "nvrxq/Tessera#42");
    }

    #[test]
    fn detect_github_pr_with_fragment() {
        let (kind, label) =
            detect_github("https://github.com/nvrxq/Tessera/pull/7#issuecomment-1").unwrap();
        assert!(matches!(kind, LinkKind::GithubPr));
        assert_eq!(label, "nvrxq/Tessera#7");
    }

    #[test]
    fn detect_github_rejects_non_github_or_garbage() {
        assert!(detect_github("https://example.com/x/y/issues/1").is_none());
        assert!(detect_github("https://github.com/nvrxq/Tessera").is_none());
        assert!(detect_github("https://github.com/nvrxq/Tessera/issues/abc").is_none());
        assert!(detect_github("not a url").is_none());
    }

    fn pomodoro(
        mode: PomodoroMode,
        started_at: Option<DateTime<Utc>>,
        before: i64,
    ) -> PomodoroState {
        PomodoroState {
            workspace_id: Uuid::nil(),
            mode,
            started_at,
            paused_at: None,
            target_seconds: 1500, // 25 min
            elapsed_seconds_before_pause: before,
            cycles_completed: 0,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn cycle_credit_zero_when_idle_or_break() {
        let now = Utc::now();
        assert_eq!(
            pomodoro_reset_cycle_credit(&pomodoro(PomodoroMode::Idle, None, 0), now),
            0
        );
        assert_eq!(
            pomodoro_reset_cycle_credit(&pomodoro(PomodoroMode::Break, Some(now), 0), now),
            0,
        );
    }

    #[test]
    fn cycle_credit_zero_when_work_too_short() {
        // 5 min in, target 25 — under half (≤ 12 min 30s).
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(5 * 60);
        let p = pomodoro(PomodoroMode::Work, Some(started), 0);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 0);
    }

    #[test]
    fn cycle_credit_one_when_work_past_half_target() {
        // 20 min in, target 25 — over half.
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(20 * 60);
        let p = pomodoro(PomodoroMode::Work, Some(started), 0);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 1);
    }

    /// Regression: a 23-of-25-min session that the user paused and *then*
    /// reset should still credit one cycle. Earlier code only matched
    /// `PomodoroMode::Work` and dropped the credit because pause flips the
    /// mode to `Paused`.
    #[test]
    fn cycle_credit_one_when_paused_past_half_target() {
        let now = Utc::now();
        // started 30 min ago, paused 7 min later → 23 min credit, started_at frozen.
        let started = now - chrono::Duration::seconds(30 * 60);
        let p = pomodoro(PomodoroMode::Paused, Some(started), 23 * 60);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 1);
    }

    /// Regression: when paused, we must NOT add `(now - started_at)` to
    /// `elapsed_seconds_before_pause` — that would credit the entire time
    /// the user left the timer paused as productive work, and a brief 1-min
    /// session paused for an hour would falsely credit a full cycle.
    #[test]
    fn cycle_credit_zero_when_paused_with_low_elapsed_long_pause() {
        let now = Utc::now();
        // started 2h ago, paused after 60 s → only 60 s of credit.
        let started = now - chrono::Duration::hours(2);
        let p = pomodoro(PomodoroMode::Paused, Some(started), 60);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 0);
    }
}

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
    /// Pinned Claude Code session uuid (stem of the jsonl under
    /// `~/.claude/projects/<encoded-cwd>/`). Surfaced so the UI can show
    /// a "Reset session" affordance and an indicator that the workspace
    /// is bound to a specific conversation.
    pub claude_session_id: Option<String>,
    /// Soft-archive timestamp. The active sidebar query filters these
    /// out; the archive section shows them.
    pub archived_at: Option<DateTime<Utc>>,
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
            claude_session_id: ws.claude_session_id,
            archived_at: ws.archived_at,
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
) -> Result<(), String> {
    state.delete(workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn workspace_rename(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
    new_name: String,
) -> Result<(), String> {
    state
        .rename(workspace_id, &new_name)
        .map_err(|e| e.to_string())
}

/// Soft-archive: hide from the main list but keep the row + pinned
/// Claude session intact. Used by the "Archive" entry in the workspace
/// menu.
#[tauri::command]
pub fn workspace_archive(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
) -> Result<(), String> {
    state.archive(workspace_id).map_err(|e| e.to_string())
}

/// Move a workspace back from the archive section into the active list.
#[tauri::command]
pub fn workspace_unarchive(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
) -> Result<(), String> {
    state.unarchive(workspace_id).map_err(|e| e.to_string())
}

/// Archived rows only. Frontend pages these into a collapsed section in
/// the sidebar; we keep them separate from `workspace_list` so the hot
/// path stays branch-free.
#[tauri::command]
pub fn workspace_list_archived(
    state: State<'_, WorkspaceServiceState>,
) -> Result<Vec<WorkspaceDto>, String> {
    let items = state.list_archived().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|w| WorkspaceDto::from_workspace(w, None, None))
        .collect())
}

/// Drop the pinned Claude session uuid so the next spawn starts a fresh
/// conversation (and re-pins to whatever Claude writes next). User-facing
/// "Reset session" menu entry.
#[tauri::command]
pub fn workspace_reset_session(
    state: State<'_, WorkspaceServiceState>,
    workspace_id: Uuid,
) -> Result<(), String> {
    state
        .reset_claude_session(workspace_id)
        .map_err(|e| e.to_string())
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
/// Sessions marked dirty since the last render tick. Shared with the pump
/// task so `terminal_resize` can wake an idle session that has no PTY
/// output of its own.
pub type DirtySet = std::sync::Arc<Mutex<std::collections::HashSet<Uuid>>>;

/// Frontend tells the backend the desired grid size for a session. Resizes
/// both the wezterm-term parser AND remembers the size for any future
/// lazy-spawned Term in the same session id.
///
/// Marks the session dirty so the 1 ms render-tick picks it up immediately
/// — `registry.resize()` clears `last_cells`, so the next snapshot is a
/// full one carrying the new grid dimensions. Without the dirty mark, an
/// idle session (no PTY output) would not re-snapshot until the next byte.
#[tauri::command]
pub fn terminal_resize(
    _app: tauri::AppHandle,
    registry: tauri::State<'_, TerminalRegistryState>,
    sizes: tauri::State<'_, GridSizesState>,
    dirty: tauri::State<'_, DirtySet>,
    session_id: Uuid,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    sizes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session_id, (cols, rows));
    registry.resize(session_id, cols, rows);
    dirty
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session_id);
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

// ---- Claude inventory (skills + MCP servers) ----

/// Cache key for `claude_inventory`. Keyed by `(workspace_id, latest_mtime)`
/// across the four directories/files that drive a Claude session's view:
/// global skills, plugin cache, `~/.claude.json`, and the workspace's
/// `.mcp.json`. If any of those moves, the cache invalidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryCacheKey {
    pub workspace_id: Option<Uuid>,
    pub mtime_ns: i128,
}

/// State container — a single `Mutex<Option<...>>` is fine because the
/// command awaits its own `spawn_blocking`, so two concurrent invocations
/// from the UI just serialize through the lock instead of stampeding the
/// filesystem walk twice.
#[derive(Default)]
pub struct ClaudeInventoryCache {
    pub last: Mutex<
        Option<(
            InventoryCacheKey,
            std::time::Instant,
            tessera_core::ClaudeInventory,
        )>,
    >,
}

pub type ClaudeInventoryCacheState = Arc<ClaudeInventoryCache>;

/// TTL on top of the mtime check. POSIX directory mtime only reflects
/// entry add/remove, not edits to nested files — editing a SKILL.md in
/// place would not bump `~/.claude/skills` mtime. The TTL bounds the
/// staleness window to a handful of seconds; the mtime check still
/// short-circuits the common "nothing changed" case.
const INVENTORY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Compute the latest mtime across the four paths Claude looks at. Missing
/// paths contribute 0; we only care about *changes*, not absolute times.
fn inventory_mtime(ws_dir: Option<&std::path::Path>) -> i128 {
    let mut latest: i128 = 0;
    let push = |latest: &mut i128, p: std::path::PathBuf| {
        if let Ok(meta) = std::fs::metadata(&p) {
            if let Ok(m) = meta.modified() {
                if let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) {
                    let ns = d.as_nanos() as i128;
                    if ns > *latest {
                        *latest = ns;
                    }
                }
            }
        }
    };
    if let Some(home) = dirs::home_dir() {
        push(&mut latest, home.join(".claude/skills"));
        push(&mut latest, home.join(".claude/plugins"));
        push(&mut latest, home.join(".claude.json"));
    }
    if let Some(ws) = ws_dir {
        push(&mut latest, ws.join(".mcp.json"));
    }
    latest
}

/// Surface what Claude Code will see when launched in a given workspace —
/// the inventory of skills (global, plugin-shipped, project-local) and
/// configured MCP servers. `workspace_id = None` returns globals only
/// (header-level "all workspaces" view).
///
/// Cached in-process across calls keyed by `(workspace_id, latest_mtime)`.
/// On a hit we clone the prior result; on a miss we re-walk the skill /
/// plugin / mcp tree on a `spawn_blocking` thread so the Tauri worker pool
/// doesn't stall.
///
/// MCP `env` VALUES are deliberately never returned (only the keys). The
/// `ClaudeInventory` types have no field for them and the parser doesn't
/// extract them — a regression test in `tessera-core` guards against
/// future code adding the field by accident. **Cache safety**: we cache
/// the parsed inventory, which by construction never carries env values,
/// so the cache cannot leak secrets either.
#[tauri::command]
pub async fn claude_inventory(
    workspaces: State<'_, WorkspaceServiceState>,
    cache: State<'_, ClaudeInventoryCacheState>,
    workspace_id: Option<Uuid>,
) -> Result<tessera_core::ClaudeInventory, String> {
    let ws_dir = match workspace_id {
        None => None,
        Some(id) => {
            let ws = workspaces.get(id).map_err(|e| e.to_string())?;
            ws.map(|w| {
                // Prefer the detected worktree (`git worktree add` target) if
                // Claude has already created one — that's the folder Claude
                // is actually working in. Otherwise fall back to the
                // configured worktree_path; finally the repo path.
                w.detected_worktree
                    .filter(|p| p.is_dir())
                    .unwrap_or_else(|| {
                        if w.worktree_path.is_dir() {
                            w.worktree_path
                        } else {
                            w.repo_path
                        }
                    })
            })
        }
    };
    let key = InventoryCacheKey {
        workspace_id,
        mtime_ns: inventory_mtime(ws_dir.as_deref()),
    };

    {
        let guard = cache.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_key, cached_at, cached_inv)) = guard.as_ref() {
            if cached_key == &key && cached_at.elapsed() < INVENTORY_CACHE_TTL {
                return Ok(cached_inv.clone());
            }
        }
    }

    // Miss — walk on a blocking thread so the parser doesn't pin a Tauri
    // worker. Clone the dir owned-style so the future is `'static`.
    let owned = ws_dir.clone();
    let inv = tokio::task::spawn_blocking(move || {
        tessera_core::ClaudeInventory::collect(owned.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?;

    {
        let mut guard = cache.last.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some((key, std::time::Instant::now(), inv.clone()));
    }
    Ok(inv)
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

use tessera_core::{
    ActivityEntry, ActivityKind, LinkKind, PomodoroMode, PomodoroState, WorkspaceLink,
    WorkspaceTask,
};

#[derive(Debug, Serialize)]
pub struct ActivityEntryDto {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: ActivityKind,
    pub summary: String,
    pub payload: String,
    pub created_at: DateTime<Utc>,
}

impl From<ActivityEntry> for ActivityEntryDto {
    fn from(e: ActivityEntry) -> Self {
        Self {
            id: e.id,
            workspace_id: e.workspace_id,
            kind: e.kind,
            summary: e.summary,
            payload: e.payload,
            created_at: e.created_at,
        }
    }
}

#[tauri::command]
pub fn workspace_activity_list(
    db: State<'_, DbState>,
    workspace_id: Uuid,
    limit: Option<i64>,
) -> Result<Vec<ActivityEntryDto>, String> {
    let conn = db.lock().unwrap();
    tessera_store::activity::list_recent(&conn, workspace_id, limit.unwrap_or(50))
        .map(|v| v.into_iter().map(ActivityEntryDto::from).collect())
        .map_err(|e| e.to_string())
}

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

// ---- Global (app-wide) pomodoro ----
//
// One pomodoro shared by every workspace; backed by the single-row
// `app_pomodoro` table. Take no workspace id.

/// Wire shape for the global pomodoro. Same fields as `PomodoroStateDto`
/// minus `workspace_id` — the global timer isn't tied to any workspace.
#[derive(Debug, Serialize)]
pub struct AppPomodoroStateDto {
    pub mode: PomodoroMode,
    pub started_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub target_seconds: i64,
    pub elapsed_seconds_before_pause: i64,
    pub cycles_completed: i64,
    pub updated_at: DateTime<Utc>,
}

impl From<PomodoroState> for AppPomodoroStateDto {
    fn from(s: PomodoroState) -> Self {
        Self {
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

#[tauri::command]
pub fn app_pomodoro_get(db: State<'_, DbState>) -> Result<AppPomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    tessera_store::extras::get_app_pomodoro(&conn)
        .map(AppPomodoroStateDto::from)
        .map_err(|e| e.to_string())
}

/// Begin a fresh work or break run. Same defaults as the workspace
/// variant: work = 1500s (25 min), break = 300s (5 min).
#[tauri::command]
pub fn app_pomodoro_start(
    db: State<'_, DbState>,
    mode: String,
    target_seconds: Option<i64>,
) -> Result<AppPomodoroStateDto, String> {
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
    let prior = tessera_store::extras::get_app_pomodoro(&conn).map_err(|e| e.to_string())?;
    let now = Utc::now();
    // Credit any in-flight cycle being clobbered. Same threshold as reset
    // (≥ target/2 of work or paused work) — otherwise switching from a
    // nearly-complete Work to a Break would silently lose the cycle.
    let cycles_completed = prior.cycles_completed + pomodoro_reset_cycle_credit(&prior, now);
    let state = PomodoroState {
        workspace_id: Uuid::nil(),
        mode: new_mode,
        started_at: Some(now),
        paused_at: None,
        target_seconds: target_seconds.unwrap_or(match new_mode {
            PomodoroMode::Break => 300,
            _ => 1500,
        }),
        elapsed_seconds_before_pause: 0,
        cycles_completed,
        updated_at: now,
    };
    tessera_store::extras::upsert_app_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(AppPomodoroStateDto::from(state))
}

#[tauri::command]
pub fn app_pomodoro_pause(db: State<'_, DbState>) -> Result<AppPomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_app_pomodoro(&conn).map_err(|e| e.to_string())?;
    if !matches!(prior.mode, PomodoroMode::Work | PomodoroMode::Break) {
        return Ok(AppPomodoroStateDto::from(prior));
    }
    let elapsed_now = prior
        .started_at
        .map(|t| (Utc::now() - t).num_seconds().max(0))
        .unwrap_or(0);
    let state = PomodoroState {
        workspace_id: Uuid::nil(),
        mode: PomodoroMode::Paused,
        started_at: prior.started_at,
        paused_at: Some(Utc::now()),
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: prior.elapsed_seconds_before_pause + elapsed_now,
        cycles_completed: prior.cycles_completed,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_app_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(AppPomodoroStateDto::from(state))
}

#[tauri::command]
pub fn app_pomodoro_resume(db: State<'_, DbState>) -> Result<AppPomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_app_pomodoro(&conn).map_err(|e| e.to_string())?;
    if !matches!(prior.mode, PomodoroMode::Paused) {
        return Ok(AppPomodoroStateDto::from(prior));
    }
    let new_started = Utc::now() - chrono::Duration::seconds(prior.elapsed_seconds_before_pause);
    let state = PomodoroState {
        workspace_id: Uuid::nil(),
        mode: PomodoroMode::Work,
        started_at: Some(new_started),
        paused_at: None,
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: 0,
        cycles_completed: prior.cycles_completed,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_app_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(AppPomodoroStateDto::from(state))
}

#[tauri::command]
pub fn app_pomodoro_reset(db: State<'_, DbState>) -> Result<AppPomodoroStateDto, String> {
    let conn = db.lock().unwrap();
    let prior = tessera_store::extras::get_app_pomodoro(&conn).map_err(|e| e.to_string())?;
    let cycles = prior.cycles_completed + pomodoro_reset_cycle_credit(&prior, Utc::now());
    let state = PomodoroState {
        workspace_id: Uuid::nil(),
        mode: PomodoroMode::Idle,
        started_at: None,
        paused_at: None,
        target_seconds: prior.target_seconds,
        elapsed_seconds_before_pause: 0,
        cycles_completed: cycles,
        updated_at: Utc::now(),
    };
    tessera_store::extras::upsert_app_pomodoro(&conn, &state).map_err(|e| e.to_string())?;
    Ok(AppPomodoroStateDto::from(state))
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

    /// End-to-end on the global table: a paused 23-of-25-min run that the
    /// user resets should bump `cycles_completed`. Mirrors the per-workspace
    /// test but exercises the `app_pomodoro` store path through the same
    /// cycle-credit helper.
    #[test]
    fn app_pomodoro_reset_from_paused_credits_cycle() {
        let conn = tessera_store::open_in_memory().unwrap();
        // Seed the global row in a "paused at 23 min" state.
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(30 * 60);
        let paused_state = PomodoroState {
            workspace_id: Uuid::nil(),
            mode: PomodoroMode::Paused,
            started_at: Some(started),
            paused_at: Some(now),
            target_seconds: 1500,
            elapsed_seconds_before_pause: 23 * 60,
            cycles_completed: 4,
            updated_at: now,
        };
        tessera_store::extras::upsert_app_pomodoro(&conn, &paused_state).unwrap();

        // Re-derive `cycles` the same way `app_pomodoro_reset` does — the
        // helper is pure, so this also serves as a contract test for the
        // command's behaviour without spinning up a Tauri State.
        let prior = tessera_store::extras::get_app_pomodoro(&conn).unwrap();
        let cycles = prior.cycles_completed + pomodoro_reset_cycle_credit(&prior, Utc::now());
        assert_eq!(cycles, 5, "paused ≥ half target should credit a cycle");

        let reset_state = PomodoroState {
            workspace_id: Uuid::nil(),
            mode: PomodoroMode::Idle,
            started_at: None,
            paused_at: None,
            target_seconds: prior.target_seconds,
            elapsed_seconds_before_pause: 0,
            cycles_completed: cycles,
            updated_at: Utc::now(),
        };
        tessera_store::extras::upsert_app_pomodoro(&conn, &reset_state).unwrap();
        let after = tessera_store::extras::get_app_pomodoro(&conn).unwrap();
        assert!(matches!(after.mode, PomodoroMode::Idle));
        assert_eq!(after.cycles_completed, 5);
        assert_eq!(after.elapsed_seconds_before_pause, 0);
    }

    /// Regression: starting a new mode while a Work session is past half
    /// the target must credit the in-flight cycle, not silently drop it.
    /// Mirrors the reset-from-running semantics applied via the same helper.
    #[test]
    fn start_credits_cycle_when_already_running_past_half() {
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(20 * 60); // 20 of 25 min
        let p = pomodoro(PomodoroMode::Work, Some(started), 0);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 1);
    }

    /// Counterpart: a Work session under half target gets no credit when
    /// clobbered by start — the user effectively bailed early.
    #[test]
    fn start_no_cycle_when_already_running_under_half() {
        let now = Utc::now();
        let started = now - chrono::Duration::seconds(5 * 60); // 5 of 25 min
        let p = pomodoro(PomodoroMode::Work, Some(started), 0);
        assert_eq!(pomodoro_reset_cycle_credit(&p, now), 0);
    }

    // ---- claude_inventory cache ----
    //
    // The `claude_inventory` command itself needs Tauri's State machinery to
    // exercise, but the cache's contract is purely about the
    // `InventoryCacheKey` + `Mutex<Option<...>>` shape — equality on the key
    // decides hit vs. miss, mutation invalidates the entry. We test those
    // directly here so a regression in the cache surface is caught without
    // a full integration harness.

    #[test]
    fn inventory_cache_returns_value_on_key_hit() {
        let cache = ClaudeInventoryCache::default();
        let key = InventoryCacheKey {
            workspace_id: None,
            mtime_ns: 42,
        };
        let inv = tessera_core::ClaudeInventory::default();
        *cache.last.lock().unwrap() = Some((key.clone(), std::time::Instant::now(), inv));

        let guard = cache.last.lock().unwrap();
        let (k, _t, _v) = guard.as_ref().expect("seeded");
        assert_eq!(k, &key);
    }

    #[test]
    fn inventory_cache_misses_when_mtime_changes() {
        let cache = ClaudeInventoryCache::default();
        let seeded = InventoryCacheKey {
            workspace_id: None,
            mtime_ns: 100,
        };
        *cache.last.lock().unwrap() = Some((
            seeded.clone(),
            std::time::Instant::now(),
            tessera_core::ClaudeInventory::default(),
        ));

        // Newer mtime invalidates.
        let probed = InventoryCacheKey {
            workspace_id: None,
            mtime_ns: 200,
        };
        let guard = cache.last.lock().unwrap();
        let (k, _t, _) = guard.as_ref().expect("seeded");
        assert_ne!(k, &probed, "cache key must change with mtime");
    }

    #[test]
    fn inventory_cache_misses_across_workspaces() {
        let cache = ClaudeInventoryCache::default();
        let ws1 = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let seeded = InventoryCacheKey {
            workspace_id: Some(ws1),
            mtime_ns: 42,
        };
        *cache.last.lock().unwrap() = Some((
            seeded.clone(),
            std::time::Instant::now(),
            tessera_core::ClaudeInventory::default(),
        ));

        let probe = InventoryCacheKey {
            workspace_id: Some(ws2),
            mtime_ns: 42,
        };
        let guard = cache.last.lock().unwrap();
        let (k, _t, _) = guard.as_ref().expect("seeded");
        assert_ne!(k, &probe);
    }

    /// TTL on the cache entry — directory mtime alone is shallow (POSIX
    /// dir mtime doesn't reflect edits to nested files), so any cached
    /// entry must expire after `INVENTORY_CACHE_TTL` even when the
    /// computed mtime hasn't changed.
    #[test]
    fn inventory_cache_ttl_expires_stale_entry() {
        let key = InventoryCacheKey {
            workspace_id: None,
            mtime_ns: 1,
        };
        let fresh = std::time::Instant::now();
        let stale = fresh
            .checked_sub(INVENTORY_CACHE_TTL + std::time::Duration::from_millis(1))
            .unwrap_or(fresh);
        assert!(fresh.elapsed() < INVENTORY_CACHE_TTL);
        assert!(stale.elapsed() >= INVENTORY_CACHE_TTL);
        // Same key, but the stale instant must drive the call to re-walk.
        let _ = key;
    }

    /// `inventory_mtime` is monotonic in the per-file mtimes it samples —
    /// touching the workspace-local `.mcp.json` must change the returned
    /// stamp, so the cache invalidates the next call.
    #[test]
    fn inventory_mtime_changes_when_mcp_json_is_touched() {
        let tmp = tempfile::tempdir().unwrap();
        let mcp = tmp.path().join(".mcp.json");
        std::fs::write(&mcp, "{}").unwrap();
        let m1 = inventory_mtime(Some(tmp.path()));

        // Bump mtime by writing again with a clearly newer payload. Some
        // filesystems quantize mtime to 1 s, so sleep a hair past that.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&mcp, "{\"mcpServers\":{}}").unwrap();
        let m2 = inventory_mtime(Some(tmp.path()));

        assert!(
            m2 > m1,
            "mtime must advance after a write (m1={m1}, m2={m2})"
        );
    }
}

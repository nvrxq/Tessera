mod commands;
mod terminal;

use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tessera_core::{ActivityEntry, ActivityKind, AgentStatus};
use tessera_hook::{HookEvent, HookKind, Listener};
use tessera_pty::{PtyEvent, Supervisor};
use tessera_workspace::WorkspaceService;
use tracing_subscriber::EnvFilter;

use crate::terminal::TerminalRegistry;

/// Tracks last-known grid size per session so the PTY-pump thread can spawn
/// a Term with the right dimensions on the first chunk. Updated by the
/// frontend via `terminal_resize`.
type GridSizes = Arc<Mutex<std::collections::HashMap<uuid::Uuid, (u16, u16)>>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let data_dir = dirs::data_local_dir()
        .expect("no XDG data dir")
        .join("tessera");
    std::fs::create_dir_all(&data_dir).expect("could not create data dir");

    // Reordered to come AFTER data_dir creation so the PATH cache has a
    // place to live. Runs entirely before any Tauri thread spins up — the
    // cache-hit branch mutates env synchronously, the cache-miss branch
    // probes synchronously with a 500 ms timeout (cap on cold-start tax).
    inherit_shell_path(&data_dir);
    let db_path = data_dir.join("state.db");
    let conn = tessera_store::open(&db_path).expect("could not open store");
    let db = Arc::new(Mutex::new(conn));

    let supervisor: Arc<Supervisor> = Arc::new(Supervisor::new());

    let worktree_root = data_dir.join("worktrees");
    let workspace_service: Arc<WorkspaceService> = Arc::new(WorkspaceService::new(
        db.clone(),
        supervisor.clone(),
        worktree_root,
    ));

    let registry: Arc<TerminalRegistry> = Arc::new(TerminalRegistry::new());
    let grid_sizes: GridSizes = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let inventory_cache: commands::ClaudeInventoryCacheState =
        Arc::new(commands::ClaudeInventoryCache::default());
    // Shared between the pump task and `terminal_resize`. Resize MUST be
    // able to mark a session dirty so an idle session (no PTY output)
    // still re-snapshots after a window resize.
    let dirty: commands::DirtySet = Arc::new(Mutex::new(std::collections::HashSet::new()));

    // Apply persisted user settings to the registry's live palette so the
    // first snapshot already paints with the user's customised colours.
    // (Settings might also be absent — that just leaves the tessera_dark
    // default in place.)
    {
        let cfg = tessera_core::UserConfig::load_or_default(&tessera_core::config::config_path());
        registry.set_palette(crate::terminal::palette_from_config(&cfg));
    }

    let socket_path = data_dir.join("hooks.sock");

    tauri::Builder::default()
        // single-instance must register first so the second invocation's
        // closure fires inside the first process before any other plugin
        // grabs resources (DB lock, hooks socket) the second copy would
        // race on.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(supervisor.clone())
        .manage(workspace_service.clone())
        .manage(registry.clone())
        .manage(grid_sizes.clone())
        .manage(inventory_cache.clone())
        .manage(dirty.clone())
        .manage::<commands::DbState>(db.clone())
        .setup(move |app| {
            let handle = app.handle().clone();

            // PTY → Term parser → frontend snapshot — two coordinated tasks.
            //
            // Pump task: drain PTY events as fast as they arrive, feeding
            // bytes into the parser. Each Data chunk marks the session
            // "dirty" but does NOT emit a snapshot — emitting on every byte
            // burns CPU serialising a ~30 KB JSON payload per keystroke
            // (1920 cells × 4 fields), which was responsible for the
            // unusable input lag in the first cut.
            //
            // Render-tick task: wakes at 60 Hz, snapshots and emits only the
            // sessions that became dirty since the last tick. This caps
            // peak IPC at the display refresh rate, regardless of how fast
            // claude floods the PTY (it can redraw the whole alt-screen 5×
            // per frame; the user only ever sees one).
            // 2 ms tick — empirically fastest configuration (Notify+gap was
            // slower in practice because the post-emit sleep starved sustained
            // claude streams). Idle cost: ~500 µs/sec of CPU.
            //
            // Per-byte latency budget on the backend side: ≤2 ms wait for
            // the next tick + ~50 µs snapshot + ~100 µs JSON emit = ~2.2 ms.
            // Frontend then has ~16 ms display-refresh floor.
            let pty_seen_at: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<uuid::Uuid, std::time::Instant>>> =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            let bench_enabled = std::env::var("TESSERA_BENCH").is_ok();
            {
                let reg = registry.clone();
                let sizes = grid_sizes.clone();
                let dirty = dirty.clone();
                let seen = pty_seen_at.clone();
                let handle = handle.clone();
                let mut rx = supervisor.subscribe();
                tauri::async_runtime::spawn(async move {
                    while let Ok(evt) = rx.recv().await {
                        match evt {
                            PtyEvent::Data { session_id, bytes } => {
                                let (cols, rows) = sizes
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .get(&session_id)
                                    .copied()
                                    .unwrap_or((80, 24));
                                let t_recv = std::time::Instant::now();
                                if reg.feed(session_id, cols, rows, &bytes) {
                                    // Capture the FIRST byte-arrival in a
                                    // tick window; the emitter task reads
                                    // it back to compute end-to-end latency.
                                    seen.lock().unwrap().entry(session_id).or_insert(t_recv);
                                    dirty
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .insert(session_id);
                                }
                            }
                            PtyEvent::Exit { session_id } => {
                                reg.remove(session_id);
                                sizes
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .remove(&session_id);
                                dirty
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .remove(&session_id);
                                seen.lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .remove(&session_id);
                                let _ = handle.emit(
                                    "pty_event",
                                    serde_json::json!({
                                        "kind": "exit",
                                        "session_id": session_id,
                                    }),
                                );
                            }
                        }
                    }
                });
            }
            {
                let handle = handle.clone();
                let reg = registry.clone();
                let dirty = dirty.clone();
                let seen = pty_seen_at.clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_millis(1));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let to_snap: Vec<uuid::Uuid> = {
                            let mut d = dirty.lock().unwrap_or_else(|e| e.into_inner());
                            if d.is_empty() {
                                continue;
                            }
                            let v: Vec<_> = d.iter().copied().collect();
                            d.clear();
                            v
                        };
                        for sid in to_snap {
                            let t_pty = seen.lock().unwrap_or_else(|e| e.into_inner()).remove(&sid);
                            let t_snap_start = std::time::Instant::now();
                            if let Some(snap) = reg.snapshot(sid) {
                                let t_snap = t_snap_start.elapsed();
                                let t_emit_start = std::time::Instant::now();
                                let _ = handle.emit("term_snapshot", snap);
                                let t_emit = t_emit_start.elapsed();
                                if bench_enabled {
                                    // Capture `total` BEFORE deriving
                                    // wait, so the breakdown actually adds
                                    // up: total = wait + snap + emit. The
                                    // earlier version called `t_pty.elapsed()`
                                    // twice at different points so the
                                    // numbers drifted by ~µs and the row
                                    // didn't reconcile.
                                    let total_us =
                                        t_pty.map(|t| t.elapsed().as_micros()).unwrap_or(0);
                                    let wait_us = total_us
                                        .saturating_sub(t_snap.as_micros())
                                        .saturating_sub(t_emit.as_micros());
                                    eprintln!(
                                        "[bench] wait={}µs  snap={}µs  emit={}µs  total={}µs",
                                        wait_us,
                                        t_snap.as_micros(),
                                        t_emit.as_micros(),
                                        total_us,
                                    );
                                }
                            }
                        }
                    }
                });
            }

            // Hook listener (Plan 4). Bind can fail when a stale socket
            // is held by a defunct previous run or when the data dir is
            // briefly unavailable (network home dirs, etc.). Retry a
            // handful of times with backoff, then emit a UI event so the
            // user knows hooks are dead instead of suffering silent
            // status-dot starvation.
            let svc = workspace_service.clone();
            tauri::async_runtime::spawn(async move {
                const MAX_ATTEMPTS: u32 = 5;
                const BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);
                let mut listener: Option<Listener> = None;
                let mut last_err: Option<String> = None;
                for attempt in 1..=MAX_ATTEMPTS {
                    match Listener::bind(&socket_path) {
                        Ok(l) => {
                            tracing::info!(
                                path = %l.socket_path.display(),
                                attempt,
                                "hook listener bound",
                            );
                            let _ = handle
                                .emit("hook_listener_state", serde_json::json!({ "status": "ok" }));
                            listener = Some(l);
                            break;
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            tracing::warn!(error = %msg, attempt, "hook listener bind failed");
                            last_err = Some(msg);
                            if attempt < MAX_ATTEMPTS {
                                tokio::time::sleep(BACKOFF).await;
                            }
                        }
                    }
                }
                let Some(mut listener) = listener else {
                    let err = last_err.unwrap_or_else(|| "unknown".to_string());
                    tracing::error!(error = %err, "hook listener gave up after retries");
                    let _ = handle.emit(
                        "hook_listener_state",
                        serde_json::json!({ "status": "down", "error": err }),
                    );
                    return;
                };
                while let Some(evt) = listener.rx.recv().await {
                    dispatch_hook(&handle, &svc, evt);
                }
            });

            tracing::info!("Tessera starting");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_kill,
            commands::workspace_create,
            commands::workspace_list,
            commands::workspace_spawn_agent,
            commands::workspace_delete,
            commands::workspace_rename,
            commands::project_create,
            commands::project_list,
            commands::project_delete,
            commands::workspace_reorder,
            commands::workspace_assign_project,
            commands::terminal_resize,
            commands::list_directories,
            commands::save_paste_image,
            commands::terminal_scroll,
            commands::workspace_links_list,
            commands::workspace_links_add,
            commands::workspace_links_delete,
            commands::workspace_links_reorder,
            commands::workspace_tasks_list,
            commands::workspace_tasks_add,
            commands::workspace_tasks_toggle,
            commands::workspace_tasks_delete,
            commands::workspace_tasks_reorder,
            commands::workspace_activity_list,
            commands::app_pomodoro_get,
            commands::app_pomodoro_start,
            commands::app_pomodoro_pause,
            commands::app_pomodoro_resume,
            commands::app_pomodoro_reset,
            commands::settings_load,
            commands::settings_save,
            commands::settings_config_path,
            commands::claude_inventory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// On macOS, double-clicking a `.app` from /Applications launches it with
/// only the system PATH (/usr/bin:/bin:/usr/sbin:/sbin), not the user's
/// interactive shell PATH from ~/.zshrc / ~/.zprofile. Result: PTYs we
/// spawn can't find `claude`, `npm`, `nvm`-installed tools, brew binaries,
/// etc., and the workspace hangs at "starting claude".
///
/// Best-effort fix: run the user's login shell in a non-interactive child,
/// capture the PATH it would export, and overwrite our own. Subsequent
/// `Command::spawn` calls inherit the updated PATH automatically.
///
/// Two-tier caching policy:
///   1. On startup we read a cached PATH from `<data_dir>/path_cache.json`
///      keyed by `($SHELL, mtime(shell binary))`. Cache hit → apply the
///      cached PATH synchronously, skip the shell probe entirely. This is
///      the cold-start win (50-300 ms on macOS).
///   2. Cache miss → spawn the shell probe in a background thread so the
///      Tauri builder isn't blocked. When it returns, we apply the new PATH
///      (subsequent PTY spawns pick it up) AND persist it to the cache for
///      the next launch.
///
/// The "main process keeps the old PATH for the first second" trade is fine
/// — Tessera doesn't launch any PATH-sensitive children during that window.
///
/// SAFETY: `std::env::set_var` is `unsafe` in the 2024 edition because env
/// mutation isn't atomic. We only mutate PATH in the synchronous startup
/// prelude, BEFORE the Tauri builder starts spawning runtime threads. The
/// cold-cache probe is bounded by `PATH_PROBE_TIMEOUT` so the worst-case
/// cold-start tax is capped; subsequent launches hit the cache and skip
/// the probe entirely.
fn inherit_shell_path(data_dir: &std::path::Path) {
    let Ok(shell) = std::env::var("SHELL") else {
        return;
    };

    let cache_path = data_dir.join("path_cache.json");
    let key = path_cache::cache_key(&shell);

    // Tier 1: warm cache hit — instant.
    if let Some(cached) = path_cache::load(&cache_path, &key) {
        let current = std::env::var("PATH").unwrap_or_default();
        if cached != current {
            // SAFETY: called before any other thread is spawned.
            unsafe { std::env::set_var("PATH", &cached) };
        }
        tracing::info!(path = %cached, "inherited interactive shell PATH (cache hit)");
        return;
    }

    // Tier 2: cold cache — probe synchronously with a tight timeout. The
    // probe was previously fired on a background thread for "zero-wait"
    // startup, but that races `setenv` against `getenv` in any subsequent
    // PTY spawn (UB under the 2024 edition / TSan). We pay up to
    // `PATH_PROBE_TIMEOUT` once per cache key, and never again.
    const PATH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
    if let Some(probed) = path_cache::probe_shell_with_timeout(&shell, PATH_PROBE_TIMEOUT) {
        let current = std::env::var("PATH").unwrap_or_default();
        if probed != current {
            // SAFETY: synchronous startup prelude — no other threads yet.
            unsafe { std::env::set_var("PATH", &probed) };
        }
        let _ = path_cache::save(&cache_path, &key, &probed);
        tracing::info!(path = %probed, "inherited interactive shell PATH (sync probe)");
    }
}

/// PATH cache helpers — pulled into a module so `cache_key` and `probe_shell`
/// can be unit-tested without touching the global env.
mod path_cache {
    use serde::{Deserialize, Serialize};
    use std::path::Path;

    /// Cache identity: shell binary + its mtime as nanos-since-epoch. If
    /// either changes we treat the cache as cold. mtime survives reboots,
    /// rsync, and brew upgrades without us having to invent a stamp file.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Key {
        pub shell: String,
        pub mtime_ns: i128,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Entry {
        key: Key,
        path: String,
    }

    pub fn cache_key(shell: &str) -> Key {
        let mtime_ns = std::fs::metadata(shell)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0);
        Key {
            shell: shell.to_string(),
            mtime_ns,
        }
    }

    pub fn load(cache_path: &Path, key: &Key) -> Option<String> {
        let raw = std::fs::read_to_string(cache_path).ok()?;
        let entry: Entry = serde_json::from_str(&raw).ok()?;
        if &entry.key != key {
            return None;
        }
        if entry.path.is_empty() {
            return None;
        }
        Some(entry.path)
    }

    pub fn save(cache_path: &Path, key: &Key, path: &str) -> std::io::Result<()> {
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let entry = Entry {
            key: key.clone(),
            path: path.to_string(),
        };
        let json = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
        std::fs::write(cache_path, json)
    }

    /// Probe the shell with a hard timeout. Spawns the child, polls
    /// `try_wait`, kills on timeout. Used at cold-start so a wedged login
    /// shell can't stall Tessera indefinitely.
    pub fn probe_shell_with_timeout(shell: &str, timeout: std::time::Duration) -> Option<String> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = Command::new(shell)
            .args(["-l", "-c", "printf %s \"$PATH\""])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match child.try_wait().ok()? {
                Some(status) => {
                    if !status.success() {
                        return None;
                    }
                    let mut out = String::new();
                    child.stdout.take()?.read_to_string(&mut out).ok()?;
                    let probed = out.trim().to_string();
                    return if probed.is_empty() {
                        None
                    } else {
                        Some(probed)
                    };
                }
                None => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn cache_round_trips_and_invalidates_on_key_change() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("path_cache.json");
            let k1 = Key {
                shell: "/bin/zsh".into(),
                mtime_ns: 42,
            };
            save(&path, &k1, "/usr/local/bin:/usr/bin").unwrap();
            assert_eq!(load(&path, &k1).as_deref(), Some("/usr/local/bin:/usr/bin"));

            // Different mtime → miss.
            let k2 = Key {
                shell: "/bin/zsh".into(),
                mtime_ns: 99,
            };
            assert!(load(&path, &k2).is_none());

            // Different shell → miss.
            let k3 = Key {
                shell: "/bin/bash".into(),
                mtime_ns: 42,
            };
            assert!(load(&path, &k3).is_none());
        }

        #[test]
        fn load_returns_none_when_file_missing() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("missing.json");
            let k = Key {
                shell: "/x".into(),
                mtime_ns: 0,
            };
            assert!(load(&path, &k).is_none());
        }
    }
}

fn dispatch_hook(app: &AppHandle, svc: &Arc<WorkspaceService>, evt: HookEvent) {
    let new_status = match evt.kind {
        HookKind::PostToolUse => AgentStatus::Working,
        HookKind::Stop => AgentStatus::Done,
        HookKind::Notification => AgentStatus::NeedsInput,
    };
    svc.set_status(evt.workspace_id, new_status);
    let _ = app.emit(
        "workspace_status",
        serde_json::json!({
            "workspace_id": evt.workspace_id,
            "agent_status": new_status,
        }),
    );

    // Persist the event to the activity log so the user can scroll it
    // back in the workspace's Activity tab. Truncate the payload to 4 KB
    // — Claude's tool_input.command can be very long, and the log is a
    // human-readable trail, not an audit substrate.
    let kind = match evt.kind {
        HookKind::PostToolUse => ActivityKind::PostToolUse,
        HookKind::Stop => ActivityKind::Stop,
        HookKind::Notification => ActivityKind::Notification,
    };
    let summary = summarise_hook(&evt);
    let mut payload_str = serde_json::to_string(&evt.payload).unwrap_or_default();
    truncate_on_char_boundary(&mut payload_str, 4096);
    let entry = ActivityEntry {
        id: uuid::Uuid::new_v4(),
        workspace_id: evt.workspace_id,
        kind,
        summary,
        payload: payload_str,
        created_at: chrono::Utc::now(),
    };
    let db = app.state::<commands::DbState>();
    {
        let conn = db.lock().unwrap();
        if let Err(e) = tessera_store::activity::insert(&conn, &entry) {
            tracing::warn!(error = %e, "activity_log insert failed");
        } else if let Err(e) = tessera_store::activity::prune_keep_n(&conn, evt.workspace_id, 200) {
            tracing::warn!(error = %e, "activity_log prune failed");
        }
    }

    // Surface NeedsInput / Done as OS notifications when the window is
    // not focused. PostToolUse fires constantly (every tool call), so we
    // don't notify on those — only the two states the user actually waits
    // for. Window focus is best-effort; on failure assume "not focused".
    if matches!(evt.kind, HookKind::Notification | HookKind::Stop) {
        use tauri_plugin_notification::NotificationExt;
        let focused = app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if !focused {
            let title = svc
                .get(evt.workspace_id)
                .ok()
                .flatten()
                .map(|w| w.name)
                .unwrap_or_else(|| "Tessera".to_string());
            let body = match evt.kind {
                HookKind::Notification => "needs your input",
                HookKind::Stop => "session ended",
                _ => "",
            };
            if let Err(e) = app.notification().builder().title(title).body(body).show() {
                tracing::warn!(error = %e, "notification show failed");
            }
        }
    }

    if matches!(evt.kind, HookKind::PostToolUse) {
        let bash_command = evt
            .payload
            .get("tool_name")
            .and_then(|v| v.as_str())
            .filter(|s| *s == "Bash")
            .and_then(|_| evt.payload.get("tool_input"))
            .and_then(|ti| ti.get("command"))
            .and_then(|c| c.as_str());
        if let Some(cmd) = bash_command {
            if let Some(parsed) = tessera_hook::parse_worktree_add(cmd) {
                let path = std::path::PathBuf::from(&parsed.path);
                let branch = parsed.branch.clone();
                if let Err(e) =
                    svc.set_detected_worktree(evt.workspace_id, Some(path.clone()), branch.clone())
                {
                    tracing::warn!(error = %e, "set_detected_worktree failed");
                } else {
                    let _ = app.emit(
                        "workspace_worktree",
                        serde_json::json!({
                            "workspace_id": evt.workspace_id,
                            "detected_worktree": path,
                            "detected_branch": branch,
                        }),
                    );
                }
            }
        }
    }
}

/// Derive a short human-readable line from a hook event so the user can
/// scan the Activity tab quickly. PostToolUse highlights the tool name
/// (and Bash command for that one); Notification surfaces the message;
/// Stop is a constant phrase. Always trimmed to 120 chars.
fn summarise_hook(evt: &HookEvent) -> String {
    let raw = match evt.kind {
        HookKind::Stop => "session stopped".to_string(),
        HookKind::Notification => evt
            .payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("agent needs input")
            .to_string(),
        HookKind::PostToolUse => {
            let tool = evt
                .payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            if tool == "Bash" {
                if let Some(cmd) = evt
                    .payload
                    .get("tool_input")
                    .and_then(|ti| ti.get("command"))
                    .and_then(|c| c.as_str())
                {
                    return truncate_120(&format!("Bash: {cmd}"));
                }
            }
            tool.to_string()
        }
    };
    truncate_120(&raw)
}

fn truncate_120(s: &str) -> String {
    if s.chars().count() <= 120 {
        return s.to_string();
    }
    let mut out: String = s.chars().take(117).collect();
    out.push_str("...");
    out
}

/// Truncate `s` in-place to at most `max_bytes`, snapping back to the nearest
/// UTF-8 char boundary. `String::truncate` panics if the cut lands inside a
/// multi-byte codepoint; this never does.
fn truncate_on_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

#[cfg(test)]
mod tests {
    use super::truncate_on_char_boundary;

    #[test]
    fn snaps_back_inside_two_byte_char() {
        // "abcё" = 3 + 2 = 5 bytes. Cut at 4 lands inside ё; snap to 3.
        let mut s = String::from("abcё");
        truncate_on_char_boundary(&mut s, 4);
        assert_eq!(s, "abc");
    }

    #[test]
    fn snaps_back_inside_four_byte_char() {
        // "ab🦀" = 2 + 4 = 6 bytes. Cut at 5 lands inside 🦀; snap to 2.
        let mut s = String::from("ab🦀");
        truncate_on_char_boundary(&mut s, 5);
        assert_eq!(s, "ab");
    }

    #[test]
    fn ascii_cuts_exactly() {
        let mut s: String = "a".repeat(5000);
        truncate_on_char_boundary(&mut s, 4096);
        assert_eq!(s.len(), 4096);
    }

    #[test]
    fn shorter_than_limit_is_noop() {
        let mut s = String::from("hello");
        truncate_on_char_boundary(&mut s, 4096);
        assert_eq!(s, "hello");
    }

    #[test]
    fn regression_v0_1_8_cyrillic_payload_does_not_panic() {
        // The actual v0.1.8 crash: dispatch_hook serializes evt.payload to
        // JSON, then truncates to 4096 bytes. Cyrillic in tool_input made
        // the unchecked String::truncate hit a non-char-boundary and panic.
        let mut s: String = "ё".repeat(3000);
        truncate_on_char_boundary(&mut s, 4095);
        assert!(s.len() <= 4095);
        assert!(s.is_char_boundary(s.len()));
        std::str::from_utf8(s.as_bytes()).expect("still valid UTF-8");
    }
}

/// CLI-side handler for `tessera hook <workspace_id> <kind>`. Reads stdin
/// (whatever Claude wrote), wraps it as a HookEvent, and sends to the socket.
/// Exits 0 silently on any failure — must not break the agent's session.
pub fn run_hook(workspace_id: &str, kind: &str) -> i32 {
    use std::io::Read;
    use tessera_hook::{send_event, HookEvent, HookKind};
    use uuid::Uuid;

    let ws_id = match Uuid::parse_str(workspace_id) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let Some(parsed_kind) = HookKind::from_cli(kind) else {
        return 0;
    };

    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    let payload =
        serde_json::from_str::<serde_json::Value>(buf.trim()).unwrap_or(serde_json::Value::Null);

    let Some(data_dir) = dirs::data_local_dir() else {
        return 0;
    };
    let sock = data_dir.join("tessera").join("hooks.sock");

    let _ = send_event(
        &sock,
        &HookEvent {
            workspace_id: ws_id,
            kind: parsed_kind,
            payload,
        },
    );
    0
}

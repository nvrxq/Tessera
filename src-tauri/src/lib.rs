mod commands;
mod terminal;

use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tessera_core::AgentStatus;
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

    let socket_path = data_dir.join("hooks.sock");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(supervisor.clone())
        .manage(workspace_service.clone())
        .manage(registry.clone())
        .manage(grid_sizes.clone())
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
            let dirty: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<uuid::Uuid>>> =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
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
                                    seen.lock()
                                        .unwrap()
                                        .entry(session_id)
                                        .or_insert(t_recv);
                                    dirty.lock().unwrap_or_else(|e| e.into_inner()).insert(session_id);
                                }
                            }
                            PtyEvent::Exit { session_id } => {
                                reg.remove(session_id);
                                sizes.lock().unwrap_or_else(|e| e.into_inner()).remove(&session_id);
                                dirty.lock().unwrap_or_else(|e| e.into_inner()).remove(&session_id);
                                seen.lock().unwrap_or_else(|e| e.into_inner()).remove(&session_id);
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
                    let mut tick = tokio::time::interval(
                        std::time::Duration::from_millis(1),
                    );
                    tick.set_missed_tick_behavior(
                        tokio::time::MissedTickBehavior::Delay,
                    );
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
                            let t_pty = seen
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .remove(&sid);
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
                                    let total_us = t_pty
                                        .map(|t| t.elapsed().as_micros())
                                        .unwrap_or(0);
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

            // Hook listener (Plan 4).
            let svc = workspace_service.clone();
            tauri::async_runtime::spawn(async move {
                match Listener::bind(&socket_path) {
                    Ok(mut listener) => {
                        tracing::info!(path = %listener.socket_path.display(), "hook listener bound");
                        while let Some(evt) = listener.rx.recv().await {
                            dispatch_hook(&handle, &svc, evt);
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "hook listener bind failed"),
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
            commands::workspace_pomodoro_get,
            commands::workspace_pomodoro_start,
            commands::workspace_pomodoro_pause,
            commands::workspace_pomodoro_resume,
            commands::workspace_pomodoro_reset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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

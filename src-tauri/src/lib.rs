mod commands;

use base64::Engine;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tessera_core::AgentStatus;
use tessera_hook::{HookEvent, HookKind, Listener};
use tessera_pty::{PtyEvent, Supervisor};
use tessera_workspace::WorkspaceService;
use tracing_subscriber::EnvFilter;

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
    let workspace_service: Arc<WorkspaceService> =
        Arc::new(WorkspaceService::new(db, supervisor.clone(), worktree_root));

    let socket_path = data_dir.join("hooks.sock");

    tauri::Builder::default()
        .manage(supervisor.clone())
        .manage(workspace_service.clone())
        .setup(move |app| {
            let handle = app.handle().clone();

            // PTY event pump (Plan 2).
            {
                let handle = handle.clone();
                let mut rx = supervisor.subscribe();
                tauri::async_runtime::spawn(async move {
                    while let Ok(evt) = rx.recv().await {
                        let payload = match &evt {
                            PtyEvent::Data { session_id, bytes } => serde_json::json!({
                                "kind": "data",
                                "session_id": session_id,
                                "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes),
                            }),
                            PtyEvent::Exit { session_id } => serde_json::json!({
                                "kind": "exit",
                                "session_id": session_id,
                            }),
                        };
                        let _ = handle.emit("pty_event", payload);
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

    // PostToolUse + Bash + `git worktree add` -> remember the new worktree.
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

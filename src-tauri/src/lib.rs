mod commands;

use base64::Engine;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
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

    tauri::Builder::default()
        .manage(supervisor.clone())
        .manage(workspace_service)
        .setup(move |app| {
            let handle = app.handle().clone();
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

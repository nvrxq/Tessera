mod commands;

use base64::Engine;
use std::sync::Arc;
use tauri::Emitter;
use tessera_pty::{PtyEvent, Supervisor};
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let supervisor: Arc<Supervisor> = Arc::new(Supervisor::new());

    tauri::Builder::default()
        .manage(supervisor.clone())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

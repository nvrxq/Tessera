// Prevent the launcher console on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // tessera hook <workspace_id> <kind>
    if args.len() >= 4 && args[1] == "hook" {
        let code = tessera_lib::run_hook(&args[2], &args[3]);
        // Bypass atexit handlers (Tauri/GTK link in glib teardown that blocks
        // on X11 when stdin came from a pipe). The hook subprocess must exit
        // promptly so Claude Code's hook chain doesn't stall.
        unsafe { libc::_exit(code) }
    }

    // webkit2gtk's DMA-BUF renderer (default since 2.42) is the #1 cause of
    // input lag and idle jitter on Linux/X11. Disable before any webkit/GTK
    // init runs. User can override by exporting the var beforehand.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    tessera_lib::run();
}

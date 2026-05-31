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

    // WebKitGTK rendering mode (Linux only) — set before any webkit/GTK init.
    //
    // WebKitGTK's DMA-BUF GPU renderer is mature as of 2.5x and gives smooth,
    // low-latency compositing. An earlier build force-DISABLED it (a workaround
    // for the buggy 2.42-era renderer), but on current WebKitGTK that drops the
    // webview onto a CPU-side compositing fallback that repaints the *entire*
    // webview every frame — which is itself the #1 cause of input lag: every
    // keystroke echo blocks on a full software composite, in every pane. So we
    // now leave the GPU path ON by default and expose an escape hatch for the
    // minority of GPU/driver combos where it misbehaves (e.g. a black webview on
    // some NVIDIA setups). A user-exported WEBKIT_* var always wins.
    //
    //   TESSERA_GPU unset / "auto" → WebKitGTK default DMA-BUF GPU renderer
    //   TESSERA_GPU=no-dmabuf      → GPU compositing, but skip the DMA-BUF path
    //   TESSERA_GPU=software|off   → disable accelerated compositing entirely
    #[cfg(target_os = "linux")]
    {
        let disable = |key: &str| {
            if std::env::var_os(key).is_none() {
                std::env::set_var(key, "1");
            }
        };
        match std::env::var("TESSERA_GPU").unwrap_or_default().as_str() {
            "software" | "off" => {
                disable("WEBKIT_DISABLE_COMPOSITING_MODE");
                disable("WEBKIT_DISABLE_DMABUF_RENDERER");
            }
            "no-dmabuf" => disable("WEBKIT_DISABLE_DMABUF_RENDERER"),
            _ => {}
        }
    }

    tessera_lib::run();
}

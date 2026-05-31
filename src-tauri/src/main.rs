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
    // WebKitGTK's accelerated (GPU) compositor is the #1 cause of input lag on
    // Linux: it re-composites the *entire* webview through the GPU driver on every
    // frame, so each keystroke echo stalls on a full composite — in every pane.
    // On NVIDIA/X11 hybrids it's especially bad, and neither the DMA-BUF path nor
    // disabling just DMA-BUF helps. The fix that actually makes typing snappy is
    // disabling accelerated compositing entirely (software composite) — for a
    // text app that's lower-latency and immune to GPU/driver jank. Confirmed on
    // the reporter's NVIDIA+AMD / X11 / i3 box: only WEBKIT_DISABLE_COMPOSITING_MODE
    // gave native-feeling input. So that's the DEFAULT now, with an opt-out for
    // anyone whose GPU compositing is fine. A user-exported WEBKIT_* var wins.
    //
    //   TESSERA_GPU unset / "software" / "off" → software compositing (default)
    //   TESSERA_GPU=no-dmabuf                   → GPU compositing, skip DMA-BUF
    //   TESSERA_GPU=gpu / "hardware" / "dmabuf" → full GPU/DMA-BUF compositing
    #[cfg(target_os = "linux")]
    {
        let disable = |key: &str| {
            if std::env::var_os(key).is_none() {
                std::env::set_var(key, "1");
            }
        };
        match std::env::var("TESSERA_GPU").unwrap_or_default().as_str() {
            "gpu" | "hardware" | "dmabuf" => {}
            "no-dmabuf" => disable("WEBKIT_DISABLE_DMABUF_RENDERER"),
            _ => disable("WEBKIT_DISABLE_COMPOSITING_MODE"),
        }
    }

    tessera_lib::run();
}

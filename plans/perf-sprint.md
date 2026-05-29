# Perf Sprint — speed up every Tessera action

Goal (from user): "Ускорить каждое действие в Tessera ... начиная от переключения Workspace до выделения и копирования."

Workflow: 3 parallel worktrees off `origin/main`, parallel implementers, parallel Opus reviews, draft PRs with `[skip ci]`, squash-merge into `main`, build fresh binary.

Hard constraints (CLAUDE.md / memory): nvrxq authorship; no `gh` CLI (raw HTTPS via `/home/save/nvr_tok`); `/home/save/tessera` is off-limits; design system rules in `DESIGN.md`.

## Hot-spots (from recon)
- `inherit_shell_path` synchronously forks login shell on cold start (50–300 ms)
- `snapshot()` allocates fresh `Vec<WireCell>` and per-row `Vec<GridCell>` every tick (1ms cadence)
- Mouse-drag selection runs `paintFull` over the entire grid every rAF
- `cellAtClient` calls `getBoundingClientRect()` on every pointermove (forced layout)
- ResizeObserver fires un-debounced; each tick = 2 IPCs + 2 full snapshots
- Wheel scroll = one IPC + full snapshot per line; trackpad gestures hammer this
- `claude_inventory` blocks the Tauri worker with sync `std::fs::read_dir` over `~/.claude/{skills,plugins,...}`
- `Supervisor::write` holds the sessions-map lock through `flush()` — multi-session contention
- `decoFills.sort(localeCompare)` per `paintFull`
- Status-dot animates `box-shadow` (CPU compositor)
- Font-slider in Settings drives main Terminal full IPC resize per pixel

## Worktree A — `perf-backend` (Rust)

1. Lazy/cached `inherit_shell_path`: persist probed PATH to a small JSON keyed by SHELL+SHELL_mtime; re-probe only on miss. Move probe off the Tauri builder critical path.
2. Reuse the `WireSnapshot.cells` Vec inside `SessionState` (preallocate, clear + extend each tick); reuse per-row `Vec<GridCell>` in `rows_iter_with_offset` via a thread-local or per-`SessionState` scratch.
3. SQLite pragmas at open: `journal_mode=WAL`, `synchronous=NORMAL`, `mmap_size=256MB`, `cache_size=-20000`, `temp_store=MEMORY`, `busy_timeout=5000`.
4. Convert hot `conn.prepare(...)` to `conn.prepare_cached(...)` in workspace_list / extras / pomodoro paths.
5. `claude_inventory` → `tokio::task::spawn_blocking`; cache the collected inventory keyed by (workspace_id, max(mtime) of source dirs); invalidate on miss.
6. `set_scroll_delta`: drop the unnecessary `palette` lock for the `scrollback_max` read (already protected by `inner`).
7. `terminal_resize` no longer emits an immediate snapshot — let the next render-tick emit it (kills the double-snap during drag-resize).
8. ~~Cargo release profile~~ — owned by user in `8a3103d` (CI speed traded for binary size; `profile.dist` reserved for tight binary).
9. `Supervisor::write`: drop the sessions-map lock before `writer.flush()` (clone the `Arc<Mutex<W>>` for the writer, release outer lock, then flush). Reduces multi-session lock contention.

Verification: `cargo check`, `cargo test --workspace`, `cargo build --release --bin tessera`, `cargo bench` if benches exist.

## Worktree B — `perf-canvas` (Terminal.tsx)

1. **Layered selection canvas**: add `selCanvas` overlay above the text canvas (same size, z-index 1, `pointer-events: none`). `scheduleSelectionRepaint` paints ONLY the overlay (clear + per-row `fillRect` for the selection rect). `paintFull` no longer touches the selection.
2. Cache `host.getBoundingClientRect()` on `pointerdown`; invalidate on `pointerup`, ResizeObserver fire, and scroll. Avoids forced layout per `pointermove`.
3. Coalesce `terminal_scroll` IPC: collect `wheel` delta over a frame, send one `invoke` per rAF with the summed line count.
4. `getContext("2d", { alpha: false })` on the text canvas (default bg `#0F0F10`).
5. Replace `decoFills.sort(localeCompare)` with bucket-per-color (small Map<string, [number,number,number,number][]>) — strictly faster on typical inputs.
6. `addEventListener("wheel", h, { passive: true })` everywhere we don't `preventDefault`.
7. Debounce ResizeObserver via rAF: collapse N sub-pixel ticks into one `syncGrid`.
8. Wrap snapshot-apply signal writes in `solid-js` `batch()`; wrap reactive reads inside the rAF paint in `untrack()`.
9. `text-rendering: optimizeSpeed` CSS on the canvas wrapper.

Verification: `npx tsc --noEmit`, `cargo check`, build the UI and verify with the manual checklist below.

## Worktree C — `perf-ux` (CSS + Settings + Vite)

1. Status-dot animation: replace the `box-shadow` keyframes with `transform: scale(...)` + `opacity` (GPU-compositor friendly). Match visual intent from `DESIGN.md` (`working` dot breathes 1.6s).
2. Settings font-size slider: debounce `setDraft` for `fontPx`/`fontFamily`/`backgroundColor`/palette changes by 60 ms (or on `change` instead of `input`), so the main Terminal does not fully reflow on every pixel of slider drag.
3. `vite.config.ts` → `build.rollupOptions.output.manualChunks`: split `solid-js`, `@tauri-apps/*`, and `@fontsource/*` into stable vendor chunks (better cache hit across releases).
4. Idle prefetch lazy modals: after first paint, `requestIdleCallback(() => import("./SettingsModal"))` etc. so the first open is instant.

Verification: `npx tsc --noEmit`, build the UI, browse Settings drag + status-dot animation visually.

## Manual verification checklist (post-merge, on fresh binary)

- Click between two workspaces — no visible jitter; first click stays sub-second.
- Mouse-drag selection across a screen of text — no FPS drop, no flicker on the unselected portion of the grid.
- Trackpad scroll across long output — smooth, no torn frames.
- Drag the window edge to resize — fluid, no frozen frame.
- Open Settings → drag font-size slider — preview redraws smoothly, main terminal does not stutter.
- Open Inventory — modal appears within ~16 ms even on fresh-start (cache).
- Cold-start binary — first paint within ~500 ms perceived (down from ~1 s).
- Copy-paste a 2-page block of text — no UI freeze.

## Out of scope for this sprint

- Glyph atlas via ImageBitmap (highest-impact item; deserves its own PR with benches).
- OffscreenCanvas + Worker paint thread (LARGE).
- Full SQLite writer-task + reader-pool refactor (LARGE).
- Replacing Tauri `emit` with `tauri::ipc::Channel` for the snapshot pump (MEDIUM but invasive — separate PR).

# Plan: migrate Tessera's renderer to xterm.js (per user request — reuse Terax's approach)

## What Terax actually does (investigated 2026-05-29, repo crynta/terax-ai, Apache-2.0)
- Stack: Tauri 2 + Rust + React 19, renderer = **@xterm/xterm 6 + @xterm/addon-webgl**
  (+ fit/search/serialize/web-links). xterm.js owns ALL parsing, rendering,
  scrollback, selection, links, ligatures.
- Data flow: Rust `pty_open(cols,rows,cwd, onData: Channel<ArrayBuffer>, onExit)`
  streams **raw PTY bytes** (no JSON/base64) straight into `term.write(bytes)`.
  Input: `term.onData -> pty_write`. Resize: `FitAddon -> pty_resize`.
- No bespoke snapshot/delta/cell protocol exists. xterm does it all.

## Why this is the right long-term fix
Tessera's bespoke Canvas2D + wezterm-term snapshot/delta pipeline has produced
repeated subtle render bugs (wide-glyph spacer, cumulative drift, cursor ghost,
half-blank-on-switch). xterm.js is the renderer behind VS Code/Hyper — mature,
correct on wide glyphs/scrollback/selection. Migrating DELETES the whole buggy
layer rather than patching it cell-by-cell.

## Scope (this is a real migration, ~1–2 focused days, NOT a hotfix)
### Frontend (`ui/`)
- Add deps: `@xterm/xterm @xterm/addon-webgl @xterm/addon-fit` (+ optional
  search/web-links). NOTE: `npm install` is guard-blocked in this env — the user
  must run it, or we vendor.
- Rewrite `Terminal.tsx`: instantiate `Terminal` + `WebglAddon` + `FitAddon` in
  `onMount` (xterm is framework-agnostic — mounts into a div, SolidJS is fine).
  Wire: backend byte-channel -> `term.write`; `term.onData` -> `pty_write`;
  ResizeObserver/FitAddon -> `pty_resize`. Re-wire features currently hand-rolled:
  loading/“exited”/error overlays (KEEP — they sit above the xterm div),
  session switch (one Terminal per session, or `term.reset()`), drag-drop paste,
  copy/paste, font size, theme. DELETE the manual paint/delta/cursor/selection/
  scrollback code (xterm replaces all of it).
- Map DESIGN.md palette -> xterm `ITheme` (background #0F0F10, accent #C8825B, the
  16 ANSI entries from settings). Geist Mono font.

### Backend (`src-tauri`, `crates/`)
- Stream raw bytes: replace `term_snapshot` emit with a per-session
  `Channel<ArrayBuffer>` (or keep `Emitter` with base64, but Channel is what Terax
  uses and avoids the JSON tax). Pump task forwards PTY `Data` bytes verbatim.
- RETIRE: `terminal.rs` (TerminalRegistry/Snapshot/diff/keyframe), `crates/term`
  (`cell.rs`, `grid.rs` — wezterm-term parser no longer needed for render),
  `terminal_resize`/`terminal_scroll`/`term_snapshot`, `GridSizes`, the dirty set.
  KEEP: `Supervisor`/`PtySession`, `pty_write`/`pty_resize`/`pty_kill`, workspace
  identity (`--session-id`), hooks, settings, extras.
- This removes a large amount of code and the entire render-bug surface.

## Risk / why NOT a blind swap right now
- I cannot run the Tauri GUI in this environment (the running binary is off-limits;
  no display), so a renderer swap can't be end-to-end verified by me. Shipping it
  unverified is exactly the “shipped broken” pattern to avoid.
- WebGL must work in the WebView (WKWebView/WebKitGTK). xterm webgl addon falls
  back to canvas; verify on the user's Linux box.
- Reverses the earlier deliberate Canvas2D decision (see memory). That's fine —
  the user is explicitly choosing xterm.js now.

## Recommended sequencing
1. Ship v0.1.12 (mixing + vanish + wide-glyph/keyframe artifact fixes) for relief. ✅ in progress
2. Execute this migration in a focused session WHERE THE USER CAN RUN A DEV BUILD
   (`npm install` + `npm run tauri dev`) to confirm before release. I implement,
   they smoke-test, then release v0.1.13.

## License
xterm.js + addons are MIT (used as npm deps). Terax is Apache-2.0 — since the user
asked to copy their code directly, we DO copy renderer/keymap/theme logic and add a
NOTICE crediting crynta/terax-ai (Apache-2.0 §4 attribution). Tessera is also
Apache-2.0, so it's compatible.

---

## CONCRETE DESIGN (branch feat/xterm-renderer, 2026-05-29)

### Backend (src-tauri) — verifiable here via cargo
- NEW `src-tauri/src/ptystream.rs`: `PtyStreamRegistry` = per-session
  `{ ring: VecDeque<u8> capped ~512KB, channel: Option<Channel<tauri::ipc::Response>> }`.
  - `feed(sid,&[u8])`: append to ring (drop-oldest past cap) + if a channel is
    attached, send the bytes as `Response::new(...)`.
  - `attach(sid, channel)`: replay the ring into the channel, then store it (so a
    late-attaching frontend still gets the welcome screen / history).
  - `remove(sid)`.
- NEW command `terminal_attach(session_id, on_data: Channel<Response>)`.
- lib.rs pump: Data -> stream.feed; Exit -> stream.remove + emit pty_event.
  DELETE the 1ms snapshot tick task, DirtySet, GridSizes, bench.
- DELETE: terminal_resize, terminal_scroll, term_snapshot, Snapshot, WireCell,
  TerminalRegistry. KEEP pty_write, pty_resize (PTY winsize), pty_kill,
  workspace_spawn_agent (identity), hooks, settings, extras, inventory.
- crates/term parser becomes unused -> drop dep from src-tauri.

### Frontend (ui) — NOT verifiable here (guarded installer, no GUI); USER builds
- package.json: add the @xterm/* renderer + addons.
- NEW ui/src/lib/xtermKeymap.ts — copy Terax keymap.ts verbatim (framework-agnostic).
- NEW ui/src/lib/terminalTheme.ts — copy Terax, map Tessera settings palette -> ITheme.
- REWRITE Terminal.tsx (SolidJS): create Terminal+FitAddon+WebglAddon+WebLinks+Search+
  Serialize into a div; attach via the byte Channel -> term.write; term.onData ->
  pty_write; ResizeObserver+FitAddon -> pty_resize; attachCustomKeyEventHandler (IME 229
  guard, word/line nav, ctrl+shift+c/v, shift-enter) copied from Terax rendererPool.
  KEEP overlays (loading/connecting/exited+Restart/error), drag-drop paste, settings
  effect, pty_event exit. DELETE the Canvas2D painter entirely.

### Verify
- cargo test/clippy/fmt (backend) — me.
- USER smoke-tests a dev build, then we release v0.1.13. Never ship the swap blind.

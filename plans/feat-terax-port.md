# feat/terax-port — Global Terax port into Tessera

**Goal (user, 2026-05-29):** Global rework — take `crynta/terax-ai` (Apache-2.0,
React) and port its terminal-host code (where Claude Code runs) + app
customization into Tessera (SolidJS). Copy code 1:1 where it's framework-agnostic;
translate React→Solid for view components. User wants all four subsystems:
terminal-host robustness, pane splitting, theme system + backgrounds, settings
window + shortcuts.

Terax clone: `/tmp/terax-ai`. NOTICE attribution already exists; extend it.

## Framework decision (settled)
Port into SolidJS. Copy the Rust filters + framework-agnostic `.ts` engine
verbatim; translate `.tsx` views to Solid. A full React rewrite would be slower
and would destroy Tessera's working identity (Sidebar/pomodoro/skills/session-id).

## Architecture decision (settled — lower-risk than wholesale transport swap)
KEEP Tessera's proven backend transport (supervisor by session-UUID, per-session
`PtyStreamRegistry` ring, `terminal_attach/detach`, deterministic claude
`--session-id` identity, idempotent `spawn_agent`). LAYER Terax's pieces on top:

- **Frontend engine (port, mostly 1:1):** renderer pool (≤5 reused xterm slots +
  WebGL context-loss RECOVERY + canvas-context release — fixes vanish), dormantRing
  (buffer bytes for an unbound leaf), osc-handlers (cwd via OSC 7, prompt via 133),
  keymap (already have), panes (tree split/close/focus). Solid components:
  TerminalPane / PaneTreeView / TerminalStack.
- **Backend filters (port ~verbatim):** `da_filter.rs` (strip DA-query echoes —
  fixes artifacts), `agent_detect.rs` (OSC 133/777 → working/attention/finished →
  feeds Tessera's breathing status dot), flusher coalescing (chunk PTY output).
- Each pane leaf = a Tessera PTY session UUID. Primary leaf = workspace claude
  (existing `spawn_agent`, deterministic id). Split leaves = a new per-pane spawn
  (shell or claude-in-cwd; default decided in Phase B).
- Channel-per-session-for-life: the data channel routes bytes to the bound slot's
  term OR the leaf's dormantRing. Backend ring still covers pre-mount/eager-spawn
  replay on first attach.

## Why no full transport rewrite
Tessera's session-identity fix (deterministic `--session-id`, migration 0011) took
two attempts to get right; replacing the transport risks regressing same-folder
mixing. The user cares about the terminal CODE/behaviour + features, not the IPC
plumbing. DA filter + flusher + agent-detect give the valuable backend wins.

## Verification reality
No GUI/display here. Build-verify only: `cargo test/clippy/fmt` + CI-exact bun
Docker sandbox (`oven/bun:1.3`: install + tsc + build + frozen-lockfile). DO NOT
release until the USER GUI-tests on their computer — this is a big GUI rework.
Ship on branch `feat/terax-port`; iterate from dev-console errors → release later.

## Phases (each shippable / committable)

### Phase A — Terminal-host robustness (the bug fix + engine core)
Backend:
- New `src-tauri/src/pty/da_filter.rs` (copy verbatim + tests).
- New `src-tauri/src/pty/agent_detect.rs` (copy ~verbatim; map signals →
  `AgentStatus` → existing workspace status event → status dot). Tessera runs
  claude directly (no shell preexec) so OSC 133-C arming won't fire; rely on the
  OSC 777 hook marker path + claude's own sequences. Keep DEFAULT_AGENTS=["claude"].
- Wire DA filter + agent-detect into the data path. Cleanest: run per-session DA
  filter in the `lib.rs` pump before `stream.feed`, replying DA via
  `supervisor.write`. Keep per-session filter state in a map keyed by session UUID.
  (Flusher coalescing already partly covered by broadcast; revisit if needed.)
Frontend:
- Port `ui/src/lib/term/rendererPool.ts` (1:1, swap deps: prefs→settings store,
  fonts→Tessera fonts, theme→terminalTheme, opener kept). KEEP WebGL recovery +
  releaseCanvasContext (the vanish fix).
- Port `dormantRing.ts` (1:1), `osc-handlers.ts` (1:1), reuse `xtermKeymap.ts`.
- New Solid `useTerminalSession` equivalent (`ui/src/lib/term/session.ts`):
  module-level sessions Map; channel-per-session via `terminal_attach`; routes
  bytes to slot or dormantRing; pty_write/pty_resize bridge; exit handling.
- Rewrite `Terminal.tsx` to drive a single leaf through the pool (panes come in B).
  Preserve overlay phases (spawning/connecting/ready/exited/error) + drag-drop.

### Phase B — Pane splitting
- Port `panes.ts` (1:1). Solid `PaneTreeView` (CSS fl: nested resizable split) +
  `TerminalStack`. Per-workspace pane tree state (frontend; persist later).
- Split/close/focus keybindings. New backend `workspace_spawn_pane` (claude-in-cwd
  with fresh uuid) and/or reuse `pty_spawn` for a shell companion. Decide default:
  start with a shell companion (most useful next to claude); make it a choice.
- Focus follows mouse/Tab; closing collapses single-child splits (panes.ts handles).

### Phase C — Theme system + backgrounds
- Copy theme DATA files 1:1 (`themes/*.ts`: catppuccin, nord, gruvbox, tokyo-night,
  rose-pine, claude, caffeine, sage, tide, terax-default). Translate ThemeProvider /
  applyTheme / SurfaceLayer / bgImageStore / customThemes / validateTheme to Solid
  signals + CSS variables. Map theme → xterm ITheme (extend terminalTheme.ts).
- Respect DESIGN.md: default stays warm-dark terracotta; themes are opt-in.

### Phase D — Settings window + shortcuts
- Translate Terax settings sections (General / Themes / Shortcuts / About) to Solid,
  integrating Tessera's existing `settings.ts` store. Configurable keybindings
  (shortcuts.ts + useGlobalShortcuts) → Solid. Decide: in-app modal (current) vs
  separate window. Start by extending the existing SettingsModal with Themes +
  Shortcuts tabs (less churn than a second window).

## NOTICE / licensing
Extend `NOTICE` to credit crynta/terax-ai for: renderer pool, dormantRing,
osc-handlers, da_filter, agent_detect, panes, theme system. Apache-2.0.

## Status log
- [done] Phase A — DA filter (crates/pty) + renderer pool w/ WebGL recovery
  (rendererPool.ts) + session registry (termSession.ts) + Terminal.tsx rewrite.
  Commit a6c18ec.
- [done] Phase C — theme presets (themes.ts, index.css blocks) + live picker in
  SettingsModal. Commit fd33416. (Background images deferred.)
- [done] Phase B + D — companion shell split panes (WorkspacePanes.tsx,
  ShellPane.tsx, pty_spawn_shell) + Shortcuts reference section. Commit 93393e2.
- [done] Adversarial-review fixes (WebGL re-entrancy, shell-spawn-race PTY leak,
  stuck-overlay fallback, TDZ hygiene). Commit fa97061.
- VERIFIED-AS-BUILDS only: cargo test --workspace (all green) + fmt + clippy +
  tsc + vite build + bun 1.3.11 (no lockfile drift). **GUI runtime UNVERIFIED**
  (no display here). Branch feat/terax-port, NOT merged, NOT released.
- NEXT: user GUI-tests on their machine (`cd ui && npm run tauri dev` on the
  branch, or build it). Then merge → main + cut a release. Do NOT release blind.
- Follow-ups noted: draggable pane resize + arbitrary nesting (currently flat
  even split); theme background images; configurable (not just reference)
  shortcuts; consider lifting the settings→pool effect to one global owner.

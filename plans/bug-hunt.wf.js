export const meta = {
  name: 'tessera-bug-hunt',
  description: 'Exhaustive user-facing bug hunt across the Tessera codebase, anchored on 3 known symptoms',
  phases: [
    { title: 'Hunt', detail: '12 dimension finders sweep for user-facing bugs' },
    { title: 'Verify', detail: 'adversarially verify every candidate finding' },
  ],
}

// ── Shared architecture brief handed to every agent ───────────────────────
const BRIEF = `
PROJECT: Tessera — a desktop app (Tauri 2 + SolidJS/TS frontend + Rust workspace
backend) that is a *Claude Code TUI viewer*. It spawns the \`claude\` CLI in a PTY,
parses the terminal with wezterm-term in Rust, and renders the grid via Canvas2D
in the WebView. Repo root: /home/save/Work/Tessera

ARCHITECTURE / DATA FLOW (read the real files — line numbers drift):
- PTY layer: crates/pty/src/{session.rs,supervisor.rs}. Supervisor holds
  HashMap<Uuid session_id, PtySession> and a tokio broadcast::channel(1024) of
  PtyEvent::{Data,Exit}. Each spawn starts an OS thread draining the PTY reader
  into the broadcast.
- Backend glue: src-tauri/src/{lib.rs,commands.rs,terminal.rs}.
  - lib.rs run(): a "pump" task drains supervisor.subscribe(), feeds bytes into
    TerminalRegistry, marks the session dirty. A "render-tick" task wakes every
    1ms (code) and emits "term_snapshot" for dirty sessions. Also: hook listener,
    inherit_shell_path (PATH cache), dispatch_hook (status + activity + notify +
    worktree detect).
  - terminal.rs: TerminalRegistry owns per-session wezterm-term Term + last_cells.
    snapshot() returns FULL grid (first/after-resize) or DELTA (changed cells +
    positions). Palette hot-swap clears last_cells. scroll_offset handling.
  - commands.rs: all #[tauri::command]s — pty_*, workspace_*, project_*,
    terminal_resize/scroll, settings_*, claude_inventory, extras (links/tasks/
    pomodoro), activity_list.
- Workspace/session mapping: crates/workspace/src/service.rs. WorkspaceService
  holds sessions: Mutex<HashMap<workspace_id, session_id>> (TRANSIENT, empty on
  restart) and statuses. spawn_agent inserts into that map. current_session reads it.
- Persistence: crates/store/src/{migrations.rs,workspaces.rs,projects.rs,extras.rs,
  activity.rs}. Single SQLite Connection behind Arc<Mutex>.
- Terminal emulation: crates/term/src/{term.rs,grid.rs,cell.rs,color.rs,palette.rs,
  cursor.rs,blocks.rs}.
- Hooks: crates/hook/src/{listener.rs,client.rs,event.rs,worktree_parse.rs}.
- Core types/config: crates/core/src/{config.rs,session.rs,workspace.rs,claude.rs,
  activity.rs,extras.rs}.
- Frontend (SolidJS, NOT React): ui/src/{App.tsx,Terminal.tsx,Sidebar.tsx,
  NewWorkspaceForm.tsx,ProjectsSettings.tsx,SettingsModal.tsx,WorkspaceExtrasPanel.tsx,
  ClaudeInventoryModal.tsx,PomodoroHud.tsx} and ui/src/lib/{ipc.ts,workspaces.ts,
  settings.ts,extras.ts,pomodoro.ts,claudeInventory.ts}, styles in ui/src/index.css.
  - Exactly ONE <Terminal> instance is mounted at a time (App.tsx <Show>); it is
    NOT keyed, so switching workspaces updates props on the same instance and an
    internal createEffect re-syncs activeSessionId. Snapshots are filtered by
    \`snap.session_id === activeSessionId\`.
  - THREE overlapping spawn-dedup layers: App.onSelect pre-spawn (80x24),
    lib/workspaces.ts inFlightSpawns map, Terminal.tsx \`spawning\` Set + its
    createEffect spawn branch.

THREE KNOWN USER-REPORTED SYMPTOMS (root-cause these inside your dimension if relevant):
  S1. Strange visual artifacts on screen (stray cells / glyphs / wrong colors).
  S2. The workspace area flickers / "vibrates" (jitter) when Tessera is launched.
  S3. The FIRST workspace always shows the WRONG session (someone else's session /
      not its own) — a session/workspace attribution bug.

WHAT COUNTS AS A BUG (these are "user-упячки"): anything a user would notice or that
produces wrong behavior — crashes/panics, data loss/corruption, races causing
flicker or wrong content, wrong session/workspace attribution, stale/leaked state,
broken keyboard/mouse/clipboard/scroll behavior, incorrect rendering, deadlocks,
dropped PTY output, notification/status errors, off-by-one, sign errors, leaks that
degrade the session over time, security/secret leaks. EXCLUDE pure code-style nits,
naming, and speculative "could be cleaner" with no user-visible consequence.

RULES: Cite real file paths + line numbers you actually read. Be concrete about the
USER-VISIBLE consequence and a trigger/repro. Prefer fewer high-confidence findings
over many speculative ones, but do not miss real bugs.`

const FINDING_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['findings'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['title', 'locations', 'symptom', 'user_consequence', 'trigger', 'root_cause', 'severity', 'confidence'],
        properties: {
          title: { type: 'string', description: 'one-line bug title' },
          locations: { type: 'array', items: { type: 'string' }, description: 'file:line references actually read' },
          symptom: { type: 'string' },
          user_consequence: { type: 'string', description: 'what the user sees/experiences' },
          trigger: { type: 'string', description: 'concrete steps/conditions that trigger it' },
          root_cause: { type: 'string' },
          maps_to_known_symptom: { type: 'string', enum: ['S1', 'S2', 'S3', 'none'] },
          severity: { type: 'string', enum: ['crit', 'high', 'med', 'low'] },
          fix_sketch: { type: 'string' },
          confidence: { type: 'number', description: '0..1' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['verdict', 'user_facing', 'severity', 'reasoning', 'confidence'],
  properties: {
    verdict: { type: 'string', enum: ['real', 'likely', 'refuted'] },
    user_facing: { type: 'boolean' },
    severity: { type: 'string', enum: ['crit', 'high', 'med', 'low'] },
    reasoning: { type: 'string', description: 'why real or refuted, citing code you re-read' },
    corrected_root_cause: { type: 'string' },
    repro: { type: 'string' },
    fix_sketch: { type: 'string' },
    confidence: { type: 'number' },
  },
}

const DIMENSIONS = [
  {
    key: 'session-lifecycle',
    focus: `Workspace<->session attribution & spawn lifecycle. THIS OWNS S3.
Trace EXACTLY what happens from app launch -> first workspace click -> first
snapshot, across App.tsx (onSelect pre-spawn, selected(), the non-keyed <Terminal>,
onSpawned, createResource mutate), Terminal.tsx (activeSessionId init,
setActiveSession, the two createEffects at the workspace/session change, the
\`spawning\` Set, listener filter snap.session_id===activeSessionId), lib/workspaces.ts
(inFlightSpawns dedup + .finally delete timing), service.rs (sessions map, spawn_agent,
current_session, spawn_agent overwrite), supervisor.spawn. Hunt for: double-spawn
races (two claude processes / second spawn after inFlightSpawns deletes its entry but
before props.sessionId propagates), stale session adopted by the first Terminal,
snapshot from session A painted into workspace B, activeSessionId initialized from a
prior workspace's session, sessions-map overwrite losing a live session, session id
reused/misrouted, '--continue' / has_prior_session logic.`,
    files: 'ui/src/App.tsx, ui/src/Terminal.tsx, ui/src/lib/workspaces.ts, crates/workspace/src/service.rs, crates/pty/src/supervisor.rs, src-tauri/src/lib.rs, src-tauri/src/commands.rs',
  },
  {
    key: 'canvas-render',
    focus: `Canvas2D rendering correctness. THIS OWNS S1 and shares S2.
Examine Terminal.tsx paintFull, paintCell, paintCursor, measureCell, resizeCanvasBacking,
applySnapshot, the cursor erase/restamp logic, blink timer, selection overlay canvas,
font metrics cache, dpr handling, run-batching in paintFull (bg runs, glyph runs, deco
buckets), clipping. Hunt for: stray/ghost cells (artifacts), wrong colors, cursor
left behind, glyph bleed, off-by-one in cell rects, grid padding, blank-cell skip in
delta vs full mismatch, baseline rounding, decorations drawn at wrong y, selection
overlay misalignment, repaint races causing flicker, canvas cleared then not repainted.`,
    files: 'ui/src/Terminal.tsx, src-tauri/src/terminal.rs, ui/src/index.css, ui/src/lib/settings.ts',
  },
  {
    key: 'snapshot-protocol',
    focus: `Backend snapshot/diff protocol & the 1ms render tick. Shares S1/S2.
Examine terminal.rs snapshot() full-vs-delta decision (needs_full when len/cols/rows
differ), the scratch/last_cells mem::swap choreography, palette hot-swap clearing
last_cells, scroll_offset reset on feed, set_scroll_delta, cursor_visible && scroll==0.
Also lib.rs pump + render-tick: dirty set, seen map, resize marking dirty,
terminal_resize (sizes map + registry.resize + dirty), terminal_scroll re-emit. Hunt
for: delta applied against a grid the frontend resized differently (cols/rows skew),
race where resize clears last_cells but frontend already advanced, snapshot for a
session whose Term was created at default 80x24 then mismatched, a Data chunk feeding
a Term at stale (cols,rows) from sizes map, lost first 'full' snapshot, dirty/seen
desync, flicker from full<->delta thrash, off-by-one in positions indexing.`,
    files: 'src-tauri/src/terminal.rs, src-tauri/src/lib.rs, src-tauri/src/commands.rs, ui/src/Terminal.tsx',
  },
  {
    key: 'term-emulation',
    focus: `wezterm-term wrapper correctness in crates/term. Shares S1.
Examine term.rs (feed, resize, cursor, grid, scrollback_max), grid.rs
(for_each_row_with_offset, row iteration, offset math, wide/zero-width cells),
cell.rs (attrs, default colors), color.rs + palette.rs (ANSI index -> rgb, bright,
256-color, truecolor, default fg/bg resolution), cursor.rs (shape/visibility),
blocks.rs (DCS/sixel/superscript). Hunt for: wrong color mapping, off-by-one row
offset in scrollback, wide-char/CJK/emoji cell handling, cursor position past grid,
resize losing content, default color fallback wrong, panics on malformed input.`,
    files: 'crates/term/src/term.rs, crates/term/src/grid.rs, crates/term/src/cell.rs, crates/term/src/color.rs, crates/term/src/palette.rs, crates/term/src/cursor.rs, crates/term/src/blocks.rs',
  },
  {
    key: 'solid-reactivity',
    focus: `SolidJS reactivity correctness across all components. Shares S2/S3.
Hunt for: stale-closure reads of signals, effects that should/shouldn't track a dep,
missing onCleanup (listener/interval/observer leaks), createResource mutate vs refetch
races, optimistic-update rollback bugs, derived signals recomputing wrongly, <Show>/<For>
keying issues (esp. the non-keyed Terminal), event listeners registered with capture
that swallow events, setInterval not cleared, double subscription on remount/HMR.
Cover App.tsx, Sidebar.tsx, the modals, PomodoroHud, lib/settings.ts onSettingsChanged,
lib/pomodoro.ts.`,
    files: 'ui/src/App.tsx, ui/src/Sidebar.tsx, ui/src/SettingsModal.tsx, ui/src/WorkspaceExtrasPanel.tsx, ui/src/ProjectsSettings.tsx, ui/src/PomodoroHud.tsx, ui/src/NewWorkspaceForm.tsx, ui/src/lib/settings.ts, ui/src/lib/pomodoro.ts',
  },
  {
    key: 'pty-concurrency',
    focus: `Rust concurrency in the PTY + pump path. Shares S2 and dropped-output.
CRITICAL: supervisor uses tokio broadcast::channel(1024). If the pump task (the only
subscriber) falls behind a fast claude flood, broadcast returns RecvError::Lagged and
SILENTLY DROPS messages — does the pump handle Lagged? (lib.rs 'while let Ok(evt) =
rx.recv().await' breaks the loop on Err — meaning a single Lagged KILLS the pump and
ALL terminal output freezes forever). Verify. Also: writer lock contention, resize/
kill races, the reader thread lifecycle, sessions map removal on exit vs in-flight
write, panics poisoning a Mutex, thread leaks.`,
    files: 'crates/pty/src/supervisor.rs, crates/pty/src/session.rs, src-tauri/src/lib.rs',
  },
  {
    key: 'store-persistence',
    focus: `SQLite persistence correctness. Examine migrations.rs (ordering,
idempotency, missing columns vs structs), workspaces.rs/projects.rs/extras.rs/
activity.rs queries (column order vs row mapping, NULL handling, sort_order
resequencing collisions, FK ON DELETE behavior, transaction atomicity), the single
Arc<Mutex<Connection>> shared by WorkspaceService AND extras commands (lock ordering,
held-across-await? deadlocks?). activity prune_keep_n correctness. Hunt for data
loss, wrong ordering shown to user, rows mapped to wrong fields, duplicate sort_order.`,
    files: 'crates/store/src/migrations.rs, crates/store/src/workspaces.rs, crates/store/src/projects.rs, crates/store/src/extras.rs, crates/store/src/activity.rs, crates/store/src/lib.rs, crates/core/src/workspace.rs',
  },
  {
    key: 'settings-config',
    focus: `Settings/config correctness end-to-end. Examine core/config.rs
(UserConfig defaults, HexColor validation, palette length, font bounds, deserialize of
partial/old configs, load_or_default on malformed file), commands settings_load/save,
lib/settings.ts (store, onSettingsChanged, setTerminalFontSize, save debounce),
SettingsModal, the legacy localStorage migrations in App.tsx (fontPx) and Terminal.tsx
(alwaysShowCursor). Hunt for: a malformed/old settings.json wiping user settings,
palette index out of range, font size clamp mismatch (UI vs backend), live-apply not
firing, debounce dropping the final save, hex validation gaps that crash palette_from_config.`,
    files: 'crates/core/src/config.rs, src-tauri/src/commands.rs, ui/src/lib/settings.ts, ui/src/SettingsModal.tsx, ui/src/SettingsPreview.tsx, ui/src/App.tsx',
  },
  {
    key: 'sidebar-nav-reorder',
    focus: `Sidebar, drag-reorder, keyboard nav, project assignment, rename.
Examine Sidebar.tsx (sectioning active/passive, sort by sort_order then created_at,
drag-and-drop reorder, status dot, rename inline, 3-dot menu), App.tsx onReorder
(sort_order gap=10 recompute, optimistic mutate, the active/passive split — does
reorder of one section corrupt the other's ordering or collide sort_order across
sections?), onGlobalKey (Cmd/Ctrl+1..9 and [ ] — index math, e.key range, capture
phase stopImmediatePropagation interfering with terminal). Hunt for: reorder writing
wrong order, cross-section sort_order collision, keyboard switch selecting wrong
workspace, rename losing focus/data, drag ghost, off-by-one in quick-switch index.`,
    files: 'ui/src/Sidebar.tsx, ui/src/App.tsx, ui/tests/Sidebar.test.tsx, ui/src/lib/workspaces.ts',
  },
  {
    key: 'hooks-activity',
    focus: `Claude hook ingestion -> status/activity/notifications. Examine
hook/listener.rs (socket bind, parsing, framing, partial reads), client.rs (send_event),
event.rs (HookEvent/HookKind), worktree_parse.rs (parse_worktree_add command parsing —
quoting, flags, branch extraction), lib.rs dispatch_hook (status mapping, activity
insert+prune, notification focus check, worktree detect emit), run_hook CLI. Hunt for:
wrong status shown, hook for workspace A updating B, notification firing when focused
or not firing, activity payload truncation/UTF-8 (already patched — verify), worktree
parse false positives/negatives, socket race / multiple writers, status dot stuck.`,
    files: 'crates/hook/src/listener.rs, crates/hook/src/client.rs, crates/hook/src/event.rs, crates/hook/src/worktree_parse.rs, src-tauri/src/lib.rs, crates/core/src/activity.rs',
  },
  {
    key: 'input-keyboard-clipboard',
    focus: `Keyboard/mouse/clipboard/scroll/drag-drop input correctness in Terminal.tsx.
Examine encodeKey (ctrl-letter range, function keys, Enter/Tab/Backspace, missing keys
like Shift+Tab/Ctrl+arrows/Alt/F-keys, IME composition/dead keys — does it send garbage
during composition? isComposing checked?), the keydown handler (zoom, copy/paste branch,
ctrl+c SIGINT passthrough vs copy, the global App keydown capturing Cmd/Ctrl+digits
BEFORE the terminal sees them), wheel->scroll sign and accumulation, drag-drop path
gating + bracketed paste, paste image flow. Hunt for: keys that don't reach claude,
wrong escape sequences, IME breakage, copy/paste swallowed, scroll inverted, Cmd+1..9
stolen from terminal, modifier combos mis-encoded.`,
    files: 'ui/src/Terminal.tsx, ui/src/App.tsx',
  },
  {
    key: 'lifecycle-updater-misc',
    focus: `App lifecycle & miscellaneous user-facing surfaces. Examine the in-app
updater (App.tsx: poll interval cleanup, error display, downloadAndInstall progress,
relaunch), single-instance plugin (lib.rs — second launch shows window; but does the
2nd instance still try to open the DB/socket and error first? order of plugin init),
inherit_shell_path (PATH probe timeout, Linux vs macOS, missing claude -> stuck at
'starting claude'), useClock interval, version fetch, save_paste_image (no cleanup,
path injection), claude_inventory (env-value leak claim — verify it truly never returns
env VALUES), ClaudeInventoryModal, lazy modal prefetch. Hunt for: updater stuck/looping,
second instance corrupting state, PATH probe failing silently, secret leak in inventory,
unbounded paste dir, clock drift.`,
    files: 'ui/src/App.tsx, src-tauri/src/lib.rs, src-tauri/src/commands.rs, ui/src/ClaudeInventoryModal.tsx, ui/src/lib/claudeInventory.ts, crates/core/src/claude.rs',
  },
]

phase('Hunt')

const results = await pipeline(
  DIMENSIONS,
  // Stage 1 — dimension finder
  (dim) =>
    agent(
      `${BRIEF}

YOU ARE THE "${dim.key}" BUG FINDER.

Your dimension focus:
${dim.focus}

Primary files (read these and anything they call into; line numbers in the brief are stale, read fresh):
${dim.files}

Read the actual code carefully. Reason about concrete execution sequences and races.
Return EVERY genuine user-facing bug you can substantiate in this dimension, each with
real file:line citations, the user-visible consequence, a concrete trigger, the root
cause, a fix sketch, and a calibrated confidence. If a finding root-causes one of the
three known symptoms (S1/S2/S3), set maps_to_known_symptom. Do NOT pad with style nits.`,
      { label: `find:${dim.key}`, phase: 'Hunt', schema: FINDING_SCHEMA },
    ).catch(() => ({ findings: [] })),
  // Stage 2 — adversarially verify each finding from this dimension
  (found, dim) => {
    const findings = (found && found.findings) || []
    if (findings.length === 0) return []
    return parallel(
      findings.map((f) => () =>
        agent(
          `${BRIEF}

ADVERSARIAL VERIFICATION. A finder claims this bug exists in dimension "${dim.key}":

  TITLE: ${f.title}
  LOCATIONS: ${(f.locations || []).join(', ')}
  SYMPTOM: ${f.symptom}
  USER CONSEQUENCE: ${f.user_consequence}
  TRIGGER: ${f.trigger}
  CLAIMED ROOT CAUSE: ${f.root_cause}
  MAPS TO KNOWN SYMPTOM: ${f.maps_to_known_symptom || 'none'}
  CLAIMED SEVERITY: ${f.severity}
  FINDER CONFIDENCE: ${f.confidence}

Your job is to REFUTE it. Open the cited files and surrounding code yourself. Check
whether existing guards/comments already prevent it, whether the control flow actually
reaches the buggy state, whether the consequence is truly user-visible, and whether the
severity is right. Default toward 'refuted' if you cannot independently confirm a
concrete user-visible failure from the real code. Return 'real' only if you traced a
plausible execution path to the bad outcome, 'likely' if probable but not fully proven.
Correct the root cause / severity / repro if the finder got them wrong.`,
          { label: `verify:${dim.key}:${(f.title || '').slice(0, 28)}`, phase: 'Verify', schema: VERDICT_SCHEMA },
        )
          .then((v) => ({ finding: { ...f, dimension: dim.key }, verdict: v }))
          .catch(() => null),
      ),
    )
  },
)

// Flatten + keep confirmed, user-facing findings.
const all = results.flat().filter(Boolean)
const confirmed = all.filter(
  (x) => x.verdict && x.verdict.verdict !== 'refuted' && x.verdict.user_facing,
)
const refuted = all.filter((x) => x.verdict && x.verdict.verdict === 'refuted')

const sevRank = { crit: 0, high: 1, med: 2, low: 3 }
confirmed.sort(
  (a, b) =>
    (sevRank[a.verdict.severity] ?? 9) - (sevRank[b.verdict.severity] ?? 9),
)

log(`Hunt complete: ${all.length} candidates, ${confirmed.length} confirmed user-facing, ${refuted.length} refuted`)

return {
  confirmed: confirmed.map((x) => ({
    title: x.finding.title,
    dimension: x.finding.dimension,
    maps_to: x.finding.maps_to_known_symptom || 'none',
    severity: x.verdict.severity,
    verdict: x.verdict.verdict,
    confidence: x.verdict.confidence,
    locations: x.finding.locations,
    user_consequence: x.finding.user_consequence,
    trigger: x.verdict.repro || x.finding.trigger,
    root_cause: x.verdict.corrected_root_cause || x.finding.root_cause,
    fix_sketch: x.verdict.fix_sketch || x.finding.fix_sketch,
    reasoning: x.verdict.reasoning,
  })),
  refuted_count: refuted.length,
  candidate_count: all.length,
}

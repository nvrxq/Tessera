import {
  createEffect,
  createResource,
  createSignal,
  lazy,
  onCleanup,
  onMount,
  Show,
  Suspense,
  type Component,
} from "solid-js";
import { getVersion } from "@tauri-apps/api/app";
import { message } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkForAppUpdate, type Update } from "@tauri-apps/plugin-updater";
import Sidebar from "./Sidebar";
import NewWorkspaceForm from "./NewWorkspaceForm";
import ProjectsSettings from "./ProjectsSettings";
import Terminal from "./Terminal";
// Modals are heavy and opened on demand. Lazy-load them so the initial
// bundle drops the SettingsModal/ClaudeInventoryModal/WorkspaceExtrasPanel
// payloads — the first click pays a one-frame fetch, every subsequent open
// hits the in-memory cache. Wrapped in <Suspense fallback={null}> since a
// blank frame on a button-click is invisible.
const SettingsModal = lazy(() => import("./SettingsModal"));
const ClaudeInventoryModal = lazy(() => import("./ClaudeInventoryModal"));
const WorkspaceExtrasPanel = lazy(() => import("./WorkspaceExtrasPanel"));
import {
  DEFAULT_CONFIG,
  loadSettings,
  onSettingsChanged,
  saveSettings,
  setSettings,
  settings,
} from "./lib/settings";
import {
  archiveWorkspace,
  createProject,
  deleteProject,
  deleteWorkspace,
  listArchivedWorkspaces,
  listProjects,
  listWorkspaces,
  onWorkspaceStatus,
  onWorkspaceWorktree,
  renameWorkspace,
  resetWorkspaceSession,
  spawnAgent,
  unarchiveWorkspace,
  workspaceAssignProject,
  workspaceReorder,
  type Project,
  type WorkspaceDto,
} from "./lib/workspaces";

function useClock() {
  const [time, setTime] = createSignal(new Date());
  let id: number | null = null;
  onMount(() => {
    id = window.setInterval(() => setTime(new Date()), 30_000);
  });
  onCleanup(() => {
    if (id != null) window.clearInterval(id);
  });
  return () => {
    const d = time();
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    return `${hh}:${mm}`;
  };
}

const App: Component = () => {
  const [workspaces, { mutate, refetch }] = createResource<WorkspaceDto[]>(listWorkspaces);
  const [archivedWorkspaces, { mutate: mutateArchived, refetch: refetchArchived }] =
    createResource<WorkspaceDto[]>(listArchivedWorkspaces);
  const [projects, { refetch: refetchProjects }] =
    createResource<Project[]>(listProjects);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [showNew, setShowNew] = createSignal(false);
  const [showProjects, setShowProjects] = createSignal(false);
  // Right-side extras panel (Links / Tasks / Pomodoro). Each workspace
  // remembers its own open/closed state under `tessera.extrasOpen:<ws id>`
  // so toggling on workspace A doesn't pop the panel open on workspace B
  // — different workspaces have different rhythms (background daemons vs
  // active pomodoro tracking) and the panel's visibility should follow
  // that. Brand-new workspaces default to closed.
  const extrasKey = (id: string) => `tessera.extrasOpen:${id}`;
  const [showExtras, setShowExtras] = createSignal(false);
  const toggleExtras = () => {
    setShowExtras((v) => {
      const next = !v;
      const id = selectedId();
      if (id) localStorage.setItem(extrasKey(id), next ? "1" : "0");
      return next;
    });
  };
  const [showSettings, setShowSettings] = createSignal(false);
  const [showInventory, setShowInventory] = createSignal(false);
  const clock = useClock();

  let unlistenStatus: (() => void) | null = null;
  let unlistenWorktree: (() => void) | null = null;
  let unlistenSettings: (() => void) | null = null;

  // Push the user-controlled terminal background into a CSS variable so
  // the canvas host (.overlay-anchor) and any other surface that wants
  // to feel "in-terminal" can read it. We re-apply on every settings
  // change. Keeping this in App (not Terminal) means even unrelated
  // chrome stays in sync.
  createEffect(() => {
    const cfg = settings();
    document.documentElement.style.setProperty(
      "--terminal-bg",
      cfg.terminal.background,
    );
    document.documentElement.style.setProperty(
      "--terminal-fg",
      cfg.terminal.foreground,
    );
    // Apply the (previously dead) appearance controls. The UI font flows
    // into --font-ui — overriding the static index.css default — and the
    // density choice toggles a documentElement data attribute that the
    // CSS reads to relax/compact spacing. Access defensively: the type
    // lives in lib/settings.ts (another owner) and older persisted configs
    // may predate the appearance block.
    const uiFont = cfg.appearance?.ui_font_family;
    if (uiFont) {
      document.documentElement.style.setProperty("--font-ui", uiFont);
    }
    document.documentElement.dataset.density =
      cfg.appearance?.density ?? "compact";
  });

  // ── In-app updater ─────────────────────────────────────────────
  // The topbar always shows the running version. Clicking it triggers
  // a re-check; the pill next to it shows the current update lifecycle
  // state (checking → up-to-date / available / downloading / error).
  // Surfacing every state — including errors — instead of silently
  // hiding them is the only way to debug an updater that doesn't appear
  // to fire, which was exactly the bug at v0.1.0→v0.1.1 cutover.
  type UpdateState =
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "uptodate" }
    | { kind: "available"; update: Update }
    | { kind: "downloading"; update: Update; progress: number }
    | { kind: "error"; message: string };
  const [updateState, setUpdateState] = createSignal<UpdateState>({ kind: "idle" });
  const [version, setVersion] = createSignal<string>("");
  void getVersion().then(setVersion).catch(() => setVersion("?"));

  const runUpdateCheck = async () => {
    // Re-entrancy guard: a check (manual click or the 30-min poll) must not
    // interrupt an in-progress download — resetting to "checking" would hide
    // the progress UI and could kick off a SECOND concurrent download/relaunch.
    if (updateState().kind === "downloading") return;
    setUpdateState({ kind: "checking" });
    try {
      const update = await checkForAppUpdate();
      if (update?.available) {
        setUpdateState({ kind: "available", update });
      } else {
        setUpdateState({ kind: "uptodate" });
      }
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
      console.warn("update check failed", e);
    }
  };

  // Check immediately on mount, then poll every 30 min — long-running
  // sessions still discover new releases without forcing a relaunch.
  void runUpdateCheck();
  const updatePollId = window.setInterval(runUpdateCheck, 30 * 60 * 1000);
  onCleanup(() => window.clearInterval(updatePollId));

  const applyUpdate = async () => {
    const st = updateState();
    if (st.kind !== "available") return;
    setUpdateState({ kind: "downloading", update: st.update, progress: 0 });
    try {
      let total = 0;
      let received = 0;
      await st.update.downloadAndInstall((evt) => {
        if (evt.event === "Started") {
          total = evt.data.contentLength ?? 0;
        } else if (evt.event === "Progress") {
          received += evt.data.chunkLength;
          const pct = total > 0 ? Math.min(100, (received / total) * 100) : 0;
          setUpdateState({ kind: "downloading", update: st.update, progress: pct });
        }
      });
      // Install is complete on disk; relaunch swaps over to it.
      await relaunch();
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
    }
  };

  onMount(async () => {
    // Pull the persisted settings into the live store before any
    // children render — Terminal reads `settings()` synchronously on
    // setup, so racing the load means it would flash with defaults
    // before the user's customisation takes effect.
    try {
      const cfg = await loadSettings();
      // One-time migration: pre-settings.json builds stored the
      // terminal font size in `localStorage.tessera.fontPx`. If the
      // freshly-loaded config still carries the default size and the
      // legacy key exists, adopt it and persist so the modal /
      // settings file become the single source of truth.
      const legacy = localStorage.getItem("tessera.fontPx");
      if (
        legacy &&
        cfg.terminal.font_size_px === DEFAULT_CONFIG.terminal.font_size_px
      ) {
        const px = Number(legacy);
        if (Number.isFinite(px) && px >= 8 && px <= 32) {
          cfg.terminal.font_size_px = Math.round(px);
          try {
            await saveSettings(cfg);
          } catch (e) {
            console.warn("legacy fontPx migration save failed", e);
          }
        }
        localStorage.removeItem("tessera.fontPx");
      }
      setSettings(cfg);
    } catch (e) {
      console.warn("settings_load failed; using defaults", e);
    }
    unlistenSettings = await onSettingsChanged((cfg) => setSettings(cfg));
    // Idle-prefetch the lazy modal chunks. By the time the user first
    // clicks Settings / Inventory / Extras the JS is already in the
    // module cache, so opening is instant instead of "spinner →
    // chunk fetch → mount". `requestIdleCallback` waits for an idle
    // slot so first paint isn't slowed down; we also defer through a
    // microtask + ~200 ms timeout fallback for Safari/WebView2 which
    // don't ship `requestIdleCallback`.
    queueMicrotask(() => {
      const ric: (cb: () => void) => void =
        (window as unknown as { requestIdleCallback?: (cb: () => void) => void })
          .requestIdleCallback ?? ((cb: () => void) => setTimeout(cb, 200));
      ric(() => {
        void import("./SettingsModal");
        void import("./ClaudeInventoryModal");
        void import("./WorkspaceExtrasPanel");
      });
    });
    unlistenStatus = await onWorkspaceStatus((evt) => {
      // Equality guard: if the status is unchanged, return the SAME object
      // reference so <For> doesn't dispose+recreate that row. Re-creating a
      // row mid-rename would blow away the inline <input>'s live text.
      mutate((list) =>
        list?.map((w) =>
          w.id === evt.workspace_id
            ? w.agent_status === evt.agent_status
              ? w
              : { ...w, agent_status: evt.agent_status }
            : w,
        ) ?? list,
      );
    });
    unlistenWorktree = await onWorkspaceWorktree((evt) => {
      // Same equality guard as status — only swap the ref when a worktree
      // field actually changed, so an idle re-detect doesn't churn the row.
      mutate((list) =>
        list?.map((w) =>
          w.id === evt.workspace_id
            ? w.detected_worktree === evt.detected_worktree &&
              w.detected_branch === evt.detected_branch
              ? w
              : {
                  ...w,
                  detected_worktree: evt.detected_worktree,
                  detected_branch: evt.detected_branch,
                }
            : w,
        ) ?? list,
      );
    });
  });
  // Keyboard quick-switch. Mirrors the Sidebar's section + sort rule
  // (active section first, then passive; each sorted by sort_order then
  // created_at) so what the user sees in the sidebar is exactly the
  // order Cmd/Ctrl + N walks. Skipped when focus is in a text input —
  // those should keep typing normally.
  const orderedWorkspaceList = (): WorkspaceDto[] => {
    const all = workspaces() ?? [];
    const active: WorkspaceDto[] = [];
    const passive: WorkspaceDto[] = [];
    for (const w of all) {
      (w.session_id != null ? active : passive).push(w);
    }
    const cmp = (a: WorkspaceDto, b: WorkspaceDto) =>
      a.sort_order !== b.sort_order
        ? a.sort_order - b.sort_order
        : a.created_at.localeCompare(b.created_at);
    active.sort(cmp);
    passive.sort(cmp);
    return active.concat(passive);
  };
  const onGlobalKey = (e: KeyboardEvent) => {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const target = e.target as HTMLElement | null;
    if (target) {
      const tag = target.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable) {
        return;
      }
    }
    const list = orderedWorkspaceList();
    if (list.length === 0) return;
    // Match on the PHYSICAL key (e.code) rather than e.key: on AZERTY and
    // other non-US layouts the digit row needs Shift, so e.key would be
    // "&", "é", … instead of "1".."9". e.code is layout-independent, so the
    // same physical 1-key works everywhere — and on US it's unchanged.
    const digit = /^Digit([1-9])$/.exec(e.code);
    if (digit) {
      const idx = Number(digit[1]) - 1;
      if (idx >= list.length) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      onSelect(list[idx].id);
      return;
    }
    if (e.code === "BracketRight" || e.code === "BracketLeft") {
      e.preventDefault();
      e.stopImmediatePropagation();
      const curIdx = list.findIndex((w) => w.id === selectedId());
      const step = e.code === "BracketRight" ? 1 : -1;
      const base = curIdx < 0 ? (step > 0 ? -1 : 0) : curIdx;
      const next = ((base + step) % list.length + list.length) % list.length;
      onSelect(list[next].id);
    }
  };
  onMount(() => document.addEventListener("keydown", onGlobalKey, true));
  onCleanup(() => {
    unlistenStatus?.();
    unlistenWorktree?.();
    unlistenSettings?.();
    document.removeEventListener("keydown", onGlobalKey, true);
  });

  const selected = () => workspaces()?.find((w) => w.id === selectedId()) ?? null;

  // When the user switches workspaces, restore that workspace's saved
  // extras-panel state. Unknown/new ids default to closed.
  createEffect(() => {
    const id = selectedId();
    if (!id) {
      setShowExtras(false);
      return;
    }
    setShowExtras(localStorage.getItem(extrasKey(id)) === "1");
  });

  const onSelect = (id: string) => {
    setSelectedId(id);
    // Pre-warm claude: kick off `workspace_spawn_agent` the moment the
    // workspace is clicked, in parallel with `<Terminal>` mounting. This
    // overlaps claude's ~200–500 ms cold start with the component setup
    // and listener registration — by the time Terminal asks for a session,
    // the spawn may already have resolved. `spawnAgent` is dedupe'd
    // (lib/workspaces.ts), so Terminal's own spawn call will get the same
    // session id rather than launching a second claude. We use a default
    // 80×24 grid here; Terminal calls `terminal_resize` after measuring
    // its container, which the backend handles instantly.
    const ws = workspaces()?.find((w) => w.id === id);
    if (ws && !ws.session_id) {
      void spawnAgent(id, 80, 24)
        .then((sid) => {
          mutate(
            (list) =>
              list?.map((w) => (w.id === id ? { ...w, session_id: sid } : w)) ??
              list,
          );
        })
        .catch(() => {
          /* Terminal will retry from its own createEffect */
        });
    }
  };

  const onTerminalSpawned = (workspaceId: string, sessionId: string) => {
    mutate((list) =>
      list?.map((w) => (w.id === workspaceId ? { ...w, session_id: sessionId } : w)) ?? list,
    );
  };

  const onCreated = (ws: WorkspaceDto) => {
    mutate((list) => (list ? [ws, ...list] : [ws]));
    setSelectedId(ws.id);
    setShowNew(false);
  };

  // Reorder a section of workspaces. The Sidebar passes the new ordered list
  // of ids for either the "active" or "passive" section; we recompute
  // sort_order (gapped by 10 so future inserts don't trigger a full
  // resequence) and push the update to the backend. Optimistic: we mutate
  // the local list first, then call workspace_reorder; on failure we refetch.
  const onReorder = async (
    section: "active" | "passive",
    orderedIds: string[],
  ) => {
    // sort_order is a SINGLE global column, but the sidebar shows two
    // sections (active = has session_id, passive = none). If we only
    // sequenced the reordered section's ids, both sections would reuse
    // 10,20,30… and collide — so positions jump whenever a workspace
    // crosses sections (session start/stop, restart). Fix: rebuild the
    // FULL display order, splice the new section order into its slice,
    // then assign one monotonic gapped sequence across the whole list so
    // every sort_order is globally unique and stable.
    const full = orderedWorkspaceList();
    const reorderedSet = new Set(orderedIds);
    // Guard against a section-membership shift between the drag start (when
    // the Sidebar captured orderedIds) and now — e.g. a session started or
    // ended mid-drag, moving a row active↔passive. If the count no longer
    // matches, the positional consumption below would consume an orderedId
    // for a row that's no longer in the section, duplicating one id and
    // dropping another. Bail to a server refetch rather than persist a
    // corrupted order.
    const inSectionCount = full.filter(
      (w) =>
        (section === "active" ? w.session_id != null : w.session_id == null) &&
        reorderedSet.has(w.id),
    ).length;
    if (inSectionCount !== orderedIds.length) {
      refetch();
      return;
    }
    // Walk the full display order; where the reordered section's rows sit,
    // emit them in the NEW order (consumed left-to-right), leaving the
    // other section's rows untouched in place.
    let cursor = 0;
    const merged: WorkspaceDto[] = full.map((w) => {
      const inSection =
        (section === "active" ? w.session_id != null : w.session_id == null) &&
        reorderedSet.has(w.id);
      if (!inSection) return w;
      const id = orderedIds[cursor++];
      return full.find((x) => x.id === id)!;
    });
    // Gap of 10 between rows so a future single-row insert can land between
    // two neighbours without a full resequence. Wire shape is named-struct
    // to match Rust `ReorderEntry` (see lib/workspaces.ts).
    const updates = merged.map((w, idx) => ({
      workspace_id: w.id,
      sort_order: (idx + 1) * 10,
    }));
    const order = new Map(updates.map((u) => [u.workspace_id, u.sort_order]));
    mutate((list) =>
      list?.map((w) =>
        order.has(w.id) ? { ...w, sort_order: order.get(w.id)! } : w,
      ) ?? list,
    );
    try {
      await workspaceReorder(updates);
    } catch (e) {
      console.error("workspace_reorder failed", e);
      refetch();
    }
  };

  const onProjectsChanged = () => {
    refetchProjects();
  };

  /** Workspace's 3-dot menu → "assign project". Optimistic mutate so the
   *  chip swap feels instant; on backend failure we refetch the list. */
  const onAssignProject = async (
    workspaceId: string,
    projectId: string | null,
  ) => {
    mutate((list) =>
      list?.map((w) =>
        w.id === workspaceId ? { ...w, project_id: projectId } : w,
      ) ?? list,
    );
    try {
      await workspaceAssignProject(workspaceId, projectId);
    } catch (e) {
      console.error("workspace_assign_project failed", e);
      refetch();
    }
  };

  /** ProjectsSettings → create. We refresh the projects list on success
   *  so the new entry shows up everywhere it's used (Sidebar chips,
   *  3-dot menus, NewWorkspaceForm dropdown). */
  const onCreateProject = async (name: string, accent: string | null) => {
    await createProject(name, accent);
    refetchProjects();
  };

  /** ProjectsSettings → delete. The backend FK on workspaces.project_id
   *  is ON DELETE SET NULL, so any workspace still pointing at this
   *  project ends up un-assigned. We refetch BOTH lists so chips
   *  disappear from those workspaces immediately. */
  const onDeleteProject = async (id: string) => {
    await deleteProject(id);
    refetchProjects();
    refetch();
  };

  const onRename = async (id: string, newName: string) => {
    mutate(
      (list) =>
        list?.map((w) => (w.id === id ? { ...w, name: newName } : w)) ?? list,
    );
    try {
      await renameWorkspace(id, newName);
    } catch (e) {
      console.error("rename failed", e);
      refetch();
    }
  };

  const onDelete = async (id: string) => {
    try {
      await deleteWorkspace(id);
      mutate((list) => list?.filter((w) => w.id !== id) ?? list);
      mutateArchived((list) => list?.filter((w) => w.id !== id) ?? list);
      if (selectedId() === id) setSelectedId(null);
    } catch (e) {
      void message(`Delete failed: ${String(e)}`, { kind: "error", title: "Tessera" });
      refetch();
      refetchArchived();
    }
  };

  /** Soft-archive: row stays in the DB (with its pinned Claude session)
   *  but disappears from the main sidebar. Selection clears so the user
   *  doesn't end up staring at a terminal for a row that's now hidden. */
  const onArchive = async (id: string) => {
    try {
      await archiveWorkspace(id);
      if (selectedId() === id) setSelectedId(null);
      refetch();
      refetchArchived();
    } catch (e) {
      void message(`Archive failed: ${String(e)}`, { kind: "error", title: "Tessera" });
      refetch();
      refetchArchived();
    }
  };

  const onUnarchive = async (id: string) => {
    try {
      await unarchiveWorkspace(id);
      refetch();
      refetchArchived();
    } catch (e) {
      void message(`Restore failed: ${String(e)}`, { kind: "error", title: "Tessera" });
      refetch();
      refetchArchived();
    }
  };

  /** Drop the pinned Claude session uuid. Mutate locally so the menu's
   *  "Pinned to …" hint updates immediately; the next spawn re-runs
   *  detection and re-pins. */
  const onResetSession = async (id: string) => {
    mutate(
      (list) =>
        list?.map((w) => (w.id === id ? { ...w, claude_session_id: null } : w)) ?? list,
    );
    try {
      await resetWorkspaceSession(id);
    } catch (e) {
      console.error("reset session failed", e);
      refetch();
    }
  };

  return (
    <div class="frame">
      <div class="card">
        <header class="topbar">
          <div class="brand">
            <span class="brand-mark" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="20" height="20" fill="none">
                <path
                  d="M3 4l5-1 4 1 4-1 5 1v8l-5 5-4 1-4-1-5-5V4z"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linejoin="round"
                />
              </svg>
            </span>
            <span class="brand-name">
              <span class="brand-name-strong">Tessera.</span>{" "}
              <span class="brand-name-tag">Agent orchestrator.</span>
            </span>
          </div>
          <div class="topmeta">
            <button
              type="button"
              class="topmeta-version"
              onClick={() => void runUpdateCheck()}
              // Disable while a check/download is already in flight so a stray
              // click can't restart the check or spawn a second download.
              disabled={
                updateState().kind === "checking" ||
                updateState().kind === "downloading"
              }
              title={
                updateState().kind === "downloading"
                  ? "Update in progress…"
                  : "Click to check for updates"
              }
            >
              v{version() || "…"}
            </button>
            {(() => {
              const st = updateState();
              if (st.kind === "checking") {
                return (
                  <span class="topmeta-update topmeta-update--busy" title="Checking for updates…">
                    Checking…
                  </span>
                );
              }
              if (st.kind === "uptodate") {
                return (
                  <span class="topmeta-update topmeta-update--ok" title="You're on the latest version">
                    ✓ Up to date
                  </span>
                );
              }
              if (st.kind === "available") {
                return (
                  <button
                    type="button"
                    class="topmeta-update"
                    onClick={applyUpdate}
                    title={`Install Tessera v${st.update.version} and relaunch.${st.update.body ? "\n\n" + st.update.body : ""}`}
                  >
                    <span aria-hidden="true">↑</span> Update v{st.update.version}
                  </button>
                );
              }
              if (st.kind === "downloading") {
                return (
                  <span class="topmeta-update topmeta-update--busy" title="Downloading update…">
                    Updating… {Math.round(st.progress)}%
                  </span>
                );
              }
              if (st.kind === "error") {
                return (
                  <button
                    type="button"
                    class="topmeta-update topmeta-update--error"
                    onClick={() => void runUpdateCheck()}
                    title={`Update check failed:\n${st.message}\n\nClick to retry.`}
                  >
                    ⚠ {st.message.slice(0, 40)}{st.message.length > 40 ? "…" : ""}
                  </button>
                );
              }
              return null;
            })()}
            <span class="topmeta-clock">{clock()}</span>
            <button
              type="button"
              class="topmeta-mode"
              onClick={() => setShowProjects(true)}
              title="Projects"
              aria-label="Open projects settings"
            >
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden="true">
                <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.6" />
                <path
                  d="M19.4 13a7.5 7.5 0 0 0 0-2l2-1.5-2-3.5-2.4 1a7.6 7.6 0 0 0-1.7-1L14.5 3h-5L9 6a7.6 7.6 0 0 0-1.7 1l-2.4-1-2 3.5L4.6 11a7.5 7.5 0 0 0 0 2L2.6 14.5l2 3.5 2.4-1c.52.4 1.1.74 1.7 1l.5 3h5l.5-3c.6-.26 1.18-.6 1.7-1l2.4 1 2-3.5L19.4 13z"
                  stroke="currentColor"
                  stroke-width="1.4"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
            <button
              type="button"
              class="topmeta-mode"
              onClick={() => setShowInventory(true)}
              title="Claude inventory (skills + MCP)"
              aria-label="Open Claude inventory"
            >
              {/* Sparkles-on-a-page: a stand-in for "everything Claude
                  will see on launch" — skills + MCP servers, the surfaces
                  the agent reads at boot. */}
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden="true">
                <path
                  d="M6 3h9l4 4v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linejoin="round"
                />
                <path d="M15 3v4h4" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
                <path
                  d="M11.5 11l.7 1.7 1.8.4-1.4 1.2.4 1.8-1.5-1-1.5 1 .4-1.8L9 13.1l1.8-.4.7-1.7z"
                  fill="currentColor"
                />
              </svg>
            </button>
            <button
              type="button"
              class="topmeta-mode"
              onClick={() => setShowSettings(true)}
              title="Settings"
              aria-label="Open settings"
            >
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden="true">
                <line x1="4" y1="7" x2="20" y2="7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
                <line x1="4" y1="12" x2="20" y2="12" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
                <line x1="4" y1="17" x2="20" y2="17" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
                <circle cx="9" cy="7" r="2.2" fill="currentColor" />
                <circle cx="15" cy="12" r="2.2" fill="currentColor" />
                <circle cx="8" cy="17" r="2.2" fill="currentColor" />
              </svg>
            </button>
          </div>
        </header>

        <div class="layout">
          <Sidebar
            workspaces={workspaces() ?? []}
            archived={archivedWorkspaces() ?? []}
            projects={projects() ?? []}
            selectedId={selectedId()}
            onSelect={onSelect}
            onDelete={onDelete}
            onNew={() => setShowNew(true)}
            onReorder={onReorder}
            onAssignProject={onAssignProject}
            onRename={onRename}
            onArchive={onArchive}
            onUnarchive={onUnarchive}
            onResetSession={onResetSession}
          />
          <main class="main-pane">
            <Show when={showNew()}>
              <NewWorkspaceForm
                projects={projects() ?? []}
                onCreated={onCreated}
                onCancel={() => setShowNew(false)}
                onProjectsChanged={onProjectsChanged}
              />
            </Show>
            <Show when={!showNew() && selected()?.id}>
              <div class="workspace-shell">
                <button
                  type="button"
                  class="workspace-extras-toggle"
                  classList={{ "workspace-extras-toggle--active": showExtras() }}
                  onClick={toggleExtras}
                  title={showExtras() ? "Hide extras panel" : "Show extras panel"}
                  aria-label="Toggle extras panel"
                  aria-pressed={showExtras()}
                >
                  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden="true">
                    <rect x="3" y="4" width="13" height="16" rx="1.5" stroke="currentColor" stroke-width="1.6" />
                    <rect x="17" y="4" width="4" height="16" rx="1" fill="currentColor" />
                  </svg>
                </button>
                <Terminal
                  workspaceId={selected()!.id}
                  sessionId={selected()?.session_id ?? null}
                  onSpawned={(sid) => onTerminalSpawned(selected()!.id, sid)}
                />
                <Show when={showExtras()}>
                  <Suspense fallback={null}>
                    <WorkspaceExtrasPanel
                      workspaceId={selected()!.id}
                      onClose={() => {
                        setShowExtras(false);
                        localStorage.setItem(
                          extrasKey(selected()!.id),
                          "0",
                        );
                      }}
                    />
                  </Suspense>
                </Show>
              </div>
            </Show>
            <Show when={!showNew() && !selected()}>
              <section class="hero">
                <div class="hero-mosaic" aria-hidden="true">
                  <span /><span /><span /><span />
                  <span /><span /><span /><span />
                  <span /><span /><span /><span />
                </div>
              </section>
            </Show>
          </main>
        </div>
      </div>
      <Show when={showProjects()}>
        <ProjectsSettings
          projects={projects() ?? []}
          onClose={() => setShowProjects(false)}
          onCreate={onCreateProject}
          onDelete={onDeleteProject}
        />
      </Show>
      <Show when={showSettings()}>
        <Suspense fallback={null}>
          <SettingsModal onClose={() => setShowSettings(false)} />
        </Suspense>
      </Show>
      <Show when={showInventory()}>
        <Suspense fallback={null}>
          <ClaudeInventoryModal
            workspaceId={selected()?.id ?? null}
            workspaceLabel={selected()?.name ?? null}
            onClose={() => setShowInventory(false)}
          />
        </Suspense>
      </Show>
    </div>
  );
};

export default App;

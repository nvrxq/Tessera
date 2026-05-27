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
  createProject,
  deleteProject,
  deleteWorkspace,
  listProjects,
  listWorkspaces,
  onWorkspaceStatus,
  onWorkspaceWorktree,
  spawnAgent,
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

type Theme = "dark" | "light";
function initialTheme(): Theme {
  const stored = localStorage.getItem("tessera.theme");
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}
function applyTheme(t: Theme) {
  document.documentElement.setAttribute("data-theme", t);
  localStorage.setItem("tessera.theme", t);
}

const App: Component = () => {
  const [workspaces, { mutate, refetch }] = createResource<WorkspaceDto[]>(listWorkspaces);
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
  const [theme, setTheme] = createSignal<Theme>(initialTheme());
  applyTheme(theme());
  const toggleTheme = () => {
    const next: Theme = theme() === "dark" ? "light" : "dark";
    setTheme(next);
    applyTheme(next);
  };

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
      mutate((list) =>
        list?.map((w) =>
          w.id === evt.workspace_id ? { ...w, agent_status: evt.agent_status } : w,
        ) ?? list,
      );
    });
    unlistenWorktree = await onWorkspaceWorktree((evt) => {
      mutate((list) =>
        list?.map((w) =>
          w.id === evt.workspace_id
            ? {
                ...w,
                detected_worktree: evt.detected_worktree,
                detected_branch: evt.detected_branch,
              }
            : w,
        ) ?? list,
      );
    });
  });
  onCleanup(() => {
    unlistenStatus?.();
    unlistenWorktree?.();
    unlistenSettings?.();
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
    _section: "active" | "passive",
    orderedIds: string[],
  ) => {
    // Recompute sort_order with a gap of 10 between rows so a future
    // single-row insert can land between two neighbours without
    // triggering a full resequence. Wire shape is named-struct to
    // match Rust `ReorderEntry` (see lib/workspaces.ts).
    const updates = orderedIds.map((id, idx) => ({
      workspace_id: id,
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

  const onDelete = async (id: string) => {
    try {
      await deleteWorkspace(id, true);
      mutate((list) => list?.filter((w) => w.id !== id) ?? list);
      if (selectedId() === id) setSelectedId(null);
    } catch (e) {
      void message(`Delete failed: ${String(e)}`, { kind: "error", title: "Tessera" });
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
          <nav class="topnav">
            <span class="topnav-item">Workspaces.</span>
            <span class="topnav-item">Sessions.</span>
            <span class="topnav-item">Hooks.</span>
          </nav>
          <div class="topmeta">
            <button
              type="button"
              class="topmeta-version"
              onClick={() => void runUpdateCheck()}
              title="Click to check for updates"
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
            <button
              type="button"
              class="topmeta-mode"
              onClick={toggleTheme}
              title={theme() === "dark" ? "Switch to light" : "Switch to dark"}
              aria-label="Toggle theme"
            >
              {theme() === "dark" ? "☾" : "☀"}
            </button>
          </div>
        </header>

        <div class="layout">
          <Sidebar
            workspaces={workspaces() ?? []}
            projects={projects() ?? []}
            selectedId={selectedId()}
            onSelect={onSelect}
            onDelete={onDelete}
            onNew={() => setShowNew(true)}
            onReorder={onReorder}
            onAssignProject={onAssignProject}
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

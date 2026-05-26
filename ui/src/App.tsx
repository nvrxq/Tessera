import { createResource, createSignal, onCleanup, onMount, Show, type Component } from "solid-js";
import Sidebar from "./Sidebar";
import NewWorkspaceForm from "./NewWorkspaceForm";
import Terminal from "./Terminal";
import {
  deleteWorkspace,
  listWorkspaces,
  onWorkspaceStatus,
  onWorkspaceWorktree,
  spawnAgent,
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
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [showNew, setShowNew] = createSignal(false);
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
  onMount(async () => {
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
  });

  const selected = () => workspaces()?.find((w) => w.id === selectedId()) ?? null;

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

  const onDelete = async (id: string) => {
    try {
      await deleteWorkspace(id, true);
      mutate((list) => list?.filter((w) => w.id !== id) ?? list);
      if (selectedId() === id) setSelectedId(null);
    } catch (e) {
      alert(`Delete failed: ${String(e)}`);
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
            <span class="topmeta-clock">{clock()}</span>
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
            selectedId={selectedId()}
            onSelect={onSelect}
            onDelete={onDelete}
            onNew={() => setShowNew(true)}
          />
          <main class="main-pane">
            <Show when={showNew()}>
              <NewWorkspaceForm onCreated={onCreated} onCancel={() => setShowNew(false)} />
            </Show>
            <Show when={!showNew() && selected()?.id}>
              <Terminal
                workspaceId={selected()!.id}
                sessionId={selected()?.session_id ?? null}
                onSpawned={(sid) => onTerminalSpawned(selected()!.id, sid)}
              />
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
    </div>
  );
};

export default App;

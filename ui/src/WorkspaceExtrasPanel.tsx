import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type Component,
} from "solid-js";
import {
  formatMmSs,
  pomodoroRemainingSeconds,
  workspaceLinksAdd,
  workspaceLinksDelete,
  workspaceLinksList,
  workspacePomodoroGet,
  workspacePomodoroPause,
  workspacePomodoroReset,
  workspacePomodoroResume,
  workspacePomodoroStart,
  workspaceTasksAdd,
  workspaceTasksDelete,
  workspaceTasksList,
  workspaceTasksToggle,
  type PomodoroState,
  type WorkspaceLink,
  type WorkspaceTask,
} from "./lib/extras";

export interface WorkspaceExtrasPanelProps {
  workspaceId: string;
  onClose: () => void;
}

type Tab = "links" | "tasks" | "pomodoro";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "links", label: "Links" },
  { id: "tasks", label: "Tasks" },
  { id: "pomodoro", label: "Pomodoro" },
];

const WorkspaceExtrasPanel: Component<WorkspaceExtrasPanelProps> = (props) => {
  const [tab, setTab] = createSignal<Tab>("links");

  return (
    <aside class="extras-panel" role="complementary" aria-label="Workspace extras">
      <header class="extras-panel-head">
        <nav class="extras-tabs" role="tablist">
          <For each={TABS}>
            {(t) => (
              <button
                type="button"
                class="extras-tab"
                classList={{ "extras-tab--active": tab() === t.id }}
                role="tab"
                aria-selected={tab() === t.id}
                onClick={() => setTab(t.id)}
              >
                {t.label}
              </button>
            )}
          </For>
        </nav>
        <button
          type="button"
          class="extras-panel-close"
          aria-label="Close extras panel"
          title="Close"
          onClick={props.onClose}
        >
          ×
        </button>
      </header>

      <div class="extras-panel-body">
        <Show when={tab() === "links"}>
          <LinksTab workspaceId={props.workspaceId} />
        </Show>
        <Show when={tab() === "tasks"}>
          <TasksTab workspaceId={props.workspaceId} />
        </Show>
        <Show when={tab() === "pomodoro"}>
          <PomodoroTab workspaceId={props.workspaceId} />
        </Show>
      </div>
    </aside>
  );
};

// ---- Links ----

const LinksTab: Component<{ workspaceId: string }> = (props) => {
  const [links, { mutate, refetch }] = createResource(
    () => props.workspaceId,
    (id) => workspaceLinksList(id),
  );
  const [url, setUrl] = createSignal("");
  const [label, setLabel] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const onSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    const u = url().trim();
    if (!u || busy()) return;
    setBusy(true);
    setError(null);
    try {
      const created = await workspaceLinksAdd(
        props.workspaceId,
        u,
        label().trim() || null,
      );
      mutate((list) => [...(list ?? []), created]);
      setUrl("");
      setLabel("");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (id: string) => {
    const before = links() ?? [];
    mutate(before.filter((l) => l.id !== id));
    try {
      await workspaceLinksDelete(id);
    } catch (err) {
      console.error("workspace_links_delete failed", err);
      refetch();
    }
  };

  return (
    <div class="extras-tab-body">
      <form class="extras-link-form" onSubmit={onSubmit}>
        <input
          type="url"
          class="extras-input"
          placeholder="https://…"
          value={url()}
          onInput={(e) => setUrl(e.currentTarget.value)}
          disabled={busy()}
          required
        />
        <input
          type="text"
          class="extras-input"
          placeholder="Label (optional)"
          value={label()}
          onInput={(e) => setLabel(e.currentTarget.value)}
          disabled={busy()}
          maxLength={80}
        />
        <button
          type="submit"
          class="extras-btn extras-btn--primary"
          disabled={busy() || !url().trim()}
        >
          {busy() ? "Adding…" : "Add link"}
        </button>
        <Show when={error()}>
          <div class="extras-error">{error()}</div>
        </Show>
      </form>

      <Show
        when={(links() ?? []).length > 0}
        fallback={<EmptyState message="No links yet. Drop a URL above." />}
      >
        <ul class="extras-link-list">
          <For each={links() ?? []}>
            {(l) => <LinkRow link={l} onDelete={onDelete} />}
          </For>
        </ul>
      </Show>
    </div>
  );
};

const LinkRow: Component<{ link: WorkspaceLink; onDelete: (id: string) => void }> = (
  p,
) => {
  const display = () =>
    p.link.label && p.link.label.length > 0 ? p.link.label : p.link.url;
  const isGithub = () =>
    p.link.kind === "github_issue" || p.link.kind === "github_pr";

  const openLink = (e: MouseEvent) => {
    e.preventDefault();
    // Tauri WebView opens external links via the OS default browser if we
    // simply use window.open; we go through the standard anchor click but
    // also expose a button to avoid the Tauri navigation guards.
    window.open(p.link.url, "_blank");
  };

  return (
    <li class="extras-link-row">
      <span class="extras-link-icon" aria-hidden="true">
        <Show when={isGithub()} fallback={<UrlIcon />}>
          <GithubIcon />
        </Show>
      </span>
      <a
        href={p.link.url}
        class="extras-link-text"
        classList={{ "extras-link-text--mono": isGithub() }}
        onClick={openLink}
        title={p.link.url}
      >
        {display()}
      </a>
      <button
        type="button"
        class="extras-row-delete"
        aria-label="Delete link"
        title="Delete"
        onClick={() => p.onDelete(p.link.id)}
      >
        ×
      </button>
    </li>
  );
};

// ---- Tasks ----

const TasksTab: Component<{ workspaceId: string }> = (props) => {
  const [tasks, { mutate, refetch }] = createResource(
    () => props.workspaceId,
    (id) => workspaceTasksList(id, true),
  );
  const [title, setTitle] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [showCompleted, setShowCompleted] = createSignal(false);

  const onSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    const t = title().trim();
    if (!t || busy()) return;
    setBusy(true);
    try {
      const created = await workspaceTasksAdd(props.workspaceId, t, null);
      mutate((list) => [...(list ?? []), created]);
      setTitle("");
    } catch (err) {
      console.error("workspace_tasks_add failed", err);
    } finally {
      setBusy(false);
    }
  };

  const onToggle = async (id: string) => {
    // Optimistic flip; on failure refetch authoritative state.
    const before = tasks() ?? [];
    mutate(
      before.map((t) =>
        t.id === id
          ? {
              ...t,
              done: !t.done,
              completed_at: !t.done ? new Date().toISOString() : null,
            }
          : t,
      ),
    );
    try {
      await workspaceTasksToggle(id);
    } catch (err) {
      console.error("workspace_tasks_toggle failed", err);
      refetch();
    }
  };

  const onDelete = async (id: string) => {
    mutate((list) => (list ?? []).filter((t) => t.id !== id));
    try {
      await workspaceTasksDelete(id);
    } catch (err) {
      console.error("workspace_tasks_delete failed", err);
      refetch();
    }
  };

  const active = () => (tasks() ?? []).filter((t) => !t.done);
  const completed = () => (tasks() ?? []).filter((t) => t.done);

  return (
    <div class="extras-tab-body">
      <form class="extras-task-form" onSubmit={onSubmit}>
        <input
          type="text"
          class="extras-input"
          placeholder="Add a task — Enter to save"
          value={title()}
          onInput={(e) => setTitle(e.currentTarget.value)}
          disabled={busy()}
          maxLength={140}
        />
      </form>

      <Show
        when={active().length > 0}
        fallback={<EmptyState message="No tasks. Inbox zero, for now." />}
      >
        <ul class="extras-task-list">
          <For each={active()}>
            {(t) => <TaskRow task={t} onToggle={onToggle} onDelete={onDelete} />}
          </For>
        </ul>
      </Show>

      <Show when={completed().length > 0}>
        <button
          type="button"
          class="extras-task-toggle-completed"
          onClick={() => setShowCompleted((v) => !v)}
        >
          {showCompleted() ? "Hide" : "Show"} completed ({completed().length})
        </button>
        <Show when={showCompleted()}>
          <ul class="extras-task-list extras-task-list--done">
            <For each={completed()}>
              {(t) => <TaskRow task={t} onToggle={onToggle} onDelete={onDelete} />}
            </For>
          </ul>
        </Show>
      </Show>
    </div>
  );
};

const TaskRow: Component<{
  task: WorkspaceTask;
  onToggle: (id: string) => void;
  onDelete: (id: string) => void;
}> = (p) => {
  return (
    <li
      class="extras-task-row"
      classList={{ "extras-task-row--done": p.task.done }}
    >
      <input
        type="checkbox"
        class="extras-task-checkbox"
        checked={p.task.done}
        onChange={() => p.onToggle(p.task.id)}
        aria-label={p.task.done ? "Mark as not done" : "Mark as done"}
      />
      <span class="extras-task-title">{p.task.title}</span>
      <Show when={p.task.due_date}>
        <span class="extras-task-due" title="Due date">
          {p.task.due_date}
        </span>
      </Show>
      <button
        type="button"
        class="extras-row-delete"
        aria-label="Delete task"
        title="Delete"
        onClick={() => p.onDelete(p.task.id)}
      >
        ×
      </button>
    </li>
  );
};

// ---- Pomodoro ----

const PomodoroTab: Component<{ workspaceId: string }> = (props) => {
  const [state, setState] = createSignal<PomodoroState | null>(null);
  // Re-rendered every second so the countdown ticks locally without
  // burning a backend round-trip per second.
  const [now, setNow] = createSignal(Date.now());
  let intervalId: number | null = null;

  const refresh = async () => {
    try {
      setState(await workspacePomodoroGet(props.workspaceId));
    } catch (err) {
      console.error("workspace_pomodoro_get failed", err);
    }
  };

  onMount(() => {
    void refresh();
    intervalId = window.setInterval(() => setNow(Date.now()), 1000);
  });
  onCleanup(() => {
    if (intervalId != null) window.clearInterval(intervalId);
  });

  // Refetch when the workspace id changes (panel mounts once per
  // workspace, but be defensive — Solid will re-run this effect).
  createEffect(() => {
    void props.workspaceId;
    void refresh();
  });

  const remaining = () => {
    const s = state();
    if (!s) return 0;
    return pomodoroRemainingSeconds(s, now());
  };

  const modeLabel = () => {
    const s = state();
    if (!s) return "Idle";
    switch (s.mode) {
      case "work":
        return "Work";
      case "break":
        return "Break";
      case "paused":
        return "Paused";
      default:
        return "Idle";
    }
  };

  const primaryLabel = () => {
    const s = state();
    if (!s) return "Start";
    if (s.mode === "work" || s.mode === "break") return "Pause";
    if (s.mode === "paused") return "Resume";
    return "Start";
  };

  const onPrimary = async () => {
    const s = state();
    if (!s) return;
    try {
      let next: PomodoroState;
      if (s.mode === "idle") {
        next = await workspacePomodoroStart(props.workspaceId, "work", null);
      } else if (s.mode === "paused") {
        next = await workspacePomodoroResume(props.workspaceId);
      } else {
        next = await workspacePomodoroPause(props.workspaceId);
      }
      setState(next);
    } catch (err) {
      console.error("pomodoro action failed", err);
      void refresh();
    }
  };

  const onReset = async () => {
    try {
      setState(await workspacePomodoroReset(props.workspaceId));
    } catch (err) {
      console.error("workspace_pomodoro_reset failed", err);
      void refresh();
    }
  };

  const onStartBreak = async () => {
    try {
      setState(await workspacePomodoroStart(props.workspaceId, "break", null));
    } catch (err) {
      console.error("workspace_pomodoro_start break failed", err);
    }
  };

  return (
    <div class="extras-tab-body extras-pomodoro">
      <Show
        when={state()}
        fallback={<EmptyState message="Loading…" />}
      >
        <div class="extras-pomodoro-mode">{modeLabel()}</div>
        <div
          class="extras-pomodoro-countdown"
          classList={{
            "extras-pomodoro-countdown--running":
              state()?.mode === "work" || state()?.mode === "break",
          }}
        >
          {formatMmSs(remaining())}
        </div>
        <div class="extras-pomodoro-cycles">
          {state()!.cycles_completed} cycle
          {state()!.cycles_completed === 1 ? "" : "s"} today
        </div>
        <div class="extras-pomodoro-actions">
          <button
            type="button"
            class="extras-btn extras-btn--primary extras-pomodoro-primary"
            onClick={onPrimary}
          >
            {primaryLabel()}
          </button>
          <Show when={state()?.mode === "idle"}>
            <button
              type="button"
              class="extras-btn"
              onClick={onStartBreak}
            >
              Start break
            </button>
          </Show>
          <button type="button" class="extras-btn" onClick={onReset}>
            Reset
          </button>
        </div>
      </Show>
    </div>
  );
};

// ---- shared bits ----

const EmptyState: Component<{ message: string }> = (p) => (
  <div class="extras-empty">
    <div class="extras-empty-mosaic" aria-hidden="true">
      <span /><span /><span /><span />
      <span /><span /><span /><span />
    </div>
    <div class="extras-empty-message">{p.message}</div>
  </div>
);

const GithubIcon: Component = () => (
  <svg viewBox="0 0 16 16" width="14" height="14" fill="currentColor" aria-hidden="true">
    <path d="M8 0C3.58 0 0 3.58 0 8a8 8 0 0 0 5.47 7.59c.4.07.55-.17.55-.38v-1.34c-2.22.48-2.69-1.07-2.69-1.07-.36-.92-.89-1.17-.89-1.17-.73-.5.06-.49.06-.49.81.06 1.23.83 1.23.83.72 1.22 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.03 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
  </svg>
);

const UrlIcon: Component = () => (
  <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true">
    <path d="M6.5 9.5l3-3" />
    <path d="M7 4.5l1.5-1.5a3 3 0 1 1 4.2 4.2L11 9" stroke-linecap="round" />
    <path d="M9 11.5L7.5 13a3 3 0 1 1-4.2-4.2L5 7" stroke-linecap="round" />
  </svg>
);

export default WorkspaceExtrasPanel;

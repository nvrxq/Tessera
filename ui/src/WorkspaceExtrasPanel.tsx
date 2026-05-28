import {
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from "solid-js";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  workspaceActivityList,
  workspaceLinksAdd,
  workspaceLinksDelete,
  workspaceLinksList,
  workspaceTasksAdd,
  workspaceTasksDelete,
  workspaceTasksList,
  workspaceTasksToggle,
  type ActivityEntry,
  type ActivityKind,
  type WorkspaceLink,
  type WorkspaceTask,
} from "./lib/extras";

export interface WorkspaceExtrasPanelProps {
  workspaceId: string;
  onClose: () => void;
}

type Tab = "links" | "tasks" | "activity";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "links", label: "Links" },
  { id: "tasks", label: "Tasks" },
  { id: "activity", label: "Activity" },
];

// Per-workspace storage of the last-selected tab. Pre-global-pomodoro
// builds wrote "pomodoro" here; we now fall back to "links" so the panel
// doesn't open on a dead tab id.
const tabKey = (id: string) => `tessera.extrasTab:${id}`;
function readStoredTab(id: string): Tab {
  const v = localStorage.getItem(tabKey(id));
  if (v === "tasks" || v === "activity") return v;
  return "links";
}

const WorkspaceExtrasPanel: Component<WorkspaceExtrasPanelProps> = (props) => {
  const [tab, setTab] = createSignal<Tab>(readStoredTab(props.workspaceId));
  const selectTab = (next: Tab) => {
    setTab(next);
    localStorage.setItem(tabKey(props.workspaceId), next);
  };

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
                onClick={() => selectTab(t.id)}
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
        <Show when={tab() === "activity"}>
          <ActivityTab workspaceId={props.workspaceId} />
        </Show>
      </div>
    </aside>
  );
};

// ---- Activity ----

const ACTIVITY_KIND_LABEL: Record<ActivityKind, string> = {
  post_tool_use: "tool",
  stop: "stop",
  notification: "notify",
};

function formatActivityTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

const ActivityTab: Component<{ workspaceId: string }> = (props) => {
  // Read-only view. We re-fetch when the workspace switches; live updates
  // can land in a follow-up once we wire a `workspace_activity` event.
  const [entries] = createResource(
    () => props.workspaceId,
    (id) => workspaceActivityList(id, 50),
  );

  return (
    <div class="extras-tab-body">
      <Show
        when={(entries() ?? []).length > 0}
        fallback={<EmptyState message="No activity yet. Hook events will appear here." />}
      >
        <ul class="extras-activity-list">
          <For each={entries() ?? []}>
            {(e: ActivityEntry) => (
              <li class="extras-activity-row">
                <span class="extras-activity-time">{formatActivityTime(e.created_at)}</span>
                <span class={`extras-activity-kind extras-activity-kind--${e.kind}`}>
                  {ACTIVITY_KIND_LABEL[e.kind] ?? e.kind}
                </span>
                <span class="extras-activity-summary" title={e.payload}>{e.summary}</span>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
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
          class="extras-btn extras-btn--primary extras-btn--with-icon"
          disabled={busy() || !url().trim()}
          aria-label={busy() ? "Adding link" : "Add link"}
        >
          <PlusIcon />
          <span>{busy() ? "Adding…" : "Add link"}</span>
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

  const openLink = async (_e: MouseEvent) => {
    // In Tauri 2 WebView, `window.open(url, "_blank")` silently no-ops
    // unless the opener plugin is installed. We hand the URL to the
    // plugin so it hits the OS default browser. We deliberately do NOT
    // `preventDefault()` — if the invoke throws (plugin missing, perm
    // denied), the anchor's native `href` still fires as a fallback.
    try {
      await openUrl(p.link.url);
    } catch (err) {
      console.error("openUrl failed", err);
    }
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
    // Optimistic flip; on failure refetch authoritative state. The
    // backend returns the post-toggle `done` bool — trust it over the
    // local flip in case the row was deleted/toggled between list and
    // toggle (e.g. another window also has the panel open).
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
      const serverDone = await workspaceTasksToggle(id);
      mutate((list) =>
        (list ?? []).map((t) =>
          t.id === id
            ? {
                ...t,
                done: serverDone,
                completed_at: serverDone ? new Date().toISOString() : null,
              }
            : t,
        ),
      );
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
        {/* Iconified submit so the affordance is visible — previously the
          * form had no button at all, which left the action undiscoverable
          * (you had to know to press Enter). The plus icon matches the
          * link-add button's stroke weight. */}
        <button
          type="submit"
          class="extras-task-add"
          disabled={busy() || !title().trim()}
          aria-label="Add task"
          title="Add task"
        >
          <PlusIcon />
        </button>
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

// 12px plus glyph matching the stroke weight of the close (×) icon —
// used on the "Add link" / "Add task" buttons so the action affordance is
// visible at a glance.
const PlusIcon: Component = () => (
  <svg
    viewBox="0 0 12 12"
    width="12"
    height="12"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    aria-hidden="true"
  >
    <path d="M6 2v8" />
    <path d="M2 6h8" />
  </svg>
);

export default WorkspaceExtrasPanel;

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

const App: Component = () => {
  const [workspaces, { mutate, refetch }] = createResource<WorkspaceDto[]>(listWorkspaces);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [showNew, setShowNew] = createSignal(false);

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

  const onSelect = async (id: string) => {
    setSelectedId(id);
    const ws = workspaces()?.find((w) => w.id === id);
    if (ws && ws.session_id == null) {
      const sid = await spawnAgent(id);
      mutate((list) =>
        list?.map((w) => (w.id === id ? { ...w, session_id: sid } : w)) ?? list,
      );
    }
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
    <>
      <header>
        <h1>Tessera</h1>
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
          <Show when={!showNew() && selected()?.session_id} keyed>
            {(sid) => <Terminal sessionId={sid} />}
          </Show>
          <Show when={!showNew() && !selected()}>
            <div class="empty-state">Select a workspace or create a new one.</div>
          </Show>
        </main>
      </div>
    </>
  );
};

export default App;

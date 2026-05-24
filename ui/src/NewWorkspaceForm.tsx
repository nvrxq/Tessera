import { createSignal } from "solid-js";
import type { Component } from "solid-js";
import { createWorkspace, type WorkspaceDto } from "./lib/workspaces";

export interface NewWorkspaceFormProps {
  onCreated: (ws: WorkspaceDto) => void;
  onCancel: () => void;
}

const NewWorkspaceForm: Component<NewWorkspaceFormProps> = (props) => {
  const [name, setName] = createSignal("");
  const [folderPath, setFolderPath] = createSignal("");
  const [task, setTask] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!name().trim() || !folderPath().trim() || !task().trim()) return;
    setBusy(true);
    setError(null);
    try {
      const ws = await createWorkspace(folderPath().trim(), name().trim(), task().trim());
      props.onCreated(ws);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form class="new-workspace" onSubmit={submit}>
      <label>
        <span>Name *</span>
        <input
          type="text"
          placeholder="Login refactor"
          value={name()}
          onInput={(e) => setName(e.currentTarget.value)}
          required
          autofocus
        />
      </label>
      <label>
        <span>Folder path *</span>
        <input
          type="text"
          placeholder="/home/save/Work/Some/Repo"
          value={folderPath()}
          onInput={(e) => setFolderPath(e.currentTarget.value)}
          required
        />
      </label>
      <label>
        <span>Task *</span>
        <textarea
          rows="6"
          placeholder="Describe what Claude should do. Claude will be launched in the folder above and this prompt will be sent as its first message."
          value={task()}
          onInput={(e) => setTask(e.currentTarget.value)}
          required
        />
      </label>
      {error() && <p class="form-error">{error()}</p>}
      <div class="form-actions">
        <button type="button" onClick={props.onCancel} disabled={busy()}>
          Cancel
        </button>
        <button type="submit" disabled={busy()}>
          {busy() ? "Creating…" : "Create"}
        </button>
      </div>
    </form>
  );
};

export default NewWorkspaceForm;

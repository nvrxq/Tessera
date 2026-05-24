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
  const [branch, setBranch] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!name().trim() || !folderPath().trim()) return;
    setBusy(true);
    setError(null);
    try {
      const ws = await createWorkspace(folderPath().trim(), name().trim(), branch().trim() || null);
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
          placeholder="/home/save/Work/Some/Folder"
          value={folderPath()}
          onInput={(e) => setFolderPath(e.currentTarget.value)}
          required
        />
      </label>
      <label>
        <span>Branch name <em>(optional — leave empty to use folder as-is)</em></span>
        <input
          type="text"
          placeholder="feat/login"
          value={branch()}
          onInput={(e) => setBranch(e.currentTarget.value)}
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

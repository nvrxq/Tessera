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
  const [dangerous, setDangerous] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!name().trim() || !folderPath().trim()) return;
    setBusy(true);
    setError(null);
    try {
      const ws = await createWorkspace(folderPath().trim(), name().trim(), dangerous());
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
      <label class="checkbox-row">
        <input
          type="checkbox"
          checked={dangerous()}
          onChange={(e) => setDangerous(e.currentTarget.checked)}
        />
        <div class="checkbox-meta">
          <span class="checkbox-title">
            Run with <code>--dangerously-skip-permissions</code>
          </span>
          <span class="checkbox-hint">
            Claude won't prompt for tool permissions. Use only in trusted folders.
          </span>
        </div>
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

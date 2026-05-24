import { createSignal } from "solid-js";
import type { Component } from "solid-js";
import { createWorkspace, type WorkspaceDto } from "./lib/workspaces";

export interface NewWorkspaceFormProps {
  onCreated: (ws: WorkspaceDto) => void;
  onCancel: () => void;
}

const NewWorkspaceForm: Component<NewWorkspaceFormProps> = (props) => {
  const [repoPath, setRepoPath] = createSignal("");
  const [branch, setBranch] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!repoPath() || !branch()) return;
    setBusy(true);
    setError(null);
    try {
      const ws = await createWorkspace(repoPath(), branch());
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
        <span>Repo path</span>
        <input
          type="text"
          placeholder="/home/save/Work/Some/Repo"
          value={repoPath()}
          onInput={(e) => setRepoPath(e.currentTarget.value)}
          required
        />
      </label>
      <label>
        <span>Branch name</span>
        <input
          type="text"
          placeholder="feat/new-thing"
          value={branch()}
          onInput={(e) => setBranch(e.currentTarget.value)}
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

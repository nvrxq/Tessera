import { createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { createWorkspace, listDirectories, type WorkspaceDto } from "./lib/workspaces";

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

  // —— autocomplete state ——
  const [suggestions, setSuggestions] = createSignal<string[]>([]);
  const [showSuggest, setShowSuggest] = createSignal(false);
  const [activeIdx, setActiveIdx] = createSignal(-1);
  let debounceId: number | null = null;

  const fetchSuggestions = (value: string) => {
    if (debounceId != null) window.clearTimeout(debounceId);
    debounceId = window.setTimeout(async () => {
      const v = value.trim();
      if (!v || (!v.startsWith("/") && !v.startsWith("~"))) {
        setSuggestions([]);
        return;
      }
      try {
        const list = await listDirectories(v);
        setSuggestions(list);
        setActiveIdx(-1);
      } catch {
        setSuggestions([]);
      }
    }, 80);
  };

  const handleFolderInput = (e: InputEvent & { currentTarget: HTMLInputElement }) => {
    const v = e.currentTarget.value;
    setFolderPath(v);
    setShowSuggest(true);
    fetchSuggestions(v);
  };

  const pickSuggestion = (path: string) => {
    setFolderPath(path);
    setSuggestions([]);
    setShowSuggest(false);
    setActiveIdx(-1);
  };

  const handleFolderKey = (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => {
    const list = suggestions();
    if (!showSuggest() || list.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((i) => (i + 1) % list.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((i) => (i <= 0 ? list.length - 1 : i - 1));
    } else if (e.key === "Tab") {
      const idx = activeIdx() < 0 ? 0 : activeIdx();
      if (list[idx]) {
        e.preventDefault();
        pickSuggestion(list[idx] + "/");
        fetchSuggestions(list[idx] + "/");
        setShowSuggest(true);
      }
    } else if (e.key === "Enter" && activeIdx() >= 0) {
      e.preventDefault();
      const idx = activeIdx();
      if (list[idx]) pickSuggestion(list[idx]);
    } else if (e.key === "Escape") {
      setShowSuggest(false);
    }
  };

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
      <h3>New workspace.</h3>
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
      <label class="folder-field">
        <span>Folder path *</span>
        <div class="folder-input-wrap">
          <input
            type="text"
            placeholder="/home/save/Work or ~/Work"
            value={folderPath()}
            onInput={handleFolderInput}
            onKeyDown={handleFolderKey}
            onFocus={() => {
              setShowSuggest(true);
              if (folderPath().trim()) fetchSuggestions(folderPath());
            }}
            onBlur={() => {
              // delay so a click on a suggestion can fire first
              window.setTimeout(() => setShowSuggest(false), 120);
            }}
            autocomplete="off"
            spellcheck={false}
            required
          />
          <Show when={showSuggest() && suggestions().length > 0}>
            <ul class="folder-suggest">
              <For each={suggestions()}>
                {(p, i) => (
                  <li
                    classList={{ active: i() === activeIdx() }}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      pickSuggestion(p);
                    }}
                    onMouseEnter={() => setActiveIdx(i())}
                  >
                    {p}
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </div>
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

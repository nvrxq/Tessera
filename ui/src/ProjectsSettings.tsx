import { createSignal, For, onCleanup, onMount, Show, type Component } from "solid-js";
import {
  PROJECT_SWATCHES,
  type Project,
} from "./lib/workspaces";

export interface ProjectsSettingsProps {
  projects: Project[];
  onClose: () => void;
  /** Create a project. The parent is expected to call the backend
   *  `createProject` and refresh the list. */
  onCreate: (name: string, accent: string | null) => Promise<void>;
  /** Delete a project. Backend FK is `ON DELETE SET NULL`, so workspaces
   *  with that project_id will silently lose their assignment — no
   *  cascade-confirmation needed here. */
  onDelete: (id: string) => Promise<void>;
}

/**
 * Projects settings — modal that lists projects with a delete button and
 * a small inline create form (name + 6-swatch accent picker).
 *
 * Closes on background click, on Escape, or via the explicit ×. The
 * accent palette mirrors `PROJECT_SWATCHES` from `lib/workspaces.ts` so
 * the modal and the inline picker in `NewWorkspaceForm` stay aligned.
 */
const ProjectsSettings: Component<ProjectsSettingsProps> = (props) => {
  const [name, setName] = createSignal("");
  const [accent, setAccent] = createSignal<string | null>(null);
  const [creating, setCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const onSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    const trimmed = name().trim();
    if (!trimmed) return;
    setCreating(true);
    setError(null);
    try {
      await props.onCreate(trimmed, accent());
      setName("");
      setAccent(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setCreating(false);
    }
  };

  // Escape MUST work no matter what's focused inside the modal (swatch
  // button, name input, delete button). A keydown handler on the overlay
  // only fires while focus is on the overlay itself; once the user
  // tab-targets an inner button, the inner button or its ancestors could
  // stop propagation and the modal would refuse to close on Esc. Bind at
  // document level for the modal's lifetime, then clean up on unmount.
  const onDocKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      props.onClose();
    }
  };
  onMount(() => document.addEventListener("keydown", onDocKey, true));
  onCleanup(() => document.removeEventListener("keydown", onDocKey, true));

  return (
    <div
      class="projects-modal-overlay"
      onClick={props.onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Projects"
      tabIndex={-1}
      ref={(el) => queueMicrotask(() => el.focus())}
    >
      <div
        class="projects-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <header class="projects-modal-head">
          <h2 class="projects-modal-title">Projects</h2>
          <button
            type="button"
            class="projects-modal-close"
            onClick={props.onClose}
            aria-label="Close"
            title="Close"
          >
            ×
          </button>
        </header>

        <Show
          when={props.projects.length > 0}
          fallback={
            <div class="projects-modal-empty">
              No projects yet. Create one below to start grouping workspaces.
            </div>
          }
        >
          <ul class="projects-modal-list">
            <For each={props.projects}>
              {(p) => (
                <li class="projects-modal-row">
                  <span
                    class="projects-modal-swatch"
                    style={p.accent ? { "background-color": p.accent } : undefined}
                  />
                  <span class="projects-modal-name">{p.name}</span>
                  <button
                    type="button"
                    class="projects-modal-delete"
                    title="Delete project"
                    aria-label={`Delete ${p.name}`}
                    onClick={() => {
                      if (
                        confirm(
                          `Delete project "${p.name}"? Workspaces assigned to it will become un-assigned (they're not deleted).`,
                        )
                      ) {
                        void props.onDelete(p.id);
                      }
                    }}
                  >
                    ×
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <form class="projects-modal-form" onSubmit={onSubmit}>
          <div class="projects-modal-form-label">New project</div>
          <input
            type="text"
            class="projects-modal-input"
            placeholder="Project name"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
            disabled={creating()}
            maxLength={48}
          />
          <div class="projects-modal-swatches" role="group" aria-label="Accent">
            <For each={PROJECT_SWATCHES}>
              {(s) => (
                <button
                  type="button"
                  class="projects-modal-swatch-pick"
                  classList={{
                    "projects-modal-swatch-pick--selected": accent() === s.value,
                  }}
                  style={
                    s.value
                      ? { "background-color": s.value }
                      : undefined
                  }
                  title={s.label}
                  aria-label={s.label}
                  aria-pressed={accent() === s.value}
                  onClick={() => setAccent(s.value)}
                />
              )}
            </For>
          </div>
          <Show when={error()}>
            <div class="projects-modal-error">{error()}</div>
          </Show>
          <div class="projects-modal-form-actions">
            <button
              type="submit"
              class="projects-modal-create"
              disabled={creating() || !name().trim()}
            >
              {creating() ? "Creating…" : "Create"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

export default ProjectsSettings;

import { For, Show } from "solid-js";
import type { Component } from "solid-js";
import type { WorkspaceDto } from "./lib/workspaces";

export interface SidebarProps {
  workspaces: WorkspaceDto[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
  onNew: () => void;
}

const Sidebar: Component<SidebarProps> = (props) => {
  return (
    <aside class="sidebar">
      <div class="sidebar-head">
        <span>Workspaces</span>
        <button type="button" onClick={props.onNew} title="New workspace">+</button>
      </div>
      <ul class="workspace-list">
        <Show
          when={props.workspaces.length > 0}
          fallback={<li class="workspace-empty">No workspaces yet</li>}
        >
          <For each={props.workspaces}>
            {(ws) => (
              <li
                class="workspace-item"
                classList={{ selected: ws.id === props.selectedId }}
                onClick={() => props.onSelect(ws.id)}
              >
                <div class="workspace-meta">
                  <div class="workspace-name">{ws.name}</div>
                  <div class="workspace-branch">{ws.branch}</div>
                </div>
                <button
                  type="button"
                  class="workspace-delete"
                  title="Delete workspace"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (confirm(`Delete workspace "${ws.name}"? Worktree will be removed.`)) {
                      props.onDelete(ws.id);
                    }
                  }}
                >
                  ×
                </button>
              </li>
            )}
          </For>
        </Show>
      </ul>
    </aside>
  );
};

export default Sidebar;

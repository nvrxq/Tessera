import { For, Show } from "solid-js";
import type { Component } from "solid-js";
import { statusLabel, type AgentStatus, type WorkspaceDto } from "./lib/workspaces";

export interface SidebarProps {
  workspaces: WorkspaceDto[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
  onNew: () => void;
}

function statusClass(s: AgentStatus | null): string {
  if (!s) return "status-dot status-none";
  return `status-dot status-${s}`;
}

function statusLabelClass(s: AgentStatus | null): string {
  if (!s) return "status-label status-label-none";
  return `status-label status-label-${s}`;
}

const CLAUDE_PETALS = [0, 30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330];

const ClaudeMark: Component = () => (
  <span class="claude-mark" aria-hidden="true" title="Claude Code">
    <svg viewBox="0 0 24 24" width="13" height="13" fill="none">
      <g
        transform="translate(12 12)"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
      >
        <For each={CLAUDE_PETALS}>
          {(deg) => <line y1="-9" y2="-4" transform={`rotate(${deg})`} />}
        </For>
      </g>
    </svg>
  </span>
);

function subline(ws: WorkspaceDto): string {
  if (ws.detected_worktree) {
    const branch = ws.detected_branch ? ` · ${ws.detected_branch}` : "";
    return `→ ${ws.detected_worktree}${branch}`;
  }
  return ws.repo_path;
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
                <span class={statusClass(ws.agent_status)} title={statusLabel(ws.agent_status)} />
                <div class="workspace-meta">
                  <div class="workspace-name">
                    <ClaudeMark />
                    <span class="workspace-name-text">{ws.name}</span>
                    <Show when={ws.dangerous_skip_permissions}>
                      <span class="dangerous-badge" title="--dangerously-skip-permissions">⚡</span>
                    </Show>
                  </div>
                  <div class="workspace-substack">
                    <span class={statusLabelClass(ws.agent_status)}>
                      {statusLabel(ws.agent_status)}
                    </span>
                    <span class="workspace-branch">{subline(ws)}</span>
                  </div>
                </div>
                <button
                  type="button"
                  class="workspace-delete"
                  title="Delete workspace"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (confirm(`Delete workspace "${ws.name}"? The DB row is removed; on-disk files are left alone.`)) {
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
      <button type="button" class="sidebar-newbutton" onClick={props.onNew} title="New workspace">
        <span class="sidebar-newbutton-circle">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none">
            <path
              d="M5 12h14M13 6l6 6-6 6"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </span>
        <span class="sidebar-newbutton-label">New workspace</span>
      </button>
    </aside>
  );
};

export default Sidebar;

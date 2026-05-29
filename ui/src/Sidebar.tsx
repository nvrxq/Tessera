import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { ask } from "@tauri-apps/plugin-dialog";
import PomodoroHud from "./PomodoroHud";
import {
  statusLabel,
  type AgentStatus,
  type Project,
  type WorkspaceDto,
} from "./lib/workspaces";

type SectionKey = "active" | "passive";

export interface SidebarProps {
  workspaces: WorkspaceDto[];
  /** Soft-archived workspaces, surfaced under a collapsible section at
   *  the bottom of the sidebar. App fetches this list — Sidebar stays
   *  presentational. */
  archived: WorkspaceDto[];
  projects: Project[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
  onNew: () => void;
  onReorder: (section: SectionKey, orderedIds: string[]) => void;
  onAssignProject: (workspaceId: string, projectId: string | null) => void;
  onRename: (workspaceId: string, newName: string) => void;
  /** Soft-archive: hide from active list, keep row + pinned Claude
   *  session. Triggered from the workspace menu. */
  onArchive: (workspaceId: string) => void;
  /** Bring an archived workspace back into the active list. */
  onUnarchive: (workspaceId: string) => void;
  /** Drop the pinned Claude session so the next spawn starts fresh. */
  onResetSession: (workspaceId: string) => void;
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

/** Differentiating subline: only worth showing when Claude has spawned
 *  a git worktree (then the line tells you the branch + path). Falling
 *  back to `repo_path` was redundant — most users keep all workspaces
 *  in the same folder and the path was identical for every row. */
function subline(ws: WorkspaceDto): string | null {
  if (ws.detected_worktree) {
    const branch = ws.detected_branch ? ` · ${ws.detected_branch}` : "";
    return `→ ${ws.detected_worktree}${branch}`;
  }
  return null;
}

function sectionOf(ws: WorkspaceDto): SectionKey {
  return ws.session_id != null ? "active" : "passive";
}

function compareWorkspaces(a: WorkspaceDto, b: WorkspaceDto): number {
  if (a.sort_order !== b.sort_order) return a.sort_order - b.sort_order;
  return a.created_at.localeCompare(b.created_at);
}

const Sidebar: Component<SidebarProps> = (props) => {
  // —— drag state ——
  // dragId: workspace id currently being dragged.
  // dragSection: which section the drag originated in (used to gate
  // cross-section drops — we ignore them silently per spec).
  // overId: workspace id currently being hovered over as a drop target.
  const [dragId, setDragId] = createSignal<string | null>(null);
  const [dragSection, setDragSection] = createSignal<SectionKey | null>(null);
  const [overId, setOverId] = createSignal<string | null>(null);

  // Single open popover at a time — `openMenuId` holds the workspace id
  // whose `⋯` menu is showing, or null. Outside-click handler closes it.
  const [openMenuId, setOpenMenuId] = createSignal<string | null>(null);
  // Inline rename mode — id of the workspace whose name cell is currently
  // an <input>. Only one at a time; Enter/blur commits, Esc cancels.
  const [renamingId, setRenamingId] = createSignal<string | null>(null);
  // Archive section is collapsed by default — keeps the active list as
  // the primary focus, but one click away when the user wants to
  // restore something. Persisted to localStorage so the choice
  // survives reloads.
  const archiveOpenKey = "tessera.sidebar.archive_open";
  const [archiveOpen, setArchiveOpen] = createSignal<boolean>(
    typeof localStorage !== "undefined" && localStorage.getItem(archiveOpenKey) === "1",
  );
  const toggleArchive = () => {
    const next = !archiveOpen();
    setArchiveOpen(next);
    try {
      localStorage.setItem(archiveOpenKey, next ? "1" : "0");
    } catch {
      /* localStorage can throw in privacy modes; ignore. */
    }
  };
  // Controlled draft for the rename input. Seeded once when rename opens so
  // that even if a status/worktree event recreates the <For> row mid-edit,
  // the freshly-mounted <input> re-reads the live draft rather than ws.name.
  const [renameDraft, setRenameDraft] = createSignal("");
  // Guards against the Enter→blur double-commit: Enter blurs the input,
  // which fires onBlur; this flag tells onBlur the commit already happened.
  let renameCommitted = false;
  const startRename = (ws: WorkspaceDto) => {
    renameCommitted = false;
    setRenameDraft(ws.name);
    setRenamingId(ws.id);
  };
  // Commit the current draft (trimmed) if it's non-empty and changed.
  // Idempotent via renameCommitted so Enter and the ensuing blur don't
  // both fire onRename. Always clears rename mode.
  const commitRename = (ws: WorkspaceDto) => {
    if (renameCommitted) return;
    renameCommitted = true;
    const next = renameDraft().trim();
    if (next && next !== ws.name) props.onRename(ws.id, next);
    setRenamingId(null);
  };
  const cancelRename = () => {
    renameCommitted = true;
    setRenamingId(null);
  };
  const onDocClick = (e: MouseEvent) => {
    // Any click outside `.workspace-menu-wrap` closes the popover.
    const target = e.target as HTMLElement | null;
    if (!target?.closest(".workspace-menu-wrap")) {
      setOpenMenuId(null);
    }
  };
  onMount(() => document.addEventListener("click", onDocClick, true));
  onCleanup(() => document.removeEventListener("click", onDocClick, true));

  const projectMap = createMemo(() => {
    const m = new Map<string, Project>();
    for (const p of props.projects) m.set(p.id, p);
    return m;
  });

  const sections = createMemo(() => {
    const active: WorkspaceDto[] = [];
    const passive: WorkspaceDto[] = [];
    for (const ws of props.workspaces) {
      (sectionOf(ws) === "active" ? active : passive).push(ws);
    }
    active.sort(compareWorkspaces);
    passive.sort(compareWorkspaces);
    return { active, passive };
  });

  const handleDragStart = (
    e: DragEvent,
    ws: WorkspaceDto,
  ) => {
    setDragId(ws.id);
    setDragSection(sectionOf(ws));
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      // Some browsers require setData for the drag to actually fire.
      try {
        e.dataTransfer.setData("text/plain", ws.id);
      } catch {
        /* setData can throw in restricted contexts; safe to ignore. */
      }
    }
  };

  const handleDragOver = (
    e: DragEvent,
    target: WorkspaceDto,
  ) => {
    const src = dragId();
    if (!src || src === target.id) return;
    if (dragSection() !== sectionOf(target)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    if (overId() !== target.id) setOverId(target.id);
  };

  const handleDragLeave = (target: WorkspaceDto) => {
    if (overId() === target.id) setOverId(null);
  };

  const handleDrop = (
    e: DragEvent,
    target: WorkspaceDto,
  ) => {
    const src = dragId();
    const section = dragSection();
    if (!src || !section) return;
    if (section !== sectionOf(target)) return;
    e.preventDefault();
    const list = section === "active" ? sections().active : sections().passive;
    const srcIdx = list.findIndex((w) => w.id === src);
    const dstIdx = list.findIndex((w) => w.id === target.id);
    if (srcIdx < 0 || dstIdx < 0 || srcIdx === dstIdx) {
      setDragId(null);
      setDragSection(null);
      setOverId(null);
      return;
    }
    const reordered = list.slice();
    const [moved] = reordered.splice(srcIdx, 1);
    reordered.splice(dstIdx, 0, moved);
    props.onReorder(section, reordered.map((w) => w.id));
    setDragId(null);
    setDragSection(null);
    setOverId(null);
  };

  const handleDragEnd = () => {
    setDragId(null);
    setDragSection(null);
    setOverId(null);
  };

  const renderRow = (ws: WorkspaceDto) => {
    const project = ws.project_id ? projectMap().get(ws.project_id) ?? null : null;
    return (
      <li
        class="workspace-item"
        classList={{
          selected: ws.id === props.selectedId,
          "workspace-item--dragging": ws.id === dragId(),
          "workspace-item--drop-target": ws.id === overId() && ws.id !== dragId(),
        }}
        title={
          project
            ? `${ws.name} · ${project.name}`
            : ws.name
        }
        draggable={true}
        onClick={() => props.onSelect(ws.id)}
        onDragStart={(e) => handleDragStart(e, ws)}
        onDragOver={(e) => handleDragOver(e, ws)}
        onDragLeave={() => handleDragLeave(ws)}
        onDrop={(e) => handleDrop(e, ws)}
        onDragEnd={handleDragEnd}
      >
        <span class={statusClass(ws.agent_status)} title={statusLabel(ws.agent_status)} />
        <div class="workspace-meta">
          <div class="workspace-name">
            <ClaudeMark />
            <Show when={project}>
              {(p) => (
                <span
                  class="workspace-project-dot"
                  title={`Project: ${p().name}`}
                  style={p().accent ? { "background-color": p().accent! } : undefined}
                />
              )}
            </Show>
            <Show
              when={renamingId() === ws.id}
              fallback={<span class="workspace-name-text" title={ws.name}>{ws.name}</span>}
            >
              <input
                type="text"
                class="workspace-name-input"
                // Controlled by the draft signal (not ws.name), so the live
                // text survives a row recreate triggered by an agent event.
                value={renameDraft()}
                autofocus
                draggable={false}
                onMouseDown={(e) => e.stopPropagation()}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => e.stopPropagation()}
                ref={(el) => {
                  // select-all on open so the user can type a fresh name
                  // immediately without hitting Cmd-A first.
                  queueMicrotask(() => {
                    el.focus();
                    el.select();
                  });
                }}
                onInput={(e) => setRenameDraft(e.currentTarget.value)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") {
                    e.preventDefault();
                    // Blur first so the commit happens once: the resulting
                    // onBlur sees renameCommitted and no-ops.
                    commitRename(ws);
                    e.currentTarget.blur();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    cancelRename();
                    e.currentTarget.blur();
                  }
                }}
                // Blur commits like Enter — clicking elsewhere should keep
                // the typed name, not silently discard it.
                onBlur={() => commitRename(ws)}
              />
            </Show>
            <Show when={ws.dangerous_skip_permissions}>
              <span class="dangerous-badge" title="--dangerously-skip-permissions">⚡</span>
            </Show>
          </div>
          <div class="workspace-substack">
            <span class={statusLabelClass(ws.agent_status)}>
              {statusLabel(ws.agent_status)}
            </span>
            <Show when={subline(ws)}>
              {(line) => <span class="workspace-branch">{line()}</span>}
            </Show>
          </div>
        </div>
        <div class="workspace-menu-wrap">
          <button
            type="button"
            class="workspace-menu-button"
            title="Workspace options"
            aria-haspopup="menu"
            // String literal, not bool — both Solid's attribute serialiser
            // edge cases AND our CSS selector (`[aria-expanded="true"]`)
            // need a stable string value to reliably style the open state.
            aria-expanded={openMenuId() === ws.id ? "true" : "false"}
            draggable={false}
            onMouseDown={(e) => e.stopPropagation()}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              setOpenMenuId(openMenuId() === ws.id ? null : ws.id);
            }}
          >
            ⋯
          </button>
          <Show when={openMenuId() === ws.id}>
            <div class="workspace-menu-popover" role="menu">
              <button
                type="button"
                class="workspace-menu-item"
                onClick={(e) => {
                  e.stopPropagation();
                  startRename(ws);
                  setOpenMenuId(null);
                }}
              >
                <span class="workspace-menu-item-label">Rename</span>
              </button>
              <button
                type="button"
                class="workspace-menu-item"
                title={
                  ws.claude_session_id
                    ? `Pinned to ${ws.claude_session_id.slice(0, 8)}…`
                    : "No Claude session pinned yet"
                }
                onClick={async (e) => {
                  e.stopPropagation();
                  setOpenMenuId(null);
                  const ok = await ask(
                    `Reset Claude session for "${ws.name}"?\n\nThe next launch will start a fresh conversation. The current jsonl on disk is left alone.`,
                    {
                      title: "Reset Claude session",
                      kind: "warning",
                      okLabel: "Reset",
                      cancelLabel: "Cancel",
                    },
                  );
                  if (ok) props.onResetSession(ws.id);
                }}
              >
                <span class="workspace-menu-item-label">Reset session</span>
              </button>
              <button
                type="button"
                class="workspace-menu-item"
                onClick={(e) => {
                  e.stopPropagation();
                  setOpenMenuId(null);
                  props.onArchive(ws.id);
                }}
              >
                <span class="workspace-menu-item-label">Archive</span>
              </button>
              <div class="workspace-menu-label">Project</div>
              <button
                type="button"
                class="workspace-menu-item"
                classList={{ "workspace-menu-item--current": ws.project_id == null }}
                onClick={(e) => {
                  e.stopPropagation();
                  props.onAssignProject(ws.id, null);
                  setOpenMenuId(null);
                }}
              >
                <span class="workspace-menu-swatch workspace-menu-swatch--none" />
                <span class="workspace-menu-item-label">None</span>
              </button>
              <For each={props.projects}>
                {(p) => (
                  <button
                    type="button"
                    class="workspace-menu-item"
                    classList={{
                      "workspace-menu-item--current": ws.project_id === p.id,
                    }}
                    onClick={(e) => {
                      e.stopPropagation();
                      props.onAssignProject(ws.id, p.id);
                      setOpenMenuId(null);
                    }}
                  >
                    <span
                      class="workspace-menu-swatch"
                      style={p.accent ? { "background-color": p.accent } : undefined}
                    />
                    <span class="workspace-menu-item-label">{p.name}</span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
        <button
          type="button"
          class="workspace-delete"
          title="Delete workspace"
          draggable={false}
          onMouseDown={(e) => e.stopPropagation()}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={async (e) => {
            e.stopPropagation();
            const ok = await ask(
              `Delete workspace "${ws.name}"?\n\nThe DB row is removed; on-disk files are left alone.`,
              { title: "Delete workspace", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" },
            );
            if (ok) props.onDelete(ws.id);
          }}
        >
          ×
        </button>
      </li>
    );
  };

  return (
    <aside class="sidebar">
      <div class="sidebar-head">
        <span>Workspaces</span>
        <button type="button" onClick={props.onNew} title="New workspace">+</button>
      </div>
      <div class="workspace-sections">
        <Show
          when={props.workspaces.length > 0}
          fallback={<div class="workspace-empty">No workspaces yet</div>}
        >
          <Show when={sections().active.length > 0}>
            <div class="workspace-section-header">Active</div>
            <ul class="workspace-list">
              <For each={sections().active}>{renderRow}</For>
            </ul>
          </Show>
          <Show when={sections().passive.length > 0}>
            <div class="workspace-section-header">Passive</div>
            <ul class="workspace-list">
              <For each={sections().passive}>{renderRow}</For>
            </ul>
          </Show>
        </Show>
        <Show when={props.archived.length > 0}>
          <button
            type="button"
            class="workspace-section-header workspace-section-header--toggle"
            aria-expanded={archiveOpen() ? "true" : "false"}
            onClick={toggleArchive}
            title={archiveOpen() ? "Collapse archive" : "Expand archive"}
          >
            <span>Archive ({props.archived.length})</span>
            <span class="workspace-section-caret" aria-hidden="true">
              {archiveOpen() ? "▾" : "▸"}
            </span>
          </button>
          <Show when={archiveOpen()}>
            <ul class="workspace-list workspace-list--archive">
              <For each={props.archived}>
                {(ws) => (
                  <li class="workspace-item workspace-item--archived" title={ws.name}>
                    <span class="status-dot status-none" aria-hidden="true" />
                    <div class="workspace-meta">
                      <div class="workspace-name">
                        <ClaudeMark />
                        <span class="workspace-name-text" title={ws.name}>
                          {ws.name}
                        </span>
                      </div>
                      <div class="workspace-substack">
                        <span class="status-label status-label-none">Archived</span>
                      </div>
                    </div>
                    <button
                      type="button"
                      class="workspace-archive-restore"
                      title="Restore workspace"
                      onClick={(e) => {
                        e.stopPropagation();
                        props.onUnarchive(ws.id);
                      }}
                    >
                      ↺
                    </button>
                    <button
                      type="button"
                      class="workspace-delete"
                      title="Delete forever"
                      onClick={async (e) => {
                        e.stopPropagation();
                        const ok = await ask(
                          `Delete archived workspace "${ws.name}" forever?\n\nThe DB row is removed; on-disk files (including the Claude jsonl) are left alone.`,
                          {
                            title: "Delete forever",
                            kind: "warning",
                            okLabel: "Delete forever",
                            cancelLabel: "Cancel",
                          },
                        );
                        if (ok) props.onDelete(ws.id);
                      }}
                    >
                      ×
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </div>
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
      {/* Global pomodoro HUD pinned to the sidebar foot — visible from any
        * workspace because the timer itself is app-wide, not per-workspace. */}
      <PomodoroHud />
    </aside>
  );
};

export default Sidebar;

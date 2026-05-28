import { invoke } from "@tauri-apps/api/core";

// ---- types ----

export type LinkKind = "url" | "github_issue" | "github_pr";

export interface WorkspaceLink {
  id: string;
  workspace_id: string;
  label: string | null;
  url: string;
  kind: LinkKind;
  created_at: string;
  sort_order: number;
}

export interface WorkspaceTask {
  id: string;
  workspace_id: string;
  title: string;
  done: boolean;
  sort_order: number;
  due_date: string | null;
  created_at: string;
  completed_at: string | null;
}

export type PomodoroMode = "idle" | "work" | "break" | "paused";

export type ActivityKind = "post_tool_use" | "stop" | "notification";

export interface ActivityEntry {
  id: string;
  workspace_id: string;
  kind: ActivityKind;
  summary: string;
  payload: string;
  created_at: string;
}

export function workspaceActivityList(
  workspaceId: string,
  limit: number = 50,
): Promise<ActivityEntry[]> {
  return invoke<ActivityEntry[]>("workspace_activity_list", { workspaceId, limit });
}

// ---- links ----

export function workspaceLinksList(workspaceId: string): Promise<WorkspaceLink[]> {
  return invoke<WorkspaceLink[]>("workspace_links_list", { workspaceId });
}

export function workspaceLinksAdd(
  workspaceId: string,
  url: string,
  label?: string | null,
): Promise<WorkspaceLink> {
  return invoke<WorkspaceLink>("workspace_links_add", {
    workspaceId,
    url,
    label: label ?? null,
  });
}

export function workspaceLinksDelete(id: string): Promise<void> {
  return invoke<void>("workspace_links_delete", { id });
}

export function workspaceLinksReorder(ids: string[]): Promise<void> {
  return invoke<void>("workspace_links_reorder", { ids });
}

// ---- tasks ----

export function workspaceTasksList(
  workspaceId: string,
  includeCompleted: boolean,
): Promise<WorkspaceTask[]> {
  return invoke<WorkspaceTask[]>("workspace_tasks_list", {
    workspaceId,
    includeCompleted,
  });
}

export function workspaceTasksAdd(
  workspaceId: string,
  title: string,
  dueDate?: string | null,
): Promise<WorkspaceTask> {
  return invoke<WorkspaceTask>("workspace_tasks_add", {
    workspaceId,
    title,
    dueDate: dueDate ?? null,
  });
}

export function workspaceTasksToggle(id: string): Promise<boolean> {
  return invoke<boolean>("workspace_tasks_toggle", { id });
}

export function workspaceTasksDelete(id: string): Promise<void> {
  return invoke<void>("workspace_tasks_delete", { id });
}

export function workspaceTasksReorder(ids: string[]): Promise<void> {
  return invoke<void>("workspace_tasks_reorder", { ids });
}

// ---- helpers ----

export function formatMmSs(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

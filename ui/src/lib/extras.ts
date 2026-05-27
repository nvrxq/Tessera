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

export interface PomodoroState {
  workspace_id: string;
  mode: PomodoroMode;
  started_at: string | null;
  paused_at: string | null;
  target_seconds: number;
  elapsed_seconds_before_pause: number;
  cycles_completed: number;
  updated_at: string;
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

// ---- pomodoro ----

export function workspacePomodoroGet(workspaceId: string): Promise<PomodoroState> {
  return invoke<PomodoroState>("workspace_pomodoro_get", { workspaceId });
}

export function workspacePomodoroStart(
  workspaceId: string,
  mode: "work" | "break",
  targetSeconds?: number | null,
): Promise<PomodoroState> {
  return invoke<PomodoroState>("workspace_pomodoro_start", {
    workspaceId,
    mode,
    targetSeconds: targetSeconds ?? null,
  });
}

export function workspacePomodoroPause(workspaceId: string): Promise<PomodoroState> {
  return invoke<PomodoroState>("workspace_pomodoro_pause", { workspaceId });
}

export function workspacePomodoroResume(workspaceId: string): Promise<PomodoroState> {
  return invoke<PomodoroState>("workspace_pomodoro_resume", { workspaceId });
}

export function workspacePomodoroReset(workspaceId: string): Promise<PomodoroState> {
  return invoke<PomodoroState>("workspace_pomodoro_reset", { workspaceId });
}

// ---- helpers ----

/** Compute the remaining seconds for a Pomodoro state, given the current time. */
export function pomodoroRemainingSeconds(state: PomodoroState, nowMs: number): number {
  if (state.mode === "idle") return state.target_seconds;
  if (state.mode === "paused") {
    return Math.max(0, state.target_seconds - state.elapsed_seconds_before_pause);
  }
  // work / break
  if (!state.started_at) return state.target_seconds;
  const startedMs = new Date(state.started_at).getTime();
  const elapsed = Math.max(0, Math.floor((nowMs - startedMs) / 1000));
  return Math.max(0, state.target_seconds - elapsed);
}

export function formatMmSs(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

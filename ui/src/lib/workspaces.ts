import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AgentStatus = "idle" | "working" | "needs_input" | "done" | "crashed";

export interface WorkspaceDto {
  id: string;
  name: string;
  repo_path: string;
  worktree_path: string;
  setup_status: { kind: "pending" | "running" | "ok" };
  created_at: string;
  detected_worktree: string | null;
  detected_branch: string | null;
  dangerous_skip_permissions: boolean;
  session_id: string | null;
  agent_status: AgentStatus | null;
  project_id: string | null;
  sort_order: number;
}

export interface Project {
  id: string;
  name: string;
  accent: string | null;
  created_at: string;
}

export interface WorkspaceStatusEvent {
  workspace_id: string;
  agent_status: AgentStatus;
}

export interface WorkspaceWorktreeEvent {
  workspace_id: string;
  detected_worktree: string;
  detected_branch: string | null;
}

export function createWorkspace(
  folderPath: string,
  name: string,
  dangerousSkipPermissions: boolean,
  projectId: string | null = null,
): Promise<WorkspaceDto> {
  return invoke<WorkspaceDto>("workspace_create", {
    args: {
      folder_path: folderPath,
      name,
      dangerous_skip_permissions: dangerousSkipPermissions,
      project_id: projectId,
    },
  });
}

export function listProjects(): Promise<Project[]> {
  return invoke<Project[]>("project_list");
}

export function createProject(name: string, accent: string | null): Promise<Project> {
  return invoke<Project>("project_create", { name, accent });
}

export function deleteProject(id: string): Promise<void> {
  return invoke<void>("project_delete", { id });
}

/** Reorder request item. Self-describing wire shape lines up with the
 *  Rust `ReorderEntry` struct in `src-tauri/src/commands.rs`. */
export interface ReorderEntry {
  workspace_id: string;
  sort_order: number;
}

export function workspaceReorder(updates: ReorderEntry[]): Promise<void> {
  return invoke<void>("workspace_reorder", { updates });
}

export function workspaceAssignProject(
  workspaceId: string,
  projectId: string | null,
): Promise<void> {
  return invoke<void>("workspace_assign_project", { workspaceId, projectId });
}

/** Curated set of accent swatches for project creation. Single source of
 *  truth so the modal in ProjectsSettings.tsx and the inline picker in
 *  NewWorkspaceForm.tsx stay visually aligned. Order is intentional
 *  (warm → cool, default first). `null` means "no accent — fall back to
 *  the global terracotta". */
export const PROJECT_SWATCHES: Array<{ label: string; value: string | null }> = [
  { label: "Default", value: null },
  { label: "Terracotta", value: "#C8825B" },
  { label: "Sienna", value: "#A85A3C" },
  { label: "Apricot", value: "#E0A370" },
  { label: "Russet", value: "#8C4A2E" },
  { label: "Sand", value: "#C9B58B" },
];

export function listWorkspaces(): Promise<WorkspaceDto[]> {
  return invoke<WorkspaceDto[]>("workspace_list");
}

/** In-flight `workspace_spawn_agent` invocations, keyed by workspaceId.
 *  Idempotent: a second call while the first is still resolving returns
 *  the same promise. Lets the App-level pre-spawn (driven by clicking a
 *  workspace) overlap safely with the Terminal-mount-driven spawn —
 *  whichever caller arrives second just awaits the first one's result
 *  instead of starting a duplicate claude process. */
const inFlightSpawns = new Map<string, Promise<string>>();

export function spawnAgent(
  workspaceId: string,
  cols: number,
  rows: number,
): Promise<string> {
  const existing = inFlightSpawns.get(workspaceId);
  if (existing) return existing;
  const p = invoke<string>("workspace_spawn_agent", { workspaceId, cols, rows })
    .finally(() => inFlightSpawns.delete(workspaceId));
  inFlightSpawns.set(workspaceId, p);
  return p;
}

export function deleteWorkspace(workspaceId: string): Promise<void> {
  return invoke<void>("workspace_delete", { workspaceId });
}

export function renameWorkspace(workspaceId: string, newName: string): Promise<void> {
  return invoke<void>("workspace_rename", { workspaceId, newName });
}

export function listDirectories(input: string): Promise<string[]> {
  return invoke<string[]>("list_directories", { input });
}

export function onWorkspaceStatus(cb: (e: WorkspaceStatusEvent) => void): Promise<UnlistenFn> {
  return listen<WorkspaceStatusEvent>("workspace_status", (event) => cb(event.payload));
}

export function onWorkspaceWorktree(
  cb: (e: WorkspaceWorktreeEvent) => void,
): Promise<UnlistenFn> {
  return listen<WorkspaceWorktreeEvent>("workspace_worktree", (event) => cb(event.payload));
}

export function statusLabel(s: AgentStatus | null): string {
  switch (s) {
    case "working":
      return "Working";
    case "needs_input":
      return "Needs input";
    case "done":
      return "Done";
    case "crashed":
      return "Crashed";
    case "idle":
      return "Idle";
    default:
      return "Ready";
  }
}

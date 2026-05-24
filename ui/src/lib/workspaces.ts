import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AgentStatus = "idle" | "working" | "needs_input" | "done" | "crashed";

export interface WorkspaceDto {
  id: string;
  name: string;
  branch: string;
  repo_path: string;
  worktree_path: string;
  setup_status: { kind: "pending" | "running" | "ok" | "failed"; stderr_tail?: string };
  created_at: string;
  session_id: string | null;
  agent_status: AgentStatus | null;
}

export interface WorkspaceStatusEvent {
  workspace_id: string;
  agent_status: AgentStatus;
}

export function createWorkspace(
  folderPath: string,
  name: string,
  branchName: string | null,
): Promise<WorkspaceDto> {
  return invoke<WorkspaceDto>("workspace_create", {
    args: {
      folder_path: folderPath,
      name,
      branch_name: branchName && branchName.length > 0 ? branchName : null,
    },
  });
}

export function listWorkspaces(): Promise<WorkspaceDto[]> {
  return invoke<WorkspaceDto[]>("workspace_list");
}

export function spawnAgent(workspaceId: string): Promise<string> {
  return invoke<string>("workspace_spawn_agent", { workspaceId });
}

export function deleteWorkspace(workspaceId: string, force: boolean): Promise<void> {
  return invoke<void>("workspace_delete", { workspaceId, force });
}

export function onWorkspaceStatus(
  cb: (e: WorkspaceStatusEvent) => void,
): Promise<UnlistenFn> {
  return listen<WorkspaceStatusEvent>("workspace_status", (event) => cb(event.payload));
}

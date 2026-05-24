import { invoke } from "@tauri-apps/api/core";

export interface WorkspaceDto {
  id: string;
  name: string;
  branch: string;
  repo_path: string;
  worktree_path: string;
  setup_status: { kind: "pending" | "running" | "ok" | "failed"; stderr_tail?: string };
  created_at: string;
  session_id: string | null;
}

export function createWorkspace(repoPath: string, branchName: string): Promise<WorkspaceDto> {
  return invoke<WorkspaceDto>("workspace_create", {
    args: { repo_path: repoPath, branch_name: branchName },
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

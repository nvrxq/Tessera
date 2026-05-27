import { invoke } from "@tauri-apps/api/core";

/** Mirrors `tessera_core::claude::SkillSource` (serde tag = "kind"). */
export type SkillSource =
  | { kind: "global" }
  | { kind: "plugin"; marketplace: string; plugin: string; version: string }
  | { kind: "project" };

export interface Skill {
  name: string;
  description: string;
  triggers: string[];
  source: SkillSource;
  path: string;
}

/** Mirrors `tessera_core::claude::McpSource`. */
export type McpSource = { kind: "global" } | { kind: "project" };

export interface McpServer {
  name: string;
  /** "stdio" | "http" | "" (unspecified). */
  kind: string;
  command: string | null;
  args: string[];
  /** Just the env variable names — values are categorically never returned. */
  env_keys: string[];
  source: McpSource;
  config_path: string;
}

export interface ClaudeInventory {
  skills: Skill[];
  mcp_servers: McpServer[];
}

/**
 * Pull the live skills + MCP inventory Claude Code would see in
 * `workspaceId`. Pass `null` for the header-level view (globals only).
 *
 * Backend command name: `claude_inventory`. Argument is intentionally
 * snake_case-friendly (`workspaceId` here, `workspace_id` on the wire —
 * Tauri remaps automatically).
 */
export function claudeInventory(workspaceId: string | null): Promise<ClaudeInventory> {
  return invoke<ClaudeInventory>("claude_inventory", {
    workspaceId: workspaceId ?? null,
  });
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface SpawnResponse {
  session_id: string;
}

export type PtyEventPayload =
  | { kind: "data"; session_id: string; data_b64: string }
  | { kind: "exit"; session_id: string };

export function spawnShell(cwd: string, cols: number, rows: number): Promise<SpawnResponse> {
  const program = "/bin/bash";
  return invoke<SpawnResponse>("pty_spawn", {
    args: { program, args: ["-l"], cwd, cols, rows },
  });
}

export function writePty(sessionId: string, data: string): Promise<void> {
  const data_b64 = btoa(unescape(encodeURIComponent(data)));
  return invoke<void>("pty_write", { sessionId, dataB64: data_b64 });
}

export function resizePty(sessionId: string, cols: number, rows: number): Promise<void> {
  return invoke<void>("pty_resize", { sessionId, cols, rows });
}

export function killPty(sessionId: string): Promise<void> {
  return invoke<void>("pty_kill", { sessionId });
}

export function onPtyEvent(cb: (e: PtyEventPayload) => void): Promise<UnlistenFn> {
  return listen<PtyEventPayload>("pty_event", (event) => cb(event.payload));
}

export function decodeB64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

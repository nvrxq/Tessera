import { createEffect, onCleanup, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { selectOverlaySession, syncOverlayToElement } from "./lib/overlay";
import { spawnAgent } from "./lib/workspaces";

export interface TerminalProps {
  workspaceId: string;
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
}

/**
 * Anchor element for the native wgpu overlay window.
 *
 * The terminal pane no longer mounts xterm.js. Instead this component:
 *   1. Reserves a div in the layout (the overlay window covers this div).
 *   2. Tells the native overlay which session to render.
 *   3. Spawns the workspace's agent on first mount.
 *   4. Forwards keystrokes via `pty_write` while the host has focus.
 */
export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
  let cleanupSync: (() => void) | null = null;
  let activeSessionId: string | null = props.sessionId;

  onMount(async () => {
    cleanupSync = syncOverlayToElement(host);

    let sid = props.sessionId;
    if (!sid) {
      sid = await spawnAgent(props.workspaceId);
      props.onSpawned(sid);
    }
    activeSessionId = sid;
    await selectOverlaySession(sid);

    // Keyboard input — forward every keystroke to the PTY while host is focused.
    function onKey(ev: KeyboardEvent) {
      if (document.activeElement !== host) return;
      const sid = activeSessionId;
      if (!sid) return;
      ev.preventDefault();
      const bytes = encodeKey(ev);
      if (bytes.length > 0) {
        void invoke("pty_write", {
          sessionId: sid,
          dataB64: btoa(String.fromCharCode(...bytes)),
        });
      }
    }
    document.addEventListener("keydown", onKey);
    onCleanup(() => document.removeEventListener("keydown", onKey));

    // Focus the host so it can receive keystrokes.
    host.focus();
  });

  // If the session prop changes (workspace switch reuses this component),
  // re-select the overlay's active session.
  createEffect(() => {
    if (props.sessionId) {
      activeSessionId = props.sessionId;
      void selectOverlaySession(props.sessionId);
    }
  });

  onCleanup(() => {
    cleanupSync?.();
    void selectOverlaySession(null);
  });

  return <div class="overlay-anchor" ref={host} tabIndex={-1} />;
}

/** Minimal keymap: printable ASCII, Enter, Tab, Backspace, arrows, Ctrl-C/Ctrl-D/Ctrl-L. */
function encodeKey(ev: KeyboardEvent): number[] {
  if (ev.ctrlKey && !ev.altKey && !ev.metaKey) {
    if (ev.key === "c") return [0x03];
    if (ev.key === "d") return [0x04];
    if (ev.key === "l") return [0x0c];
  }
  switch (ev.key) {
    case "Enter":      return [0x0d];
    case "Tab":        return [0x09];
    case "Backspace":  return [0x7f];
    case "Escape":     return [0x1b];
    case "ArrowUp":    return [0x1b, 0x5b, 0x41];
    case "ArrowDown":  return [0x1b, 0x5b, 0x42];
    case "ArrowRight": return [0x1b, 0x5b, 0x43];
    case "ArrowLeft":  return [0x1b, 0x5b, 0x44];
    case "Home":       return [0x1b, 0x5b, 0x48];
    case "End":        return [0x1b, 0x5b, 0x46];
    case "PageUp":     return [0x1b, 0x5b, 0x35, 0x7e];
    case "PageDown":   return [0x1b, 0x5b, 0x36, 0x7e];
  }
  if (ev.key.length === 1) {
    return [ev.key.charCodeAt(0) & 0xff];
  }
  return [];
}

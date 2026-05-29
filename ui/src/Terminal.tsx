// Terminal view — a workspace's Claude Code session, rendered through the
// renderer pool (xterm.js + WebGL with context-loss recovery — see
// lib/rendererPool.ts). The pool + session engine are ported from Terax
// (github.com/crynta/terax-ai, Apache-2.0 — see NOTICE). The backend streams
// raw PTY bytes over an IPC Channel (terminal_attach) and answers
// Device-Attributes queries on the child's behalf (crates/pty da_filter).
//
// This component owns one leaf (one pane). Workspace/pane splitting layers more
// leaves on top in a later phase; the pool already supports up to POOL_MAX_SIZE
// live slots.

import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { settings } from "./lib/settings";
import {
  applyCursor,
  applyFontFamily,
  applyFontSize,
  applyScrollback,
  applyTheme,
} from "./lib/rendererPool";
import {
  disposeLeaf,
  focusLeaf,
  leafDims,
  leafSessionId,
  newLeafId,
  showLeaf,
  writeLeaf,
} from "./lib/termSession";
import { spawnAgent } from "./lib/workspaces";

export interface TerminalProps {
  workspaceId: string;
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
}

export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
  let ptyUnlisten: UnlistenFn | null = null;
  let dropUnlisten: UnlistenFn | null = null;
  // Stable leaf id for this component, reused as it rebinds across workspace
  // switches (one visible terminal at a time).
  const leafId = newLeafId();
  const spawning = new Set<string>();
  // Sessions whose claude has exited — never (re)bind to them.
  const exitedSessions = new Set<string>();

  const [phase, setPhase] = createSignal<
    "spawning" | "connecting" | "ready" | "exited" | "error"
  >(props.sessionId ? "connecting" : "spawning");
  const [spawnError, setSpawnError] = createSignal<string | null>(null);

  function showSession(sid: string) {
    setPhase("connecting");
    const { cols, rows } = leafDims(leafId);
    const warm = showLeaf({
      leafId,
      sessionId: sid,
      container: host,
      cols,
      rows,
      focused: true,
      onFirstBytes: () => setPhase("ready"),
    });
    if (warm) setPhase("ready");
  }

  function attemptSpawn(ws: string) {
    if (spawning.has(ws)) return;
    spawning.add(ws);
    setSpawnError(null);
    setPhase("spawning");
    void (async () => {
      try {
        const { cols, rows } = leafDims(leafId);
        const newSid = await spawnAgent(ws, cols, rows);
        if (props.workspaceId === ws) {
          showSession(newSid);
          props.onSpawned(newSid);
        }
      } catch (e) {
        if (props.workspaceId === ws) {
          setSpawnError(String(e));
          setPhase("error");
        }
      } finally {
        spawning.delete(ws);
      }
    })();
  }

  const retrySpawn = () => {
    setPhase("spawning");
    attemptSpawn(props.workspaceId);
  };

  onMount(() => {
    void (async () => {
      ptyUnlisten = await listen<{ kind: string; session_id: string }>(
        "pty_event",
        (event) => {
          const { kind, session_id } = event.payload;
          if (kind !== "exit") return;
          exitedSessions.add(session_id);
          if (session_id === leafSessionId(leafId)) setPhase("exited");
        },
      );

      // Drag-and-drop a file from the OS → type its path as a bracketed paste
      // (Claude Code recognises a pasted image path as an attachment).
      dropUnlisten = await listen<{
        paths: string[];
        position: { x: number; y: number };
      }>("tauri://drag-drop", (event) => {
        if (!leafSessionId(leafId)) return;
        const { paths, position } = event.payload;
        if (!paths || paths.length === 0) return;
        const rect = host.getBoundingClientRect();
        const inside =
          position.x >= rect.left &&
          position.x <= rect.right &&
          position.y >= rect.top &&
          position.y <= rect.bottom;
        if (!inside) return;
        writeLeaf(leafId, `\x1b[200~${paths.join(" ")}\x1b[201~`);
      });
    })();

    // Initial bind — done explicitly (host now exists) rather than relying on
    // the binding createEffect firing first. The effect below handles every
    // subsequent workspace/session switch; its guard (leafSessionId === sid)
    // makes this first bind a no-op on the effect's first run.
    const initSid = props.sessionId;
    if (initSid) {
      if (exitedSessions.has(initSid)) setPhase("exited");
      else showSession(initSid);
    } else {
      attemptSpawn(props.workspaceId);
    }
  });

  // (Re)bind whenever the selected workspace or its session changes.
  createEffect(() => {
    const ws = props.workspaceId;
    const sid = props.sessionId;
    if (!host) return;
    if (spawning.has(ws)) return;
    if (sid) {
      if (exitedSessions.has(sid)) {
        setPhase("exited");
        return;
      }
      if (leafSessionId(leafId) === sid) return;
      showSession(sid);
      return;
    }
    attemptSpawn(ws);
  });

  // Focus the terminal whenever it becomes ready.
  createEffect(() => {
    if (phase() === "ready") focusLeaf(leafId);
  });

  // Live settings → pooled terminals (theme / font / cursor / scrollback).
  createEffect(() => {
    const cfg = settings();
    applyTheme();
    applyFontFamily(cfg.terminal.font_family);
    applyFontSize(cfg.terminal.font_size_px);
    applyCursor();
    applyScrollback(cfg.behavior?.save_scrollback_lines ?? 10_000);
  });

  onCleanup(() => {
    ptyUnlisten?.();
    dropUnlisten?.();
    disposeLeaf(leafId);
  });

  return (
    <div
      class="overlay-anchor"
      style={{ position: "relative", width: "100%", height: "100%" }}
    >
      <div ref={host} style={{ width: "100%", height: "100%" }} />
      <div
        class="terminal-loading"
        classList={{ "terminal-loading--ready": phase() === "ready" }}
        aria-hidden={phase() === "ready"}
      >
        <div class="terminal-loading-stack">
          <span class="terminal-loading-brand">tessera</span>
          <Show when={phase() === "spawning" || phase() === "connecting"}>
            <div class="terminal-loading-dots" aria-label="loading">
              <span />
              <span />
              <span />
            </div>
            <span class="terminal-loading-caption">
              {phase() === "spawning" ? "starting claude" : "connecting"}
            </span>
          </Show>
          <Show when={phase() === "error"}>
            <span
              class="terminal-loading-caption"
              style={{ color: "var(--accent, #C8825B)" }}
            >
              couldn’t start claude
            </span>
            <Show when={spawnError()}>
              <span
                class="terminal-loading-caption"
                style={{
                  "max-width": "34ch",
                  "font-size": "11px",
                  opacity: "0.7",
                  "text-align": "center",
                  "word-break": "break-word",
                }}
              >
                {spawnError()}
              </span>
            </Show>
            <button type="button" class="terminal-loading-retry" onClick={retrySpawn}>
              Retry
            </button>
          </Show>
          <Show when={phase() === "exited"}>
            <span class="terminal-loading-caption">session ended</span>
            <button type="button" class="terminal-loading-retry" onClick={retrySpawn}>
              Restart
            </button>
          </Show>
        </div>
      </div>
    </div>
  );
}

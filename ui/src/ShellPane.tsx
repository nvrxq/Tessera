// ShellPane — a companion login-shell terminal beside the Claude pane (run
// git/tests/etc. while claude works). Renders through the same renderer pool as
// the main Terminal; spawns via `pty_spawn_shell`. The shell PTY lives in the
// backend supervisor, so it survives workspace switches (hideLeaf only detaches;
// the backend ring replays on return). Closing the pane kills it.

import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
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
  leafDims,
  leafSessionId,
  newLeafId,
  showLeaf,
} from "./lib/termSession";
import { activeThemeId } from "./lib/themes";

export interface ShellPaneProps {
  cwd: string;
  /** Existing shell session to reattach to (survives workspace switches), or
   *  null to spawn a fresh one. */
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
  onClose: () => void;
}

export default function ShellPane(props: ShellPaneProps) {
  let host!: HTMLDivElement;
  const leafId = newLeafId();
  let ptyUnlisten: UnlistenFn | null = null;
  let spawning = false;
  const [phase, setPhase] = createSignal<
    "spawning" | "connecting" | "ready" | "exited" | "error"
  >(props.sessionId ? "connecting" : "spawning");
  const [err, setErr] = createSignal<string | null>(null);

  function showSession(sid: string) {
    setPhase("connecting");
    const { cols, rows } = leafDims(leafId);
    const warm = showLeaf({
      leafId,
      sessionId: sid,
      container: host,
      cols,
      rows,
      focused: false,
      onFirstBytes: () => setPhase("ready"),
    });
    if (warm) setPhase("ready");
  }

  function spawnShell() {
    if (spawning) return;
    spawning = true;
    setErr(null);
    setPhase("spawning");
    const { cols, rows } = leafDims(leafId);
    void invoke<{ session_id: string }>("pty_spawn_shell", {
      cwd: props.cwd,
      cols,
      rows,
    })
      .then((res) => {
        props.onSpawned(res.session_id);
        showSession(res.session_id);
      })
      .catch((e) => {
        setErr(String(e));
        setPhase("error");
      })
      .finally(() => {
        spawning = false;
      });
  }

  onMount(() => {
    void (async () => {
      ptyUnlisten = await listen<{ kind: string; session_id: string }>(
        "pty_event",
        (ev) => {
          if (
            ev.payload.kind === "exit" &&
            ev.payload.session_id === leafSessionId(leafId)
          ) {
            setPhase("exited");
            props.onClose();
          }
        },
      );
    })();
    if (props.sessionId) showSession(props.sessionId);
    else spawnShell();
  });

  // Live settings + theme → pooled terminals (idempotent across panes).
  createEffect(() => {
    activeThemeId();
    const cfg = settings();
    applyTheme();
    applyFontFamily(cfg.terminal.font_family);
    applyFontSize(cfg.terminal.font_size_px);
    applyCursor();
    applyScrollback(cfg.behavior?.save_scrollback_lines ?? 10_000);
  });

  onCleanup(() => {
    ptyUnlisten?.();
    // Detach only — the backend keeps the shell alive so switching workspaces
    // and back restores it. The pane's close button is what kills it.
    disposeLeaf(leafId);
  });

  return (
    <div class="pane-body">
      <div ref={host} style={{ width: "100%", height: "100%" }} />
      <button
        type="button"
        class="pane-close"
        title="Close shell pane"
        aria-label="Close shell pane"
        onClick={() => props.onClose()}
      >
        ×
      </button>
      <Show when={phase() !== "ready"}>
        <div class="pane-overlay">
          <span class="pane-overlay-text">
            {phase() === "error"
              ? (err() ?? "couldn’t start shell")
              : phase() === "exited"
                ? "shell exited"
                : "starting shell…"}
          </span>
        </div>
      </Show>
    </div>
  );
}

// WorkspacePanes — lays out a workspace's Claude pane plus any companion shell
// panes as an even flex split (row or column). The Claude pane is always the
// main, unchanged <Terminal>; "+ shell" adds a login-shell pane in the
// workspace cwd. Pane state is kept per-workspace in a store so it survives
// switching away and back (the shell PTYs live in the backend; ShellPane
// reattaches by sessionId).
//
// A flat even-split model (no draggable resize / nested tree) — deliberately
// simple and robust. The renderer pool (lib/rendererPool.ts) supplies a fresh
// xterm slot per pane and caps live WebGL contexts.

import { createStore } from "solid-js/store";
import { For, Show } from "solid-js";
import Terminal from "./Terminal";
import ShellPane from "./ShellPane";
import { invoke } from "@tauri-apps/api/core";

export interface WorkspacePanesProps {
  workspaceId: string;
  cwd: string;
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
}

type ShellState = { key: number; sessionId: string | null };
type PanesState = { dir: "row" | "col"; shells: ShellState[] };

export default function WorkspacePanes(props: WorkspacePanesProps) {
  const [store, setStore] = createStore<Record<string, PanesState>>({});
  let keyCounter = 1;

  const ws = () => props.workspaceId;
  const panes = (): PanesState => store[ws()] ?? { dir: "row", shells: [] };

  function ensureWs() {
    if (!store[ws()]) setStore(ws(), { dir: "row", shells: [] });
  }

  function addShell() {
    ensureWs();
    const key = keyCounter++;
    setStore(ws(), "shells", (s) => [...s, { key, sessionId: null }]);
  }

  // Take the owning workspace id explicitly: a shell's spawn can resolve AFTER
  // the user switched away (the pane unmounted), and the session id must still
  // be recorded against the workspace that owns it — otherwise the PTY leaks.
  function setShellSession(wsId: string, key: number, sid: string) {
    if (!store[wsId]) return;
    setStore(wsId, "shells", (s) => s.key === key, "sessionId", sid);
  }

  function closeShell(wsId: string, key: number) {
    const sp = store[wsId]?.shells.find((s) => s.key === key);
    if (sp?.sessionId) {
      void invoke("pty_kill", { sessionId: sp.sessionId }).catch(() => {});
    }
    if (store[wsId]) {
      setStore(wsId, "shells", (s) => s.filter((x) => x.key !== key));
    }
  }

  function toggleDir() {
    ensureWs();
    setStore(ws(), "dir", (d) => (d === "row" ? "col" : "row"));
  }

  return (
    <div
      class="panes"
      classList={{ "panes--col": panes().dir === "col" }}
      style={{ width: "100%", height: "100%" }}
    >
      <div class="pane">
        <Terminal
          workspaceId={props.workspaceId}
          sessionId={props.sessionId}
          onSpawned={props.onSpawned}
        />
      </div>
      <For each={panes().shells}>
        {(sp) => {
          // Capture the workspace this pane belongs to at render time (the For
          // only renders the active workspace's shells), so async callbacks
          // target the right workspace even after a switch.
          const owner = props.workspaceId;
          return (
            <div class="pane">
              <ShellPane
                cwd={props.cwd}
                sessionId={sp.sessionId}
                onSpawned={(sid) => setShellSession(owner, sp.key, sid)}
                onClose={() => closeShell(owner, sp.key)}
              />
            </div>
          );
        }}
      </For>
      <div class="panes-toolbar">
        <Show when={panes().shells.length > 0}>
          <button
            type="button"
            class="panes-toolbar-btn"
            title={panes().dir === "row" ? "Stack panes vertically" : "Place panes side by side"}
            aria-label="Toggle split direction"
            onClick={toggleDir}
          >
            ⇄
          </button>
        </Show>
        <button
          type="button"
          class="panes-toolbar-btn"
          title="Split: add a shell pane in this workspace"
          aria-label="Add shell pane"
          onClick={addShell}
        >
          + shell
        </button>
      </div>
    </div>
  );
}

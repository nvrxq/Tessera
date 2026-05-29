// Terminal view — xterm.js + WebGL renderer.
//
// Replaces the previous bespoke Canvas2D painter + wezterm-term snapshot
// pipeline. xterm.js owns VT parsing, rendering, scrollback, and selection;
// the Rust backend streams raw PTY bytes over an IPC Channel (terminal_attach)
// and we feed them straight into `term.write`. The renderer wiring, IME guard,
// keymap, and copy/paste handling are copied from Terax
// (github.com/crynta/terax-ai, Apache-2.0) — see NOTICE.

import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Terminal as Xterm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import { setTerminalFontSize, settings } from "./lib/settings";
import { buildTerminalTheme } from "./lib/terminalTheme";
import {
  terminalDeleteSequence,
  terminalLineNavigationSequence,
  terminalWordNavigationSequence,
} from "./lib/xtermKeymap";
import { spawnAgent } from "./lib/workspaces";

export interface TerminalProps {
  workspaceId: string;
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
}

const MIN_FONT_PX = 8;
const MAX_FONT_PX = 32;
const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);
const TEXT_ENCODER = new TextEncoder();

const clampFont = (px: number) =>
  Math.min(MAX_FONT_PX, Math.max(MIN_FONT_PX, Math.round(px)));

/** Encode an xterm `onData` string as UTF-8 → base64 (the wire format the
 *  `pty_write` command expects) and ship it to the PTY. */
function ptyWrite(sessionId: string, data: string): void {
  const bytes = TEXT_ENCODER.encode(data);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  void invoke("pty_write", { sessionId, dataB64: btoa(bin) }).catch(() => {});
}

export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
  let term: Xterm | null = null;
  let fit: FitAddon | null = null;
  let search: SearchAddon | null = null;
  let webgl: WebglAddon | null = null;
  let ro: ResizeObserver | null = null;
  let ptyUnlisten: UnlistenFn | null = null;
  let dropUnlisten: UnlistenFn | null = null;
  // The byte channel currently bound to `activeSid`. Stale channels (from a
  // previous session) compare unequal and have their messages ignored.
  let dataChannel: Channel<ArrayBuffer> | null = null;
  let activeSid: string | null = null;
  let lastCols = 0;
  let lastRows = 0;
  let sawFirstByte = false;
  const spawning = new Set<string>();
  // Sessions whose claude has exited — never (re)bind to them.
  const exitedSessions = new Set<string>();

  const [phase, setPhase] = createSignal<
    "spawning" | "connecting" | "ready" | "exited" | "error"
  >(props.sessionId ? "connecting" : "spawning");
  const [spawnError, setSpawnError] = createSignal<string | null>(null);

  function createTerm() {
    const cfg = settings().terminal;
    term = new Xterm({
      fontFamily: cfg.font_family || '"Geist Mono", monospace',
      fontSize: clampFont(cfg.font_size_px),
      theme: buildTerminalTheme(),
      cursorBlink: cfg.cursor_blink,
      cursorStyle: "bar",
      cursorInactiveStyle: "outline",
      allowProposedApi: true,
      scrollback: 10_000,
    });
    fit = new FitAddon();
    search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(
      new WebLinksAddon((_e, uri) => {
        void openUrl(uri).catch(() => {});
      }),
    );
    term.open(host);
    attachWebgl();

    term.onData((data) => {
      const sid = activeSid;
      if (sid) ptyWrite(sid, data);
    });

    term.attachCustomKeyEventHandler((event) => {
      // During IME composition the browser is assembling a multi-keystroke
      // character; raw keydowns (incl. the committing Enter) must NOT reach the
      // PTY — xterm forwards the composed string via compositionend. keyCode
      // 229 is what Chromium reports for keys pressed inside an IME session.
      if (event.isComposing || event.keyCode === 229) return false;
      const sid = activeSid;
      if (!sid) return true;

      // Ctrl/Cmd +/- /0 → terminal font zoom (routed through settings).
      if ((event.ctrlKey || event.metaKey) && !event.altKey) {
        if (event.key === "=" || event.key === "+") {
          event.preventDefault();
          if (event.type === "keydown")
            void setTerminalFontSize(clampFont(settings().terminal.font_size_px + 1));
          return false;
        }
        if (event.key === "-" || event.key === "_") {
          event.preventDefault();
          if (event.type === "keydown")
            void setTerminalFontSize(clampFont(settings().terminal.font_size_px - 1));
          return false;
        }
        if (event.key === "0") {
          event.preventDefault();
          if (event.type === "keydown") void setTerminalFontSize(14);
          return false;
        }
      }

      const lineNav = terminalLineNavigationSequence(event, { isMac: IS_MAC });
      if (lineNav) {
        event.preventDefault();
        if (event.type === "keydown") ptyWrite(sid, lineNav);
        return false;
      }
      const wordNav = terminalWordNavigationSequence(event);
      if (wordNav) {
        event.preventDefault();
        if (event.type === "keydown") ptyWrite(sid, wordNav);
        return false;
      }
      const del = terminalDeleteSequence(event, { isMac: IS_MAC });
      if (del) {
        event.preventDefault();
        if (event.type === "keydown") ptyWrite(sid, del);
        return false;
      }
      // Shift+Enter → ESC + CR (Claude Code uses this for a soft newline).
      if (
        event.key === "Enter" &&
        event.shiftKey &&
        !event.altKey &&
        !event.ctrlKey &&
        !event.metaKey
      ) {
        event.preventDefault();
        if (event.type === "keydown") ptyWrite(sid, "\x1b\r");
        return false;
      }
      // Copy: macOS Cmd+C, others Ctrl+Shift+C. Bare Ctrl+C (no shift) is left
      // alone so it still reaches the PTY as SIGINT.
      const copyMod = IS_MAC
        ? event.metaKey && !event.ctrlKey && !event.altKey
        : event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
      if (copyMod && (event.code === "KeyC" || event.key.toLowerCase() === "c")) {
        if (event.type === "keydown" && term?.hasSelection()) {
          const sel = term.getSelection();
          if (sel) void navigator.clipboard.writeText(sel).catch(() => {});
          event.preventDefault();
          return false;
        }
        if (IS_MAC) {
          event.preventDefault();
          return false;
        }
      }
      // Paste: macOS Cmd+V, others Ctrl+Shift+V.
      const pasteMod = IS_MAC
        ? event.metaKey && !event.ctrlKey && !event.altKey
        : event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
      if (pasteMod && (event.code === "KeyV" || event.key.toLowerCase() === "v")) {
        if (event.type === "keydown") {
          void navigator.clipboard
            .readText()
            .then((t) => {
              if (t) term?.paste(t);
            })
            .catch(() => {});
        }
        event.preventDefault();
        return false;
      }
      return true;
    });
  }

  function attachWebgl() {
    if (!term || webgl) return;
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        try {
          addon.dispose();
        } catch {
          /* noop */
        }
        if (webgl === addon) webgl = null;
      });
      term.loadAddon(addon);
      webgl = addon;
    } catch (e) {
      // WebGL unavailable (headless WebView / GPU reset) — xterm falls back to
      // its DOM renderer automatically.
      console.warn("[tessera] webgl renderer unavailable", e);
    }
  }

  function doFit() {
    if (!term || !fit) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    const sid = activeSid;
    if (sid && (term.cols !== lastCols || term.rows !== lastRows)) {
      lastCols = term.cols;
      lastRows = term.rows;
      void invoke("pty_resize", {
        sessionId: sid,
        cols: term.cols,
        rows: term.rows,
      }).catch(() => {});
    }
  }

  /** Bind a session's byte stream into the (single, reused) terminal. Resets
   *  the screen, registers a fresh Channel — the backend replays the session's
   *  ring buffer then streams live — and sizes the PTY to the current fit. */
  function bindSession(sid: string) {
    if (!term) return;
    // Stop streaming the session we're leaving — its PTY keeps running and its
    // ring keeps buffering, so a switch-back replays everything, but we don't
    // want the backend pushing its live bytes to a channel we've abandoned.
    if (activeSid && activeSid !== sid) {
      void invoke("terminal_detach", { sessionId: activeSid }).catch(() => {});
    }
    activeSid = sid;
    sawFirstByte = false;
    lastCols = 0;
    lastRows = 0;
    term.reset();

    const ch = new Channel<ArrayBuffer>();
    dataChannel = ch;
    ch.onmessage = (buf) => {
      // Ignore a stale channel left over from a previous session.
      if (ch !== dataChannel || activeSid !== sid || !term) return;
      term.write(new Uint8Array(buf));
      if (!sawFirstByte) {
        sawFirstByte = true;
        setPhase("ready");
        // Re-fit now that the overlay is gone and content is flowing — the
        // first fit during bind may have run before the container settled.
        queueMicrotask(() => doFit());
      }
    };
    void invoke("terminal_attach", { sessionId: sid, onData: ch }).catch((e) => {
      console.warn("terminal_attach failed", e);
    });
    doFit();
    term.focus();
  }

  function attemptSpawn(ws: string) {
    if (spawning.has(ws)) return;
    spawning.add(ws);
    setSpawnError(null);
    void (async () => {
      try {
        const cols = term?.cols ?? 80;
        const rows = term?.rows ?? 24;
        const newSid = await spawnAgent(ws, cols, rows);
        if (props.workspaceId === ws) {
          setPhase("connecting");
          bindSession(newSid);
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
    createTerm();
    doFit();
    ro = new ResizeObserver(() => doFit());
    ro.observe(host);

    void (async () => {
      ptyUnlisten = await listen<{ kind: string; session_id: string }>(
        "pty_event",
        (event) => {
          const { kind, session_id } = event.payload;
          if (kind !== "exit") return;
          exitedSessions.add(session_id);
          if (session_id === activeSid) setPhase("exited");
        },
      );

      // Drag-and-drop a file from the OS → type its path as a bracketed paste
      // (Claude Code recognises a pasted image path as an attachment).
      dropUnlisten = await listen<{
        paths: string[];
        position: { x: number; y: number };
      }>("tauri://drag-drop", (event) => {
        const sid = activeSid;
        if (!sid) return;
        const { paths, position } = event.payload;
        if (!paths || paths.length === 0) return;
        const rect = host.getBoundingClientRect();
        const inside =
          position.x >= rect.left &&
          position.x <= rect.right &&
          position.y >= rect.top &&
          position.y <= rect.bottom;
        if (!inside) return;
        ptyWrite(sid, `\x1b[200~${paths.join(" ")}\x1b[201~`);
      });
    })();

    // Initial bind — done explicitly here (term now exists) rather than
    // relying on the binding createEffect firing after onMount. The effect
    // below still handles every subsequent workspace/session switch; its
    // guards (`sid === activeSid`, `spawning.has(ws)`) make this first bind
    // a no-op on the effect's first run, so there's no double-bind.
    const initSid = props.sessionId;
    if (initSid) {
      if (exitedSessions.has(initSid)) {
        setPhase("exited");
      } else {
        setPhase("connecting");
        bindSession(initSid);
      }
    } else {
      attemptSpawn(props.workspaceId);
    }
  });

  // (Re)bind whenever the selected workspace or its session changes. Runs
  // after onMount (registered later), so `term` exists. Guards against
  // binding a dead session and against fighting an in-flight spawn.
  createEffect(() => {
    const ws = props.workspaceId;
    const sid = props.sessionId;
    if (!term) return;
    if (spawning.has(ws)) return;
    if (sid) {
      if (sid === activeSid) return;
      if (exitedSessions.has(sid)) {
        activeSid = null;
        setPhase("exited");
        return;
      }
      setPhase("connecting");
      bindSession(sid);
      return;
    }
    attemptSpawn(ws);
  });

  // Live settings → xterm theme / font.
  createEffect(() => {
    const cfg = settings();
    if (!term) return;
    term.options.theme = buildTerminalTheme();
    const fpx = clampFont(cfg.terminal.font_size_px);
    if (term.options.fontSize !== fpx) term.options.fontSize = fpx;
    if (term.options.fontFamily !== cfg.terminal.font_family) {
      term.options.fontFamily = cfg.terminal.font_family;
    }
    term.options.cursorBlink = cfg.terminal.cursor_blink;
    queueMicrotask(() => doFit());
  });

  onCleanup(() => {
    ro?.disconnect();
    ptyUnlisten?.();
    dropUnlisten?.();
    try {
      webgl?.dispose();
    } catch {
      /* noop */
    }
    try {
      term?.dispose();
    } catch {
      /* noop */
    }
    term = null;
  });

  return (
    <div class="overlay-anchor" style={{ position: "relative", width: "100%", height: "100%" }}>
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

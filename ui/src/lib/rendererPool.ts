// Renderer pool — a small bounded pool of reused xterm.js instances ("slots")
// shared across terminal "leaves" (panes). Ported from Terax
// (github.com/crynta/terax-ai, Apache-2.0 — see NOTICE), adapted to Tessera:
//
//   * Bind/unbind a leaf's PTY stream via Tessera's `terminal_attach` /
//     `terminal_detach` (the backend replays its per-session ring on attach),
//     instead of Terax's frontend serialize-snapshot + dormant-ring.
//   * Tessera settings/theme instead of Terax's preferences store.
//
// Why a pool at all: every WebGL-backed `Terminal` holds a scarce GPU context.
// Creating one per leaf and never releasing it exhausts the browser's context
// budget — the terminal then silently falls back to the DOM renderer or goes
// blank ("vanishes"). The pool caps live contexts at POOL_MAX_SIZE and recycles
// the least-recently-used slot. It also RECOVERS from context loss (GPU
// reset / sleep-wake) by re-attaching the WebGL addon, which Tessera's previous
// single-terminal renderer did not do.

import { invoke, Channel } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { setTerminalFontSize, settings } from "./settings";
import { buildTerminalTheme } from "./terminalTheme";
import {
  terminalDeleteSequence,
  terminalLineNavigationSequence,
  terminalWordNavigationSequence,
} from "./xtermKeymap";

export const POOL_MAX_SIZE = 5;
const FIT_DEBOUNCE_MS = 8;
const PTY_RESIZE_DEBOUNCE_MS = 256;
const MIN_FONT_PX = 8;
const MAX_FONT_PX = 32;

const TEXT_ENCODER = new TextEncoder();

const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

const clampFont = (px: number) =>
  Math.min(MAX_FONT_PX, Math.max(MIN_FONT_PX, Math.round(px)));

/** Identifies a terminal leaf (pane). Carries the backend PTY session uuid so
 *  the pool can attach/detach + write/resize the right session. */
export type LeafRef = {
  leafId: number;
  sessionId: string;
};

/** What the session layer must provide so the pool can talk to the backend. */
export type PoolAdapter = {
  /** uuid of the PTY session currently behind `leafId`, or null if gone. */
  sessionIdFor(leafId: number): string | null;
  isLeafFocused(leafId: number): boolean;
  /** A slot was recycled away from this leaf — stop streaming its session. */
  evictLeaf(leafId: number): void;
  /** First bytes arrived for this leaf's freshly-bound slot. */
  onFirstBytes(leafId: number): void;
  /** The leaf's PTY exited (Exit event observed elsewhere is separate — this is
   *  not used for that; reserved for parity with Terax). */
};

export type Slot = {
  readonly id: number;
  readonly term: Terminal;
  readonly fitAddon: FitAddon;
  readonly searchAddon: SearchAddon;
  readonly host: HTMLDivElement;
  webglAddon: WebglAddon | null;
  webglCanvases: HTMLCanvasElement[];
  currentLeafId: number | null;
  /** Backend PTY session uuid currently streamed into this slot (attached via
   *  `terminal_attach`). Null when the slot is parked in the recycler. */
  sessionId: string | null;
  /** The byte Channel currently attached for `sessionId`. Stale channels
   *  (from a previous binding) compare unequal and have messages ignored. */
  dataChannel: Channel<ArrayBuffer> | null;
  observer: ResizeObserver | null;
  fitTimer: ReturnType<typeof setTimeout> | null;
  ptyTimer: ReturnType<typeof setTimeout> | null;
  unhideRaf: number | null;
  sawFirstByte: boolean;
  lastCols: number;
  lastRows: number;
  lastW: number;
  lastH: number;
  lastUsedAt: number;
};

const slots: Slot[] = [];
let recyclerEl: HTMLDivElement | null = null;
let adapter: PoolAdapter | null = null;

export function configureRendererPool(a: PoolAdapter): void {
  adapter = a;
}

export function forEachSlot(fn: (slot: Slot) => void): void {
  for (const s of slots) fn(s);
}

export function poolSize(): number {
  return slots.length;
}

function getRecycler(): HTMLDivElement {
  if (recyclerEl && recyclerEl.isConnected) return recyclerEl;
  const el = document.createElement("div");
  el.setAttribute("data-tessera-recycler", "");
  el.style.cssText =
    "position:fixed;left:-99999px;top:-99999px;width:1024px;height:768px;overflow:hidden;pointer-events:none;contain:strict;";
  document.body.appendChild(el);
  recyclerEl = el;
  return el;
}

function cursorStyle(): "block" | "bar" | "underline" {
  const s = settings().terminal.cursor_shape;
  return s === "bar" || s === "underline" ? s : "block";
}

function termOptions() {
  const t = settings().terminal;
  return {
    fontFamily: t.font_family || '"Geist Mono", monospace',
    fontSize: clampFont(t.font_size_px),
    theme: buildTerminalTheme(),
    cursorBlink: t.cursor_blink,
    cursorStyle: cursorStyle(),
    cursorInactiveStyle: "outline" as const,
    scrollback: settings().behavior?.save_scrollback_lines ?? 10_000,
    allowProposedApi: true,
  };
}

function writeToPty(leafId: number, data: string): void {
  const sid = adapter?.sessionIdFor(leafId);
  if (!sid) return;
  const bytes = TEXT_ENCODER.encode(data);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  void invoke("pty_write", { sessionId: sid, dataB64: btoa(bin) }).catch(() => {});
}

function resizePty(leafId: number, cols: number, rows: number): void {
  const sid = adapter?.sessionIdFor(leafId);
  if (!sid || cols <= 0 || rows <= 0) return;
  void invoke("pty_resize", { sessionId: sid, cols, rows }).catch(() => {});
}

function createSlot(): Slot {
  const term = new Terminal(termOptions());
  const fitAddon = new FitAddon();
  const searchAddon = new SearchAddon();
  term.loadAddon(fitAddon);
  term.loadAddon(searchAddon);
  term.loadAddon(
    new WebLinksAddon((_e, uri) => {
      void openUrl(uri).catch(() => {});
    }),
  );

  const host = document.createElement("div");
  host.style.cssText = "width:100%;height:100%;";
  host.setAttribute("data-tessera-slot", String(slots.length));
  getRecycler().appendChild(host);
  term.open(host);

  const slot: Slot = {
    id: slots.length,
    term,
    fitAddon,
    searchAddon,
    host,
    webglAddon: null,
    webglCanvases: [],
    currentLeafId: null,
    sessionId: null,
    dataChannel: null,
    observer: null,
    fitTimer: null,
    ptyTimer: null,
    unhideRaf: null,
    sawFirstByte: false,
    lastCols: term.cols,
    lastRows: term.rows,
    lastW: 0,
    lastH: 0,
    lastUsedAt: 0,
  };

  attachWebgl(slot);

  term.attachCustomKeyEventHandler((event) => {
    // During IME composition the browser assembles a multi-keystroke character
    // (pinyin → hanzi, jamo → syllable). Raw keydowns — including the Enter
    // that commits a candidate — must NOT reach the PTY; xterm forwards the
    // composed string via compositionend. keyCode 229 is Chromium's "Process".
    if (event.isComposing || event.keyCode === 229) return false;

    const leafId = slot.currentLeafId;
    if (leafId === null) return false;

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
      if (event.type === "keydown") writeToPty(leafId, lineNav);
      return false;
    }
    const wordNav = terminalWordNavigationSequence(event);
    if (wordNav) {
      event.preventDefault();
      if (event.type === "keydown") writeToPty(leafId, wordNav);
      return false;
    }
    const del = terminalDeleteSequence(event, { isMac: IS_MAC });
    if (del) {
      event.preventDefault();
      if (event.type === "keydown") writeToPty(leafId, del);
      return false;
    }
    // Shift+Enter → ESC + CR (Claude Code's soft newline).
    if (
      event.key === "Enter" &&
      event.shiftKey &&
      !event.altKey &&
      !event.ctrlKey &&
      !event.metaKey
    ) {
      event.preventDefault();
      if (event.type === "keydown") writeToPty(leafId, "\x1b\r");
      return false;
    }
    // Copy: macOS Cmd+C, others Ctrl+Shift+C. Bare Ctrl+C stays as SIGINT.
    const copyMod = IS_MAC
      ? event.metaKey && !event.ctrlKey && !event.altKey
      : event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;
    if (copyMod && (event.code === "KeyC" || event.key.toLowerCase() === "c")) {
      if (event.type === "keydown" && slot.term.hasSelection()) {
        const sel = slot.term.getSelection();
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
          .then((text) => {
            if (text) slot.term.paste(text);
          })
          .catch(() => {});
      }
      event.preventDefault();
      return false;
    }
    return true;
  });

  term.onData((data) => {
    const leafId = slot.currentLeafId;
    if (leafId === null) return;
    writeToPty(leafId, data);
  });

  slots.push(slot);
  return slot;
}

type PickResult = { slot: Slot; previousLeafId: number | null };

function isAltScreen(s: Slot): boolean {
  try {
    return s.term.buffer.active.type === "alternate";
  } catch {
    return false;
  }
}

function pickSlotFor(leafId: number): PickResult {
  const free = slots.find((s) => s.currentLeafId === null);
  if (free) return { slot: free, previousLeafId: null };
  if (slots.length < POOL_MAX_SIZE)
    return { slot: createSlot(), previousLeafId: null };

  // All slots busy and pool full — recycle the least valuable. Prefer
  // not-focused, not-alt-screen, least-recently-used.
  let best: Slot | null = null;
  let bestScore = Number.POSITIVE_INFINITY;
  for (const s of slots) {
    if (s.currentLeafId === leafId) return { slot: s, previousLeafId: null };
    const focused =
      s.currentLeafId !== null && (adapter?.isLeafFocused(s.currentLeafId) ?? false);
    const score =
      (isAltScreen(s) ? 100 : 0) + (focused ? 10 : 0) + s.lastUsedAt / 1e12;
    if (score < bestScore) {
      bestScore = score;
      best = s;
    }
  }
  const chosen = best!;
  return { slot: chosen, previousLeafId: chosen.currentLeafId };
}

export type AcquireParams = {
  leafId: number;
  sessionId: string;
  container: HTMLDivElement;
  cols: number;
  rows: number;
};

export function acquireSlot(params: AcquireParams): Slot {
  const existing = slots.find((s) => s.currentLeafId === params.leafId);
  if (existing) {
    // Same leaf, new PTY session (workspace switched / session restarted) →
    // full rebind. Same session → just re-home + re-fit (instant warm switch).
    if (existing.sessionId !== params.sessionId) bindSlot(existing, params);
    else rewireSlot(existing, params);
    return existing;
  }

  const pick = pickSlotFor(params.leafId);
  if (pick.previousLeafId !== null) {
    adapter?.evictLeaf(pick.previousLeafId);
  }
  if (
    pick.slot.currentLeafId !== null &&
    pick.slot.currentLeafId !== params.leafId
  ) {
    detachSlotFromLeaf(pick.slot);
  }
  bindSlot(pick.slot, params);
  return pick.slot;
}

function bindSlot(slot: Slot, p: AcquireParams): void {
  const stale = !slot.webglAddon || performance.now() - slot.lastUsedAt > SLOT_STALE_MS;
  // Stop streaming any session previously bound here before attaching the new
  // one (covers a same-leaf rebind and a recycled slot).
  detachChannel(slot);
  slot.currentLeafId = p.leafId;
  slot.lastUsedAt = performance.now();
  slot.sawFirstByte = false;

  cancelPendingUnhide(slot);
  slot.host.style.visibility = "hidden";

  if (slot.host.parentNode !== p.container) {
    p.container.appendChild(slot.host);
  }

  slot.term.clear();
  slot.term.reset();

  if (
    p.cols > 0 &&
    p.rows > 0 &&
    (slot.term.cols !== p.cols || slot.term.rows !== p.rows)
  ) {
    slot.term.resize(p.cols, p.rows);
  }

  // Attach the session's byte stream. The backend replays its per-session ring
  // (the full recent screen, incl. anything produced before this slot existed),
  // then streams live. A stale channel is guarded out by identity comparison.
  const ch = new Channel<ArrayBuffer>();
  slot.dataChannel = ch;
  slot.sessionId = p.sessionId;
  const sid = p.sessionId;
  ch.onmessage = (buf) => {
    if (slot.dataChannel !== ch || slot.currentLeafId !== p.leafId) return;
    slot.term.write(new Uint8Array(buf));
    if (!slot.sawFirstByte) {
      slot.sawFirstByte = true;
      adapter?.onFirstBytes(p.leafId);
      // Re-fit now that content is flowing — the first fit may have run before
      // the container settled.
      queueMicrotask(() => {
        if (slot.currentLeafId === p.leafId) slot.fitAddon.fit();
      });
    }
  };
  void invoke("terminal_attach", { sessionId: sid, onData: ch }).catch((e) => {
    console.warn("[tessera] terminal_attach failed", e);
  });

  setupResizeObserver(slot, p);
  try {
    slot.fitAddon.fit();
  } catch {}
  slot.lastCols = slot.term.cols;
  slot.lastRows = slot.term.rows;
  slot.lastW = p.container.clientWidth;
  slot.lastH = p.container.clientHeight;
  if (slot.lastCols !== p.cols || slot.lastRows !== p.rows) {
    resizePty(p.leafId, slot.lastCols, slot.lastRows);
  }

  scheduleUnhide(slot, stale);
}

function scheduleUnhide(slot: Slot, stale: boolean): void {
  slot.unhideRaf = requestAnimationFrame(() => {
    slot.unhideRaf = requestAnimationFrame(() => {
      slot.unhideRaf = null;
      slot.host.style.visibility = "";
      if (stale) {
        if (!slot.webglAddon) attachWebgl(slot);
        try {
          slot.term.refresh(0, slot.term.rows - 1);
        } catch {}
      }
      const leafId = slot.currentLeafId;
      if (leafId !== null && adapter?.isLeafFocused(leafId)) {
        slot.term.focus();
      }
    });
  });
}

function cancelPendingUnhide(slot: Slot): void {
  if (slot.unhideRaf !== null) {
    cancelAnimationFrame(slot.unhideRaf);
    slot.unhideRaf = null;
  }
}

function rewireSlot(slot: Slot, p: AcquireParams): void {
  slot.lastUsedAt = performance.now();
  if (slot.host.parentNode !== p.container) {
    p.container.appendChild(slot.host);
  }
  setupResizeObserver(slot, p);
  try {
    slot.fitAddon.fit();
  } catch {}
  slot.lastW = p.container.clientWidth;
  slot.lastH = p.container.clientHeight;
  if (slot.term.cols !== p.cols || slot.term.rows !== p.rows) {
    resizePty(p.leafId, slot.term.cols, slot.term.rows);
  }
  slot.lastCols = slot.term.cols;
  slot.lastRows = slot.term.rows;
}

function setupResizeObserver(slot: Slot, p: AcquireParams): void {
  slot.observer?.disconnect();
  if (slot.fitTimer) clearTimeout(slot.fitTimer);
  if (slot.ptyTimer) clearTimeout(slot.ptyTimer);
  slot.fitTimer = null;
  slot.ptyTimer = null;

  const container = p.container;
  const flushPty = () => {
    slot.ptyTimer = null;
    if (slot.currentLeafId !== p.leafId) return;
    if (slot.term.cols === slot.lastCols && slot.term.rows === slot.lastRows) return;
    slot.lastCols = slot.term.cols;
    slot.lastRows = slot.term.rows;
    resizePty(p.leafId, slot.lastCols, slot.lastRows);
  };

  slot.observer = new ResizeObserver(() => {
    if (slot.fitTimer) clearTimeout(slot.fitTimer);
    slot.fitTimer = setTimeout(() => {
      slot.fitTimer = null;
      if (slot.currentLeafId !== p.leafId) return;
      const w = container.clientWidth;
      const h = container.clientHeight;
      if (w === slot.lastW && h === slot.lastH) return;
      slot.lastW = w;
      slot.lastH = h;
      try {
        slot.fitAddon.fit();
      } catch {}
      if (slot.ptyTimer) clearTimeout(slot.ptyTimer);
      slot.ptyTimer = setTimeout(flushPty, PTY_RESIZE_DEBOUNCE_MS);
    }, FIT_DEBOUNCE_MS);
  });
  slot.observer.observe(container);
}

export function releaseSlot(leafId: number): void {
  const slot = slots.find((s) => s.currentLeafId === leafId);
  if (!slot) return;
  detachSlotFromLeaf(slot);
}

/** Stop streaming the slot's current PTY session to the frontend. The PTY keeps
 *  running and the backend keeps buffering its ring, so a later re-bind replays
 *  the full screen. */
function detachChannel(slot: Slot): void {
  if (slot.sessionId) {
    const sid = slot.sessionId;
    void invoke("terminal_detach", { sessionId: sid }).catch(() => {});
  }
  slot.sessionId = null;
  slot.dataChannel = null;
}

function detachSlotFromLeaf(slot: Slot): void {
  slot.observer?.disconnect();
  slot.observer = null;
  if (slot.fitTimer) clearTimeout(slot.fitTimer);
  if (slot.ptyTimer) clearTimeout(slot.ptyTimer);
  slot.fitTimer = null;
  slot.ptyTimer = null;

  cancelPendingUnhide(slot);
  slot.host.style.visibility = "";
  detachChannel(slot);

  if (slot.host.parentNode !== getRecycler()) {
    getRecycler().appendChild(slot.host);
  }

  slot.currentLeafId = null;
  slot.lastUsedAt = performance.now();
}

const WEBGL_RECOVERY_DELAY_MS = 250;
// Below this a re-shown slot is fresh enough to trust; above it, repaint on
// unhide to defeat silent GPU/context staleness.
const SLOT_STALE_MS = 10_000;

function attachWebgl(slot: Slot): void {
  if (slot.webglAddon || !slot.term.element) return;
  const elem = slot.term.element;
  const before = new Set<HTMLCanvasElement>(
    elem.querySelectorAll<HTMLCanvasElement>("canvas"),
  );
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      const cur = slot.webglAddon;
      if (cur === webgl) {
        slot.webglAddon = null;
        slot.webglCanvases = [];
      }
      try {
        webgl.dispose();
      } catch {}
      // Recovery: WebKit/Chromium may transiently lose contexts on sleep/wake
      // or GPU reset; without re-attach the slot would silently fall back to
      // the DOM renderer (or blank) forever. Defer past the reset window.
      setTimeout(() => {
        if (slot.webglAddon) return;
        attachWebgl(slot);
        if (slot.webglAddon) {
          try {
            slot.term.refresh(0, slot.term.rows - 1);
          } catch {}
        }
      }, WEBGL_RECOVERY_DELAY_MS);
    });
    slot.term.loadAddon(webgl);
    const after = elem.querySelectorAll<HTMLCanvasElement>("canvas");
    const added: HTMLCanvasElement[] = [];
    for (const c of after) if (!before.has(c)) added.push(c);
    slot.webglAddon = webgl;
    slot.webglCanvases = added;
  } catch (e) {
    // WebGL unavailable — xterm falls back to its DOM renderer automatically.
    console.warn("[tessera] webgl renderer unavailable:", e);
  }
}

function releaseCanvasContext(canvas: HTMLCanvasElement): void {
  let gl: WebGL2RenderingContext | WebGLRenderingContext | null = null;
  try {
    gl = canvas.getContext("webgl2") as WebGL2RenderingContext | null;
  } catch {}
  if (!gl) {
    try {
      gl = canvas.getContext("webgl") as WebGLRenderingContext | null;
    } catch {}
  }
  if (gl) {
    try {
      const ext = gl.getExtension("WEBGL_lose_context");
      if (ext && !gl.isContextLost()) ext.loseContext();
    } catch {}
  }
  try {
    canvas.width = 0;
    canvas.height = 0;
  } catch {}
}

function disposeSlotWebgl(slot: Slot): void {
  if (!slot.webglAddon) return;
  const addon = slot.webglAddon;
  for (const canvas of slot.webglCanvases) releaseCanvasContext(canvas);
  slot.webglCanvases = [];
  try {
    addon.dispose();
  } catch (e) {
    console.warn("[tessera] webgl dispose failed:", e);
  }
  slot.webglAddon = null;
}

export function applyFontSize(size: number): void {
  const px = clampFont(size);
  for (const slot of slots) {
    if (slot.term.options.fontSize === px) continue;
    slot.term.options.fontSize = px;
    try {
      slot.fitAddon.fit();
    } catch {}
    if (slot.currentLeafId !== null) {
      slot.lastCols = slot.term.cols;
      slot.lastRows = slot.term.rows;
      resizePty(slot.currentLeafId, slot.term.cols, slot.term.rows);
    }
  }
}

export function applyFontFamily(family: string): void {
  const resolved = family || '"Geist Mono", monospace';
  for (const slot of slots) {
    if (slot.term.options.fontFamily === resolved) continue;
    slot.term.options.fontFamily = resolved;
    try {
      slot.fitAddon.fit();
    } catch {}
    if (slot.currentLeafId !== null) {
      slot.lastCols = slot.term.cols;
      slot.lastRows = slot.term.rows;
      resizePty(slot.currentLeafId, slot.term.cols, slot.term.rows);
    }
  }
}

export function applyScrollback(value: number): void {
  for (const slot of slots) {
    if (slot.term.options.scrollback === value) continue;
    slot.term.options.scrollback = value;
  }
}

export function applyCursor(): void {
  const blink = settings().terminal.cursor_blink;
  const style = cursorStyle();
  for (const slot of slots) {
    slot.term.options.cursorBlink = blink;
    slot.term.options.cursorStyle = style;
  }
}

export function applyTheme(): void {
  const theme = buildTerminalTheme();
  for (const slot of slots) {
    slot.term.options.theme = theme;
  }
}

export function focusSlot(leafId: number): void {
  const slot = slots.find((s) => s.currentLeafId === leafId);
  slot?.term.focus();
}

export function getSlotForLeaf(leafId: number): Slot | null {
  return slots.find((s) => s.currentLeafId === leafId) ?? null;
}

export function searchAddonFor(leafId: number): SearchAddon | null {
  return getSlotForLeaf(leafId)?.searchAddon ?? null;
}

/** Tear everything down (app teardown / HMR). */
export function disposeAllSlots(): void {
  for (const slot of slots) {
    disposeSlotWebgl(slot);
    try {
      slot.term.dispose();
    } catch {}
  }
  slots.length = 0;
}

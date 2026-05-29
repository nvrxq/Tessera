// Terminal session registry — the small shared layer between Solid components
// and the renderer pool. Tracks each leaf's backend PTY session uuid + focus
// state, implements the pool's adapter, and exposes show/hide/focus/write.
//
// Ported in spirit from Terax's useTerminalSession (github.com/crynta/terax-ai,
// Apache-2.0 — see NOTICE), reduced to Tessera's transport: the backend owns the
// replay ring (via terminal_attach/detach inside the pool), so there's no
// frontend dormant-ring or serialize-snapshot here.

import { invoke } from "@tauri-apps/api/core";
import {
  acquireSlot,
  configureRendererPool,
  focusSlot,
  getSlotForLeaf,
  releaseSlot,
} from "./rendererPool";

type Leaf = {
  leafId: number;
  sessionId: string | null;
  focused: boolean;
};

const leaves = new Map<number, Leaf>();
const firstBytesCbs = new Map<number, () => void>();
const TEXT_ENCODER = new TextEncoder();

configureRendererPool({
  sessionIdFor: (id) => leaves.get(id)?.sessionId ?? null,
  isLeafFocused: (id) => leaves.get(id)?.focused ?? false,
  // A slot was recycled away from this leaf under pool pressure. The pool has
  // already detached the backend channel; nothing else to do — the leaf's
  // component re-acquires (replaying the ring) next time it's shown.
  evictLeaf: () => {},
  onFirstBytes: (id) => firstBytesCbs.get(id)?.(),
});

let nextLeafId = 1;
export function newLeafId(): number {
  return nextLeafId++;
}

function ensureLeaf(leafId: number): Leaf {
  let l = leaves.get(leafId);
  if (!l) {
    l = { leafId, sessionId: null, focused: false };
    leaves.set(leafId, l);
  }
  return l;
}

export function leafSessionId(leafId: number): string | null {
  return leaves.get(leafId)?.sessionId ?? null;
}

export type ShowLeafParams = {
  leafId: number;
  sessionId: string;
  container: HTMLDivElement;
  cols: number;
  rows: number;
  focused?: boolean;
  onFirstBytes?: () => void;
};

/**
 * Show a leaf's PTY session in `container` via a pooled xterm slot. Returns
 * true when the slot already had content (a warm re-show — the caller should
 * treat it as ready immediately, since `onFirstBytes` will not fire again).
 */
export function showLeaf(p: ShowLeafParams): boolean {
  const l = ensureLeaf(p.leafId);
  l.sessionId = p.sessionId;
  l.focused = p.focused ?? true;
  if (p.onFirstBytes) firstBytesCbs.set(p.leafId, p.onFirstBytes);
  else firstBytesCbs.delete(p.leafId);
  const slot = acquireSlot({
    leafId: p.leafId,
    sessionId: p.sessionId,
    container: p.container,
    cols: p.cols,
    rows: p.rows,
  });
  if (l.focused) focusSlot(p.leafId);
  return slot.sawFirstByte;
}

export function hideLeaf(leafId: number): void {
  releaseSlot(leafId);
  firstBytesCbs.delete(leafId);
  const l = leaves.get(leafId);
  if (l) l.sessionId = null;
}

export function disposeLeaf(leafId: number): void {
  hideLeaf(leafId);
  leaves.delete(leafId);
}

export function setLeafFocused(leafId: number, focused: boolean): void {
  const l = leaves.get(leafId);
  if (l) l.focused = focused;
}

export function focusLeaf(leafId: number): void {
  setLeafFocused(leafId, true);
  focusSlot(leafId);
}

/** Current xterm cols/rows for sizing a spawn, or sensible defaults. */
export function leafDims(leafId: number): { cols: number; rows: number } {
  const slot = getSlotForLeaf(leafId);
  return { cols: slot?.term.cols ?? 80, rows: slot?.term.rows ?? 24 };
}

/** Write a raw string to a leaf's PTY (base64 over `pty_write`). */
export function writeLeaf(leafId: number, data: string): void {
  const sid = leaves.get(leafId)?.sessionId;
  if (!sid) return;
  const bytes = TEXT_ENCODER.encode(data);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  void invoke("pty_write", { sessionId: sid, dataB64: btoa(bin) }).catch(() => {});
}

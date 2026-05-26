import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  readText as clipReadText,
  writeText as clipWriteText,
} from "@tauri-apps/plugin-clipboard-manager";
import { spawnAgent } from "./lib/workspaces";

export interface TerminalProps {
  workspaceId: string;
  sessionId: string | null;
  onSpawned: (sessionId: string) => void;
}

interface WireCell {
  c: string;
  f: number;
  b: number;
  a: number;
}

interface Snapshot {
  session_id: string;
  cols: number;
  rows: number;
  /** true → `cells` is the full grid in row-major order (cols·rows entries).
   *  false → `cells[i]` lives at linear index `positions[i]` in the grid. */
  full: boolean;
  cells: WireCell[];
  positions: number[];
  cursor_col: number;
  cursor_row: number;
  cursor_visible: boolean;
  cursor_shape: "block" | "bar" | "underline";
}

const FONT_FAMILY =
  '"JetBrains Mono", "Geist Mono", "Fira Code", ui-monospace, Menlo, monospace';
const DEFAULT_FONT_PX = 14;
const MIN_FONT_PX = 8;
const MAX_FONT_PX = 32;
const DEFAULT_BG = 0x0f0f10;
const DEFAULT_FG_HEX = "#E8E8E6";

const FONT_PX_STORAGE_KEY = "tessera.fontPx";
const loadFontPx = (): number => {
  const raw = localStorage.getItem(FONT_PX_STORAGE_KEY);
  const n = raw == null ? DEFAULT_FONT_PX : Number(raw);
  if (!Number.isFinite(n)) return DEFAULT_FONT_PX;
  return Math.min(MAX_FONT_PX, Math.max(MIN_FONT_PX, n));
};

export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  let unlisten: UnlistenFn | null = null;
  let activeSessionId: string | null = props.sessionId;

  /** Swap the active session and drop any snapshots still queued for the
   *  previous one. Without this, a delta sitting in `pendingSnaps` from
   *  the old session could be drained in the next rAF and applied (or
   *  worse, used as `startIdx` reference) against the new session's
   *  freshly-baselined grid. The inner session_id guard at apply-time
   *  catches mismatches, but it does NOT prevent stale items from
   *  influencing the "find latest full" scan. Cleanest fix: clear on
   *  switch and gate every write to `activeSessionId` through here. */
  const setActiveSession = (sid: string | null) => {
    if (sid === activeSessionId) return;
    activeSessionId = sid;
    pendingSnaps.length = 0;
    // Selection is per-grid; the new session has its own dimensions and
    // content, so any leftover highlight from the previous session would
    // be visually nonsensical. Reset without repainting — the upcoming
    // `full` snapshot from the new session will repaint shortly.
    selStart = null;
    selEnd = null;
    selDragging = false;
  };

  // Local grid mirror — flat row-major Array(cols*rows). Updated on every
  // snapshot (full → overwrite, delta → patch). Paints read from here so
  // partial-redraw paths don't have to touch the wire payload at all.
  let grid: WireCell[] = [];
  let gridCols = 0;
  let gridRows = 0;
  let lastCursorCol = -1;
  let lastCursorRow = -1;
  let lastCursorVisible = false;
  let lastCursorShape: Snapshot["cursor_shape"] = "block";

  /** Cached 2D context — `canvas.getContext('2d')` is cheap (browsers
   *  cache it) but still a per-call property lookup; we hit it
   *  per delta cell and per cursor-erase, so we keep one reference. */
  let ctx2d: CanvasRenderingContext2D | null = null;
  const ctx2dOf = (): CanvasRenderingContext2D | null => {
    if (!ctx2d) ctx2d = canvas.getContext("2d");
    return ctx2d;
  };

  let fontPx = loadFontPx();
  let cellW = 0;
  let cellH = 0;
  let baseline = 0;
  let lastKeydownAt = 0;
  const spawning = new Set<string>();

  // ── Text selection on the canvas ──
  // Canvas2D has no DOM text, so we maintain our own grid selection.
  // selStart/selEnd are in grid coords (col, row). Pointer down → start
  // a fresh selection; pointer move (while dragging) → extend end; pointer
  // up with start==end clears (treat as a plain click).
  type CellPos = { col: number; row: number };
  let selStart: CellPos | null = null;
  let selEnd: CellPos | null = null;
  let selDragging = false;
  // Rate-limit selection-driven repaints to one per frame.
  let selRepaintQueued = false;

  function selectionRange(): { a: CellPos; b: CellPos } | null {
    if (!selStart || !selEnd) return null;
    const aFirst =
      selStart.row < selEnd.row ||
      (selStart.row === selEnd.row && selStart.col <= selEnd.col);
    return aFirst
      ? { a: selStart, b: selEnd }
      : { a: selEnd, b: selStart };
  }

  function selectionEmpty(r: { a: CellPos; b: CellPos } | null): boolean {
    return !r || (r.a.col === r.b.col && r.a.row === r.b.row);
  }

  function selectionToText(): string {
    const r = selectionRange();
    if (!r || gridCols === 0) return "";
    const { a, b } = r;
    const lines: string[] = [];
    for (let row = a.row; row <= b.row; row++) {
      const startCol = row === a.row ? a.col : 0;
      const endCol = row === b.row ? b.col : gridCols - 1;
      let s = "";
      for (let col = startCol; col <= endCol; col++) {
        const cell = grid[row * gridCols + col];
        s += cell ? cell.c : " ";
      }
      lines.push(s.replace(/\s+$/, ""));
    }
    return lines.join("\n");
  }

  /** Mouse coord → grid cell. Coordinates come in CSS pixels; cellW/cellH
   *  are in canvas backing-store pixels (multiplied by dpr), so we divide
   *  back out. Clamped to grid bounds. */
  function cellAtClient(clientX: number, clientY: number): CellPos | null {
    if (cellW === 0 || cellH === 0 || gridCols === 0 || gridRows === 0) {
      return null;
    }
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const cssW = cellW / dpr;
    const cssH = cellH / dpr;
    const col = Math.floor((clientX - rect.left) / cssW);
    const row = Math.floor((clientY - rect.top) / cssH);
    return {
      col: Math.max(0, Math.min(gridCols - 1, col)),
      row: Math.max(0, Math.min(gridRows - 1, row)),
    };
  }

  /** Translucent blue overlay over the selected cells. Painted last so it
   *  sits on top of glyphs and cursor; low alpha keeps text readable.
   *  Clamps to current grid dims so a stale selection after a shrink
   *  doesn't paint outside the canvas. */
  function paintSelectionOverlay(ctx: CanvasRenderingContext2D) {
    const r = selectionRange();
    if (!r || gridCols === 0 || gridRows === 0) return;
    ctx.fillStyle = "rgba(110, 160, 220, 0.32)";
    const startRow = Math.max(0, Math.min(gridRows - 1, r.a.row));
    const endRow = Math.max(0, Math.min(gridRows - 1, r.b.row));
    if (endRow < startRow) return;
    for (let row = startRow; row <= endRow; row++) {
      let startCol = row === r.a.row ? r.a.col : 0;
      let endCol = row === r.b.row ? r.b.col : gridCols - 1;
      startCol = Math.max(0, Math.min(gridCols - 1, startCol));
      endCol = Math.max(0, Math.min(gridCols - 1, endCol));
      if (endCol < startCol) continue;
      const x = startCol * cellW;
      const y = row * cellH;
      const w = (endCol - startCol + 1) * cellW;
      ctx.fillRect(x, y, w, cellH);
    }
  }

  /** Full repaint from the local grid mirror + cursor + selection. Used by
   *  pointermove during a drag — cheap (≤ ~12000 cells at 60Hz). */
  function repaintFromGrid() {
    const ctx = ctx2dOf();
    if (!ctx || gridCols === 0) return;
    const px = fontPx * (window.devicePixelRatio || 1);
    paintFull(px);
    if (lastCursorVisible) {
      paintCursor(ctx, lastCursorCol, lastCursorRow, lastCursorShape, px);
    }
    paintSelectionOverlay(ctx);
  }

  function scheduleSelectionRepaint() {
    if (selRepaintQueued) return;
    selRepaintQueued = true;
    requestAnimationFrame(() => {
      selRepaintQueued = false;
      repaintFromGrid();
    });
  }

  function clearSelection() {
    if (!selStart && !selEnd) return;
    selStart = null;
    selEnd = null;
    selDragging = false;
    repaintFromGrid();
  }

  /** Three-stage load state for the loading overlay:
   *  - "spawning":   waiting on `workspace_spawn_agent` to return a session id
   *  - "connecting": session id known, waiting for the first `term_snapshot`
   *  - "ready":      first snapshot painted — overlay fades out (240ms)
   * Covers the canvas (z-index 2) so we don't briefly flash an empty grid
   * while claude is cold-starting. */
  const [phase, setPhase] = createSignal<"spawning" | "connecting" | "ready">(
    props.sessionId ? "connecting" : "spawning",
  );

  function measureCell(dpr: number) {
    const ctx = ctx2dOf();
    if (!ctx) return;
    const px = fontPx * dpr;
    ctx.font = `${px}px ${FONT_FAMILY}`;
    const m = ctx.measureText("M");
    cellW = Math.max(1, Math.round(m.width));
    const ascent =
      typeof m.actualBoundingBoxAscent === "number"
        ? m.actualBoundingBoxAscent
        : px * 0.8;
    const descent =
      typeof m.actualBoundingBoxDescent === "number"
        ? m.actualBoundingBoxDescent
        : px * 0.2;
    cellH = Math.max(1, Math.ceil((ascent + descent) * 1.25));
    baseline = Math.floor(cellH - descent - (cellH - ascent - descent) * 0.5);
  }

  function gridFromCanvas(): { cols: number; rows: number } {
    const r = host.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(r.width * dpr));
    const h = Math.max(1, Math.round(r.height * dpr));
    measureCell(dpr);
    const cols = Math.max(20, Math.floor(w / cellW));
    const rows = Math.max(5, Math.floor(h / cellH));
    return { cols, rows };
  }

  function resizeCanvasBacking() {
    const r = host.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const newW = Math.max(1, Math.round(r.width * dpr));
    const newH = Math.max(1, Math.round(r.height * dpr));
    // Assigning to canvas.width/height ALWAYS clears the canvas — even
    // when the value is unchanged. That was the root cause of the flicker:
    // every ResizeObserver tick (which fires on sub-pixel layout shifts —
    // scrollbar appearance, focus rings, anim frames) would wipe the
    // grid and we'd briefly see black until the next snapshot painted.
    // Skip the assignment when the backing store already matches.
    const dimsChanged = canvas.width !== newW || canvas.height !== newH;
    if (dimsChanged) {
      canvas.width = newW;
      canvas.height = newH;
    }
    canvas.style.width = `${r.width}px`;
    canvas.style.height = `${r.height}px`;
    // If we DID resize, the canvas is now blank. Repaint immediately
    // from the local grid mirror so there's no black-flash window
    // between the resize and the next `term_snapshot` arrival.
    if (dimsChanged && grid.length > 0) {
      paintFull(fontPx * dpr);
    }
  }

  function hexColor(rgb: number): string {
    return "#" + rgb.toString(16).padStart(6, "0");
  }

  /** Repaint one cell — clear its background to the cell.b colour, then
   *  draw the glyph + decorations. Used by delta paint, and as the
   *  primitive that the full-paint loop falls back to in slow paths. */
  function paintCell(ctx: CanvasRenderingContext2D, idx: number, px: number) {
    const cell = grid[idx];
    if (!cell) return;
    const r = Math.floor(idx / gridCols);
    const c = idx - r * gridCols;
    const x = c * cellW;
    const y = r * cellH;

    ctx.fillStyle = hexColor(cell.b);
    ctx.fillRect(x, y, cellW, cellH);

    if (cell.c !== " " || (cell.a & 28) !== 0) {
      const bold = (cell.a & 1) !== 0;
      const italic = (cell.a & 2) !== 0;
      ctx.font =
        (bold ? "bold " : "") +
        (italic ? "italic " : "") +
        `${px}px ${FONT_FAMILY}`;
      ctx.fillStyle = hexColor(cell.f);
      ctx.textBaseline = "alphabetic";
      ctx.fillText(cell.c, x, y + baseline);

      const thickness = Math.max(1, Math.round(px * 0.06));
      if (cell.a & 4) {
        ctx.fillRect(x, y + cellH - thickness, cellW, thickness);
      } else if (cell.a & 8) {
        ctx.fillRect(x, y + cellH - thickness * 3, cellW, thickness);
        ctx.fillRect(x, y + cellH - thickness, cellW, thickness);
      }
      if (cell.a & 16) {
        ctx.fillRect(x, y + Math.round(cellH / 2), cellW, thickness);
      }
    }
  }

  function paintCursor(
    ctx: CanvasRenderingContext2D,
    col: number,
    row: number,
    shape: Snapshot["cursor_shape"],
    px: number,
  ) {
    if (col < 0 || row < 0) return;
    const x = col * cellW;
    const y = row * cellH;
    const thick = Math.max(1, Math.round(px * 0.1));
    ctx.fillStyle = DEFAULT_FG_HEX;
    if (shape === "block") {
      ctx.globalAlpha = 0.6;
      ctx.fillRect(x, y, cellW, cellH);
      ctx.globalAlpha = 1.0;
    } else if (shape === "underline") {
      ctx.fillRect(x, y + cellH - thick, cellW, thick);
    } else {
      ctx.fillRect(x, y, thick, cellH);
    }
  }

  function paintFull(px: number) {
    const ctx = ctx2dOf();
    if (!ctx) return;
    ctx.fillStyle = "#0F0F10";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.textBaseline = "alphabetic";
    ctx.font = `${px}px ${FONT_FAMILY}`;

    // Pass 1: backgrounds — batch contiguous non-default runs per row.
    for (let r = 0; r < gridRows; r++) {
      const yTop = r * cellH;
      let runColor = -1;
      let runStart = 0;
      for (let c = 0; c < gridCols; c++) {
        const cell = grid[r * gridCols + c];
        const bg = cell.b !== DEFAULT_BG ? cell.b : -1;
        if (bg !== runColor) {
          if (runColor >= 0) {
            ctx.fillStyle = hexColor(runColor);
            ctx.fillRect(runStart * cellW, yTop, (c - runStart) * cellW, cellH);
          }
          runColor = bg;
          runStart = c;
        }
      }
      if (runColor >= 0) {
        ctx.fillStyle = hexColor(runColor);
        ctx.fillRect(
          runStart * cellW,
          yTop,
          (gridCols - runStart) * cellW,
          cellH,
        );
      }
    }

    // Pass 2: glyphs batched per (fg + font) run.
    const thickness = Math.max(1, Math.round(px * 0.06));
    const decoFills: Array<[number, number, number, number, string]> = [];
    let prevFont = "";
    let prevFill = "";
    for (let r = 0; r < gridRows; r++) {
      const yBase = r * cellH + baseline;
      let runStart = -1;
      let runChars = "";
      let runFg = -1;
      let runFontKey = "";
      const flushRun = () => {
        if (runStart < 0 || runChars.length === 0) return;
        const fill = hexColor(runFg);
        if (runFontKey !== prevFont) {
          ctx.font = runFontKey;
          prevFont = runFontKey;
        }
        if (fill !== prevFill) {
          ctx.fillStyle = fill;
          prevFill = fill;
        }
        ctx.fillText(runChars, runStart * cellW, yBase);
      };
      for (let c = 0; c < gridCols; c++) {
        const cell = grid[r * gridCols + c];
        const bold = (cell.a & 1) !== 0;
        const italic = (cell.a & 2) !== 0;
        const fontKey =
          (bold ? "bold " : "") +
          (italic ? "italic " : "") +
          `${px}px ${FONT_FAMILY}`;
        const isBlank = cell.c === " " && (cell.a & 28) === 0;
        if (
          !isBlank &&
          runStart >= 0 &&
          runFg === cell.f &&
          runFontKey === fontKey
        ) {
          runChars += cell.c;
        } else {
          flushRun();
          if (isBlank) {
            runStart = -1;
            runChars = "";
          } else {
            runStart = c;
            runChars = cell.c;
            runFg = cell.f;
            runFontKey = fontKey;
          }
        }
        if (cell.a & 4) {
          decoFills.push([
            c * cellW,
            (r + 1) * cellH - thickness,
            cellW,
            thickness,
            hexColor(cell.f),
          ]);
        } else if (cell.a & 8) {
          decoFills.push([
            c * cellW,
            (r + 1) * cellH - thickness * 3,
            cellW,
            thickness,
            hexColor(cell.f),
          ]);
          decoFills.push([
            c * cellW,
            (r + 1) * cellH - thickness,
            cellW,
            thickness,
            hexColor(cell.f),
          ]);
        }
        if (cell.a & 16) {
          decoFills.push([
            c * cellW,
            r * cellH + Math.round(cellH / 2),
            cellW,
            thickness,
            hexColor(cell.f),
          ]);
        }
      }
      flushRun();
    }
    decoFills.sort((a, b) => a[4].localeCompare(b[4]));
    for (const [x, y, w, h, col] of decoFills) {
      if (col !== prevFill) {
        ctx.fillStyle = col;
        prevFill = col;
      }
      ctx.fillRect(x, y, w, h);
    }
    paintSelectionOverlay(ctx);
  }

  function applySnapshot(snap: Snapshot) {
    // Defensive: snapshots can land before `onMount` finishes resizing
    // the canvas backing store (listener registers synchronously; the
    // first `term_snapshot` from a pre-spawn can arrive in that gap).
    // Painting into a 0×0 canvas silently no-ops — guarantee the
    // backing store is sized before we draw anything.
    if (canvas.width === 0 || canvas.height === 0) {
      resizeCanvasBacking();
      measureCell(window.devicePixelRatio || 1);
    }
    const px = fontPx * (window.devicePixelRatio || 1);
    const sizeChanged = snap.cols !== gridCols || snap.rows !== gridRows;
    if (snap.full || sizeChanged) {
      gridCols = snap.cols;
      gridRows = snap.rows;
      grid = snap.cells.slice();
      // grid may be shorter than cols*rows on first paint — pad blanks.
      while (grid.length < gridCols * gridRows) {
        grid.push({ c: " ", f: 0xe8e8e6, b: DEFAULT_BG, a: 0 });
      }
      paintFull(px);
    } else {
      // Delta: patch changed cells and repaint just those.
      const ctx = ctx2dOf();
      if (!ctx) return;
      for (let i = 0; i < snap.positions.length; i++) {
        const idx = snap.positions[i];
        grid[idx] = snap.cells[i];
        paintCell(ctx, idx, px);
      }
    }

    // Cursor: erase old, draw new. Each cursor cell is repainted via
    // paintCell to restore its underlying glyph + bg before deciding to
    // re-overlay the cursor on top.
    const ctx = ctx2dOf();
    if (!ctx) return;
    if (
      lastCursorVisible &&
      (lastCursorCol !== snap.cursor_col || lastCursorRow !== snap.cursor_row)
    ) {
      const oldIdx = lastCursorRow * gridCols + lastCursorCol;
      if (oldIdx >= 0 && oldIdx < grid.length) {
        paintCell(ctx, oldIdx, px);
      }
    }
    if (snap.cursor_visible) {
      paintCursor(
        ctx,
        snap.cursor_col,
        snap.cursor_row,
        snap.cursor_shape,
        px,
      );
    }
    lastCursorCol = snap.cursor_col;
    lastCursorRow = snap.cursor_row;
    lastCursorVisible = snap.cursor_visible;
    lastCursorShape = snap.cursor_shape;

    // A delta path may have overpainted cells that the user has selected;
    // re-stamp the translucent overlay on top so the highlight survives.
    // (paintFull above already includes selection internally.)
    if (!snap.full && (snap.positions?.length ?? 0) > 0) {
      paintSelectionOverlay(ctx);
    }

    // First snapshot for this workspace landed — fade out the loading
    // overlay. Subsequent snapshots are no-ops here.
    if (phase() !== "ready") setPhase("ready");
  }

  async function syncGrid() {
    const sid = activeSessionId;
    if (!sid) return;
    const { cols, rows } = gridFromCanvas();
    resizeCanvasBacking();
    try {
      await invoke("terminal_resize", { sessionId: sid, cols, rows });
      await invoke("pty_resize", { sessionId: sid, cols, rows });
    } catch {
      /* session may have exited */
    }
    // Backend will send a `full` snapshot on next data after resize. Until
    // then we keep showing the stale-but-resized canvas.
  }

  // Register the snapshot listener IMMEDIATELY (synchronous component
  // setup). If we deferred this to onMount, the spawn-on-mount createEffect
  // could fire `spawnAgent` before the listener is wired up — the very
  // first `term_snapshot` event (with `full: true`, containing claude's
  // welcome screen) would then be dropped on the floor and the canvas
  // would stay black until something forced another emit. `listenerReady`
  // gates spawning so the first snapshot always lands.
  // Snapshot queue: when claude streams output, the backend can emit
  // multiple snapshots before our rAF callback fires. The previous
  // single-slot design dropped intermediate deltas — `pendingSnap = snap`
  // overwrote unapplied deltas, causing the local grid to drift out of
  // sync with backend state (visible as cell glitches / flicker).
  //
  // The queue keeps every snapshot in arrival order. In the rAF callback
  // we scan from the end for the latest `full` snapshot — everything
  // before it is necessarily stale (a full subsumes prior deltas) — and
  // apply from there onward. Worst case per frame: 1 full + N deltas;
  // typical case for typing-echo: 1 delta.
  const pendingSnaps: Snapshot[] = [];
  let rafQueued = false;
  const bench = (window as unknown as { __TESSERA_BENCH?: boolean })
    .__TESSERA_BENCH === true;
  // Track the listen promise itself, not just the resolved unlisten —
  // if the component unmounts before `.then` runs (fast workspace
  // switching, hot module reload), we still need a handle to call
  // unlisten on later. See onCleanup below.
  const listenerPromise = listen<Snapshot>("term_snapshot", (event) => {
    const tRecv = performance.now();
    const snap = event.payload;
    if (snap.session_id !== activeSessionId) return;
    pendingSnaps.push(snap);
    if (rafQueued) return;
    rafQueued = true;
    const tQueued = performance.now();
    requestAnimationFrame(() => {
      rafQueued = false;
      const tRafFired = performance.now();
      // Drain the queue into a local slice so further events while we
      // paint go into the next frame.
      const queue = pendingSnaps.splice(0);
      // Skip stale snapshots that precede the latest `full` — a full
      // re-baselines the entire grid so anything before it is wasted
      // paint work and (more importantly) would be applied to a grid
      // sized for the new dims.
      let startIdx = 0;
      for (let i = queue.length - 1; i >= 0; i--) {
        if (queue[i].full) {
          startIdx = i;
          break;
        }
      }
      let painted = 0;
      let cellsTotal = 0;
      let sawFull = false;
      for (let i = startIdx; i < queue.length; i++) {
        const s = queue[i];
        if (s.session_id !== activeSessionId) continue;
        applySnapshot(s);
        painted += 1;
        cellsTotal += s.cells.length;
        if (s.full) sawFull = true;
      }
      if (bench && painted > 0) {
        const tPainted = performance.now();
        const keyAge = lastKeydownAt > 0 ? tRecv - lastKeydownAt : -1;
        console.log(
          `[bench JS] keydown→recv=${keyAge.toFixed(1)}ms ` +
            `recv→raf=${(tRafFired - tQueued).toFixed(1)}ms ` +
            `paint=${(tPainted - tRafFired).toFixed(1)}ms ` +
            `snaps=${painted}(${sawFull ? "+full" : "delta"}) ` +
            `cells=${cellsTotal} ` +
            `total=${(tPainted - lastKeydownAt).toFixed(1)}ms`,
        );
      }
    });
  });
  const listenerReady: Promise<void> = listenerPromise.then((u) => {
    unlisten = u;
  });

  onMount(async () => {
    resizeCanvasBacking();
    measureCell(window.devicePixelRatio || 1);
    await listenerReady;

    const ro = new ResizeObserver(() => {
      void syncGrid();
    });
    ro.observe(host);
    onCleanup(() => ro.disconnect());

    function onKey(ev: KeyboardEvent) {
      const sid = activeSessionId;
      if (!sid) return;
      lastKeydownAt = performance.now();
      if (
        (ev.ctrlKey || ev.metaKey) &&
        (ev.key === "r" || ev.key === "R" || ev.key === "F5")
      ) {
        return;
      }
      if ((ev.ctrlKey || ev.metaKey) && !ev.altKey) {
        if (ev.key === "=" || ev.key === "+") {
          ev.preventDefault();
          fontPx = Math.min(MAX_FONT_PX, fontPx + 1);
          localStorage.setItem(FONT_PX_STORAGE_KEY, String(fontPx));
          void syncGrid();
          return;
        }
        if (ev.key === "-" || ev.key === "_") {
          ev.preventDefault();
          fontPx = Math.max(MIN_FONT_PX, fontPx - 1);
          localStorage.setItem(FONT_PX_STORAGE_KEY, String(fontPx));
          void syncGrid();
          return;
        }
        if (ev.key === "0") {
          ev.preventDefault();
          fontPx = DEFAULT_FONT_PX;
          localStorage.setItem(FONT_PX_STORAGE_KEY, String(fontPx));
          void syncGrid();
          return;
        }
        // Copy / paste. macOS: Cmd+C/V. Linux convention: Ctrl+Shift+C/V
        // (bare Ctrl+C must still pass through as SIGINT to the agent).
        const isCopyPasteMod =
          ev.metaKey || (ev.ctrlKey && ev.shiftKey);
        if (isCopyPasteMod) {
          const k = ev.key.toLowerCase();
          if (k === "c") {
            const range = selectionRange();
            if (range && !selectionEmpty(range)) {
              const text = selectionToText();
              if (text) {
                ev.preventDefault();
                void clipWriteText(text).catch((e) =>
                  console.warn("clipboard write failed", e),
                );
                return;
              }
            }
            // No selection — let the event fall through so Ctrl+C
            // (no shift, no meta) can still reach encodeKey → PTY SIGINT.
            // With meta or ctrl+shift held we already know it's not SIGINT
            // intent; swallow it silently.
            if (ev.metaKey || ev.shiftKey) return;
          }
          if (k === "v") {
            ev.preventDefault();
            void (async () => {
              try {
                const text = await clipReadText();
                if (!text) return;
                const enc = new TextEncoder().encode(text);
                let bin = "";
                for (const byte of enc) bin += String.fromCharCode(byte);
                await invoke("pty_write", {
                  sessionId: sid,
                  dataB64: btoa(bin),
                });
              } catch (e) {
                console.warn("paste failed", e);
              }
            })();
            return;
          }
        }
      }
      const bytes = encodeKey(ev);
      if (bytes.length === 0) return;
      ev.preventDefault();
      let bin = "";
      for (const b of bytes) bin += String.fromCharCode(b);
      void invoke("pty_write", { sessionId: sid, dataB64: btoa(bin) });
    }
    document.addEventListener("keydown", onKey);
    onCleanup(() => document.removeEventListener("keydown", onKey));

    host.focus();
  });

  createEffect(() => {
    const ws = props.workspaceId;
    const sid = props.sessionId;
    // Workspace changed — assume we need to reload until a snapshot
    // arrives. Skip the reset if the same session is already ready.
    if (phase() === "ready" && sid !== activeSessionId) {
      setPhase(sid ? "connecting" : "spawning");
    }
    if (sid) {
      setActiveSession(sid);
      if (phase() === "spawning") setPhase("connecting");
      void syncGrid();
      return;
    }
    if (spawning.has(ws)) return;
    spawning.add(ws);
    void (async () => {
      try {
        // Wait for the snapshot listener to be live BEFORE asking the
        // backend to spawn claude — otherwise the very first emit can
        // arrive in the dead window between createEffect firing and
        // listen() resolving, and we'd lose the initial `full` payload.
        await listenerReady;
        const { cols, rows } = gridFromCanvas();
        const newSid = await spawnAgent(ws, cols, rows);
        // Guard against a stale spawn finishing AFTER the user already
        // switched workspaces. Without this check we would:
        //   1. Bind `newSid` to the now-current workspace's terminal,
        //      breaking the previous workspace's session mapping.
        //   2. Fire `onSpawned(newSid)` which tells App to set the
        //      session_id on the CURRENT workspace, not the one we
        //      actually spawned for — silently corrupting state.
        // Both `activeSessionId` adoption AND the parent callback must
        // be gated by the workspace-still-matches invariant.
        if (props.workspaceId === ws) {
          setActiveSession(newSid);
          setPhase("connecting");
          await invoke("terminal_resize", { sessionId: newSid, cols, rows });
          props.onSpawned(newSid);
        }
      } finally {
        spawning.delete(ws);
      }
    })();
  });

  onCleanup(() => {
    if (unlisten) {
      unlisten();
    } else {
      // Listener not registered yet — chain the cleanup so we never
      // leak a Tauri event subscription if the component unmounts
      // inside the registration microtask. (Workspace flash-switch
      // could otherwise pile up live listeners across reloads.)
      void listenerPromise.then((u) => u()).catch(() => {});
    }
  });

  // ── Pointer-driven text selection ──
  const onPointerDown = (ev: PointerEvent) => {
    if (ev.button !== 0) return; // left button only
    const cell = cellAtClient(ev.clientX, ev.clientY);
    if (!cell) return;
    selStart = cell;
    selEnd = cell;
    selDragging = true;
    try {
      canvas.setPointerCapture(ev.pointerId);
    } catch {
      /* setPointerCapture can throw if the pointer id is already lost; harmless */
    }
    ev.preventDefault();
    scheduleSelectionRepaint();
  };
  const onPointerMove = (ev: PointerEvent) => {
    if (!selDragging) return;
    const cell = cellAtClient(ev.clientX, ev.clientY);
    if (!cell) return;
    if (selEnd && cell.col === selEnd.col && cell.row === selEnd.row) return;
    selEnd = cell;
    scheduleSelectionRepaint();
  };
  const onPointerUp = (ev: PointerEvent) => {
    if (!selDragging) return;
    selDragging = false;
    try {
      canvas.releasePointerCapture(ev.pointerId);
    } catch {
      /* same — releasePointerCapture can throw on unknown id */
    }
    const range = selectionRange();
    if (selectionEmpty(range)) {
      // Plain click without drag — clear the selection entirely.
      selStart = null;
      selEnd = null;
      scheduleSelectionRepaint();
    }
  };

  return (
    <div class="overlay-anchor" ref={host} tabIndex={-1}>
      <canvas
        ref={canvas}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      />
      <div
        class="terminal-loading"
        classList={{ "terminal-loading--ready": phase() === "ready" }}
        aria-hidden={phase() === "ready"}
      >
        <div class="terminal-loading-stack">
          <span class="terminal-loading-brand">tessera</span>
          <div class="terminal-loading-dots" aria-label="loading">
            <span />
            <span />
            <span />
          </div>
          <Show when={phase() !== "ready"}>
            <span class="terminal-loading-caption">
              {phase() === "spawning" ? "starting claude" : "connecting"}
            </span>
          </Show>
        </div>
      </div>
    </div>
  );
}

const TEXT_ENCODER = new TextEncoder();

function encodeKey(ev: KeyboardEvent): number[] {
  if (ev.ctrlKey && !ev.altKey && !ev.metaKey && ev.key.length === 1) {
    const lc = ev.key.toLowerCase();
    const cc = lc.charCodeAt(0);
    if (cc >= 0x61 && cc <= 0x7a) {
      return [cc - 0x60];
    }
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
    case "Delete":     return [0x1b, 0x5b, 0x33, 0x7e];
  }
  if (ev.key.length >= 1 && ev.key.length <= 2 && !ev.metaKey) {
    return Array.from(TEXT_ENCODER.encode(ev.key));
  }
  return [];
}

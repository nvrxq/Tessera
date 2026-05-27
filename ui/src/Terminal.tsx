import {
  createEffect,
  createSignal,
  getOwner,
  onCleanup,
  onMount,
  runWithOwner,
  Show,
} from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  readImage as clipReadImage,
  readText as clipReadText,
  writeText as clipWriteText,
} from "@tauri-apps/plugin-clipboard-manager";
import { setTerminalFontSize, settings } from "./lib/settings";
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
  /** "hidden" is reserved for future PTY backends that distinguish a
   *  shape-level hide (e.g. nested fullscreen apps, password prompts)
   *  from DECTCEM visibility. wezterm-term currently never emits it,
   *  but the alwaysShowCursor override must still respect it when it
   *  does — see `shouldShowCursor` below. */
  cursor_shape: "block" | "bar" | "underline" | "hidden";
}

const MIN_FONT_PX = 8;
const MAX_FONT_PX = 32;

/** Parse `#RRGGBB` → 0xRRGGBB int. Used to detect "this cell is the
 *  default background, skip painting it" in the full-paint fast path. */
function hexToInt(hex: string): number {
  if (hex.length !== 7 || hex[0] !== "#") return 0x0f0f10;
  const n = parseInt(hex.slice(1), 16);
  return Number.isFinite(n) ? n : 0x0f0f10;
}

const CURSOR_BLINK_MS = 530;

/** Module-level cache of `ctx.measureText("M")`-derived cell metrics, keyed
 *  by `"${px}px ${fontFamily}"`. measureText is a sync layout call (~tens of
 *  microseconds, sometimes more on cold web-font load) and the result is a
 *  pure function of the font shorthand — so we cache it once per (size,
 *  family) pair and reuse across the whole app lifetime. Shared across all
 *  Terminal instances since the cache is keyed on the actual font string;
 *  no per-component invalidation is needed when settings change because a
 *  changed font_size_px or font_family simply produces a new key. */
const fontMetricsCache = new Map<
  string,
  { cellW: number; cellH: number; baseline: number }
>();

/** Claude Code's TUI frequently sends DECTCEM (`\e[?25l`) to hide the
 *  cursor, which then propagates faithfully through wezterm-term and lands
 *  in our snapshots as `cursor_visible: false`. The result for the user
 *  is "у меня нет курсора" — they never see a typing indicator. Override
 *  the PTY's choice and paint our own cursor anyway when this is true.
 *  Defaults to `true` so the cursor is visible out of the box; a future
 *  Settings panel can flip this via the same key. */
const ALWAYS_SHOW_CURSOR_KEY = "tessera.alwaysShowCursor";
/** Old flat-namespaced key from before we standardised on `tessera.<field>`.
 *  Migrated on first read; safe to drop entirely after a few releases. */
const ALWAYS_SHOW_CURSOR_KEY_LEGACY = "tessera.term.alwaysShowCursor";
const loadAlwaysShowCursor = (): boolean => {
  // One-time migration: lift the value from the old key to the new one
  // and remove the legacy entry. Only runs when the new key is absent
  // and the old key is present, so it's idempotent across reloads.
  let raw = localStorage.getItem(ALWAYS_SHOW_CURSOR_KEY);
  if (raw === null) {
    const legacy = localStorage.getItem(ALWAYS_SHOW_CURSOR_KEY_LEGACY);
    if (legacy !== null) {
      localStorage.setItem(ALWAYS_SHOW_CURSOR_KEY, legacy);
      localStorage.removeItem(ALWAYS_SHOW_CURSOR_KEY_LEGACY);
      raw = legacy;
    }
  }
  if (raw === null) return true;
  return raw !== "false" && raw !== "0";
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

  // Snapshot values from the settings store. Live updates re-run the
  // createEffect below so changes apply without remounting the canvas.
  let fontPx = settings().terminal.font_size_px;
  let fontFamily = settings().terminal.font_family;
  let bgInt = hexToInt(settings().terminal.background);
  let bgHex = settings().terminal.background;
  let cursorColorHex = settings().terminal.cursor_color;
  let cursorBlinkEnabled = settings().terminal.cursor_blink;
  const alwaysShowCursor = loadAlwaysShowCursor();
  let cellW = 0;
  let cellH = 0;
  let baseline = 0;
  let lastKeydownAt = 0;
  const spawning = new Set<string>();

  // Cursor blink: `cursorBlinkVisible` toggles every CURSOR_BLINK_MS while
  // the cursor is on; it's reset to true on every keystroke so the user
  // never types into an "invisible" gap. Disabled when settings say so.
  let cursorBlinkVisible = true;
  let cursorBlinkTimer: number | null = null;
  function stopCursorBlink() {
    if (cursorBlinkTimer != null) {
      window.clearInterval(cursorBlinkTimer);
      cursorBlinkTimer = null;
    }
    cursorBlinkVisible = true;
  }
  function startCursorBlink() {
    stopCursorBlink();
    if (!cursorBlinkEnabled) return;
    cursorBlinkTimer = window.setInterval(() => {
      if (!lastCursorVisible) return;
      cursorBlinkVisible = !cursorBlinkVisible;
      const ctx = ctx2dOf();
      if (!ctx) return;
      const px = fontPx * (window.devicePixelRatio || 1);
      const idx = lastCursorRow * gridCols + lastCursorCol;
      if (idx >= 0 && idx < grid.length) {
        paintCell(ctx, idx, px);
      }
      if (cursorBlinkVisible) {
        paintCursor(ctx, lastCursorCol, lastCursorRow, lastCursorShape, px);
      }
    }, CURSOR_BLINK_MS);
  }

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
    ctx.save();
    try {
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
    } finally {
      ctx.restore();
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
    const key = `${px}px ${fontFamily}`;
    const cached = fontMetricsCache.get(key);
    if (cached) {
      cellW = cached.cellW;
      cellH = cached.cellH;
      baseline = cached.baseline;
      return;
    }
    ctx.font = key;
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
    baseline = Math.round(cellH - descent - (cellH - ascent - descent) * 0.5);
    fontMetricsCache.set(key, { cellW, cellH, baseline });
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
    const r = (idx / gridCols) | 0;
    const c = idx - r * gridCols;
    const x = (c * cellW) | 0;
    const y = (r * cellH) | 0;

    ctx.fillStyle = hexColor(cell.b);
    ctx.fillRect(x, y, cellW, cellH);

    if (cell.c !== " " || (cell.a & 28) !== 0) {
      const bold = (cell.a & 1) !== 0;
      const italic = (cell.a & 2) !== 0;
      ctx.font =
        (bold ? "bold " : "") +
        (italic ? "italic " : "") +
        `${px}px ${fontFamily}`;
      ctx.fillStyle = hexColor(cell.f);
      ctx.textBaseline = "alphabetic";
      // Clip the glyph to its cell so antialiased halos from italics /
      // descenders can't bleed into neighbouring cells and survive a
      // future delta repaint as a stray pixel.
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, y, cellW, cellH);
      ctx.clip();
      ctx.fillText(cell.c, x, (y + baseline) | 0);
      ctx.restore();

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

  /** Returns true if a cursor was actually painted, false if the call
   *  was a no-op (out-of-bounds coords or hidden shape). The caller uses
   *  this to decide whether to update `lastCursor*` state — otherwise a
   *  no-op paint with bad coords would later trigger an erase at the
   *  wrong cell on the next snapshot. */
  function paintCursor(
    ctx: CanvasRenderingContext2D,
    col: number,
    row: number,
    shape: Snapshot["cursor_shape"],
    px: number,
  ): boolean {
    if (col < 0 || row < 0) return false;
    if (col >= gridCols || row >= gridRows) return false;
    if (shape === "hidden") return false;
    const x = col * cellW;
    const y = row * cellH;
    // Bar/underline thickness: 10% of font px, floored to 2 device-pixels.
    // Floor of 1 produced a 1-CSS-px bar at 14px that all but disappeared
    // against `#0F0F10` on a low-DPI display.
    const thick = Math.max(2, Math.round(px * 0.1));
    ctx.save();
    try {
      ctx.fillStyle = cursorColorHex;
      if (shape === "block") {
        ctx.globalAlpha = 0.6;
        ctx.fillRect(x, y, cellW, cellH);
      } else if (shape === "underline") {
        ctx.fillRect(x, y + cellH - thick, cellW, thick);
      } else {
        ctx.fillRect(x, y, thick, cellH);
      }
    } finally {
      ctx.restore();
    }
    return true;
  }

  function paintFull(px: number) {
    const ctx = ctx2dOf();
    if (!ctx) return;
    ctx.fillStyle = bgHex;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.textBaseline = "alphabetic";
    ctx.font = `${px}px ${fontFamily}`;

    // Pass 1: backgrounds — batch contiguous non-default runs per row.
    for (let r = 0; r < gridRows; r++) {
      const yTop = r * cellH;
      let runColor = -1;
      let runStart = 0;
      for (let c = 0; c < gridCols; c++) {
        const cell = grid[r * gridCols + c];
        const bg = cell.b !== bgInt ? cell.b : -1;
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
          `${px}px ${fontFamily}`;
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
        grid.push({ c: " ", f: hexToInt(settings().terminal.foreground), b: bgInt, a: 0 });
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
    //
    // Two failure modes the previous code had:
    //   1. Delta overpaint: when typing-echo lands in a delta whose
    //      positions include the cursor's current cell, paintCell wipes
    //      the cursor and we never re-stamp it (the erase branch only
    //      fires when the cursor MOVED). Fix: always re-stamp when
    //      `shouldShowCursor` is true, regardless of move.
    //   2. Claude Code's TUI ships DECTCEM (\e[?25l) frequently —
    //      wezterm-term faithfully reports `visible: false` and the
    //      user sees no cursor at all ("у меня нет курсора"). Fix:
    //      override with `alwaysShowCursor` (default `true`).
    const ctx = ctx2dOf();
    if (!ctx) return;
    // Respect a backend-reported "hidden" shape even when the user opted
    // into `alwaysShowCursor`: TUIs intentionally swap to Hidden for
    // nested fullscreen apps and password prompts where leaking a cursor
    // would be wrong (or, worse, security-sensitive).
    const shouldShowCursor =
      snap.cursor_visible ||
      (alwaysShowCursor && snap.cursor_shape !== "hidden");
    // Erase the previous cursor cell if we painted one AND either the
    // position changed or we're about to stop painting a cursor. (If
    // we're about to repaint at the SAME position, the unconditional
    // paintCursor below covers it.)
    if (
      lastCursorVisible &&
      (!shouldShowCursor ||
        lastCursorCol !== snap.cursor_col ||
        lastCursorRow !== snap.cursor_row)
    ) {
      const oldIdx = lastCursorRow * gridCols + lastCursorCol;
      if (oldIdx >= 0 && oldIdx < grid.length) {
        paintCell(ctx, oldIdx, px);
      }
    }
    // Track the actual paint outcome — if paintCursor clamps (OOB) or
    // refuses (hidden shape), we must NOT remember the requested coords
    // as "painted here". Otherwise the next snapshot's erase branch
    // would compute `oldIdx = badRow * gridCols + badCol`, which for
    // some OOB combinations wraps into a valid index on a different row
    // and erases an unrelated cell. Only commit `last*` to what we drew.
    let painted = false;
    if (shouldShowCursor) {
      painted = paintCursor(
        ctx,
        snap.cursor_col,
        snap.cursor_row,
        snap.cursor_shape,
        px,
      );
    }
    if (painted) {
      lastCursorCol = snap.cursor_col;
      lastCursorRow = snap.cursor_row;
      lastCursorVisible = true;
      lastCursorShape = snap.cursor_shape;
    } else {
      // No cursor on screen this frame — invalidate the cached position
      // so the next snapshot's erase branch can't fire on stale coords.
      lastCursorVisible = false;
      lastCursorCol = -1;
      lastCursorRow = -1;
    }

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

  onMount(() => {
    // SolidJS resource owner — `onCleanup` calls registered after an
    // `await` inside an async callback lose the implicit owner and are
    // silently dropped. Capture it synchronously here and reattach via
    // `runWithOwner` for every cleanup that lives past the first await.
    const owner = getOwner();
    resizeCanvasBacking();
    measureCell(window.devicePixelRatio || 1);

    void (async () => {
      await listenerReady;

      const ro = new ResizeObserver(() => {
        void syncGrid();
      });
      ro.observe(host);

      if (cursorBlinkEnabled) startCursorBlink();

      // Drag-and-drop of files from Finder / file manager. Tauri 2 captures
      // OS-level drag-drop and emits `tauri://drag-drop` with the absolute
      // paths plus the drop position. HTML5 drop events do NOT fire while
      // Tauri's interception is enabled, so we must go through the event.
      //
      // Position-gate: only attach to the active terminal if the drop landed
      // inside its canvas. The sidebar and chrome shouldn't accept image
      // attaches. Position from Tauri 2 on macOS is in CSS pixels, matching
      // `canvas.getBoundingClientRect()`.
      const dropUnlisten = await listen<{
        paths: string[];
        position: { x: number; y: number };
      }>("tauri://drag-drop", (event) => {
        const sid = activeSessionId;
        if (!sid) return;
        const { paths, position } = event.payload;
        if (!paths || paths.length === 0) return;
        const rect = canvas.getBoundingClientRect();
        const inside =
          position.x >= rect.left &&
          position.x <= rect.right &&
          position.y >= rect.top &&
          position.y <= rect.bottom;
        if (!inside) return;
        // Claude Code accepts a path typed in as input and recognises image
        // files as attachments — same flow as Alacritty's "type the dropped
        // file path" behaviour. Multiple files: space-separated. Wrapped in
        // bracketed-paste markers so the TUI treats it as one paste rather
        // than per-char typing.
        const payload = `\x1b[200~${paths.join(" ")}\x1b[201~`;
        void ptyWriteString(sid, payload);
      });

      function onKey(ev: KeyboardEvent) {
        const sid = activeSessionId;
        if (!sid) return;
        lastKeydownAt = performance.now();
        // Cursor should always be visible while the user is actively
        // typing; reset the blink phase so the cursor doesn't disappear
        // mid-keystroke. The interval continues running.
        if (cursorBlinkEnabled && !cursorBlinkVisible) {
          cursorBlinkVisible = true;
        }
        if (
          (ev.ctrlKey || ev.metaKey) &&
          (ev.key === "r" || ev.key === "R" || ev.key === "F5")
        ) {
          return;
        }
        if ((ev.ctrlKey || ev.metaKey) && !ev.altKey) {
          // Ctrl/Cmd +/- zooms the terminal font. Persist through the
          // settings store (not localStorage) so the change is part of the
          // user's settings JSON and the modal stays in sync. The
          // settings_changed event re-runs our createEffect → fontPx and
          // measureCell are reapplied automatically.
          if (ev.key === "=" || ev.key === "+") {
            ev.preventDefault();
            const next = Math.min(MAX_FONT_PX, fontPx + 1);
            void setTerminalFontSize(next);
            return;
          }
          if (ev.key === "-" || ev.key === "_") {
            ev.preventDefault();
            const next = Math.max(MIN_FONT_PX, fontPx - 1);
            void setTerminalFontSize(next);
            return;
          }
          if (ev.key === "0") {
            ev.preventDefault();
            void setTerminalFontSize(14);
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
                // 1. Text on clipboard → paste as-is into PTY.
                try {
                  const text = await clipReadText();
                  if (text) {
                    await ptyWriteString(sid, text);
                    return;
                  }
                } catch {
                  /* No text — fall through to image. plugin-clipboard-manager
                   *  rejects when the clipboard holds non-text formats. */
                }
                // 2. Image on clipboard → save PNG to disk, type the path
                //    (bracketed-paste wrapped) so Claude Code attaches it.
                try {
                  const path = await saveClipboardImageToDisk();
                  if (!path) return;
                  await ptyWriteString(sid, `\x1b[200~${path}\x1b[201~`);
                } catch (e) {
                  console.warn("image paste failed", e);
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

      // Re-attach all post-await cleanups to the original component
      // owner — without this, every `onCleanup` here would silently
      // no-op because the owner ref is gone after the first `await`.
      runWithOwner(owner, () => {
        onCleanup(() => ro.disconnect());
        onCleanup(() => stopCursorBlink());
        onCleanup(() => dropUnlisten());
        onCleanup(() => document.removeEventListener("keydown", onKey));
      });

      host.focus();
    })();
  });

  // Live-react to settings changes: font / palette swaps re-measure the
  // cell grid and force a full repaint. `prev*` guards skip re-syncs
  // when the new value is identical (e.g. an unrelated section of the
  // config changed — typing in the cursor color picker shouldn't
  // re-measure the grid).
  let prevFont = fontFamily;
  let prevFontPx = fontPx;
  let prevBg = bgHex;
  let prevFg = settings().terminal.foreground;
  let prevCursorColor = cursorColorHex;
  let prevPalette = settings().terminal.palette.join("|");
  let prevBlink = cursorBlinkEnabled;
  createEffect(() => {
    const cfg = settings();
    fontFamily = cfg.terminal.font_family;
    fontPx = cfg.terminal.font_size_px;
    bgHex = cfg.terminal.background;
    bgInt = hexToInt(bgHex);
    cursorColorHex = cfg.terminal.cursor_color;
    cursorBlinkEnabled = cfg.terminal.cursor_blink;
    // Read foreground + palette through the signal so SolidJS tracks
    // them as dependencies; without these reads, the effect never
    // re-runs when the user picks a new foreground or ANSI swatch.
    const fgHex = cfg.terminal.foreground;
    const paletteKey = cfg.terminal.palette.join("|");
    const fontChanged = prevFont !== fontFamily || prevFontPx !== fontPx;
    const bgChanged = prevBg !== bgHex;
    const fgChanged = prevFg !== fgHex;
    const cursorColorChanged = prevCursorColor !== cursorColorHex;
    const paletteChanged = prevPalette !== paletteKey;
    const blinkChanged = prevBlink !== cursorBlinkEnabled;
    prevFont = fontFamily;
    prevFontPx = fontPx;
    prevBg = bgHex;
    prevFg = fgHex;
    prevCursorColor = cursorColorHex;
    prevPalette = paletteKey;
    prevBlink = cursorBlinkEnabled;
    if (fontChanged) {
      void syncGrid();
    } else if (bgChanged || fgChanged || cursorColorChanged || paletteChanged) {
      // Grid dims unchanged — repaint with the new colours. Skip a
      // backend resize round-trip; the existing grid mirror is still
      // valid, only the pixels need to be redrawn. `paintFull` keys
      // off the live `bgHex` / `cursorColorHex` / `settings()` reads
      // inside the painters, so picking new colours takes effect on
      // the next frame.
      const dpr = window.devicePixelRatio || 1;
      paintFull(fontPx * dpr);
    }
    if (blinkChanged) {
      if (cursorBlinkEnabled) startCursorBlink();
      else stopCursorBlink();
    }
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

  // ── Mouse wheel / trackpad → terminal scrollback ──
  // The previous Up/Down-arrow translation was wrong: in Claude Code's TUI,
  // Up/Down navigates the user's prompt history, not the chat-history view.
  // Real terminal scrollback is OUR responsibility — wezterm-term parses
  // and stores it on the backend, and `terminal_scroll` repositions our
  // sampling window over that buffer.
  //
  // Trackpad on macOS emits many small-delta events per gesture; we
  // accumulate pixel deltas and flush whole-line increments. Browsers
  // sometimes report `deltaMode` in lines or pages, so normalise first.
  const LINE_PIXELS = 20;
  let wheelAccum = 0;
  const onWheel = (ev: WheelEvent) => {
    const sid = activeSessionId;
    if (!sid) return;
    ev.preventDefault();
    let delta = ev.deltaY;
    if (ev.deltaMode === 1) delta *= LINE_PIXELS;       // lines
    else if (ev.deltaMode === 2) delta *= LINE_PIXELS * 10; // pages
    wheelAccum += delta;
    const lines = Math.trunc(wheelAccum / LINE_PIXELS);
    if (lines === 0) return;
    wheelAccum -= lines * LINE_PIXELS;
    // deltaY > 0 means content scrolls UP (= view moves DOWN) — i.e. we
    // want to move toward the live tail, which is `delta_back < 0`.
    // Conventional wheel-up gives deltaY < 0 → `delta_back > 0` (further
    // back into history). Hence the sign flip.
    void invoke("terminal_scroll", {
      sessionId: sid,
      deltaBack: -lines,
    }).catch((e) => console.warn("terminal_scroll failed", e));
  };

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
        onWheel={onWheel}
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

/** Encode `text` as UTF-8 then base64 (the wire format `pty_write` expects)
 *  and ship it to the supervisor. `btoa` only accepts bytes 0–255 as a
 *  string, so we walk the encoded buffer with `String.fromCharCode` rather
 *  than going through unsafe-character-corrupting paths like `btoa(text)`. */
async function ptyWriteString(sessionId: string, text: string): Promise<void> {
  const bytes = TEXT_ENCODER.encode(text);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  await invoke("pty_write", { sessionId, dataB64: btoa(bin) });
}

/** Read an image from the system clipboard, re-encode it as PNG via
 *  Canvas, persist it through the backend `save_paste_image` command, and
 *  return the on-disk path (or null if the clipboard had no image).
 *
 *  Re-encoding via Canvas is required because the clipboard plugin gives
 *  us raw RGBA, not PNG. We avoid shipping the raw RGBA over IPC (~4× the
 *  PNG size for a typical screenshot) by encoding in the WebView. */
async function saveClipboardImageToDisk(): Promise<string | null> {
  const img = await clipReadImage();
  const size = await img.size();
  const rgba = await img.rgba();
  if (!size.width || !size.height || rgba.byteLength === 0) return null;

  const canvas = document.createElement("canvas");
  canvas.width = size.width;
  canvas.height = size.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  const data = new ImageData(
    new Uint8ClampedArray(rgba),
    size.width,
    size.height,
  );
  ctx.putImageData(data, 0, 0);

  // toDataURL emits `data:image/png;base64,<payload>` — strip the prefix
  // and hand the payload straight to the Rust command, which decodes once
  // and writes to <data_dir>/tessera/pastes/<uuid>.png.
  const dataUrl = canvas.toDataURL("image/png");
  const commaIdx = dataUrl.indexOf(",");
  if (commaIdx < 0) return null;
  const b64 = dataUrl.slice(commaIdx + 1);
  const path = await invoke<string>("save_paste_image", { dataB64: b64 });
  return path;
}

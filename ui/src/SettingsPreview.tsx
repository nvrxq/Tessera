import {
  createEffect,
  onCleanup,
  onMount,
  type Component,
} from "solid-js";
import type { CursorShape, UserConfig } from "./lib/settings";

export interface SettingsPreviewProps {
  /** Accessor for the live *draft* settings — invoked inside an effect so
   *  SolidJS tracks every field as a dependency. */
  cfg: () => UserConfig;
}

/** Width/height of the preview pane in CSS pixels. Held constant so the
 *  layout doesn't shift while the user drags the font-size slider; instead
 *  the grid re-measures (cols × rows shrink) inside the fixed box. */
const PREVIEW_CSS_W = 480;
const PREVIEW_CSS_H = 180;
const CURSOR_BLINK_MS = 530;

/** One painted cell — `ch` plus indices into the palette. `fg = -1` falls
 *  back to the terminal foreground; this keeps the demo lines readable
 *  even when the user picks an oddball foreground colour. */
interface Cell {
  ch: string;
  fg: number; // -1 → use cfg.terminal.foreground
  bold?: boolean;
}

const SPACE: Cell = { ch: " ", fg: -1 };

/** Build a fixed-content mini-terminal — three lines that exercise the
 *  foreground colour (line 1), several ANSI palette slots including the
 *  bright range (line 2), and a `claude>` prompt with cursor (line 3).
 *  Returned as a row-major array of cells with one bit of metadata: the
 *  (col, row) where the cursor should sit, so the painter can stamp it
 *  after the glyph pass. */
function buildScene(cols: number, rows: number): {
  cells: Cell[];
  cursorCol: number;
  cursorRow: number;
} {
  const cells: Cell[] = new Array(cols * rows);
  for (let i = 0; i < cells.length; i++) cells[i] = SPACE;

  const put = (row: number, col: number, text: string, fg: number, bold = false) => {
    if (row < 0 || row >= rows) return;
    for (let i = 0; i < text.length; i++) {
      const c = col + i;
      if (c < 0 || c >= cols) continue;
      cells[row * cols + c] = { ch: text[i], fg, bold };
    }
  };

  // Line 1 — prompt + command. Foreground colour exercises cfg.foreground.
  // ANSI 8 (bright black) for the host path so the user can tell it's
  // a distinct slot, ANSI 6 (cyan) for the trailing $.
  put(0, 0, "tessera", -1, true);
  put(0, 8, "~/projects/demo", 8);
  put(0, 24, "$", 6);
  put(0, 26, "ls", -1);
  put(0, 29, "-la", 8);

  // Line 2 — `ls -la` output, sampling palette slots 0, 1, 2, 3, 4, 5, 7, 10.
  // Format: drwxr-xr-x then four sample filenames with distinct colours.
  put(1, 0, "drwxr-xr-x", 7);
  put(1, 11, "./", 2);
  put(1, 14, "main.rs", 1);
  put(1, 22, "Cargo.toml", 3);
  put(1, 33, "README.md", 4);
  put(1, 43, "build/", 5);
  put(1, 50, ".env", 0);
  put(1, 55, "logs", 10);

  // Line 3 — claude prompt; the cursor sits one cell past the `> `.
  put(2, 0, "claude>", -1, true);
  const cursorCol = 8;
  const cursorRow = 2;

  return { cells, cursorCol, cursorRow };
}

const SettingsPreview: Component<SettingsPreviewProps> = (props) => {
  let canvas!: HTMLCanvasElement;

  // Blink state. We toggle every CURSOR_BLINK_MS while the cfg says blink
  // is enabled; otherwise we hold true (cursor always visible). The state
  // lives outside the painter so a blink tick can request a repaint
  // without re-running every cfg-tracking effect.
  let cursorVisible = true;
  let blinkTimer: number | null = null;
  let paintQueued = false;

  /** Schedule a single repaint on the next animation frame. Coalesces
   *  bursts of effect re-runs (slider drag = one effect run per pixel)
   *  into at most one paint per refresh. */
  function schedulePaint() {
    if (paintQueued) return;
    paintQueued = true;
    requestAnimationFrame(() => {
      paintQueued = false;
      paint();
    });
  }

  function startBlink(enabled: boolean) {
    if (blinkTimer != null) {
      window.clearInterval(blinkTimer);
      blinkTimer = null;
    }
    cursorVisible = true;
    if (!enabled) return;
    blinkTimer = window.setInterval(() => {
      cursorVisible = !cursorVisible;
      schedulePaint();
    }, CURSOR_BLINK_MS);
  }

  /** Convert ANSI index → hex string from the live palette. Falls back to
   *  the configured foreground for `-1` and for indices outside 0..15
   *  (defence-in-depth — `buildScene` only uses valid slots). */
  function resolveFg(c: UserConfig, idx: number): string {
    if (idx < 0 || idx >= c.terminal.palette.length) return c.terminal.foreground;
    return c.terminal.palette[idx];
  }

  function paint() {
    const c = props.cfg();
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    // Backing store dims are derived from the fixed CSS box × dpr, so the
    // canvas stays crisp on hidpi without resizing the pane.
    const wantW = Math.round(PREVIEW_CSS_W * dpr);
    const wantH = Math.round(PREVIEW_CSS_H * dpr);
    if (canvas.width !== wantW) canvas.width = wantW;
    if (canvas.height !== wantH) canvas.height = wantH;
    canvas.style.width = `${PREVIEW_CSS_W}px`;
    canvas.style.height = `${PREVIEW_CSS_H}px`;

    const px = c.terminal.font_size_px * dpr;
    ctx.textBaseline = "alphabetic";
    ctx.font = `${px}px ${c.terminal.font_family}`;

    // Cell metrics — replicated from Terminal.tsx#measureCell so the
    // preview looks the same as the real grid.
    const m = ctx.measureText("M");
    const cellW = Math.max(1, Math.round(m.width));
    const ascent =
      typeof m.actualBoundingBoxAscent === "number"
        ? m.actualBoundingBoxAscent
        : px * 0.8;
    const descent =
      typeof m.actualBoundingBoxDescent === "number"
        ? m.actualBoundingBoxDescent
        : px * 0.2;
    const cellH = Math.max(1, Math.ceil((ascent + descent) * 1.25));
    const baseline = Math.floor(cellH - descent - (cellH - ascent - descent) * 0.5);

    const cols = Math.max(10, Math.floor(canvas.width / cellW));
    const rows = Math.max(3, Math.floor(canvas.height / cellH));

    // Background fill — uses cfg.background so the user sees the colour
    // they're picking applied to the whole pane (not just behind glyphs).
    ctx.fillStyle = c.terminal.background;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const scene = buildScene(cols, rows);

    // Glyph pass — paint visible cells one at a time. The real terminal
    // batches by colour for throughput; here we only ever paint ~25 cells
    // so the simplest implementation is plenty fast.
    for (let r = 0; r < rows; r++) {
      for (let col = 0; col < cols; col++) {
        const cell = scene.cells[r * cols + col];
        if (cell.ch === " ") continue;
        const fg = resolveFg(c, cell.fg);
        ctx.fillStyle = fg;
        ctx.font = (cell.bold ? "bold " : "") + `${px}px ${c.terminal.font_family}`;
        ctx.fillText(cell.ch, col * cellW, r * cellH + baseline);
      }
    }

    // Cursor — only paint when the blink phase says visible (or blink is
    // disabled, in which case `cursorVisible` is held true). Bar /
    // underline thickness floor of 2 device-px matches Terminal.tsx.
    if (cursorVisible) {
      const cx = scene.cursorCol * cellW;
      const cy = scene.cursorRow * cellH;
      const shape: CursorShape = c.terminal.cursor_shape;
      const thick = Math.max(2, Math.round(px * 0.1));
      ctx.fillStyle = c.terminal.cursor_color;
      if (shape === "block") {
        ctx.globalAlpha = 0.6;
        ctx.fillRect(cx, cy, cellW, cellH);
        ctx.globalAlpha = 1.0;
      } else if (shape === "underline") {
        ctx.fillRect(cx, cy + cellH - thick, cellW, thick);
      } else {
        // "bar"
        ctx.fillRect(cx, cy, thick, cellH);
      }
    }
  }

  onMount(() => {
    // Initial paint after mount so the canvas has a non-zero CSS size
    // before we start measuring. Subsequent paints come from the
    // cfg-tracking effect and the blink interval.
    schedulePaint();
  });

  // Track every relevant cfg field. Reading each one inside the effect
  // body registers it as a Solid dependency, so any draft edit reruns
  // this and queues a repaint. `startBlink` is called here too because
  // the blink toggle lives in cfg.
  createEffect(() => {
    const c = props.cfg();
    // Touch the fields the painter cares about so they're tracked.
    void c.terminal.font_family;
    void c.terminal.font_size_px;
    void c.terminal.background;
    void c.terminal.foreground;
    void c.terminal.cursor_color;
    void c.terminal.cursor_shape;
    void c.terminal.cursor_blink;
    // Palette is an array — joining it gives Solid a primitive to diff.
    void c.terminal.palette.join("|");
    startBlink(c.terminal.cursor_blink);
    schedulePaint();
  });

  onCleanup(() => {
    if (blinkTimer != null) {
      window.clearInterval(blinkTimer);
      blinkTimer = null;
    }
  });

  return (
    <canvas
      ref={canvas}
      class="settings-preview-canvas"
      width={PREVIEW_CSS_W}
      height={PREVIEW_CSS_H}
      aria-label="Live terminal preview"
    />
  );
};

export default SettingsPreview;

// Map Tessera's user settings palette to an xterm.js ITheme.
// Shape adapted from Terax (github.com/crynta/terax-ai,
// src/styles/terminalTheme.ts, Apache-2.0) — see NOTICE — but sourced from
// Tessera's own settings store instead of Terax's token system.

import type { ITheme } from "@xterm/xterm";
import { settings } from "./settings";

/** Build an xterm ITheme from the current terminal settings. The 16-entry
 *  ANSI palette is guaranteed to be exactly 16 by the Rust config loader
 *  (it rejects a wrong-length palette and falls back to defaults). */
export function buildTerminalTheme(): ITheme {
  const t = settings().terminal;
  const p = t.palette ?? [];
  return {
    background: t.background,
    foreground: t.foreground,
    cursor: t.cursor_color,
    cursorAccent: t.background,
    selectionBackground: "rgba(110, 160, 220, 0.32)",
    black: p[0],
    red: p[1],
    green: p[2],
    yellow: p[3],
    blue: p[4],
    magenta: p[5],
    cyan: p[6],
    white: p[7],
    brightBlack: p[8],
    brightRed: p[9],
    brightGreen: p[10],
    brightYellow: p[11],
    brightBlue: p[12],
    brightMagenta: p[13],
    brightCyan: p[14],
    brightWhite: p[15],
  };
}

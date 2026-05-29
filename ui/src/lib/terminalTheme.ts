// Map the active theme (or, for the default, the user's settings palette) to an
// xterm.js ITheme. Shape adapted from Terax (github.com/crynta/terax-ai,
// src/styles/terminalTheme.ts, Apache-2.0) — see NOTICE.

import type { ITheme } from "@xterm/xterm";
import { settings } from "./settings";
import { activeTheme } from "./themes";

/** Build an xterm ITheme. A curated theme supplies its own fixed palette; the
 *  default ("tessera"/"light") theme uses the user-editable settings().terminal
 *  palette (guaranteed 16 entries by the Rust config loader). */
export function buildTerminalTheme(): ITheme {
  const preset = activeTheme().terminal;
  const t = settings().terminal;
  const background = preset?.background ?? t.background;
  const foreground = preset?.foreground ?? t.foreground;
  const cursor = preset?.cursor ?? t.cursor_color;
  const p = preset?.palette ?? t.palette ?? [];
  return {
    background,
    foreground,
    cursor,
    cursorAccent: background,
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

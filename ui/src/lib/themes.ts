// Theme registry. Tessera already ships a CSS-variable theme system keyed off
// the `data-theme` attribute on <html> (dark default + a dormant light theme).
// This adds curated presets on top — inspired by Terax's theme set
// (github.com/crynta/terax-ai, Apache-2.0 — see NOTICE) — without disturbing
// the warm-dark "tessera" default that DESIGN.md mandates: it stays the default
// and its terminal palette is still the user-editable settings().terminal.
//
// Each preset supplies (a) a `data-theme` value whose matching CSS block in
// index.css recolours the app chrome, and (b) a fixed 16-colour terminal
// palette consumed by lib/terminalTheme.ts.

import { createSignal } from "solid-js";

export type ThemeId =
  | "tessera"
  | "light"
  | "catppuccin"
  | "nord"
  | "gruvbox"
  | "tokyo-night";

export type TerminalPalette = {
  background: string;
  foreground: string;
  cursor: string;
  /** 16 hex strings, ANSI 0..15. */
  palette: string[];
};

export type ThemeDef = {
  id: ThemeId;
  label: string;
  /** Value written to `<html data-theme>`; index.css blocks key off this. */
  dataTheme: string;
  /** null → terminal uses the user-editable settings().terminal palette. */
  terminal: TerminalPalette | null;
};

export const THEMES: ThemeDef[] = [
  {
    id: "tessera",
    label: "Tessera — warm dark",
    dataTheme: "dark",
    terminal: null,
  },
  {
    id: "light",
    label: "Tessera — light",
    dataTheme: "light",
    terminal: null,
  },
  {
    id: "catppuccin",
    label: "Catppuccin Mocha",
    dataTheme: "catppuccin",
    terminal: {
      background: "#1E1E2E",
      foreground: "#CDD6F4",
      cursor: "#F5E0DC",
      palette: [
        "#45475A", "#F38BA8", "#A6E3A1", "#F9E2AF",
        "#89B4FA", "#F5C2E7", "#94E2D5", "#BAC2DE",
        "#585B70", "#F38BA8", "#A6E3A1", "#F9E2AF",
        "#89B4FA", "#F5C2E7", "#94E2D5", "#A6ADC8",
      ],
    },
  },
  {
    id: "nord",
    label: "Nord",
    dataTheme: "nord",
    terminal: {
      background: "#2E3440",
      foreground: "#D8DEE9",
      cursor: "#D8DEE9",
      palette: [
        "#3B4252", "#BF616A", "#A3BE8C", "#EBCB8B",
        "#81A1C1", "#B48EAD", "#88C0D0", "#E5E9F0",
        "#4C566A", "#BF616A", "#A3BE8C", "#EBCB8B",
        "#81A1C1", "#B48EAD", "#8FBCBB", "#ECEFF4",
      ],
    },
  },
  {
    id: "gruvbox",
    label: "Gruvbox Dark",
    dataTheme: "gruvbox",
    terminal: {
      background: "#282828",
      foreground: "#EBDBB2",
      cursor: "#EBDBB2",
      palette: [
        "#282828", "#CC241D", "#98971A", "#D79921",
        "#458588", "#B16286", "#689D6A", "#A89984",
        "#928374", "#FB4934", "#B8BB26", "#FABD2F",
        "#83A598", "#D3869B", "#8EC07C", "#EBDBB2",
      ],
    },
  },
  {
    id: "tokyo-night",
    label: "Tokyo Night",
    dataTheme: "tokyo-night",
    terminal: {
      background: "#1A1B26",
      foreground: "#C0CAF5",
      cursor: "#C0CAF5",
      palette: [
        "#15161E", "#F7768E", "#9ECE6A", "#E0AF68",
        "#7AA2F7", "#BB9AF7", "#7DCFFF", "#A9B1D6",
        "#414868", "#F7768E", "#9ECE6A", "#E0AF68",
        "#7AA2F7", "#BB9AF7", "#7DCFFF", "#C0CAF5",
      ],
    },
  },
];

const STORAGE_KEY = "tessera.theme";

function readStored(): ThemeId {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v && THEMES.some((t) => t.id === v)) return v as ThemeId;
  } catch {}
  return "tessera";
}

export function themeDef(id: ThemeId): ThemeDef {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

const [activeThemeId, setActiveThemeIdSig] = createSignal<ThemeId>(readStored());
export { activeThemeId };

export function activeTheme(): ThemeDef {
  return themeDef(activeThemeId());
}

/** Switch theme: recolour app chrome (data-theme), persist, and bump the
 *  reactive signal so terminal palette consumers re-run. */
export function setTheme(id: ThemeId): void {
  const def = themeDef(id);
  try {
    document.documentElement.setAttribute("data-theme", def.dataTheme);
  } catch {}
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {}
  setActiveThemeIdSig(id);
}

// Apply the persisted theme immediately on module load, before first paint, so
// there's no flash of the default chrome.
try {
  document.documentElement.setAttribute("data-theme", themeDef(readStored()).dataTheme);
} catch {}

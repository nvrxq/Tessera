import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { createSignal } from "solid-js";

/** Mirrors `tessera_core::config::UserConfig`. Field names use snake_case
 *  to match the serde JSON wire shape — keep them in sync, otherwise the
 *  Rust deserializer will reject the round-trip on save. */

export type Density = "compact" | "comfortable";
export type CursorShape = "block" | "bar" | "underline";

export interface AppearanceConfig {
  ui_font_family: string;
  density: Density;
}

export interface TerminalConfig {
  font_family: string;
  font_size_px: number;
  background: string;
  foreground: string;
  cursor_shape: CursorShape;
  cursor_color: string;
  cursor_blink: boolean;
  /** 16 hex strings matching ANSI 0..15. */
  palette: string[];
}

export interface BehaviorConfig {
  auto_spawn_on_workspace_open: boolean;
  save_scrollback_lines: number;
}

export interface UserConfig {
  schema_version: number;
  appearance: AppearanceConfig;
  terminal: TerminalConfig;
  behavior: BehaviorConfig;
}

export const DEFAULT_ANSI_PALETTE: string[] = [
  "#000000", // 0  black
  "#CD0000", // 1  red
  "#00CD00", // 2  green
  "#CDCD00", // 3  yellow
  "#0000EE", // 4  blue
  "#CD00CD", // 5  magenta
  "#00CDCD", // 6  cyan
  "#E5E5E5", // 7  white
  "#7F7F7F", // 8  bright black
  "#FF0000", // 9  bright red
  "#00FF00", // 10 bright green
  "#FFFF00", // 11 bright yellow
  "#5C5CFF", // 12 bright blue
  "#FF00FF", // 13 bright magenta
  "#00FFFF", // 14 bright cyan
  "#FFFFFF", // 15 bright white
];

/** Identical to `UserConfig::default()` on the Rust side — used as the
 *  initial signal value before the first `loadSettings()` resolves, and
 *  as the "Reset to defaults" button's payload. */
export const DEFAULT_CONFIG: UserConfig = {
  schema_version: 1,
  appearance: {
    ui_font_family: "Geist, system-ui, -apple-system, sans-serif",
    density: "compact",
  },
  terminal: {
    font_family:
      '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, Menlo, monospace',
    font_size_px: 14,
    background: "#0F0F10",
    foreground: "#E8E8E6",
    cursor_shape: "block",
    cursor_color: "#E8E8E6",
    cursor_blink: false,
    palette: DEFAULT_ANSI_PALETTE.slice(),
  },
  behavior: {
    auto_spawn_on_workspace_open: true,
    save_scrollback_lines: 5000,
  },
};

export function loadSettings(): Promise<UserConfig> {
  return invoke<UserConfig>("settings_load");
}

export function saveSettings(config: UserConfig): Promise<void> {
  return invoke<void>("settings_save", { config });
}

export function settingsConfigPath(): Promise<string> {
  return invoke<string>("settings_config_path");
}

export function onSettingsChanged(
  cb: (cfg: UserConfig) => void,
): Promise<UnlistenFn> {
  return listen<UserConfig>("settings_changed", (event) => cb(event.payload));
}

/** Module-level signal that holds the live settings — kept as a singleton
 *  so any component can read the current config and react to updates
 *  without prop-drilling. App initialises it on mount. */
const [config, setConfig] = createSignal<UserConfig>(DEFAULT_CONFIG);
export { config as settings };

/** Replace the signal's value. Use after `loadSettings` resolves, on
 *  every `settings_changed` event, and after a successful local save
 *  (so the UI reflects the change without waiting for the round-trip
 *  event). */
export function setSettings(cfg: UserConfig): void {
  setConfig(cfg);
}

/** Bump just the font size and persist — keeps the Ctrl+/- terminal-zoom
 *  flow working but routes through the settings store instead of
 *  `localStorage`. The settings_changed event will fire as a result of
 *  the save and any other live components will re-render with the new
 *  size.  */
export async function setTerminalFontSize(px: number): Promise<void> {
  // Clamp to the same 8..32 range the Rust config enforces, so a stray caller
  // can't push an out-of-range size into the live signal (and through the save
  // round-trip) before the backend normalizes it back.
  const clamped = Math.max(8, Math.min(32, Math.round(px)));
  const cur = config();
  const next: UserConfig = {
    ...cur,
    terminal: { ...cur.terminal, font_size_px: clamped },
  };
  setSettings(next);
  await saveSettings(next);
}

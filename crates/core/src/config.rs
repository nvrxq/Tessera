//! User-controlled application settings persisted as JSON on disk.
//!
//! Loaded once at app start and re-read whenever the file changes. The
//! Tauri layer additionally broadcasts a `settings_changed` event on save
//! so live UI updates work without a relaunch.
//!
//! Design rules of thumb:
//! - Every field has a sensible default so a brand-new install (or a
//!   corrupted JSON file) Just Works.
//! - `#[serde(default)]` everywhere → adding a field in a later version
//!   does not break older configs.
//! - Unknown fields are silently ignored (serde default behaviour) so a
//!   newer config dropped onto an older binary degrades gracefully.
//! - Hex colour strings are validated on load via the `HexColor` newtype;
//!   a malformed entry falls back to its default, not a parse error.
//!
//! The whole file is one shot of I/O — load returns `Self`, save returns
//! `Result<()>`. Anything more elaborate (file-watching, diff-driven
//! patching) belongs in the host process, not in the data model.

use serde::{Deserialize, Deserializer, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Bump when an incompatible change is made (e.g. a field removed or
/// renamed). Older readers can refuse to load or migrate as appropriate.
/// At v1 nothing migrates; readers ignore the value.
pub const SCHEMA_VERSION: u32 = 1;

/// Hex colour string of the form `#RRGGBB`. Stored as a `String` for the
/// wire/serde shape; deserialisation rejects anything that isn't a
/// 7-char hash-prefixed hex triplet.
///
/// The inner field is private so a `HexColor` can only be built via
/// `new`/`from_str`/serde deserialisation — all of which run the
/// `is_valid_hex` check. That guarantee lets downstream parsers
/// (`parse_hex` in the Tauri layer) skip re-validation safely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HexColor(String);

impl HexColor {
    pub fn new(s: impl Into<String>) -> Result<Self, String> {
        let s = s.into();
        if is_valid_hex(&s) {
            Ok(HexColor(s))
        } else {
            Err(format!("invalid hex colour: {s:?} (expected #RRGGBB)"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for HexColor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HexColor::new(s)
    }
}

fn is_valid_hex(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        HexColor::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    Comfortable,
}

impl Default for Density {
    fn default() -> Self {
        Self::Compact
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

impl Default for CursorShape {
    fn default() -> Self {
        Self::Block
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppearanceConfig {
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,
    #[serde(default)]
    pub density: Density,
}

fn default_ui_font_family() -> String {
    "Geist, system-ui, -apple-system, sans-serif".to_string()
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            ui_font_family: default_ui_font_family(),
            density: Density::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalConfig {
    #[serde(default = "default_term_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size_px")]
    pub font_size_px: u16,
    #[serde(default = "default_term_bg")]
    pub background: HexColor,
    #[serde(default = "default_term_fg")]
    pub foreground: HexColor,
    #[serde(default)]
    pub cursor_shape: CursorShape,
    #[serde(default = "default_cursor_color")]
    pub cursor_color: HexColor,
    #[serde(default)]
    pub cursor_blink: bool,
    /// 16-entry ANSI palette, indices 0..=15 (black, red, green, yellow,
    /// blue, magenta, cyan, white, then the bright variants).
    #[serde(default = "default_ansi_palette")]
    pub palette: Vec<HexColor>,
}

fn default_term_font_family() -> String {
    "\"Geist Mono\", \"JetBrains Mono\", \"Fira Code\", ui-monospace, Menlo, monospace".to_string()
}

fn default_font_size_px() -> u16 {
    14
}

fn default_term_bg() -> HexColor {
    HexColor::new("#0F0F10").expect("static default hex is valid")
}

fn default_term_fg() -> HexColor {
    HexColor::new("#E8E8E6").expect("static default hex is valid")
}

fn default_cursor_color() -> HexColor {
    HexColor::new("#E8E8E6").expect("static default hex is valid")
}

/// The standard xterm 16 (matches `tessera_term::palette::tessera_dark`'s
/// 0..15 entries). Keep these in sync with `palette.rs`.
pub fn default_ansi_palette() -> Vec<HexColor> {
    [
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
    ]
    .iter()
    .map(|s| HexColor::new(*s).expect("static palette entries are valid"))
    .collect()
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: default_term_font_family(),
            font_size_px: default_font_size_px(),
            background: default_term_bg(),
            foreground: default_term_fg(),
            cursor_shape: CursorShape::default(),
            cursor_color: default_cursor_color(),
            cursor_blink: false,
            palette: default_ansi_palette(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BehaviorConfig {
    #[serde(default = "default_true")]
    pub auto_spawn_on_workspace_open: bool,
    #[serde(default = "default_scrollback")]
    pub save_scrollback_lines: u32,
}

fn default_true() -> bool {
    true
}

fn default_scrollback() -> u32 {
    5000
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            auto_spawn_on_workspace_open: true,
            save_scrollback_lines: default_scrollback(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserConfig {
    /// Self-reported schema version. Defaults to `SCHEMA_VERSION` on a
    /// fresh load so newly written files always carry the current version.
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
}

fn current_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            appearance: AppearanceConfig::default(),
            terminal: TerminalConfig::default(),
            behavior: BehaviorConfig::default(),
        }
    }
}

impl UserConfig {
    pub fn schema_version() -> u32 {
        SCHEMA_VERSION
    }

    /// Read `path` and parse it. Missing file → default. Malformed JSON or
    /// validation error → log a warning and return default (so a broken
    /// config never bricks the app). Always returns *something* usable.
    pub fn load_or_default(path: &Path) -> Self {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Self::default();
            }
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "settings: read failed; using defaults");
                return Self::default();
            }
        };
        match serde_json::from_slice::<UserConfig>(&bytes) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "settings: parse failed; falling back to defaults"
                );
                Self::default()
            }
        }
    }

    /// Atomically write the config to `path`. Creates parent directories
    /// as needed.
    ///
    /// Atomicity guarantees:
    ///   1. The payload is written to a uniquely-named temp file in the
    ///      same directory (via `tempfile::NamedTempFile::new_in`), so
    ///      two concurrent `save` calls can't race on a shared
    ///      `*.json.tmp` name.
    ///   2. The temp file is fsynced before the rename, so a power loss
    ///      mid-write never leaves a torn payload as the live file.
    ///   3. After `persist`, the parent directory itself is fsynced so
    ///      the rename's directory entry is durable on Linux — without
    ///      this, a crash can roll back the rename and lose the save
    ///      even though the data file is on disk.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => {
                std::fs::create_dir_all(p)?;
                p
            }
            // No parent component (e.g. `settings.json` with no dir) —
            // fall back to the current working directory for the temp
            // file and the dir fsync.
            _ => Path::new("."),
        };
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(&json)?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| e.error)?;
        // Fsync the parent directory so the rename's dirent is durable.
        // Some filesystems (e.g. ext4 with data=ordered + power loss)
        // can otherwise replay the rename in a way that drops it.
        let dir = std::fs::OpenOptions::new().read(true).open(parent)?;
        dir.sync_all()?;
        Ok(())
    }
}

/// Conventional config file location for the platform.
///   - Linux:   `~/.config/tessera/settings.json`
///   - macOS:   `~/Library/Application Support/tessera/settings.json`
///   - Windows: `%APPDATA%/tessera/settings.json`
///
/// Falls back to `./settings.json` if no platform config dir is
/// discoverable — uncommon, but means tests and odd container
/// environments still get a usable path.
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("tessera").join("settings.json"))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

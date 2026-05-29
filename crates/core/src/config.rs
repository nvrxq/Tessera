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
//! - Load is per-field tolerant: a single invalid value (a malformed hex
//!   colour, an out-of-range or wrong-typed size, a wrong JSON type) only
//!   resets *that* field to its default — every other valid field is
//!   preserved. The parse goes through a lenient mirror (`RawConfig`)
//!   whose colour fields are plain strings and whose sizes are wide
//!   integers, then each field is validated as it is promoted into a
//!   `UserConfig`. Only a file that is missing or not valid JSON at all
//!   falls back to the full default.
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

/// Inclusive bounds for the terminal font size, in pixels. Mirrors the
/// frontend slider constants (`min={8} max={32}` in `SettingsModal.tsx`).
/// A size of 0 renders an invisible grid; an enormous size collapses the
/// grid to a few unusable cells — so both disk-read and disk-write paths
/// clamp into this range regardless of where the value came from.
pub const FONT_SIZE_PX_MIN: u16 = 8;
pub const FONT_SIZE_PX_MAX: u16 = 32;

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

/// Lenient on-disk mirror of [`UserConfig`]. Every field is optional and
/// every leaf that `UserConfig` validates (colours, sizes, enums) is held
/// here in its loosest serde-acceptable shape — colours as `String`,
/// sizes as a wide `i64`/`f64`-tolerant number, enums as `String`. This
/// lets the whole document parse even when a single field is invalid, so
/// the promotion step below can drop *just* the offending field instead of
/// failing the entire load. Unknown fields are ignored as usual.
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    schema_version: Option<u32>,
    appearance: Option<RawAppearance>,
    terminal: Option<RawTerminal>,
    behavior: Option<RawBehavior>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAppearance {
    ui_font_family: Option<String>,
    // Enums are read as raw strings so an unknown variant resets only this
    // field rather than failing the whole struct (and so cascading to a
    // full-default load).
    density: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTerminal {
    font_family: Option<String>,
    // Read as a free-form JSON value (not `Number`) so a wrong-typed size
    // (e.g. the string `"huge"`) resets only this field instead of failing
    // the whole `terminal` section and cascading to a full-default load.
    font_size_px: Option<serde_json::Value>,
    background: Option<String>,
    foreground: Option<String>,
    cursor_shape: Option<String>,
    cursor_color: Option<String>,
    // `Value` (not `bool`) so a wrong-typed flag resets only this field.
    cursor_blink: Option<serde_json::Value>,
    // `Value` (not `Vec<String>`) so a non-array, or an array with a
    // non-string entry, resets only the palette instead of failing the whole
    // `terminal` section.
    palette: Option<serde_json::Value>,
}

/// Parse a snake_case enum value via its own `Deserialize`, falling back
/// to `default` (with a warning) on an unknown variant.
fn enum_or_default<T>(raw: Option<String>, field: &str, default: T) -> T
where
    T: for<'de> Deserialize<'de>,
{
    match raw {
        None => default,
        Some(s) => match serde_json::from_value::<T>(serde_json::Value::String(s.clone())) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(field, value = %s, error = %e, "settings: unknown variant; using default for this field");
                default
            }
        },
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawBehavior {
    auto_spawn_on_workspace_open: Option<serde_json::Value>,
    save_scrollback_lines: Option<serde_json::Value>,
}

/// Validate a single hex colour string, falling back to `default` and
/// warning (with the field name) when it is missing or malformed.
fn hex_or_default(raw: Option<String>, field: &str, default: HexColor) -> HexColor {
    match raw {
        None => default,
        Some(s) => match HexColor::new(s) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(field, error = %e, "settings: invalid value; using default for this field");
                default
            }
        },
    }
}

/// Coerce a lenient JSON number into a `u16`, falling back to `default`
/// (with a warning) when it is absent, fractional, negative, or overflows.
fn u16_or_default(raw: Option<serde_json::Value>, field: &str, default: u16) -> u16 {
    match raw {
        None | Some(serde_json::Value::Null) => default,
        Some(v) => match v.as_u64().and_then(|n| u16::try_from(n).ok()) {
            Some(n) => n,
            None => {
                tracing::warn!(field, value = %v, "settings: non-numeric or out-of-range size; using default for this field");
                default
            }
        },
    }
}

/// Coerce a lenient JSON number into a `u32`, falling back to `default`
/// (with a warning) when it is absent, fractional, negative, or overflows.
fn u32_or_default(raw: Option<serde_json::Value>, field: &str, default: u32) -> u32 {
    match raw {
        None | Some(serde_json::Value::Null) => default,
        Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => n,
            None => {
                tracing::warn!(field, value = %v, "settings: non-numeric or out-of-range value; using default for this field");
                default
            }
        },
    }
}

/// Coerce a lenient JSON value into a `bool`, falling back to `default`
/// (with a warning) when it is absent or not a boolean. Read as `Value`
/// rather than `Option<bool>` so a wrong-typed flag resets only this field
/// instead of failing the whole section and cascading to a full default.
fn bool_or_default(raw: Option<serde_json::Value>, field: &str, default: bool) -> bool {
    match raw {
        None | Some(serde_json::Value::Null) => default,
        Some(v) => match v.as_bool() {
            Some(b) => b,
            None => {
                tracing::warn!(field, value = %v, "settings: non-boolean value; using default for this field");
                default
            }
        },
    }
}

impl From<RawAppearance> for AppearanceConfig {
    fn from(raw: RawAppearance) -> Self {
        Self {
            ui_font_family: raw.ui_font_family.unwrap_or_else(default_ui_font_family),
            density: enum_or_default(raw.density, "appearance.density", Density::default()),
        }
    }
}

impl From<RawTerminal> for TerminalConfig {
    fn from(raw: RawTerminal) -> Self {
        // A whole-palette default kicks in if any single entry is bad —
        // the palette is read as a 16-tuple, so a partial repair would
        // silently shift the ANSI indices.
        let palette = match raw.palette {
            None | Some(serde_json::Value::Null) => default_ansi_palette(),
            // A whole-palette default kicks in if the value isn't an array,
            // or any single entry is missing/non-string/bad-hex — the palette
            // is read as a 16-tuple, so a partial repair would silently shift
            // the ANSI indices.
            Some(serde_json::Value::Array(entries)) => {
                let mut out = Vec::with_capacity(entries.len());
                let mut bad = false;
                for (i, v) in entries.into_iter().enumerate() {
                    match v.as_str().map(|s| HexColor::new(s.to_string())) {
                        Some(Ok(c)) => out.push(c),
                        other => {
                            tracing::warn!(field = "terminal.palette", index = i, value = ?other, "settings: invalid palette entry; using default palette");
                            bad = true;
                            break;
                        }
                    }
                }
                if bad {
                    default_ansi_palette()
                } else {
                    out
                }
            }
            Some(other) => {
                tracing::warn!(field = "terminal.palette", value = %other, "settings: palette is not an array; using default palette");
                default_ansi_palette()
            }
        };
        Self {
            font_family: raw.font_family.unwrap_or_else(default_term_font_family),
            font_size_px: u16_or_default(
                raw.font_size_px,
                "terminal.font_size_px",
                default_font_size_px(),
            ),
            background: hex_or_default(raw.background, "terminal.background", default_term_bg()),
            foreground: hex_or_default(raw.foreground, "terminal.foreground", default_term_fg()),
            cursor_shape: enum_or_default(
                raw.cursor_shape,
                "terminal.cursor_shape",
                CursorShape::default(),
            ),
            cursor_color: hex_or_default(
                raw.cursor_color,
                "terminal.cursor_color",
                default_cursor_color(),
            ),
            cursor_blink: bool_or_default(raw.cursor_blink, "terminal.cursor_blink", false),
            palette,
        }
    }
}

impl From<RawBehavior> for BehaviorConfig {
    fn from(raw: RawBehavior) -> Self {
        Self {
            auto_spawn_on_workspace_open: bool_or_default(
                raw.auto_spawn_on_workspace_open,
                "behavior.auto_spawn_on_workspace_open",
                true,
            ),
            save_scrollback_lines: u32_or_default(
                raw.save_scrollback_lines,
                "behavior.save_scrollback_lines",
                default_scrollback(),
            ),
        }
    }
}

impl From<RawConfig> for UserConfig {
    fn from(raw: RawConfig) -> Self {
        Self {
            schema_version: raw.schema_version.unwrap_or(SCHEMA_VERSION),
            appearance: raw.appearance.unwrap_or_default().into(),
            terminal: raw.terminal.unwrap_or_default().into(),
            behavior: raw.behavior.unwrap_or_default().into(),
        }
    }
}

impl UserConfig {
    pub fn schema_version() -> u32 {
        SCHEMA_VERSION
    }

    /// Clamp loosely-bounded numeric fields into their valid ranges. Run
    /// on both the disk-read and the disk-write path so an out-of-range
    /// value can neither be loaded into a broken grid nor round-tripped
    /// back to disk by the modal.
    pub fn normalize(&mut self) {
        self.terminal.font_size_px = self
            .terminal
            .font_size_px
            .clamp(FONT_SIZE_PX_MIN, FONT_SIZE_PX_MAX);
    }

    /// Read `path` and parse it. Missing file → default. JSON that is not
    /// well-formed at all → log a warning and return default (so a broken
    /// config never bricks the app). A well-formed document with one or
    /// more *invalid field values* is parsed leniently: each bad field is
    /// reset to its own default (with a warning naming it) while every
    /// other valid field is preserved. Always returns *something* usable.
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
        let mut cfg = match serde_json::from_slice::<RawConfig>(&bytes) {
            Ok(raw) => UserConfig::from(raw),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "settings: parse failed; falling back to defaults"
                );
                Self::default()
            }
        };
        cfg.normalize();
        cfg
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
        // Clamp before serializing so a bad in-memory value (e.g. one the
        // modal round-tripped) can't land on disk out of range.
        let mut normalized = self.clone();
        normalized.normalize();
        let json = serde_json::to_vec_pretty(&normalized).map_err(std::io::Error::other)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn load_from_str(body: &str) -> UserConfig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, body).unwrap();
        UserConfig::load_or_default(&path)
    }

    /// A fully-custom config with exactly one bad hex colour keeps every
    /// other field; only the offending colour falls back to its default.
    #[test]
    fn one_bad_hex_color_preserves_every_other_field() {
        let cfg = load_from_str(
            r##"{
                "schema_version": 1,
                "appearance": { "ui_font_family": "MyFont", "density": "comfortable" },
                "terminal": {
                    "font_family": "MyMono",
                    "font_size_px": 18,
                    "background": "not-a-hex",
                    "foreground": "#123456",
                    "cursor_shape": "bar",
                    "cursor_color": "#ABCDEF",
                    "cursor_blink": true,
                    "palette": [
                        "#000000","#111111","#222222","#333333",
                        "#444444","#555555","#666666","#777777",
                        "#888888","#999999","#AAAAAA","#BBBBBB",
                        "#CCCCCC","#DDDDDD","#EEEEEE","#FFFFFF"
                    ]
                },
                "behavior": { "auto_spawn_on_workspace_open": false, "save_scrollback_lines": 1234 }
            }"##,
        );

        // Only `background` reset to default …
        assert_eq!(cfg.terminal.background, default_term_bg());
        // … everything else preserved.
        assert_eq!(cfg.appearance.ui_font_family, "MyFont");
        assert_eq!(cfg.appearance.density, Density::Comfortable);
        assert_eq!(cfg.terminal.font_family, "MyMono");
        assert_eq!(cfg.terminal.font_size_px, 18);
        assert_eq!(cfg.terminal.foreground.as_str(), "#123456");
        assert_eq!(cfg.terminal.cursor_shape, CursorShape::Bar);
        assert_eq!(cfg.terminal.cursor_color.as_str(), "#ABCDEF");
        assert!(cfg.terminal.cursor_blink);
        assert_eq!(cfg.terminal.palette[1].as_str(), "#111111");
        assert!(!cfg.behavior.auto_spawn_on_workspace_open);
        assert_eq!(cfg.behavior.save_scrollback_lines, 1234);
    }

    /// A wrong-typed / non-numeric font size resets *only* the font size;
    /// the rest of the document survives.
    #[test]
    fn bad_font_size_type_only_resets_font_size() {
        let cfg = load_from_str(
            r##"{
                "terminal": {
                    "font_family": "Keep",
                    "font_size_px": "huge",
                    "foreground": "#0A0B0C"
                }
            }"##,
        );
        assert_eq!(cfg.terminal.font_size_px, default_font_size_px());
        assert_eq!(cfg.terminal.font_family, "Keep");
        assert_eq!(cfg.terminal.foreground.as_str(), "#0A0B0C");
        // Untouched fields still default.
        assert_eq!(cfg.terminal.background, default_term_bg());
    }

    /// Out-of-range font sizes from disk clamp into [MIN, MAX].
    #[test]
    fn font_size_out_of_range_clamps_on_load() {
        let zero = load_from_str(r#"{ "terminal": { "font_size_px": 0 } }"#);
        assert_eq!(zero.terminal.font_size_px, FONT_SIZE_PX_MIN);
        assert_eq!(FONT_SIZE_PX_MIN, 8);

        let huge = load_from_str(r#"{ "terminal": { "font_size_px": 9999 } }"#);
        assert_eq!(huge.terminal.font_size_px, FONT_SIZE_PX_MAX);
        assert_eq!(FONT_SIZE_PX_MAX, 32);
    }

    /// `normalize` runs on the write path too, so a bad in-memory value
    /// (e.g. round-tripped by the modal) is clamped before it hits disk.
    #[test]
    fn save_clamps_out_of_range_font_size_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let mut cfg = UserConfig::default();
        cfg.terminal.font_size_px = 9999;
        cfg.save(&path).unwrap();

        let reloaded = UserConfig::load_or_default(&path);
        assert_eq!(reloaded.terminal.font_size_px, FONT_SIZE_PX_MAX);

        cfg.terminal.font_size_px = 0;
        cfg.save(&path).unwrap();
        let reloaded = UserConfig::load_or_default(&path);
        assert_eq!(reloaded.terminal.font_size_px, FONT_SIZE_PX_MIN);
    }

    /// Missing file → full default (existing behaviour preserved).
    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert_eq!(UserConfig::load_or_default(&path), UserConfig::default());
    }

    /// Not-valid-JSON-at-all → full default (existing behaviour preserved).
    #[test]
    fn garbage_json_returns_default() {
        let cfg = load_from_str("{ this is not json");
        assert_eq!(cfg, UserConfig::default());
    }

    /// A single bad palette entry resets the whole palette (ANSI indices
    /// must stay aligned) but leaves sibling fields intact.
    #[test]
    fn bad_palette_entry_resets_whole_palette_only() {
        let cfg = load_from_str(
            r##"{
                "terminal": {
                    "font_size_px": 20,
                    "palette": ["#000000", "nope", "#222222"]
                }
            }"##,
        );
        assert_eq!(cfg.terminal.palette, default_ansi_palette());
        assert_eq!(cfg.terminal.font_size_px, 20);
    }

    /// A wrong-typed boolean (string where a bool is expected) resets only
    /// that flag — it must NOT fail the whole `terminal` section.
    #[test]
    fn wrong_typed_bool_preserves_other_fields() {
        let cfg = load_from_str(
            r##"{ "terminal": { "font_family": "Keep", "cursor_blink": "yes" } }"##,
        );
        assert_eq!(cfg.terminal.font_family, "Keep");
        assert!(!cfg.terminal.cursor_blink, "bad bool falls back to default");
    }

    /// A non-array palette (wrong shape entirely) resets only the palette.
    #[test]
    fn non_array_palette_resets_only_palette() {
        let cfg =
            load_from_str(r##"{ "terminal": { "font_size_px": 20, "palette": "nope" } }"##);
        assert_eq!(cfg.terminal.palette, default_ansi_palette());
        assert_eq!(cfg.terminal.font_size_px, 20);
    }

    /// A wrong-typed top-level flag (behavior bool as a number) resets only
    /// that flag.
    #[test]
    fn wrong_typed_behavior_bool_resets_only_that_flag() {
        let cfg = load_from_str(
            r##"{ "behavior": { "auto_spawn_on_workspace_open": 1, "save_scrollback_lines": 4321 } }"##,
        );
        assert!(cfg.behavior.auto_spawn_on_workspace_open, "bad bool → default true");
        assert_eq!(cfg.behavior.save_scrollback_lines, 4321);
    }
}

//! Maps `wezterm_term::ColorAttribute` → `tessera_render::Color`.
//!
//! - Default fg/bg come from the Tessera DESIGN.md palette.
//! - PaletteIndex 0..15 are the xterm 16-color names.
//! - PaletteIndex 16..231 are the xterm 6×6×6 color cube.
//! - PaletteIndex 232..255 are the xterm grayscale ramp.
//! - TrueColor passes through verbatim.

use tessera_render::geometry::Color;
use wezterm_term::color::ColorAttribute;

#[derive(Debug, Clone)]
pub struct ColorPalette {
    pub default_fg: Color,
    pub default_bg: Color,
    /// Indexed 0..255 — xterm extended palette.
    pub table: [Color; 256],
}

impl ColorPalette {
    /// Tessera warm-dark palette (see DESIGN.md):
    /// - default_fg = `--text-primary` #E8E8E6
    /// - default_bg = `--bg` #0F0F10
    /// - 0..15 = xterm 16-color names (standard, not tinted in v1 — terminal
    ///   colors are semantic; users running `ls --color` expect them recognisable).
    pub fn tessera_dark() -> Self {
        let mut table = [Color::rgb(0, 0, 0); 256];

        let xterm16: [Color; 16] = [
            Color::rgb(0x00, 0x00, 0x00), // 0 black
            Color::rgb(0xCD, 0x00, 0x00), // 1 red
            Color::rgb(0x00, 0xCD, 0x00), // 2 green
            Color::rgb(0xCD, 0xCD, 0x00), // 3 yellow
            Color::rgb(0x00, 0x00, 0xEE), // 4 blue
            Color::rgb(0xCD, 0x00, 0xCD), // 5 magenta
            Color::rgb(0x00, 0xCD, 0xCD), // 6 cyan
            Color::rgb(0xE5, 0xE5, 0xE5), // 7 white
            Color::rgb(0x7F, 0x7F, 0x7F), // 8 bright black
            Color::rgb(0xFF, 0x00, 0x00), // 9 bright red
            Color::rgb(0x00, 0xFF, 0x00), // 10 bright green
            Color::rgb(0xFF, 0xFF, 0x00), // 11 bright yellow
            Color::rgb(0x5C, 0x5C, 0xFF), // 12 bright blue
            Color::rgb(0xFF, 0x00, 0xFF), // 13 bright magenta
            Color::rgb(0x00, 0xFF, 0xFF), // 14 bright cyan
            Color::rgb(0xFF, 0xFF, 0xFF), // 15 bright white
        ];
        for (i, c) in xterm16.iter().enumerate() {
            table[i] = *c;
        }

        // 16..231: 6×6×6 cube. Levels: 0, 95, 135, 175, 215, 255.
        let levels: [u8; 6] = [0, 95, 135, 175, 215, 255];
        for i in 0..216 {
            let r = levels[(i / 36) % 6];
            let g = levels[(i / 6) % 6];
            let b = levels[i % 6];
            table[16 + i] = Color::rgb(r, g, b);
        }

        // 232..255: grayscale ramp 0x08..0xEE step 0x0A.
        for i in 0..24 {
            let v = 8 + 10 * (i as u8);
            table[232 + i] = Color::rgb(v, v, v);
        }

        ColorPalette {
            default_fg: Color::rgb(0xE8, 0xE8, 0xE6),
            default_bg: Color::rgb(0x0F, 0x0F, 0x10),
            table,
        }
    }

    /// Build a palette from user-supplied overrides: default fg/bg + the
    /// first 16 ANSI slots. Slots 16..=255 (the 6×6×6 cube and the
    /// grayscale ramp) keep their `tessera_dark` values — those are
    /// algorithmic and not worth exposing for editing.
    ///
    /// `ansi_16` must contain exactly 16 entries; anything else is
    /// silently truncated/padded with the matching tessera_dark slot, so
    /// a caller can pass a Vec built from user settings without crashing
    /// on a malformed config.
    pub fn from_user(default_fg: Color, default_bg: Color, ansi_16: &[Color]) -> Self {
        let mut p = Self::tessera_dark();
        p.default_fg = default_fg;
        p.default_bg = default_bg;
        let n = ansi_16.len().min(16);
        p.table[..n].copy_from_slice(&ansi_16[..n]);
        p
    }

    /// Resolve a foreground `ColorAttribute` against this palette.
    pub fn resolve_fg(&self, attr: &ColorAttribute) -> Color {
        match attr {
            ColorAttribute::Default => self.default_fg,
            ColorAttribute::PaletteIndex(idx) => self.table[*idx as usize],
            ColorAttribute::TrueColorWithDefaultFallback(srgb)
            | ColorAttribute::TrueColorWithPaletteFallback(srgb, _) => Color::rgb(
                (srgb.0 * 255.0) as u8,
                (srgb.1 * 255.0) as u8,
                (srgb.2 * 255.0) as u8,
            ),
        }
    }

    /// Resolve a background `ColorAttribute` against this palette.
    pub fn resolve_bg(&self, attr: &ColorAttribute) -> Color {
        match attr {
            ColorAttribute::Default => self.default_bg,
            ColorAttribute::PaletteIndex(idx) => self.table[*idx as usize],
            ColorAttribute::TrueColorWithDefaultFallback(srgb)
            | ColorAttribute::TrueColorWithPaletteFallback(srgb, _) => Color::rgb(
                (srgb.0 * 255.0) as u8,
                (srgb.1 * 255.0) as u8,
                (srgb.2 * 255.0) as u8,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fg_matches_design_system() {
        let p = ColorPalette::tessera_dark();
        assert_eq!(p.default_fg, Color::rgb(0xE8, 0xE8, 0xE6));
    }

    #[test]
    fn xterm_red_is_dark_red() {
        let p = ColorPalette::tessera_dark();
        let red = p.resolve_fg(&ColorAttribute::PaletteIndex(1));
        assert_eq!(red, Color::rgb(0xCD, 0x00, 0x00));
    }

    #[test]
    fn xterm_color_cube_corner_is_white() {
        let p = ColorPalette::tessera_dark();
        // Index 16 + 5*36 + 5*6 + 5 = 16 + 215 = 231 = bottom-right of cube = (255,255,255).
        let white = p.resolve_fg(&ColorAttribute::PaletteIndex(231));
        assert_eq!(white, Color::rgb(255, 255, 255));
    }

    #[test]
    fn grayscale_ramp_first_is_dark() {
        let p = ColorPalette::tessera_dark();
        let gray = p.resolve_fg(&ColorAttribute::PaletteIndex(232));
        assert_eq!(gray, Color::rgb(8, 8, 8));
    }
}

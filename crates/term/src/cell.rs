//! `GridCell` — a flat, ready-to-render representation of one terminal cell.

use crate::palette::ColorPalette;
use tessera_render::geometry::Color;
use unicode_normalization::UnicodeNormalization;
use wezterm_term::Cell as WezCell;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridCell {
    /// Primary scalar char of the cell's grapheme. For multi-codepoint clusters
    /// (e.g. emoji + skin-tone), the first base codepoint. Future T2 work may
    /// promote this to a `&str` slice referencing a grapheme arena.
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Whether the cell wants a double-underline (SGR 21). Distinct from
    /// `underline` so the renderer can match Warp's
    /// `Flags::DOUBLE_UNDERLINE` branch and draw a 2×-thickness rect.
    pub double_underline: bool,
    /// SGR 9 strikethrough.
    pub strikethrough: bool,
}

impl GridCell {
    /// Resolve a wezterm `Cell` against the given palette.
    ///
    /// Applies NFKC compatibility normalization: superscript / modifier-letter
    /// codepoints (e.g. U+2071 `ⁱ`, U+1D49 `ᵉ`, U+02B7 `ʷ`) fold to their
    /// base ASCII forms. Without this, Claude Code's TUI styling — which uses
    /// these codepoints as decorative "small text" — renders unreadably tiny.
    pub fn from_wez(cell: &WezCell, palette: &ColorPalette) -> Self {
        let s = cell.str();
        // Fast path: pure ASCII bytes are NFKC-identical, skip the
        // normalization machinery entirely. 1920 cells × NFKC per snapshot
        // was eating ~200-500 µs of `snap` time; almost every cell in a
        // typical claude-code TUI is ASCII (block-drawing chars + box
        // glyphs that live in the > U+007F range are the exception).
        // `len() == 1` is a sufficient test in UTF-8: every single-byte
        // UTF-8 sequence is by definition ASCII (codepoints 0x00–0x7F).
        // We still assert it in debug to catch a hypothetical wezterm-term
        // 8-bit / Latin-1 mode that hands back a raw high byte.
        let ch = if s.len() == 1 {
            debug_assert!(s.is_ascii(), "single-byte cell str must be ASCII; got {s:?}");
            s.as_bytes()[0] as char
        } else {
            s.nfkc().next().unwrap_or(' ')
        };
        let a = cell.attrs();
        Self {
            ch,
            fg: palette.resolve_fg(&a.foreground()),
            bg: palette.resolve_bg(&a.background()),
            bold: matches!(a.intensity(), wezterm_term::Intensity::Bold),
            italic: a.italic(),
            underline: matches!(
                a.underline(),
                wezterm_term::Underline::Single
                    | wezterm_term::Underline::Curly
                    | wezterm_term::Underline::Dotted
                    | wezterm_term::Underline::Dashed
            ),
            double_underline: matches!(a.underline(), wezterm_term::Underline::Double),
            strikethrough: a.strikethrough(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wezterm_term::CellAttributes;

    #[test]
    fn ascii_letter_has_default_colors() {
        let cell = WezCell::new('M', CellAttributes::default());
        let pal = ColorPalette::tessera_dark();
        let g = GridCell::from_wez(&cell, &pal);
        assert_eq!(g.ch, 'M');
        assert_eq!(g.fg, Color::rgb(0xE8, 0xE8, 0xE6));
        assert_eq!(g.bg, Color::rgb(0x0F, 0x0F, 0x10));
        assert!(!g.bold);
        assert!(!g.italic);
        assert!(!g.underline);
    }

    #[test]
    fn superscript_modifier_letters_fold_to_ascii() {
        let pal = ColorPalette::tessera_dark();
        // U+2071 SUPERSCRIPT LATIN SMALL LETTER I → 'i'
        let cell = WezCell::new('\u{2071}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch, 'i');
        // U+1D49 MODIFIER LETTER SMALL E → 'e'
        let cell = WezCell::new('\u{1D49}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch, 'e');
        // U+02B7 MODIFIER LETTER SMALL W → 'w'
        let cell = WezCell::new('\u{02B7}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch, 'w');
        // U+1D39 MODIFIER LETTER CAPITAL M (small-caps style) → 'M'
        let cell = WezCell::new('\u{1D39}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch, 'M');
    }

    #[test]
    fn cyrillic_passes_through_unchanged() {
        let pal = ColorPalette::tessera_dark();
        let cell = WezCell::new('Ф', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch, 'Ф');
    }

    #[test]
    fn bold_italic_underline_round_trip() {
        let mut a = CellAttributes::default();
        a.set_intensity(wezterm_term::Intensity::Bold);
        a.set_italic(true);
        a.set_underline(wezterm_term::Underline::Single);
        let cell = WezCell::new('B', a);
        let g = GridCell::from_wez(&cell, &ColorPalette::tessera_dark());
        assert!(g.bold);
        assert!(g.italic);
        assert!(g.underline);
        assert!(!g.double_underline);
        assert!(!g.strikethrough);
    }

    #[test]
    fn double_underline_and_strikethrough_split() {
        let pal = ColorPalette::tessera_dark();
        let mut a = CellAttributes::default();
        a.set_underline(wezterm_term::Underline::Double);
        let cell = WezCell::new('X', a);
        let g = GridCell::from_wez(&cell, &pal);
        assert!(g.double_underline, "double underline should be tracked");
        assert!(!g.underline, "single-underline flag stays off for Double");

        let mut a = CellAttributes::default();
        a.set_strikethrough(true);
        let cell = WezCell::new('X', a);
        let g = GridCell::from_wez(&cell, &pal);
        assert!(g.strikethrough);
    }
}

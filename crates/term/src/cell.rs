//! `GridCell` — a flat, ready-to-render representation of one terminal cell.

use tessera_render::geometry::Color;
use wezterm_term::Cell as WezCell;
use crate::palette::ColorPalette;

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
}

impl GridCell {
    /// Resolve a wezterm `Cell` against the given palette.
    pub fn from_wez(cell: &WezCell, palette: &ColorPalette) -> Self {
        let s = cell.str();
        let ch = s.chars().next().unwrap_or(' ');
        let a = cell.attrs();
        Self {
            ch,
            fg: palette.resolve_fg(&a.foreground()),
            bg: palette.resolve_bg(&a.background()),
            bold: matches!(a.intensity(), wezterm_term::Intensity::Bold),
            italic: a.italic(),
            underline: !matches!(a.underline(), wezterm_term::Underline::None),
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
    }
}

//! `GridCell` — a flat, ready-to-render representation of one terminal cell.

use crate::color::Color;
use crate::palette::ColorPalette;
use serde::{Serialize, Serializer};
use unicode_normalization::UnicodeNormalization;
use wezterm_term::Cell as WezCell;

/// Inline, `Copy` grapheme cluster (UTF-8) for one cell. Holds the full
/// NFKC-normalized cluster — ZWJ emoji, regional-indicator flags, emoji +
/// skin-tone modifiers — without a per-cell heap allocation on the 1 ms
/// snapshot tick (which is why `GridCell`/`WireCell` can stay `Copy`).
/// Oversized clusters (rare 7-scalar family emoji that exceed `CAP` bytes)
/// truncate on a char boundary; the common case (ASCII, CJK, single emoji,
/// flags, skin-tone) fits comfortably.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Grapheme {
    len: u8,
    buf: [u8; Grapheme::CAP],
}

impl Grapheme {
    const CAP: usize = 27;

    pub fn from_char(c: char) -> Self {
        let mut buf = [0u8; Self::CAP];
        let len = c.encode_utf8(&mut buf).len() as u8;
        Self { len, buf }
    }

    /// Encode an iterator of chars (already NFKC-normalized by the caller)
    /// into the inline buffer, stopping at `CAP` on a char boundary. Empty
    /// input collapses to a single space so a cell always has a glyph.
    pub fn from_chars(chars: impl Iterator<Item = char>) -> Self {
        let mut buf = [0u8; Self::CAP];
        let mut len = 0usize;
        for c in chars {
            let need = c.len_utf8();
            if len + need > Self::CAP {
                break;
            }
            c.encode_utf8(&mut buf[len..]);
            len += need;
        }
        if len == 0 {
            return Self::from_char(' ');
        }
        Self {
            len: len as u8,
            buf,
        }
    }

    pub fn as_str(&self) -> &str {
        // Safe: `buf[..len]` is only ever filled via `char::encode_utf8`,
        // which writes whole UTF-8 sequences, so it is always valid UTF-8.
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or(" ")
    }
}

impl std::fmt::Debug for Grapheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl Serialize for Grapheme {
    /// Wire form is the cluster as a JSON string, so the Canvas2D frontend
    /// can `fillText` the whole grapheme (multi-codepoint emoji included).
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridCell {
    /// The cell's full grapheme cluster (NFKC-normalized).
    pub ch: Grapheme,
    /// On-screen column span: 1 for a normal cell, 2 for an East-Asian wide
    /// glyph / wide emoji. The renderer draws the glyph across `width`
    /// columns; the column(s) physically occupied by a wide cell's right
    /// half are emitted as separate blank slots so positional indexing stays
    /// aligned with the terminal's real columns.
    pub width: u8,
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
    /// A default blank cell (one space, default colors, width 1). Used to
    /// pad rows out to `cols` and to fill the column physically occupied by
    /// the right half of a wide glyph.
    pub fn blank(palette: &ColorPalette) -> Self {
        Self {
            ch: Grapheme::from_char(' '),
            width: 1,
            fg: palette.default_fg,
            bg: palette.default_bg,
            bold: false,
            italic: false,
            underline: false,
            double_underline: false,
            strikethrough: false,
        }
    }
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
            debug_assert!(
                s.is_ascii(),
                "single-byte cell str must be ASCII; got {s:?}"
            );
            Grapheme::from_char(s.as_bytes()[0] as char)
        } else {
            // Keep the NFKC fold (superscript / modifier letters → base ASCII)
            // but preserve the WHOLE cluster — ZWJ emoji, flags, skin-tone —
            // instead of collapsing to the first scalar.
            Grapheme::from_chars(s.nfkc())
        };
        let a = cell.attrs();
        Self {
            ch,
            // On-screen column span (1 normal, 2 for wide CJK / wide emoji).
            // The grid builder uses `CellRef::cell_index()` to gap-pad the
            // physical column(s) a wide glyph's right half occupies.
            width: cell.width().clamp(1, 2) as u8,
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
        assert_eq!(g.ch.as_str(), "M");
        assert_eq!(g.width, 1);
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
        assert_eq!(GridCell::from_wez(&cell, &pal).ch.as_str(), "i");
        // U+1D49 MODIFIER LETTER SMALL E → 'e'
        let cell = WezCell::new('\u{1D49}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch.as_str(), "e");
        // U+02B7 MODIFIER LETTER SMALL W → 'w'
        let cell = WezCell::new('\u{02B7}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch.as_str(), "w");
        // U+1D39 MODIFIER LETTER CAPITAL M (small-caps style) → 'M'
        let cell = WezCell::new('\u{1D39}', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch.as_str(), "M");
    }

    #[test]
    fn cyrillic_passes_through_unchanged() {
        let pal = ColorPalette::tessera_dark();
        let cell = WezCell::new('Ф', CellAttributes::default());
        assert_eq!(GridCell::from_wez(&cell, &pal).ch.as_str(), "Ф");
    }

    #[test]
    fn multi_codepoint_cluster_is_preserved() {
        // A ZWJ / skin-tone style cluster must survive whole, not collapse to
        // its first scalar. wezterm stores the full cluster as the cell str;
        // `from_chars(s.nfkc())` must round-trip it.
        let pal = ColorPalette::tessera_dark();
        let cluster = "👍\u{1F3FD}"; // thumbs-up + medium skin tone
        let cell = WezCell::new_grapheme(cluster, CellAttributes::default(), None);
        let g = GridCell::from_wez(&cell, &pal);
        assert_eq!(g.ch.as_str(), cluster);
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

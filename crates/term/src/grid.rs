//! `Grid<'_>` — a borrowed, read-only iterator interface onto the current
//! visible cells of a `Term`. Not stored across `feed()` calls — re-borrow
//! after each.

use crate::cell::GridCell;
use crate::palette::ColorPalette;
use wezterm_term::Terminal;

pub struct Grid<'a> {
    term: &'a Terminal,
    palette: &'a ColorPalette,
}

impl<'a> Grid<'a> {
    pub(crate) fn new(term: &'a Terminal, palette: &'a ColorPalette) -> Self {
        Self { term, palette }
    }

    pub fn cols(&self) -> usize {
        self.term.screen().physical_cols
    }
    pub fn rows(&self) -> usize {
        self.term.screen().physical_rows
    }

    /// Iterate visible rows top-to-bottom. Each item is a `Vec<GridCell>` of
    /// length `cols()`. Cells shorter than `cols()` are padded with default
    /// blanks so callers can rely on consistent row width.
    ///
    /// # wezterm API notes
    /// - `Screen::lines` is private and `Screen::visible_lines()` is
    ///   `#[cfg(test)]`-only in wezterm-term. We use `screen.phys_row(0)` to
    ///   find the first physical row of the visible window and call
    ///   `screen.lines_in_phys_range(start..start+rows)` which is always `pub`.
    /// - `Line::visible_cells()` yields `CellRef<'_>`, not `&Cell`. We call
    ///   `CellRef::as_cell()` to obtain an owned `Cell`, then pass `&cell`
    ///   to `GridCell::from_wez`.
    pub fn rows_iter<'b>(&'b self) -> impl Iterator<Item = Vec<GridCell>> + 'b {
        self.rows_iter_with_offset(0)
    }

    /// Like `rows_iter` but `offset_back` rows up into the scrollback
    /// history (0 = live viewport, larger = further back). Saturates at
    /// the top of the scrollback buffer — callers can pass a delta from
    /// the frontend without bounds-checking it. `scrollback_max()` reports
    /// how far up they can go.
    pub fn rows_iter_with_offset<'b>(
        &'b self,
        offset_back: usize,
    ) -> impl Iterator<Item = Vec<GridCell>> + 'b {
        let screen = self.term.screen();
        let cols = screen.physical_cols;
        let rows = screen.physical_rows;
        let palette = self.palette;
        let top = screen.phys_row(0);
        let start = top.saturating_sub(offset_back);
        screen
            .lines_in_phys_range(start..start + rows)
            .into_iter()
            .map(move |line| {
                let mut row: Vec<GridCell> = Vec::with_capacity(cols);
                for c in line.visible_cells() {
                    // Place each cell at its TRUE physical column. wezterm's
                    // `visible_cells()` omits the blank spacer that trails a
                    // wide glyph, so `cell_index()` can skip a column — gap-pad
                    // up to it so positional indexing stays aligned with the
                    // terminal's real columns (otherwise everything after a
                    // CJK/emoji glyph shifts left by one).
                    let idx = c.cell_index();
                    while row.len() < idx && row.len() < cols {
                        row.push(GridCell::blank(palette));
                    }
                    if row.len() >= cols {
                        break;
                    }
                    row.push(GridCell::from_wez(&c.as_cell(), palette));
                }
                while row.len() < cols {
                    row.push(GridCell::blank(palette));
                }
                row.truncate(cols);
                row
            })
    }

    /// Zero-allocation per-row variant of `rows_iter_with_offset`. Reuses
    /// `scratch` for every row — caller hands in one `Vec<GridCell>`, we
    /// refill it row-by-row and invoke `f` with a borrow. Used by the
    /// snapshot hot path (1 ms tick) to skip the `Vec::collect` per row
    /// that `rows_iter_with_offset` would otherwise force.
    pub fn for_each_row_with_offset<F>(
        &self,
        offset_back: usize,
        scratch: &mut Vec<GridCell>,
        mut f: F,
    ) where
        F: FnMut(&[GridCell]),
    {
        let screen = self.term.screen();
        let cols = screen.physical_cols;
        let rows = screen.physical_rows;
        let palette = self.palette;
        let top = screen.phys_row(0);
        let start = top.saturating_sub(offset_back);
        let blank = GridCell::blank(palette);
        for line in screen.lines_in_phys_range(start..start + rows) {
            scratch.clear();
            scratch.reserve(cols);
            for c in line.visible_cells() {
                // Gap-pad to the cell's true physical column so wide-glyph
                // spacer columns stay aligned — see `rows_iter_with_offset`.
                let idx = c.cell_index();
                while scratch.len() < idx && scratch.len() < cols {
                    scratch.push(blank);
                }
                if scratch.len() >= cols {
                    break;
                }
                scratch.push(GridCell::from_wez(&c.as_cell(), palette));
            }
            while scratch.len() < cols {
                scratch.push(blank);
            }
            scratch.truncate(cols);
            f(scratch);
        }
    }

    /// Maximum allowed `offset_back` — equals the number of physical rows
    /// of scrollback above the current viewport top.
    pub fn scrollback_max(&self) -> usize {
        self.term.screen().phys_row(0)
    }

    /// Convenience: collect the full grid into `Vec<Vec<GridCell>>`. Allocates;
    /// prefer `rows_iter` for hot paths.
    pub fn to_vec(&self) -> Vec<Vec<GridCell>> {
        self.rows_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Term;
    use std::io::Cursor;

    fn writer() -> Box<dyn std::io::Write + Send> {
        Box::new(Cursor::new(Vec::new()))
    }

    fn row_text(row: &[GridCell]) -> String {
        row.iter().map(|c| c.ch.as_str()).collect()
    }

    #[test]
    fn plain_ascii_lands_at_top_row() {
        let mut t = Term::new(20, 5, writer());
        t.feed(b"hello world");
        let pal = ColorPalette::tessera_dark();
        let grid = t.grid(&pal);
        let rows = grid.to_vec();
        assert_eq!(rows.len(), 5);
        assert!(row_text(&rows[0]).starts_with("hello world"));
    }

    #[test]
    fn newline_advances_row() {
        let mut t = Term::new(20, 5, writer());
        t.feed(b"line1\r\nline2");
        let pal = ColorPalette::tessera_dark();
        let rows = t.grid(&pal).to_vec();
        assert!(row_text(&rows[0]).starts_with("line1"));
        assert!(row_text(&rows[1]).starts_with("line2"));
    }

    #[test]
    fn wide_char_keeps_following_cells_aligned() {
        // Regression: a width-2 glyph must occupy its true two physical
        // columns so the next cell lands at its real column, not one to the
        // left. wezterm omits the trailing spacer of a wide cell, so without
        // gap-padding by cell_index the row collapses left.
        let mut t = Term::new(20, 3, writer());
        t.feed("世X".as_bytes());
        let pal = ColorPalette::tessera_dark();
        let rows = t.grid(&pal).to_vec();
        let row = &rows[0];
        assert_eq!(row.len(), 20, "row padded to cols");
        assert_eq!(row[0].ch.as_str(), "世");
        assert_eq!(row[0].width, 2, "wide glyph reports width 2");
        assert_eq!(row[1].ch.as_str(), " ", "wide glyph spacer column is blank");
        assert_eq!(row[2].ch.as_str(), "X", "X sits at its true physical col 2");
    }
}

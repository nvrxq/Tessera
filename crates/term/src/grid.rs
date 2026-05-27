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
                let mut row: Vec<GridCell> = line
                    .visible_cells()
                    .map(|c| GridCell::from_wez(&c.as_cell(), palette))
                    .collect();
                while row.len() < cols {
                    row.push(GridCell {
                        ch: ' ',
                        fg: palette.default_fg,
                        bg: palette.default_bg,
                        bold: false,
                        italic: false,
                        underline: false,
                        double_underline: false,
                        strikethrough: false,
                    });
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
        let blank = GridCell {
            ch: ' ',
            fg: palette.default_fg,
            bg: palette.default_bg,
            bold: false,
            italic: false,
            underline: false,
            double_underline: false,
            strikethrough: false,
        };
        for line in screen.lines_in_phys_range(start..start + rows) {
            scratch.clear();
            scratch.reserve(cols);
            for c in line.visible_cells() {
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
        row.iter().map(|c| c.ch).collect()
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
}

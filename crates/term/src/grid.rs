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
        let screen = self.term.screen();
        let cols = screen.physical_cols;
        let rows = screen.physical_rows;
        let palette = self.palette;
        // lines_in_phys_range is unconditionally pub; visible_lines() is cfg(test).
        let start = screen.phys_row(0);
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

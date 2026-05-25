//! The `Term` struct — owns a `wezterm_term::Terminal` and presents the
//! Tessera API.

use std::io::Write;
use wezterm_term::{Terminal, TerminalSize};

use crate::blocks::{BlockEvent, BlockHandler, BlockSink};
use crate::config::shared_config;

pub struct Term {
    inner: Terminal,
    blocks: BlockSink,
}

impl Term {
    /// Construct a new Term. `writer` is what user keystrokes will be sent to
    /// (typically the PTY master input). `cols` × `rows` is the initial grid
    /// size in cells.
    pub fn new(cols: u16, rows: u16, writer: Box<dyn Write + Send>) -> Self {
        let size = TerminalSize {
            cols: cols as usize,
            rows: rows as usize,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        };
        let mut inner = Terminal::new(
            size,
            shared_config(),
            "tessera",
            env!("CARGO_PKG_VERSION"),
            writer,
        );
        let blocks = BlockSink::new();
        inner.set_device_control_handler(Box::new(BlockHandler::new(&blocks)));
        Self { inner, blocks }
    }

    /// Drain and return any block events accumulated since the last call.
    pub fn take_block_events(&mut self) -> Vec<BlockEvent> {
        self.blocks.drain()
    }

    /// Feed PTY output bytes into the parser. May be partial sequences.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.inner.advance_bytes(bytes);
    }

    /// Resize the grid in cells. Pixel dimensions are derived later by the
    /// renderer; wezterm only cares about cell counts.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let new = TerminalSize {
            cols: cols as usize,
            rows: rows as usize,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 0,
        };
        self.inner.resize(new);
    }

    pub fn grid<'a>(&'a self, palette: &'a crate::palette::ColorPalette) -> crate::grid::Grid<'a> {
        crate::grid::Grid::new(&self.inner, palette)
    }

    pub fn cursor(&self) -> crate::cursor::CursorPos {
        let cp = self.inner.cursor_pos();
        // CursorVisibility is in wezterm_surface (not a direct dep); Default is
        // Visible, so equality with default() tells us the cursor is shown.
        let visible = cp.visibility == Default::default();
        crate::cursor::CursorPos {
            col: cp.x,
            row: cp.y.max(0) as usize,
            visible,
        }
    }

    pub fn cols(&self) -> usize {
        self.inner.screen().physical_cols
    }
    pub fn rows(&self) -> usize {
        self.inner.screen().physical_rows
    }

    // `pub(crate)` exposes the wezterm Terminal to sibling modules
    // (grid.rs, cell.rs, cursor.rs, blocks.rs) without leaking it externally.
    pub(crate) fn inner(&self) -> &Terminal {
        &self.inner
    }
    pub(crate) fn inner_mut(&mut self) -> &mut Terminal {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn writer() -> Box<dyn Write + Send> {
        Box::new(Cursor::new(Vec::new()))
    }

    #[test]
    fn new_term_has_requested_dimensions() {
        let t = Term::new(80, 24, writer());
        assert_eq!(t.cols(), 80);
        assert_eq!(t.rows(), 24);
    }

    #[test]
    fn feed_does_not_panic_on_partial_sequence() {
        let mut t = Term::new(80, 24, writer());
        // Half of a SGR sequence — wezterm must buffer the rest.
        t.feed(b"\x1b[1");
        t.feed(b";31mHello\x1b[0m");
        // No assert needed; just exercise the path.
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut t = Term::new(80, 24, writer());
        t.resize(120, 40);
        assert_eq!(t.cols(), 120);
        assert_eq!(t.rows(), 40);
    }

    #[test]
    fn cursor_moves_after_feed() {
        let mut t = Term::new(80, 24, writer());
        let before = t.cursor();
        t.feed(b"hello");
        let after = t.cursor();
        assert_eq!(before.col, 0);
        assert_eq!(after.col, 5);  // "hello" advances cursor 5 cells
        assert_eq!(after.row, 0);
    }
}

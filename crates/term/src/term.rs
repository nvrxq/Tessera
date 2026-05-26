//! The `Term` struct — owns a `wezterm_term::Terminal` and presents the
//! Tessera API.

use std::io::Write;
use wezterm_term::{Terminal, TerminalSize};

use crate::blocks::{Block, BlockEvent, BlockHandler, BlockSink};
use crate::config::shared_config;

pub struct Term {
    inner: Terminal,
    blocks: BlockSink,
    /// Resolved Start/End pairs, in chronological order. `current_block` is
    /// the index of the block currently being built (with `end_row=None`)
    /// when set.
    block_list: Vec<Block>,
    current_block: Option<usize>,
    /// Raw events accumulated since the last `take_block_events()` call.
    /// `feed()` now both materialises blocks into `block_list` AND buffers
    /// raw events here so the historical `take_block_events()` consumers
    /// (tests, future external observers) keep working.
    pending_events: Vec<BlockEvent>,
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
        Self {
            inner,
            blocks,
            block_list: Vec::new(),
            current_block: None,
            pending_events: Vec::new(),
        }
    }

    /// Drain and return any block events accumulated since the last call.
    /// Note: `feed()` also internally consumes events to build the resolved
    /// `blocks()` list; this method drains the parallel buffer kept
    /// specifically for external observers.
    pub fn take_block_events(&mut self) -> Vec<BlockEvent> {
        // Also drain anything that arrived in the sink between feeds (e.g.
        // a direct DCS poked in without an enclosing feed()).
        let extra = self.blocks.drain();
        self.pending_events.extend(extra);
        std::mem::take(&mut self.pending_events)
    }

    /// Materialised blocks for the current viewport — Start/End pairs the
    /// shell told us about, anchored to cursor-row positions captured at
    /// the moment each event arrived.
    pub fn blocks(&self) -> &[Block] {
        &self.block_list
    }

    /// Feed PTY output bytes into the parser. May be partial sequences.
    /// After byte advancement we materialise any pending block events into
    /// the persistent `block_list` using the post-feed cursor row.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.inner.advance_bytes(bytes);
        // Drain newly-arrived block events and resolve them with the cursor
        // row AS OF NOW. Reasoning: preexec fires AFTER the user pressed
        // Enter, so the shell has already advanced the cursor to a fresh
        // row below the prompt+command line by the time the DCS hits us.
        // precmd fires before the next prompt is drawn — cursor is on the
        // row immediately after the command's last output line.
        let events = self.blocks.drain();
        if events.is_empty() {
            return;
        }
        // Mirror events into the external-facing buffer so callers of
        // `take_block_events()` can still observe them.
        self.pending_events.extend(events.iter().cloned());
        let cur_row = self.inner.cursor_pos().y.max(0) as usize;
        for event in events {
            match event {
                BlockEvent::Start { id, command } => {
                    // start_row is the row above the cursor — that's the
                    // command-line itself (cursor moved down past it).
                    let start_row = cur_row.saturating_sub(1);
                    self.block_list.push(Block {
                        id: id.0,
                        command,
                        start_row,
                        end_row: None,
                        exit_code: None,
                    });
                    self.current_block = Some(self.block_list.len() - 1);
                }
                BlockEvent::End { id, exit_code } => {
                    if let Some(idx) = self.current_block {
                        if let Some(b) = self.block_list.get_mut(idx) {
                            if b.id == id.0 {
                                // end_row is row above cursor — last output
                                // line (cursor moved down to next prompt).
                                b.end_row = Some(cur_row.saturating_sub(1));
                                b.exit_code = exit_code;
                            }
                        }
                    }
                    self.current_block = None;
                }
            }
        }
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
        let visible = matches!(cp.visibility, wezterm_surface::CursorVisibility::Visible);
        // Map wezterm's 7-variant CursorShape onto our 3-variant enum. We
        // ignore blink for now (the overlay doesn't animate).
        let shape = match cp.shape {
            wezterm_surface::CursorShape::BlinkingBar | wezterm_surface::CursorShape::SteadyBar => {
                crate::cursor::CursorShape::Bar
            }
            wezterm_surface::CursorShape::BlinkingUnderline
            | wezterm_surface::CursorShape::SteadyUnderline => {
                crate::cursor::CursorShape::Underline
            }
            // Default / BlinkingBlock / SteadyBlock all → Block.
            _ => crate::cursor::CursorShape::Block,
        };
        crate::cursor::CursorPos {
            col: cp.x,
            row: cp.y.max(0) as usize,
            visible,
            shape,
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
    // Currently unused — block-tracking direction was abandoned — kept for
    // a future feature that needs direct wezterm access; cheap to leave.
    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &Terminal {
        &self.inner
    }
    #[allow(dead_code)]
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
        assert_eq!(after.col, 5); // "hello" advances cursor 5 cells
        assert_eq!(after.row, 0);
    }
}

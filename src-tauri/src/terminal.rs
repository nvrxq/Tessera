//! Backend ownership of `wezterm-term` parser per PTY session, plus
//! delta-encoded snapshot emission to the frontend.
//!
//! Each session keeps a copy of the last sent cell grid; new snapshots
//! diff against it and emit only changed cells. Single-keystroke echoes
//! → 1-3 cells changed → ~50× smaller JSON payload and ~1000× cheaper
//! Canvas2D repaint on the frontend.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tessera_term::{palette::ColorPalette, CursorShape, GridCell, Term};
use uuid::Uuid;

/// Compact wire form for one cell — sent inside `cells` whether the
/// snapshot is full or delta.
#[derive(Serialize, Copy, Clone, PartialEq, Eq)]
pub struct WireCell {
    pub c: char,
    pub f: u32,
    pub b: u32,
    pub a: u8,
}

#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub session_id: Uuid,
    pub cols: u16,
    pub rows: u16,
    /// `true` → `cells` is the full grid in row-major order (`cols·rows`
    /// entries, `positions` empty). `false` → only changed cells, with
    /// `positions[i]` giving the linear index of `cells[i]`.
    pub full: bool,
    pub cells: Vec<WireCell>,
    pub positions: Vec<u32>,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub cursor_visible: bool,
    pub cursor_shape: &'static str,
}

/// Per-session state held alongside the parser so we can diff successive
/// snapshots. `last_cells` is whatever we most recently sent; cleared on
/// resize to force the next snapshot to be `full`.
struct SessionState {
    term: Term,
    last_cells: Vec<WireCell>,
    last_cols: u16,
    last_rows: u16,
    /// How many rows up into scrollback we're rendering. 0 = live tail.
    /// Reset to 0 the moment new PTY bytes arrive (`feed`) so the live
    /// stream "snaps back" to current — that's how iTerm/Wezterm behave
    /// and avoids the "wait, I'm reading stale output" trap.
    scroll_offset: usize,
}

pub struct TerminalRegistry {
    inner: Mutex<HashMap<Uuid, SessionState>>,
    palette: ColorPalette,
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            palette: ColorPalette::tessera_dark(),
        }
    }

    pub fn feed(&self, sid: Uuid, cols: u16, rows: u16, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.entry(sid).or_insert_with(|| SessionState {
            term: Term::new(cols, rows, Box::new(DevNull)),
            last_cells: Vec::new(),
            last_cols: cols,
            last_rows: rows,
            scroll_offset: 0,
        });
        st.term.feed(bytes);
        // Snap back to live tail on every new chunk — see SessionState.
        if st.scroll_offset != 0 {
            st.scroll_offset = 0;
            st.last_cells.clear();
        }
        true
    }

    pub fn resize(&self, sid: Uuid, cols: u16, rows: u16) {
        if let Some(st) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&sid)
        {
            st.term.resize(cols, rows);
            // Force the next snapshot to be a full one — both ends need to
            // re-allocate their buffers to the new dimensions.
            st.last_cells.clear();
            st.last_cols = cols;
            st.last_rows = rows;
        }
    }

    pub fn remove(&self, sid: Uuid) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&sid);
    }

    /// Adjust the scrollback view by `delta_back` rows. Positive = move
    /// further back into history, negative = move toward the live tail.
    /// Clamped to `[0, scrollback_max]`. Clearing `last_cells` forces the
    /// next snapshot to be `full`, which is correct because the entire
    /// grid content shifts with scroll. Returns the resulting offset; the
    /// caller should re-emit a snapshot to repaint.
    pub fn set_scroll_delta(&self, sid: Uuid, delta_back: i32) -> usize {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(st) = map.get_mut(&sid) else {
            return 0;
        };
        let max = st.term.grid(&self.palette).scrollback_max();
        let want = (st.scroll_offset as i64) + delta_back as i64;
        let clamped = want.clamp(0, max as i64) as usize;
        if clamped != st.scroll_offset {
            st.scroll_offset = clamped;
            st.last_cells.clear();
        }
        clamped
    }

    /// Snapshot the current grid. Returns a delta against the previous
    /// snapshot when sizes match; otherwise a full grid. Updates the
    /// stored last-sent buffer in either case.
    pub fn snapshot(&self, sid: Uuid) -> Option<Snapshot> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.get_mut(&sid)?;
        let cols = st.term.cols() as u16;
        let rows = st.term.rows() as u16;
        let total = (cols as usize) * (rows as usize);
        let cur = st.term.cursor();

        // Build the current flat cell buffer. When `scroll_offset > 0` we
        // sample N rows back into the scrollback buffer; otherwise it's
        // the live viewport.
        let scroll_offset = st.scroll_offset;
        let mut current: Vec<WireCell> = Vec::with_capacity(total);
        {
            let grid = st.term.grid(&self.palette);
            for row in grid.rows_iter_with_offset(scroll_offset) {
                for cell in &row {
                    current.push(wire_cell(cell));
                }
            }
        }
        debug_assert_eq!(current.len(), total);

        let needs_full =
            st.last_cells.len() != total || cols != st.last_cols || rows != st.last_rows;

        let snap = if needs_full {
            // First snapshot or post-resize — send full grid. Keep our
            // last_cells in sync by cloning the wire vec *once* into
            // `st.last_cells` and moving `current` into the outgoing
            // Snapshot (Tauri emit takes Snapshot by value). The opposite
            // direction (move into last_cells, clone into Snapshot) would
            // double-allocate 1920 cells × ~11 bytes for every full snap.
            st.last_cells = current.clone();
            st.last_cols = cols;
            st.last_rows = rows;
            Snapshot {
                session_id: sid,
                cols,
                rows,
                full: true,
                cells: current,
                positions: Vec::new(),
                cursor_col: cur.col,
                cursor_row: cur.row,
                cursor_visible: cur.visible && scroll_offset == 0,
                cursor_shape: cursor_shape(&cur.shape),
            }
        } else {
            // Diff: collect (index, cell) for cells that changed.
            let mut positions: Vec<u32> = Vec::new();
            let mut cells: Vec<WireCell> = Vec::new();
            for (i, (new, old)) in current.iter().zip(st.last_cells.iter()).enumerate() {
                if new != old {
                    positions.push(i as u32);
                    cells.push(*new);
                }
            }
            st.last_cells = current;
            st.last_cols = cols;
            st.last_rows = rows;
            Snapshot {
                session_id: sid,
                cols,
                rows,
                full: false,
                cells,
                positions,
                cursor_col: cur.col,
                cursor_row: cur.row,
                cursor_visible: cur.visible && scroll_offset == 0,
                cursor_shape: cursor_shape(&cur.shape),
            }
        };

        Some(snap)
    }
}

fn cursor_shape(s: &CursorShape) -> &'static str {
    match s {
        CursorShape::Block => "block",
        CursorShape::Bar => "bar",
        CursorShape::Underline => "underline",
    }
}

fn wire_cell(c: &GridCell) -> WireCell {
    let f = ((c.fg.r as u32) << 16) | ((c.fg.g as u32) << 8) | (c.fg.b as u32);
    let b = ((c.bg.r as u32) << 16) | ((c.bg.g as u32) << 8) | (c.bg.b as u32);
    let mut a = 0u8;
    if c.bold {
        a |= 1;
    }
    if c.italic {
        a |= 2;
    }
    if c.underline {
        a |= 4;
    }
    if c.double_underline {
        a |= 8;
    }
    if c.strikethrough {
        a |= 16;
    }
    WireCell { c: c.ch, f, b, a }
}

struct DevNull;
impl std::io::Write for DevNull {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

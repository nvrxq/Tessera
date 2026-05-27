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
use tessera_core::{HexColor, UserConfig};
use tessera_term::{palette::ColorPalette, Color, CursorShape, GridCell, Term};
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
    /// Reusable scratch buffer for the current tick's wire cells. Kept on
    /// the session so we don't allocate a fresh `Vec<WireCell>` (1920 cap at
    /// 80×24) every 1 ms render tick. Cleared at the top of each
    /// `snapshot()` and refilled in place.
    scratch_cells: Vec<WireCell>,
    /// Reusable scratch buffer for one row of `GridCell`s. Passed into
    /// `rows_iter_with_offset_into` so the grid iterator can refill it per
    /// row instead of allocating a fresh `Vec<GridCell>` for every row of
    /// every tick.
    scratch_row: Vec<GridCell>,
}

pub struct TerminalRegistry {
    inner: Mutex<HashMap<Uuid, SessionState>>,
    /// Wrapped so the settings layer can hot-swap the palette without
    /// rebuilding the registry. Snapshots clone-by-ref through the mutex.
    palette: Mutex<ColorPalette>,
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
            palette: Mutex::new(ColorPalette::tessera_dark()),
        }
    }

    /// Replace the active palette and force every live session to emit a
    /// full snapshot on its next sample so the new colours actually paint.
    /// Called from `settings_save` after the user changes the palette in
    /// the Settings modal.
    pub fn set_palette(&self, palette: ColorPalette) {
        *self.palette.lock().unwrap_or_else(|e| e.into_inner()) = palette;
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for st in map.values_mut() {
            st.last_cells.clear();
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
            scratch_cells: Vec::new(),
            scratch_row: Vec::new(),
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
        // No palette lock — `scrollback_max` only reads `screen().phys_row(0)`.
        let max = st.term.scrollback_max();
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
    ///
    /// Allocation policy on the 1 ms render-tick hot path:
    /// - `scratch_cells` is reused tick-to-tick — `clear()` + `extend()`
    ///   keeps the underlying capacity, so we don't re-allocate 1920 entries
    ///   per tick.
    /// - `scratch_row` is reused row-to-row inside one tick — same trick.
    /// - The delta-case `positions` / `cells` Vecs are necessarily fresh
    ///   per emit (Tauri's serializer moves them by value), but they're
    ///   typically tiny on a keystroke (1–3 cells).
    pub fn snapshot(&self, sid: Uuid) -> Option<Snapshot> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = map.get_mut(&sid)?;
        let cols = st.term.cols() as u16;
        let rows = st.term.rows() as u16;
        let total = (cols as usize) * (rows as usize);
        let cur = st.term.cursor();

        // Build the current flat cell buffer into the reusable scratch.
        // When `scroll_offset > 0` we sample N rows back into the scrollback
        // buffer; otherwise it's the live viewport.
        //
        // Borrow choreography: the row callback needs `&mut scratch_cells`
        // AND we have to hand `for_each_row_with_offset` a `&mut scratch_row`
        // — two disjoint fields of `*st`. Split them up-front so the closure
        // only captures `scratch_cells`. NLL drops both before the if/else
        // below touches `st.scratch_cells` again via `mem::swap`.
        let scroll_offset = st.scroll_offset;
        {
            let scratch_cells = &mut st.scratch_cells;
            let scratch_row = &mut st.scratch_row;
            let term = &st.term;
            scratch_cells.clear();
            scratch_cells.reserve(total);
            let palette = self.palette.lock().unwrap_or_else(|e| e.into_inner());
            let grid = term.grid(&palette);
            grid.for_each_row_with_offset(scroll_offset, scratch_row, |row| {
                for cell in row.iter() {
                    scratch_cells.push(wire_cell(cell));
                }
            });
        }
        debug_assert_eq!(st.scratch_cells.len(), total);

        let needs_full =
            st.last_cells.len() != total || cols != st.last_cols || rows != st.last_rows;

        let snap = if needs_full {
            // First snapshot or post-resize — send full grid. We have to
            // hand a fresh `Vec<WireCell>` to Tauri (serializer moves by
            // value), so allocate it once here and copy from scratch. Keep
            // `last_cells` synced by swapping it with the scratch buffer:
            // the scratch's allocation becomes the next `last_cells`, and
            // the old `last_cells` (with its capacity intact) becomes the
            // next tick's scratch. Either way, the *next* tick doesn't
            // allocate, only this first-snapshot tick does.
            let mut out: Vec<WireCell> = Vec::with_capacity(total);
            out.extend_from_slice(&st.scratch_cells);
            std::mem::swap(&mut st.last_cells, &mut st.scratch_cells);
            st.last_cols = cols;
            st.last_rows = rows;
            Snapshot {
                session_id: sid,
                cols,
                rows,
                full: true,
                cells: out,
                positions: Vec::new(),
                cursor_col: cur.col,
                cursor_row: cur.row,
                cursor_visible: cur.visible && scroll_offset == 0,
                cursor_shape: cursor_shape(&cur.shape),
            }
        } else {
            // Diff: collect (index, cell) for cells that changed. These
            // Vecs are intentionally fresh each tick — they're typically
            // 1–3 entries on a keystroke and get moved into the outgoing
            // Snapshot anyway.
            let mut positions: Vec<u32> = Vec::new();
            let mut cells: Vec<WireCell> = Vec::new();
            for (i, (new, old)) in st
                .scratch_cells
                .iter()
                .zip(st.last_cells.iter())
                .enumerate()
            {
                if new != old {
                    positions.push(i as u32);
                    cells.push(*new);
                }
            }
            // Swap roles: scratch_cells holds the just-built grid → it
            // becomes the new `last_cells`. The old `last_cells` (capacity
            // intact) goes back to being scratch for the next tick.
            std::mem::swap(&mut st.last_cells, &mut st.scratch_cells);
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

/// Translate the user-facing settings into a `ColorPalette`. Invalid hex
/// would have been rejected at `UserConfig` deserialise time, so every
/// string here is known to be `#RRGGBB`.
pub fn palette_from_config(cfg: &UserConfig) -> ColorPalette {
    let bg = parse_hex(&cfg.terminal.background);
    let fg = parse_hex(&cfg.terminal.foreground);
    let ansi: Vec<Color> = cfg.terminal.palette.iter().map(parse_hex).collect();
    ColorPalette::from_user(fg, bg, &ansi)
}

fn parse_hex(c: &HexColor) -> Color {
    let s = c.as_str();
    // safe: HexColor's deserializer guarantees `#RRGGBB`.
    let r = u8::from_str_radix(&s[1..3], 16).unwrap_or(0);
    let g = u8::from_str_radix(&s[3..5], 16).unwrap_or(0);
    let b = u8::from_str_radix(&s[5..7], 16).unwrap_or(0);
    Color::rgb(r, g, b)
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

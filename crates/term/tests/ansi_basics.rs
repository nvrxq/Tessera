//! Drive `Term` with raw ANSI fixtures and assert grid + cursor behavior.

use std::io::Cursor;
use tessera_term::{palette::ColorPalette, Color, Term};

fn writer() -> Box<dyn std::io::Write + Send> {
    Box::new(Cursor::new(Vec::new()))
}

fn row_string(t: &Term, pal: &ColorPalette, row: usize) -> String {
    let grid = t.grid(pal);
    let rows = grid.to_vec();
    rows[row].iter().map(|c| c.ch).collect()
}

#[test]
fn cursor_home_then_clear_screen_resets_grid() {
    let mut t = Term::new(20, 5, writer());
    let pal = ColorPalette::tessera_dark();
    t.feed(b"some text\r\nline 2");
    assert!(row_string(&t, &pal, 0).starts_with("some text"));

    // ESC[H = cursor home, ESC[2J = clear entire screen
    t.feed(b"\x1b[H\x1b[2J");
    let after = row_string(&t, &pal, 0);
    assert_eq!(
        after.trim_end(),
        "",
        "row 0 should be blank after clear, got {after:?}"
    );
    let cur = t.cursor();
    assert_eq!((cur.col, cur.row), (0, 0));
}

#[test]
fn sgr_31_makes_text_red() {
    let mut t = Term::new(20, 5, writer());
    let pal = ColorPalette::tessera_dark();
    t.feed(b"\x1b[31mRED\x1b[0mWHITE");
    let rows = t.grid(&pal).to_vec();
    let row0 = &rows[0];
    assert_eq!(row0[0].fg, Color::rgb(0xCD, 0x00, 0x00));
    assert_eq!(row0[3].fg, pal.default_fg); // 'W' is default after reset
}

#[test]
fn cursor_position_responds_to_cup() {
    let mut t = Term::new(20, 5, writer());
    // ESC[5;10H = CUP to row 5, col 10 (1-based)
    t.feed(b"\x1b[5;10H");
    let c = t.cursor();
    assert_eq!((c.col, c.row), (9, 4)); // 0-based internally
}

#[test]
fn truecolor_sgr_resolves_to_exact_rgb() {
    let mut t = Term::new(20, 5, writer());
    let pal = ColorPalette::tessera_dark();
    t.feed(b"\x1b[38;2;200;130;91mX\x1b[0m"); // accent terracotta
    let rows = t.grid(&pal).to_vec();
    assert_eq!(rows[0][0].fg, Color::rgb(200, 130, 91));
}

#[test]
fn resize_preserves_existing_content() {
    let mut t = Term::new(20, 5, writer());
    let pal = ColorPalette::tessera_dark();
    t.feed(b"abc");
    t.resize(40, 10);
    let row0 = row_string(&t, &pal, 0);
    assert!(row0.starts_with("abc"));
    assert_eq!(t.cols(), 40);
    assert_eq!(t.rows(), 10);
}

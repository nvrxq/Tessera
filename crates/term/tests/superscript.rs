//! Verify the full path: wezterm parse → grid → NFKC fold.
use std::io::{Cursor, Write};
use tessera_term::{palette::ColorPalette, Term};

fn writer() -> Box<dyn Write + Send> {
    Box::new(Cursor::new(Vec::new()))
}

#[test]
fn modifier_letters_fold_to_ascii_through_full_pipe() {
    let mut t = Term::new(40, 4, writer());
    // "ᴮ⁵: ᴾᵉⁿᵗᵉˢᵗ" = U+1D2E U+2075 U+003A U+0020 U+1D3E U+1D49 U+207F U+1D57 U+1D49 U+02E2 U+1D57
    let bytes = "ᴮ⁵: ᴾᵉⁿᵗᵉˢᵗ".as_bytes();
    t.feed(bytes);
    let pal = ColorPalette::tessera_dark();
    let rows = t.grid(&pal).to_vec();
    let row0: String = rows[0].iter().map(|c| c.ch).collect();
    let trimmed = row0.trim_end();
    eprintln!("ROW0: {trimmed:?}");
    for c in trimmed.chars() {
        let cp = c as u32;
        eprintln!("  ch=U+{cp:04X} {c:?}");
    }
    assert_eq!(trimmed, "B5: Pentest");
}

//! Cursor position + shape in cell coordinates (top-left origin).

/// The three primary cursor shapes a TUI can request via the
/// `CSI Ps SP q` (DECSCUSR) escape sequence:
///   - Block:      solid filled rect over the cell
///   - Bar:        thin vertical line at the cell's left edge
///   - Underline:  thin horizontal line at the cell's bottom edge
///
/// Maps `wezterm_surface::CursorShape` down — we collapse Blinking* and
/// Steady* into the same variant because the overlay does not (yet) animate
/// the cursor.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CursorPos {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
    pub shape: CursorShape,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_eq_round_trips() {
        let a = CursorPos {
            col: 4,
            row: 2,
            visible: true,
            shape: CursorShape::Block,
        };
        assert_eq!(
            a,
            CursorPos {
                col: 4,
                row: 2,
                visible: true,
                shape: CursorShape::Block
            }
        );
        assert_ne!(
            a,
            CursorPos {
                col: 4,
                row: 3,
                visible: true,
                shape: CursorShape::Block
            }
        );
        assert_ne!(
            a,
            CursorPos {
                col: 4,
                row: 2,
                visible: true,
                shape: CursorShape::Bar
            }
        );
    }
}

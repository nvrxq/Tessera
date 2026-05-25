//! Cursor position in cell coordinates (top-left origin).

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CursorPos {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_eq_round_trips() {
        let a = CursorPos { col: 4, row: 2, visible: true };
        assert_eq!(a, CursorPos { col: 4, row: 2, visible: true });
        assert_ne!(a, CursorPos { col: 4, row: 3, visible: true });
    }
}

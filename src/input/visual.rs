use crate::db::models::Line;

/// Tracks the visual selection range (anchor..cursor).
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
}

impl SelectionState {
    pub fn new(line: usize) -> Self {
        Self {
            anchor_line: line,
            cursor_line: line,
        }
    }

    /// Returns (start, end) as an inclusive range, regardless of direction.
    pub fn range(&self) -> (usize, usize) {
        let start = self.anchor_line.min(self.cursor_line);
        let end = self.anchor_line.max(self.cursor_line);
        (start, end)
    }
}

/// A snapshot of state before a destructive action, for undo.
pub struct UndoEntry {
    pub db_lines: Vec<Line>,
    pub file_backup: Option<(String, String)>,
    pub cursor_line: usize,
}

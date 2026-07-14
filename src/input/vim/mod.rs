//! Pure modal-vim editor engine for the journal in-place editor. ZERO gtk4
//! deps — operates on a `String` buffer + char-index cursor so the full verb
//! set is unit-testable. See docs/superpowers/specs/2026-06-30-journal-vim-edit-design.md.

pub mod buffer;
pub mod motion;
pub mod textobject;
pub mod registers;
pub mod engine;
pub mod journal_doc;
pub mod highlight;

pub use engine::VimEngine;

/// The engine's GTK-independent input alphabet. Printable input arrives as
/// `Char`; control keys are named. `CtrlR` is vim redo (distinct from the
/// `R` rewrite key, which arrives as `Char('R')` and the engine maps to the
/// rewrite action).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimKey {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    CtrlR,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

/// What the engine asks the host (adapter) to do after a key.
// Not `Copy`: `CopyToClipboard` carries an owned `String`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    Nop,
    Save,
    SaveQuit,
    Cancel,
    CancelForce,
    OpenRewrite,
    /// Visual-mode `H` toggled an inline `<hi>` highlight over the selection. The
    /// engine has already mutated its buffer; the host re-mirrors + marks dirty.
    ToggleHighlight,
    /// Visual-mode `y` yanked the selection. The engine already stored it in the
    /// unnamed (or named) register; the host ALSO writes the carried text to the
    /// system clipboard so it can be pasted into other apps.
    CopyToClipboard(String),
}

/// Half-open char range `[start, end)` into the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Result of `VimEngine::handle_key`. The adapter mirrors `cursor`/`mode`/
/// `selection` to GTK and acts on `action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub buffer_changed: bool,
    pub cursor: usize,
    pub mode: Mode,
    pub selection: Option<Range>,
    pub action: EditorAction,
}

impl Outcome {
    pub fn nop(mode: Mode, cursor: usize) -> Self {
        Outcome {
            buffer_changed: false,
            cursor,
            mode,
            selection: None,
            action: EditorAction::Nop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_defaults_are_nop_normal() {
        let o = Outcome::nop(Mode::Normal, 0);
        assert_eq!(o.action, EditorAction::Nop);
        assert_eq!(o.mode, Mode::Normal);
        assert_eq!(o.cursor, 0);
        assert!(!o.buffer_changed);
        assert!(o.selection.is_none());
    }

    #[test]
    fn range_len_is_half_open() {
        assert_eq!(Range { start: 2, end: 5 }.len(), 3);
        assert_eq!(Range { start: 4, end: 4 }.len(), 0);
        assert!(Range { start: 4, end: 4 }.is_empty());
    }
}

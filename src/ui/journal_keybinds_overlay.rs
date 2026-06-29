use gtk4::prelude::*;

/// Static legend of the journal-overlay keybinds, shown over the journal overlay
/// via Ctrl+/ (replacing the footer hint). Mirrors `EchoKeybindsOverlay`.
pub struct JournalKeybindsOverlay {
    pub container: gtk4::Box,
    pub scrim: gtk4::Box,
}

/// (key, action) rows. Matches handle_journal_key + journal visual mode.
const BINDS: &[(&str, &str)] = &[
    ("j / q", "next block"),
    ("k / ,", "prev block"),
    ("g g / G", "first / last block"),
    ("Space / Tab", "read cursor block (TTS)"),
    ("a", "restart cursor block TTS"),
    ("A", "ask a new question"),
    ("E", "edit Q&A"),
    ("D", "delete Q&A"),
    ("c", "copy Q&A id"),
    ("Ctrl+n / Ctrl+p", "prev / next Q&A in band"),
    ("Alt+n / Alt+p", "prev / next scene"),
    ("Alt+w", "whole-work band"),
    ("Ctrl+\\", "pick a Q&A"),
    ("Alt+g", "gloss this passage"),
    ("Ctrl+g", "view gloss for passage"),
    ("Ctrl+Shift+J", "move Q&A to another band"),
    ("Shift+V", "visual select (y yank)"),
    ("Ctrl+j / Esc", "close"),
    ("Ctrl+/", "close this legend"),
];

impl JournalKeybindsOverlay {
    pub fn new() -> Self {
        let (container, scrim) =
            super::keybinds_legend::build_legend("Journal keybinds", BINDS);
        Self { container, scrim }
    }

    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.scrim);
        overlay.add_overlay(&self.container);
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.scrim.set_visible(false);
        self.container.set_visible(false);
    }
}

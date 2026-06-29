use gtk4::prelude::*;

/// Static legend of the journal-overlay keybinds, shown over the journal overlay
/// via Ctrl+/ (replacing the footer hint). Mirrors `EchoKeybindsOverlay`.
pub struct JournalKeybindsOverlay {
    pub container: gtk4::Box,
    pub scrim: gtk4::Box,
}

/// Grouped (key, action) rows. Matches handle_journal_key + journal visual mode.
const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / q", "next block"),
        ("k / ,", "prev block"),
        ("g g / G", "first / last block"),
        ("Ctrl+n / Ctrl+p", "prev / next Q&A in band"),
        ("Alt+n / Alt+p", "prev / next scene"),
        ("Alt+w", "whole-work band"),
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS", &[
        ("Space / Tab", "read cursor block (TTS)"),
        ("a", "restart cursor block TTS"),
    ]),
    ("Editing", &[
        ("A", "ask a new question"),
        ("E", "edit Q&A"),
        ("D", "delete Q&A"),
        ("c", "copy Q&A id"),
        ("Ctrl+Shift+J", "move Q&A to another band"),
    ]),
    ("Cross-reference", &[
        ("Ctrl+\\", "pick a Q&A"),
        ("Alt+g", "gloss this passage"),
        ("Ctrl+g", "view gloss for passage"),
    ]),
    ("Close", &[
        ("Ctrl+j / Esc", "close"),
        ("Ctrl+/", "close this legend"),
    ]),
];

impl JournalKeybindsOverlay {
    pub fn new() -> Self {
        let (container, scrim) =
            super::keybinds_legend::build_legend("Journal keybinds", GROUPS);
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

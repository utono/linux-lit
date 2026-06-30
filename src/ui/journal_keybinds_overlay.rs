//! Journal-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Journal keybinds";

/// Grouped (key, action) rows. Matches handle_journal_key + journal visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
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
        ("r", "ask a new question"),
        ("e", "edit Q&A"),
        ("u", "undo last edit (confirm)"),
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

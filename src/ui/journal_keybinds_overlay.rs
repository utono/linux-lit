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
        ("e", "edit Q&A in place (vim)"),
        ("u", "undo last saved edit (confirm)"),
        ("D", "delete Q&A"),
        ("c", "copy Q&A id"),
        ("Ctrl+Shift+J", "move Q&A to another band"),
    ]),
    ("Vim edit mode (after e)", &[
        ("h j k l / w b e / 0 ^ $", "motions"),
        ("g g / G / f t F T / %", "go top/end · find · match"),
        ("i a o / I A O", "insert / append / open line"),
        ("x dd D / cw ciw / r J ~", "delete · change · replace · join"),
        ("y p P / v V", "yank · put · visual"),
        ("H", "highlight selection (visual; toggles)"),
        ("u / Ctrl+R / .", "undo · redo · repeat"),
        (":w / :wq", "save / save & quit"),
        ("R", "ask Claude to rewrite"),
        (":q / Esc / :q!", "quit (warns if unsaved) · force"),
    ]),
    ("Cross-reference", &[
        ("Ctrl+\\", "pick a Q&A"),
        ("Alt+g", "gloss this passage"),
        ("Ctrl+g / Ctrl+j", "view gloss for passage"),
    ]),
    ("Close", &[
        ("Esc", "close"),
        ("Ctrl+/", "close this legend"),
    ]),
];

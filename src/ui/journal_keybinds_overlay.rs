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
        ("x / y", "next / prev page (this Q&A)"),
        ("g g / G", "first / last block"),
        ("Ctrl+n / Ctrl+p", "prev / next Q&A in band"),
        ("Alt+n / Alt+p", "prev / next scene"),
        ("Alt+s", "current scene band"),
        ("Alt+w", "whole-work band"),
        ("Alt+a", "author corpus band"),
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS", &[
        ("Space / Tab", "read cursor block (TTS)"),
        ("a", "restart cursor block TTS"),
    ]),
    ("Editing", &[
        ("r", "ask a new question"),
        ("R", "ask Claude to rewrite this Q&A"),
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
        ("Ctrl+v", "paste clipboard (also in the r ask prompt)"),
        (":w / :wq", "save / save & quit"),
        ("R", "ask Claude to rewrite"),
        (":q / Esc / :q!", "quit (warns if unsaved) · force"),
    ]),
    ("Cross-reference", &[
        ("Ctrl+\\", "pick a Q&A"),
        ("Alt+g", "gloss this passage"),
        ("Ctrl+g", "view gloss for passage"),
    ]),
    ("Close", &[
        ("Esc / Ctrl+j", "close → reader"),
        ("Ctrl+Shift+L", "save & quit app"),
        ("Ctrl+/", "close this legend"),
    ]),
];

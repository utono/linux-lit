//! Gloss-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Gloss keybinds";

/// Grouped (key, action) rows. Matches handle_gloss_key + the gloss visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / q", "next block"),
        ("k / ,", "prev block"),
        ("x / y", "next / prev page (this gloss)"),
        ("g g / G", "first / last block"),
        ("Alt+n / Alt+p", "prev / next gloss"),
        ("Ctrl+n / Ctrl+p", "next / prev passage"),
        ("Shift+V", "visual select (y yank → clipboard)"),
    ]),
    ("TTS / voice", &[
        ("a", "play / pause (MPV, same as main card)"),
        ("Ctrl+Space", "read cursor block (TTS)"),
        ("A", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all prose blocks"),
        ("l", "play / stop source verse TTS"),
        ("L", "pick voice for source reading"),
        ("v", "voice picker"),
        ("Ctrl+v", "cycle active voice"),
    ]),
    ("Editing", &[
        ("R", "ask Claude to rewrite this gloss"),
        ("Ctrl+Shift+n / Ctrl+Shift+p", "browse rewrite history (view-only)"),
        ("Ctrl+Shift+r", "restore the viewed revision"),
        ("e", "edit gloss in place (vim)"),
        ("u", "undo last edit (confirm)"),
        ("D", "delete current gloss"),
        ("c", "copy gloss id"),
    ]),
    ("Vim edit mode (after e)", &[
        ("h j k l / w b e / 0 ^ $", "motions"),
        ("g g / G / f t F T / %", "go top/end · find · match"),
        ("i a o / I A O", "insert / append / open line"),
        ("x dd D / cw ciw / r J ~", "delete · change · replace · join"),
        ("y p P / v V", "yank · put · visual"),
        ("H", "highlight selection (visual; toggles)"),
        ("u / Ctrl+R / .", "undo · redo · repeat"),
        ("Ctrl+v", "paste clipboard (also in ask prompts)"),
        (":w / :wq", "save / save & quit"),
        ("R", "ask Claude to rewrite"),
        (":q / Esc / :q!", "quit (warns if unsaved) · force"),
    ]),
    ("Journal", &[
        ("Alt+g", "glosses picker"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("\\", "cycle: → synopsis (same segment)"),
        ("Esc", "close (jump to source)"),
        ("Ctrl+Shift+L", "save & quit app"),
        ("Ctrl+/", "close this legend"),
    ]),
];

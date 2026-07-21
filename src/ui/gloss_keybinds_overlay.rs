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
        ("r", "vocab popup (rr toggles · r next word)"),
    ]),
    ("TTS / voice", &[
        ("a", "play / pause (MPV, same as main card)"),
        ("Space", "loop source audio from its start / pause"),
        ("Ctrl+Space", "play / stop cursor block TTS (synthesizes on miss)"),
        ("A", "restart cursor block TTS from start (synthesizes on miss)"),
        ("Shift+Space", "synthesize all prose blocks"),
        ("l", "play / stop source verse TTS"),
        ("L", "pick voice for source reading"),
        ("v", "voice picker"),
        ("Ctrl+v", "cycle active voice"),
    ]),
    ("Editing", &[
        ("C-r", "ask Claude to rewrite this gloss"),
        ("Ctrl+Shift+n / Ctrl+Shift+p", "browse rewrite history (view-only)"),
        ("Ctrl+Shift+r", "restore the viewed revision"),
        ("e", "edit gloss in place (vim)"),
        ("u", "undo last edit (confirm)"),
        ("D", "delete current gloss"),
        ("c", "copy gloss id"),
    ]),
    super::keybinds_legend::VIM_EDIT_GROUP,
    ("Journal", &[
        ("Alt+g", "glosses picker"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "mpv volume"),
        ("Ctrl+Alt+↑ / ↓", "TTS volume (saved)"),
        ("\\", "cycle: → synopsis (same segment)"),
        ("Ctrl+f", "search all Q&As / glosses"),
        ("Esc", "close vocab popup / close (jump to source)"),
        ("Ctrl+Shift+L", "save & quit app"),
        ("Ctrl+/", "close this legend"),
    ]),
];

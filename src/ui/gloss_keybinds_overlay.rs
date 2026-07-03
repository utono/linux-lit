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
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS / voice", &[
        ("Space / Tab", "read cursor block (TTS)"),
        ("a", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all prose blocks"),
        ("l", "play / stop source verse TTS"),
        ("L", "pick voice for source reading"),
        ("v", "voice picker"),
        ("Ctrl+v", "cycle active voice"),
    ]),
    ("Editing", &[
        ("e", "edit gloss in place (vim)"),
        ("v … H", "in editor: visual-select, H toggles highlight"),
        (":w / :q / R", "save · quit · ask-Claude rewrite (in editor)"),
        ("Ctrl+v", "paste clipboard (in editor / prompts)"),
        ("u", "undo last edit (confirm)"),
        ("D", "delete current gloss"),
        ("c", "copy gloss id"),
    ]),
    ("Journal", &[
        ("r", "new journal Q&A from passage"),
        ("Ctrl+j", "view journal for passage"),
        ("Alt+g", "glosses picker"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("Esc / n / Ctrl+g", "close (jump to source)"),
        ("Ctrl+Shift+L", "save & quit app"),
        ("Ctrl+/", "close this legend"),
    ]),
];

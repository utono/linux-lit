//! Synopsis-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Synopsis keybinds";

/// Grouped (key, action) rows. Matches handle_synopsis_overlay_key + visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / k", "next / prev block"),
        ("g g / G", "first / last block"),
        ("Ctrl+n / Ctrl+p", "cycle synopsis fwd / back"),
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS", &[
        ("Space / Tab", "play / stop cursor block TTS"),
        ("a", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all paragraphs"),
    ]),
    ("Editing", &[
        ("e", "edit synopsis in place (vim)"),
        (":w / :q / R", "save · quit · ask-Claude rewrite (in editor)"),
        ("u", "undo last edit (confirm)"),
    ]),
    ("Journal", &[
        ("r", "new journal Q&A for scene"),
        ("Alt+g", "work glosses"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("| / !", "font size +/−"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("h / Esc", "close"),
        ("Ctrl+/", "close this legend"),
    ]),
];

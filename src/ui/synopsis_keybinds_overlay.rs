//! Synopsis-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Synopsis keybinds";

/// Grouped (key, action) rows. Matches handle_synopsis_overlay_key + visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / k", "next / prev block"),
        ("x / y", "next / prev page"),
        ("Ctrl+d / Ctrl+u", "next / prev page"),
        ("g g / G", "first / last block"),
        ("Ctrl+n / Ctrl+p", "cycle synopsis fwd / back"),
        ("Shift+V", "visual select (y yank → clipboard)"),
    ]),
    ("TTS", &[
        ("Space", "restart cursor block TTS"),
        ("a", "play / stop cursor block TTS"),
        ("Shift+Space", "synthesize gist/précis/account"),
    ]),
    ("Editing", &[
        ("R", "ask Claude to rewrite this synopsis"),
        ("e", "edit synopsis in place (vim)"),
        ("u", "undo last edit (confirm)"),
        ("c", "copy synopsis debug info"),
    ]),
    super::keybinds_legend::VIM_EDIT_GROUP,
    ("Journal", &[
        ("Alt+g", "work glosses"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("\\", "cycle: → journal Q&A (same segment)"),
        ("Esc", "close"),
        ("Ctrl+Shift+L", "save & quit app"),
        ("Ctrl+/", "close this legend"),
    ]),
];

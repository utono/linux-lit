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
        ("Shift+V", "visual select (y yank → clipboard)"),
    ]),
    ("TTS", &[
        ("Space / Tab", "play / stop cursor block TTS"),
        ("a", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all paragraphs"),
    ]),
    ("Editing", &[
        ("R", "ask Claude to rewrite this synopsis"),
        ("e", "edit synopsis in place (vim)"),
        ("u", "undo last edit (confirm)"),
        ("c", "copy synopsis id"),
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

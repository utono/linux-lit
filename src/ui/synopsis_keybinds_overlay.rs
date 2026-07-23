//! Synopsis-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Synopsis keybinds";

/// Most-used binds, pinned to the legend's upper-right column. Rows here are
/// MOVED out of the groups below (not duplicated). Every MRU bind must actually
/// work in this overlay's handler — of the shared MRU set only Alt+g does
/// (r, Ctrl+r, Ctrl+Shift+n/p/r, Ctrl+a, Ctrl+f, and `\` are consumed no-ops
/// in handle_synopsis_overlay_key; the synopsis left the `\` cycle lap), so
/// only it is listed.
pub const MRU: super::keybinds_legend::Group = ("MRU", &[
    ("Alt+g", "gloss_picker (work glosses)"),
]);

/// Grouped (key, action) rows. Matches handle_synopsis_overlay_key + visual mode.
/// Basic nav (j/k · x/y · gg/G) is deliberately unlisted — the binds stay live
/// in the handler; the legend keeps only the less-guessable rows.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("Ctrl+d / Ctrl+u", "next / prev page"),
        ("Ctrl+n / Ctrl+p", "cycle_synopsis fwd / back"),
        ("Shift+V", "visual select (y yank → clipboard)"),
    ]),
    ("TTS", &[
        ("Space", "restart cursor_block TTS"),
        ("a", "play / stop cursor_block TTS"),
        ("Shift+Space", "synth_all_synopsis_blocks (gist/précis/account)"),
    ]),
    ("Editing", &[
        ("R", "begin_rewrite: Claude rewrite of this synopsis"),
        ("e", "begin_edit (vim, in place)"),
        ("u", "undo last edit (confirm)"),
        ("c", "copy_synopsis_id (debug info)"),
        ("Ctrl+Alt+\\", "vocab_add_card: add a vocab word"),
    ]),
    super::keybinds_legend::VIM_EDIT_GROUP,
    ("Misc", &[
        ("+", "copy_work_division → clipboard"),
        ("Ctrl+↑ / Ctrl+↓", "mpv + TTS (rodio) volume"),
        ("Ctrl+Alt+↑ / ↓", "TTS volume (saved)"),
        ("Esc", "close"),
        ("Ctrl+/", "close this legend"),
    ]),
];

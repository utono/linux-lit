//! Gloss-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Gloss keybinds";

/// Most-used binds, pinned to the legend's upper-right column. Rows here are
/// MOVED out of the groups below (not duplicated). Every MRU bind must actually
/// work in this overlay's handler.
pub const MRU: super::keybinds_legend::Group = ("MRU", &[
    ("Ctrl+f", "corpus_search: all Q&As / glosses"),
    ("Ctrl+a", "passage Q&A: floats here → answer in journal"),
    ("Ctrl+Tab", "toggle focus: ask card ↔ gloss card (2-col)"),
    ("Ctrl+r", "vocab_add_card: add a vocab word"),
    ("Ctrl+w", "begin_rewrite: Claude rewrite of this gloss"),
    ("Ctrl+Shift+r", "browse_restore: the viewed revision"),
    ("Ctrl+Shift+n / Ctrl+Shift+p", "browse_step: rewrite_revisions (view-only)"),
    ("r", "vocab_popup (rr toggles · r next word)"),
    ("Alt+g", "gloss_picker"),
    ("\\", "cycle_from_gloss: → journal Q&A (same segment)"),
]);

/// Grouped (key, action) rows. Matches handle_gloss_key + the gloss visual mode.
/// Basic nav (j/q · k/, · x/y · gg/G) is deliberately unlisted — the binds
/// stay live in the handler; the legend keeps only the less-guessable rows.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("Ctrl+n / Ctrl+p", "navigate_gloss_passage: next / prev"),
        ("Alt+n / Alt+p", "navigate_gloss: prev / next"),
        ("Shift+V", "visual select (y yank → clipboard)"),
        ("Shift+V, s", "syntax diagram of the selected blocks"),
    ]),
    // L (pick_source_voice), v (voice_picker), Ctrl+v (cycle_active_voice)
    // are deliberately unlisted (2026-07-22) — the binds stay live in the
    // handler, same policy as the basic-nav rows above.
    ("TTS", &[
        ("Space", "restart cursor_block TTS (synthesizes on miss)"),
        ("a", "play / stop cursor_block TTS (synthesizes on miss)"),
        ("Shift+Space", "synthesize all prose blocks"),
    ]),
    ("Editing", &[
        ("e", "begin_edit (vim, in place)"),
        ("u", "undo_gloss_edit (confirm)"),
        ("D", "delete_current_gloss (confirm)"),
        ("c", "copy_gloss_id"),
    ]),
    super::keybinds_legend::VIM_EDIT_GROUP,
    ("Misc", &[
        ("+", "copy_work_division → clipboard"),
        ("Ctrl+↑ / Ctrl+↓", "mpv + TTS (rodio) volume"),
        ("Ctrl+Alt+↑ / ↓", "TTS volume (saved)"),
        ("Esc", "close vocab_popup / jump to source_text"),
        ("Ctrl+/", "close this legend"),
    ]),
];

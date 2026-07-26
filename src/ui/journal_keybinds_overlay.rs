//! Journal-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Journal keybinds";

/// Most-used binds, pinned to the legend's upper-right column. Rows here are
/// MOVED out of the groups below (not duplicated). Every MRU bind must actually
/// work in this overlay's handler — Alt+g is a consumed no-op in
/// handle_journal_key (dropped), so it is NOT listed. Ctrl+r = add vocab word.
pub const MRU: super::keybinds_legend::Group = ("MRU", &[
    ("Ctrl+f", "corpus_search: all Q&As / glosses"),
    ("Ctrl+a", "begin_ask: new Q&A in this band"),
    ("Ctrl+Tab", "toggle focus: ask card ↔ journal card (2-col)"),
    ("Alt+s", "JournalBand::Scene (cursor scene)"),
    ("Alt+w", "JournalBand::Work"),
    ("Alt+a", "JournalBand::Author (corpus)"),
    ("Ctrl+Shift+r", "browse_restore: the viewed revision"),
    ("Ctrl+Shift+n / Ctrl+Shift+p", "browse_step: rewrite_revisions (view-only)"),
    ("r", "vocab_popup (rr toggles \u{b7} r next word)"),
    ("Ctrl+r", "vocab_add_card: add a vocab word"),
    ("\\", "cycle_from_journal: → syntax gloss (same segment)"),
]);

/// Grouped (key, action) rows. Matches handle_journal_key + journal visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("Ctrl+n / Ctrl+p", "nav_page: next / prev Q&A in band"),
        ("Alt+n / Alt+p", "nav_scene: next / prev scene band"),
        ("Shift+V", "visual select (y yank → clipboard)"),
    ]),
    ("TTS", &[
        ("Space", "restart cursor_block TTS (synthesizes on miss)"),
        ("a", "play / stop cursor_block TTS (synthesizes on miss)"),
        ("Shift+Space", "synthesize all_paragraphs"),
    ]),
    ("Editing", &[
        ("C-w", "open_rewrite_target: Claude rewrite (q/a/b)"),
        ("e", "begin_edit (vim, in place)"),
        ("u", "undo_journal_edit (confirm)"),
        ("D", "delete_current (confirm)"),
        ("c", "copy_current_id: Q&A id → clipboard"),
        ("Ctrl+Shift+J", "open_move_picker: another JournalBand"),
    ]),
    ("Vim edit mode (after e)", &[
        ("H", "in visual mode: toggle <hi> highlight on the selection"),
        ("Ctrl+v", "paste clipboard (also in the ask_card prompt)"),
    ]),
    ("Misc", &[
        ("+", "copy_work_division → clipboard"),
        ("Esc", "close vocab_popup / jump to source_text"),
        ("Ctrl+/", "close this legend"),
    ]),
];

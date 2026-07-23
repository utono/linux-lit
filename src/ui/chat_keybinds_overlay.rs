//! Chat-panel Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds. Opened from BOTH panel focus contexts (transcript + prompt).

/// Legend card title.
pub const TITLE: &str = "Chat panel keybinds";

/// Grouped (key, action) rows. Matches handle_chat_transcript_key +
/// handle_chat_prompt_key.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Focus", &[
        ("Tab", "cycle focus: input → transcript → reader → …"),
        ("Ctrl+Tab", "close_chat_layout"),
        ("-", "close_chat_layout (transcript)"),
        ("Esc", "focus_reader · exit V-select first"),
        ("Ctrl+l", "flip_panel_side"),
        ("a", "focus_prompt_insert: re-show the input"),
    ]),
    ("Navigation", &[
        ("j / h", "next exchange (cursor down)"),
        ("k / t", "prev exchange (cursor up)"),
        ("g g / G", "first / last landable row"),
        ("Ctrl+d / Ctrl+u", "transcript_half_page down / up"),
        ("Ctrl+n / Ctrl+p", "cycle_gloss fwd / back"),
        ("V", "visual select rows (j/k/h/t extend)"),
        ("y", "yank selection or cursor row → clipboard"),
    ]),
    ("Playback", &[
        ("space", "toggle_source_loop · armed: pause/resume"),
    ]),
    ("Editing", &[
        ("s", "save the selected exchange to the journal"),
        ("Ctrl+r", "add vocab word"),
        ("Ctrl+w", "Gloss view: regloss_pinned · Journal view: rewrite_journal_entry"),
        ("c", "copy_gloss_id · copy_journal_id (per view)"),
        ("D", "delete_current_panel_item (y/Esc confirm)"),
    ]),
    ("Vim edit mode (prompt)", &[
        ("a", "from transcript: open the input in insert"),
        ("i / a / o", "insert / append / open line"),
        ("Ctrl+Enter", "send question (or revise)"),
        ("s / S", "on a bare line: save / consolidate transcript"),
        ("Ctrl+v", "paste clipboard"),
        ("Esc / :q", "hide the input (returns to transcript)"),
    ]),
    ("Misc", &[
        ("r", "vocab_popup (rr toggles · r next word)"),
        ("\\", "toggle_panel_view: gloss ↔ journal Q&A"),
        ("Ctrl+/", "close this legend"),
    ]),
];

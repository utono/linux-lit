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
        ("Ctrl+Tab", "close the chat panel"),
        ("-", "close the chat panel (transcript)"),
        ("Esc", "transcript → reader · exit V-select first"),
        ("Ctrl+l", "flip panel to the other column"),
    ]),
    ("Transcript navigation", &[
        ("j / h", "next exchange (cursor down)"),
        ("k / t", "prev exchange (cursor up)"),
        ("g g / G", "first / last landable row"),
        ("Ctrl+d / Ctrl+u", "half-page down / up"),
        ("Ctrl+n / Ctrl+p", "cycle gloss fwd / back"),
    ]),
    ("Transcript actions", &[
        ("a", "ask: re-show the input, land in insert"),
        ("s", "save the selected exchange to the journal"),
        ("r / R", "Gloss view: re-gloss · Journal view: ask / rewrite"),
        ("\\", "toggle view: gloss ↔ journal Q&A"),
        ("c", "copy id: Gloss view → gloss id · Journal view → Q&A id"),
        ("D", "delete: Gloss view → current gloss · Journal view → Q&A (y/Esc confirm)"),
        ("V", "visual select rows (j/k/h/t extend)"),
        ("y", "yank selection or cursor row → clipboard"),
        ("space", "loop the entry's source audio · armed: pause/resume"),
    ]),
    ("Prompt (vim editor)", &[
        ("a", "from transcript: open the input in insert"),
        ("i / a / o", "insert / append / open line"),
        ("Ctrl+Enter", "send question (or revise)"),
        ("s / S", "on a bare line: save / consolidate transcript"),
        ("Ctrl+v", "paste clipboard"),
        ("Esc / :q", "hide the input (returns to transcript)"),
    ]),
    ("Legend", &[
        ("Ctrl+/", "close this legend"),
    ]),
];

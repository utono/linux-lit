//! Journal-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #50).

/// Legend card title.
pub const TITLE: &str = "Journal keybinds";

/// Grouped (key, action) rows. Matches handle_journal_key + journal visual mode.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("Ctrl+n / Ctrl+p", "next / prev Q&A in band"),
        ("Alt+n / Alt+p", "next / prev scene"),
        ("Alt+s", "current scene band"),
        ("Alt+w", "whole-work band"),
        ("Alt+a", "author corpus band"),
    ]),
    ("Playback / TTS", &[
        ("a", "play / pause (MPV, same as main card)"),
        ("Ctrl+Space", "read cursor block (TTS)"),
        ("A", "play / pause cursor block TTS (cached only)"),
        ("Ctrl+s", "restart cursor block TTS"),
    ]),
    ("Editing", &[
        ("r", "ask a new question"),
        ("R", "ask Claude to rewrite this Q&A"),
        ("Ctrl+Shift+n / Ctrl+Shift+p", "browse rewrite history (view-only)"),
        ("Ctrl+Shift+r", "restore the viewed revision"),
        ("e", "edit Q&A in place (vim)"),
        ("u", "undo last saved edit (confirm)"),
        ("D", "delete Q&A (confirm)"),
        ("c", "copy Q&A id"),
        ("Ctrl+Shift+J", "move Q&A to another band"),
    ]),
    ("Vim edit mode (after e)", &[
        ("H", "in visual mode: toggle <hi> highlight on the selection"),
        ("Ctrl+v", "paste clipboard (also in the r ask prompt)"),
    ]),
    ("Cross-reference", &[
        ("\\", "cycle: → gloss (same segment)"),
    ]),
];

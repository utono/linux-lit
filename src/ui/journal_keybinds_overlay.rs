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
        ("r", "vocab popup (rr toggles \u{b7} r next word)"),
        ("Esc", "close vocab popup / close (jump to source)"),
    ]),
    ("Playback / TTS", &[
        ("a", "play / pause (MPV, same as main card)"),
        ("Ctrl+Space", "play / stop cursor block TTS (synthesizes on miss)"),
        ("A", "restart cursor block TTS from start (synthesizes on miss)"),
        ("Ctrl+s", "restart cursor block TTS from start (synthesizes on miss)"),
    ]),
    ("Editing", &[
        ("C-r", "ask a new question"),
        ("C-w", "ask Claude to rewrite this Q&A"),
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
        ("Ctrl+f", "search all Q&As / glosses"),
    ]),
];

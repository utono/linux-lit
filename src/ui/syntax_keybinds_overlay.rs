//! Syntax-diagram Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds. Modelled on `gloss_keybinds_overlay.rs`.

/// Legend card title.
pub const TITLE: &str = "Syntax diagram keybinds";

/// Grouped (key, action) rows. Matches handle_syntax_diagram_key.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Misc", &[
        ("n", "toggle_note: show / hide the commentary"),
        ("Esc", "close, return to the reader"),
        ("Ctrl+/", "close this legend"),
    ]),
];

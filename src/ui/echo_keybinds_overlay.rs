//! Echoes-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #83 — echo was the legend that keybinds_legend was
//! originally factored out of, and the last one still hand-rolling its widget).

/// Legend card title.
pub const TITLE: &str = "Echo keybinds";

/// Grouped (key, action) rows. Matches handle_echoes_overlay_key.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("n / p", "next / prev echo"),
        ("g g / G", "first / last echo"),
        ("j / k", "scroll list"),
        ("Enter", "open echo's work"),
    ]),
    ("Playback", &[
        ("a", "play source turn (AB-loop)"),
        ("Space", "play selected echo"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
    ]),
    ("Curate", &[
        ("A", "add echo"),
        ("↑ / ↓", "reorder (curate)"),
        ("s", "toggle curate"),
        ("c", "copy echo"),
        ("d", "delete selected echo"),
        ("D", "delete all echoes for turn"),
        ("R", "refresh echoes"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("Esc", "close echoes → reader"),
        ("Ctrl+/", "close this legend"),
    ]),
];

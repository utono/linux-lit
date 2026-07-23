//! Echoes-overlay Ctrl+/ keybind legend DATA. The widget is the shared
//! `keybinds_legend::KeybindsLegend`; this file contributes only the title +
//! grouped binds (audit #83 — echo was the legend that keybinds_legend was
//! originally factored out of, and the last one still hand-rolling its widget).

/// Legend card title.
pub const TITLE: &str = "Echo keybinds";

/// Grouped (key, action) rows. Matches handle_echoes_overlay_key.
pub const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("n / p", "move_echo_selection: next / prev"),
        ("g g / G", "select_first_echo / select_last_echo"),
        ("j / k", "scroll list"),
        ("Enter", "jump_to_selected_echo (open its work)"),
    ]),
    ("Playback", &[
        ("a", "play_source_turn (AB-loop)"),
        ("Space", "play_selected_echo"),
    ]),
    ("Editing", &[
        ("A", "open_add_echo_picker"),
        ("↑ / ↓", "reorder (curate)"),
        ("s", "toggle_curated"),
        ("c", "copy_selected_echo"),
        ("d", "delete_selected_echo"),
        ("D", "delete_all_echoes (this turn)"),
        ("R", "refresh_echoes"),
    ]),
    ("Misc", &[
        (";", "show_chapter_toast"),
        ("Ctrl+↑ / Ctrl+↓", "mpv + TTS (rodio) volume"),
        ("Esc", "close_echoes_to_reader"),
        ("Ctrl+/", "close this legend"),
    ]),
];

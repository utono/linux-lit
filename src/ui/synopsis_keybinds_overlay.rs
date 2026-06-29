use gtk4::prelude::*;

/// Static legend of the synopsis-overlay keybinds, shown over the synopsis
/// overlay via Ctrl+/ (replacing the footer hint). Mirrors `EchoKeybindsOverlay`.
pub struct SynopsisKeybindsOverlay {
    pub container: gtk4::Box,
    pub scrim: gtk4::Box,
}

/// Grouped (key, action) rows. Matches handle_synopsis_overlay_key + visual mode.
const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / k", "next / prev block"),
        ("g g / G", "first / last block"),
        ("Ctrl+n / Ctrl+p", "cycle synopsis fwd / back"),
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS", &[
        ("Space / Tab", "play / stop cursor block TTS"),
        ("a", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all paragraphs"),
    ]),
    ("Editing", &[
        ("A", "ask (amend the synopsis)"),
        ("E", "edit synopsis"),
        ("U", "undo last amend"),
        ("Alt+g", "work glosses"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("| / !", "font size +/−"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("h / Esc", "close"),
        ("Ctrl+/", "close this legend"),
    ]),
];

impl SynopsisKeybindsOverlay {
    pub fn new() -> Self {
        let (container, scrim) =
            super::keybinds_legend::build_legend("Synopsis keybinds", GROUPS);
        Self { container, scrim }
    }

    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.scrim);
        overlay.add_overlay(&self.container);
    }

    pub fn show(&self) {
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.scrim.set_visible(false);
        self.container.set_visible(false);
    }
}

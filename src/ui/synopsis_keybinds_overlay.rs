use gtk4::prelude::*;

/// Static legend of the synopsis-overlay keybinds, shown over the synopsis
/// overlay via Ctrl+/ (replacing the footer hint). Mirrors `EchoKeybindsOverlay`.
pub struct SynopsisKeybindsOverlay {
    pub container: gtk4::Box,
    pub scrim: gtk4::Box,
}

/// (key, action) rows. Matches handle_synopsis_overlay_key + synopsis visual mode.
const BINDS: &[(&str, &str)] = &[
    ("j / k", "next / prev block"),
    ("g g / G", "first / last block"),
    ("Space / Tab", "play / stop cursor block TTS"),
    ("a", "restart cursor block TTS"),
    ("Shift+Space", "synthesize all paragraphs"),
    ("A", "ask (amend the synopsis)"),
    ("E", "edit synopsis"),
    ("U", "undo last amend"),
    ("Ctrl+n / Ctrl+p", "cycle synopsis fwd / back"),
    ("Alt+g", "work glosses"),
    ("Shift+V", "visual select (y yank)"),
    (";", "show chapter"),
    ("| / !", "font size +/−"),
    ("Ctrl+, ", "settings"),
    ("Ctrl+↑ / Ctrl+↓", "volume"),
    ("h / Esc", "close"),
    ("Ctrl+/", "close this legend"),
];

impl SynopsisKeybindsOverlay {
    pub fn new() -> Self {
        let (container, scrim) =
            super::keybinds_legend::build_legend("Synopsis keybinds", BINDS);
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

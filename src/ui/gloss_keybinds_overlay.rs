use gtk4::prelude::*;

/// Static legend of the gloss-overlay keybinds, shown over the gloss overlay via
/// Ctrl+/ (replacing the footer hint). Mirrors `EchoKeybindsOverlay`.
pub struct GlossKeybindsOverlay {
    pub container: gtk4::Box,
    pub scrim: gtk4::Box,
}

/// Grouped (key, action) rows. Matches handle_gloss_key + the gloss visual mode.
const GROUPS: &[super::keybinds_legend::Group] = &[
    ("Navigation", &[
        ("j / q", "next block"),
        ("k / ,", "prev block"),
        ("g g / G", "first / last block"),
        ("Alt+n / Alt+p", "next / prev passage"),
        ("Ctrl+n / Ctrl+p", "prev / next gloss"),
        ("Shift+V", "visual select (y yank)"),
    ]),
    ("TTS / voice", &[
        ("Space / Tab", "read cursor block (TTS)"),
        ("a", "restart cursor block TTS"),
        ("Shift+Space", "synthesize all prose blocks"),
        ("r", "play / stop source verse TTS"),
        ("R", "pick voice for source reading"),
        ("v", "voice picker"),
        ("Ctrl+v", "cycle active voice"),
    ]),
    ("Editing", &[
        ("A", "add / amend gloss"),
        ("E", "edit gloss"),
        ("D", "delete gloss"),
        ("c", "copy gloss id"),
    ]),
    ("Journal", &[
        ("J", "new journal Q&A from passage"),
        ("Ctrl+j", "view journal for passage"),
        ("Alt+g", "glosses picker"),
    ]),
    ("View", &[
        (";", "show chapter"),
        ("| / !", "font size +/−"),
        ("Ctrl+,", "settings"),
        ("Ctrl+↑ / Ctrl+↓", "volume"),
        ("Esc / n", "close (jump to source)"),
        ("Ctrl+/", "close this legend"),
    ]),
];

impl GlossKeybindsOverlay {
    pub fn new() -> Self {
        let (container, scrim) = super::keybinds_legend::build_legend("Gloss keybinds", GROUPS);
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

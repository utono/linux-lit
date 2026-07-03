use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// Static legend of the echoes-overlay keybinds, shown over the echoes overlay.
pub struct EchoKeybindsOverlay {
    pub container: GtkBox,
    pub scrim: GtkBox,
}

/// (key, action) rows shown in the legend. Matches handle_echoes_overlay_key.
const BINDS: &[(&str, &str)] = &[
    ("a", "play echo"),
    ("A", "add echo"),
    ("n / p", "next / prev echo"),
    ("↑ / ↓", "reorder (curate)"),
    ("g g / G", "first / last echo"),
    ("j / k", "scroll list"),
    (";", "show chapter"),
    ("Tab", "play source turn"),
    ("Enter", "open echo's work"),
    ("c", "copy echo"),
    ("s", "toggle curate"),
    ("d", "delete selected echo"),
    ("D", "delete all echoes for turn"),
    ("R", "refresh echoes"),
    ("Ctrl+↑ / Ctrl+↓", "volume"),
    ("Ctrl+j", "go to journal for turn"),
    ("Esc / Ctrl+g", "close echoes → reader"),
    ("Ctrl+/ / Esc", "close this legend"),
];

impl EchoKeybindsOverlay {
    pub fn new() -> Self {
        let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(420)
            .build();
        container.add_css_class("picker-box");
        container.set_visible(false);

        let title = Label::builder()
            .label("Echo keybinds")
            .halign(Align::Start)
            .build();
        title.add_css_class("picker-item-title");
        container.append(&title);

        for (key, action) in BINDS {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(12)
                .build();
            let key_label = Label::builder()
                .label(*key)
                .halign(Align::Start)
                .width_chars(16)
                .xalign(0.0)
                .build();
            key_label.add_css_class("picker-item-title");
            let action_label = Label::builder()
                .label(*action)
                .halign(Align::Start)
                .hexpand(true)
                .xalign(0.0)
                .build();
            row.append(&key_label);
            row.append(&action_label);
            container.append(&row);
        }

        Self { container, scrim }
    }

    /// Add the legend onto an outer overlay (NOT a chain link).
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

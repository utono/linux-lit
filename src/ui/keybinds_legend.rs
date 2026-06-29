//! Shared builder for the per-overlay Ctrl+/ keybind legends (gloss, synopsis,
//! journal). A legend is a simple centered card listing `(key, action)` rows —
//! the full keybind set for that overlay, replacing the old footer hint. Modeled
//! on `echo_keybinds_overlay`; factored here so the three legends share one row
//! layout.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// Build a legend `(container, scrim)` with `title` and one row per
/// `(key, action)`. Both start hidden; the caller attaches them to an outer
/// overlay and toggles `show`/`hide`.
pub fn build_legend(title: &str, binds: &[(&str, &str)]) -> (GtkBox, GtkBox) {
    let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
    scrim.add_css_class("gloss-scrim");
    scrim.set_visible(false);

    let container = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .halign(Align::Center)
        .valign(Align::Center)
        .width_request(460)
        .build();
    container.add_css_class("picker-box");
    container.set_visible(false);

    let title_label = Label::builder().label(title).halign(Align::Start).build();
    title_label.add_css_class("picker-item-title");
    container.append(&title_label);

    for (key, action) in binds {
        let row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
        let key_label = Label::builder()
            .label(*key)
            .halign(Align::Start)
            .width_chars(18)
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

    (container, scrim)
}

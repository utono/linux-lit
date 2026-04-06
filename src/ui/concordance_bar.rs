use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// Bottom status bar showing concordance mode state.
pub struct ConcordanceBar {
    pub container: GtkBox,
    word_label: Label,
    position_label: Label,
}

impl ConcordanceBar {
    pub fn new() -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 0);
        container.set_hexpand(true);
        container.set_visible(false);
        container.add_css_class("concordance-bar");

        let word_label = Label::new(None);
        word_label.set_halign(Align::Start);
        word_label.set_hexpand(true);
        word_label.add_css_class("concordance-bar-word");

        let position_label = Label::new(None);
        position_label.set_halign(Align::Center);
        position_label.set_hexpand(true);
        position_label.add_css_class("concordance-bar-position");

        let hint_label = Label::new(Some("r/R: next/prev | Esc: exit"));
        hint_label.set_halign(Align::End);
        hint_label.set_hexpand(true);
        hint_label.add_css_class("concordance-bar-hint");

        container.append(&word_label);
        container.append(&position_label);
        container.append(&hint_label);

        Self {
            container,
            word_label,
            position_label,
        }
    }

    pub fn update(&self, word: &str, position: &str) {
        self.word_label.set_text(&format!("concordance: {}", word));
        self.position_label.set_text(position);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

}

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow};

pub struct GlossPanel {
    pub container: GtkBox,
    scrolled: ScrolledWindow,
    content_box: GtkBox,
    #[allow(dead_code)]
    hint_label: Label,
}

/// A single gloss entry: word + gloss text.
pub struct GlossEntry {
    pub word: String,
    pub gloss: Option<String>,
}

impl GlossPanel {
    pub fn new() -> Self {
        let content_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&content_box)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .build();

        let hint_label = Label::builder()
            .label("g next gloss \u{00B7} \\ hide")
            .halign(gtk4::Align::Center)
            .build();
        hint_label.add_css_class("definition-hint");

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();
        container.add_css_class("definition-panel");
        container.add_css_class("gloss-panel");
        container.append(&scrolled);
        container.append(&hint_label);
        container.set_visible(false);

        GlossPanel {
            container,
            scrolled,
            content_box,
            hint_label,
        }
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Display a single gloss entry (the currently focused one),
    /// with a counter like "1 / 3" when there are multiple.
    pub fn update(
        &self,
        entry: &GlossEntry,
        current_index: usize,
        total: usize,
        vocab_fg: &str,
    ) {
        // Clear existing children
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }

        let header_text = if total > 1 {
            format!("GLOSS  {} / {}", current_index + 1, total)
        } else {
            "GLOSS".to_string()
        };
        let header = Label::builder()
            .label(&header_text)
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header.add_css_class("definition-header");
        self.content_box.append(&header);

        let word_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(4)
            .build();
        word_label.add_css_class("definition-word");
        word_label.set_markup(&format!(
            "<span foreground=\"{}\">{}</span>",
            glib::markup_escape_text(vocab_fg),
            glib::markup_escape_text(&entry.word),
        ));
        self.content_box.append(&word_label);

        if let Some(ref gloss) = entry.gloss {
            let gloss_label = Label::builder()
                .halign(gtk4::Align::Start)
                .wrap(true)
                .wrap_mode(gtk4::pango::WrapMode::Word)
                .margin_bottom(4)
                .build();
            gloss_label.add_css_class("definition-text");
            gloss_label.set_text(gloss);
            self.content_box.append(&gloss_label);
        } else {
            let no_gloss = Label::builder()
                .halign(gtk4::Align::Start)
                .margin_bottom(4)
                .build();
            no_gloss.add_css_class("definition-text");
            no_gloss.set_text("(no gloss)");
            self.content_box.append(&no_gloss);
        }

        self.scrolled.vadjustment().set_value(0.0);
    }
}

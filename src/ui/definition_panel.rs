use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow};

pub struct DefinitionPanel {
    pub container: GtkBox,
    scrolled: ScrolledWindow,
    content_box: GtkBox,
    #[allow(dead_code)]
    hint_label: Label,
}

impl DefinitionPanel {
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
            .label("w next \u{00B7} W prev \u{00B7} \\ hide \u{00B7} Alt+\\ highlights")
            .halign(gtk4::Align::Center)
            .build();
        hint_label.add_css_class("definition-hint");

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();
        container.add_css_class("definition-panel");
        container.append(&scrolled);
        container.append(&hint_label);
        container.set_visible(false);

        DefinitionPanel {
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

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    #[allow(dead_code)]
    pub fn toggle(&self) {
        self.container.set_visible(!self.container.is_visible());
    }

    /// Update the panel with definitions and etymologies for multiple words.
    /// Each entry is (word, optional definition, optional etymology Pango markup).
    pub fn update_definitions(
        &self,
        entries: &[DefinitionEntry<'_>],
        vocab_fg: &str,
    ) {
        // Remove all existing children from content_box
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }

        let header = Label::builder()
            .label("DEFINITION")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header.add_css_class("definition-header");
        self.content_box.append(&header);

        let has_any = entries.iter().any(|e| e.definition.is_some());
        if !has_any {
            header.set_visible(false);
        }

        let mut shown = 0;
        for entry in entries {
            if entry.definition.is_none() && entry.etymology_markup.is_none() {
                continue;
            }
            let word_label = Label::builder()
                .halign(gtk4::Align::Start)
                .margin_bottom(4)
                .margin_top(if shown > 0 { 12 } else { 0 })
                .build();
            word_label.add_css_class("definition-word");
            word_label.set_markup(&format!(
                "<span foreground=\"{}\">{}</span>",
                glib::markup_escape_text(vocab_fg),
                glib::markup_escape_text(entry.word),
            ));
            self.content_box.append(&word_label);

            if let Some(def) = entry.definition {
                let def_label = Label::builder()
                    .halign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::Word)
                    .margin_bottom(4)
                    .build();
                def_label.add_css_class("definition-text");
                def_label.set_text(def);
                self.content_box.append(&def_label);
            }

            if let Some(etym) = entry.etymology_markup {
                let etym_label = Label::builder()
                    .halign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::Word)
                    .margin_bottom(4)
                    .build();
                etym_label.add_css_class("definition-etymology");
                etym_label.set_markup(etym);
                self.content_box.append(&etym_label);
            }

            shown += 1;
        }

        self.scrolled.vadjustment().set_value(0.0);
    }
}

pub struct DefinitionEntry<'a> {
    pub word: &'a str,
    pub definition: Option<&'a str>,
    pub etymology_markup: Option<&'a str>,
}

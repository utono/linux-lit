use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

/// Data for a single vocab word: definition + etymology + gloss.
pub struct VocabWordData {
    pub word: String,
    pub definition: Option<String>,
    pub etymology_markup: Option<String>,
    pub gloss: Option<String>,
}

/// Which view is currently shown in the popup.
#[derive(Clone, Copy, PartialEq)]
pub enum VocabView {
    Definition,
    Gloss,
}

pub struct VocabPopup {
    container: GtkBox,
    content_box: GtkBox,
    header_label: Label,
    counter_label: Label,
    footer_label: Label,
}

impl VocabPopup {
    pub fn new() -> Self {
        let content_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        let header_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header_label.add_css_class("definition-header");

        let counter_label = Label::builder()
            .halign(gtk4::Align::End)
            .margin_bottom(8)
            .build();
        counter_label.add_css_class("definition-header");

        let header_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(0)
            .build();
        header_row.append(&header_label);
        let spacer = GtkBox::builder().hexpand(true).build();
        header_row.append(&spacer);
        header_row.append(&counter_label);

        let footer_label = Label::builder()
            .halign(gtk4::Align::Center)
            .build();
        footer_label.add_css_class("definition-hint");

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .valign(gtk4::Align::End)
            .margin_end(5)
            .margin_bottom(24)
            .build();
        container.add_css_class("vocab-popup");
        container.append(&header_row);
        container.append(&content_box);
        container.append(&footer_label);
        container.set_visible(false);

        VocabPopup {
            container,
            content_box,
            header_label,
            counter_label,
            footer_label,
        }
    }

    /// Add the popup as an overlay child of a full-width Overlay widget.
    pub fn attach_to(&self, overlay: &gtk4::Overlay) {
        overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }

    /// Set the left margin so the popup starts to the right of the text card.
    pub fn set_margin_start(&self, margin: i32) {
        self.container.set_margin_start(margin);
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }


    /// Render the popup content for a given word and view.
    pub fn update(
        &self,
        data: &VocabWordData,
        index: usize,
        total: usize,
        view: VocabView,
        _work_abbrev: &str,
    ) {
        // Clear content
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }

        // Header: hide work abbreviation
        self.header_label.set_visible(false);

        if total > 1 {
            self.counter_label.set_text(&format!("{} / {}", index + 1, total));
            self.counter_label.set_visible(true);
        } else {
            self.counter_label.set_visible(false);
        }

        // Word
        let word_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        word_label.add_css_class("definition-word");
        word_label.set_text(&data.word);
        self.content_box.append(&word_label);

        match view {
            VocabView::Definition => {
                if let Some(ref def) = data.definition {
                    let def_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .margin_bottom(8)
                        .build();
                    def_label.add_css_class("definition-text");
                    def_label.set_text(def);
                    self.content_box.append(&def_label);
                }

                if let Some(ref etym) = data.etymology_markup {
                    let etym_header = Label::builder()
                        .label("ETYMOLOGY")
                        .halign(gtk4::Align::Start)
                        .margin_top(8)
                        .margin_bottom(4)
                        .build();
                    etym_header.add_css_class("definition-header");
                    self.content_box.append(&etym_header);

                    let etym_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .build();
                    etym_label.add_css_class("definition-etymology");
                    etym_label.set_markup(etym);
                    self.content_box.append(&etym_label);
                }
            }
            VocabView::Gloss => {
                if let Some(ref gloss) = data.gloss {
                    let gloss_label = Label::builder()
                        .halign(gtk4::Align::Start)
                        .wrap(true)
                        .wrap_mode(gtk4::pango::WrapMode::Word)
                        .build();
                    gloss_label.add_css_class("definition-text");
                    gloss_label.set_text(gloss);
                    self.content_box.append(&gloss_label);
                } else {
                    let no_gloss = Label::builder()
                        .halign(gtk4::Align::Start)
                        .build();
                    no_gloss.add_css_class("definition-text");
                    no_gloss.set_text("(no gloss for this passage)");
                    self.content_box.append(&no_gloss);
                }
            }
        }

        self.footer_label.set_visible(false);
    }
}

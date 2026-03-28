use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Overlay};

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
    pub overlay: Overlay,
    container: GtkBox,
    content_box: GtkBox,
    header_label: Label,
    counter_label: Label,
    footer_label: Label,
}

impl VocabPopup {
    pub fn new() -> Self {
        let overlay = Overlay::new();

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

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&content_box)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .propagate_natural_height(true)
            .max_content_height(300)
            .build();

        let footer_label = Label::builder()
            .halign(gtk4::Align::Center)
            .build();
        footer_label.add_css_class("definition-hint");

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .width_request(500)
            .build();
        container.add_css_class("vocab-popup");
        container.append(&header_row);
        container.append(&scrolled);
        container.append(&footer_label);
        container.set_visible(false);

        VocabPopup {
            overlay,
            container,
            content_box,
            header_label,
            counter_label,
            footer_label,
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.container);
        self.container.set_visible(false);
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    /// Set the top margin to position the popup below a given y coordinate.
    pub fn set_y_position(&self, y: i32) {
        self.container.set_margin_top(y);
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
        vocab_fg: &str,
        work_abbrev: &str,
    ) {
        // Clear content
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }

        // Header: work abbreviation
        self.header_label.set_text(work_abbrev);
        self.header_label.set_visible(true);

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
        word_label.set_markup(&format!(
            "<span foreground=\"{}\">{}</span>",
            glib::markup_escape_text(vocab_fg),
            glib::markup_escape_text(&data.word),
        ));
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

        // Footer
        let mut hints = Vec::new();
        if total > 1 {
            hints.push("n next word");
        }
        match view {
            VocabView::Definition => hints.push("g gloss"),
            VocabView::Gloss => hints.push("g definition"),
        }
        hints.push("h close");
        self.footer_label.set_text(&hints.join(" \u{00B7} "));
    }
}

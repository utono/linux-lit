use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, ScrolledWindow};

pub struct DefinitionPanel {
    pub container: GtkBox,
    scrolled: ScrolledWindow,
    content_box: GtkBox,
    header_label: Label,
    word_label: Label,
    definition_label: Label,
    etymology_header: Label,
    etymology_label: Label,
    gloss_header: Label,
    gloss_label: Label,
    hint_label: Label,
}

impl DefinitionPanel {
    pub fn new() -> Self {
        let content_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();

        let header_label = Label::builder()
            .label("DEFINITION")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        header_label.add_css_class("definition-header");
        content_box.append(&header_label);

        let word_label = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        word_label.add_css_class("definition-word");
        content_box.append(&word_label);

        let definition_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .margin_bottom(16)
            .build();
        definition_label.add_css_class("definition-text");
        content_box.append(&definition_label);

        let etymology_header = Label::builder()
            .label("ETYMOLOGY")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        etymology_header.add_css_class("definition-header");
        content_box.append(&etymology_header);

        let etymology_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .margin_bottom(16)
            .build();
        etymology_label.add_css_class("definition-etymology");
        content_box.append(&etymology_label);

        let gloss_header = Label::builder()
            .label("GLOSS")
            .halign(gtk4::Align::Start)
            .margin_bottom(8)
            .build();
        gloss_header.add_css_class("definition-header");
        content_box.append(&gloss_header);

        let gloss_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .build();
        gloss_label.add_css_class("definition-gloss");
        content_box.append(&gloss_label);

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
            .width_request(320)
            .build();
        container.add_css_class("definition-panel");
        container.append(&scrolled);
        container.append(&hint_label);
        container.set_visible(false);

        DefinitionPanel {
            container,
            scrolled,
            content_box,
            header_label,
            word_label,
            definition_label,
            etymology_header,
            etymology_label,
            gloss_header,
            gloss_label,
            hint_label,
        }
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

    pub fn toggle(&self) {
        self.container.set_visible(!self.container.is_visible());
    }

    pub fn update(
        &self,
        word: &str,
        definition: Option<&str>,
        etymology: Option<&str>,
        gloss: Option<&str>,
        vocab_fg: &str,
    ) {
        self.word_label.set_markup(&format!(
            "<span foreground=\"{}\">{}</span>",
            glib::markup_escape_text(vocab_fg),
            glib::markup_escape_text(word),
        ));

        if let Some(def) = definition {
            self.definition_label.set_text(def);
            self.definition_label.set_visible(true);
            self.header_label.set_visible(true);
        } else {
            self.definition_label.set_visible(false);
            self.header_label.set_visible(false);
        }

        if let Some(etym) = etymology {
            self.etymology_label.set_markup(etym);
            self.etymology_label.set_visible(true);
            self.etymology_header.set_visible(true);
        } else {
            self.etymology_label.set_visible(false);
            self.etymology_header.set_visible(false);
        }

        if let Some(g) = gloss {
            self.gloss_label.set_text(g);
            self.gloss_label.set_visible(true);
            self.gloss_header.set_visible(true);
        } else {
            self.gloss_label.set_visible(false);
            self.gloss_header.set_visible(false);
        }

        self.scrolled.vadjustment().set_value(0.0);
    }
}

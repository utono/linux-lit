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
            // In float mode the container is column-height; expanding the
            // content region pins the hint footer to the panel bottom. At
            // natural height (strip mode) this has no effect.
            .vexpand(true)
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

        // Anchored lower-right: the popup hugs the window bottom beside the
        // card (margin_start is set live to clear the card's right edge).
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

    /// Single-column placement: the strip right of the text card. Restores
    /// every container property `place_float` changes, so the two calls can
    /// alternate as the work's layout changes.
    pub fn place_strip(&self, margin_start: i32) {
        self.container.remove_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        self.container.set_halign(gtk4::Align::Fill);
        self.container.set_valign(gtk4::Align::End);
        self.container.set_margin_start(margin_start);
        self.container.set_margin_end(5);
        self.container.set_margin_top(0);
        self.container.set_margin_bottom(24);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Two-column placement: a COMPACT card centered in the reading column
    /// the cursor is NOT in (x/w = that column's window-coord rect from
    /// layout::column_float_rect). Natural height, capped width — never the
    /// full column panel it used to be. `h` is accepted for signature
    /// compatibility but unused: the card takes its natural height (content is
    /// short, and the Journal view caps its own body height).
    pub fn place_float(&self, x: i32, w: i32, h: i32) {
        self.container.add_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        let width = (w - 48).clamp(200, 420);
        let centered_x = x + (w - width) / 2;
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Center);
        self.container.set_margin_start(centered_x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_top(0);
        self.container.set_margin_bottom(0);
        self.container.set_width_request(width);
        self.container.set_height_request(-1);
        // Never taller than the card: content itself is short (definition +
        // etymology); the Journal view already caps its body height.
        let _ = h;
    }

    /// Overlay placement, primary form: the SAME frameless strip as the
    /// reader (`place_strip` geometry), with a transparent background so the
    /// overlay scrim shows through — the popup reads as loose ink beside the
    /// card, exactly like the main-card presentation, instead of a boxed
    /// corner card.
    pub fn place_overlay_strip(&self, margin_start: i32) {
        self.place_strip(margin_start);
        self.container.add_css_class("vocab-popup-overlay");
    }

    /// Overlay fallback placement (strip too narrow to wrap text): compact
    /// card anchored to the window's lower right, natural size (the popup
    /// floats above the overlay chain).
    pub fn place_corner(&self) {
        self.container.remove_css_class("vocab-popup-float");
        self.container.remove_css_class("vocab-popup-overlay");
        self.container.set_halign(gtk4::Align::End);
        self.container.set_valign(gtk4::Align::End);
        self.container.set_margin_start(0);
        self.container.set_margin_end(24);
        self.container.set_margin_top(0);
        self.container.set_margin_bottom(24);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Chat placement: the compact card in the strip `chat::size_panel` frees
    /// BELOW the raised chat-panel bottom. `x`/`y` are window coords (the
    /// popup lives in the window-filling outer overlay, like the panel);
    /// `w` pins the card to the panel's transcript text width so its left and
    /// right borders land on the text margins.
    pub fn place_chat(&self, x: i32, y: i32, w: i32) {
        // Keep the float-card skin (border + radius): the chat slot sits on
        // the bare root, where the plain .vocab-popup (bg == root, no border)
        // would render as loose text with no card edge.
        self.container.add_css_class("vocab-popup-float");
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Start);
        self.container.set_margin_start(x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_top(y.max(0));
        self.container.set_margin_bottom(0);
        self.container.set_width_request(w.max(0));
        self.container.set_height_request(-1);
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

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }


    /// Show "index+1 / total" when stepping through multiple entries; hidden
    /// for a single entry.
    fn set_counter(&self, index: usize, total: usize) {
        if total > 1 {
            self.counter_label.set_text(&format!("{} / {}", index + 1, total));
            self.counter_label.set_visible(true);
        } else {
            self.counter_label.set_visible(false);
        }
    }

    /// Remove every child from the content region (a plain GtkBox — not a
    /// ListBox, so `picker_nav::clear_list` does not apply).
    fn clear_content(&self) {
        while let Some(child) = self.content_box.first_child() {
            self.content_box.remove(&child);
        }
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
        self.clear_content();

        // Header: hide work abbreviation
        self.header_label.set_visible(false);

        self.set_counter(index, total);

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

    /// Render a scene synopsis in the popup.
    pub fn update_synopsis(&self, scene_label: &str, synopsis: &str) {
        self.clear_content();

        self.header_label.set_text("SYNOPSIS");
        self.header_label.set_visible(true);
        self.counter_label.set_visible(false);

        let scene_label_widget = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_bottom(12)
            .build();
        scene_label_widget.add_css_class("definition-word");
        scene_label_widget.set_text(scene_label);
        self.content_box.append(&scene_label_widget);

        let synopsis_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::Word)
            .build();
        synopsis_label.add_css_class("definition-text");
        synopsis_label.set_text(synopsis);
        self.content_box.append(&synopsis_label);

        self.footer_label.set_visible(false);
    }
}

// (The popup's Journal Q&A view was removed: Ctrl+r now holds a toast for the
// API round-trip and opens the JOURNAL OVERLAY on the saved entry — see
// src/input/actions/vocab_journal.rs.)

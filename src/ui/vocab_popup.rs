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
    /// The vocab journal Q&A (R): paged answer above a pinned
    /// word+definition block. Rendered by `update_journal`, not `update`.
    Journal,
}

pub struct VocabPopup {
    container: GtkBox,
    content_box: GtkBox,
    header_label: Label,
    counter_label: Label,
    footer_label: Label,
    /// The Journal answer's scroll region while that view is showing —
    /// Ctrl+n/p page it. None in Definition/Gloss views.
    journal_scroll: std::cell::RefCell<Option<gtk4::ScrolledWindow>>,
    /// Footer text without the page suffix ("saved · <model>").
    journal_footer_base: std::cell::RefCell<String>,
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
            journal_scroll: std::cell::RefCell::new(None),
            journal_footer_base: std::cell::RefCell::new(String::new()),
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
        self.container.set_halign(gtk4::Align::Fill);
        self.container.set_valign(gtk4::Align::End);
        self.container.set_margin_start(margin_start);
        self.container.set_margin_end(5);
        self.container.set_margin_bottom(24);
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Two-column placement: a COMPACT card centered in the reading column
    /// the cursor is NOT in (x/w = that column's window-coord rect from
    /// layout::column_float_rect). Natural height, capped width — never the
    /// full column panel it used to be. `h` caps the card so long content
    /// can't overrun the card vertically.
    pub fn place_float(&self, x: i32, w: i32, h: i32) {
        self.container.add_css_class("vocab-popup-float");
        let width = (w - 48).clamp(200, 420);
        let centered_x = x + (w - width) / 2;
        self.container.set_halign(gtk4::Align::Start);
        self.container.set_valign(gtk4::Align::Center);
        self.container.set_margin_start(centered_x.max(0));
        self.container.set_margin_end(0);
        self.container.set_margin_bottom(0);
        self.container.set_width_request(width);
        self.container.set_height_request(-1);
        // Never taller than the card: content itself is short (definition +
        // etymology); the Journal view already caps its body height.
        let _ = h;
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
        *self.journal_scroll.borrow_mut() = None;

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
            // Journal renders via update_journal; reaching here means the
            // caller forgot to route — show nothing rather than stale data.
            VocabView::Journal => {}
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

/// Body content for the Journal view.
#[derive(Clone, Copy)]
pub enum JournalBody<'a> {
    Pending { model: &'a str },
    Answer { text: &'a str },
    Error { message: &'a str },
}

impl VocabPopup {
    /// Render the Journal Q&A view: JOURNAL Q&A header, dim question line,
    /// the paged answer body (capped at `max_body_height`), then the pinned
    /// word + definition block that stays visible on every page. Footer
    /// ("saved · model — page N / M") appears only for a saved Answer.
    pub fn update_journal(
        &self,
        data: &VocabWordData,
        index: usize,
        total: usize,
        question: &str,
        body: JournalBody,
        saved_model: Option<&str>,
        max_body_height: i32,
    ) {
        self.clear_content();
        *self.journal_scroll.borrow_mut() = None;

        self.header_label.set_visible(false);
        self.set_counter(index, total);

        let qa_header = Label::builder()
            .label("JOURNAL Q&A")
            .halign(gtk4::Align::Start)
            .margin_bottom(4)
            .build();
        qa_header.add_css_class("definition-header");
        self.content_box.append(&qa_header);

        let q_label = Label::builder()
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .margin_bottom(8)
            .build();
        q_label.add_css_class("definition-etymology");
        q_label.set_text(&format!("Q \u{b7} {question}"));
        self.content_box.append(&q_label);

        match body {
            JournalBody::Pending { model } => {
                let pending = Label::builder().halign(gtk4::Align::Start).build();
                pending.add_css_class("definition-etymology");
                pending.set_text(&format!("asking {model}\u{2026}"));
                self.content_box.append(&pending);
            }
            JournalBody::Error { message } => {
                let err = Label::builder()
                    .halign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::WordChar)
                    .build();
                err.add_css_class("definition-etymology");
                err.set_text(message);
                self.content_box.append(&err);
            }
            JournalBody::Answer { text } => {
                let answer = Label::builder()
                    .halign(gtk4::Align::Start)
                    .valign(gtk4::Align::Start)
                    .wrap(true)
                    .wrap_mode(gtk4::pango::WrapMode::WordChar)
                    .build();
                answer.add_css_class("definition-text");
                answer.set_text(text);
                // External vscroll policy: no visible scrollbar; Ctrl+n/p
                // drive the adjustment in whole viewport-height pages.
                let scroll = gtk4::ScrolledWindow::builder()
                    .hscrollbar_policy(gtk4::PolicyType::Never)
                    .vscrollbar_policy(gtk4::PolicyType::External)
                    .propagate_natural_height(true)
                    .max_content_height(max_body_height)
                    .child(&answer)
                    .build();
                self.content_box.append(&scroll);
                *self.journal_scroll.borrow_mut() = Some(scroll);
            }
        }

        // Pinned block: the word + its definition, fixed below the paged
        // body — visible on every page (spec: never scrolls away).
        let pin = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .build();
        pin.add_css_class("journal-pin");
        let word_label = Label::builder().halign(gtk4::Align::Start).build();
        word_label.add_css_class("definition-word");
        word_label.set_text(&data.word);
        pin.append(&word_label);
        if let Some(ref def) = data.definition {
            let def_label = Label::builder()
                .halign(gtk4::Align::Start)
                .wrap(true)
                .wrap_mode(gtk4::pango::WrapMode::Word)
                .build();
            def_label.add_css_class("definition-etymology");
            def_label.set_text(def);
            pin.append(&def_label);
        }
        self.content_box.append(&pin);

        // Footer only once an answer is saved (spec: hidden while pending).
        if let (JournalBody::Answer { .. }, Some(model)) = (body, saved_model) {
            let base = format!("saved \u{b7} {model}");
            *self.journal_footer_base.borrow_mut() = base.clone();
            self.footer_label.set_visible(true);
            self.footer_label.set_text(&base);
            // Page count needs a viewport allocation, which is not in place
            // by the time an idle would fire (page_size()==0 → page 1's
            // indicator would be missing). Refresh from the adjustment's own
            // `changed` signal instead: it fires once the ScrolledWindow is
            // allocated. The adjustment is created fresh per render, so no
            // disconnect bookkeeping is needed.
            if let Some(scroll) = self.journal_scroll.borrow().clone() {
                let footer = self.footer_label.clone();
                scroll.vadjustment().connect_changed(move |adj| {
                    refresh_journal_footer(adj, &footer, &base);
                });
            }
        } else {
            self.journal_footer_base.borrow_mut().clear();
            self.footer_label.set_visible(false);
        }
    }

    /// Page the Journal answer by `dir` viewport-heights. Returns true when
    /// a page turn happened (Journal view with overflowing content only).
    pub fn journal_page(&self, dir: i32) -> bool {
        let scroll = match self.journal_scroll.borrow().clone() {
            Some(s) => s,
            None => return false,
        };
        let adj = scroll.vadjustment();
        let page = adj.page_size();
        if page <= 0.0 || adj.upper() <= page + 1.0 {
            return false;
        }
        let max = (adj.upper() - page).max(0.0);
        let new = (adj.value() + f64::from(dir) * page).clamp(0.0, max);
        if (new - adj.value()).abs() < 1.0 {
            return false;
        }
        adj.set_value(new);
        refresh_journal_footer(&scroll.vadjustment(), &self.footer_label, &self.journal_footer_base.borrow());
        true
    }
}

/// Rewrite the Journal footer with the page position ("saved · m — page
/// 2 / 3 · C-n ▸"). Free function so the post-layout idle can call it with
/// cloned widgets (VocabPopup itself is not reference-counted).
fn refresh_journal_footer(adj: &gtk4::Adjustment, footer: &Label, base: &str) {
    let page = adj.page_size();
    if page <= 0.0 || adj.upper() <= page + 1.0 {
        footer.set_text(base);
        return;
    }
    let pages = (adj.upper() / page).ceil() as usize;
    let cur = ((adj.value() / page).round() as usize + 1).min(pages.max(1));
    footer.set_text(&format!("{base} \u{2014} page {cur} / {pages} \u{b7} C-n \u{25b8}"));
}

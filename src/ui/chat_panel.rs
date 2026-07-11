//! Left chat panel for the Tab chat layout. "Bare on root": no card chrome —
//! labels render directly over the themed root background using CSS classes
//! emitted by theme::generate_css (.chat-panel*, .chat-q, .chat-a, ...).

use gtk4::glib;
use gtk4::prelude::*;

pub enum TranscriptRow {
    Question(String),
    Answer(String),
    /// Context chip: italic excerpt of the cursor segment an exchange was
    /// asked from (shown when it differs from the previous exchange).
    Chip(String),
    Error(String),
    Thinking,
    SavedMark,
}

pub struct ChatPanel {
    pub container: gtk4::Box,
    header_title: gtk4::Label,
    header_scene: gtk4::Label,
    transcript_box: gtk4::Box,
    transcript_scroll: gtk4::ScrolledWindow,
    input: crate::ui::ask_card::AskCard,
}

impl ChatPanel {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        container.add_css_class("chat-panel");
        container.set_margin_start(24);
        container.set_visible(false);

        let header_title = gtk4::Label::new(None);
        header_title.set_halign(gtk4::Align::Start);
        header_title.add_css_class("chat-panel-header");
        let header_scene = gtk4::Label::new(None);
        header_scene.set_halign(gtk4::Align::Start);
        header_scene.add_css_class("chat-panel-header");
        let rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        rule.add_css_class("chat-panel-rule");

        let transcript_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        let transcript_scroll = gtk4::ScrolledWindow::new();
        transcript_scroll.set_child(Some(&transcript_box));
        transcript_scroll.set_vexpand(true);
        transcript_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        // AskCard::new(text_margins, return_focus) — the panel has no card
        // chrome, so text_margins is 0; return_focus is the transcript scroll
        // (GTK focus lands there when the input closes/loses focus), mirroring
        // how journal_overlay hands its page view as return_focus.
        let input = crate::ui::ask_card::AskCard::new(0, &transcript_scroll);
        input.container().add_css_class("chat-input");

        container.append(&header_title);
        container.append(&header_scene);
        container.append(&rule);
        container.append(&transcript_scroll);
        container.append(input.container());

        Self {
            container,
            header_title,
            header_scene,
            transcript_box,
            transcript_scroll,
            input,
        }
    }

    pub fn set_header(&self, title: &str, author: &str, scene: &str) {
        self.header_title
            .set_text(&format!("{} \u{2014} {}", title, author));
        self.header_scene.set_text(scene);
    }

    pub fn size_to(&self, w: i32, h: i32) {
        self.container.set_width_request(w.max(0));
        self.container.set_height_request(h.max(0));
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Rebuild the transcript from rows, newest last, and scroll to the end.
    pub fn render_rows(&self, rows: &[TranscriptRow]) {
        while let Some(child) = self.transcript_box.first_child() {
            self.transcript_box.remove(&child);
        }
        for row in rows {
            let (text, class) = match row {
                TranscriptRow::Question(t) => (t.as_str(), "chat-q"),
                TranscriptRow::Answer(t) => (t.as_str(), "chat-a"),
                TranscriptRow::Chip(t) => (t.as_str(), "chat-chip"),
                TranscriptRow::Error(t) => (t.as_str(), "chat-error"),
                TranscriptRow::Thinking => ("thinking\u{2026}", "chat-a"),
                TranscriptRow::SavedMark => ("\u{2713} saved", "chat-saved"),
            };
            let label = gtk4::Label::new(Some(text));
            label.set_wrap(true);
            label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
            label.set_halign(gtk4::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(false);
            label.add_css_class(class);
            self.transcript_box.append(&label);
        }
        let adj = self.transcript_scroll.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(adj.upper()));
    }

    // ---- ask-input passthroughs (mirror journal_overlay's ask_host wrappers)

    /// AskCard::open takes (title, hint, card_width, block_fill, block_fg); the
    /// panel has no card-width-derived margin re-alignment (fixed 0 margin, set
    /// in `new`), so card_width is passed as 0 (the real `open` no-ops the
    /// margin re-align when `card_width <= 0`).
    pub fn open_input(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str) {
        self.input.open(title, hint, 0, block_fill, block_fg);
    }
    pub fn take_input_text(&self) -> String {
        self.input.take_text()
    }
    pub fn feed_input_vim_key(
        &self,
        k: crate::input::vim::VimKey,
    ) -> crate::input::vim::EditorAction {
        self.input.feed_vim_key(k)
    }
    pub fn paste_input_text(&self, t: &str) {
        self.input.paste_text(t);
    }

    /// Apply the reader font to the ask input, same technique as
    /// `JournalOverlay::apply_font` / `GlossOverlay::apply_font`.
    pub fn apply_font(&self, font_family: &str, font_size: u32) {
        let font_str = format!("{} {}", font_family, font_size);
        crate::ui::apply_font_to_views(&[self.input.input()], &font_str, "chat-input-font");
    }
}

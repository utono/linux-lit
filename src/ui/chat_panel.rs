//! Left chat panel for the Tab chat layout. "Bare on root": no card chrome —
//! labels render directly over the themed root background using CSS classes
//! emitted by theme::generate_css (.chat-panel*, .chat-q, .chat-a, ...).

use gtk4::glib;
use gtk4::prelude::*;

pub enum TranscriptRow {
    Question(String),
    /// Plain-prose answer (journal Q&A, revision, consolidation): rendered as
    /// a single `chat-a` label, no markup parsing.
    Answer(String),
    /// A reader-gloss answer, carrying the RAW `<speaker>`/`<verse>`/`<gloss>`
    /// markup exactly as stored in lit.db. Rendered as several typed rows
    /// (`gloss_render::chat_gloss_rows`) so the quoted source and the model's
    /// commentary read distinctly instead of showing literal tags. Falls back
    /// to a single plain `chat-a` label when the text carries no recognized
    /// tags (defensive — should not happen for a real gloss answer).
    GlossAnswer(String),
    /// Context chip: italic excerpt of the cursor segment an exchange was
    /// asked from (shown when it differs from the previous exchange).
    Chip(String),
    Error(String),
    Thinking,
    SavedMark,
}

pub struct ChatPanel {
    pub container: gtk4::Box,
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

        let header_scene = gtk4::Label::new(None);
        header_scene.set_halign(gtk4::Align::Start);
        header_scene.add_css_class("chat-panel-header");
        let rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        rule.add_css_class("chat-panel-rule");

        let transcript_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        // Reader font, slightly smaller than the main card (theme.rs sets
        // .chat-transcript to font_size - 2pt); the row labels inherit it.
        transcript_box.add_css_class("chat-transcript");
        let transcript_scroll = gtk4::ScrolledWindow::new();
        transcript_scroll.add_css_class("chat-transcript-scroll");
        transcript_scroll.set_child(Some(&transcript_box));
        transcript_scroll.set_vexpand(true);
        transcript_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        // AskCard::new(text_margins, return_focus) — the panel has no card
        // chrome, so text_margins is 0; return_focus is the transcript scroll
        // (GTK focus lands there when the input closes/loses focus), mirroring
        // how journal_overlay hands its page view as return_focus.
        let input = crate::ui::ask_card::AskCard::new(0, &transcript_scroll);
        input.container().add_css_class("chat-input");
        // Give the ask-input a taller default field than the shared 160px
        // AskCard floor — the chat panel has vertical room and a cramped input
        // is awkward to type multi-line questions into.
        input.set_input_height(420);

        container.append(&header_scene);
        container.append(&rule);
        container.append(&transcript_scroll);
        container.append(input.container());

        Self {
            container,
            header_scene,
            transcript_box,
            transcript_scroll,
            input,
        }
    }

    /// Header shows ONLY the journal Q&A band (chapter/scene label) — the
    /// work title/author line was dropped by request (the title bar already
    /// names the work).
    pub fn set_header(&self, scene: &str) {
        self.header_scene.set_text(scene);
    }

    pub fn size_to(&self, w: i32, h: i32) {
        self.container.set_width_request(w.max(0));
        self.container.set_height_request(h.max(0));
    }

    /// Release the panel's explicit width/height hold (-1 = unset the
    /// request in GTK). Used when a work switch invalidates the panel's
    /// current size hold BEFORE the new geometry is known, so the stale
    /// request can't inflate the window while the real re-gate is deferred
    /// to settled geometry (see `chat::on_work_switched` /
    /// `chat::regate_panel`).
    pub fn size_to_natural(&self) {
        self.container.set_width_request(-1);
        self.container.set_height_request(-1);
    }

    /// Flash the input box as the "now active" Tab-cycle cue. The shared
    /// opacity dip alone is easy to miss on the input card, so also paint a
    /// brief cursor-colored border/glow wash that CSS fades out (mirroring
    /// `flash_transcript`).
    pub fn flash_input(&self) {
        let card = self.input.container();
        crate::ui::flash_widget(card.upcast_ref());
        card.add_css_class("chat-flash-active");
        let card = card.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(240), move || {
            card.remove_css_class("chat-flash-active");
        });
    }

    /// Flash the transcript area as the "now active" Tab-cycle cue. The
    /// shared opacity dip is invisible on a bare-on-root (possibly empty)
    /// transcript, so paint a brief background wash that CSS fades out.
    pub fn flash_transcript(&self) {
        crate::ui::flash_widget(self.transcript_scroll.upcast_ref());
        let sc = self.transcript_scroll.clone();
        sc.add_css_class("chat-flash-wash");
        glib::timeout_add_local_once(std::time::Duration::from_millis(160), move || {
            sc.remove_css_class("chat-flash-wash");
        });
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Rebuild the transcript from rows, newest last, and scroll to the end.
    pub fn render_rows(&self, rows: &[TranscriptRow]) {
        self.rebuild_rows(rows);
        let adj = self.transcript_scroll.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(adj.upper()));
    }

    /// Rebuild the transcript and scroll the row at index `focus` to the top
    /// of the viewport (exchange-cursor navigation). Unlike `render_rows`,
    /// this does NOT pin the scroll to the end — pinning is what made j/k
    /// cursor moves look like dead keys.
    pub fn render_rows_focused(&self, rows: &[TranscriptRow], focus: usize) {
        self.rebuild_rows(rows);
        let boxx = self.transcript_box.clone();
        let scroll = self.transcript_scroll.clone();
        glib::idle_add_local_once(move || {
            let mut child = boxx.first_child();
            let mut i = 0usize;
            while let Some(c) = child {
                if i == focus {
                    if let Some(b) = c.compute_bounds(&boxx) {
                        let adj = scroll.vadjustment();
                        let max = (adj.upper() - adj.page_size()).max(0.0);
                        adj.set_value((b.y() as f64).clamp(0.0, max));
                    }
                    return;
                }
                child = c.next_sibling();
                i += 1;
            }
        });
    }

    /// Scroll the transcript viewport by `dir` (±1) steps of ~a third of a
    /// page. Used when a j/k cursor move is already clamped at a boundary,
    /// so an answer taller than the viewport stays fully readable.
    pub fn scroll_transcript_step(&self, dir: f64) {
        let adj = self.transcript_scroll.vadjustment();
        let max = (adj.upper() - adj.page_size()).max(0.0);
        adj.set_value((adj.value() + dir * adj.page_size() * 0.35).clamp(0.0, max));
    }

    fn rebuild_rows(&self, rows: &[TranscriptRow]) {
        while let Some(child) = self.transcript_box.first_child() {
            self.transcript_box.remove(&child);
        }
        for row in rows {
            if let TranscriptRow::GlossAnswer(markup) = row {
                self.append_gloss_answer(markup);
                continue;
            }
            let (text, class) = match row {
                TranscriptRow::Question(t) => (t.as_str(), "chat-q"),
                TranscriptRow::Answer(t) => (t.as_str(), "chat-a"),
                TranscriptRow::Chip(t) => (t.as_str(), "chat-chip"),
                TranscriptRow::Error(t) => (t.as_str(), "chat-error"),
                TranscriptRow::Thinking => ("thinking\u{2026}", "chat-a"),
                TranscriptRow::SavedMark => ("\u{2713} saved", "chat-saved"),
                TranscriptRow::GlossAnswer(_) => unreachable!("handled above"),
            };
            self.append_row_label(text, class);
        }
    }

    /// `WordChar` (break inside a word when a line is too narrow) is right
    /// for verse/gloss prose, which legitimately needs to wrap at any width.
    /// A speaker name (`chat-a-speaker`) must never hyphenate mid-word — e.g.
    /// "CYMBELINE" rendering as "CYMBELIN-" / "E" — so it gets plain `Word`
    /// wrapping instead (wraps at whitespace only; a single long name just
    /// runs to its natural width, never split).
    fn append_row_label(&self, text: &str, class: &str) {
        let label = gtk4::Label::new(Some(text));
        label.set_wrap(true);
        label.set_wrap_mode(if class == "chat-a-speaker" {
            gtk4::pango::WrapMode::Word
        } else {
            gtk4::pango::WrapMode::WordChar
        });
        label.set_halign(gtk4::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(false);
        label.add_css_class(class);
        self.transcript_box.append(&label);
    }

    /// Render a reader-gloss's raw `<speaker>`/`<verse>`/`<gloss>` markup as
    /// several typed labels (`gloss_render::chat_gloss_rows`) instead of one
    /// plain label, so the quoted source (speaker/verse/stage) reads visually
    /// distinct from the model's own commentary (gloss) — mirroring, at the
    /// label level, how the gloss OVERLAY styles the same tags with
    /// `TextTag`s. Falls back to one plain `chat-a` label with the raw text
    /// when the markup carries none of the recognized tags (defensive: should
    /// not happen for a real gloss answer, but never show a blank row).
    fn append_gloss_answer(&self, markup: &str) {
        use crate::ui::gloss_render::{chat_gloss_rows, ChatGlossRowKind};
        let rows = chat_gloss_rows(markup);
        if rows.is_empty() {
            self.append_row_label(markup, "chat-a");
            return;
        }
        for (kind, text) in rows {
            let class = match kind {
                ChatGlossRowKind::Speaker => "chat-a-speaker",
                ChatGlossRowKind::Verse => "chat-a-verse",
                ChatGlossRowKind::Stage => "chat-a-stage",
                ChatGlossRowKind::Gloss => "chat-a-gloss",
            };
            self.append_row_label(&text, class);
        }
    }

    // ---- ask-input passthroughs (mirror journal_overlay's ask_host wrappers)

    /// AskCard::open takes (title, hint, legend, card_width, block_fill,
    /// block_fg); the panel has no card-width-derived margin re-alignment (fixed
    /// 0 margin, set in `new`), so card_width is passed as 0 (the real `open`
    /// no-ops the margin re-align when `card_width <= 0`). No legend on the chat
    /// input ("" opts out).
    pub fn open_input(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str) {
        self.input.open(title, hint, "", 0, block_fill, block_fg);
    }
    pub fn take_input_text(&self) -> String {
        self.input.take_text()
    }
    /// Hide the ask-card input (answer arrived / Esc in Normal mode). `a` on
    /// the transcript (via `focus_prompt`) reopens it.
    pub fn close_input(&self) {
        self.input.close();
    }
    /// Whether the ask-card input is currently open (visible). Used by
    /// `focus_prompt` to avoid reopening (and wiping a draft) on refocus.
    pub fn input_is_open(&self) -> bool {
        self.input.is_open()
    }
    /// Peek the input's current text without clearing it.
    pub fn peek_input_text(&self) -> String {
        self.input.peek_text()
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

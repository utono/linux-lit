//! Left chat panel for the Tab chat layout. "Bare on root": no card chrome —
//! labels render directly over the themed root background using CSS classes
//! emitted by theme::generate_css (.chat-panel*, .chat-q, .chat-a, ...).

use gtk4::glib;
use gtk4::prelude::*;

/// The placeholder text of a `Thinking` row, and the label of a `SavedMark`.
///
/// Shared by the two paths that must agree on what a row SAYS: `rebuild_rows`
/// (which paints the label) and `row_widget_texts` (which `y` yanks). They are
/// separate code paths by necessity — one builds widgets, the other extracts
/// text — so a literal spelled out in both would let `y` silently copy stale
/// text with nothing failing to compile.
const THINKING_TEXT: &str = "thinking\u{2026}";
const SAVED_MARK_TEXT: &str = "\u{2713} saved";

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

        // Spacing 0: source lines must sit at pure line-height like the main
        // card (which sets pixels_above/below_lines(0)), so every gap is
        // padding on the row's own CSS class — a Box spacing would add itself
        // to all of them and no per-row rule could take it back.
        let transcript_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
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

        container.append(&transcript_scroll);
        container.append(input.container());

        Self {
            container,
            transcript_box,
            transcript_scroll,
            input,
        }
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

    /// Rebuild the transcript, paint the `.chat-cursor-row` accent bar
    /// (`theme::generate_css`) on the WIDGET at index `cursor` (j/k's row
    /// cursor — see `input::actions::chat::transcript_rows`' doc comment for
    /// why this must be a widget index, not a `TranscriptRow` index: a
    /// `GlossAnswer` row explodes into several widgets), and scroll it to the
    /// top of the viewport. Unlike `render_rows`, this does NOT pin the
    /// scroll to the end — pinning is what made j/k cursor moves look like
    /// dead keys.
    ///
    /// `selection`, when `Some((start, end))` (widget-row space, inclusive,
    /// `start <= end`), also paints `.chat-visual-row` on every widget in that
    /// range — the `V` visual-mode highlight. `start`/`end` may equal
    /// `cursor` (a one-row selection just anchored); both classes can land on
    /// the same widget, and the CSS box-shadow/background compose fine
    /// together (see `theme::generate_css`'s `.chat-visual-row` comment).
    pub fn render_rows_focused_cursor(
        &self,
        rows: &[TranscriptRow],
        cursor: usize,
        selection: Option<(usize, usize)>,
    ) {
        self.rebuild_rows(rows);
        let boxx = self.transcript_box.clone();
        let scroll = self.transcript_scroll.clone();
        glib::idle_add_local_once(move || {
            let mut child = boxx.first_child();
            let mut i = 0usize;
            while let Some(c) = child {
                if i == cursor {
                    c.add_css_class("chat-cursor-row");
                    if let Some(b) = c.compute_bounds(&boxx) {
                        let adj = scroll.vadjustment();
                        let max = (adj.upper() - adj.page_size()).max(0.0);
                        adj.set_value((b.y() as f64).clamp(0.0, max));
                    }
                } else {
                    c.remove_css_class("chat-cursor-row");
                }
                if selection.is_some_and(|(start, end)| i >= start && i <= end) {
                    c.add_css_class("chat-visual-row");
                } else {
                    c.remove_css_class("chat-visual-row");
                }
                child = c.next_sibling();
                i += 1;
            }
        });
    }

    /// Flash the transcript row WIDGETS in `(start, end)` (widget-row space,
    /// inclusive) as the "copied to clipboard" confirmation that replaces
    /// `y`'s old toast — the wash lands on the lines actually copied, not the
    /// whole scroll area (`flash_transcript` washes the whole area for a
    /// different cue, the Tab-cycle "now active" signal; do not conflate the
    /// two).
    ///
    /// Deferred via `idle_add_local_once`, same as `render_rows_focused_cursor`'s
    /// own class application: when `y` copies a VISUAL selection, the caller
    /// clears `visual_anchor` and calls `render_transcript` (full rebuild,
    /// itself idle-deferred) BEFORE flashing, so the transcript-box children at
    /// the moment `flash_rows` is CALLED may still be the OLD widgets about to
    /// be destroyed. Queuing this flash on the idle loop too means it runs
    /// after that pending rebuild's own idle closure has already replaced the
    /// children, so `boxx.first_child()`/`next_sibling()` here walk the LIVE
    /// widget tree the copied text actually came from. In the no-selection
    /// case no rebuild happens, so the extra idle hop is a harmless no-op
    /// delay.
    pub fn flash_rows(&self, start: usize, end: usize) {
        let boxx = self.transcript_box.clone();
        glib::idle_add_local_once(move || {
            let mut child = boxx.first_child();
            let mut i = 0usize;
            while let Some(c) = child {
                if i >= start && i <= end {
                    c.add_css_class("chat-flash-row");
                    let w = c.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(160), move || {
                        w.remove_css_class("chat-flash-row");
                    });
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

    /// Scroll a HALF page down (`down = true`) or up — vim `Ctrl-d`/`Ctrl-u`.
    /// Half is `page_size * 0.5`, distinct from `scroll_transcript_step`'s 0.35
    /// nudge.
    pub fn scroll_transcript_half_page(&self, down: bool) {
        let adj = self.transcript_scroll.vadjustment();
        let max = (adj.upper() - adj.page_size()).max(0.0);
        let delta = adj.page_size() * 0.5 * if down { 1.0 } else { -1.0 };
        adj.set_value((adj.value() + delta).clamp(0.0, max));
    }

    /// Jump the transcript viewport straight to the top (`to_end = false`) or
    /// bottom (`to_end = true`) — the scroll-only `gg`/`G` behavior for
    /// `PanelView::Journal`/`Question`, which have no row cursor for
    /// `render_rows_focused_cursor` to center (see
    /// `transcript_cursor_move`'s Journal/Question guard for why those views
    /// degrade to plain scrolling). Mirrors `render_rows`'s own
    /// scroll-to-end (`adj.set_value(adj.upper())`) for the `G` case.
    pub fn scroll_transcript_to_edge(&self, to_end: bool) {
        let adj = self.transcript_scroll.vadjustment();
        if to_end {
            adj.set_value(adj.upper());
        } else {
            adj.set_value(0.0);
        }
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
                TranscriptRow::Thinking => (THINKING_TEXT, "chat-a"),
                TranscriptRow::SavedMark => (SAVED_MARK_TEXT, "chat-saved"),
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
    ///
    /// Indentation mirrors the overlay's block-quote model
    /// (`gloss_render::{QUOTE_SPEAKER_INDENT, QUOTE_VERSE_INDENT}`, applied at
    /// `populate_verse_buffer` gloss_render.rs:343-351): the source verse hangs
    /// one dialogue step past the speaker label ONLY when the block actually
    /// HAS a speaker (`chat-a-verse`/`chat-a-stage`); a speakerless (prose)
    /// source has no label to hang past, so it sits at the shallower speaker
    /// indent instead (`chat-a-verse-flush`/`chat-a-stage-flush`) — the deep
    /// indent would read as arbitrary over-indentation there, exactly the
    /// subtlety the overlay's `has_speaker` branch documents.
    fn append_gloss_answer(&self, markup: &str) {
        use crate::ui::gloss_render::{chat_gloss_rows, ChatGlossRowKind};
        let rows = chat_gloss_rows(markup);
        if rows.is_empty() {
            self.append_row_label(markup, "chat-a");
            return;
        }
        let has_speaker = rows.iter().any(|(k, _)| *k == ChatGlossRowKind::Speaker);
        for (kind, text) in rows {
            let class = match kind {
                ChatGlossRowKind::Speaker => "chat-a-speaker",
                ChatGlossRowKind::Verse if has_speaker => "chat-a-verse",
                ChatGlossRowKind::Verse => "chat-a-verse-flush",
                ChatGlossRowKind::Stage if has_speaker => "chat-a-stage",
                ChatGlossRowKind::Stage => "chat-a-stage-flush",
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
    pub fn open_input(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str, insert: bool) {
        if insert {
            self.input.open_insert(title, hint, "", 0, block_fill, block_fg);
        } else {
            self.input.open(title, hint, "", 0, block_fill, block_fg);
        }
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

/// The RENDERED text for a single `TranscriptRow`, in WIDGET space — one
/// entry per label `rebuild_rows`/`append_gloss_answer` would actually paint,
/// same granularity `widget_row_count` (chat.rs) counts and `y` (the
/// transcript's yank bind) copies. This is a second, parallel implementation
/// of `rebuild_rows`'s dispatch — kept in sync by `chat_gloss_rows_tests`-style
/// coverage below and by the fact that both read the exact same
/// `gloss_render::chat_gloss_rows` split for `GlossAnswer`.
///
/// Deliberately mirrors `rebuild_rows`'s text, NOT the raw markup: a
/// `GlossAnswer` row's `<speaker>`/`<verse>`/`<gloss>` tags are stripped by
/// `chat_gloss_rows` exactly as they are for display, so `y` copies
/// "CYMBELINE" / "Stand by my side..." — never a literal `<speaker>` tag.
/// `Thinking` yields the same placeholder text the label shows ("thinking…")
/// rather than an empty string, so a yank mid-request doesn't silently copy
/// nothing.
pub(crate) fn row_widget_texts(row: &TranscriptRow) -> Vec<String> {
    match row {
        TranscriptRow::GlossAnswer(markup) => {
            use crate::ui::gloss_render::chat_gloss_rows;
            let rows = chat_gloss_rows(markup);
            if rows.is_empty() {
                // Mirrors append_gloss_answer's own plain-label fallback.
                vec![markup.clone()]
            } else {
                rows.into_iter().map(|(_, text)| text).collect()
            }
        }
        TranscriptRow::Question(t) => vec![t.clone()],
        TranscriptRow::Answer(t) => vec![t.clone()],
        TranscriptRow::Chip(t) => vec![t.clone()],
        TranscriptRow::Error(t) => vec![t.clone()],
        TranscriptRow::Thinking => vec![THINKING_TEXT.to_string()],
        TranscriptRow::SavedMark => vec![SAVED_MARK_TEXT.to_string()],
    }
}

/// Whether each WIDGET of a single `TranscriptRow` is a valid j/k landing
/// spot — same widget-space granularity as `row_widget_texts` (one entry per
/// label `rebuild_rows`/`append_gloss_answer` actually paints), derived from
/// the SAME `chat_gloss_rows` split so this can never drift out of sync with
/// what `append_gloss_answer` renders. Only a `ChatGlossRowKind::Speaker`
/// widget is unlandable — a speaker name isn't a line you'd read, copy, or
/// mark (see the chat panel's Fix 2). Every other row kind (verse, stage,
/// gloss, question, answer, chip, thinking, saved-mark, error) is landable.
pub(crate) fn row_widget_landable(row: &TranscriptRow) -> Vec<bool> {
    match row {
        TranscriptRow::GlossAnswer(markup) => {
            use crate::ui::gloss_render::{chat_gloss_rows, ChatGlossRowKind};
            let rows = chat_gloss_rows(markup);
            if rows.is_empty() {
                // Mirrors append_gloss_answer's plain-label fallback: one
                // widget, landable (it's the raw text, not a speaker label).
                vec![true]
            } else {
                rows.iter()
                    .map(|(kind, _)| *kind != ChatGlossRowKind::Speaker)
                    .collect()
            }
        }
        // Every other row kind is exactly one widget, always landable.
        _ => vec![true; row_widget_texts(row).len()],
    }
}

#[cfg(test)]
mod row_widget_texts_tests {
    use super::{row_widget_texts, TranscriptRow as R};

    #[test]
    fn plain_rows_yield_their_own_text() {
        assert_eq!(row_widget_texts(&R::Question("Q: x".into())), vec!["Q: x"]);
        assert_eq!(row_widget_texts(&R::Answer("A".into())), vec!["A"]);
        assert_eq!(row_widget_texts(&R::Chip("chip".into())), vec!["chip"]);
        assert_eq!(row_widget_texts(&R::Error("err".into())), vec!["err"]);
        assert_eq!(row_widget_texts(&R::Thinking), vec!["thinking\u{2026}"]);
        assert_eq!(row_widget_texts(&R::SavedMark), vec!["\u{2713} saved"]);
    }

    /// The load-bearing case: a GlossAnswer must yield the RENDERED source
    /// text ("CYMBELINE", "Stand by my side..."), never the raw
    /// `<speaker>`/`<verse>`/`<gloss>` markup — proving `y` copies what the
    /// user sees, matching `chat_gloss_rows_tests::speaker_verse_gloss_become_typed_rows`
    /// in gloss_render.rs.
    #[test]
    fn gloss_answer_yields_rendered_text_not_raw_markup() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side, you whom the gods have made</verse>\n\
                       <gloss>Cymbeline honors the disguised Belarius.</gloss>";
        let texts = row_widget_texts(&R::GlossAnswer(markup.to_string()));
        assert_eq!(
            texts,
            vec![
                "CYMBELINE".to_string(),
                "Stand by my side, you whom the gods have made".to_string(),
                "Cymbeline honors the disguised Belarius.".to_string(),
            ]
        );
        for t in &texts {
            assert!(!t.contains('<'), "row text must not contain raw markup: {t:?}");
        }
    }

    #[test]
    fn untagged_gloss_answer_falls_back_to_raw_text_as_one_widget() {
        let texts = row_widget_texts(&R::GlossAnswer("no tags here".to_string()));
        assert_eq!(texts, vec!["no tags here".to_string()]);
    }
}

#[cfg(test)]
mod row_widget_landable_tests {
    use super::{row_widget_landable, TranscriptRow as R};

    #[test]
    fn plain_rows_are_all_landable() {
        assert_eq!(row_widget_landable(&R::Question("Q: x".into())), vec![true]);
        assert_eq!(row_widget_landable(&R::Answer("A".into())), vec![true]);
        assert_eq!(row_widget_landable(&R::Chip("chip".into())), vec![true]);
        assert_eq!(row_widget_landable(&R::Error("err".into())), vec![true]);
        assert_eq!(row_widget_landable(&R::Thinking), vec![true]);
        assert_eq!(row_widget_landable(&R::SavedMark), vec![true]);
    }

    /// The load-bearing case: only the Speaker widget is unlandable — verse
    /// and gloss widgets in the same GlossAnswer stay landable.
    #[test]
    fn gloss_answer_marks_only_speaker_unlandable() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side, you whom the gods have made</verse>\n\
                       <gloss>Cymbeline honors the disguised Belarius.</gloss>";
        let landable = row_widget_landable(&R::GlossAnswer(markup.to_string()));
        assert_eq!(landable, vec![false, true, true]);
    }

    /// A source turn with two speakers (a scene changing hands): every
    /// Speaker widget is unlandable, not just the first.
    #[test]
    fn multiple_speakers_are_all_unlandable() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>line one</verse>\n\
                       <speaker>BELARIUS</speaker>\n\
                       <verse>line two</verse>";
        let landable = row_widget_landable(&R::GlossAnswer(markup.to_string()));
        assert_eq!(landable, vec![false, true, false, true]);
    }

    #[test]
    fn untagged_gloss_answer_single_widget_is_landable() {
        let landable = row_widget_landable(&R::GlossAnswer("no tags here".to_string()));
        assert_eq!(landable, vec![true]);
    }
}

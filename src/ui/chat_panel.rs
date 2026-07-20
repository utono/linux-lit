//! Left chat panel for the Tab chat layout. "Bare on root": no card chrome —
//! labels render directly over the themed root background using CSS classes
//! emitted by theme::generate_css (.chat-panel*, .chat-q, .chat-a, ...).

use gtk4::glib;
use gtk4::prelude::*;

/// The placeholder text of a `Thinking` row, and the label of a `SavedMark`.
///
/// Shared by the two paths that must agree on what a row SAYS:
/// `rebuild_from_specs` (which paints the label) and `row_widget_texts`
/// (which `y` yanks). They are
/// separate code paths by necessity — one builds widgets, the other extracts
/// text — so a literal spelled out in both would let `y` silently copy stale
/// text with nothing failing to compile.
const THINKING_TEXT: &str = "thinking\u{2026}";
const SAVED_MARK_TEXT: &str = "\u{2713} saved";

/// Vertical padding of the `.chat-transcript` content box (padding-top 0 +
/// padding-bottom 16px). Widgets live in `transcript_box` inside
/// `transcript_scroll`, so this padding consumes scroll height the widgets can
/// never use — `transcript_budget` must subtract it or pagination packs one
/// row's overflow past the bottom and the last line clips.
/// MIRRORS `.chat-transcript { padding-bottom }` in theme.rs (~1362) — keep in sync.
const CHAT_TRANSCRIPT_PAD_V: i32 = 16;

/// Sub-pixel safety margin shaved off the transcript budget: pango's
/// logical-vs-ink height rounds up a hair, so without this a page can pack to a
/// hairline overflow. 2px absorbs that rounding.
const CHAT_BUDGET_SAFETY: i32 = 2;

/// Add `class` to `w` and remove it again after `ms` — the widget-CSS flash
/// primitive behind the input / transcript / row flashes. (The Label-TEXT
/// sibling is `ui::toast::show_transient`; this is the CSS-class analogue.)
fn flash_class<W: IsA<gtk4::Widget>>(w: &W, class: &'static str, ms: u64) {
    w.add_css_class(class);
    let w = w.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
        w.remove_css_class(class);
    });
}

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
    /// Vocab-highlight word set + span color for transcript labels; empty
    /// set disables. Set by chat render paths from AppState (the panel has
    /// no state access of its own).
    vocab_words: std::cell::RefCell<std::collections::HashSet<String>>,
    vocab_color: std::cell::RefCell<Option<String>>,
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
        // Horizontal policy Never makes a ScrolledWindow report its child's FULL
        // natural width as its own minimum — so a wide transcript row (a pinned
        // prose passage whose wrapping label reports a large natural width)
        // inflated the panel container past its requested width, pushing the
        // pinned panel flush to the window's right edge (the 1-column prose bug).
        // Stop propagating the child's natural width so the ScrolledWindow (and
        // thus the container) honors its allocated/`width_request` width and the
        // labels WRAP within it instead of forcing the panel wider.
        transcript_scroll.set_propagate_natural_width(false);

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

        // The panel now PAGINATES (renders only the whole widgets that fit —
        // `render_page`), so a partial row can never straddle either edge and
        // the old bottom-clip Overlay/guard is gone. The scroll is appended
        // directly to the container again.
        container.append(&transcript_scroll);
        container.append(input.container());

        Self {
            container,
            transcript_box,
            transcript_scroll,
            input,
            vocab_words: std::cell::RefCell::new(std::collections::HashSet::new()),
            vocab_color: std::cell::RefCell::new(None),
        }
    }

    /// Set the vocab-highlight word set and span color applied to transcript
    /// labels on the next (and every subsequent) render. An empty word set or
    /// a `None` color disables highlighting (labels render as plain text).
    /// The panel has no AppState access; the chat render paths feed this from
    /// `AppState.vocab_words`/`vocab_highlight_visible` before rebuilding.
    pub fn set_vocab_highlight(
        &self,
        words: std::collections::HashSet<String>,
        color: Option<String>,
    ) {
        *self.vocab_words.borrow_mut() = words;
        *self.vocab_color.borrow_mut() = color;
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
        flash_class(card, "chat-flash-active", 240);
    }

    /// Flash the transcript area as the "now active" Tab-cycle cue. The
    /// shared opacity dip is invisible on a bare-on-root (possibly empty)
    /// transcript, so paint a brief background wash that CSS fades out.
    pub fn flash_transcript(&self) {
        crate::ui::flash_widget(self.transcript_scroll.upcast_ref());
        flash_class(&self.transcript_scroll, "chat-flash-wash", 160);
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// The transcript scroll's usable pixel budget for one page — the height a
    /// page slice must fit within. Uses the scroll's allocated height once GTK
    /// has laid it out; before first allocation falls back to the container's
    /// requested height minus the input card's height (the two children of the
    /// vertical container) so pagination has a sane budget on the very first
    /// render. A small guard floor keeps `paginate_grouped` from producing a
    /// per-widget explosion when queried before any geometry exists.
    pub fn transcript_budget(&self) -> i32 {
        let alloc = self.transcript_scroll.height();
        if alloc > 1 {
            (alloc - CHAT_TRANSCRIPT_PAD_V - CHAT_BUDGET_SAFETY).max(1)
        } else {
            let container_h = self.container.height();
            let container_req = self.container.height_request();
            let input_h = self.input.container().height();
            (container_h.max(container_req) - input_h.max(0)
                - CHAT_TRANSCRIPT_PAD_V
                - CHAT_BUDGET_SAFETY)
                .max(200)
        }
    }

    /// The pixel wrap width a transcript label lays out at — the scroll's
    /// allocated width minus the transcript's horizontal padding (padding-right
    /// 14px, theme.rs:1368). Falls back to the container's requested width when
    /// unallocated so measurement matches the eventual wrap.
    fn transcript_wrap_width(&self) -> i32 {
        let w = self.transcript_scroll.width();
        let w = if w > 1 {
            w
        } else {
            self.container.width().max(self.container.width_request()).max(1)
        };
        (w - 14).max(1)
    }

    /// Pixel height one widget renders at: the pango-measured wrapped text
    /// height (at the transcript wrap width) plus the CSS class padding.
    /// Injected into `widget_heights` as the `measure` closure; `widget_heights`
    /// adds `class_pad` + the src-lead extra itself, so this returns ONLY the
    /// text height (no padding).
    ///
    /// `family`/`size_pt` are the REAL transcript font — `config.font_family` /
    /// `config.font_size`, the same source the `.chat-transcript` CSS is
    /// generated from (theme.rs). Do NOT read them off the pango context's font
    /// description: that returns a stale/inherited size (e.g. 21pt) that does
    /// not match the CSS render size (16pt), which inflated every paragraph's
    /// height ~1.6× and under-filled the page. The transcript labels render at
    /// plain pango line height (no `set_pixels_inside_wrap`/leading, Box spacing
    /// 0), so measure PLAIN — not the leaded variant.
    pub fn measure_widget(&self, text: &str, family: &str, size_pt: i32) -> i32 {
        let pctx = self.transcript_box.pango_context();
        crate::ui::pagination::measure_text_height(
            &pctx,
            text,
            size_pt,
            family,
            self.transcript_wrap_width(),
        )
    }

    /// Paginate a full spec vec at the current transcript budget. The single
    /// entry point the whole-transcript callers share so measure + slice always
    /// agree. Returns the page ranges (block indices into `specs`).
    ///
    /// `family`/`size_pt` are the real transcript font (`config.font_family` /
    /// `config.font_size`) — threaded through to `measure_widget` so
    /// measurement matches the CSS render size, not the pango context's stale
    /// font description.
    pub fn paginate_specs(
        &self,
        specs: &[crate::ui::chat_pagination::ChatWidget],
        family: &str,
        size_pt: i32,
    ) -> Vec<crate::ui::pagination::Page> {
        let (heights, group_start) = crate::ui::chat_pagination::widget_heights(specs, |t| {
            self.measure_widget(t, family, size_pt)
        });
        let budget = self.transcript_budget();
        crate::ui::pagination::paginate_grouped(&heights, &group_start, budget)
    }

    /// Render ONLY the widgets in `page` (`specs[page.start..page.end]`) — a
    /// whole-widget slice that fits the transcript budget by construction, so no
    /// partial row can straddle either edge (the pagination replacement for the
    /// old free-scroll clip machinery). The `.chat-cursor-row` accent bar is
    /// painted at the PAGE-LOCAL cursor (`cursor_widget - page.start`) when
    /// `cursor_widget` falls inside this page; `.chat-visual-row` is painted over
    /// the page-local intersection of `selection` (widget-space, inclusive) with
    /// the page. The vadjustment is reset to the top on every render — a page
    /// normally fits, but an oversized single widget scrolls, and the adjustment
    /// would otherwise carry over from the previous render.
    pub fn render_page(
        &self,
        specs: &[crate::ui::chat_pagination::ChatWidget],
        page: crate::ui::pagination::Page,
        cursor_widget: Option<usize>,
        selection: Option<(usize, usize)>,
    ) {
        let start = page.start.min(specs.len());
        let end = page.end.min(specs.len());
        let slice = &specs[start..end];
        self.rebuild_from_specs(slice);
        // A page normally fits the budget, but a single widget taller than
        // the viewport (an unsplit long answer) DOES scroll — and GTK keeps
        // the previous adjustment value across the child rebuild, so the view
        // inherited whatever scroll position the panel was at. Every page
        // render starts at the top.
        self.transcript_scroll.vadjustment().set_value(0.0);
        // Page-local cursor index (None when the cursor isn't on this page).
        let local_cursor =
            cursor_widget.filter(|&c| c >= start && c < end).map(|c| c - start);
        let boxx = self.transcript_box.clone();
        glib::idle_add_local_once(move || {
            let mut child = boxx.first_child();
            let mut i = 0usize; // page-local widget index
            while let Some(c) = child {
                if local_cursor == Some(i) {
                    c.add_css_class("chat-cursor-row");
                } else {
                    c.remove_css_class("chat-cursor-row");
                }
                // selection is in GLOBAL widget space; translate to page-local.
                let global = start + i;
                if selection.is_some_and(|(s, e)| global >= s && global <= e) {
                    c.add_css_class("chat-visual-row");
                } else {
                    c.remove_css_class("chat-visual-row");
                }
                child = c.next_sibling();
                i += 1;
            }
        });
    }

    /// Whole-transcript render for STREAMING/transient views (thinking, error,
    /// current-question, pushed gloss) that have no persisted page state: build
    /// specs, paginate, and render the LAST page (newest content) with no accent
    /// bar. Replaces the old scroll-to-end `render_rows`.
    pub fn render_rows(&self, rows: &[TranscriptRow], family: &str, size_pt: i32) {
        let specs = row_widget_specs(rows);
        let pages = self.paginate_specs(&specs, family, size_pt);
        let Some(&page) = pages.last() else {
            self.rebuild_from_specs(&[]);
            return;
        };
        self.render_page(&specs, page, None, None);
    }

    /// Whole-transcript render for the static saved/revision snapshot: build
    /// specs, paginate, and render the FIRST page (the reader wants the `Q:`
    /// line at the top of the entry). No accent bar — the saved view is a
    /// static snapshot, not the j/k-navigable Gloss transcript.
    pub fn render_rows_to_top(&self, rows: &[TranscriptRow], family: &str, size_pt: i32) {
        let specs = row_widget_specs(rows);
        let pages = self.paginate_specs(&specs, family, size_pt);
        let Some(&page) = pages.first() else {
            self.rebuild_from_specs(&[]);
            return;
        };
        self.render_page(&specs, page, None, None);
    }

    /// Flash the transcript row WIDGETS in `(start, end)` (widget-row space,
    /// inclusive) as the "copied to clipboard" confirmation that replaces
    /// `y`'s old toast — the wash lands on the lines actually copied, not the
    /// whole scroll area (`flash_transcript` washes the whole area for a
    /// different cue, the Tab-cycle "now active" signal; do not conflate the
    /// two).
    ///
    /// Deferred via `idle_add_local_once`, same as `render_page`'s own
    /// cursor/selection class application: when `y` copies a VISUAL selection,
    /// the caller clears `visual_anchor` and calls `render_transcript` (full rebuild,
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
                    flash_class(&c, "chat-flash-row", 160);
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
    /// bottom (`to_end = true`) — the scroll-only `gg`/`G` fallback for
    /// `PanelView::Journal`/`Question` when there is no row cursor/page state
    /// to land the accent bar on (empty list, no exchange at the cursor, or a
    /// pending Question view — see `transcript_cursor_move`'s Journal/Question
    /// guards). Both views otherwise have a real row cursor and page state and
    /// render through `render_paginated` like Gloss.
    pub fn scroll_transcript_to_edge(&self, to_end: bool) {
        let adj = self.transcript_scroll.vadjustment();
        if to_end {
            adj.set_value(adj.upper());
        } else {
            adj.set_value(0.0);
        }
    }

    /// Rebuild the transcript's widget children from a slice of already-expanded
    /// widget specs (`row_widget_specs` output) — one `gtk4::Label` per spec, in
    /// order, with its primary class and (when present) its `extra_class`
    /// second class. This is the SINGLE render primitive: `render_page` passes a
    /// page slice, the whole-transcript callers pass the full spec vec. Both this
    /// and the pagination height model consume the same expansion, so render and
    /// measure cannot drift.
    fn rebuild_from_specs(&self, specs: &[crate::ui::chat_pagination::ChatWidget]) {
        while let Some(child) = self.transcript_box.first_child() {
            self.transcript_box.remove(&child);
        }
        for w in specs {
            self.append_spec_label(w);
        }
    }

    /// Append one label for a widget spec.
    ///
    /// `WordChar` (break inside a word when a line is too narrow) is right for
    /// verse/gloss prose, which legitimately needs to wrap at any width. A
    /// speaker name (`chat-a-speaker`) must never hyphenate mid-word — e.g.
    /// "CYMBELINE" rendering as "CYMBELIN-" / "E" — so it gets plain `Word`
    /// wrapping instead. When present, `extra_class` (today only
    /// `chat-a-src-lead`) is added as a second CSS class for the block-leading
    /// source row's top gap.
    fn append_spec_label(&self, w: &crate::ui::chat_pagination::ChatWidget) {
        let label = gtk4::Label::new(Some(&w.text));
        label.set_wrap(true);
        label.set_wrap_mode(if w.class == "chat-a-speaker" {
            gtk4::pango::WrapMode::Word
        } else {
            gtk4::pango::WrapMode::WordChar
        });
        label.set_halign(gtk4::Align::Start);
        label.set_xalign(0.0);
        label.set_selectable(false);
        label.add_css_class(&w.class);
        if let Some(extra) = &w.extra_class {
            label.add_css_class(extra);
        }
        // Vocab highlight: when enabled and this label's text contains a vocab
        // word, replace the plain text with Pango markup wrapping each match in
        // a colored span. Specs are plain text (GlossAnswer markup is already
        // exploded into text rows by chat_gloss_rows before reaching here), so
        // escaping in vocab_markup is safe on every row.
        if let Some(color) = self.vocab_color.borrow().as_deref() {
            let words = self.vocab_words.borrow();
            if !words.is_empty() {
                if let Some(markup) = vocab_markup(&w.text, &words, color) {
                    label.set_markup(&markup);
                }
            }
        }
        self.transcript_box.append(&label);
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
/// entry per label `rebuild_from_specs`/`gloss_answer_specs` would actually
/// paint, same granularity `widget_row_count` (chat.rs) counts and `y` (the
/// transcript's yank bind) copies. This is a second, parallel implementation
/// of `row_widget_specs`'s dispatch — kept in sync by `chat_gloss_rows_tests`-style
/// coverage below and by the fact that both read the exact same
/// `gloss_render::chat_gloss_rows` split for `GlossAnswer`.
///
/// Deliberately mirrors `row_widget_specs`'s text, NOT the raw markup: a
/// `GlossAnswer` row's `<speaker>`/`<verse>`/`<gloss>` tags are stripped by
/// `chat_gloss_rows` exactly as they are for display, so `y` copies
/// "CYMBELINE" / "Stand by my side..." — never a literal `<speaker>` tag.
/// `Thinking` yields the same placeholder text the label shows ("thinking…")
/// rather than an empty string, so a yank mid-request doesn't silently copy
/// nothing.
/// Pango markup for a chat label: vocab matches wrapped in a colored span,
/// everything escaped. None when the text has no match (caller keeps plain
/// set_text — cheaper and avoids markup parsing for the common case).
pub(crate) fn vocab_markup(
    text: &str,
    words: &std::collections::HashSet<String>,
    color: &str,
) -> Option<String> {
    let mut spans = Vec::new();
    crate::vocab_scan::scan_line(text, 0, words, &mut spans);
    if spans.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut pos = 0usize;
    for s in &spans {
        let before: String = chars[pos..s.char_start].iter().collect();
        let hit: String = chars[s.char_start..s.char_end].iter().collect();
        out.push_str(&glib::markup_escape_text(&before));
        out.push_str(&format!(
            "<span foreground=\"{}\">{}</span>",
            color,
            glib::markup_escape_text(&hit)
        ));
        pos = s.char_end;
    }
    let rest: String = chars[pos..].iter().collect();
    out.push_str(&glib::markup_escape_text(&rest));
    Some(out)
}

pub(crate) fn row_widget_texts(row: &TranscriptRow) -> Vec<String> {
    match row {
        TranscriptRow::GlossAnswer(markup) => {
            use crate::ui::gloss_render::chat_gloss_rows;
            let rows = chat_gloss_rows(markup);
            if rows.is_empty() {
                // Mirrors gloss_answer_specs's own plain-label fallback.
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
/// label `rebuild_from_specs`/`gloss_answer_specs` actually paints), derived
/// from the SAME `chat_gloss_rows` split so this can never drift out of sync
/// with what `gloss_answer_specs` renders. Only a `ChatGlossRowKind::Speaker`
/// widget is unlandable — a speaker name isn't a line you'd read, copy, or
/// mark (see the chat panel's Fix 2). Every other row kind (verse, stage,
/// gloss, question, answer, chip, thinking, saved-mark, error) is landable.
pub(crate) fn row_widget_landable(row: &TranscriptRow) -> Vec<bool> {
    match row {
        TranscriptRow::GlossAnswer(markup) => {
            use crate::ui::gloss_render::{chat_gloss_rows, ChatGlossRowKind};
            let rows = chat_gloss_rows(markup);
            if rows.is_empty() {
                // Mirrors gloss_answer_specs's plain-label fallback: one
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

/// The SINGLE source of truth for how a transcript flattens into rendered
/// widgets: one `ChatWidget { text, class, group_start }` per label
/// `rebuild_from_specs`/`gloss_answer_specs` paints, in order.
/// `rebuild_from_specs` renders from this, and the pagination height model
/// (`chat_pagination`) measures from it, so render and measure can never
/// drift.
///
/// `group_start` is `true` for the FIRST widget of each `TranscriptRow` and
/// `false` for a `GlossAnswer`'s continuation widgets (verse/stage/gloss labels
/// after the first) — i.e. the flag marks an indivisible pagination unit's
/// leading widget. Class + text assignment mirror `gloss_answer_specs` EXACTLY.
/// `class` is the PRIMARY class; `extra_class` carries the stateful
/// `chat-a-src-lead` second class (the top gap on the first source row that
/// follows a gloss) so pagination can account for its rendered height — it is
/// rendered via `append_spec_label`'s `extra_class` handling. Carrying the
/// extra as DATA (not dropping it) is required: dropping it undercounts that
/// row's height and pagination packs one row too many (the returning bottom
/// clip).
pub(crate) fn row_widget_specs(
    rows: &[TranscriptRow],
) -> Vec<crate::ui::chat_pagination::ChatWidget> {
    use crate::ui::chat_pagination::ChatWidget;
    let mut out: Vec<ChatWidget> = Vec::new();
    for row in rows {
        match row {
            TranscriptRow::GlossAnswer(markup) => {
                for (text, class, extra, group_start) in gloss_answer_specs(markup) {
                    out.push(ChatWidget {
                        text,
                        class: class.to_string(),
                        extra_class: extra.map(|e| e.to_string()),
                        group_start,
                    });
                }
            }
            other => {
                let (text, class) = plain_row_spec(other);
                out.push(ChatWidget {
                    text: text.to_string(),
                    class: class.to_string(),
                    extra_class: None,
                    group_start: true,
                });
            }
        }
    }
    out
}

/// The (text, class) a single non-`GlossAnswer` row renders as — the exact
/// mapping `row_widget_specs` uses for every non-gloss row. `GlossAnswer` is
/// handled by `gloss_answer_specs`; calling this with one panics (it never
/// should).
fn plain_row_spec(row: &TranscriptRow) -> (&str, &'static str) {
    match row {
        TranscriptRow::Question(t) => (t.as_str(), "chat-q"),
        TranscriptRow::Answer(t) => (t.as_str(), "chat-a"),
        TranscriptRow::Chip(t) => (t.as_str(), "chat-chip"),
        TranscriptRow::Error(t) => (t.as_str(), "chat-error"),
        TranscriptRow::Thinking => (THINKING_TEXT, "chat-a"),
        TranscriptRow::SavedMark => (SAVED_MARK_TEXT, "chat-saved"),
        TranscriptRow::GlossAnswer(_) => unreachable!("GlossAnswer handled by gloss_answer_specs"),
    }
}

/// Flatten a `GlossAnswer`'s raw `<speaker>`/`<verse>`/`<gloss>` markup into
/// `(text, class, extra_class, group_start)` per widget — the SINGLE source of
/// the (text, class) + `chat-a-src-lead` decision, so `row_widget_specs`,
/// `rebuild_from_specs`, and the pagination height model share ONE expansion.
/// Renders the quoted source (speaker/verse/stage)
/// visually distinct from the model's own commentary (gloss) — mirroring, at the
/// label level, how the gloss OVERLAY styles the same tags with `TextTag`s.
/// Falls back to one plain `chat-a` widget with the raw text when the markup
/// carries none of the recognized tags (defensive: should not happen for a real
/// gloss answer, but never show a blank row).
///
/// Indentation mirrors the overlay's block-quote model
/// (`gloss_render::{QUOTE_SPEAKER_INDENT, QUOTE_VERSE_INDENT}`): the source
/// verse hangs one dialogue step past the speaker label ONLY when the block
/// actually HAS a speaker (`chat-a-verse`/`chat-a-stage`); a speakerless (prose)
/// source has no label to hang past, so it sits at the shallower speaker indent
/// instead (`chat-a-verse-flush`/`chat-a-stage-flush`).
///
/// `extra_class` is `Some("chat-a-src-lead")` for the FIRST source row that
/// follows a gloss (the extra top gap separating this block's source from the
/// preceding block's gloss commentary). `prev_was_gloss` starts false so the
/// very first source row (no preceding gloss) gets no extra gap. `group_start`
/// is true only for the first widget of the block.
fn gloss_answer_specs(
    markup: &str,
) -> Vec<(String, &'static str, Option<&'static str>, bool)> {
    use crate::ui::gloss_render::{chat_gloss_rows, ChatGlossRowKind};
    let rows = chat_gloss_rows(markup);
    if rows.is_empty() {
        // The plain-label fallback: one widget with the raw text.
        return vec![(markup.to_string(), "chat-a", None, true)];
    }
    let has_speaker = rows.iter().any(|(k, _)| *k == ChatGlossRowKind::Speaker);
    let mut prev_was_gloss = false;
    let mut out = Vec::with_capacity(rows.len());
    for (i, (kind, text)) in rows.into_iter().enumerate() {
        let is_source = kind != ChatGlossRowKind::Gloss;
        let class = match kind {
            ChatGlossRowKind::Speaker => "chat-a-speaker",
            ChatGlossRowKind::Verse if has_speaker => "chat-a-verse",
            ChatGlossRowKind::Verse => "chat-a-verse-flush",
            ChatGlossRowKind::Stage if has_speaker => "chat-a-stage",
            ChatGlossRowKind::Stage => "chat-a-stage-flush",
            ChatGlossRowKind::Gloss => "chat-a-gloss",
        };
        let extra = if is_source && prev_was_gloss {
            Some("chat-a-src-lead")
        } else {
            None
        };
        out.push((text, class, extra, i == 0));
        prev_was_gloss = kind == ChatGlossRowKind::Gloss;
    }
    out
}

#[cfg(test)]
mod vocab_markup_tests {
    use super::vocab_markup;

    #[test]
    fn vocab_markup_escapes_and_wraps_matches() {
        let words: std::collections::HashSet<String> =
            ["censure".to_string()].into_iter().collect();
        let m = vocab_markup("Should censure <thus> on gentlemen.", &words, "#ffcc66").unwrap();
        assert!(m.contains("&lt;thus&gt;"), "text must be escaped: {m}");
        assert!(m.contains("<span foreground=\"#ffcc66\">censure</span>"), "{m}");
        assert!(vocab_markup("no matches here", &words, "#ffcc66").is_none());
    }
}

#[cfg(test)]
mod row_widget_specs_tests {
    use super::{row_widget_specs, TranscriptRow as R};

    #[test]
    fn row_widget_specs_explodes_gloss_and_marks_groups() {
        let rows = vec![
            R::Question("q".into()),
            R::GlossAnswer("<speaker>X</speaker>\n<verse>v1</verse>\n<gloss>g</gloss>".into()),
        ];
        let specs = row_widget_specs(&rows);
        // Question → 1 widget (group start). GlossAnswer → 3 widgets: first is a
        // group start, the rest continue the same unit.
        assert_eq!(specs.len(), 4);
        assert_eq!(specs[0].group_start, true); // Question
        assert_eq!(specs[1].group_start, true); // gloss unit begins
        assert_eq!(specs[2].group_start, false); // verse continues
        assert_eq!(specs[3].group_start, false); // gloss continues
        assert_eq!(specs[1].class, "chat-a-speaker");
    }

    /// The three widget-space views (specs, texts, landable) must stay the same
    /// length and order for any rows vec — pagination and render walk `specs`,
    /// while `y`/j-k walk `texts`/`landable`.
    #[test]
    fn specs_texts_landable_same_length() {
        let rows = vec![
            R::Question("q".into()),
            R::Answer("a".into()),
            R::Chip("c".into()),
            R::Error("e".into()),
            R::Thinking,
            R::SavedMark,
            R::GlossAnswer(
                "<speaker>CYMBELINE</speaker>\n<verse>v1</verse>\n<gloss>g1</gloss>\n\
                 <speaker>BELARIUS</speaker>\n<stage>enters</stage>\n<gloss>g2</gloss>"
                    .into(),
            ),
            R::GlossAnswer("no tags here".into()),
        ];
        let specs_len = row_widget_specs(&rows).len();
        let texts_len: usize = rows.iter().map(|r| super::row_widget_texts(r).len()).sum();
        let landable_len: usize = rows.iter().map(|r| super::row_widget_landable(r).len()).sum();
        assert_eq!(specs_len, texts_len);
        assert_eq!(specs_len, landable_len);
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

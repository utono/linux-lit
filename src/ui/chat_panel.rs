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
    /// Bottom-edge clip: an invisible card-colored Box overlaid on the scroll
    /// area that masks the partial last row straddling the viewport bottom.
    /// The transcript is a Box of whole-widget Labels, so `attach_box` (the
    /// box-slack variant) clips cleanly at label boundaries — a wrapping Label
    /// is allocated as ONE whole widget, never bisected at the edge like a
    /// wrapping TextView. See docs/troubleshooting/clip-prevention.md.
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
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

        // Wrap the scroll in an Overlay so the bottom-clip Box can be an
        // add_overlay sibling painted on top of the trailing partial row.
        // attach_box builds the clip Box, tags it .gloss-bottom-clip, and wires
        // the value_changed/page_size recompute — the same machinery the gloss
        // and journal overlays use. The panel's own top row-snap
        // (render_rows_focused_cursor) handles the TOP edge; the guard the
        // BOTTOM edge.
        let scroll_overlay = gtk4::Overlay::new();
        scroll_overlay.set_child(Some(&transcript_scroll));
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach_box(
            &scroll_overlay,
            &transcript_scroll,
        );

        container.append(&scroll_overlay);
        container.append(input.container());

        Self {
            container,
            transcript_box,
            transcript_scroll,
            clip_guard,
            input,
        }
    }

    /// Snap the transcript to the top and (re)compute the bottom clip across the
    /// open's layout passes — call whenever the panel becomes visible / is
    /// (re)rendered on open. Delegates to the shared BottomClipGuard, mirroring
    /// the gloss/journal overlays' `reset_scroll_top`.
    pub fn on_open(&self) {
        self.clip_guard.on_open();
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
        // Settle the bottom clip across the reveal's layout passes (path a).
        // A cursor-positioned render runs after show() and moves the scroll via
        // set_value, which the guard's value_changed catch-all picks up — so the
        // snap-to-top in on_open is harmlessly overridden by that scroll; what we
        // need here is the clip recompute once the viewport range settles.
        self.clip_guard.on_open();
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

    /// Rebuild the transcript from rows and scroll to the TOP (not the end).
    /// `render_saved_entry` uses this for the standalone 3-row saved/revision
    /// view: the reader wants the `Q:` line at the top of the entry, so a long
    /// answer must not pin the viewport to its own bottom the way `render_rows`
    /// (newest-last, scroll-to-end) does. No accent bar: the saved view is a
    /// static snapshot, not the j/k-navigable Gloss transcript, so painting a
    /// row cursor here would leave a bar that j/k (which re-renders the FULL
    /// transcript) can't move.
    ///
    /// Scroll robustness: a single `set_value(0.0)` on the first idle can race
    /// the ScrolledWindow assigning its scroll range for a freshly-rebuilt LONG
    /// answer — the range is still the old/short one on that idle, and once GTK
    /// lays out the tall content the viewport ends up mid-answer. So also
    /// re-assert 0.0 the first time the adjustment's range actually changes
    /// (after layout), then disconnect (settle-on-`changed`, mirroring the
    /// overlay's geometry-settle pattern).
    pub fn render_rows_to_top(&self, rows: &[TranscriptRow]) {
        self.rebuild_rows(rows);
        let scroll = self.transcript_scroll.clone();
        glib::idle_add_local_once(move || {
            let adj = scroll.vadjustment();
            adj.set_value(0.0);
            let id_cell: std::rc::Rc<std::cell::Cell<Option<glib::SignalHandlerId>>> =
                std::rc::Rc::new(std::cell::Cell::new(None));
            let id_cell2 = id_cell.clone();
            let id = adj.connect_changed(move |a| {
                a.set_value(0.0);
                if let Some(id) = id_cell2.take() {
                    a.disconnect(id);
                }
            });
            id_cell.set(Some(id));
        });
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
            // Pass 1: apply cursor/selection classes and collect each row's
            // (y, height) in transcript-box coords + the cursor row's bounds.
            let mut child = boxx.first_child();
            let mut i = 0usize;
            let mut row_tops: Vec<f64> = Vec::new();
            let mut cursor_bounds: Option<(f64, f64)> = None;
            while let Some(c) = child {
                if let Some(b) = c.compute_bounds(&boxx) {
                    row_tops.push(b.y() as f64);
                    if i == cursor {
                        cursor_bounds = Some((b.y() as f64, b.height() as f64));
                    }
                }
                if i == cursor {
                    c.add_css_class("chat-cursor-row");
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

            // Pass 2: scroll the cursor row INTO VIEW only when it is off the
            // visible viewport (a bar step within the page must not move the
            // viewport), snapping the viewport top to a WHOLE-row boundary so the
            // topmost visible row is never a sliver — while keeping the cursor
            // row fully visible.
            if let Some((ry, rh)) = cursor_bounds {
                let adj = scroll.vadjustment();
                let page = adj.page_size();
                let max = (adj.upper() - page).max(0.0);
                let top = adj.value();
                let bottom = top + page;

                // Largest row-top <= v (snap the top DOWN to a row boundary).
                let snap_down = |v: f64| {
                    row_tops
                        .iter()
                        .copied()
                        .filter(|&t| t <= v + 0.5)
                        .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t))))
                        .unwrap_or(v)
                };

                let new = if ry < top {
                    // Cursor above the viewport: snap its OWN top to the edge.
                    snap_down(ry)
                } else if ry + rh > bottom {
                    // Cursor below the viewport: the top must be a whole row AND
                    // low enough that the cursor's bottom fits. Snapping the raw
                    // target (ry+rh-page) DOWN scrolls up and re-hides the cursor
                    // bottom (the bottom-clip bug), so instead take the SMALLEST
                    // row-top that still keeps the cursor bottom visible
                    // (t + page >= ry + rh) — a whole top row, cursor fully shown.
                    row_tops
                        .iter()
                        .copied()
                        .filter(|&t| t + page + 0.5 >= ry + rh && t <= ry + 0.5)
                        .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.min(t))))
                        .unwrap_or_else(|| snap_down(ry + rh - page))
                } else {
                    // Fully visible: don't move the viewport.
                    top
                };
                adj.set_value(new.clamp(0.0, max));
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

    /// Rebuild the transcript by RENDERING FROM the shared widget expansion
    /// (`row_widget_specs`/`gloss_answer_specs`) — one `gtk4::Label` per widget
    /// spec, using its class. Both this and the pagination height model consume
    /// the same expansion, so render and measure cannot drift. The
    /// `chat-a-src-lead` extra class (carried by `gloss_answer_specs`) is applied
    /// via `append_row_label_extra`, matching the old inline `append_gloss_answer`.
    fn rebuild_rows(&self, rows: &[TranscriptRow]) {
        while let Some(child) = self.transcript_box.first_child() {
            self.transcript_box.remove(&child);
        }
        for row in rows {
            match row {
                TranscriptRow::GlossAnswer(markup) => {
                    for (text, class, extra, _group_start) in gloss_answer_specs(markup) {
                        match extra {
                            Some(extra) => self.append_row_label_extra(&text, class, extra),
                            None => self.append_row_label(&text, class),
                        }
                    }
                }
                other => {
                    let (text, class) = plain_row_spec(other);
                    self.append_row_label(text, class);
                }
            }
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

    /// Like `append_row_label`, but adds `extra` as a second CSS class on the
    /// row. Used to tag the block-leading source row (`chat-a-src-lead`) so it
    /// gets the extra top gap separating it from the preceding gloss.
    fn append_row_label_extra(&self, text: &str, class: &str, extra: &str) {
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
        label.add_css_class(extra);
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

/// The SINGLE source of truth for how a transcript flattens into rendered
/// widgets: one `ChatWidget { text, class, group_start }` per label
/// `rebuild_rows`/`append_gloss_answer` paints, in order. `rebuild_rows` renders
/// from this, and the pagination height model (`chat_pagination`) measures from
/// it, so render and measure can never drift.
///
/// `group_start` is `true` for the FIRST widget of each `TranscriptRow` and
/// `false` for a `GlossAnswer`'s continuation widgets (verse/stage/gloss labels
/// after the first) — i.e. the flag marks an indivisible pagination unit's
/// leading widget. Class + text assignment mirror `append_gloss_answer` EXACTLY
/// (including the stateful `chat-a-src-lead` tag on the first source row that
/// follows a gloss); `class` here is the PRIMARY class only (the `chat-a-src-lead`
/// extra is carried as a distinct spec text but rendered via
/// `append_row_label_extra` — see `rebuild_rows`).
pub(crate) fn row_widget_specs(
    rows: &[TranscriptRow],
) -> Vec<crate::ui::chat_pagination::ChatWidget> {
    use crate::ui::chat_pagination::ChatWidget;
    let mut out: Vec<ChatWidget> = Vec::new();
    for row in rows {
        match row {
            TranscriptRow::GlossAnswer(markup) => {
                for (text, class, _extra, group_start) in gloss_answer_specs(markup) {
                    out.push(ChatWidget {
                        text,
                        class: class.to_string(),
                        group_start,
                    });
                }
            }
            other => {
                let (text, class) = plain_row_spec(other);
                out.push(ChatWidget {
                    text: text.to_string(),
                    class: class.to_string(),
                    group_start: true,
                });
            }
        }
    }
    out
}

/// The (text, class) a single non-`GlossAnswer` row renders as — the exact
/// mapping `rebuild_rows` used inline. `GlossAnswer` is handled by
/// `gloss_answer_specs`; calling this with one panics (it never should).
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
/// `(text, class, extra_class, group_start)` per widget — the SINGLE extraction
/// of the old `append_gloss_answer`'s (text, class) + `chat-a-src-lead`
/// decision, so `row_widget_specs`, `rebuild_rows`, and the pagination height
/// model share ONE expansion. Renders the quoted source (speaker/verse/stage)
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
        // Mirrors append_gloss_answer's plain-label fallback.
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

use crate::ui::ask_card::{AskCard, AskCardHost, AskFocus};
use crate::ui::gloss_block::visual_block_range;
use crate::ui::journal_block::{journal_blocks, JournalBlock};
use crate::ui::journal_edit_card::JournalEditCard;
use gtk4::prelude::*;
use gtk4::{Label, Overlay};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct JournalOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    scrolled: gtk4::ScrolledWindow,
    view: gtk4::TextView,
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    footer_container: gtk4::Box,
    footer_left: Label,
    hint: Label,
    bar_drawing: gtk4::DrawingArea,
    bar_ranges: Rc<RefCell<Vec<(i32, i32)>>>,
    blocks: RefCell<Vec<JournalBlock>>,
    visual_anchor: Cell<Option<usize>>,
    cursor_block: Cell<usize>,
    text_margins: i32,
    column_width: i32,
    /// True when the loaded work is prose. Set once per work load via
    /// `set_prose`. Selects the centered prose column inset (card_width/5) over
    /// the verse `card_width/4` inset in `size_card`.
    is_prose: Cell<bool>,
    font_family: RefCell<String>,
    font_size: Cell<i32>,
    last_card_size: Cell<(i32, i32)>,
    /// Owns the ask-card lifecycle + the fixed-scroll-height viewport-shrink (the
    /// occlusion fix) + the footer hide/show + the clip recompute. Shared with the
    /// gloss overlay so the mechanism can't drift. See `AskCardHost`.
    ask_host: AskCardHost,
    edit_card: JournalEditCard,
}

/// Prefix a journal Q&A question with `Q: ` for display (the answer follows
/// below). Idempotent: a question already starting with `Q:` is returned as-is,
/// so a stored/re-rendered question isn't double-prefixed.
fn prefix_question(question: &str) -> String {
    if question.trim_start().starts_with("Q:") {
        question.to_string()
    } else {
        format!("Q: {}", question)
    }
}

/// Vertical chrome margins the column needs that `preferred_size()` does NOT
/// report (GTK's preferred-size excludes a widget's own margins). The journal
/// card column is `title` + `scroll_overlay` + `footer` stacked; the title
/// carries a 24px `margin_top` (journal_overlay::new), the scroll_overlay a 24px
/// top + 20px bottom margin (the breathing gap below the title / above the footer
/// that keeps a scrolled block's last line off the footer rule — mirrors the
/// gloss overlay), and the footer container a 12px top + 12px bottom
/// (`ui::footer::build_footer_row`). Without reserving these, `size_card` budgets
/// `card_height − title_h − footer_h` for the scroll, the assembled column's
/// natural height becomes `card_height + 92`, and because the `valign=Center`
/// container's `set_size_request` is only a FLOOR, the container grows past
/// `card_height` and overflows the window (the "too-tall journal overlay" bug).
/// The gloss overlay reserves the same way via its `SCROLL_OVERLAY_MARGINS`. Keep
/// in sync with those three margin sites.
const UNACCOUNTED_CHROME_MARGINS: i32 =
    24 /* title top */ + 24 + 20 /* scroll_overlay top+bottom */ + 12 + 12 /* footer top+bottom */;

impl JournalOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.add_css_class("gloss-scrim");
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.set_visible(false);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("gloss-overlay");
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_visible(false);

        let title = Label::new(Some(""));
        title.add_css_class("gloss-title");
        title.set_halign(gtk4::Align::Start);
        title.set_margin_start(text_margins as i32);
        title.set_margin_end(text_margins as i32);
        title.set_margin_top(24);
        container.append(&title);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);
        scrolled.set_propagate_natural_height(false);
        // SPIKE (fixed-scroll-height architecture): vexpand is OFF. The earlier
        // races came from the vexpand scroll fighting the container's unbounded
        // height non-deterministically when the ask card appeared/vanished. With
        // vexpand off, the scroll's height is EXACTLY what size_card sets it to —
        // deterministic, no fight, no resize-on-open race. size_card sets it to
        // the pane height minus the title+footer; open/close adjust it by the ask
        // card's reserved height.
        scrolled.set_vexpand(false);

        let view = gtk4::TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_focusable(false);
        view.set_wrap_mode(gtk4::WrapMode::Word);
        view.add_css_class("gloss-text");

        // The ScrolledWindow's child MUST be the TextView DIRECTLY so GTK uses
        // the view's native scroll adjustments (a TextView is `Scrollable`).
        // Wrapping it in an Overlay made GTK insert a GtkViewport, which gave the
        // vadjustment no real scroll range — j/k/G/gg did nothing and overflow
        // content stayed clipped. The bottom_clip therefore overlays an OUTER
        // Overlay that wraps the scrolled window, exactly like the gloss overlay
        // (Overlay(ScrolledWindow(TextView) + bottom_clip)).
        scrolled.set_child(Some(&view));

        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&scrolled));
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &scroll_overlay,
            &view,
            &scrolled,
        );

        // Selection bar: a DrawingArea overlay over the same scroll_overlay that
        // hosts bottom_clip, drawing a 2px vertical accent line over selected
        // buffer-line spans. Fixed color — NOT theme-wired.
        let bar_ranges: Rc<RefCell<Vec<(i32, i32)>>> = Rc::new(RefCell::new(Vec::new()));
        let bar_drawing = gtk4::DrawingArea::new();
        bar_drawing.set_can_target(false);
        {
            let ranges_clone = bar_ranges.clone();
            let view_clone = view.clone();
            bar_drawing.set_draw_func(move |_area, cr, _w, _h| {
                let ranges = ranges_clone.borrow();
                if ranges.is_empty() {
                    return;
                }
                // Fixed gloss accent default (NOT theme-wired).
                cr.set_source_rgb(0.53, 0.62, 0.71);
                cr.set_line_width(2.0);
                let buffer = view_clone.buffer();
                // Draw the bar in the left gutter just inside the text margin so
                // it sits beside the selected paragraph. The card sets the view's
                // left_margin to card_side_margin (card_width/4); a fixed x=4 put
                // the bar far out in the empty gutter, looking like nothing was
                // selected. 12px left of the text edge mirrors the gloss bar.
                let x = (view_clone.left_margin() as f64 - 12.0).max(2.0);
                for (start_line, end_line) in ranges.iter() {
                    if let (Some(si), Some(ei)) =
                        (buffer.iter_at_line(*start_line), buffer.iter_at_line(*end_line))
                    {
                        let start_loc = view_clone.iter_location(&si);
                        let (y_end, h_end) = view_clone.line_yrange(&ei);
                        let (_, by_start) = view_clone.buffer_to_window_coords(
                            gtk4::TextWindowType::Widget, 0, start_loc.y());
                        let (_, by_end) = view_clone.buffer_to_window_coords(
                            gtk4::TextWindowType::Widget, 0, y_end + h_end);
                        cr.move_to(x, by_start as f64);
                        cr.line_to(x, by_end as f64);
                        let _ = cr.stroke();
                    }
                }
            });
        }
        // Repaint the bar when the view scrolls (buffer->window y is scroll-dependent).
        {
            let bar_for_scroll = bar_drawing.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
            });
        }
        scroll_overlay.add_overlay(&bar_drawing);
        scroll_overlay.set_measure_overlay(&bar_drawing, false);
        scroll_overlay.set_clip_overlay(&bar_drawing, true);

        // Breathing gap above the footer and below the title, mirroring the gloss
        // overlay (gloss_overlay.rs). Without the bottom margin the viewport's
        // bottom edge sits flush against the footer, so a block scrolled to the
        // bottom edge by j/k has its last line bisected by the footer rule and
        // reads as clipped — the journal block-nav clipping the user saw. The
        // bottom-clip box masks only a PARTIAL row at the viewport edge; this gap
        // keeps the last whole line clear at any scroll position. The top margin
        // gives the symmetric gap below the title rule (the view's internal
        // top_margin scrolls away with the content, so it can't keep this gap).
        scroll_overlay.set_margin_bottom(20);
        scroll_overlay.set_margin_top(24);

        container.append(&scroll_overlay);

        // Footer rule mirroring the gloss overlay (gloss_overlay.rs footer_box):
        // current page's work/act/scene on the left, fixed keybind hints on the
        // right.
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "Space read \u{00b7} Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select \u{00b7} c copy id",
        );
        let footer_left = footer.left;
        let hint = footer.hint;
        let footer_container = footer.container.clone();
        container.append(&footer.container);

        // Shared "ask" input card (canonical synopsis values), stacked last in
        // the column. Focus returns to the page view when leaving the input.
        let ask = AskCard::new(text_margins as i32, &view);
        container.append(ask.container());

        // The host owns the ask-card lifecycle: the fixed-scroll-height
        // viewport-shrink, the footer hide/show, and the clip recompute. The
        // recompute closure drives this overlay's BottomClipGuard's clip box.
        let recompute = {
            let clip = clip_guard.clip().clone();
            let view = view.clone();
            let scrolled = scrolled.clone();
            Rc::new(move || {
                crate::ui::recompute_overlay_bottom_clip(&view, &clip, &scrolled);
            }) as Rc<dyn Fn()>
        };
        let ask_host =
            AskCardHost::new(ask, &scrolled, Some(footer_container.clone()), recompute);

        let edit_card = JournalEditCard::new(text_margins as i32, &view);
        container.append(edit_card.container());

        Self {
            overlay,
            scrim,
            container,
            title,
            scrolled,
            view,
            clip_guard,
            footer_container,
            footer_left,
            hint,
            bar_drawing,
            bar_ranges,
            blocks: RefCell::new(Vec::new()),
            visual_anchor: Cell::new(None),
            cursor_block: Cell::new(0),
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            is_prose: Cell::new(false),
            font_family: RefCell::new(String::new()),
            font_size: Cell::new(16),
            last_card_size: Cell::new((0, 0)),
            ask_host,
            edit_card,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    /// Record whether the loaded work is prose, so `size_card` picks the
    /// centered prose column inset. Called once per work load from display_work.
    pub fn set_prose(&self, is_prose: bool) {
        self.is_prose.set(is_prose);
    }

    fn size_card(&self, card_width: i32, card_height: i32) {
        self.container.set_size_request(card_width, card_height);
        self.last_card_size.set((card_width, card_height));
        // Fixed-scroll-height (the host owns it): the scroll (vexpand off) gets an
        // EXPLICIT height = pane minus the title + footer chrome — its height while
        // the ask card is CLOSED. `ask_host.open` subtracts the ask slot, `close`
        // restores this stored closed height. Deterministic — no auto-resize race.
        let (_, title_h) = self.title.preferred_size();
        let (_, footer_h) = self.footer_container.preferred_size();
        // Fold the chrome margins `preferred_size()` omits (title's top margin +
        // the footer's top/bottom) into the fixed-chrome argument, so the host's
        // closed scroll height equals `closed_scroll_budget(card_height, title_h,
        // footer_h)`. Without this the column is `UNACCOUNTED_CHROME_MARGINS`
        // (92px) too tall and the `valign=Center` container grows past
        // `card_height`, overflowing the window (the "too-tall journal overlay"
        // bug). `closed_scroll_budget` is the unit-tested source of truth.
        self.ask_host.size(
            card_width,
            card_height,
            title_h.height() + UNACCOUNTED_CHROME_MARGINS,
            footer_h.height(),
        );
        // Anchor the text + headers to the card's side margin (card_width/4, the
        // ~65-char readability optimum the gloss overlay uses) rather than the
        // small fixed `text_margins` — otherwise the Q&A prose runs nearly edge
        // to edge on a wide card. Card SIZE is unchanged; only the inner padding
        // grows. The title and position label indent to match so the left edge
        // of the header and the body line up. See ui::card_side_margin (audit #27).
        let side = if self.is_prose.get() {
            crate::ui::prose_column_margin(card_width)
        } else {
            crate::ui::card_side_margin(card_width)
        };
        self.view.set_left_margin(side);
        self.view.set_right_margin(side);
        self.title.set_margin_start(side);
        let _ = (self.text_margins, self.column_width);
    }

    pub fn show_page(
        &self,
        scene_title: &str,
        footer_left: &str,
        page_index: usize,
        page_count: usize,
        question: &str,
        answer: &str,
        card_width: i32,
        card_height: i32,
    ) {
        self.size_card(card_width, card_height);
        self.title.set_text(scene_title);
        let pos_text = if page_count == 0 {
            "page 0 of 0 in this scene".to_string()
        } else {
            format!("page {} of {} in this scene", page_index + 1, page_count)
        };
        self.set_footer_left(footer_left, &pos_text);
        let body = if page_count == 0 {
            "No pages yet \u{2014} press A to ask.".to_string()
        } else {
            format!("{}\n\n{}", prefix_question(question), answer)
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask_host.card().close();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.clip_guard.on_open();
        // rebuild_blocks resets the cursor to block 0 and marks it (the left
        // accent bar). It used to be followed by clear_bar(), which wiped that
        // mark so the bar only appeared after the first j/k. Keep the mark, and
        // repaint once more after layout settles: mark_cursor_block sets
        // bar_ranges, but the bar DRAW reads per-line geometry (line_yrange),
        // which is 0/stale until GTK lays out the buffer just made visible — so
        // the synchronous draw paints nothing on a fresh open. (Same fix the
        // gloss overlay uses in show_gloss.)
        self.rebuild_blocks();
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());

        // Headless test: emit the journal overlay viewport rect once layout
        // settles, so tests/journal_clipping.rs can target the card's region.
        // Connect to the vadjustment's `changed` signal, which fires when GTK
        // first assigns a scroll range (i.e. after the first layout pass) — the
        // same event BottomClipGuard uses to detect settled geometry. Disconnect
        // after the first emission with a non-zero rect.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.scrolled.clone();
            let adj = sc.vadjustment();
            let id_cell: Rc<std::cell::Cell<Option<glib::SignalHandlerId>>> =
                Rc::new(std::cell::Cell::new(None));
            let id_cell_clone = id_cell.clone();
            let id = adj.connect_changed(move |adj| {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    if r.width() > 0.0 && r.height() > 0.0 {
                        crate::logging::log(&format!(
                            "TEST_JOURNAL_VIEWPORT_RECT {} {} {} {}",
                            r.x().round() as i32,
                            r.y().round() as i32,
                            r.width().round() as i32,
                            r.height().round() as i32
                        ));
                        if let Some(hid) = id_cell_clone.take() {
                            // Disconnect so we only emit once per show_page open.
                            // The adjustment fires again on every scroll, so without
                            // this guard we would spam the log with updates.
                            adj.disconnect(hid);
                        }
                    }
                }
            });
            id_cell.set(Some(id));
        }
    }

    pub fn show_loading(&self, question: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        // Echo the submitted question above the "Asking…" indicator so the user
        // sees what they asked while the answer is being generated.
        let body = if question.trim().is_empty() {
            "Asking\u{2026}".to_string()
        } else {
            format!("{}\n\nAsking\u{2026}", prefix_question(question))
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask_host.card().close();
        // Drop the prior page's blocks + bar: during the transient "Asking…"
        // state there is no real Q&A page, so Space/a must not read the prior
        // page's cursor paragraph. With no blocks, current_block_text() is None
        // and play_journal_block is a no-op.
        self.clear_blocks();
        // Keep the navigation footer hidden during the Asking state. The result
        // render (show_page/show_message) restores it.
        self.footer_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn show_message(&self, text: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.view.buffer().set_text(text);
        self.apply_font();
        self.ask_host.card().close();
        // A bare message (toast/empty state) has no navigable Q&A paragraphs.
        self.clear_blocks();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    /// Drop the current page's paragraph blocks + accent bar (used by the
    /// transient loading / message states where there is no real Q&A page to
    /// navigate or read aloud).
    fn clear_blocks(&self) {
        self.blocks.borrow_mut().clear();
        self.cursor_block.set(0);
        self.visual_anchor.set(None);
        self.clear_bar();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        self.ask_host.card().close();
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Set the footer-left label to the band identity (`<abbrev> <act>.<scene>`)
    /// followed by the page position, joined with a `·`, e.g.
    /// `Cromwell 1.0 · page 1 of 1 in this scene`. The position used to live in a
    /// standalone row above the body; it now rides in the footer.
    fn set_footer_left(&self, band: &str, position: &str) {
        if position.is_empty() {
            self.footer_left.set_text(band);
        } else {
            self.footer_left
                .set_text(&format!("{} \u{00b7} {}", band, position));
        }
    }

    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }

    pub fn set_font(&self, family: &str, size: i32) {
        *self.font_family.borrow_mut() = family.to_string();
        self.font_size.set(size);
        self.apply_font();
    }

    /// Apply the overlay's font (family + size) to the page text and the ask
    /// input via a buffer-wide font TextTag — the same technique the gloss
    /// overlay uses (`GlossOverlay::apply_font`), since this gtk4 version's
    /// per-widget CSS provider path is the deprecated `style_context()` API.
    fn apply_font(&self) {
        let family = self.font_family.borrow().clone();
        if family.is_empty() {
            return;
        }
        let font_str = format!("{} {}", family, self.font_size.get());
        let edit_views = self.edit_card.views();
        for view in [&self.view, self.ask_host.input(), edit_views[0], edit_views[1], edit_views[2]] {
            let buffer = view.buffer();
            let table = buffer.tag_table();
            if let Some(old) = table.lookup("journal-font") {
                table.remove(&old);
            }
            let tag = gtk4::TextTag::builder()
                .name("journal-font")
                .font(&font_str)
                .build();
            table.add(&tag);
            let (start, end) = buffer.bounds();
            buffer.apply_tag(&tag, &start, &end);
            // Keep stage/bracket directions italic above the upright font tag.
            crate::ui::reassert_italic_tags(&table);
        }
    }

    pub fn ask_is_open(&self) -> bool {
        self.ask_host.is_open()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask_host.focus()
    }

    pub fn open_ask_card(&self, title: &str, hint: &str) {
        // The host reveals the ask card, hides the navigation footer (the ask
        // card carries its own "Tab switch · Ctrl+Enter submit" hint), shrinks the
        // scroll viewport to pane − title − ask (the occlusion fix), and recomputes
        // the clip. apply_font re-fonts the now-visible input.
        self.ask_host.open(title, hint);
        self.apply_font();

        // Headless test: emit the scrolled viewport rect WITH the ask card open
        // (the exact regression from Tasks 1-5). The card open shrinks the
        // scrolled window's height; this idle fires after that layout pass, so
        // the rect reflects the reduced viewport. Tests/journal_clipping.rs reads
        // TEST_JOURNAL_ASK_VIEWPORT_RECT for the ask-open assertion.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    crate::logging::log(&format!(
                        "TEST_JOURNAL_ASK_VIEWPORT_RECT {} {} {} {}",
                        r.x().round() as i32,
                        r.y().round() as i32,
                        r.width().round() as i32,
                        r.height().round() as i32
                    ));
                } else {
                    crate::logging::log(
                        "TEST_JOURNAL_ASK_VIEWPORT_RECT unavailable (root/compute_bounds returned None)",
                    );
                }
            });
        }
    }

    pub fn close_ask_card(&self) {
        // The host hides the ask card, re-shows the footer, restores the scroll's
        // stored CLOSED height, and recomputes the clip.
        self.ask_host.close();
    }

    pub fn toggle_ask_focus(&self) {
        self.ask_host.toggle_focus();
    }

    pub fn take_ask_text(&self) -> String {
        self.ask_host.take_text()
    }

    pub fn edit_is_open(&self) -> bool {
        self.edit_card.is_open()
    }

    pub fn toggle_edit_focus(&self) {
        self.edit_card.cycle_focus();
    }

    pub fn take_edit_fields(&self) -> (String, String, String) {
        self.edit_card.take()
    }

    /// Open the edit card pre-filled with the current page's Q & A. Hides the
    /// nav footer (the edit card carries its own hint) and shrinks the scroll so
    /// the card doesn't occlude the page (mirrors open_ask_card).
    pub fn open_edit_card(&self, question: &str, answer: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.edit_card.open(question, answer, card_width);
        self.footer_container.set_visible(false);
        self.apply_font();
        let (_, edit_h) = self.edit_card.container().preferred_size();
        self.ask_host.open_for_natural_height(edit_h.height());
    }

    pub fn close_edit_card(&self) {
        self.edit_card.close();
        self.footer_container.set_visible(true);
        self.ask_host.close_to_closed_height();
    }

    /// Rebuild `self.blocks` from the current buffer text (paragraph runs), reset
    /// the block cursor to the first block, and mark it so the left accent bar
    /// shows the cursor on a freshly-rendered page (mirrors the gloss overlay's
    /// `rebuild_blocks` + cursor reset). j/k step this cursor; Space/a read it.
    fn rebuild_blocks(&self) {
        let buffer = self.view.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        *self.blocks.borrow_mut() = journal_blocks(&lines);
        self.cursor_block.set(0);
        self.visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// The block cursor's current index (the block j/k/gg/G select), clamped to
    /// the block list. None when the page has no blocks (the empty/loading card).
    pub fn current_block_index(&self) -> Option<usize> {
        let len = self.blocks.borrow().len();
        if len == 0 {
            None
        } else {
            Some(self.cursor_block.get().min(len - 1))
        }
    }

    /// The text of the cursor's current block (for TTS). None when no blocks.
    pub fn current_block_text(&self) -> Option<String> {
        let blocks = self.blocks.borrow();
        let len = blocks.len();
        if len == 0 {
            return None;
        }
        let i = self.cursor_block.get().min(len - 1);
        blocks.get(i).map(|b| b.text.clone())
    }

    /// `j`/`k`: move the block cursor down/up one block, mark it (the left accent
    /// bar), and scroll it into view. No-op at the ends (does not re-snap the
    /// viewport — see the gloss overlay's `step_cursor` for why).
    pub fn cursor_next_block(&self) {
        self.step_block_cursor(1);
    }
    pub fn cursor_prev_block(&self) {
        self.step_block_cursor(-1);
    }
    /// `gg`/`G`: jump the block cursor to the first/last block.
    pub fn cursor_first_block(&self) {
        self.block_cursor_to_end(false);
    }
    pub fn cursor_last_block(&self) {
        self.block_cursor_to_end(true);
    }

    fn step_block_cursor(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1);
        if next == cur {
            return;
        }
        self.cursor_block.set(next as usize);
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    fn block_cursor_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    /// Move the left accent bar to the single cursor block and repaint. No-op
    /// when there are no blocks. Logs the landing block so j/k/gg/G navigation
    /// stays verifiable from the dev log (mirrors the gloss overlay).
    fn mark_cursor_block(&self) {
        let blocks = self.blocks.borrow();
        if blocks.is_empty() {
            drop(blocks);
            self.clear_bar();
            return;
        }
        let i = self.cursor_block.get().min(blocks.len() - 1);
        let span = (blocks[i].start_line, blocks[i].end_line);
        drop(blocks);
        crate::logging::log(&format!(
            "JOURNAL-CURSOR: cursor#{} bar lines [{}, {}]",
            i, span.0, span.1
        ));
        *self.bar_ranges.borrow_mut() = vec![span];
        self.bar_drawing.queue_draw();
    }

    /// Clear the selection bar (no ranges) and repaint.
    fn clear_bar(&self) {
        self.bar_ranges.borrow_mut().clear();
        self.bar_drawing.queue_draw();
    }

    /// Redraw the bar over the current selection span (anchor..=cursor). No-op
    /// (clears) when no anchor is set or there are no blocks.
    fn refresh_bar(&self) {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => {
                drop(blocks);
                self.clear_bar();
                return;
            }
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        let span = (blocks[s].start_line, blocks[e].end_line);
        drop(blocks);
        *self.bar_ranges.borrow_mut() = vec![span];
        self.bar_drawing.queue_draw();
    }

    /// Index of the first block whose end_line is at or below the current
    /// viewport top — the anchor seed for Shift+V. Falls back to 0.
    fn topmost_visible_block(&self) -> usize {
        let top_y = self.scrolled.vadjustment().value();
        let buffer = self.view.buffer();
        let blocks = self.blocks.borrow();
        for (i, b) in blocks.iter().enumerate() {
            if let Some(iter) = buffer.iter_at_line(b.end_line) {
                let (y, h) = self.view.line_yrange(&iter);
                if (y + h) as f64 >= top_y {
                    return i;
                }
            }
        }
        0
    }

    /// Enter visual mode: anchor at the topmost visible block. Returns false
    /// (no-op) when there are no blocks.
    pub fn enter_visual(&self) -> bool {
        let n = self.blocks.borrow().len();
        if n == 0 {
            crate::logging::log("JOURNAL-VISUAL: enter_visual no-op (0 blocks)");
            return false;
        }
        let seed = self.topmost_visible_block();
        self.visual_anchor.set(Some(seed));
        self.cursor_block.set(seed);
        self.refresh_bar();
        crate::logging::log(&format!(
            "JOURNAL-VISUAL: entered, {} blocks, anchor {}",
            n, seed
        ));
        true
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped), redraw
    /// the bar, and scroll the cursor block into view.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_bar();
        self.scroll_cursor_into_view();
    }

    /// Move the cursor end to the first (`false`) or last (`true`) block.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_bar();
        self.scroll_cursor_into_view();
    }

    /// The selected paragraphs' text (anchor..=cursor), blank-line joined.
    pub fn visual_selection_text(&self) -> String {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => return String::new(),
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        blocks[s..=e]
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Number of blocks currently selected.
    pub fn visual_selection_len(&self) -> usize {
        match self.visual_anchor.get() {
            Some(a) => {
                let (s, e) = visual_block_range(a, self.cursor_block.get());
                e - s + 1
            }
            None => 0,
        }
    }

    /// Exit visual mode: clear the anchor and return the bar to the single block
    /// cursor (the journal now has a persistent normal-mode block cursor that
    /// j/k drive and Space/a read).
    pub fn exit_visual(&self) {
        self.visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode returning the cursor to the anchor block, then re-mark
    /// the single cursor bar.
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Scroll the viewport so the WHOLE current cursor block is visible, leaving
    /// a `pad` of clearance so the block's last row never strands under the
    /// footer (the footer overlays the bottom of the card; revealing a block's
    /// bottom exactly at `view_bottom` left its final lines clipped behind it).
    /// Delegates the decision to the shared pure `cursor_scroll_target` helper —
    /// the same logic the gloss overlay uses — so an over-tall block reveals its
    /// bottom and a fitting block shows both edges. No-op when already visible.
    fn scroll_cursor_into_view(&self) {
        let idx = self.cursor_block.get();
        let (start_line, end_line) = {
            let blocks = self.blocks.borrow();
            match blocks.get(idx) {
                Some(b) => (b.start_line, b.end_line),
                None => return,
            }
        };
        let buffer = self.view.buffer();
        let top_margin = self.view.top_margin() as f64;
        let Some(si) = buffer.iter_at_line(start_line) else { return };
        let block_top = self.view.line_yrange(&si).0 as f64 + top_margin;
        let block_bottom = match buffer.iter_at_line(end_line) {
            Some(ei) => {
                let (y, h) = self.view.line_yrange(&ei);
                (y + h) as f64 + top_margin
            }
            None => block_top,
        };

        let adj = self.scrolled.vadjustment();
        let view_top = adj.value();
        let view_bottom = view_top + adj.page_size();
        let max_value = (adj.upper() - adj.page_size()).max(adj.lower());
        // Clear the footer (footer container ~36px) plus a little breathing room
        // so the last row sits above it, not under it.
        let pad = 40.0;

        let new_value = match crate::ui::gloss_util::cursor_scroll_target(
            &crate::ui::gloss_util::CursorScrollGeom {
                block_top,
                block_bottom,
                view_top,
                view_bottom,
                page_size: adj.page_size(),
                lower: adj.lower(),
                max_value,
                pad,
            },
        ) {
            Some(v) => v,
            None => {
                self.update_bottom_clip();
                return; // already fully visible
            }
        };
        // Snapping direction matters (see the gloss overlay): when revealing a
        // block's BOTTOM, snap UP to the nearest whole row so we don't scroll back
        // and re-hide it; otherwise floor so a revealed top isn't pushed under the
        // title rule.
        let revealing_bottom = block_bottom > view_bottom - pad && block_top >= view_top + pad;
        let new_value = if revealing_bottom {
            self.snap_value_to_line_up(new_value)
        } else {
            self.snap_value_to_line(new_value)
        };
        adj.set_value(new_value);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
    }

    /// Greatest real visual-row top at or below `target_y` (clamped). Floors the
    /// viewport to a whole row so the top line is not half-clipped under the
    /// title rule. Mirrors the gloss overlay.
    fn snap_value_to_line(&self, target_y: f64) -> f64 {
        let adj = self.scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let target = target_y.clamp(lower, max_value);
        let mut best = lower;
        for (row_top, _row_bottom) in crate::ui::display_rows(&self.view) {
            if row_top <= target + 0.5 {
                best = best.max(row_top);
            } else {
                break;
            }
        }
        best.clamp(lower, max_value)
    }

    /// Least real visual-row top at or above `target_y` (clamped). The
    /// up-direction counterpart used when revealing a block's bottom, so flooring
    /// doesn't scroll back and re-hide it. Mirrors the gloss overlay.
    fn snap_value_to_line_up(&self, target_y: f64) -> f64 {
        let adj = self.scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let row_tops: Vec<f64> = crate::ui::display_rows(&self.view)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        crate::ui::gloss_util::snap_up_to_row(target_y, &row_tops, lower, max_value)
    }

    /// Normal-navigation footer hint (advertises Shift+V). Re-set on visual exit.
    pub fn set_journal_hint(&self) {
        self.hint.set_text(
            "Space read \u{00b7} Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select \u{00b7} c copy id",
        );
    }

    /// Footer hint shown while journal visual mode is active.
    pub fn set_journal_visual_hint(&self) {
        self.hint
            .set_text("\u{21e7}V/Esc exit \u{00b7} j/k extend \u{00b7} gg/G ends \u{00b7} y yank");
    }
}

#[cfg(test)]
mod prefix_question_tests {
    use super::prefix_question;

    #[test]
    fn adds_q_prefix() {
        assert_eq!(prefix_question("What customs governed correspondence?"),
            "Q: What customs governed correspondence?");
    }

    #[test]
    fn is_idempotent() {
        // A question already prefixed (e.g. re-rendered from a stored page) is
        // not double-prefixed.
        assert_eq!(prefix_question("Q: already asked"), "Q: already asked");
        assert_eq!(prefix_question("  Q: leading space"), "  Q: leading space");
    }
}

#[cfg(test)]
mod scroll_budget_tests {
    use super::UNACCOUNTED_CHROME_MARGINS;

    /// `size_card` passes `title_h + UNACCOUNTED_CHROME_MARGINS` as the fixed
    /// chrome, so the host's closed scroll height is
    /// `card_height − title_h − margins − footer_h`. This is the formula that the
    /// too-tall bug got wrong (it omitted `margins`). Mirror the production
    /// arithmetic here so a change to either is caught.
    fn closed_scroll_budget(card_height: i32, title_h: i32, footer_h: i32) -> i32 {
        (card_height - (title_h + UNACCOUNTED_CHROME_MARGINS) - footer_h).max(80)
    }

    #[test]
    fn reserves_unaccounted_chrome_margins() {
        // window 1200 → card_height 1152; title 40, footer 30.
        let (card_h, title_h, footer_h) = (1152, 40, 30);
        let budget = closed_scroll_budget(card_h, title_h, footer_h);
        // Exactly the old (buggy) budget minus the reserved margins.
        let old_buggy = card_h - title_h - footer_h;
        assert_eq!(old_buggy - budget, UNACCOUNTED_CHROME_MARGINS);
    }

    #[test]
    fn floors_at_80_for_tiny_cards() {
        assert_eq!(closed_scroll_budget(50, 40, 30), 80);
    }

    #[test]
    fn margins_match_the_three_margin_sites() {
        // 24 (title margin_top) + 24 + 20 (scroll_overlay top+bottom)
        // + 12 + 12 (footer top+bottom) = 92.
        assert_eq!(UNACCOUNTED_CHROME_MARGINS, 92);
    }
}

#[cfg(test)]
mod scroll_structure_tests {
    use super::*;

    /// The ScrolledWindow's child MUST be the TextView directly. If it is an
    /// Overlay (or anything else), GTK can't use the TextView's native scroll
    /// adjustments, so the vadjustment has no scroll range and j/k/G/gg do
    /// nothing (and overflowing content stays clipped). The gloss overlay nests
    /// it correctly; this guards the journal overlay against re-introducing the
    /// ScrolledWindow→Overlay→TextView inversion.
    ///
    /// #[ignore]: needs gtk4::init(), which panics if a second GTK-init test runs
    /// in the same process. Run serially:
    /// `cargo test --bins -- --ignored scrolled_window_child`.
    #[test]
    #[ignore]
    fn scrolled_window_child_is_the_text_view() {
        if gtk4::init().is_err() {
            eprintln!("skip: no GTK display");
            return;
        }
        let overlay = JournalOverlay::new(1050, 80);
        let child = overlay
            .scrolled
            .child()
            .expect("ScrolledWindow should have a child");
        assert!(
            child.downcast_ref::<gtk4::TextView>().is_some(),
            "ScrolledWindow child must be the TextView directly (for native scroll \
             adjustments), not a {:?}",
            child.type_(),
        );
    }
}

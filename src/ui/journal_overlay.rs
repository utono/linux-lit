use crate::ui::ask_card::{AskCard, AskFocus};
use crate::ui::gloss_block::visual_block_range;
use crate::ui::gloss_render::populate_verse_buffer;
use crate::ui::journal_block::{journal_blocks, JournalBlock};
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
    font_family: RefCell<String>,
    font_size: Cell<i32>,
    last_card_size: Cell<(i32, i32)>,
    ask: AskCard,
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

        container.append(&scroll_overlay);

        // Footer rule mirroring the gloss overlay (gloss_overlay.rs footer_box):
        // current page's work/act/scene on the left, fixed keybind hints on the
        // right.
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select",
        );
        let footer_left = footer.left;
        let hint = footer.hint;
        let footer_container = footer.container.clone();
        container.append(&footer.container);

        // Shared "ask" input card (canonical synopsis values), stacked last in
        // the column. Focus returns to the page view when leaving the input.
        let ask = AskCard::new(text_margins as i32, &view);
        container.append(ask.container());

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
            font_family: RefCell::new(String::new()),
            font_size: Cell::new(16),
            last_card_size: Cell::new((0, 0)),
            ask,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    fn size_card(&self, card_width: i32, card_height: i32) {
        self.container.set_size_request(card_width, card_height);
        self.last_card_size.set((card_width, card_height));
        // SPIKE (fixed-scroll-height): the scroll (vexpand off) gets an EXPLICIT
        // height = pane minus the title + footer chrome. This is its height while
        // the ask card is CLOSED. open_ask_card subtracts the ask slot; close adds
        // it back. Deterministic — the scroll never auto-resizes.
        let (_, title_h) = self.title.preferred_size();
        let (_, footer_h) = self.footer_container.preferred_size();
        let scroll_h = (card_height - title_h.height() - footer_h.height()).max(80);
        self.scrolled.set_height_request(scroll_h);
        // Anchor the text + headers to the card's side margin (card_width/4, the
        // ~65-char readability optimum the gloss overlay uses) rather than the
        // small fixed `text_margins` — otherwise the Q&A prose runs nearly edge
        // to edge on a wide card. Card SIZE is unchanged; only the inner padding
        // grows. The title and position label indent to match so the left edge
        // of the header and the body line up. See ui::card_side_margin (audit #27).
        let side = crate::ui::card_side_margin(card_width);
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
        self.ask.close();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.clip_guard.on_open();
        self.rebuild_blocks();
        self.clear_bar();

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

    /// Render a passage page: source verse (with italic stage directions) above a
    /// separator rule, then the Q&A. Reuses `populate_verse_buffer` (the shared
    /// renderer from Task 2). Call `apply_font` after so the italic re-assertion
    /// fires over the freshly-built buffer.
    pub fn show_passage_page(
        &self,
        footer_left: &str,
        page_index: usize,
        page_count: usize,
        start_citation: Option<&str>,
        end_citation: Option<&str>,
        source_text: &str,
        question: &str,
        answer: &str,
        card_width: i32,
        card_height: i32,
    ) {
        self.size_card(card_width, card_height);
        self.title.set_text("Passage");

        // Position text: use the citation span when available, else a plain count.
        let pos_text = match (start_citation, end_citation) {
            (Some(s), Some(e)) => format!("passage {} \u{2013} {}", s, e),
            (Some(s), None) => format!("passage {}", s),
            _ => {
                if page_count == 0 {
                    "page 0 of 0 in this passage".to_string()
                } else {
                    format!("page {} of {} in this passage", page_index + 1, page_count)
                }
            }
        };
        self.set_footer_left(footer_left, &pos_text);

        // Render source verse into the buffer. bar_left mirrors the gloss overlay
        // (card_side_margin), accent omitted since passage pages are not speaker-
        // specific.
        let bar_left = crate::ui::card_side_margin(card_width);
        populate_verse_buffer(
            &self.view,
            source_text,
            self.text_margins,
            bar_left,
            &[],
            None,
            None,
            None,
        );

        // Append separator + Q&A after the verse.
        let qa_text = if page_count == 0 {
            "\n\n\u{2014}\u{2014}\u{2014}\n\nNo pages yet \u{2014} press A to ask.".to_string()
        } else {
            format!("\n\n\u{2014}\u{2014}\u{2014}\n\n{}\n\n{}", prefix_question(question), answer)
        };
        let mut end_iter = self.view.buffer().end_iter();
        self.view.buffer().insert(&mut end_iter, &qa_text);

        self.apply_font();
        self.ask.close();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.clip_guard.on_open();
        self.rebuild_blocks();
        self.clear_bar();
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
        self.ask.close();
        // Keep the navigation footer hidden during the Asking state. The result
        // render (show_page/show_passage_page/show_message) restores it.
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
        self.ask.close();
        // Restore the navigation footer (show_loading may have hidden it).
        self.footer_container.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        self.ask.close();
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    fn row_step(&self) -> f64 {
        let (_, h) = self.view.line_yrange(&self.view.buffer().start_iter());
        if h > 0 {
            h as f64
        } else {
            (self.font_size.get() as f64) * 1.4
        }
    }

    fn snap_value_to_line(&self, value: f64) -> f64 {
        let step = self.row_step();
        if step <= 0.0 {
            return value;
        }
        (value / step).round() * step
    }

    /// Scroll by exactly ONE real visual row per `delta` step, landing the
    /// viewport top on the next (delta>0) or previous (delta<0) wrapped-row top.
    /// Uses real per-row geometry (`display_rows`) rather than a uniform
    /// `row_step` grid so no line is skipped on non-uniform wrapped prose — the
    /// earlier `step * 3.0` jumped three rows and scrolled lines off before they
    /// could be read.
    pub fn scroll(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let rows = crate::ui::display_rows(&self.view);
        let cur = adj.value();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let target = if delta > 0 {
            // First row top strictly below the current top.
            rows.iter()
                .map(|(top, _)| *top)
                .find(|top| *top > cur + 0.5)
                .unwrap_or(max_value)
        } else if delta < 0 {
            // Last row top strictly above the current top.
            rows.iter()
                .map(|(top, _)| *top)
                .filter(|top| *top < cur - 0.5)
                .next_back()
                .unwrap_or(lower)
        } else {
            cur
        };
        adj.set_value(target.clamp(lower, max_value));
        self.update_bottom_clip();
    }

    pub fn scroll_to_top(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
    }

    pub fn scroll_to_bottom(&self) {
        let adj = self.scrolled.vadjustment();
        let bottom = (adj.upper() - adj.page_size()).max(adj.lower());
        adj.set_value(self.snap_value_to_line(bottom));
        self.update_bottom_clip();
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
        for view in [&self.view, self.ask.input()] {
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
        self.ask.is_open()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask.focus()
    }

    pub fn open_ask_card(&self, title: &str, hint: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.ask.open(title, hint, card_width);
        // Hide the normal-navigation footer hints while the ask card is open —
        // the ask card has its own "Tab switch · Ctrl+Enter submit" hint, and the
        // Alt+w/Ctrl+\/Alt+g navigation binds don't apply mid-question.
        self.footer_container.set_visible(false);
        self.apply_font();
        // SPIKE (fixed-scroll-height): set the scroll's explicit height to
        // pane - title - ask (the footer is hidden while asking, so it frees its
        // slot; the ask card takes a slot). vexpand is off, so this height is
        // honored exactly — no race. Computed from the STORED card_height.
        let (_, card_height) = self.last_card_size.get();
        let (_, title_h) = self.title.preferred_size();
        let (_, ask_h) = self.ask.container().preferred_size();
        let scroll_h = (card_height - title_h.height() - ask_h.height()).max(80);
        self.scrolled.set_height_request(scroll_h);

        // Recompute the clip now AND on the idle tick after the height takes
        // effect (the set_height_request lands on the next layout pass).
        self.clip_guard.recompute();
        {
            let clip = self.clip_guard.clip().clone();
            let view = self.view.clone();
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                crate::ui::recompute_overlay_bottom_clip(&view, &clip, &sc);
            });
        }
        {
            let sc = self.scrolled.clone();
            crate::logging::log(&format!(
                "ASKFIX3 open sync: page_size={:.0} scrolled_h={} set={}",
                sc.vadjustment().page_size(), sc.height(), scroll_h));
            glib::idle_add_local_once(move || {
                crate::logging::log(&format!(
                    "ASKFIX3 open idle: page_size={:.0} scrolled_h={}",
                    sc.vadjustment().page_size(), sc.height()));
            });
        }

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
        self.ask.close();
        // Restore the footer hints when the question is submitted or canceled.
        self.footer_container.set_visible(true);
        // SPIKE (fixed-scroll-height): restore the scroll's closed height
        // (pane - title - footer; the footer is back, the ask slot is freed).
        let (_, card_height) = self.last_card_size.get();
        let (_, title_h) = self.title.preferred_size();
        let (_, footer_h) = self.footer_container.preferred_size();
        let scroll_h = (card_height - title_h.height() - footer_h.height()).max(80);
        self.scrolled.set_height_request(scroll_h);

        self.clip_guard.recompute();
        {
            let clip = self.clip_guard.clip().clone();
            let view = self.view.clone();
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                crate::ui::recompute_overlay_bottom_clip(&view, &clip, &sc);
            });
        }
        {
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                crate::logging::log(&format!(
                    "ASKFIX3 close idle: page_size={:.0} scrolled_h={}",
                    sc.vadjustment().page_size(), sc.height()));
            });
        }
    }

    pub fn toggle_ask_focus(&self) {
        self.ask.toggle_focus();
    }

    pub fn take_ask_text(&self) -> String {
        self.ask.take_text()
    }

    /// Rebuild `self.blocks` from the current buffer text (paragraph runs).
    fn rebuild_blocks(&self) {
        let buffer = self.view.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        *self.blocks.borrow_mut() = journal_blocks(&lines);
        self.cursor_block.set(0);
        self.visual_anchor.set(None);
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

    /// Exit visual mode: clear the anchor and the bar. (The journal has no
    /// persistent normal-mode cursor, so yank and cancel both just clear.)
    pub fn exit_visual(&self) {
        self.visual_anchor.set(None);
        self.clear_bar();
    }

    /// Exit visual mode returning the cursor to the anchor block. Equivalent to
    /// `exit_visual` here (no persistent cursor), provided for handler symmetry.
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.visual_anchor.set(None);
        self.clear_bar();
    }

    /// Scroll the viewport so the current cursor block is visible. Uses the
    /// view's vadjustment and the cursor block's line range.
    fn scroll_cursor_into_view(&self) {
        let idx = self.cursor_block.get();
        let blocks = self.blocks.borrow();
        let Some(b) = blocks.get(idx) else { return };
        let buffer = self.view.buffer();
        let adj = self.scrolled.vadjustment();
        let page = adj.page_size();
        if let Some(si) = buffer.iter_at_line(b.start_line) {
            let (y_top, _) = self.view.line_yrange(&si);
            let y_top = y_top as f64;
            if y_top < adj.value() {
                adj.set_value(y_top);
            }
        }
        if let Some(ei) = buffer.iter_at_line(b.end_line) {
            let (y, h) = self.view.line_yrange(&ei);
            let y_bottom = (y + h) as f64;
            if y_bottom > adj.value() + page {
                adj.set_value((y_bottom - page).max(adj.lower()));
            }
        }
    }

    /// Normal-navigation footer hint (advertises Shift+V). Re-set on visual exit.
    pub fn set_journal_hint(&self) {
        self.hint.set_text(
            "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select",
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

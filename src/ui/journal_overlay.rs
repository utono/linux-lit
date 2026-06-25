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
    position_label: Label,
    scrolled: gtk4::ScrolledWindow,
    view: gtk4::TextView,
    bottom_clip: gtk4::Box,
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

        let position_label = Label::new(Some(""));
        position_label.add_css_class("gloss-header");
        position_label.set_halign(gtk4::Align::Start);
        position_label.set_margin_start(text_margins as i32);
        position_label.set_margin_end(text_margins as i32);
        container.append(&position_label);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);
        scrolled.set_propagate_natural_height(false);
        scrolled.set_vexpand(true);

        let view = gtk4::TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_focusable(false);
        view.set_wrap_mode(gtk4::WrapMode::Word);
        view.add_css_class("gloss-text");

        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&view));
        let bottom_clip = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        bottom_clip.add_css_class("gloss-bottom-clip");
        bottom_clip.set_valign(gtk4::Align::End);
        bottom_clip.set_halign(gtk4::Align::Fill);
        bottom_clip.set_vexpand(false);
        bottom_clip.set_can_target(false);
        scroll_overlay.add_overlay(&bottom_clip);
        scroll_overlay.set_measure_overlay(&bottom_clip, false);
        scroll_overlay.set_clip_overlay(&bottom_clip, true);

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
                let x = 4.0; // left inset; the card side margin already pads the text
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

        scrolled.set_child(Some(&scroll_overlay));
        container.append(&scrolled);

        // Footer rule mirroring the gloss overlay (gloss_overlay.rs footer_box):
        // current page's work/act/scene on the left, fixed keybind hints on the
        // right.
        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select",
        );
        let footer_left = footer.left;
        let hint = footer.hint;
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
            position_label,
            scrolled,
            view,
            bottom_clip,
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
        self.position_label.set_margin_start(side);
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
        self.footer_left.set_text(footer_left);
        if page_count == 0 {
            self.position_label.set_text("page 0 of 0 in this scene");
        } else {
            self.position_label.set_text(&format!(
                "page {} of {} in this scene",
                page_index + 1,
                page_count
            ));
        }
        let body = if page_count == 0 {
            "No pages yet \u{2014} press A to ask.".to_string()
        } else {
            format!("{}\n\n{}", question, answer)
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask.close();
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
        self.rebuild_blocks();
        self.clear_bar();
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
        self.footer_left.set_text(footer_left);

        // Position label: use the citation span when available, else a plain count.
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
        self.position_label.set_text(&pos_text);

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
            format!("\n\n\u{2014}\u{2014}\u{2014}\n\n{}\n\n{}", question, answer)
        };
        let mut end_iter = self.view.buffer().end_iter();
        self.view.buffer().insert(&mut end_iter, &qa_text);

        self.apply_font();
        self.ask.close();
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
        self.rebuild_blocks();
        self.clear_bar();
    }

    pub fn show_loading(&self) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.position_label.set_text("");
        self.view.buffer().set_text("Asking\u{2026}");
        self.apply_font();
        self.ask.close();
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

    pub fn scroll(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let step = self.row_step();
        let raw = adj.value() + step * 3.0 * delta as f64;
        adj.set_value(self.snap_value_to_line(raw));
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

    fn update_bottom_clip(&self) {
        let adj = self.scrolled.vadjustment();
        let step = self.row_step();
        if step <= 0.0 {
            self.bottom_clip.set_size_request(-1, 0);
            return;
        }
        let page = adj.page_size();
        let remainder = page - (page / step).floor() * step;
        let clip_h = remainder.round().max(0.0) as i32;
        self.bottom_clip.set_size_request(-1, clip_h);
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
        self.apply_font();
    }

    pub fn close_ask_card(&self) {
        self.ask.close();
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
        if self.blocks.borrow().is_empty() {
            return false;
        }
        let seed = self.topmost_visible_block();
        self.visual_anchor.set(Some(seed));
        self.cursor_block.set(seed);
        self.refresh_bar();
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

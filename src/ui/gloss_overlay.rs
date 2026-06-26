use crate::ui::ask_card::{AskCard, AskFocus};
use crate::ui::gloss_block::{
    gloss_blocks, render_synopsis_with_labels, selected_blocks_text,
    synopsis_blocks, visual_block_range, BlockKind, GlossBlock,
};
use crate::ui::gloss_render::{
    populate_gloss_buffer, populate_verse_buffer, BarRange, LineNumber,
};
use crate::ui::gloss_util::{
    build_diff_markup, cursor_scroll_target, format_citation_range, parse_hex_color,
    snap_up_to_row, CursorScrollGeom,
};
use gtk4::pango;
use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Buffer-line span of one cursor-stop block (source or explication).
struct BlockRange {
    kind: BlockKind,
    index: i32,
    start_line: i32,
    end_line: i32,
}

pub struct GlossOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    orig_header: Label,
    original_label: Label,
    corr_header: Label,
    corrected_label: Label,
    hint: Label,
    citation_label: Label,
    position_label: Label,
    gloss_scroll_overlay: Overlay,
    gloss_scrolled: gtk4::ScrolledWindow,
    gloss_view: gtk4::TextView,
    bar_drawing: gtk4::DrawingArea,
    /// Owns the clip Box pinned to the bottom of the gloss viewport and all three
    /// recompute paths (value_changed catch-all, reset_scroll_top range+idle,
    /// update_bottom_clip). Replaces the hand-wired `bottom_clip` + inline
    /// connect_value_changed + reset_scroll_top body.
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    bar_ranges: Rc<RefCell<Vec<BarRange>>>,
    bar_color: Rc<RefCell<(f64, f64, f64)>>,
    bar_x: Rc<RefCell<i32>>,
    line_numbers: Rc<RefCell<Vec<LineNumber>>>,
    echo_lines: Rc<RefCell<Vec<i32>>>,
    echo_header_view: gtk4::TextView,
    echo_rule: gtk4::Separator,
    text_margins: i32,
    column_width: i32,
    /// The overlay's own font (independent of the main reader). `!`/`|` adjust
    /// the size while a gloss/synopsis is open without touching the main card.
    /// Applied as a font TextTag over the gloss buffer on every show, overriding
    /// the global `.gloss-text` CSS. Defaults to Charter 19pt.
    font_family: RefCell<String>,
    font_size: std::cell::Cell<i32>,
    /// Char ranges (start, end) of standalone label paragraphs in the current
    /// synopsis buffer (e.g. "Shakespearean parallels:"), bolded on show and
    /// re-asserted after every `apply_font` (which else overrides their weight
    /// with the regular-weight buffer-wide font tag). Empty for glosses/echoes.
    synopsis_label_ranges: RefCell<Vec<(usize, usize)>>,
    /// Last `(card_width, card_height)` a show_* call sized the card to. The
    /// loading state reuses it so "Glossing…" presents as a full card rather
    /// than a label-sized box. `(0,0)` until the first card is shown.
    last_card_size: Cell<(i32, i32)>,
    /// Cursor-stop blocks of the currently shown gloss, with their buffer
    /// line spans, in document order. Empty in echo/synopsis/glossing modes.
    blocks: Rc<RefCell<Vec<BlockRange>>>,
    /// Index into `blocks` of the selected cursor block. `j`/`k` step it, `gg`/
    /// `G` jump to first/last; the accent bar marks it and `Space` acts on it.
    /// Reset to 0 on each gloss render. The cursor is an explicit selection, NOT
    /// derived from scroll position (a tall card's viewport center can leave
    /// top/bottom blocks unreachable — see GLOSS-CURSOR debug logging).
    cursor_block: Cell<usize>,
    /// `Some(block_index)` while synopsis visual mode is active — the anchor end
    /// of the selection. The cursor end is `cursor_block`. `None` in normal
    /// synopsis navigation. Selected range: `visual_block_range(anchor, cursor)`.
    synopsis_visual_anchor: Cell<Option<usize>>,
    /// The synopsis string currently shown (raw, `<p>`-tagged), retained so
    /// visual-mode yank can rebuild the selected paragraphs via
    /// `selected_blocks_text`. Set by `show_synopsis`.
    current_synopsis: RefCell<String>,
    /// Shared "ask" input card, stacked below the synopsis/gloss card inside the
    /// same `container` (after the footer). Serves both the synopsis "ask" flow
    /// and the gloss add/edit prompts. See `crate::ui::ask_card::AskCard`.
    ask: AskCard,
}

/// Default font for the synopsis/gloss/echoes overlay cards.
const GLOSS_DEFAULT_FONT_FAMILY: &str = "Charter";
const GLOSS_DEFAULT_FONT_SIZE: i32 = 19;

impl GlossOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.set_width_request(column_width as i32);
        container.add_css_class("gloss-overlay");

        let title = Label::new(Some("Gloss"));
        title.add_css_class("gloss-title");
        title.set_margin_start(text_margins as i32);
        title.set_margin_end(text_margins as i32);
        title.set_margin_top(24);
        container.append(&title);

        // Left margin for these diff/error labels is set per-display in `show()`
        // (card_width/4), so the error/diff card lines up with the loading and
        // result cards rather than hugging the container's left edge.
        let orig_header = Label::new(Some("ORIGINAL"));
        orig_header.add_css_class("gloss-header");
        orig_header.set_halign(Align::Start);
        container.append(&orig_header);

        let original_label = Label::new(None);
        original_label.set_wrap(true);
        original_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        original_label.set_halign(Align::Start);
        original_label.set_hexpand(false);
        original_label.set_selectable(false);
        original_label.set_max_width_chars(1);
        original_label.add_css_class("gloss-text");
        container.append(&original_label);

        let corr_header = Label::new(Some("GLOSS"));
        corr_header.add_css_class("gloss-header");
        corr_header.set_halign(Align::Start);
        container.append(&corr_header);

        let corrected_label = Label::new(None);
        corrected_label.set_wrap(true);
        corrected_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        corrected_label.set_halign(Align::Start);
        corrected_label.set_hexpand(false);
        corrected_label.set_selectable(false);
        corrected_label.set_max_width_chars(1);
        corrected_label.add_css_class("gloss-text");
        container.append(&corrected_label);

        let gloss_scroll_overlay = Overlay::new();

        let gloss_scrolled = gtk4::ScrolledWindow::new();
        gloss_scrolled.set_vexpand(true);
        gloss_scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        gloss_scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);
        // Report a small natural height (not the full text height) so the
        // fixed-height card honors its `height_request` instead of growing to
        // fit the whole synopsis. The vexpand then distributes the card's
        // remaining space to this viewport AFTER the title/footer/ask card take
        // theirs — which is what keeps the stacked ask card inside the card
        // rather than spilling past the reader's rounded frame.
        gloss_scrolled.set_propagate_natural_height(false);

        let gloss_view = gtk4::TextView::new();
        gloss_view.set_editable(false);
        gloss_view.set_cursor_visible(false);
        gloss_view.set_focusable(false);
        gloss_view.set_wrap_mode(gtk4::WrapMode::Word);
        let right_margin = column_width as i32 / 8;
        gloss_view.set_left_margin(text_margins as i32);
        gloss_view.set_right_margin(right_margin);
        gloss_view.set_top_margin(24);
        gloss_view.set_bottom_margin(80);
        gloss_view.add_css_class("gloss-text");

        let bar_drawing = gtk4::DrawingArea::new();
        bar_drawing.set_can_target(false);

        let bar_ranges: Rc<RefCell<Vec<BarRange>>> = Rc::new(RefCell::new(Vec::new()));
        let bar_color: Rc<RefCell<(f64, f64, f64)>> = Rc::new(RefCell::new((0.53, 0.62, 0.71)));
        let bar_x: Rc<RefCell<i32>> = Rc::new(RefCell::new((column_width as i32) / 8));
        let line_numbers: Rc<RefCell<Vec<LineNumber>>> = Rc::new(RefCell::new(Vec::new()));
        let echo_lines: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let blocks: Rc<RefCell<Vec<BlockRange>>> = Rc::new(RefCell::new(Vec::new()));

        let ranges_clone = bar_ranges.clone();
        let color_clone = bar_color.clone();
        let bar_x_clone = bar_x.clone();
        let line_numbers_clone = line_numbers.clone();
        let view_clone = gloss_view.clone();
        let right_margin_val = right_margin;
        bar_drawing.set_draw_func(move |_area, cr, w, _h| {
            let ranges = ranges_clone.borrow();
            let (r, g, b) = *color_clone.borrow();
            let x = *bar_x_clone.borrow() as f64;

            // Draw bars
            if !ranges.is_empty() {
                cr.set_source_rgb(r, g, b);
                cr.set_line_width(2.0);

                let buffer = view_clone.buffer();
                for range in ranges.iter() {
                    let start_iter = buffer.iter_at_line(range.start_line);
                    let end_iter = buffer.iter_at_line(range.end_line);
                    if let (Some(si), Some(ei)) = (start_iter, end_iter) {
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
            }

            // Draw line numbers (every 5th)
            let nums = line_numbers_clone.borrow();
            if !nums.is_empty() {
                cr.set_source_rgb(r, g, b);
                let font_size = {
                    let pango_ctx = view_clone.pango_context();
                    let font_desc = pango_ctx.font_description().unwrap_or_default();
                    (font_desc.size() as f64 / pango::SCALE as f64) * 0.7
                };
                cr.select_font_face("serif", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
                cr.set_font_size(font_size);

                let buffer = view_clone.buffer();
                let num_x = (w - right_margin_val + 8) as f64;

                for ln in nums.iter() {
                    if ln.number % 5 != 0 {
                        continue;
                    }
                    if let Some(iter) = buffer.iter_at_line(ln.buffer_line) {
                        let loc = view_clone.iter_location(&iter);
                        let (_, by) = view_clone.buffer_to_window_coords(
                            gtk4::TextWindowType::Widget, 0, loc.y());
                        let text = ln.number.to_string();
                        let _ = cr.move_to(num_x, by as f64 + font_size);
                        let _ = cr.show_text(&text);
                    }
                }
            }
        });

        gloss_scrolled.set_child(Some(&gloss_view));

        // The bar overlay (accent bar, source/echo rule, line numbers) maps
        // buffer lines to window y at paint time, so it must repaint whenever
        // the view scrolls — otherwise the rule and bar stick at a stale
        // scroll offset while the text moves beneath them.
        {
            let bar_for_scroll = bar_drawing.clone();
            gloss_scrolled.vadjustment().connect_value_changed(move |_| {
                bar_for_scroll.queue_draw();
            });
        }

        gloss_scroll_overlay.set_child(Some(&gloss_scrolled));
        gloss_scroll_overlay.add_overlay(&bar_drawing);
        gloss_scroll_overlay.set_measure_overlay(&bar_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&bar_drawing, true);

        // Attach the bottom-clip guard: builds the clip Box, adds it as a
        // non-measured, clipped overlay, and wires the persistent value_changed
        // catch-all (path c). All three recompute paths (c / on_open / recompute)
        // are now owned by the guard.
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &gloss_scroll_overlay,
            &gloss_view,
            &gloss_scrolled,
        );

        gloss_scroll_overlay.set_visible(false);

        // Echoes-only: a fixed source-turn header + a fixed rule, above the
        // scrolling echo list. Hidden in all non-echo overlay modes.
        let echo_header_view = gtk4::TextView::new();
        echo_header_view.set_editable(false);
        echo_header_view.set_cursor_visible(false);
        echo_header_view.set_focusable(false);
        echo_header_view.set_wrap_mode(gtk4::WrapMode::Word);
        echo_header_view.set_left_margin(text_margins as i32);
        echo_header_view.set_right_margin(right_margin);
        echo_header_view.set_top_margin(24);
        echo_header_view.add_css_class("gloss-text");
        echo_header_view.set_visible(false);
        container.append(&echo_header_view);

        let echo_rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        echo_rule.set_margin_start(text_margins as i32);
        echo_rule.set_margin_end(right_margin);
        // Breathing room between the source turn's last line and the rule.
        echo_rule.set_margin_top(16);
        echo_rule.set_visible(false);
        container.append(&echo_rule);

        // Breathing room between the scrolling text viewport and the footer
        // hint bar. Without it the viewport's bottom edge sits flush against the
        // footer's top border, so the last visible text line is bisected by the
        // rule and reads as clipped (the symptom this margin fixes). The 80px
        // bottom margin inside `gloss_view` only helps once scrolled fully to
        // the end; this gap keeps the last line clear at any scroll position.
        gloss_scroll_overlay.set_margin_bottom(20);
        // Symmetric gap below the title rule. The viewport top otherwise sits
        // flush under the title, so when the text is scrolled a partial line at
        // the top edge reads as clipped by the title. (The gloss_view internal
        // top margin scrolls away with the content, so it can't keep this gap.)
        gloss_scroll_overlay.set_margin_top(24);

        container.append(&gloss_scroll_overlay);

        let footer = crate::ui::footer::build_footer_row(
            text_margins as i32,
            "J journal · Ctrl+j view jrnl",
        );
        let footer_box = footer.container;
        let citation_label = footer.left;
        citation_label.set_visible(false);
        let hint = footer.hint;

        let position_label = Label::new(None);
        position_label.set_halign(Align::End);
        position_label.set_visible(false);
        footer_box.append(&position_label);

        container.append(&footer_box);

        // ---- Shared "ask" input card, stacked below the synopsis -------------
        // Lives inside `container` so the two cards form one centered column and
        // the synopsis scroll viewport (which vexpands) shrinks to make room when
        // this card is revealed. Hidden until `A` opens it. Built from the
        // canonical values inside `AskCard`; focus returns to `gloss_scrolled`.
        let ask = AskCard::new(text_margins as i32, &gloss_scrolled);
        container.append(ask.container());

        container.set_visible(false);

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        GlossOverlay {
            overlay,
            scrim,
            container,
            title,
            orig_header,
            original_label,
            corr_header,
            corrected_label,
            hint,
            citation_label,
            position_label,
            gloss_scroll_overlay,
            gloss_scrolled,
            gloss_view,
            bar_drawing,
            clip_guard,
            bar_ranges,
            bar_color,
            bar_x,
            line_numbers,
            echo_lines,
            echo_header_view,
            echo_rule,
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            font_family: RefCell::new(GLOSS_DEFAULT_FONT_FAMILY.to_string()),
            font_size: std::cell::Cell::new(GLOSS_DEFAULT_FONT_SIZE),
            synopsis_label_ranges: RefCell::new(Vec::new()),
            last_card_size: Cell::new((0, 0)),
            blocks,
            cursor_block: Cell::new(0),
            synopsis_visual_anchor: Cell::new(None),
            current_synopsis: RefCell::new(String::new()),
            ask,
        }
    }

    /// Adjust the overlay's own font size by `delta` pt (clamped), then re-apply
    /// it to the currently-shown gloss text. Independent of the main reader font.
    pub fn adjust_font_size(&self, delta: i32) {
        let new_size = (self.font_size.get() + delta).clamp(8, 72);
        self.font_size.set(new_size);
        self.apply_font();
    }

    /// Apply the overlay's font (family + size) to the gloss text and header via
    /// a buffer-wide font TextTag, overriding the global `.gloss-text` CSS. Call
    /// after each populate so a rebuilt buffer keeps the chosen size.
    pub fn apply_font(&self) {
        let font_str = format!("{} {}", self.font_family.borrow(), self.font_size.get());
        for view in [&self.gloss_view, &self.echo_header_view, self.ask.input()] {
            let buffer = view.buffer();
            let table = buffer.tag_table();
            if let Some(old) = table.lookup("gloss-font") {
                table.remove(&old);
            }
            let tag = gtk4::TextTag::builder().name("gloss-font").font(&font_str).build();
            table.add(&tag);
            let (start, end) = buffer.bounds();
            buffer.apply_tag(&tag, &start, &end);
            // Keep stage/bracket directions italic above the upright font tag.
            crate::ui::reassert_italic_tags(&table);
        }
        // The buffer-wide font tag carries the family's regular weight, so it
        // overrides any earlier bold tag. Re-assert the synopsis label bold so
        // it wins (it is added/applied last, hence highest priority).
        self.apply_synopsis_label_bold();
    }

    /// Bold the stored synopsis label ranges on the gloss view. Adds the
    /// `synopsis-label` weight tag if absent and (re-)applies it last so it
    /// outranks the regular-weight `gloss-font` tag. No-op when no ranges are
    /// stored (glosses, echoes, loading states).
    fn apply_synopsis_label_bold(&self) {
        let ranges = self.synopsis_label_ranges.borrow();
        if ranges.is_empty() {
            return;
        }
        let buffer = self.gloss_view.buffer();
        let table = buffer.tag_table();
        if table.lookup("synopsis-label").is_none() {
            table.add(
                &gtk4::TextTag::builder()
                    .name("synopsis-label")
                    .weight(700)
                    .build(),
            );
        }
        if let Some(tag) = table.lookup("synopsis-label") {
            // Tag conflicts resolve by priority, which defaults to add-order.
            // The `gloss-font` tag (regular weight) is added after this one on
            // the first show, so it would win. Force the label to the highest
            // priority so its bold weight outranks the font tag's weight.
            let size = table.size();
            if size > 0 {
                tag.set_priority(size - 1);
            }
            for &(start, end) in ranges.iter() {
                let s = buffer.iter_at_offset(start as i32);
                let e = buffer.iter_at_offset(end as i32);
                buffer.apply_tag(&tag, &s, &e);
            }
        }
    }

    /// Color every block whose `is_cached(kind, index)` returns true with the
    /// theme accent (`accent`, = theme `cursor_bg`) — deliberately DISTINCT from
    /// the bar/divider `root_color`, so a synthesized block reads as "active"
    /// rather than blending into the bar. Idempotent; re-tagging an
    /// already-colored block is harmless. Call AFTER `apply_font` with
    /// `self.blocks` already populated (every `show_*` path does both). The
    /// injected `is_cached` predicate must NOT borrow the overlay's own block
    /// state (it runs while `self.blocks` is borrowed), as a re-entrant borrow
    /// would panic.
    pub fn color_audio_blocks(&self, accent: &str, is_cached: impl Fn(&BlockKind, i32) -> bool) {
        let buffer = self.gloss_view.buffer();
        let table = buffer.tag_table();
        let rgba = match parse_hex_color(accent) {
            Some((r, g, b)) => format!(
                "#{:02x}{:02x}{:02x}",
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
            ),
            None => accent.to_string(),
        };
        let tag = match table.lookup("gloss-audio-cached") {
            Some(t) => {
                t.set_foreground(Some(&rgba));
                t
            }
            None => {
                let t = gtk4::TextTag::builder()
                    .name("gloss-audio-cached")
                    .foreground(&rgba)
                    .build();
                table.add(&t);
                t
            }
        };
        // Outrank the buffer-wide `gloss-font` tag (added last on first show).
        let size = table.size();
        if size > 0 {
            tag.set_priority(size - 1);
        }
        let line_count = buffer.line_count();
        let blocks = self.blocks.borrow();
        crate::log_fmt!(
            "COLOR-AUDIO: {} blocks, prio set to {}, tag fg={}",
            blocks.len(), tag.priority(), rgba
        );
        for blk in blocks.iter() {
            if !is_cached(&blk.kind, blk.index) {
                continue;
            }
            // A Source block's range begins at its first VERSE line; the speaker
            // heading (gloss_blocks drops it from the block text) sits one line
            // above. Recolor it together with the verse so the whole turn —
            // label and body — reads as cached. Only extend when that line truly
            // carries a speaker tag, so we never bleed the accent onto a
            // preceding verse/prose line of another block.
            let start_line = if blk.kind == BlockKind::Source
                && blk.start_line > 0
                && line_is_speaker(&buffer, blk.start_line - 1)
            {
                blk.start_line - 1
            } else {
                blk.start_line
            };
            let start = buffer
                .iter_at_line(start_line)
                .unwrap_or_else(|| buffer.start_iter());
            let end_line = (blk.end_line + 1).min(line_count);
            let end = buffer
                .iter_at_line(end_line)
                .unwrap_or_else(|| buffer.end_iter());
            buffer.apply_tag(&tag, &start, &end);
            crate::log_fmt!(
                "COLOR-AUDIO: tagged {:?}#{} lines [{}, {})",
                blk.kind, blk.index, start_line, end_line
            );
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
    }

    pub fn show(&self, original: &str, corrected: &str) {
        self.hide_citation();
        self.title.set_visible(true);
        self.title.set_text("Gloss");
        // Reset the top margin in case `show_glossing` widened it (shared title).
        self.title.set_margin_top(24);
        // Indent the title and the diff/error labels to the same card_width/4
        // the loading ("Glossing…") and result cards use, so the error/diff card
        // lines up with them instead of hugging the left edge. Reuse the last
        // rendered card width (an error/toast always follows a card render); fall
        // back to the container's own width if a card was never shown.
        let card_width = match self.last_card_size.get().0 {
            w if w > 0 => w,
            _ => self.container.width().max(self.container.width_request()),
        };
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_margin_start(left);
        self.orig_header.set_margin_start(left);
        self.original_label.set_margin_start(left);
        self.corr_header.set_margin_start(left);
        self.corrected_label.set_margin_start(left);
        let orig_markup = build_diff_markup(original, corrected, true);
        let corr_markup = build_diff_markup(original, corrected, false);
        self.original_label.set_markup(&orig_markup);
        self.corrected_label.set_markup(&corr_markup);
        self.orig_header.set_visible(true);
        self.original_label.set_visible(true);
        self.corr_header.set_visible(true);
        self.corrected_label.set_visible(true);
        self.gloss_scroll_overlay.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
    }

    pub fn show_gloss_with_color(&self, _original: &str, gloss: &str, card_width: i32, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        // No synopsis label bolding in gloss view.
        self.synopsis_label_ranges.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh gloss render closes any open add/edit ask card and clears its
        // focus highlight (e.g. after an add/edit completes or n/p navigates).
        self.ask.close();
        self.title.set_visible(false);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        // Wide side margins keep gloss prose near the ~65-char readability
        // optimum. Anchor to the actual card width (the overlay is full-screen,
        // ~1660px), NOT the fixed column_width (1050) — otherwise on a wide card
        // the margin stays tiny and the text runs nearly edge to edge.
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
        self.gloss_view.set_top_margin(32);
        self.gloss_view.set_pixels_below_lines(4);
        self.set_gloss_hint();
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);

        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }

        let bar_left = crate::ui::card_side_margin(card_width);
        *self.bar_x.borrow_mut() = bar_left;

        // Gloss prose and speaker headings both keep the normal foreground.
        // The prose is set off from the verse only by a slightly smaller scale
        // and looser line spacing (no color dimming, no speaker tint).
        let (ranges, _nums) = populate_gloss_buffer(
            &self.gloss_view, gloss, self.text_margins, bar_left, source_line_numbers,
            None, None,
        );
        *self.bar_ranges.borrow_mut() = ranges;
        // Glosses do not show verse line numbers (those belong only to the main
        // reading view); clear any the buffer produced.
        self.line_numbers.borrow_mut().clear();
        *self.echo_lines.borrow_mut() = Vec::new();
        self.bar_drawing.queue_draw();

        self.rebuild_block_ranges(gloss);
        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        self.reset_scroll_top();
        self.mark_cursor_block();
        // mark_cursor_block sets bar_ranges, but the bar DRAW reads per-line
        // geometry (line_yrange) which is 0/stale until GTK lays out the buffer
        // just made visible above — so the synchronous draw paints nothing and
        // the accent bar only appeared after the first j/k/Alt+n. Repaint once
        // more after layout settles (same fix the echo/synopsis path uses).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());
    }

    /// "Glossing…" loading card that shows the passage being glossed, rendered
    /// single-column with the SAME `<speaker>`/`<verse>` formatting the gloss
    /// result uses for the original passage. `passage_doc` is the
    /// `<speaker>`/`<verse>` markup (see `build_source_header`). The "Glossing…"
    /// status sits as a header above the passage; the result simply replaces this
    /// view in place when it arrives, so the passage looks identical before/after.
    pub fn show_glossing(&self, passage_doc: &str, card_width: i32, card_height: i32, root_color: Option<&str>) {
        self.hide_citation();
        self.synopsis_label_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask.close();

        // "Glossing…" as a top header (not the centered label of
        // `show_loading_message`), matching the gloss result's title placement.
        self.title.set_text("Glossing\u{2026}");
        self.title.set_visible(true);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        // Extra breathing room above the header (constructor default is 24).
        self.title.set_margin_top(64);

        // Same passage geometry the gloss result uses (`show_gloss_with_color`):
        // wide side margins anchored to the actual card width, accent bar at
        // card_width/4.
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
        self.gloss_view.set_top_margin(32);
        self.gloss_view.set_pixels_below_lines(4);

        // No diff labels, echo views, hint, or position while loading.
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        self.hint.set_visible(false);
        self.position_label.set_visible(false);

        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }

        let bar_left = crate::ui::card_side_margin(card_width);
        *self.bar_x.borrow_mut() = bar_left;

        // Render the passage through the SAME path as the gloss result's original
        // passage, so speaker small-caps + indented verse look identical.
        let (ranges, _nums) = populate_gloss_buffer(
            &self.gloss_view, passage_doc, self.text_margins, bar_left, &[],
            None, None,
        );
        *self.bar_ranges.borrow_mut() = ranges;
        self.line_numbers.borrow_mut().clear();
        *self.echo_lines.borrow_mut() = Vec::new();
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        self.reset_scroll_top();
    }

    /// Render the echoes overlay: a fixed source-turn header + rule, above the
    /// scrolling echo list. `source_doc` is the <speaker>/<verse> turn; `echo_doc`
    /// is only the <gloss> lines.
    pub fn show_echoes(
        &self,
        source_doc: &str,
        echo_doc: &str,
        card_width: i32,
        card_height: i32,
        root_color: Option<&str>,
        dim_color: Option<&str>,
        selected: usize,
    ) {
        // No synopsis label bolding in echo view.
        self.hide_citation();
        self.synopsis_label_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask.close();
        self.title.set_visible(false);
        let left = self.column_width / 8;
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        // Reset margins/spacing the synopsis and gloss views may have widened,
        // so the echo list and its accent bar stay aligned at column_width/8.
        self.gloss_view.set_right_margin(self.column_width / 8);
        self.gloss_view.set_top_margin(24);
        self.gloss_view.set_pixels_below_lines(0);
        self.echo_header_view.set_left_margin(left);
        self.hint.set_text("Esc close · a play · A add · s curate · d/D delete · R refresh");
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);

        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }

        let bar_left = self.column_width / 8;
        *self.bar_x.borrow_mut() = bar_left;

        // Fixed header: render the source turn into the non-scrolling view.
        // Reuse populate_verse_buffer (it builds the speaker/verse tags and
        // returns empty bar data for a source-only doc).
        let _ = populate_verse_buffer(
            &self.echo_header_view, source_doc, self.text_margins, bar_left, &[], None, dim_color, None);
        self.echo_header_view.set_visible(true);
        self.echo_rule.set_visible(true);

        // Scrolling list: only the echoes. echo_lines/bar_ranges are now indexed
        // from the first echo (no source lines to offset past).
        let (ranges, nums, echo_lines) = populate_verse_buffer(
            &self.gloss_view, echo_doc, self.text_margins, bar_left, &[], Some(selected), dim_color, None);
        *self.bar_ranges.borrow_mut() = ranges;
        *self.line_numbers.borrow_mut() = nums;
        *self.echo_lines.borrow_mut() = echo_lines;
        // Repaint the bar overlay after GTK lays out the rebuilt buffer (drawing
        // synchronously reads stale per-line geometry).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());

        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        self.reset_scroll_top();
    }

    /// Scroll the card so the Nth echo's quote line is visible.
    pub fn scroll_echo_into_view(&self, echo_index: usize) {
        let line = match self.echo_lines.borrow().get(echo_index).copied() {
            Some(l) => l,
            None => return,
        };
        // Scroll the ScrolledWindow's adjustment (the actual scroller) rather
        // than the TextView's own — the view is sized to its full content, so
        // gloss_view.scroll_to_mark is a no-op here. Defer to idle so the
        // rebuilt buffer has been laid out before we query line/adjustment
        // geometry (querying synchronously after set_text yields stale bounds).
        let view = self.gloss_view.clone();
        let scrolled = self.gloss_scrolled.clone();
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || {
            let buffer = view.buffer();
            let iter = match buffer.iter_at_line(line) {
                Some(it) => it,
                None => return,
            };
            let (line_y, line_h) = view.line_yrange(&iter);
            let top_margin = view.top_margin();
            let line_top = (line_y + top_margin) as f64;
            let line_bottom = line_top + line_h as f64;

            let adj = scrolled.vadjustment();
            let view_top = adj.value();
            let view_bottom = view_top + adj.page_size();
            let pad = 24.0;
            let max_val = (adj.upper() - adj.page_size()).max(adj.lower());

            let new_val = if line_top < view_top + pad {
                (line_top - pad).clamp(adj.lower(), max_val)
            } else if line_bottom > view_bottom - pad {
                (line_bottom + pad - adj.page_size()).clamp(adj.lower(), max_val)
            } else {
                return; // Already visible — don't scroll.
            };
            adj.set_value(new_val);
            bar.queue_draw();
        });
    }

    pub fn show_synopsis(
        &self,
        title: &str,
        synopsis: &str,
        root_color: Option<&str>,
        card_width: i32,
        card_height: i32,
    ) {
        self.hide_citation();
        *self.current_synopsis.borrow_mut() = synopsis.to_string();
        // Clear any stale visual-mode anchor: showing a (possibly different)
        // synopsis rebuilds the block list, so an old anchor index is invalid.
        self.synopsis_visual_anchor.set(None);
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh synopsis render closes any open ask card and returns focus to
        // the synopsis (e.g. after an amend completes, or n/p moves scenes).
        self.ask.close();
        // Match the gloss margins: anchor to the actual (full-screen) card
        // width, not the fixed column_width, so the synopsis prose sits at the
        // same ~65-char measure as the gloss instead of running nearly edge to
        // edge.
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_text(title);
        self.title.set_visible(true);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        self.title.set_margin_start(left);
        // Reset the top margin in case `show_glossing` widened it (it shares this
        // title widget). The synopsis card gives the "Act N, Scene N" header
        // extra breathing room above it.
        self.title.set_margin_top(56);
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.position_label.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);

        *self.bar_ranges.borrow_mut() = Vec::new();
        *self.line_numbers.borrow_mut() = Vec::new();
        *self.echo_lines.borrow_mut() = Vec::new();

        // Indent the body 60px past the bar so the accent bar has the same
        // breathing room to its right as the gloss overlay (whose prose tags sit
        // at `bar_left + 60`). The bar stays at `left` (see `bar_x` below); only
        // the prose shifts right.
        self.gloss_view.set_left_margin(left + 60);
        self.gloss_view.set_right_margin(left);
        // Tighten the gap between the title rule and the first synopsis line by
        // ~one line (was 32) — the title's own margin/padding-bottom already
        // supplies separation, so the prose can sit closer under the rule.
        self.gloss_view.set_top_margin(8);
        self.gloss_view.set_pixels_below_lines(6);
        let buffer = self.gloss_view.buffer();
        let (text, label_ranges) = render_synopsis_with_labels(synopsis);
        buffer.set_text(&text);
        // Remember label paragraphs (e.g. "Shakespearean parallels:") so they
        // can be bolded now and re-bolded after every apply_font (which applies
        // a regular-weight buffer-wide font tag that would otherwise win).
        *self.synopsis_label_ranges.borrow_mut() = label_ranges;
        self.apply_synopsis_label_bold();
        // Block cursor + left accent bar, exactly like the gloss overlay. Each
        // <p> paragraph (non-label) is one Explication cursor stop; j/k move the
        // bar between them (see handle_synopsis_overlay_key). Match the gloss
        // overlay's accent color (theme root_color) so the bar is the same
        // saturated accent, not the pale constructor default.
        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }
        *self.bar_x.borrow_mut() = left;
        self.rebuild_block_ranges_from(synopsis_blocks(synopsis));
        self.mark_cursor_block();
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.set_synopsis_hint();
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        self.reset_scroll_top();

        // Headless test: emit the overlay viewport rect once layout settles, so
        // tests/overlay_clipping.rs can target the synopsis card's region.
        // GlossOverlay is not Clone, so capture the scrolled window and inline.
        if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
            let sc = self.gloss_scrolled.clone();
            glib::idle_add_local_once(move || {
                if let Some(r) = sc.root().and_then(|root| sc.compute_bounds(&root)) {
                    crate::logging::log(&format!(
                        "TEST_OVERLAY_VIEWPORT_RECT {} {} {} {}",
                        r.x().round() as i32,
                        r.y().round() as i32,
                        r.width().round() as i32,
                        r.height().round() as i32
                    ));
                } else {
                    crate::logging::log(
                        "TEST_OVERLAY_VIEWPORT_RECT unavailable (root/compute_bounds returned None)",
                    );
                }
            });
        }
    }

    /// Snap the overlay's scroll position to the very top, reliably.
    ///
    /// `set_value(0.0)` inline (or on idle) is timing-dependent: `set_visible`
    /// and `apply_font` recompute the vadjustment range on a later layout pass,
    /// and on a slow real display that pass can land after the idle fires —
    /// leaving the card scrolled partway down with the first lines clipped.
    /// Instead we react to the layout itself: a handler on the adjustment's
    /// `changed` signal (emitted whenever the range is recomputed) re-snaps to
    /// `lower()` and re-sizes the clip on EVERY layout pass during the open.
    ///
    /// Two layout passes are normal for one open (`set_visible` reflow, then a
    /// later `apply_font` reflow), so the handler must survive past the first
    /// `changed` — disconnecting after one fire leaves a second pass able to
    /// displace the scroll with no handler to correct it. We instead disconnect
    /// on a one-shot timeout after the passes have settled. The handler also
    /// only re-snaps while the open is still "fresh" (a `pinning` flag): once it
    /// clears, a stray `changed` from a later resize/font-cycle must NOT yank a
    /// user who has since scrolled back to the top.
    fn reset_scroll_top(&self) {
        self.clip_guard.on_open();
    }

    /// `&self` entry point for recomputing the bottom clip after a scroll.
    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }

    /// Recompute `blocks` line spans from the current buffer + gloss text. Each
    /// block is located by scanning buffer lines for its first text line; a
    /// source block extends to its last verse line.
    fn rebuild_block_ranges(&self, gloss: &str) {
        let blocks = gloss_blocks(gloss);
        self.rebuild_block_ranges_from(blocks);
    }

    /// Map a pre-built block list to buffer-line spans (shared by the gloss path,
    /// which builds blocks with `gloss_blocks`, and the synopsis path, which uses
    /// `synopsis_blocks`). Matches each block's first `display` line against
    /// buffer lines, stores `self.blocks`, resets the cursor to block 0.
    fn rebuild_block_ranges_from(&self, blocks: Vec<GlossBlock>) {
        let buffer = self.gloss_view.buffer();
        let line_count = buffer.line_count();
        let mut ranges: Vec<BlockRange> = Vec::new();
        let mut search_from = 0i32;

        let find_line = |needle: &str, from: i32| -> Option<i32> {
            if needle.is_empty() {
                return None;
            }
            for line in from..line_count {
                if let Some(start) = buffer.iter_at_line(line) {
                    let mut end = start.clone();
                    if !end.ends_line() {
                        end.forward_to_line_end();
                    }
                    let line_text = buffer.text(&start, &end, false);
                    if line_text.as_str().trim().starts_with(needle) {
                        return Some(line);
                    }
                }
            }
            None
        };

        for b in blocks {
            let lines: Vec<&str> = b.display.lines().collect();
            let first_needle = lines.first().map(|s| s.trim()).unwrap_or("");
            let start_line = match find_line(first_needle, search_from) {
                Some(l) => l,
                None => continue,
            };
            let end_line = if b.kind == BlockKind::Source && lines.len() > 1 {
                let last_needle = lines.last().map(|s| s.trim()).unwrap_or("");
                find_line(last_needle, start_line + 1).unwrap_or(start_line)
            } else {
                start_line
            };
            ranges.push(BlockRange {
                kind: b.kind,
                index: b.index,
                start_line,
                end_line,
            });
            search_from = end_line + 1;
        }
        *self.blocks.borrow_mut() = ranges;
        // A fresh render selects the first block.
        self.cursor_block.set(0);
    }

    /// The selected cursor block as `(kind, index)`. None when the current card
    /// has no blocks (echoes/synopsis/empty gloss). The selection is the stored
    /// `cursor_block` index (set by j/k/gg/G), clamped to the block list — NOT
    /// derived from scroll position.
    pub fn current_block(&self) -> Option<(BlockKind, i32)> {
        let ranges = self.blocks.borrow();
        if ranges.is_empty() {
            return None;
        }
        let i = self.cursor_block.get().min(ranges.len() - 1);
        ranges.get(i).map(|r| (r.kind, r.index))
    }

    /// `j`/`k`: move the block cursor down/up one block.
    pub fn cursor_next_block(&self) {
        self.step_cursor(1);
    }
    pub fn cursor_prev_block(&self) {
        self.step_cursor(-1);
    }
    /// `gg`/`G`: move the block cursor to the first/last block.
    pub fn cursor_first_block(&self) {
        self.cursor_to_end(false);
    }
    pub fn cursor_last_block(&self) {
        self.cursor_to_end(true);
    }

    /// Step the cursor to the next (`+1`) or previous (`-1`) block, clamped to
    /// the ends; mark it and scroll it into view. No-op with no blocks.
    fn step_cursor(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1);
        // No movement (already at the first/last block): do nothing. Re-running
        // scroll_cursor_into_view here would re-snap the viewport and recompute
        // the bottom clip every press, which reads as a visible "jiggle" when j
        // is held at the bottom (over-tall last block) — the cursor can't advance
        // but the scroll target keeps nudging.
        if next == cur {
            return;
        }
        self.cursor_block.set(next as usize);
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    /// Jump the cursor to the first (`false`) or last (`true`) block; mark it and
    /// scroll it into view.
    fn cursor_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.mark_cursor_block();
        self.scroll_cursor_into_view();
    }

    /// Enter synopsis visual mode: anchor at the current block. No-op if there
    /// are no blocks. Returns true if mode was entered.
    pub fn enter_visual(&self) -> bool {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return false;
        }
        let cur = self.cursor_block.get().min(len - 1);
        self.synopsis_visual_anchor.set(Some(cur));
        self.refresh_selection_bar();
        true
    }

    /// Exit synopsis visual mode: clear the anchor and redraw the bar as the
    /// single cursor block.
    pub fn exit_visual(&self) {
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode collapsing the cursor to the START of the selection
    /// (the lower of anchor/cursor), then redraw the bar as that single block.
    /// Used by the gloss `y` yank so the cursor lands on the first selected
    /// block rather than wherever the moving end finished.
    pub fn exit_visual_to_start(&self) {
        if let Some(anchor) = self.synopsis_visual_anchor.get() {
            let (start, _) = visual_block_range(anchor, self.cursor_block.get());
            self.cursor_block.set(start);
        }
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Exit visual mode returning the cursor to the ANCHOR block — the block the
    /// cursor was on when visual mode was entered (Shift+V). Used by Escape/V so
    /// cancelling a selection lands the cursor back where it started, rather than
    /// at the moving (j/k) end. Unlike `exit_visual_to_start`, the anchor may be
    /// the HIGHER of anchor/cursor (when the selection was extended upward).
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.synopsis_visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.synopsis_visual_anchor.set(None);
        self.mark_cursor_block();
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped) and
    /// re-span the bar. Used by j/k while in visual mode.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// Move the cursor end of the selection to the first (`false`) or last
    /// (`true`) block and re-span the bar. Used by gg/G while in visual mode.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_selection_bar();
        self.scroll_cursor_into_view();
    }

    /// The currently-selected paragraphs' text (blank-line joined), for yank.
    pub fn visual_selection_text(&self) -> String {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => return String::new(),
        };
        let cursor = self.cursor_block.get();
        let syn = self.current_synopsis.borrow();
        selected_blocks_text(&syn, anchor, cursor)
    }

    /// Number of blocks currently selected (for the log line).
    pub fn visual_selection_len(&self) -> usize {
        match self.synopsis_visual_anchor.get() {
            Some(a) => {
                let (s, e) = visual_block_range(a, self.cursor_block.get());
                e - s + 1
            }
            None => 0,
        }
    }

    /// Set the synopsis-overlay footer hint (normal navigation).
    pub fn set_synopsis_hint(&self) {
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · Shift+Space synth · Alt+g glosses · A ask · E edit · U undo · \u{21e7}V select");
    }

    /// Set the footer hint shown while synopsis visual mode is active.
    pub fn set_synopsis_visual_hint(&self) {
        self.hint.set_text("\u{21e7}V/Esc exit · j/k extend · gg/G ends · y yank");
    }

    /// Set the gloss-overlay footer hint (normal navigation). Called by the
    /// gloss render path and when exiting gloss visual mode, so both share one
    /// string. `\u{21e7}V select` advertises gloss visual mode.
    pub fn set_gloss_hint(&self) {
        self.hint.set_text("J journal · Ctrl+j view jrnl");
    }

    /// Set the footer hint shown while gloss visual mode is active.
    pub fn set_gloss_visual_hint(&self) {
        self.hint.set_text("\u{21e7}V/Esc exit · j/k extend · gg/G ends · y yank");
    }

    /// The currently-selected blocks' text read straight from the gloss buffer
    /// (first selected block's start line through the last block's end line),
    /// for yank in gloss visual mode. Unlike `visual_selection_text` (synopsis),
    /// this does not use `current_synopsis`; it copies the full block text —
    /// source verse plus its gloss — exactly as displayed.
    pub fn visual_selection_buffer_text(&self) -> String {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => return String::new(),
        };
        let (start_idx, end_idx) = visual_block_range(anchor, self.cursor_block.get());
        let ranges = self.blocks.borrow();
        let buffer = self.gloss_view.buffer();
        // Read each block as its own contiguous span (internal verse-line
        // newlines preserved) and join blocks with a blank line, matching the
        // synopsis yank's `\n\n` paragraph separation.
        let mut blocks: Vec<String> = Vec::new();
        for r in ranges.iter().skip(start_idx).take(end_idx.saturating_sub(start_idx) + 1) {
            let start = match buffer.iter_at_line(r.start_line) {
                Some(it) => it,
                None => continue,
            };
            let mut end = match buffer.iter_at_line(r.end_line) {
                Some(it) => it,
                None => continue,
            };
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            blocks.push(buffer.text(&start, &end, false).to_string());
        }
        blocks.join("\n\n")
    }

    /// Scroll the viewport so the selected cursor block is visible. Only scrolls
    /// when the block falls outside the current viewport: brings its top into
    /// view (with a small pad) if above, or its bottom into view if below. The
    /// scroll target is decided by the pure `cursor_scroll_target` helper.
    fn scroll_cursor_into_view(&self) {
        let (start_line, end_line) = {
            let ranges = self.blocks.borrow();
            let i = self.cursor_block.get().min(ranges.len().saturating_sub(1));
            match ranges.get(i) {
                Some(r) => (r.start_line, r.end_line),
                None => return,
            }
        };
        let buffer = self.gloss_view.buffer();
        let top_margin = self.gloss_view.top_margin() as f64;
        let block_top = match buffer.iter_at_line(start_line) {
            Some(it) => self.gloss_view.line_yrange(&it).0 as f64 + top_margin,
            None => return,
        };
        let block_bottom = match buffer.iter_at_line(end_line) {
            Some(it) => {
                let (y, h) = self.gloss_view.line_yrange(&it);
                (y + h) as f64 + top_margin
            }
            None => block_top,
        };

        let adj = self.gloss_scrolled.vadjustment();
        let view_top = adj.value();
        let view_bottom = view_top + adj.page_size();
        let max_value = (adj.upper() - adj.page_size()).max(adj.lower());
        let pad = 24.0;

        let new_value = match cursor_scroll_target(&CursorScrollGeom {
            block_top,
            block_bottom,
            view_top,
            view_bottom,
            page_size: adj.page_size(),
            lower: adj.lower(),
            max_value,
            pad,
        }) {
            Some(v) => v,
            None => return, // already fully visible
        };
        // Snap the viewport top to a whole visual row so the first line is not
        // clipped under the title rule (the top edge has no clip box).
        //
        // Direction matters. When we are revealing a block's BOTTOM (it ended
        // below the fold), flooring the top to the nearest row *below* the
        // target scrolls the viewport back UP and re-hides the bottom we just
        // tried to show — the exact bug where the last explication clipped:
        // target 514 floored to row-top 450, losing 64px. For that case snap
        // UP to the nearest whole row at/above the target (clamped to the scroll
        // ceiling), which keeps the bottom in view. Otherwise (revealing a top)
        // floor as before so the revealed top isn't pushed under the title.
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

    /// Move the left accent bar to the selected cursor block and repaint. No-op
    /// when there are no blocks. Logs the landing block so j/k/gg/G navigation
    /// stays verifiable from the dev log.
    fn mark_cursor_block(&self) {
        let (kind, index) = match self.current_block() {
            Some(t) => t,
            None => return,
        };
        let span = self
            .blocks
            .borrow()
            .iter()
            .find(|r| r.kind == kind && r.index == index)
            .map(|r| (r.start_line, r.end_line));
        if let Some((start_line, end_line)) = span {
            crate::log_fmt!(
                "GLOSS-CURSOR: cursor#{} -> {:?}#{} bar lines [{}, {}]",
                self.cursor_block.get(), kind, index, start_line, end_line
            );
            *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
            self.bar_drawing.queue_draw();
        }
    }

    /// Redraw the left bar. In visual mode (anchor set) the bar spans every
    /// selected block (`anchor..=cursor`); otherwise it marks the single cursor
    /// block. Safe to call in both modes.
    fn refresh_selection_bar(&self) {
        let anchor = match self.synopsis_visual_anchor.get() {
            Some(a) => a,
            None => {
                self.mark_cursor_block();
                return;
            }
        };
        let blocks = self.blocks.borrow();
        if blocks.is_empty() {
            return;
        }
        let last = blocks.len() - 1;
        let cursor = self.cursor_block.get().min(last);
        let (s, e) = visual_block_range(anchor.min(last), cursor);
        let start_line = blocks[s].start_line;
        let end_line = blocks[e].end_line;
        *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
        self.bar_drawing.queue_draw();
    }

    /// Approximate height of one line of gloss text, derived from the view's
    /// font. Used ONLY as the per-press *step distance* for `scroll_gloss`
    /// (how far one j/k moves before snapping) — never as a snapping grid, since
    /// real wrapped rows vary in height (per-tag `pixels_above_lines`/`scale`).
    /// The font is read from the `font-size` tag when present to dodge the GTK
    /// CSS-application race that returns the previous font's metrics for one
    /// frame after a font change (see `viewport::descender_guard_px`).
    fn row_step(&self) -> f64 {
        let ctx = self.gloss_view.pango_context();
        let font_desc = self
            .gloss_view
            .buffer()
            .tag_table()
            .lookup("font-size")
            .and_then(|tag| tag.font_desc());
        let metrics = ctx.metrics(font_desc.as_ref(), None);
        let ascent = metrics.ascent() as f64 / pango::SCALE as f64;
        let descent = metrics.descent() as f64 / pango::SCALE as f64;
        let line = ascent + descent;
        (line + self.gloss_view.pixels_below_lines() as f64).max(12.0)
    }

    /// Snap a scroll value to the greatest *real* visual-row top at or below
    /// `target_y`, clamped to `[lower, upper - page_size]`, so the viewport top
    /// aligns to a whole wrapped row (no half row clipped under the title rule).
    ///
    /// Uses actual layout (`display_rows`) rather than a fixed row height,
    /// because the gloss/synopsis buffers apply per-tag `pixels_above_lines` and
    /// `scale`, making rows non-uniform. If the snapped boundary would exceed
    /// the scroll ceiling, the clamp pulls it back to `max_value`; an
    /// uncovered partial row there is hidden by the bottom clip.
    fn snap_value_to_line(&self, target_y: f64) -> f64 {
        let adj = self.gloss_scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let target = target_y.clamp(lower, max_value);
        // Greatest real row top <= target.
        let mut best = lower;
        for (row_top, _row_bottom) in crate::ui::display_rows(&self.gloss_view) {
            if row_top <= target + 0.5 {
                best = best.max(row_top);
            } else {
                break;
            }
        }
        best.clamp(lower, max_value)
    }

    /// Snap a scroll value to the *least* real visual-row top at or above
    /// `target_y`, clamped to `[lower, max_value]`. The up-direction counterpart
    /// of `snap_value_to_line`, used when revealing a block's BOTTOM: flooring
    /// (the default) would scroll the viewport back up and re-hide the bottom we
    /// are trying to show. Snapping UP keeps a whole row at the top (no half row
    /// under the title rule) while never giving back the reveal. If no row top
    /// is >= target (target sits past the last row top but within the scroll
    /// ceiling), use `max_value` so the document end is reached.
    fn snap_value_to_line_up(&self, target_y: f64) -> f64 {
        let adj = self.gloss_scrolled.vadjustment();
        let lower = adj.lower();
        let max_value = (adj.upper() - adj.page_size()).max(lower);
        let row_tops: Vec<f64> = crate::ui::display_rows(&self.gloss_view)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        snap_up_to_row(target_y, &row_tops, lower, max_value)
    }

    // ---- "Ask about this scene" card -------------------------------------

    /// Reveal the ask card below the synopsis with the canonical heading + hint.
    pub fn open_ask_card(&self) {
        self.open_ask_card_with(
            "ASK ABOUT THIS SCENE",
            "Ask a question; the synopsis will be expanded to answer it  ·  Tab switch  ·  Ctrl+Enter submit",
        );
    }

    /// Reveal the stacked input card below the open synopsis/gloss card with the
    /// given heading and footer hint. Shared by the synopsis "ask" flow and the
    /// gloss add/edit prompts.
    pub fn open_ask_card_with(&self, title: &str, hint: &str) {
        let (card_width, _) = self.last_card_size.get();
        self.ask.open(title, hint, card_width);
        self.apply_font();
        self.schedule_ask_clip_recompute();
    }

    /// Hide the ask card and return focus + highlight to the synopsis.
    pub fn close_ask_card(&self) {
        self.ask.close();
        self.schedule_ask_clip_recompute();
    }

    /// Recompute the bottom clip on the next tick after the ask card opens/closes:
    /// revealing/hiding it resizes the scrolled viewport, so the clip must be
    /// recomputed for the new height — otherwise the body's last row pokes out
    /// behind the ask card. The resize isn't synchronous, hence the deferral.
    fn schedule_ask_clip_recompute(&self) {
        let clip = self.clip_guard.clip().clone();
        let view = self.gloss_view.clone();
        let scrolled = self.gloss_scrolled.clone();
        glib::idle_add_local_once(move || {
            crate::ui::recompute_overlay_bottom_clip(&view, &clip, &scrolled);
        });
    }

    /// Read and clear the ask input's text.
    pub fn take_ask_text(&self) -> String {
        self.ask.take_text()
    }

    /// Flip focus between the synopsis and the ask card. No-op if closed.
    pub fn toggle_ask_focus(&self) {
        self.ask.toggle_focus();
    }

    pub fn ask_is_open(&self) -> bool {
        self.ask.is_open()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask.focus()
    }

    pub fn show_loading(&self) {
        self.show_loading_message("Glossing...");
    }

    pub fn show_loading_message(&self, message: &str) {
        self.synopsis_label_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        // Size the card to the full reading area so the loading state reads as a
        // proper card (the same footprint the synopsis/gloss card will occupy)
        // rather than a label-sized box. Reuse the last card geometry; fall back
        // to the construction width if no card has been shown yet this session.
        let (cw, ch) = self.last_card_size.get();
        if cw > 0 {
            self.container.set_width_request(cw);
        }
        if ch > 0 {
            self.container.set_height_request(ch);
        }
        self.title.set_text(message);
        self.title.set_visible(true);
        self.title.set_vexpand(true);
        self.title.set_valign(Align::Center);
        self.title.set_halign(Align::Center);
        self.title.set_margin_start(0);
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.gloss_scroll_overlay.set_visible(false);
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
        self.position_label.set_visible(false);
        self.ask.close();
        self.hint.set_visible(false);
        // Show the dim scrim so the loading state reads as a modal card over the
        // page, consistent with the synopsis/gloss cards (was hidden, which made
        // the message float as bare text with no backdrop).
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn scroll_gloss(&self, delta: i32) {
        let adj = self.gloss_scrolled.vadjustment();
        // Step by ~3 line-heights per press, then snap to a real visual-row
        // boundary so no partial row is left clipped at the viewport top.
        // `row_step` is only the step distance; the snap aligns to actual rows.
        let step = self.row_step();
        let raw_target = adj.value() + step * 3.0 * delta as f64;
        // ALWAYS snap the viewport top to a whole visual row — the top edge has
        // no clip box, so any fractional top reads as a half-line clipped under
        // the title rule. We do NOT shortcut to an unsnapped `max_value` to
        // "reveal the last row" the way the old code did: when the content
        // overflows by less than a full line (max_value < line height), that
        // shortcut left the top fractional (the visible top-clip bug). The
        // bottom-clip box masks whatever partial row remains at the viewport
        // bottom of the snapped last page, so the last partial line is hidden
        // cleanly rather than shown clipped — whole rows only, like the main
        // reading card.
        let target = self.snap_value_to_line(raw_target);
        adj.set_value(target);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    pub fn scroll_gloss_to_top(&self) {
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    pub fn scroll_gloss_to_bottom(&self) {
        // Snap the top to a whole row at the bottom of the document. The raw
        // `upper - page_size` would land the top on a fractional row (clipped
        // under the title rule, since the top has no clip box); snapping floors
        // it to the greatest whole-row top that still shows the most content.
        // The bottom-clip box masks any partial row left at the viewport bottom,
        // so the document end reads as whole rows, not a clipped top + clipped
        // bottom.
        let adj = self.gloss_scrolled.vadjustment();
        let bottom = (adj.upper() - adj.page_size()).max(adj.lower());
        adj.set_value(self.snap_value_to_line(bottom));
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
        self.mark_cursor_block();
    }

    /// Re-show the overlay widget after it was hidden by `hide()` without
    /// rebuilding its content (the rendered blocks + cursor persist in the
    /// widget). Used when returning from the Ctrl+/ keybinds overlay, which
    /// hides the gloss/synopsis overlay on open. The ask card stays hidden —
    /// `hide()` reset it — matching a fresh navigation state.
    pub fn show_again(&self) {
        self.container.set_visible(true);
        self.scrim.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        // Reset the ask card so it never re-shows stale when the overlay reopens.
        self.ask.close();
    }

    pub fn set_position(&self, index: usize, total: usize) {
        if total > 1 {
            self.position_label.set_text(&format!("{} / {}", index + 1, total));
            self.position_label.set_visible(true);
        } else {
            self.position_label.set_visible(false);
        }
    }

    /// Show the open passage's citation range in the footer (gloss view only),
    /// e.g. "2H6 1.4.7–14". Pass the passage's start and end citation strings.
    /// Hidden when no usable citation is given.
    pub fn set_citation(&self, start_citation: &str, end_citation: &str) {
        match format_citation_range(start_citation, end_citation) {
            Some(text) => {
                self.citation_label.set_text(&text);
                self.citation_label.set_visible(true);
            }
            None => self.citation_label.set_visible(false),
        }
    }

    /// Hide the footer citation (non-gloss views: synopsis, diff, echoes).
    ///
    /// Blank the text but keep the label *visible* so its `hexpand` still holds
    /// the footer row's stretch. If it were `set_visible(false)`, the only
    /// hexpand child would vanish and the right-aligned `hint` would collapse to
    /// the left edge — the synopsis footer would then sit bottom-left instead of
    /// far-right like the gloss/journal overlays. (Journal keeps its empty
    /// `footer_left` visible for the same reason.)
    pub fn hide_citation(&self) {
        self.citation_label.set_text("");
        self.citation_label.set_visible(true);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}

/// True when `line` of `buffer` is rendered as a speaker heading — i.e. its
/// first character carries one of the speaker tags (`gloss-speaker`,
/// `gloss-speaker-first`, `gloss-speaker-source`). Used by `color_audio_blocks`
/// to decide whether a cached Source block should also recolor the heading on
/// the line above its first verse line.
fn line_is_speaker(buffer: &gtk4::TextBuffer, line: i32) -> bool {
    let iter = match buffer.iter_at_line(line) {
        Some(it) => it,
        None => return false,
    };
    iter.tags().iter().any(|t| {
        matches!(
            t.name().as_deref(),
            Some("gloss-speaker") | Some("gloss-speaker-first") | Some("gloss-speaker-source")
        )
    })
}

#[cfg(test)]
mod apply_font_priority_tests {
    use super::*;
    use gtk4::prelude::*;

    /// After `show_glossing` (which renders a stage line then calls `apply_font`),
    /// the italic `gloss-stage` tag must outrank the buffer-wide `gloss-font` tag,
    /// or the font tag's regular (upright) style flattens the stage italic — the
    /// "stage directions not italic in the overlay" bug. Mirrors the priority
    /// dance the main reader does for its italic translation tag (src/app/font.rs)
    /// and the overlay already does for synopsis-label bold / audio-cached color.
    // #[ignore]: needs gtk4::init(), which panics if a second GTK-init test runs
    // in the same process ("init from two different threads"). Run the GTK-init
    // tests one at a time, e.g. `cargo test --bins -- --ignored stage_tag`.
    #[test]
    #[ignore]
    fn stage_tag_outranks_font_tag_after_apply_font() {
        if gtk4::init().is_err() {
            eprintln!("skip: no GTK display");
            return;
        }
        let overlay = GlossOverlay::new(1050, 80);
        // A source turn with a stage direction (build_source_header form).
        let doc = "<speaker>YORK</speaker>\n\
                   <verse>Lay hands upon these traitors and their trash.</verse>\n\
                   <stage>[To Jourdain.]</stage>\n\
                   <verse>Beldam, I think we watched you at an</verse>";
        overlay.show_glossing(doc, 1660, 1000, Some("#88aabb"));

        let table = overlay.gloss_view.buffer().tag_table();
        let stage = table
            .lookup("gloss-stage")
            .expect("gloss-stage tag should exist after rendering a <stage> line");
        let font = table
            .lookup("gloss-font")
            .expect("gloss-font tag should exist after apply_font");
        assert!(
            stage.priority() > font.priority(),
            "gloss-stage (prio {}) must outrank gloss-font (prio {}) so its italic \
             survives the buffer-wide font tag",
            stage.priority(),
            font.priority(),
        );
    }
}

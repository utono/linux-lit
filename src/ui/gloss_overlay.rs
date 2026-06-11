use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct BarRange {
    start_line: i32,
    end_line: i32,
}

struct LineNumber {
    buffer_line: i32,
    number: i64,
}

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
    position_label: Label,
    gloss_scroll_overlay: Overlay,
    gloss_scrolled: gtk4::ScrolledWindow,
    gloss_view: gtk4::TextView,
    bar_drawing: gtk4::DrawingArea,
    /// Invisible box pinned to the bottom edge of the scrolling viewport,
    /// painted with the card background. Sized by `update_bottom_clip` to cover
    /// any partially-visible last line so it doesn't read as clipped by the
    /// footer rule. Emulates the main reading card's bottom-clip technique.
    bottom_clip: gtk4::Box,
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
    /// "Ask about this scene" card, stacked below the synopsis card (inside the
    /// same `container`, after the footer). Hidden unless the reader pressed `A`
    /// while the synopsis card is open. `ask_input` is an editable TextView that
    /// receives typed characters when the ask card holds focus.
    ask_container: gtk4::Box,
    ask_input: gtk4::TextView,
    /// Heading + footer hint of the ask card. Mutable so the same stacked card
    /// can serve both the synopsis "ask" flow and the gloss add/edit prompts,
    /// each with its own label/hint text.
    ask_title: Label,
    ask_hint: Label,
    /// Which sub-card currently has focus while the ask card is open. Drives the
    /// `.card-focused` highlight and whether `j/k` scroll vs. type.
    ask_focus: Cell<AskFocus>,
}

/// Focus target while the synopsis "ask" card is open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Synopsis,
    Ask,
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

        // Invisible bottom clip: pinned to the bottom of the viewport, painted
        // with the card background so any partially-visible last line is hidden
        // rather than bisected by the footer rule. Sized by update_bottom_clip.
        let bottom_clip = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        bottom_clip.set_valign(Align::End);
        bottom_clip.set_halign(Align::Fill);
        bottom_clip.set_can_target(false);
        bottom_clip.add_css_class("gloss-bottom-clip");
        bottom_clip.set_height_request(0);
        gloss_scroll_overlay.add_overlay(&bottom_clip);
        gloss_scroll_overlay.set_measure_overlay(&bottom_clip, false);
        gloss_scroll_overlay.set_clip_overlay(&bottom_clip, true);

        // Re-size the bottom clip on EVERY scroll, not just on the open's range
        // changes. The `changed`-signal handler in `reset_scroll_top` fires only
        // while the vadjustment *range* shifts during an open; once the user
        // scrolls with j/k the clip would otherwise keep its stale open-time
        // height and stop masking the new partial last row — so a half-line at
        // the bottom edge reads as clipped by the footer rule.
        {
            let view = gloss_view.clone();
            let clip = bottom_clip.clone();
            let scrolled = gloss_scrolled.clone();
            gloss_scrolled.vadjustment().connect_value_changed(move |_| {
                Self::recompute_bottom_clip(&view, &clip, &scrolled);
            });
        }

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

        let footer_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        footer_box.set_margin_start(text_margins as i32);
        footer_box.set_margin_end(text_margins as i32);
        footer_box.set_margin_top(12);
        footer_box.set_margin_bottom(12);
        footer_box.add_css_class("gloss-hint");

        let hint = Label::new(Some("Esc close · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage"));
        hint.set_halign(Align::Center);
        hint.set_hexpand(true);
        footer_box.append(&hint);

        let position_label = Label::new(None);
        position_label.set_halign(Align::End);
        position_label.set_visible(false);
        footer_box.append(&position_label);

        container.append(&footer_box);

        // ---- "Ask about this scene" card, stacked below the synopsis ---------
        // Lives inside `container` so the two cards form one centered column and
        // the synopsis scroll viewport (which vexpands) shrinks to make room when
        // this card is revealed. Hidden until `A` opens it.
        let ask_container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        ask_container.add_css_class("ask-card");
        ask_container.set_margin_top(14);
        ask_container.set_margin_start(text_margins as i32);
        ask_container.set_margin_end(text_margins as i32);
        ask_container.set_margin_bottom(14);

        let ask_title = Label::new(Some("ASK ABOUT THIS SCENE"));
        ask_title.add_css_class("gloss-header");
        ask_title.set_halign(Align::Start);
        ask_title.set_margin_start(16);
        ask_title.set_margin_top(12);
        ask_container.append(&ask_title);

        let ask_scrolled = gtk4::ScrolledWindow::new();
        ask_scrolled.set_min_content_height(72);
        ask_scrolled.set_max_content_height(160);
        ask_scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        ask_scrolled.set_margin_start(16);
        ask_scrolled.set_margin_end(16);
        ask_scrolled.set_margin_top(6);
        ask_scrolled.set_margin_bottom(6);

        let ask_input = gtk4::TextView::new();
        ask_input.set_editable(true);
        ask_input.set_cursor_visible(true);
        ask_input.set_wrap_mode(gtk4::WrapMode::Word);
        ask_input.set_top_margin(6);
        ask_input.set_bottom_margin(6);
        ask_input.set_left_margin(6);
        ask_input.set_right_margin(6);
        ask_input.add_css_class("gloss-text");
        ask_input.add_css_class("ask-input");
        ask_scrolled.set_child(Some(&ask_input));
        ask_container.append(&ask_scrolled);

        let ask_hint = Label::new(Some(
            "Ask a question; the synopsis will be expanded to answer it  ·  Tab switch  ·  Ctrl+Enter submit  ·  Esc cancel",
        ));
        ask_hint.add_css_class("ask-hint");
        ask_hint.set_halign(Align::Center);
        ask_hint.set_margin_bottom(10);
        ask_container.append(&ask_hint);

        ask_container.set_visible(false);
        container.append(&ask_container);

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
            position_label,
            gloss_scroll_overlay,
            gloss_scrolled,
            gloss_view,
            bar_drawing,
            bottom_clip,
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
            ask_container,
            ask_input,
            ask_title,
            ask_hint,
            ask_focus: Cell::new(AskFocus::Synopsis),
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
        for view in [&self.gloss_view, &self.echo_header_view, &self.ask_input] {
            let buffer = view.buffer();
            let table = buffer.tag_table();
            if let Some(old) = table.lookup("gloss-font") {
                table.remove(&old);
            }
            let tag = gtk4::TextTag::builder().name("gloss-font").font(&font_str).build();
            table.add(&tag);
            let (start, end) = buffer.bounds();
            buffer.apply_tag(&tag, &start, &end);
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
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.container);
        self.overlay.set_measure_overlay(&self.scrim, false);
        self.overlay.set_measure_overlay(&self.container, false);
        self.overlay.set_clip_overlay(&self.scrim, true);
        self.overlay.set_clip_overlay(&self.container, true);
    }

    pub fn show(&self, original: &str, corrected: &str) {
        self.title.set_visible(true);
        self.title.set_text("Gloss");
        // Reset the top margin in case `show_glossing` widened it (shared title).
        self.title.set_margin_top(24);
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

    pub fn show_gloss(&self, _original: &str, gloss: &str, card_width: i32, card_height: i32) {
        self.show_gloss_with_color(_original, gloss, card_width, card_height, None, &[]);
    }

    pub fn show_gloss_with_color(&self, _original: &str, gloss: &str, card_width: i32, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        // No synopsis label bolding in gloss view.
        self.synopsis_label_ranges.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh gloss render closes any open add/edit ask card and clears its
        // focus highlight (e.g. after an add/edit completes or n/p navigates).
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Synopsis);
        self.ask_container.remove_css_class("card-focused");
        self.ask_container.remove_css_class("card-dimmed");
        self.title.set_visible(false);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        // Wide side margins keep gloss prose near the ~65-char readability
        // optimum. Anchor to the actual card width (the overlay is full-screen,
        // ~1660px), NOT the fixed column_width (1050) — otherwise on a wide card
        // the margin stays tiny and the text runs nearly edge to edge.
        let left = card_width / 4;
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
        self.gloss_view.set_top_margin(32);
        self.gloss_view.set_pixels_below_lines(4);
        self.hint.set_text("Esc close · Space play/pause · a play · A add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage");
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

        let bar_left = card_width / 4;
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
        self.synopsis_label_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Synopsis);
        self.ask_container.remove_css_class("card-focused");
        self.ask_container.remove_css_class("card-dimmed");

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
        let left = card_width / 4;
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

        let bar_left = card_width / 4;
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
        self.synopsis_label_ranges.borrow_mut().clear();
        self.blocks.borrow_mut().clear();
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        self.ask_container.set_visible(false);
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
        // Reuse populate_gloss_buffer_ex (it builds the speaker/verse tags and
        // returns empty bar data for a source-only doc).
        let _ = populate_gloss_buffer_ex(
            &self.echo_header_view, source_doc, self.text_margins, bar_left, &[], None, dim_color, None);
        self.echo_header_view.set_visible(true);
        self.echo_rule.set_visible(true);

        // Scrolling list: only the echoes. echo_lines/bar_ranges are now indexed
        // from the first echo (no source lines to offset past).
        let (ranges, nums, echo_lines) = populate_gloss_buffer_ex(
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
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.last_card_size.set((card_width, card_height));
        // A fresh synopsis render closes any open ask card and returns focus to
        // the synopsis (e.g. after an amend completes, or n/p moves scenes).
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Synopsis);
        self.ask_container.remove_css_class("card-focused");
        self.ask_container.remove_css_class("card-dimmed");
        // Match the gloss margins: anchor to the actual (full-screen) card
        // width, not the fixed column_width, so the synopsis prose sits at the
        // same ~65-char measure as the gloss instead of running nearly edge to
        // edge.
        let left = card_width / 4;
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

        self.gloss_view.set_left_margin(left);
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
        self.hint.set_text("Esc close · j/k block · Space play · n/p scene · ⇧Space synth · Ctrl+g glosses · A ask · U undo");
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.apply_font();
        self.reset_scroll_top();
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
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());

        let view = self.gloss_view.clone();
        let clip = self.bottom_clip.clone();
        let scrolled = self.gloss_scrolled.clone();

        // True while we should keep forcing the scroll to the top across the
        // open's layout passes; cleared once the layout has settled so we stop
        // fighting later user scrolls.
        let pinning = Rc::new(Cell::new(true));
        let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));

        let id = adj.connect_changed({
            let pinning = pinning.clone();
            let view = view.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            move |a| {
                if pinning.get() && a.value() != a.lower() {
                    a.set_value(a.lower());
                }
                Self::recompute_bottom_clip(&view, &clip, &scrolled);
            }
        });
        *handler.borrow_mut() = Some(id);

        // Stop pinning + disconnect once layout has settled (well after both the
        // set_visible and apply_font passes). This guarantees the re-snap covers
        // every pass during the open, then releases so the handler can't leak,
        // stack across reopens, or fight a later user scroll.
        let adj_for_stop = adj.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            pinning.set(false);
            if let Some(hid) = handler.borrow_mut().take() {
                adj_for_stop.disconnect(hid);
            }
        });

        // Size the clip on first open even if `changed` never fires (range
        // unchanged across show).
        glib::idle_add_local_once(move || {
            Self::recompute_bottom_clip(&view, &clip, &scrolled);
        });
    }

    /// Recompute the bottom clip from cloned widgets. Static so it can run from
    /// signal/idle closures that can't capture `&self`. Mirrors the main card's
    /// `update_bottom_clip`: find the bottom of the last visual row that fits
    /// entirely within the viewport, then size the clip box to cover from there
    /// to the viewport bottom — hiding any partial row straddling the edge.
    ///
    /// Row geometry comes from `display_rows` (real per-visual-row rects via
    /// `iter_location`), never a fixed font estimate — the gloss/synopsis
    /// buffers join paragraphs into single multi-row buffer lines and apply
    /// per-tag `pixels_above_lines`/`scale`, so rows are not uniform and
    /// `line_yrange` (logical-line granular) would be wrong here.
    fn recompute_bottom_clip(
        view: &gtk4::TextView,
        clip: &gtk4::Box,
        scrolled: &gtk4::ScrolledWindow,
    ) {
        let adj = scrolled.vadjustment();
        let viewport_h = adj.page_size();
        if viewport_h <= 0.0 {
            if clip.height_request() != 0 {
                clip.set_height_request(0);
            }
            return;
        }
        let top_y = adj.value();
        let bottom_y = top_y + viewport_h; // viewport bottom in content space
        let content_h = adj.upper();

        // Find the bottom of the last visual row that fits ENTIRELY above the
        // viewport bottom. The clip then covers from there to the viewport
        // bottom, hiding any partial row straddling the bottom edge.
        let rows = Self::display_rows(view);
        let mut last_full_bottom = top_y; // worst case: nothing fits
        let mut any_full = false;
        for (row_top, row_bottom) in &rows {
            if *row_bottom <= bottom_y + 0.5 && *row_bottom > top_y {
                last_full_bottom = *row_bottom;
                any_full = true;
            }
            if *row_top >= bottom_y {
                break;
            }
        }

        // If the document ends within the viewport, there is no partial row at
        // the bottom — only slack below the content; cover just that.
        let effective_bottom = if content_h <= bottom_y + 0.5 {
            content_h
        } else {
            last_full_bottom
        };

        // Guard against blanking: if no full row fit (a single row taller than
        // the viewport), leave the clip at 0 so that row stays visible.
        let clip_h = if !any_full && content_h > bottom_y + 0.5 {
            0
        } else {
            (bottom_y - effective_bottom).max(0.0).round() as i32
        };

        if clip.height_request() != clip_h {
            clip.set_height_request(clip_h);
        }
    }

    /// Yield `(row_top, row_bottom)` for each visual (wrapped) row from the start
    /// of the buffer, in **vadjustment / scroll coordinate space**. Steps display
    /// line by display line with `forward_display_line` and reads each row's rect
    /// via `iter_location`, so wrapped paragraphs contribute one entry per real
    /// visual row at its true height — `line_yrange` would collapse them to one
    /// paragraph-tall row.
    ///
    /// CRITICAL: `iter_location` returns **buffer** coordinates (y = 0 at the
    /// first line of text, the view's `top_margin` NOT included), but the
    /// vadjustment scrolls over `top_margin + text + bottom_margin`, so its
    /// `value`/`upper` are `top_margin` larger. Comparing the two directly (the
    /// old code did) shifted every row up by `top_margin`, so the bottom-clip
    /// under-counted the partial last row (it poked through under the footer)
    /// and `snap_value_to_line` snapped the viewport top `top_margin` px above
    /// the real row top (the first line clipped under the title after a scroll).
    /// We add `top_margin` here so callers can compare against `adj.value()`.
    fn display_rows(view: &gtk4::TextView) -> Vec<(f64, f64)> {
        let mut rows: Vec<(f64, f64)> = Vec::new();
        let top_margin = view.top_margin() as f64;
        let buffer = view.buffer();
        let mut iter = buffer.start_iter();
        let end = buffer.end_iter();
        for _ in 0..8192 {
            let rect = view.iter_location(&iter);
            if rect.height() > 0 {
                let top = rect.y() as f64 + top_margin;
                rows.push((top, top + rect.height() as f64));
            }
            if iter == end || !view.forward_display_line(&mut iter) {
                break;
            }
        }
        rows
    }

    /// `&self` entry point for recomputing the bottom clip after a scroll.
    fn update_bottom_clip(&self) {
        Self::recompute_bottom_clip(&self.gloss_view, &self.bottom_clip, &self.gloss_scrolled);
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
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
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
        for (row_top, _row_bottom) in Self::display_rows(&self.gloss_view) {
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
        let row_tops: Vec<f64> = Self::display_rows(&self.gloss_view)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        snap_up_to_row(target_y, &row_tops, lower, max_value)
    }

    // ---- "Ask about this scene" card -------------------------------------

    /// True while the stacked ask card is visible.
    pub fn ask_is_open(&self) -> bool {
        self.ask_container.is_visible()
    }

    /// Which sub-card currently holds focus while the ask card is open.
    pub fn ask_focus(&self) -> AskFocus {
        self.ask_focus.get()
    }

    /// Reveal the ask card below the synopsis. The synopsis scroll viewport
    /// vexpands inside the fixed-height card, so it yields height to the ask
    /// card automatically. Clears any prior text, focuses the input, highlights
    /// the ask card.
    pub fn open_ask_card(&self) {
        self.open_ask_card_with(
            "ASK ABOUT THIS SCENE",
            "Ask a question; the synopsis will be expanded to answer it  ·  Tab switch  ·  Ctrl+Enter submit  ·  Esc cancel",
        );
    }

    /// Reveal the stacked input card below the open synopsis/gloss card with the
    /// given heading and footer hint. Shared by the synopsis "ask" flow and the
    /// gloss add/edit prompts, so the input always appears stacked beneath the
    /// card it edits (never as a separate floating dialog).
    pub fn open_ask_card_with(&self, title: &str, hint: &str) {
        self.ask_title.set_text(title);
        self.ask_hint.set_text(hint);
        let buffer = self.ask_input.buffer();
        buffer.set_text("");
        // Align the ask card's left/right edges with the synopsis/gloss prose,
        // which `show_synopsis`/`show_gloss` inset by `card_width / 4` (not the
        // static `text_margins` the card was built with). Without this the card
        // sat far wider than the text it sits beneath.
        let (card_width, _) = self.last_card_size.get();
        if card_width > 0 {
            let margin = card_width / 4;
            self.ask_container.set_margin_start(margin);
            self.ask_container.set_margin_end(margin);
        }
        self.ask_container.set_visible(true);
        self.apply_font();
        self.set_ask_focus(AskFocus::Ask);
    }

    /// Hide the ask card and return focus + highlight to the synopsis. Does not
    /// touch the synopsis text.
    pub fn close_ask_card(&self) {
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Synopsis);
        self.ask_container.remove_css_class("card-focused");
        self.ask_container.remove_css_class("card-dimmed");
        // Return keyboard focus to the synopsis scroller.
        if self.ask_input.has_focus() {
            let _ = self.gloss_scrolled.grab_focus();
        }
    }

    /// Read and clear the ask input's text.
    pub fn take_ask_text(&self) -> String {
        let buffer = self.ask_input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        buffer.set_text("");
        text
    }

    /// Flip focus between the synopsis and the ask card. No-op if the ask card
    /// is closed.
    pub fn toggle_ask_focus(&self) {
        if !self.ask_is_open() {
            return;
        }
        let next = match self.ask_focus.get() {
            AskFocus::Synopsis => AskFocus::Ask,
            AskFocus::Ask => AskFocus::Synopsis,
        };
        self.set_ask_focus(next);
    }

    /// Apply a focus target: move the active `.card-focused` highlight and either
    /// grab the input's focus (Ask) or release it back to the synopsis (the
    /// synopsis view is non-focusable, so j/k routing is by `ask_focus`, not GTK
    /// focus — but we still drop the input's focus so typed keys aren't captured).
    fn set_ask_focus(&self, focus: AskFocus) {
        self.ask_focus.set(focus);
        // The synopsis card is full-bleed, so an accent stripe on it would run
        // the whole window edge. Instead only the (tight) ask card changes: it
        // shows the accent bar when focused and dims when focus is on the
        // synopsis. The brighter, accented card is always the active one.
        match focus {
            AskFocus::Ask => {
                self.ask_container.remove_css_class("card-dimmed");
                self.ask_container.add_css_class("card-focused");
                self.ask_input.grab_focus();
            }
            AskFocus::Synopsis => {
                self.ask_container.remove_css_class("card-focused");
                self.ask_container.add_css_class("card-dimmed");
                // Drop keyboard focus from the editable input so j/k/Tab reach
                // the global controller instead of typing into the field.
                if self.ask_input.has_focus() {
                    let _ = self.gloss_scrolled.grab_focus();
                }
            }
        }
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
        self.ask_container.set_visible(false);
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

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        // Reset the ask card so it never re-shows stale when the overlay reopens.
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Synopsis);
        self.ask_container.remove_css_class("card-focused");
        self.ask_container.remove_css_class("card-dimmed");
    }

    pub fn set_position(&self, index: usize, total: usize) {
        if total > 1 {
            self.position_label.set_text(&format!("{} / {}", index + 1, total));
            self.position_label.set_visible(true);
        } else {
            self.position_label.set_visible(false);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}

#[derive(Debug)]
enum GlossElement {
    Speaker(String),
    Verse(String),
    Gloss(String),
    Pron(String),
}

/// Render a synopsis for display, honoring paragraph markup. Synopses may be
/// stored either as plain text (one paragraph) or as one or more `<p>...</p>`
/// tags (one per paragraph, mirroring how glosses use `<gloss>` per paragraph).
/// `<p>` paragraphs are joined with a blank line so the text view shows visible
/// paragraph breaks. Plain text with no `<p>` tags is returned trimmed, so
/// legacy single-paragraph synopses keep working.
pub fn render_synopsis_paragraphs(synopsis: &str) -> String {
    render_synopsis_with_labels(synopsis).0
}

/// True when a paragraph is a short standalone heading label — e.g.
/// `Shakespearean parallels:`. Such paragraphs are stored on their own `<p>`
/// and rendered in bold by `show_synopsis`. The rule is deliberately generic:
/// a trimmed paragraph that ends in a colon, is short, and contains no
/// sentence-internal period reads as a label rather than running prose.
fn is_label_paragraph(p: &str) -> bool {
    let t = p.trim();
    t.ends_with(':') && t.chars().count() <= 60 && !t[..t.len() - 1].contains('.')
}

/// Like [`render_synopsis_paragraphs`], but also returns the character ranges
/// (start, end) — in GTK `TextBuffer` char offsets into the joined string — of
/// any standalone label paragraphs, so the caller can bold them.
pub fn render_synopsis_with_labels(synopsis: &str) -> (String, Vec<(usize, usize)>) {
    let mut paras: Vec<String> = Vec::new();
    let mut remaining = synopsis;
    while let Some(pos) = remaining.find("<p>") {
        let after = &remaining[pos..];
        if let Some((content, rest)) = try_extract(after, "p") {
            if !content.is_empty() {
                paras.push(content.to_string());
            }
            remaining = rest;
        } else {
            remaining = &remaining[pos + 3..];
        }
    }
    if paras.is_empty() {
        return (synopsis.trim().to_string(), Vec::new());
    }
    // Join with a blank line, tracking each paragraph's char offset so label
    // paragraphs can be located precisely in the assembled string.
    let mut out = String::new();
    let mut labels: Vec<(usize, usize)> = Vec::new();
    let mut char_off = 0usize;
    for (i, p) in paras.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
            char_off += 2;
        }
        let len = p.chars().count();
        if is_label_paragraph(p) {
            labels.push((char_off, char_off + len));
        }
        out.push_str(p);
        char_off += len;
    }
    (out, labels)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    Source,
    Explication,
}

/// One cursor stop in the gloss, in document order.
pub struct GlossBlock {
    pub kind: BlockKind,
    /// 0-based index WITHIN its kind (source blocks numbered separately from
    /// explication paragraphs).
    pub index: i32,
    /// RAW text, including any inline `/IPA/` — this is what TTS synthesizes.
    /// For Source: the joined verse-line text (speaker labels excluded).
    /// For Explication: the paragraph prose.
    pub text: String,
    /// DISPLAY text: `text` with `/IPA/` stripped (`strip_ipa`). Used for the
    /// reader's buffer and the accent-bar block matcher.
    pub display: String,
}

/// Parse a `<p>`-tagged synopsis into cursor-stop blocks, one per paragraph,
/// each a `BlockKind::Explication` (synopses are prose, never verse). Label
/// paragraphs (`is_label_paragraph`, e.g. "Shakespearean parallels:") are shown
/// in the buffer but are NOT cursor stops, so they are skipped here — exactly
/// the paragraphs `render_synopsis_with_labels` marks for bolding. Synopsis text
/// carries no inline `/IPA/`, so `text == display`. Legacy untagged prose (no
/// `<p>`) is returned as a single block. Indices count the emitted (non-label)
/// blocks from 0, matching the cache `paragraph_index`.
pub fn synopsis_blocks(synopsis: &str) -> Vec<GlossBlock> {
    let mut paras: Vec<String> = Vec::new();
    let mut remaining = synopsis;
    while let Some(pos) = remaining.find("<p>") {
        let after = &remaining[pos..];
        if let Some((content, rest)) = try_extract(after, "p") {
            if !content.is_empty() {
                paras.push(content.to_string());
            }
            remaining = rest;
        } else {
            remaining = &remaining[pos + 3..];
        }
    }
    if paras.is_empty() {
        let t = synopsis.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![GlossBlock {
            kind: BlockKind::Explication,
            index: 0,
            text: t.to_string(),
            display: t.to_string(),
        }];
    }
    let mut blocks: Vec<GlossBlock> = Vec::new();
    let mut index = 0i32;
    for p in &paras {
        if is_label_paragraph(p) {
            continue;
        }
        blocks.push(GlossBlock {
            kind: BlockKind::Explication,
            index,
            text: p.clone(),
            display: p.clone(),
        });
        index += 1;
    }
    blocks
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

/// Parse a gloss into ordered cursor-stop blocks: each contiguous
/// `<speaker>`/`<verse>` run is one Source block; each non-echo `<gloss>` is one
/// Explication block. Echo `<gloss>` brackets are excluded. Source and
/// explication indices increment independently.
pub fn gloss_blocks(gloss: &str) -> Vec<GlossBlock> {
    let mut blocks = Vec::new();
    let mut source_idx = 0i32;
    let mut expl_idx = 0i32;
    let mut pending_verses: Vec<String> = Vec::new();

    let flush_source =
        |blocks: &mut Vec<GlossBlock>, source_idx: &mut i32, pending: &mut Vec<String>| {
            if !pending.is_empty() {
                let text = pending.join("\n");
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Source,
                    index: *source_idx,
                    text,
                    display,
                });
                *source_idx += 1;
                pending.clear();
            }
        };

    for el in parse_gloss_tags(gloss) {
        match el {
            GlossElement::Speaker(_) => { /* drop speaker labels from source text */ }
            GlossElement::Verse(text) => pending_verses.push(text.trim().to_string()),
            GlossElement::Gloss(text) => {
                if split_echo(&text).is_some() {
                    continue; // echo bracket: not a cursor stop
                }
                // A real explication paragraph ends the current source run.
                flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
                let text = text.trim().to_string();
                let display = strip_ipa(&text);
                blocks.push(GlossBlock {
                    kind: BlockKind::Explication,
                    index: expl_idx,
                    text,
                    display,
                });
                expl_idx += 1;
            }
            GlossElement::Pron(_) => { /* pronunciation note: not a cursor stop, not TTS */ }
        }
    }
    // Trailing source run (gloss that ends on verse).
    flush_source(&mut blocks, &mut source_idx, &mut pending_verses);
    blocks
}


fn parse_gloss_tags(gloss: &str) -> Vec<GlossElement> {
    let mut elements = Vec::new();
    let mut remaining = gloss;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find('<') {
            let after_open = &remaining[pos..];
            if let Some(el) = try_extract(after_open, "speaker") {
                elements.push(GlossElement::Speaker(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "verse") {
                elements.push(GlossElement::Verse(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "gloss") {
                elements.push(GlossElement::Gloss(el.0.to_string()));
                remaining = el.1;
            } else if let Some(el) = try_extract(after_open, "pron") {
                elements.push(GlossElement::Pron(el.0.to_string()));
                remaining = el.1;
            } else {
                remaining = &remaining[pos + 1..];
            }
        } else {
            break;
        }
    }
    carry_forward_block_speakers(elements)
}

/// Repair speaker-less verse blocks. A verse block normally opens with a
/// `<speaker>` (which supplies BOTH the label and the 36px breathing-room above
/// the block — `verse_tag` itself has no top spacing). When the gloss model
/// omits the speaker for a continued speech (an observed defect in stored data,
/// e.g. gloss 21730's middle block), the block renders with neither label nor
/// gap, jammed against the preceding `<gloss>` prose.
///
/// Here we detect a `Verse` that begins a new block — the previous element is a
/// `Gloss` — with no `Speaker` of its own, and splice in a synthetic `Speaker`
/// carrying the last-seen speaker name. The synthetic element flows through the
/// normal Speaker render arm, so the block regains both its label and its gap
/// with no special-casing downstream (block ranges, cursor, bars all follow).
fn carry_forward_block_speakers(elements: Vec<GlossElement>) -> Vec<GlossElement> {
    let mut out: Vec<GlossElement> = Vec::with_capacity(elements.len());
    let mut last_speaker: Option<String> = None;
    let mut prev_was_gloss = false;
    for el in elements {
        match &el {
            GlossElement::Speaker(name) => {
                last_speaker = Some(name.clone());
                prev_was_gloss = false;
            }
            GlossElement::Verse(_) => {
                // A verse opening a new block (right after prose) with no speaker
                // of its own: re-insert the carried speaker so the block keeps
                // its label and top spacing.
                if prev_was_gloss {
                    if let Some(name) = &last_speaker {
                        out.push(GlossElement::Speaker(name.clone()));
                    }
                }
                prev_was_gloss = false;
            }
            GlossElement::Gloss(_) => prev_was_gloss = true,
            GlossElement::Pron(_) => {}
        }
        out.push(el);
    }
    out
}

fn try_extract<'a>(s: &'a str, tag: &str) -> Option<(&'a str, &'a str)> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if !s.starts_with(&open) {
        return None;
    }
    let content_start = open.len();
    if let Some(end_pos) = s[content_start..].find(&close) {
        let content = s[content_start..content_start + end_pos].trim();
        let after = &s[content_start + end_pos + close.len()..];
        Some((content, after))
    } else {
        None
    }
}

fn populate_gloss_buffer(view: &gtk4::TextView, gloss: &str, _text_margins: i32, bar_left: i32, source_line_numbers: &[(String, i64)], gloss_dim: Option<&str>, speaker_accent: Option<&str>) -> (Vec<BarRange>, Vec<LineNumber>) {
    let (ranges, nums, _) = populate_gloss_buffer_ex(view, gloss, _text_margins, bar_left, source_line_numbers, None, gloss_dim, speaker_accent);
    (ranges, nums)
}

/// Extended populate that supports highlighting a selected echo (the Nth
/// `<gloss>` echo element). Returns the buffer line of each echo's quote.
fn populate_gloss_buffer_ex(view: &gtk4::TextView, gloss: &str, _text_margins: i32, bar_left: i32, source_line_numbers: &[(String, i64)], selected_echo: Option<usize>, dim_color: Option<&str>, speaker_accent: Option<&str>) -> (Vec<BarRange>, Vec<LineNumber>, Vec<i32>) {
    let buffer = view.buffer();
    buffer.set_text("");

    let tag_table = buffer.tag_table();
    for name in &["gloss-speaker", "gloss-speaker-first", "gloss-speaker-source", "gloss-verse", "gloss-para", "gloss-bracket", "gloss-quote", "gloss-quote-cont", "gloss-citation", "gloss-pron"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let quote_speaker = bar_left + 60;
    let quote_verse = quote_speaker + 60;

    // Speaker headings: small-caps, tinted with the accent (root) color so they
    // read as structural labels rather than body text. Falls back to inherited
    // fg when no accent is supplied.
    let apply_accent = |b: gtk4::builders::TextTagBuilder| -> gtk4::builders::TextTagBuilder {
        match speaker_accent {
            Some(c) => b.foreground(c),
            None => b,
        }
    };

    let speaker_tag = apply_accent(gtk4::TextTag::builder()
        .name("gloss-speaker")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .left_margin(quote_speaker)
        .pixels_above_lines(36))
        .build();

    let verse_tag = gtk4::TextTag::builder()
        .name("gloss-verse")
        .left_margin(quote_verse)
        .build();

    // Prose gloss recedes behind the verse it explains: dimmer color, slightly
    // smaller, looser line spacing for the dense commentary. The verse stays the
    // full-ink "hero".
    let para_builder = gtk4::TextTag::builder()
        .name("gloss-para")
        .left_margin(quote_speaker)
        .pixels_above_lines(24)
        .pixels_below_lines(6)
        .scale(0.92);
    let para_tag = match dim_color {
        Some(c) => para_builder.foreground(c).build(),
        None => para_builder.build(),
    };

    let speaker_first_tag = apply_accent(gtk4::TextTag::builder()
        .name("gloss-speaker-first")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .left_margin(quote_speaker))
        .build();

    // Speaker label inside the quoted source turn (before the echo list). The
    // turn may span several speakers; keep them tightly spaced to match the
    // reader's 8px speaker rhythm rather than the 36px echo-section gap.
    let speaker_source_tag = apply_accent(gtk4::TextTag::builder()
        .name("gloss-speaker-source")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .left_margin(quote_speaker)
        .pixels_above_lines(8))
        .build();

    let bracket_tag = gtk4::TextTag::builder()
        .name("gloss-bracket")
        .style(pango::Style::Italic)
        .scale(0.9)
        .build();

    // Echo quote line: same indent as the paragraph, italic.
    let quote_tag = gtk4::TextTag::builder()
        .name("gloss-quote")
        .left_margin(quote_speaker)
        .pixels_above_lines(24)
        .style(pango::Style::Italic)
        .build();

    // Continuation line of a multi-line verse echo: no top spacing.
    let quote_cont_tag = gtk4::TextTag::builder()
        .name("gloss-quote-cont")
        .left_margin(quote_speaker)
        .style(pango::Style::Italic)
        .build();

    // Citation line: indented further, smaller and dimmer. Use the theme's
    // dim foreground when provided so the source citations recede behind the
    // echo quotes.
    let citation_builder = gtk4::TextTag::builder()
        .name("gloss-citation")
        .left_margin(quote_verse)
        .scale(0.85);
    let citation_tag = match dim_color {
        Some(c) => citation_builder.foreground(c).build(),
        None => citation_builder.build(),
    };

    // Pronunciation teaching note beneath its verse block: italic and slightly
    // smaller (like the bracket tag), dimmed with the theme's dim foreground
    // (like the citation/para tags) so it reads as a recessed teaching aside.
    let pron_builder = gtk4::TextTag::builder()
        .name("gloss-pron")
        .left_margin(quote_verse)
        .style(pango::Style::Italic)
        .scale(0.92);
    let pron_tag = match dim_color {
        Some(c) => pron_builder.foreground(c).build(),
        None => pron_builder.build(),
    };

    tag_table.add(&speaker_tag);
    tag_table.add(&speaker_first_tag);
    tag_table.add(&speaker_source_tag);
    tag_table.add(&verse_tag);
    tag_table.add(&para_tag);
    tag_table.add(&bracket_tag);
    tag_table.add(&quote_tag);
    tag_table.add(&quote_cont_tag);
    tag_table.add(&citation_tag);
    tag_table.add(&pron_tag);

    let elements = parse_gloss_tags(gloss);
    let mut first = true;
    let mut only_speakers_so_far = true;
    // Whether we have reached the echo list (`<gloss>` elements). Speaker
    // labels before this belong to the quoted source turn and stay tight.
    let mut in_echoes = false;
    let mut bar_ranges: Vec<BarRange> = Vec::new();
    let mut line_nums: Vec<LineNumber> = Vec::new();
    let mut echo_lines: Vec<i32> = Vec::new();
    let mut echo_idx: usize = 0;

    // Build lookup: trimmed verse text → line_in_div
    let line_lookup: std::collections::HashMap<&str, i64> = source_line_numbers
        .iter()
        .map(|(text, num)| (text.trim(), *num))
        .collect();

    for el in &elements {
        if !first {
            let mut end = buffer.end_iter();
            buffer.insert(&mut end, "\n");
        }
        first = false;

        let line = buffer.end_iter().line();
        let offset = buffer.end_iter().offset();
        match el {
            GlossElement::Speaker(name) => {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, name);
                let start = buffer.iter_at_offset(offset);
                let tag = if only_speakers_so_far {
                    &speaker_first_tag
                } else if in_echoes {
                    &speaker_tag
                } else {
                    // Subsequent speaker within the quoted source turn: tight.
                    &speaker_source_tag
                };
                buffer.apply_tag(tag, &start, &buffer.end_iter());
            }
            GlossElement::Verse(text) => {
                only_speakers_so_far = false;
                let shown = strip_ipa(text);
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, &shown);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
                apply_bracket_styling(&buffer, offset, &bracket_tag);

                // line-number gutter: match on bracket+IPA-stripped, trimmed text
                let stripped = strip_brackets(&shown);
                if let Some(&num) = line_lookup.get(stripped.trim()) {
                    line_nums.push(LineNumber { buffer_line: line, number: num });
                }
            }
            GlossElement::Gloss(text) => {
                only_speakers_so_far = false;
                in_echoes = true;

                if let Some((quote, citation)) = split_echo(text) {
                    let quote = strip_ipa(&quote);
                    let citation = strip_ipa(&citation);
                    // Echo: quote on one line, citation indented below it.
                    let quote_line = buffer.end_iter().line();
                    echo_lines.push(quote_line);
                    let is_selected = selected_echo == Some(echo_idx);
                    echo_idx += 1;

                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &quote);
                    let qstart = buffer.iter_at_offset(offset);
                    let quote_end_offset = buffer.end_iter().offset();
                    let quote_end_iter = buffer.iter_at_offset(quote_end_offset);

                    // Apply quote_tag (with top spacing) to the first visual
                    // line, quote_cont_tag (no spacing) to continuation lines.
                    let first_line_end = {
                        let mut it = qstart.clone();
                        if !it.ends_line() {
                            it.forward_to_line_end();
                        }
                        if it.offset() > quote_end_offset { quote_end_iter.clone() } else { it }
                    };
                    buffer.apply_tag(&quote_tag, &qstart, &first_line_end);
                    if first_line_end.offset() < quote_end_offset {
                        buffer.apply_tag(&quote_cont_tag, &first_line_end, &quote_end_iter);
                    }

                    // The left accent bar (bar_ranges, below) marks the
                    // selected echo; no background highlight needed.

                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, "\n");
                    let cit_offset = buffer.end_iter().offset();
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &citation);
                    let cstart = buffer.iter_at_offset(cit_offset);
                    buffer.apply_tag(&citation_tag, &cstart, &buffer.end_iter());

                    // Accent bar beside the selected echo: span the quote's
                    // first line through the citation line.
                    if is_selected {
                        bar_ranges.push(BarRange {
                            start_line: quote_line,
                            end_line: buffer.end_iter().line(),
                        });
                    }
                } else {
                    let shown = strip_ipa(text);
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &shown);
                    let start = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
                }
            }
            GlossElement::Pron(_) => {
                only_speakers_so_far = false;
                // <pron> notes are no longer shown to the reader: IPA is not
                // helpful pedagogy and is TTS-only. Already-stored notes are
                // silently dropped from display. (The tag stays defined; just
                // unused now.)
            }
        }
    }

    (bar_ranges, line_nums, echo_lines)
}

fn apply_bracket_styling(buffer: &gtk4::TextBuffer, base_offset: i32, bracket_tag: &gtk4::TextTag) {
    let text = buffer.text(&buffer.iter_at_offset(base_offset), &buffer.end_iter(), false);
    let text_str = text.as_str();
    let mut pos = 0;
    while pos < text_str.len() {
        if let Some(open) = text_str[pos..].find('[') {
            let abs_open = pos + open;
            if let Some(close) = text_str[abs_open..].find(']') {
                let abs_close = abs_open + close + 1;
                let start = buffer.iter_at_offset(base_offset + abs_open as i32);
                let end = buffer.iter_at_offset(base_offset + abs_close as i32);
                buffer.apply_tag(bracket_tag, &start, &end);
                pos = abs_close;
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

fn strip_brackets(text: &str) -> String {
    let mut result = String::new();
    let mut in_bracket = false;
    for ch in text.chars() {
        match ch {
            '[' => in_bracket = true,
            ']' => { in_bracket = false; }
            _ if !in_bracket => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Remove inline `/IPA/` pronunciation spans for DISPLAY. Mirrors
/// `strip_brackets`. An IPA span is `/…/` whose contents contain at least one
/// non-ASCII-letter / IPA-class character (length marks, stress marks, schwa,
/// etc.), so a bare literal slash between plain words ("and/or") is NOT treated
/// as a span and survives. The raw, IPA-bearing text is what TTS gets; this is
/// the reader-facing form. See the gloss-IPA spec, §4.
///
/// The prompt appends IPA after a word (`Dread /drɛːd/ sovereign`), so naively
/// dropping the span leaves the two flanking spaces collapsed into a doubled
/// gap (`Dread  sovereign`), and IPA before punctuation leaves a space before it
/// (`good /gʊd/,` → `good ,`). After removing spans we therefore normalize
/// whitespace: collapse internal space runs to one, drop any space immediately
/// before `,;:.!?`, and trim the ends. This is display-only (every caller is a
/// render path); the stored gloss text is untouched.
/// Decide whether a `/…/` run is an inline IPA span (vs. a literal slash like
/// `and/or`). Two signals, either of which marks it IPA:
///
/// 1. **inner has a non-ASCII-letter char** — length `ː`, stress `ˈ`, schwa `ə`,
///    etc. Catches the overwhelming majority of OP IPA.
/// 2. **the opening `/` sits on a word boundary** (preceded by whitespace, start
///    of text, or punctuation) — catches the all-ASCII IPA spans the prompt
///    still emits, e.g. `have /hav/`, where the inner is `hav` (all ASCII
///    letters) and signal 1 alone would misread it as a literal slash. A literal
///    `and/or` fails this test because its slash is glued to a letter on the left.
///
/// Without signal 2, an all-ASCII span like `/hav/` was left unstripped, and its
/// two slashes then mis-paired with the NEXT span's slash, swallowing the text
/// between them (the `have /hav/ been. 'Tis a cruelty /ˈkruːəltɪ/` →
/// `have /havˈkruːəltɪ/` corruption seen in the reader).
fn is_ipa_span(inner: &[char], opener_on_boundary: bool) -> bool {
    if inner.is_empty() {
        return false;
    }
    inner.iter().any(|&c| !c.is_ascii_alphabetic()) || opener_on_boundary
}

/// True if the char before an opening `/` (or its absence at index 0) marks a
/// word boundary — so a free-standing `/word/` token is distinguished from a
/// letter-glued literal like `and/or`.
fn opener_on_boundary(chars: &[char], slash_idx: usize) -> bool {
    match slash_idx.checked_sub(1).map(|p| chars[p]) {
        None => true, // start of text
        Some(c) => c.is_whitespace() || matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '(' | '['),
    }
}

fn strip_ipa(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + close_rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    i = close + 1; // skip the whole /…/ span
                    continue;
                }
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    normalize_ipa_whitespace(&stripped)
}

/// Collapse the spacing artifacts left behind when `strip_ipa` removes an inline
/// IPA span: runs of spaces become one, a space directly before `,;:.!?` is
/// dropped, and leading/trailing spaces are trimmed. Only ASCII space `' '` is
/// collapsed — newlines and other whitespace are preserved verbatim so verse
/// line structure (the gloss text is single-line per block here, but be safe)
/// and the seek-matcher key stay intact.
fn normalize_ipa_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c == ' ' {
            // Defer emitting the space: collapse runs and allow it to be
            // dropped if the next char is punctuation.
            prev_space = true;
            continue;
        }
        if prev_space {
            // Emit a single space unless it would sit before close punctuation
            // or a newline (so a deferred space never trails a line).
            if !matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '\n') {
                out.push(' ');
            }
            prev_space = false;
        }
        out.push(c);
    }
    // A trailing run of spaces (prev_space == true) is intentionally dropped.
    out.trim().to_string()
}

/// Build the TTS-facing form of a verse block: REPLACE each appended `word
/// /IPA/` pair with just `/IPA/`, so ElevenLabs `eleven_v3` voices the word once
/// (via the IPA) instead of saying the plain word AND the IPA (the doubling
/// bug). The prompt emits `take /tɛːk/` — word then IPA — which the reader path
/// strips back to `take` (see `strip_ipa`); for audio we do the opposite and
/// drop the preceding word, leaving `/tɛːk/`, the docs' single-pronunciation
/// form. Words with NO following IPA span are left untouched (sparse tagging:
/// only operative words carry OP). Detection of an IPA span matches `strip_ipa`
/// (the shared [`is_ipa_span`] heuristic: non-ASCII inner OR a boundary-anchored
/// `/word/`), so an all-ASCII span like `/hav/` is handled while a letter-glued
/// literal `and/or` is not. This is applied ONLY on the TTS path; the stored
/// gloss text and the reader display are unchanged.
pub(crate) fn ipa_for_tts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + close_rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    // Drop the word the IPA annotates: remove a single run of
                    // spaces then the immediately-preceding word from `out`, so
                    // `take /tɛːk/` becomes `/tɛːk/`. Stop at the word boundary
                    // (space) or punctuation so we never eat earlier words or
                    // the punctuation between them.
                    while matches!(out.last(), Some(' ')) {
                        out.pop();
                    }
                    while let Some(&c) = out.last() {
                        // Stop at a word boundary: space, punctuation, newline,
                        // or a prior IPA span's closing `/` (so two adjacent
                        // spans never eat each other — defensive; the prompt
                        // always separates spans with a plain word).
                        if c == ' '
                            || c == '/'
                            || matches!(c, ',' | ';' | ':' | '.' | '!' | '?' | '\n')
                        {
                            break;
                        }
                        out.pop();
                    }
                    // Re-insert a separating space if the IPA now abuts a prior
                    // word/punctuation (so `good, take /tɛːk/` -> `good, /tɛːk/`,
                    // not `good,/tɛːk/`).
                    if matches!(out.last(), Some(c) if *c != ' ' && *c != '\n') {
                        out.push(' ');
                    }
                    // Copy the IPA span verbatim.
                    out.extend(&chars[i..=close]);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    let s: String = out.into_iter().collect();
    // Reuse the same whitespace normalization the display path uses, so any
    // doubled gaps / pre-punctuation spaces left by the word removal are cleaned.
    normalize_ipa_whitespace(&s)
}

/// True if `s` contains an inline IPA span. Uses the shared [`is_ipa_span`]
/// heuristic (non-ASCII inner OR a boundary-anchored `/word/`), so it agrees
/// with `strip_ipa`/`ipa_for_tts` and recognizes all-ASCII spans like `/hav/`
/// while still rejecting a letter-glued literal `and/or`. Used to decide whether
/// a fix-IPA input is a literal `/IPA/` or a plain hint.
pub(crate) fn contains_ipa_span(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + rel;
                let inner = &chars[i + 1..close];
                if is_ipa_span(inner, opener_on_boundary(&chars, i)) {
                    return true;
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    false
}

/// Replace the `/IPA/` that immediately follows each whole-word, case-insensitive
/// occurrence of `word` in `text` with `new_ipa` (which includes its slashes,
/// e.g. `"/ˈdeɪli/"`). Returns the rewritten text, or `None` if no
/// `word /IPA/` pair was found (nothing changed). A match requires the word as a
/// whole token (not a substring) directly followed (after one run of spaces) by
/// an IPA span. Used by the gloss-overlay `i` (fix-IPA) flow on a source block's
/// text.
pub(crate) fn replace_word_ipa(text: &str, word: &str, new_ipa: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let wlc: Vec<char> = word.to_ascii_lowercase().chars().collect();
    if wlc.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut replaced = false;
    while i < chars.len() {
        let at_word_boundary = i == 0 || !chars[i - 1].is_alphanumeric();
        let word_matches = at_word_boundary
            && i + wlc.len() <= chars.len()
            && chars[i..i + wlc.len()]
                .iter()
                .map(|c| c.to_ascii_lowercase())
                .eq(wlc.iter().copied())
            && chars
                .get(i + wlc.len())
                .map_or(true, |c| !c.is_alphanumeric());
        if word_matches {
            // word, then a run of spaces, then an IPA span -> replace the span.
            let mut k = i + wlc.len();
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && chars[k] == '/' {
                if let Some(rel) = chars[k + 1..].iter().position(|&c| c == '/') {
                    let close = k + 1 + rel;
                    let inner = &chars[k + 1..close];
                    let is_ipa =
                        !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic());
                    if is_ipa {
                        out.extend(&chars[i..k]); // word + original spacing verbatim
                        out.push_str(new_ipa);
                        i = close + 1;
                        replaced = true;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if replaced {
        Some(out)
    } else {
        None
    }
}

/// Rewrite the `/IPA/` after `word` (whole-word, all occurrences) within ONLY
/// the source block at `source_index`, operating on the TAGGED `gloss_text`
/// (each verse line is wrapped in `<verse>…</verse>`). Returns the full updated
/// gloss_text, or None if that block has no `word /IPA/` pair. Other blocks are
/// untouched even if they contain the same word.
///
/// Scoped by POSITION, not text: each `<verse>` is identified by its
/// document-order ordinal, and only verses belonging to the target source run
/// (per `gloss_blocks`' exact flush rule) are rewritten. This distinguishes
/// byte-identical verse lines that appear in different source blocks (e.g. a
/// repeated refrain) — a text-membership match would wrongly rewrite both.
pub(crate) fn replace_word_ipa_in_source_block(
    gloss_text: &str,
    source_index: i32,
    word: &str,
    new_ipa: &str,
) -> Option<String> {
    // Phase 1: which verse ORDINALS (0-based, document order) belong to the
    // target source block? Mirror gloss_blocks' flush rule exactly: a non-echo
    // <gloss> flushes the pending source run, and the source index advances
    // ONLY when that pending run is non-empty (matching flush_source).
    let mut target_ordinals: std::collections::HashSet<usize> = std::collections::HashSet::new();
    {
        let mut cur_source = 0i32;
        let mut verse_ord = 0usize;
        let mut pending_ords: Vec<usize> = Vec::new();
        for el in parse_gloss_tags(gloss_text) {
            match el {
                GlossElement::Verse(_) => {
                    pending_ords.push(verse_ord);
                    verse_ord += 1;
                }
                GlossElement::Gloss(text) => {
                    if split_echo(&text).is_some() {
                        continue; // echo bracket: does not flush
                    }
                    // non-echo gloss flushes the current source run (if non-empty)
                    if !pending_ords.is_empty() {
                        if cur_source == source_index {
                            target_ordinals.extend(pending_ords.iter().copied());
                        }
                        cur_source += 1;
                        pending_ords.clear();
                    }
                }
                GlossElement::Speaker(_) | GlossElement::Pron(_) => {}
            }
        }
        // trailing run (gloss that ends on verse)
        if !pending_ords.is_empty() && cur_source == source_index {
            target_ordinals.extend(pending_ords.iter().copied());
        }
    }
    if target_ordinals.is_empty() {
        return None; // no such source block / no verses
    }

    // Phase 2: walk raw <verse>…</verse> spans by ordinal (same document order
    // as Phase 1, since parse_gloss_tags emits one Verse per tag in order);
    // rewrite only target ones, copy everything else verbatim.
    let mut out = String::with_capacity(gloss_text.len());
    let mut rest = gloss_text;
    let mut ord = 0usize;
    let mut any = false;
    while let Some(open) = rest.find("<verse>") {
        let after_open = open + "<verse>".len();
        out.push_str(&rest[..after_open]);
        let tail = &rest[after_open..];
        if let Some(close_rel) = tail.find("</verse>") {
            let inner = &tail[..close_rel];
            if target_ordinals.contains(&ord) {
                if let Some(fixed) = replace_word_ipa(inner, word, new_ipa) {
                    out.push_str(&fixed);
                    any = true;
                } else {
                    out.push_str(inner);
                }
            } else {
                out.push_str(inner);
            }
            out.push_str("</verse>");
            rest = &tail[close_rel + "</verse>".len()..];
            ord += 1;
        } else {
            // malformed: no closing tag — copy the remainder and stop
            out.push_str(tail);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    if any {
        Some(out)
    } else {
        None
    }
}

/// Split an echo bracket `["quote" — Source]` into (quote, citation).
/// Returns None if the text is not in echo-bracket form. Any trailing
/// suffix outside the brackets (e.g. "(unverified)") is kept on the
/// citation line.
fn split_echo(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    let open = trimmed.find('[')?;
    let close = trimmed.rfind(']')?;
    if close <= open {
        return None;
    }
    let inner = &trimmed[open + 1..close];
    let suffix = trimmed[close + 1..].trim();

    // Split the bracket interior at the last em-dash separator.
    let sep = inner.rfind(" — ").or_else(|| inner.rfind(" - "))?;
    let quote = inner[..sep].trim().to_string();
    let mut citation = inner[sep..].trim().to_string();
    if !suffix.is_empty() {
        citation.push(' ');
        citation.push_str(suffix);
    }
    if quote.is_empty() || citation.is_empty() {
        return None;
    }
    Some((quote, citation))
}

/// Inputs to `cursor_scroll_target`: the cursor block's vertical span and the
/// current viewport/scroll geometry, all in vadjustment coordinate space.
struct CursorScrollGeom {
    block_top: f64,
    block_bottom: f64,
    view_top: f64,
    view_bottom: f64,
    page_size: f64,
    lower: f64,
    max_value: f64,
    pad: f64,
}

/// Decide the scroll value (viewport top) that brings the cursor block into
/// view, or `None` if it is already fully visible. Pure arithmetic so it can be
/// unit-tested without GTK.
///
/// Three cases:
/// - block starts above the viewport → reveal its top (clamped).
/// - block ends below the viewport → reveal its bottom, BUT keep the block's
///   top in view *only when the block actually fits* in the viewport. A block
///   TALLER than the viewport cannot show both edges; for it we reveal the
///   bottom unconditionally (the final explication is often taller than the
///   card, and capping at its top stranded the last line below the fold — the
///   bottom-clip box only masks a sub-row sliver, not a whole clipped line).
/// - otherwise already visible → `None`.
fn cursor_scroll_target(g: &CursorScrollGeom) -> Option<f64> {
    let CursorScrollGeom {
        block_top,
        block_bottom,
        view_top,
        view_bottom,
        page_size,
        lower,
        max_value,
        pad,
    } = *g;
    // Does the block (plus its top pad) fit inside one viewport height? An
    // over-tall block (e.g. the final explication, often taller than the card)
    // cannot show both edges, so it gets special handling below.
    let fits = (block_bottom - block_top) + pad <= page_size;
    let bottom_hidden = block_bottom > view_bottom - pad;
    let top_hidden = block_top < view_top + pad;

    if !fits && bottom_hidden {
        // Over-tall block whose bottom is below the fold: reveal the bottom even
        // if that scrolls the block's top off the top edge. This MUST take
        // priority over the "reveal top" branch — otherwise, once the cursor is
        // on the last (over-tall) block and the top is already in view, the
        // top-reveal branch wins forever and the final rows stay clipped below
        // the fold (the bottom-clip box only masks a sub-row sliver, not a whole
        // line). by_bottom brings `block_bottom + pad` to the viewport bottom.
        Some((block_bottom + pad - page_size).clamp(lower, max_value))
    } else if top_hidden {
        // Block starts above the viewport: bring its top into view.
        Some((block_top - pad).clamp(lower, max_value))
    } else if bottom_hidden {
        // Fitting block ending below the viewport: bring its bottom into view,
        // but never scroll its own top above the viewport top.
        let by_bottom = (block_bottom + pad - page_size).clamp(lower, max_value);
        Some(by_bottom.min((block_top - pad).max(lower)))
    } else {
        None // already fully visible
    }
}

/// Snap `target_y` to the least row top at/above it (clamped to
/// `[lower, max_value]`). If no row top is >= target, use `max_value`. Pure so
/// the snap-UP direction (used for bottom-reveal) can be unit-tested. `row_tops`
/// must be ascending.
fn snap_up_to_row(target_y: f64, row_tops: &[f64], lower: f64, max_value: f64) -> f64 {
    let target = target_y.clamp(lower, max_value);
    row_tops
        .iter()
        .copied()
        .find(|t| *t + 0.5 >= target)
        .unwrap_or(max_value)
        .clamp(lower, max_value)
}

fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
    Some((r, g, b))
}

fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String {
    let orig_words: Vec<&str> = original.split_whitespace().collect();
    let corr_words: Vec<&str> = corrected.split_whitespace().collect();

    let (source_words, other_words) = if is_original {
        (&orig_words, &corr_words)
    } else {
        (&corr_words, &orig_words)
    };

    let mut changed = vec![false; source_words.len()];
    for (i, word) in source_words.iter().enumerate() {
        changed[i] = other_words.get(i) != Some(word);
    }

    let source_text = if is_original { original } else { corrected };
    let mut result = String::new();
    let mut word_idx = 0;

    for (line_num, line) in source_text.lines().enumerate() {
        if line_num > 0 {
            result.push('\n');
        }
        for (j, word) in line.split_whitespace().enumerate() {
            if j > 0 {
                result.push(' ');
            }
            let escaped = glib::markup_escape_text(word);
            if word_idx < changed.len() && changed[word_idx] {
                let color = if is_original { "#cc3333" } else { "#228833" };
                result.push_str(&format!("<span foreground=\"{}\" weight=\"bold\">{}</span>", color, escaped));
            } else {
                result.push_str(&escaped);
            }
            word_idx += 1;
        }
    }
    result
}


#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn parse_extracts_pron_element() {
        let g = "<verse>To /biː/</verse>\n<pron>BEE: be /biː/ keeps the long vowel.</pron>";
        let els = parse_gloss_tags(g);
        assert!(matches!(els[0], GlossElement::Verse(_)));
        assert!(
            matches!(&els[1], GlossElement::Pron(t) if t.contains("long vowel")),
            "expected a Pron element carrying the note, got {:?}", els.get(1)
        );
    }

    #[test]
    fn speakerless_verse_block_carries_forward_prior_speaker() {
        // Gloss 21730's defect: a continued speech's middle verse block omits
        // its <speaker>, so it rendered with neither label nor top spacing.
        // parse_gloss_tags must splice the carried speaker back in.
        let gloss = "<speaker>KING</speaker>\n\
                     <verse>You were ever good at sudden commendations,</verse>\n\
                     <gloss>The King opens with a rebuke.</gloss>\n\
                     <verse>To me you cannot reach. You play the spaniel,</verse>\n\
                     <gloss>Blunt and final.</gloss>\n\
                     <speaker>KING</speaker>\n\
                     <verse>Good man, sit down.</verse>";
        let els = parse_gloss_tags(gloss);
        // The speaker-less middle block must now open with a carried KING.
        assert!(
            matches!(&els[3], GlossElement::Speaker(n) if n == "KING"),
            "expected a carried-forward KING speaker before the middle verse \
             block, got {:?}",
            els.get(3)
        );
        assert!(matches!(&els[4], GlossElement::Verse(t) if t.starts_with("To me")));
        // The original two real speakers plus one synthetic = three speakers.
        let speakers = els
            .iter()
            .filter(|e| matches!(e, GlossElement::Speaker(_)))
            .count();
        assert_eq!(speakers, 3, "got elements: {:?}", els);
        // The synthetic speaker is dropped by gloss_blocks, so it must NOT add a
        // spurious block: still 3 source + 2 explication = 5 blocks.
        let blocks = gloss_blocks(gloss);
        let sources = blocks.iter().filter(|b| b.kind == BlockKind::Source).count();
        assert_eq!(sources, 3, "synthetic speaker must not add a source block");
    }

    #[test]
    fn blocks_in_document_order_with_kinds() {
        let gloss = "<speaker>CRANMER</speaker>\n\
                     <verse>Ah, my good Lord of Winchester, I thank you.</verse>\n\
                     <verse>You are always my good friend.</verse>\n\
                     <gloss>Cranmer opens with cutting irony.</gloss>\n\
                     <speaker>CRANMER</speaker>\n\
                     <verse>'Tis my undoing. Love and meekness, lord,</verse>\n\
                     <gloss>The tone shifts from irony to sincere counsel.</gloss>\n\
                     <gloss>[\"a quote\" — Macbeth 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 4); // source, explication, source, explication (echo excluded)

        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(
            blocks[0].text,
            "Ah, my good Lord of Winchester, I thank you.\nYou are always my good friend."
        );

        assert_eq!(blocks[1].kind, BlockKind::Explication);
        assert_eq!(blocks[1].index, 0);
        assert_eq!(blocks[1].text, "Cranmer opens with cutting irony.");

        assert_eq!(blocks[2].kind, BlockKind::Source);
        assert_eq!(blocks[2].index, 1);
        assert_eq!(blocks[2].text, "'Tis my undoing. Love and meekness, lord,");

        assert_eq!(blocks[3].kind, BlockKind::Explication);
        assert_eq!(blocks[3].index, 1);
        assert_eq!(blocks[3].text, "The tone shifts from irony to sincere counsel.");
    }

    #[test]
    fn all_echo_gloss_has_only_source_block() {
        let gloss = "<speaker>HAMLET</speaker>\n\
                     <verse>To be, or not to be</verse>\n\
                     <gloss>[\"q\" — Lr 1.1]</gloss>";
        let blocks = gloss_blocks(gloss);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        assert_eq!(blocks[0].text, "To be, or not to be");
    }

    #[test]
    fn source_block_keeps_raw_ipa_and_derives_clean_display() {
        let g = "<speaker>HAMLET</speaker>\n<verse>To /biː/ or not to /biː/</verse>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Source);
        // raw text (for TTS) keeps the IPA
        assert_eq!(blocks[0].text, "To /biː/ or not to /biː/");
        // display text (for the reader / accent-bar matcher) is stripped and
        // whitespace-normalized — no doubled gaps, no trailing space.
        assert_eq!(blocks[0].display, "To or not to");
    }

    #[test]
    fn lone_pron_note_produces_no_block() {
        // a <pron> note is neither a source nor explication block
        let g = "<pron>BEE: be /biː/ keeps the long vowel.</pron>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn tts_field_is_raw_display_field_is_stripped() {
        // play_block_tts (gloss.rs) clones `.text` for synthesis; the reader
        // path uses `.display`. This locks that the two diverge as intended:
        // raw keeps /IPA/, display strips it.
        let g = "<verse>/biː/ or not</verse>";
        let b = &gloss_blocks(g)[0];
        assert!(b.text.contains('/'), "TTS text must keep raw /IPA/");
        assert!(!b.display.contains('/'), "display text must be stripped");
    }

    #[test]
    fn explication_block_keeps_raw_ipa_and_strips_display() {
        // The explication push is a SEPARATE code path from the source push;
        // ensure it also keeps raw /IPA/ in `text` and strips it in `display`.
        let g = "<gloss>The operative word /ˈsʊfər/ carries the line.</gloss>";
        let blocks = gloss_blocks(g);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Explication);
        assert_eq!(blocks[0].text, "The operative word /ˈsʊfər/ carries the line.");
        assert_eq!(blocks[0].display, "The operative word carries the line.");
    }
}

#[cfg(test)]
mod snap_up_tests {
    use super::snap_up_to_row;

    // Live geometry from the dev log for the clipped Cranmer gloss: revealing
    // the last block computed target=514, and the visual row tops near it were
    // [450, 520, 547, 582] with the scroll ceiling max_value=570. The OLD
    // floor-snap pulled 514 down to 450 (re-hiding the bottom). Snapping UP must
    // pick 520 — the least row top >= 514 — keeping the bottom in view.
    #[test]
    fn snaps_up_to_next_row_not_down() {
        let rows = [450.0, 520.0, 547.0, 582.0];
        let v = snap_up_to_row(514.0, &rows, 0.0, 570.0);
        assert!(
            (v - 520.0).abs() < 0.5,
            "514 must snap UP to 520 (next row), not down to 450; got {v}"
        );
    }

    #[test]
    fn target_on_a_row_top_stays_put() {
        let rows = [450.0, 520.0, 547.0];
        let v = snap_up_to_row(520.0, &rows, 0.0, 570.0);
        assert!((v - 520.0).abs() < 0.5, "exact row top stays; got {v}");
    }

    #[test]
    fn target_past_last_row_uses_max_value() {
        // No row top >= target but target <= ceiling: use max_value so the
        // document end is reachable.
        let rows = [450.0, 520.0];
        let v = snap_up_to_row(560.0, &rows, 0.0, 570.0);
        assert!(
            (v - 570.0).abs() < 0.5,
            "target past last row top should use max_value (570); got {v}"
        );
    }

    #[test]
    fn clamps_to_max_value() {
        let rows = [450.0, 520.0, 600.0];
        // target above ceiling clamps to max_value first; next row 600 > 570
        // would exceed ceiling, so result clamps to 570.
        let v = snap_up_to_row(900.0, &rows, 0.0, 570.0);
        assert!((v - 570.0).abs() < 0.5, "must clamp to max_value; got {v}");
    }
}

#[cfg(test)]
mod cursor_scroll_tests {
    use super::{cursor_scroll_target, CursorScrollGeom};

    // Geometry captured from the live dev log for the Cranmer (H8) gloss whose
    // final explication clipped: viewport page_size=1055, scroll ceiling
    // max_value=570 (upper 1625 - page 1055), last block spans 450..1539 — a
    // block 1089px tall, i.e. TALLER than the 1055px viewport. The cursor sits
    // on this last block.
    const PAGE: f64 = 1055.0;
    const MAX_VALUE: f64 = 570.0;
    const LOWER: f64 = 0.0;
    const PAD: f64 = 24.0;

    #[test]
    fn over_tall_last_block_reveals_bottom_not_top() {
        // Cursor on the last block; viewport currently at top=450 (the buggy
        // plateau). The block's bottom (1539) is below the fold (450+1055=1505).
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 450.0,
            block_bottom: 1539.0,
            view_top: 450.0,
            view_bottom: 450.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: MAX_VALUE,
            pad: PAD,
        })
        .expect("over-tall block below fold must scroll, not report already-visible");

        // The fix: reveal the bottom. by_bottom = 1539+24-1055 = 508, clamped to
        // max_value 570 => 508. The OLD code did .min(block_top-pad=426) => 426,
        // which left the last line clipped. Assert we do NOT cap at the top.
        assert!(
            target > 450.0,
            "must scroll past the plateau top (450) to reveal the last row; got {target}"
        );
        assert!(
            (target - 508.0).abs() < 0.5,
            "should target by_bottom (508) to bring the block bottom to the fold; got {target}"
        );
    }

    #[test]
    fn fitting_block_below_fold_keeps_top_in_view() {
        // A SHORT block that fits in the viewport, sitting just below the fold:
        // reveal its bottom but never scroll its own top off-screen.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 1400.0,
            block_bottom: 1500.0, // 100px tall, fits easily
            view_top: 0.0,
            view_bottom: PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: 5000.0, // big ceiling so clamping doesn't mask the cap
            pad: PAD,
        })
        .expect("block below fold must scroll");

        // by_bottom = 1500+24-1055 = 469; block_top-pad = 1376. min => 469.
        // The cap (1376) does not bind here, so we land on by_bottom and the
        // block's top stays comfortably in view.
        assert!(
            (target - 469.0).abs() < 0.5,
            "fitting block should reveal bottom via by_bottom (469); got {target}"
        );
    }

    #[test]
    fn fully_visible_block_does_not_scroll() {
        // Block already inside the viewport (with pad): no scroll.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 200.0,
            block_bottom: 400.0,
            view_top: 100.0,
            view_bottom: 100.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: MAX_VALUE,
            pad: PAD,
        });
        assert!(target.is_none(), "fully visible block must not scroll");
    }

    #[test]
    fn block_above_viewport_reveals_top() {
        // Block starts above the current viewport top: bring its top into view.
        let target = cursor_scroll_target(&CursorScrollGeom {
            block_top: 300.0,
            block_bottom: 500.0,
            view_top: 800.0,
            view_bottom: 800.0 + PAGE,
            page_size: PAGE,
            lower: LOWER,
            max_value: 5000.0,
            pad: PAD,
        })
        .expect("block above viewport must scroll up");
        // block_top - pad = 276.
        assert!(
            (target - 276.0).abs() < 0.5,
            "should reveal block top (276); got {target}"
        );
    }
}

#[cfg(test)]
mod synopsis_label_tests {
    use super::*;

    #[test]
    fn bolds_standalone_label_paragraph() {
        let syn = "<p>Plot stuff here.</p><p>Shakespearean parallels:</p><p>The Court of Chancery is Elsinore.</p>";
        let (text, labels) = render_synopsis_with_labels(syn);
        assert_eq!(
            text,
            "Plot stuff here.\n\nShakespearean parallels:\n\nThe Court of Chancery is Elsinore."
        );
        assert_eq!(labels.len(), 1, "exactly one label paragraph");
        let (s, e) = labels[0];
        let chars: Vec<char> = text.chars().collect();
        let slice: String = chars[s..e].iter().collect();
        assert_eq!(slice, "Shakespearean parallels:");
    }

    #[test]
    fn does_not_bold_running_prose() {
        let syn = "<p>The fog descends on London. It is November.</p>";
        let (_text, labels) = render_synopsis_with_labels(syn);
        assert!(labels.is_empty());
    }

    #[test]
    fn plain_text_synopsis_has_no_labels() {
        let (text, labels) = render_synopsis_with_labels("Just plain text.");
        assert_eq!(text, "Just plain text.");
        assert!(labels.is_empty());
    }

    #[test]
    fn strip_ipa_removes_tagged_words() {
        // The removed span must not leave a doubled gap, and a trailing span
        // must not leave a trailing space.
        assert_eq!(strip_ipa("To /biː/ or not to /biː/"), "To or not to");
    }

    #[test]
    fn strip_ipa_keeps_literal_slash() {
        // a bare slash between ordinary words is NOT an IPA span
        assert_eq!(strip_ipa("read and/or write"), "read and/or write");
    }

    #[test]
    fn strip_ipa_strips_all_ascii_span() {
        // `/hav/` is valid OP IPA whose inner is all ASCII letters. The old
        // non-ASCII-only heuristic missed it, so its slashes mis-paired with the
        // NEXT span and swallowed the text between — the reader saw
        // `have /havˈkruːəltɪ/`. The boundary signal (space before the opening
        // `/`) now marks it IPA and strips it cleanly.
        assert_eq!(
            strip_ipa("For what they have /hav/ been. ’Tis a cruelty /ˈkruːəltɪ/"),
            "For what they have been. ’Tis a cruelty"
        );
    }

    #[test]
    fn strip_ipa_all_ascii_span_does_not_eat_following_text() {
        // Minimal form of the corruption: an all-ASCII span must not swallow the
        // words up to the next span.
        assert_eq!(strip_ipa("a /hav/ b /biː/"), "a b");
    }

    #[test]
    fn strip_ipa_all_ascii_span_does_not_collapse_newlines() {
        // The accent-bar bug: a Source block's `display` is strip_ipa(verses
        // joined by '\n'). The old heuristic let an all-ASCII `/hav/` span
        // mis-pair across the '\n', collapsing a 5-line block into 4 lines whose
        // last line no longer matched any buffer line — so the bar's end_line
        // matcher failed and the bar shrank to one line ([1,1] in the log).
        // strip must preserve every newline so the block keeps its line count.
        let block = "However faulty, yet should find /fəɪnd/ respect\n\
                     For what they have /hav/ been. ’Tis a cruelty /ˈkruːəltɪ/\n\
                     To load /loːd/ a falling /ˈfɑlɪn/ man.";
        let stripped = strip_ipa(block);
        assert_eq!(stripped.lines().count(), 3, "newlines must survive stripping");
        assert_eq!(
            stripped.lines().last().unwrap(),
            "To load a falling man.",
            "last line must stay matchable for the accent-bar end_line lookup"
        );
        assert!(!stripped.contains('/'), "no slash should remain: {stripped:?}");
    }

    #[test]
    fn strip_ipa_no_tags_is_identity() {
        assert_eq!(strip_ipa("plain modern line"), "plain modern line");
    }

    #[test]
    fn strip_ipa_handles_stress_marks() {
        assert_eq!(strip_ipa("the /ˈsʊfər/ of it"), "the of it");
    }

    #[test]
    fn strip_ipa_removes_leaked_prose_ipa() {
        // IPA the LLM might leak into explication prose must be strippable,
        // with no doubled/trailing spaces.
        assert_eq!(
            strip_ipa("the modern diphthong /eɪ/ vs the older /eː/"),
            "the modern diphthong vs the older"
        );
    }

    #[test]
    fn strip_ipa_no_space_before_punctuation() {
        // IPA appended to a word before punctuation must not leave a space
        // before the comma/semicolon/period (the screenshot's "good , wise").
        assert_eq!(
            strip_ipa("Not only good /gʊd/, but most religious /rɪˈlɪdʒəs/;"),
            "Not only good, but most religious;"
        );
        assert_eq!(
            strip_ipa("this great offender /ɒˈfɛndər/."),
            "this great offender."
        );
    }

    #[test]
    fn strip_ipa_real_verse_line_single_spaced() {
        // The reported bug: a full verse line with several appended IPA spans
        // renders with clean single spacing.
        assert_eq!(
            strip_ipa("Dread /drɛːd/ sovereign, how much are we bound /baʊnd/ to heaven /ˈhɛvn̩/"),
            "Dread sovereign, how much are we bound to heaven"
        );
    }

    #[test]
    fn ipa_for_tts_replaces_word_with_its_ipa() {
        // 'take /tɛːk/' -> '/tɛːk/' (word dropped) so v3 voices it once.
        assert_eq!(ipa_for_tts("take /tɛːk/"), "/tɛːk/");
        assert_eq!(ipa_for_tts("To take /tɛːk/ arms"), "To /tɛːk/ arms");
    }

    #[test]
    fn ipa_for_tts_all_ascii_span_replaces_its_word() {
        // The TTS twin of strip_ipa_strips_all_ascii_span: `have /hav/` must
        // become `/hav/` (word dropped, span kept), not get mis-paired with the
        // next span.
        assert_eq!(
            ipa_for_tts("they have /hav/ been. ’Tis a cruelty /ˈkruːəltɪ/"),
            "they /hav/ been. ’Tis a /ˈkruːəltɪ/"
        );
    }

    #[test]
    fn ipa_for_tts_keeps_untagged_words() {
        // Sparse tagging: words with no following IPA span are spoken as-is.
        assert_eq!(
            ipa_for_tts("Dread /drɛːd/ sovereign, how much are we bound /baʊnd/ to heaven /ˈhɛvn̩/"),
            "/drɛːd/ sovereign, how much are we /baʊnd/ to /ˈhɛvn̩/"
        );
    }

    #[test]
    fn ipa_for_tts_before_punctuation() {
        // 'good /gʊd/,' -> '/gʊd/,' — the IPA replaces the word but the comma
        // stays attached, no doubled word.
        assert_eq!(
            ipa_for_tts("Not only good /gʊd/, but most religious /rɪˈlɪdʒəs/;"),
            "Not only /gʊd/, but most /rɪˈlɪdʒəs/;"
        );
    }

    #[test]
    fn ipa_for_tts_ipa_at_line_start_is_kept() {
        // A leading IPA span with no preceding word stays as-is (nothing to drop).
        assert_eq!(ipa_for_tts("/biː/ or not"), "/biː/ or not");
        // Each IPA span replaces the word immediately before it: here the second
        // /biː/ follows "to", so "to" is the word it replaces.
        assert_eq!(ipa_for_tts("/biː/ or not to /biː/"), "/biː/ or not /biː/");
    }

    #[test]
    fn ipa_for_tts_keeps_literal_slash_and_plain() {
        // 'and/or' is not IPA; a no-IPA line is identity.
        assert_eq!(ipa_for_tts("read and/or write"), "read and/or write");
        assert_eq!(ipa_for_tts("plain modern line"), "plain modern line");
    }

    #[test]
    fn ipa_for_tts_adjacent_spans_dont_eat_each_other() {
        // Defensive: the prompt never emits two IPA spans with no word between,
        // but if it did, the word-pop must stop at the prior span's closing `/`
        // and not delete it. Each span survives.
        assert_eq!(ipa_for_tts("/biː/ /tuː/"), "/biː/ /tuː/");
        // 'be' precedes the first span and is dropped; the second span has only a
        // prior IPA span before it, so the guard keeps that span intact.
        assert_eq!(ipa_for_tts("To be /biː/ /tuː/"), "To /biː/ /tuː/");
    }

    #[test]
    fn contains_ipa_span_detects_real_ipa() {
        assert!(contains_ipa_span("/ˈdeɪli/"));
        assert!(contains_ipa_span("daily /ˈdeɪli/"));
        assert!(!contains_ipa_span("hard a"));          // plain hint, no slashes
        assert!(!contains_ipa_span("and/or"));          // glued literal slash, not on a boundary
        // A boundary-anchored slash token IS IPA even with an all-ASCII inner —
        // the same rule that lets strip_ipa handle `/hav/`. A user who wraps a
        // token in slashes means it as a pronunciation, and an all-ASCII OP span
        // (e.g. `/hav/`) must be recognized, not mistaken for a plain hint.
        assert!(contains_ipa_span("/word/"));
        assert!(contains_ipa_span("have /hav/"));
        assert!(!contains_ipa_span(""));
    }

    #[test]
    fn replace_word_ipa_swaps_the_words_ipa() {
        assert_eq!(
            replace_word_ipa("In daily /ˈdɛːli/ thanks, that gave /gɛːv/ us", "daily", "/ˈdeɪli/"),
            Some("In daily /ˈdeɪli/ thanks, that gave /gɛːv/ us".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_all_occurrences() {
        assert_eq!(
            replace_word_ipa("good /gʊd/ and more good /gʊd/", "good", "/guːd/"),
            Some("good /guːd/ and more good /guːd/".to_string())
        );
    }

    #[test]
    fn replace_word_ipa_is_whole_word() {
        assert_eq!(replace_word_ipa("daily /ˈdɛːli/ here", "day", "/deɪ/"), None);
    }

    #[test]
    fn replace_word_ipa_word_without_following_ipa_is_none() {
        assert_eq!(replace_word_ipa("In daily /ˈdɛːli/ thanks", "thanks", "/θaŋks/"), None);
    }

    #[test]
    fn replace_word_ipa_case_insensitive_word_match() {
        assert_eq!(
            replace_word_ipa("Daily /ˈdɛːli/ thanks", "daily", "/ˈdeɪli/"),
            Some("Daily /ˈdeɪli/ thanks".to_string())
        );
    }

    #[test]
    fn replace_in_source_block_rewrites_multiline_verse() {
        let g = "<speaker>GARDINER</speaker>\n<verse>In daily /ˈdɛːli/ thanks</verse>\n<verse>that gave /gɛːv/ us</verse>\n<gloss>note</gloss>";
        let out = replace_word_ipa_in_source_block(g, 0, "daily", "/ˈdeɪli/").unwrap();
        assert!(out.contains("daily /ˈdeɪli/"));
        assert!(out.contains("gave /gɛːv/")); // other word untouched
        assert!(out.contains("<gloss>note</gloss>")); // tags intact
        assert!(out.contains("<verse>"));
    }

    #[test]
    fn replace_in_source_block_none_when_word_absent() {
        let g = "<verse>In daily /ˈdɛːli/ thanks</verse>";
        assert!(replace_word_ipa_in_source_block(g, 0, "missing", "/x/").is_none());
    }

    #[test]
    fn replace_in_source_block_scopes_to_the_block() {
        // 'good' appears in TWO source blocks; fixing block 1 must not touch block 0.
        let g = "<verse>good /gʊd/ first</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ second</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // block 0 (index 0) keeps old IPA; block 1 (index 1) gets new.
        let first = out.find("first").unwrap();
        let second = out.find("second").unwrap();
        assert!(out[..first].contains("good /gʊd/")); // block 0 unchanged
        assert!(out[..second].contains("good /guːd/")); // block 1 changed
    }

    #[test]
    fn replace_in_source_block_distinguishes_identical_lines_across_blocks() {
        // Same verse line text in TWO source blocks; fixing block 1 must leave
        // block 0's identical line untouched (position-scoped, not text-scoped).
        let g = "<verse>good /gʊd/ same</verse>\n<gloss>a</gloss>\n<verse>good /gʊd/ same</verse>\n<gloss>b</gloss>";
        let out = replace_word_ipa_in_source_block(g, 1, "good", "/guːd/").unwrap();
        // exactly ONE rewrite: block 1's. Block 0 keeps /gʊd/.
        assert_eq!(out.matches("good /guːd/ same").count(), 1);
        assert_eq!(out.matches("good /gʊd/ same").count(), 1);
    }
}

#[cfg(test)]
mod synopsis_blocks_tests {
    use super::{synopsis_blocks, BlockKind};

    #[test]
    fn each_p_becomes_one_explication_block_skipping_labels() {
        let syn = "<p>First paragraph of action.</p>\
                   <p>Shakespearean parallels:</p>\
                   <p>Second paragraph continues.</p>";
        let blocks = synopsis_blocks(syn);
        // Label paragraph ("…parallels:") is skipped as a cursor stop.
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Explication));
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[1].index, 1);
        assert_eq!(blocks[0].text, "First paragraph of action.");
        assert_eq!(blocks[0].display, "First paragraph of action.");
        assert_eq!(blocks[1].text, "Second paragraph continues.");
    }

    #[test]
    fn legacy_plain_text_is_one_block() {
        let blocks = synopsis_blocks("Just plain text, no tags.");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Explication);
        assert_eq!(blocks[0].index, 0);
        assert_eq!(blocks[0].text, "Just plain text, no tags.");
    }

    #[test]
    fn empty_yields_no_blocks() {
        assert_eq!(synopsis_blocks("").len(), 0);
    }
}

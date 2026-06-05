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
    /// "Ask about this scene" card, stacked below the synopsis card (inside the
    /// same `container`, after the footer). Hidden unless the reader pressed `A`
    /// while the synopsis card is open. `ask_input` is an editable TextView that
    /// receives typed characters when the ask card holds focus.
    ask_container: gtk4::Box,
    ask_input: gtk4::TextView,
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
            ask_container,
            ask_input,
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
        self.ask_container.set_visible(false);
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
        self.hint.set_text("Esc close · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage");
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

        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_visible(true);
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

    pub fn show_synopsis(&self, title: &str, synopsis: &str, card_width: i32, card_height: i32) {
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
        self.gloss_view.set_top_margin(32);
        self.gloss_view.set_pixels_below_lines(6);
        let buffer = self.gloss_view.buffer();
        let (text, label_ranges) = render_synopsis_with_labels(synopsis);
        buffer.set_text(&text);
        // Remember label paragraphs (e.g. "Shakespearean parallels:") so they
        // can be bolded now and re-bolded after every apply_font (which applies
        // a regular-weight buffer-wide font tag that would otherwise win).
        *self.synopsis_label_ranges.borrow_mut() = label_ranges;
        self.apply_synopsis_label_bold();
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.hint.set_text("Esc close · j/k scroll · n/p scene · Ctrl+g glosses · A ask · U undo");
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
    /// of the buffer, in `iter_location` coordinate space (buffer-content y,
    /// which matches the vadjustment value: GTK scrolls the viewport over this
    /// same content space). Steps display line by display line with
    /// `forward_display_line` and reads each row's rect via `iter_location`, so
    /// wrapped paragraphs contribute one entry per real visual row at its true
    /// height — `line_yrange` would collapse them to one paragraph-tall row.
    fn display_rows(view: &gtk4::TextView) -> Vec<(f64, f64)> {
        let mut rows: Vec<(f64, f64)> = Vec::new();
        let buffer = view.buffer();
        let mut iter = buffer.start_iter();
        let end = buffer.end_iter();
        for _ in 0..8192 {
            let rect = view.iter_location(&iter);
            if rect.height() > 0 {
                let top = rect.y() as f64;
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
        let buffer = self.ask_input.buffer();
        buffer.set_text("");
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
        let max_value = (adj.upper() - adj.page_size()).max(adj.lower());
        let raw_target = adj.value() + step * 3.0 * delta as f64;
        // Row-snapping floors the viewport top to a whole row. On the last page
        // that floor can land a fraction of a row short of `max_value`, leaving
        // the final row(s) clipped under the footer and unreachable by further
        // `j` presses (the snap keeps returning the same sub-max top). When a
        // downward scroll already targets the bottom, go to `max_value` exactly
        // so the document end is fully shown; a partial row at the top of this
        // last page is acceptable (mirrors `scroll_gloss_to_bottom`).
        let target = if delta > 0 && raw_target >= max_value {
            max_value
        } else {
            self.snap_value_to_line(raw_target)
        };
        adj.set_value(target);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
    }

    pub fn scroll_gloss_to_top(&self) {
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
    }

    pub fn scroll_gloss_to_bottom(&self) {
        // Go to the true bottom: `upper - page_size` guarantees the final row is
        // reachable and shown. We do NOT row-snap here — snapping floors the top
        // and would push the last row below the viewport, hiding the end of the
        // document. Any partial row at the *top* of this last page is acceptable
        // (the user asked for the end); the bottom edge is exact.
        let adj = self.gloss_scrolled.vadjustment();
        let bottom = (adj.upper() - adj.page_size()).max(adj.lower());
        adj.set_value(bottom);
        self.update_bottom_clip();
        self.bar_drawing.queue_draw();
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

enum GlossElement {
    Speaker(String),
    Verse(String),
    Gloss(String),
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
            } else {
                remaining = &remaining[pos + 1..];
            }
        } else {
            break;
        }
    }
    elements
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
    for name in &["gloss-speaker", "gloss-speaker-first", "gloss-speaker-source", "gloss-verse", "gloss-para", "gloss-bracket", "gloss-quote", "gloss-quote-cont", "gloss-citation"] {
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

    tag_table.add(&speaker_tag);
    tag_table.add(&speaker_first_tag);
    tag_table.add(&speaker_source_tag);
    tag_table.add(&verse_tag);
    tag_table.add(&para_tag);
    tag_table.add(&bracket_tag);
    tag_table.add(&quote_tag);
    tag_table.add(&quote_cont_tag);
    tag_table.add(&citation_tag);

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
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
                apply_bracket_styling(&buffer, offset, &bracket_tag);

                let stripped = strip_brackets(text);
                if let Some(&num) = line_lookup.get(stripped.trim()) {
                    line_nums.push(LineNumber { buffer_line: line, number: num });
                }
            }
            GlossElement::Gloss(text) => {
                only_speakers_so_far = false;
                in_echoes = true;

                if let Some((quote, citation)) = split_echo(text) {
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
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, text);
                    let start = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
                }
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
}

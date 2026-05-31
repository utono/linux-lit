use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay};
use std::cell::RefCell;
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
    bar_ranges: Rc<RefCell<Vec<BarRange>>>,
    bar_color: Rc<RefCell<(f64, f64, f64)>>,
    bar_x: Rc<RefCell<i32>>,
    line_numbers: Rc<RefCell<Vec<LineNumber>>>,
    text_margins: i32,
    column_width: i32,
}

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

        gloss_scroll_overlay.set_child(Some(&gloss_scrolled));
        gloss_scroll_overlay.add_overlay(&bar_drawing);
        gloss_scroll_overlay.set_measure_overlay(&bar_drawing, false);
        gloss_scroll_overlay.set_clip_overlay(&bar_drawing, true);

        gloss_scroll_overlay.set_visible(false);

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
            bar_ranges,
            bar_color,
            bar_x,
            line_numbers,
            text_margins: text_margins as i32,
            column_width: column_width as i32,
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
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn show_gloss(&self, _original: &str, gloss: &str, card_height: i32) {
        self.show_gloss_with_color(_original, gloss, card_height, None, &[]);
    }

    pub fn show_gloss_with_color(&self, _original: &str, gloss: &str, card_height: i32, root_color: Option<&str>, source_line_numbers: &[(String, i64)]) {
        self.container.set_height_request(card_height);
        self.title.set_visible(false);
        self.title.set_vexpand(false);
        self.title.set_valign(Align::Start);
        self.title.set_halign(Align::Start);
        let left = self.column_width / 8;
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.hint.set_text("Esc close · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage");
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

        let (ranges, nums) = populate_gloss_buffer(&self.gloss_view, gloss, self.text_margins, bar_left, source_line_numbers);
        *self.bar_ranges.borrow_mut() = ranges;
        *self.line_numbers.borrow_mut() = nums;
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.gloss_scrolled.vadjustment().set_value(0.0);
        self.hint.set_visible(true);
        self.scrim.set_visible(false);
        self.container.set_visible(true);
    }

    pub fn show_synopsis(&self, title: &str, synopsis: &str, card_height: i32) {
        self.container.set_height_request(card_height);
        let left = self.column_width / 8;
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

        *self.bar_ranges.borrow_mut() = Vec::new();
        *self.line_numbers.borrow_mut() = Vec::new();

        self.gloss_view.set_left_margin(left);
        let buffer = self.gloss_view.buffer();
        buffer.set_text(synopsis);
        self.bar_drawing.queue_draw();

        self.gloss_scroll_overlay.set_visible(true);
        self.gloss_scrolled.vadjustment().set_value(0.0);
        self.hint.set_text("Esc close · j/k scroll");
        self.hint.set_visible(true);
        self.scrim.set_visible(false);
        self.container.set_visible(true);
    }

    pub fn show_loading(&self) {
        self.show_loading_message("Glossing...");
    }

    pub fn show_loading_message(&self, message: &str) {
        self.title.set_text(message);
        self.title.set_visible(true);
        self.title.set_vexpand(true);
        self.title.set_valign(Align::Center);
        self.title.set_halign(Align::Center);
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.gloss_scroll_overlay.set_visible(false);
        self.position_label.set_visible(false);
        self.hint.set_visible(false);
        self.scrim.set_visible(false);
        self.container.set_visible(true);
    }

    pub fn scroll_gloss(&self, delta: i32) {
        let adj = self.gloss_scrolled.vadjustment();
        let step = 60.0 * delta as f64;
        let new_val = (adj.value() + step).clamp(adj.lower(), adj.upper() - adj.page_size());
        adj.set_value(new_val);
        self.bar_drawing.queue_draw();
    }

    pub fn scroll_gloss_to_top(&self) {
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.bar_drawing.queue_draw();
    }

    pub fn scroll_gloss_to_bottom(&self) {
        let adj = self.gloss_scrolled.vadjustment();
        adj.set_value(adj.upper() - adj.page_size());
        self.bar_drawing.queue_draw();
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
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

fn populate_gloss_buffer(view: &gtk4::TextView, gloss: &str, _text_margins: i32, bar_left: i32, source_line_numbers: &[(String, i64)]) -> (Vec<BarRange>, Vec<LineNumber>) {
    let buffer = view.buffer();
    buffer.set_text("");

    let tag_table = buffer.tag_table();
    for name in &["gloss-speaker", "gloss-speaker-first", "gloss-verse", "gloss-para", "gloss-bracket"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let quote_speaker = bar_left + 60;
    let quote_verse = quote_speaker + 60;

    let speaker_tag = gtk4::TextTag::builder()
        .name("gloss-speaker")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .left_margin(quote_speaker)
        .pixels_above_lines(36)
        .build();

    let verse_tag = gtk4::TextTag::builder()
        .name("gloss-verse")
        .left_margin(quote_verse)
        .build();

    let para_tag = gtk4::TextTag::builder()
        .name("gloss-para")
        .left_margin(quote_speaker)
        .pixels_above_lines(24)
        .build();

    let speaker_first_tag = gtk4::TextTag::builder()
        .name("gloss-speaker-first")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .left_margin(quote_speaker)
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

    // Citation line: indented further, smaller and dimmer.
    let citation_tag = gtk4::TextTag::builder()
        .name("gloss-citation")
        .left_margin(quote_verse)
        .scale(0.85)
        .build();

    tag_table.add(&speaker_tag);
    tag_table.add(&speaker_first_tag);
    tag_table.add(&verse_tag);
    tag_table.add(&para_tag);
    tag_table.add(&bracket_tag);
    tag_table.add(&quote_tag);
    tag_table.add(&citation_tag);

    let elements = parse_gloss_tags(gloss);
    let mut first = true;
    let mut only_speakers_so_far = true;
    let mut bar_ranges: Vec<BarRange> = Vec::new();
    let mut line_nums: Vec<LineNumber> = Vec::new();
    let mut current_block_start: Option<i32> = None;

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
                if current_block_start.is_none() {
                    current_block_start = Some(line);
                }
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, name);
                let start = buffer.iter_at_offset(offset);
                let tag = if only_speakers_so_far { &speaker_first_tag } else { &speaker_tag };
                buffer.apply_tag(tag, &start, &buffer.end_iter());
            }
            GlossElement::Verse(text) => {
                only_speakers_so_far = false;
                if current_block_start.is_none() {
                    current_block_start = Some(line);
                }
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
                if let Some(start_line) = current_block_start.take() {
                    let end_line = line - 1;
                    bar_ranges.push(BarRange { start_line, end_line });
                }

                if let Some((quote, citation)) = split_echo(text) {
                    // Echo: quote on one line, citation indented below it.
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &quote);
                    let qstart = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&quote_tag, &qstart, &buffer.end_iter());

                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, "\n");
                    let cit_offset = buffer.end_iter().offset();
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &citation);
                    let cstart = buffer.iter_at_offset(cit_offset);
                    buffer.apply_tag(&citation_tag, &cstart, &buffer.end_iter());
                } else {
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, text);
                    let start = buffer.iter_at_offset(offset);
                    buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
                }
            }
        }
    }

    if let Some(start_line) = current_block_start {
        let end_line = buffer.end_iter().line();
        bar_ranges.push(BarRange { start_line, end_line });
    }

    (bar_ranges, line_nums)
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

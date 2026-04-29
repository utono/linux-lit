use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay};

pub struct CorrectionOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    orig_header: Label,
    original_label: Label,
    corr_header: Label,
    corrected_label: Label,
    hint: Label,
    gloss_scrolled: gtk4::ScrolledWindow,
    gloss_view: gtk4::TextView,
    text_margins: i32,
}

impl CorrectionOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.set_width_request(column_width as i32);
        container.add_css_class("correction-overlay");

        let title = Label::new(Some("Gloss"));
        title.add_css_class("correction-title");
        title.set_margin_start(text_margins as i32);
        title.set_margin_end(text_margins as i32);
        title.set_margin_top(24);
        container.append(&title);

        let orig_header = Label::new(Some("ORIGINAL"));
        orig_header.add_css_class("correction-header");
        orig_header.set_halign(Align::Start);
        container.append(&orig_header);

        let original_label = Label::new(None);
        original_label.set_wrap(true);
        original_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        original_label.set_halign(Align::Start);
        original_label.set_hexpand(false);
        original_label.set_selectable(false);
        original_label.set_max_width_chars(1);
        original_label.add_css_class("correction-text");
        container.append(&original_label);

        let corr_header = Label::new(Some("GLOSS"));
        corr_header.add_css_class("correction-header");
        corr_header.set_halign(Align::Start);
        container.append(&corr_header);

        let corrected_label = Label::new(None);
        corrected_label.set_wrap(true);
        corrected_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        corrected_label.set_halign(Align::Start);
        corrected_label.set_hexpand(false);
        corrected_label.set_selectable(false);
        corrected_label.set_max_width_chars(1);
        corrected_label.add_css_class("correction-text");
        container.append(&corrected_label);

        let gloss_scrolled = gtk4::ScrolledWindow::new();
        gloss_scrolled.set_vexpand(true);
        gloss_scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        gloss_scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);

        let gloss_view = gtk4::TextView::new();
        gloss_view.set_editable(false);
        gloss_view.set_cursor_visible(false);
        gloss_view.set_wrap_mode(gtk4::WrapMode::Word);
        gloss_view.set_left_margin(text_margins as i32);
        gloss_view.set_right_margin(text_margins as i32);
        gloss_view.set_top_margin(24);
        gloss_view.set_bottom_margin(12);
        gloss_view.add_css_class("correction-text");
        gloss_scrolled.set_child(Some(&gloss_view));
        gloss_scrolled.set_visible(false);

        container.append(&gloss_scrolled);

        let hint = Label::new(Some("Esc = close  ·  a = amend  ·  r = regenerate"));
        hint.add_css_class("correction-hint");
        hint.set_margin_start(text_margins as i32);
        hint.set_margin_end(text_margins as i32);
        hint.set_margin_bottom(8);
        container.append(&hint);

        container.set_visible(false);

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.set_hexpand(true);
        scrim.set_vexpand(true);
        scrim.add_css_class("correction-scrim");
        scrim.set_visible(false);

        CorrectionOverlay {
            overlay,
            scrim,
            container,
            title,
            orig_header,
            original_label,
            corr_header,
            corrected_label,
            hint,
            gloss_scrolled,
            gloss_view,
            text_margins: text_margins as i32,
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
        self.gloss_scrolled.set_visible(false);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn show_gloss(&self, _original: &str, gloss: &str, card_height: i32) {
        self.container.set_height_request(card_height);
        self.title.set_visible(false);
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);

        populate_gloss_buffer(&self.gloss_view, gloss, self.text_margins);

        self.gloss_scrolled.set_visible(true);
        self.gloss_scrolled.vadjustment().set_value(0.0);
        self.hint.set_visible(true);
        self.scrim.set_visible(false);
        self.container.set_visible(true);
    }

    pub fn show_loading(&self) {
        self.show_loading_message("Glossing...");
    }

    pub fn show_loading_message(&self, message: &str) {
        self.title.set_text(message);
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        self.gloss_scrolled.set_visible(false);
        self.hint.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn scroll_gloss(&self, delta: i32) {
        let adj = self.gloss_scrolled.vadjustment();
        let step = 60.0 * delta as f64;
        let new_val = (adj.value() + step).clamp(adj.lower(), adj.upper() - adj.page_size());
        adj.set_value(new_val);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
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

fn populate_gloss_buffer(view: &gtk4::TextView, gloss: &str, text_margins: i32) {
    let buffer = view.buffer();
    buffer.set_text("");

    let tag_table = buffer.tag_table();
    for name in &["gloss-speaker", "gloss-speaker-first", "gloss-verse", "gloss-para"] {
        if let Some(old) = tag_table.lookup(name) {
            tag_table.remove(&old);
        }
    }

    let speaker_tag = gtk4::TextTag::builder()
        .name("gloss-speaker")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .pixels_above_lines(24)
        .build();

    let verse_tag = gtk4::TextTag::builder()
        .name("gloss-verse")
        .left_margin(text_margins + 60)
        .build();

    let para_tag = gtk4::TextTag::builder()
        .name("gloss-para")
        .left_margin(text_margins + 60)
        .pixels_above_lines(24)
        .build();

    let speaker_first_tag = gtk4::TextTag::builder()
        .name("gloss-speaker-first")
        .variant(pango::Variant::SmallCaps)
        .weight(400)
        .scale(0.75)
        .build();

    tag_table.add(&speaker_tag);
    tag_table.add(&speaker_first_tag);
    tag_table.add(&verse_tag);
    tag_table.add(&para_tag);

    let elements = parse_gloss_tags(gloss);
    let mut first = true;
    let mut first_speaker = true;

    for el in &elements {
        if !first {
            let mut end = buffer.end_iter();
            buffer.insert(&mut end, "\n");
        }
        first = false;

        let offset = buffer.end_iter().offset();
        match el {
            GlossElement::Speaker(name) => {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, name);
                let start = buffer.iter_at_offset(offset);
                let tag = if first_speaker { &speaker_first_tag } else { &speaker_tag };
                first_speaker = false;
                buffer.apply_tag(tag, &start, &buffer.end_iter());
            }
            GlossElement::Verse(text) => {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&verse_tag, &start, &buffer.end_iter());
            }
            GlossElement::Gloss(text) => {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, text);
                let start = buffer.iter_at_offset(offset);
                buffer.apply_tag(&para_tag, &start, &buffer.end_iter());
            }
        }
    }
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

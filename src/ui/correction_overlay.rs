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
    gloss_label: Label,
    full_width: i32,
}

impl CorrectionOverlay {
    pub fn new(column_width: u32) -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        container.set_halign(Align::Center);
        container.set_valign(Align::Fill);
        container.set_vexpand(true);
        container.set_width_request(column_width as i32 + 100);
        container.add_css_class("correction-overlay");

        // Title
        let title = Label::new(Some("Gloss"));
        title.add_css_class("correction-title");
        container.append(&title);

        // Original section
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

        // Corrected section
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

        // Scrollable gloss label (used by show_gloss)
        let gloss_scrolled = gtk4::ScrolledWindow::new();
        gloss_scrolled.set_vexpand(true);
        gloss_scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);
        gloss_scrolled.set_vscrollbar_policy(gtk4::PolicyType::External);

        let gloss_label = Label::new(None);
        gloss_label.set_wrap(true);
        gloss_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        gloss_label.set_halign(Align::Start);
        gloss_label.set_valign(Align::Start);
        gloss_label.set_hexpand(true);
        gloss_label.set_selectable(false);
        gloss_label.set_max_width_chars(1);
        gloss_label.add_css_class("correction-text");
        gloss_scrolled.set_child(Some(&gloss_label));
        gloss_scrolled.set_visible(false);

        container.append(&gloss_scrolled);

        // Hint
        let hint = Label::new(Some("Esc = close  ·  a = amend  ·  r = regenerate"));
        hint.add_css_class("correction-hint");
        container.append(&hint);

        container.set_visible(false);

        // Scrim: full-size semi-transparent dimming layer behind the panel
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
            gloss_label,
            full_width: column_width as i32 + 100,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.container);
    }

    pub fn show(&self, original: &str, corrected: &str) {
        self.container.set_valign(Align::Start);
        self.container.set_margin_top(80);
        self.container.set_width_request(self.full_width);
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

    pub fn show_gloss(&self, _original: &str, gloss: &str) {
        self.container.set_valign(Align::Fill);
        self.container.set_vexpand(true);
        self.container.set_margin_top(0);
        self.container.set_width_request(self.full_width);
        self.title.set_text("Gloss");
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);
        let markup = markdown_to_pango(gloss);
        self.gloss_label.set_markup(&markup);
        self.gloss_scrolled.set_visible(true);
        self.gloss_scrolled.vadjustment().set_value(0.0);
        self.hint.set_visible(true);
        self.scrim.set_visible(true);
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
        self.container.set_valign(Align::Center);
        self.container.set_margin_top(0);
        self.container.set_width_request(self.full_width);
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

fn markdown_to_pango(text: &str) -> String {
    let mut result = String::new();
    let mut in_bold = false;
    let escaped = glib::markup_escape_text(text);
    let mut chars = escaped.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            if in_bold {
                result.push_str("</b>");
            } else {
                result.push_str("<b>");
            }
            in_bold = !in_bold;
        } else {
            result.push(ch);
        }
    }
    if in_bold {
        result.push_str("</b>");
    }
    result
}

/// Build Pango markup highlighting words that differ between original and corrected.
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

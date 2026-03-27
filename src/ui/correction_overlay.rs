use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay, ScrolledWindow};

pub struct CorrectionOverlay {
    pub overlay: Overlay,
    container: gtk4::Box,
    original_label: Label,
    corrected_label: Label,
}

impl CorrectionOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.set_margin_top(40);
        container.set_margin_bottom(40);
        container.set_margin_start(60);
        container.set_margin_end(60);
        container.add_css_class("correction-overlay");

        // Title
        let title = Label::new(Some("Correction Review"));
        title.add_css_class("correction-title");
        container.append(&title);

        // Original section
        let orig_header = Label::new(Some("ORIGINAL"));
        orig_header.add_css_class("correction-header");
        orig_header.set_halign(Align::Start);
        container.append(&orig_header);

        let original_label = Label::new(None);
        original_label.set_wrap(true);
        original_label.set_halign(Align::Start);
        original_label.set_selectable(false);
        original_label.add_css_class("correction-text");

        let orig_scroll = ScrolledWindow::new();
        orig_scroll.set_child(Some(&original_label));
        orig_scroll.set_max_content_height(300);
        orig_scroll.set_propagate_natural_height(true);
        container.append(&orig_scroll);

        // Corrected section
        let corr_header = Label::new(Some("CORRECTED"));
        corr_header.add_css_class("correction-header");
        corr_header.set_halign(Align::Start);
        container.append(&corr_header);

        let corrected_label = Label::new(None);
        corrected_label.set_wrap(true);
        corrected_label.set_halign(Align::Start);
        corrected_label.set_selectable(false);
        corrected_label.add_css_class("correction-text");

        let corr_scroll = ScrolledWindow::new();
        corr_scroll.set_child(Some(&corrected_label));
        corr_scroll.set_max_content_height(300);
        corr_scroll.set_propagate_natural_height(true);
        container.append(&corr_scroll);

        // Hint
        let hint = Label::new(Some("y = accept  ·  n / Esc = reject"));
        hint.add_css_class("correction-hint");
        container.append(&hint);

        container.set_visible(false);

        CorrectionOverlay {
            overlay,
            container,
            original_label,
            corrected_label,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.container);
    }

    pub fn show(&self, original: &str, corrected: &str) {
        let orig_markup = build_diff_markup(original, corrected, true);
        let corr_markup = build_diff_markup(original, corrected, false);
        self.original_label.set_markup(&orig_markup);
        self.corrected_label.set_markup(&corr_markup);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}

/// Build Pango markup highlighting words that differ between original and corrected.
/// If `is_original` is true, highlights removed/changed words in the original;
/// otherwise highlights new/changed words in the corrected text.
fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let corr_lines: Vec<&str> = corrected.lines().collect();
    let max_lines = orig_lines.len().max(corr_lines.len());

    let mut result = String::new();
    for i in 0..max_lines {
        if i > 0 {
            result.push('\n');
        }
        let orig_line = orig_lines.get(i).copied().unwrap_or("");
        let corr_line = corr_lines.get(i).copied().unwrap_or("");

        let orig_words: Vec<&str> = orig_line.split_whitespace().collect();
        let corr_words: Vec<&str> = corr_line.split_whitespace().collect();

        let (source_words, other_words) = if is_original {
            (&orig_words, &corr_words)
        } else {
            (&corr_words, &orig_words)
        };

        for (j, word) in source_words.iter().enumerate() {
            if j > 0 {
                result.push(' ');
            }
            let differs = other_words.get(j) != Some(word);
            let escaped = glib::markup_escape_text(word);
            if differs {
                let color = if is_original { "#cc3333" } else { "#228833" };
                result.push_str(&format!("<span foreground=\"{}\" weight=\"bold\">{}</span>", color, escaped));
            } else {
                result.push_str(&escaped);
            }
        }
    }
    result
}

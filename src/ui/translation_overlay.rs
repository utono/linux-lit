use crate::db::models::Line;
use gtk4::prelude::*;
use gtk4::{Align, Label, Orientation, Overlay, ScrolledWindow, TextView};
use std::cell::RefCell;

/// One render unit in the translation overlay: either a speaker's speech
/// (with original + translation paired per line) or a non-spoken interlude
/// (stage direction / scene header, shown full-width with no translation).
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationBlock {
    /// Speaker label for a speech block; `None` for a non-spoken interlude.
    pub speaker: Option<String>,
    /// (original_text, translation_or_empty) per source line, in order.
    pub lines: Vec<(String, String)>,
    /// Inclusive range of `work.lines` indices this block covers.
    pub start_idx: usize,
    pub end_idx: usize,
}

/// Group a slice of scene lines into ordered blocks. Consecutive lines that
/// share the same `speaker` form one speech block; runs of `speaker == None`
/// lines (stage directions, scene headers) form non-spoken interlude blocks.
/// `idx_of(i)` maps the i-th element of `lines` back to its `work.lines` index;
/// `translation_of(line_id)` returns the modern translation if one exists.
pub fn group_scene_into_blocks(
    lines: &[Line],
    idx_of: impl Fn(usize) -> usize,
    translation_of: impl Fn(i64) -> Option<String>,
) -> Vec<TranslationBlock> {
    let mut blocks: Vec<TranslationBlock> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let work_idx = idx_of(i);
        let translation = if line.speaker.is_some() {
            translation_of(line.id).unwrap_or_default()
        } else {
            String::new()
        };

        let same_as_prev = blocks
            .last()
            .map(|b| b.speaker == line.speaker)
            .unwrap_or(false);

        if same_as_prev {
            let b = blocks.last_mut().unwrap();
            b.lines.push((line.text.clone(), translation));
            b.end_idx = work_idx;
        } else {
            blocks.push(TranslationBlock {
                speaker: line.speaker.clone(),
                lines: vec![(line.text.clone(), translation)],
                start_idx: work_idx,
                end_idx: work_idx,
            });
        }
    }

    blocks
}

pub struct TranslationOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    /// Scroll viewport shared by both columns (one scrollbar == lockstep).
    scrolled: ScrolledWindow,
    /// Vertical stack of header rows + paired column blocks, inside `scrolled`.
    content_vbox: gtk4::Box,
    /// Per rendered speech/interlude block: the (start_idx, end_idx) source
    /// range and the block's top widget, so we can scroll to the cursor block.
    block_widgets: RefCell<Vec<(usize, usize, gtk4::Box)>>,
}

impl TranslationOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(Orientation::Vertical, 0);
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        let container = gtk4::Box::new(Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.add_css_class("gloss-overlay");
        container.set_visible(false);

        let title = Label::new(Some("Translation"));
        title.add_css_class("gloss-title");
        title.set_halign(Align::Start);
        title.set_margin_start(24);
        title.set_margin_top(24);
        title.set_margin_bottom(8);
        container.append(&title);

        let content_vbox = gtk4::Box::new(Orientation::Vertical, 0);
        content_vbox.set_hexpand(true);

        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled.set_propagate_natural_height(false);
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        scrolled.set_margin_bottom(20);
        scrolled.set_child(Some(&content_vbox));
        container.append(&scrolled);

        Self {
            overlay,
            scrim,
            container,
            title,
            scrolled,
            content_vbox,
            block_widgets: RefCell::new(Vec::new()),
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

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
    }

    /// Populate and reveal the overlay. `blocks` come from
    /// `group_scene_into_blocks`. `text_fg`/`dim_fg` are theme colors.
    pub fn show(
        &self,
        title: &str,
        blocks: &[TranslationBlock],
        card_width: i32,
        card_height: i32,
        text_fg: &str,
        dim_fg: &str,
    ) {
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.title.set_text(title);

        // Clear any previous render.
        while let Some(child) = self.content_vbox.first_child() {
            self.content_vbox.remove(&child);
        }
        self.block_widgets.borrow_mut().clear();

        let side_margin = card_width / 12;
        let col_width = ((card_width - 2 * side_margin) / 2 - 12).max(120);

        for block in blocks {
            let block_box = gtk4::Box::new(Orientation::Vertical, 0);
            block_box.set_margin_start(side_margin);
            block_box.set_margin_end(side_margin);
            block_box.set_margin_top(14);

            if let Some(speaker) = &block.speaker {
                // Full-width speaker header. Match the main reading card's
                // `speaker-name` tag (app.rs): small-caps, normal weight (400),
                // 0.75 scale, body text color — NOT the dim, letter-spaced
                // `gloss-header` look.
                let header = Label::new(None);
                header.set_halign(Align::Start);
                header.set_markup(&format!(
                    "<span foreground='{}' font_variant='small-caps' font_weight='normal' size='75%'>{}</span>",
                    text_fg,
                    glib_escape(speaker),
                ));
                header.set_margin_bottom(4);
                block_box.append(&header);

                // Two-column paired text.
                let cols = gtk4::Box::new(Orientation::Horizontal, 0);
                let orig = make_column(col_width, text_fg, false);
                let trans = make_column(col_width, dim_fg, true);
                let mut orig_text = String::new();
                let mut trans_text = String::new();
                for (o, t) in &block.lines {
                    orig_text.push_str(o);
                    orig_text.push('\n');
                    trans_text.push_str(t);
                    trans_text.push('\n');
                }
                orig.buffer().set_text(orig_text.trim_end_matches('\n'));
                trans.buffer().set_text(trans_text.trim_end_matches('\n'));

                let divider = gtk4::Separator::new(Orientation::Vertical);
                divider.add_css_class("column-divider");
                divider.set_margin_start(12);
                divider.set_margin_end(12);

                cols.append(&orig);
                cols.append(&divider);
                cols.append(&trans);
                block_box.append(&cols);
            } else {
                // Non-spoken interlude: full-width italic, no translation column.
                let view = TextView::new();
                view.set_editable(false);
                view.set_cursor_visible(false);
                view.set_focusable(false);
                view.set_wrap_mode(gtk4::WrapMode::WordChar);
                view.add_css_class("gloss-text");
                let mut text = String::new();
                for (o, _) in &block.lines {
                    text.push_str(o);
                    text.push('\n');
                }
                view.buffer().set_text(text.trim_end_matches('\n'));
                block_box.append(&view);
            }

            self.content_vbox.append(&block_box);
            self.block_widgets
                .borrow_mut()
                .push((block.start_idx, block.end_idx, block_box));
        }

        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.scroll_to_top();
    }

    pub fn scroll(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let step = adj.page_size() * 0.15;
        let max = (adj.upper() - adj.page_size()).max(adj.lower());
        let target = (adj.value() + step * 3.0 * delta as f64)
            .clamp(adj.lower(), max);
        adj.set_value(target);
    }

    pub fn scroll_to_top(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
    }

    /// Scroll so the block whose source range contains `work_idx` sits at the
    /// top of the viewport. No-op if no block matches.
    pub fn scroll_to_block(&self, work_idx: usize) {
        let target = self.block_widgets.borrow().iter().find_map(|(s, e, w)| {
            if work_idx >= *s && work_idx <= *e {
                Some(w.clone())
            } else {
                None
            }
        });
        let Some(widget) = target else { return };
        // Defer one tick so allocations are settled before measuring.
        // `compute_point` maps the block's top-left (0,0) into the
        // content_vbox's coordinate space; that y IS the scroll offset that
        // brings the block to the viewport top.
        let scrolled = self.scrolled.clone();
        let content = self.content_vbox.clone();
        glib::idle_add_local_once(move || {
            let origin = gtk4::graphene::Point::new(0.0, 0.0);
            if let Some(point) = widget.compute_point(&content, &origin) {
                let adj = scrolled.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(adj.lower());
                adj.set_value((point.y() as f64).clamp(adj.lower(), max));
            }
        });
    }
}

fn make_column(width: i32, color: &str, italic: bool) -> TextView {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_focusable(false);
    view.set_wrap_mode(gtk4::WrapMode::WordChar);
    view.set_size_request(width, -1);
    view.add_css_class("gloss-text");
    if italic {
        view.add_css_class("translation-col");
    }
    // Color via an inline CSS provider would be heavier; rely on the
    // .gloss-text / .translation-col classes for base style and let the
    // theme's text color show. `color` reserved for a future inline tag.
    let _ = color;
    view
}

fn glib_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn mk(id: i64, text: &str, speaker: Option<&str>) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: speaker.is_some(),
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn groups_consecutive_speaker_lines_into_one_block() {
        let lines = vec![
            mk(10, "She shall be to the happiness of England", Some("CRANMER")),
            mk(11, "An aged princess; many days shall see her,", Some("CRANMER")),
            mk(12, "O lord", Some("KING")),
        ];
        let trans = |id: i64| match id {
            10 => Some("She shall be to England's happiness".to_string()),
            11 => Some("An aged princess; many days will see her,".to_string()),
            12 => Some("O lord".to_string()),
            _ => None,
        };
        let blocks = group_scene_into_blocks(&lines, |i| i, trans);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker.as_deref(), Some("CRANMER"));
        assert_eq!(blocks[0].lines.len(), 2);
        assert_eq!(blocks[0].lines[0].0, "She shall be to the happiness of England");
        assert_eq!(blocks[0].lines[0].1, "She shall be to England's happiness");
        assert_eq!(blocks[0].start_idx, 0);
        assert_eq!(blocks[0].end_idx, 1);
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
        assert_eq!(blocks[1].start_idx, 2);
        assert_eq!(blocks[1].end_idx, 2);
    }

    #[test]
    fn non_spoken_lines_form_their_own_block_with_blank_translation() {
        let lines = vec![
            mk(20, "Enter KING and CRANMER", None),
            mk(21, "Thou speakest wonders.", Some("KING")),
        ];
        let blocks = group_scene_into_blocks(&lines, |i| i, |_| None);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker, None);
        assert_eq!(blocks[0].lines[0].0, "Enter KING and CRANMER");
        assert_eq!(blocks[0].lines[0].1, "");
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
    }

    #[test]
    fn idx_of_maps_back_to_work_indices() {
        let lines = vec![
            mk(30, "first", Some("A")),
            mk(31, "second", Some("A")),
        ];
        let blocks = group_scene_into_blocks(&lines, |i| 100 + i, |_| None);
        assert_eq!(blocks[0].start_idx, 100);
        assert_eq!(blocks[0].end_idx, 101);
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        let blocks = group_scene_into_blocks(&[], |i| i, |_| None);
        assert!(blocks.is_empty());
    }
}

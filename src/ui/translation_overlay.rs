use crate::db::models::Line;
use gtk4::prelude::*;
use gtk4::{Align, Label, Orientation, Overlay, ScrolledWindow, TextView};
use std::cell::RefCell;
use std::rc::Rc;

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

/// One rendered block's views + source range, for cursor highlighting and
/// scroll-follow. `trans` is None for a non-spoken interlude block (it has a
/// single `orig` view).
struct BlockEntry {
    start_idx: usize,
    end_idx: usize,
    orig: gtk4::TextView,
    trans: Option<gtk4::TextView>,
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
    /// Per rendered speech/interlude block: source range and the original/
    /// translation views, so we can highlight and scroll to the cursor line.
    block_widgets: RefCell<Vec<BlockEntry>>,
    /// Bottom clip guard. Custom per-row mask (the columns are TextViews inside a
    /// Box, so the box-slack guard would cut the bottom column's partial row).
    clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard,
    /// The column TextViews currently rendered (every `orig` + `trans`), read live
    /// by the clip closure so the per-row mask tracks each `show()`.
    clip_views: Rc<RefCell<Vec<gtk4::TextView>>>,
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

        // Free-scroll bottom clip. The scrolled child is a Box, but its children
        // are TextView columns that DO render a partial wrapped row at the
        // viewport edge — so the box-slack guard (clip 0 on overflow) would leave
        // that row cut. Use a CUSTOM per-row mask that reads the column views'
        // visual rows. `clip_views` holds the currently-rendered column views; the
        // closure reads them live, so it tracks each `show()`.
        let clip_views: Rc<RefCell<Vec<gtk4::TextView>>> = Rc::new(RefCell::new(Vec::new()));
        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&scrolled));
        let clip_guard = {
            let views = clip_views.clone();
            let content = content_vbox.clone();
            let recompute: crate::ui::bottom_clip_guard::ClipFn =
                Rc::new(move |clip: &gtk4::Box, sw: &ScrolledWindow| {
                    let vs = views.borrow();
                    crate::ui::recompute_translation_bottom_clip(
                        clip,
                        sw,
                        content.upcast_ref::<gtk4::Widget>(),
                        &vs,
                    );
                });
            crate::ui::bottom_clip_guard::BottomClipGuard::attach_custom(
                &scroll_overlay,
                &scrolled,
                recompute,
            )
        };
        container.append(&scroll_overlay);

        Self {
            overlay,
            scrim,
            container,
            title,
            scrolled,
            content_vbox,
            block_widgets: RefCell::new(Vec::new()),
            clip_guard,
            clip_views,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_overlay_panel(
            &self.overlay, child, &self.scrim, &self.container,
        );
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
        body_font_size: i32,
        cursor_line_bg: &str,
    ) {
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.title.set_text(title);

        // Clear any previous render.
        while let Some(child) = self.content_vbox.first_child() {
            self.content_vbox.remove(&child);
        }
        self.block_widgets.borrow_mut().clear();
        self.clip_views.borrow_mut().clear();

        let side_margin = card_width / 12;
        let col_width = ((card_width - 2 * side_margin) / 2 - 12).max(120);
        // Speaker header point size: 0.75 of the body (reader) font, matching
        // the main card's `speaker-name` tag (scale 0.75). A relative `size='75%'`
        // would resolve against the Label's tiny default UI font, so we size it
        // absolutely against the overlay's actual body font.
        let header_pt = ((body_font_size as f64) * 0.75).round().max(8.0) as i32;

        for block in blocks {
            let block_box = gtk4::Box::new(Orientation::Vertical, 0);
            block_box.set_margin_start(side_margin);
            block_box.set_margin_end(side_margin);
            block_box.set_margin_top(14);

            let (orig_view, trans_view): (gtk4::TextView, Option<gtk4::TextView>) =
                if let Some(speaker) = &block.speaker {
                    let header = Label::new(None);
                    header.set_halign(Align::Start);
                    header.set_markup(&format!(
                        "<span foreground='{}' font_variant='small-caps' font_weight='normal' size='{}pt'>{}</span>",
                        text_fg,
                        header_pt,
                        glib_escape(speaker),
                    ));
                    header.set_margin_bottom(4);
                    block_box.append(&header);

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
                    ensure_cursor_tag(&orig.buffer(), cursor_line_bg);
                    ensure_cursor_tag(&trans.buffer(), cursor_line_bg);

                    let divider = gtk4::Separator::new(Orientation::Vertical);
                    divider.add_css_class("column-divider");
                    divider.set_margin_start(12);
                    divider.set_margin_end(12);

                    cols.append(&orig);
                    cols.append(&divider);
                    cols.append(&trans);
                    block_box.append(&cols);
                    (orig, Some(trans))
                } else {
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
                    ensure_cursor_tag(&view.buffer(), cursor_line_bg);
                    block_box.append(&view);
                    (view, None)
                };

            self.content_vbox.append(&block_box);
            // Register both column views for the per-row bottom-clip mask.
            self.clip_views.borrow_mut().push(orig_view.clone());
            if let Some(t) = &trans_view {
                self.clip_views.borrow_mut().push(t.clone());
            }
            self.block_widgets.borrow_mut().push(BlockEntry {
                start_idx: block.start_idx,
                end_idx: block.end_idx,
                orig: orig_view,
                trans: trans_view,
            });
        }

        self.scrim.set_visible(true);
        self.container.set_visible(true);
        // on_open: snaps to top, pins across layout passes, fires idle backstop.
        self.clip_guard.on_open();
    }

    /// Scroll so the highlighted ORIGINAL line for `work_idx` is vertically
    /// centered in the viewport, matching the reading card's `center_cursor`
    /// convention (the line lands a quarter of a page down from the top, not
    /// dead-center). Clamps at the document edges so no blank space appears.
    /// No-op if the line isn't found.
    pub fn scroll_to_highlight(&self, work_idx: usize) {
        let (orig_view, off) = {
            let entries = self.block_widgets.borrow();
            let ranges: Vec<(usize, usize)> =
                entries.iter().map(|e| (e.start_idx, e.end_idx)).collect();
            let Some((bi, off)) = locate_line(&ranges, work_idx) else { return };
            (entries[bi].orig.clone(), off as i32)
        };

        let scrolled = self.scrolled.clone();
        let content = self.content_vbox.clone();
        // Defer one tick so allocations/wrapping are settled before measuring.
        glib::idle_add_local_once(move || {
            let Some(iter) = orig_view.buffer().iter_at_line(off) else { return };
            let (line_y, _line_h) = orig_view.line_yrange(&iter);
            // `line_yrange` gives BUFFER coords; `compute_point` wants
            // WIDGET-local coords. They're equal here only because each orig
            // view is unscrolled natural-height (no inner ScrolledWindow, height
            // request -1), so its internal scroll offset is always 0. If an orig
            // view ever gets height-constrained / independently scrolled, this
            // mapping must add that view's scroll offset.
            let pt = gtk4::graphene::Point::new(0.0, line_y as f32);
            let Some(mapped) = orig_view.compute_point(&content, &pt) else { return };
            let line_top = mapped.y() as f64;

            let adj = scrolled.vadjustment();
            let page = adj.page_size();
            let max = (adj.upper() - page).max(adj.lower());

            // Center the line vertically. Like the card's `center_cursor`, place
            // the line a quarter-page down from the top rather than dead-center.
            let new_value = line_top - page * 0.25;
            adj.set_value(new_value.clamp(adj.lower(), max));
        });
    }

    /// Highlight the cursor's source line `work_idx` in BOTH columns (style A):
    /// the original line on the left and its paired translation on the right.
    /// Clears any prior highlight first. No-op if the line is outside this scene.
    pub fn highlight_work_line(&self, work_idx: usize) {
        let entries = self.block_widgets.borrow();

        // Clear every buffer's existing highlight (small block count per scene).
        for e in entries.iter() {
            clear_cursor_tag(&e.orig.buffer());
            if let Some(t) = &e.trans {
                clear_cursor_tag(&t.buffer());
            }
        }

        let ranges: Vec<(usize, usize)> =
            entries.iter().map(|e| (e.start_idx, e.end_idx)).collect();
        let Some((bi, off)) = locate_line(&ranges, work_idx) else { return };
        let entry = &entries[bi];

        apply_cursor_tag(&entry.orig.buffer(), off as i32);
        if let Some(t) = &entry.trans {
            apply_cursor_tag(&t.buffer(), off as i32);
        }
    }
}

/// Ensure the buffer has a `cursor-line` tag painting the paragraph background
/// with the theme's cursor-line color. Idempotent (lookup before add).
fn ensure_cursor_tag(buffer: &gtk4::TextBuffer, cursor_line_bg: &str) {
    if buffer.tag_table().lookup("cursor-line").is_none() {
        let tag = gtk4::TextTag::builder()
            .name("cursor-line")
            .paragraph_background(cursor_line_bg)
            .build();
        buffer.tag_table().add(&tag);
    }
}

/// Remove the `cursor-line` tag from the whole buffer (if the tag exists).
fn clear_cursor_tag(buffer: &gtk4::TextBuffer) {
    if let Some(tag) = buffer.tag_table().lookup("cursor-line") {
        let (start, end) = buffer.bounds();
        buffer.remove_tag(&tag, &start, &end);
    }
}

/// Apply the `cursor-line` tag to buffer line `line` (0-based). No-op if the
/// line or tag is missing.
fn apply_cursor_tag(buffer: &gtk4::TextBuffer, line: i32) {
    let Some(tag) = buffer.tag_table().lookup("cursor-line") else { return };
    let Some(start) = buffer.iter_at_line(line) else { return };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.apply_tag(&tag, &start, &end);
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

/// Given each block's inclusive (start_idx, end_idx) work-line range in order,
/// return (block_index, line_offset) for the block containing `work_idx`.
fn locate_line(ranges: &[(usize, usize)], work_idx: usize) -> Option<(usize, usize)> {
    for (i, (start, end)) in ranges.iter().enumerate() {
        if work_idx >= *start && work_idx <= *end {
            return Some((i, work_idx - start));
        }
    }
    None
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
            sub_line: 0,
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

    #[test]
    fn locate_line_finds_block_and_offset() {
        // Two blocks: [10..=12] and [13..=13].
        let ranges = vec![(10usize, 12usize), (13, 13)];
        assert_eq!(locate_line(&ranges, 10), Some((0, 0)));
        assert_eq!(locate_line(&ranges, 12), Some((0, 2)));
        assert_eq!(locate_line(&ranges, 13), Some((1, 0)));
    }

    #[test]
    fn locate_line_returns_none_outside_any_block() {
        let ranges = vec![(10usize, 12usize)];
        assert_eq!(locate_line(&ranges, 9), None);
        assert_eq!(locate_line(&ranges, 13), None);
        assert_eq!(locate_line(&[], 0), None);
    }
}

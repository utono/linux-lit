use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;
use sourceview5::View;


/// Set up a gutter renderer that shows signs for timestamped lines.
///
/// The renderer is inserted into the left gutter. On each visible line,
/// `query-data` fires and we set text based on the `has_timestamp` vec
/// and the `visible` toggle. Chapter lines show ▸, manual lines show ─,
/// A/B points show ◐/◑, and other timestamped lines show •.
///
/// Returns the renderer so the caller can store it for later removal.
pub fn setup_timestamp_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    has_timestamp: Rc<RefCell<Vec<bool>>>,
    is_manual: Rc<RefCell<Vec<bool>>>,
    is_chapter: Rc<RefCell<Vec<bool>>>,
    is_bookmarked: Rc<RefCell<Vec<bool>>>,
    a_line: Rc<Cell<Option<usize>>>,
    b_line: Rc<Cell<Option<usize>>>,
    left_margin: i32,
    root_color: &str,
    position: i32,
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    // Consume most of the left margin so signs sit just left of speaker names.
    // The caller must reduce the text view's left_margin by GUTTER_WIDTH.
    let gutter_width = (left_margin - 20).max(10);
    renderer.set_xpad(0);
    renderer.set_xalign(1.0);
    renderer.set_yalign(0.5);
    renderer.set_size_request(gutter_width, -1);
    // Lower position = further from text (more outward) in the left gutter. The
    // sign column is normally outermost (0); when the left column also shows
    // line numbers, the numbers take position 0 and signs take 1 so the order
    // is [numbers | signs | text].
    gutter.insert(&renderer, position);

    let color = root_color.to_string();
    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        if !visible.get() {
            text_renderer.set_markup("");
            return;
        }
        let idx = line as usize;
        let ch = is_chapter.borrow();
        let is_ch = idx < ch.len() && ch[idx];
        let bm = is_bookmarked.borrow();
        let is_bm = idx < bm.len() && bm[idx];
        let ts = has_timestamp.borrow();
        let has_ts = idx < ts.len() && ts[idx];
        let manual = is_manual.borrow();
        let is_man = idx < manual.len() && manual[idx];
        let glyph = if is_bm {
            "\u{2605}" // ★
        } else if is_ch {
            "\u{25B8}" // ▸
        } else if has_ts {
            if a_line.get() == Some(idx) {
                "\u{25D0}" // ◐
            } else if b_line.get() == Some(idx) {
                "\u{25D1}" // ◑
            } else if is_man {
                "\u{2500}" // ─
            } else {
                "\u{2022}" // •
            }
        } else {
            text_renderer.set_markup("");
            return;
        };
        text_renderer.set_markup(&format!("<span foreground=\"{}\">{}</span>", color, glyph));
    });

    renderer
}

/// Remove an existing gutter renderer from the left gutter.
pub fn remove_gutter_renderer(view: &View, renderer: &sourceview5::GutterRendererText) {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    gutter.remove(renderer);
}

#[derive(Clone, Copy, PartialEq)]
enum ChunkPos {
    None,
    Start,
    Interior,
    End,
    Single,
}

/// Build a vec mapping each buffer line index to its chunk position.
fn build_chunk_positions(
    chunks: &[crate::db::models::Chunk],
    lines: &[crate::db::models::Line],
    line_map: Option<&crate::text_file_map::LineMap>,
) -> Vec<ChunkPos> {
    let buf_len = line_map.map_or(lines.len(), |lm| lm.buffer_to_work.len());
    let mut positions = vec![ChunkPos::None; buf_len];

    for chunk in chunks {
        let mut a_idx = None;
        let mut b_idx = None;
        for (i, line) in lines.iter().enumerate() {
            if line.div1 == chunk.div1
                && Some(line.div2) == chunk.div2
                && line.line_in_div == chunk.a_line
            {
                a_idx = Some(i);
            }
            if line.div1 == chunk.div1
                && Some(line.div2) == chunk.div2
                && line.line_in_div == chunk.b_line
            {
                b_idx = Some(i);
            }
        }

        // Translate work-line indices to buffer-line indices if line_map present
        if let Some(lm) = line_map {
            a_idx = a_idx.map(|i| lm.work_to_buffer[i]);
            b_idx = b_idx.map(|i| lm.work_to_buffer[i]);
        }

        if let (Some(a), Some(b)) = (a_idx, b_idx) {
            if a == b {
                positions[a] = ChunkPos::Single;
            } else {
                positions[a] = ChunkPos::Start;
                for i in (a + 1)..b {
                    positions[i] = ChunkPos::Interior;
                }
                positions[b] = ChunkPos::End;
            }
        }
    }

    positions
}

/// Set up a gutter renderer showing chunk boundary bars.
///
/// Inserts at position -1 (before the timestamp renderer). Returns the
/// renderer so the caller can store it for later removal.
pub fn setup_chunk_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    chunks: &[crate::db::models::Chunk],
    lines: &[crate::db::models::Line],
    line_map: Option<&crate::text_file_map::LineMap>,
) -> sourceview5::GutterRendererText {
    let positions = build_chunk_positions(chunks, lines, line_map);
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(2);
    gutter.insert(&renderer, -1);

    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        if !visible.get() {
            text_renderer.set_text("");
            return;
        }
        let idx = line as usize;
        let ch = if idx < positions.len() {
            match positions[idx] {
                ChunkPos::Start => "\u{2577}",    // ╷
                ChunkPos::Interior => "\u{2502}", // │
                ChunkPos::End => "\u{2575}",      // ╵
                ChunkPos::Single => "\u{2502}",   // │
                ChunkPos::None => "",
            }
        } else {
            ""
        };
        text_renderer.set_text(ch);
    });

    renderer
}

/// Width of the line-number renderer and its trailing margin in the default
/// (single-column / monocle) layout.
pub const LINE_NUMBER_WIDTH: i32 = 40;
pub const LINE_NUMBER_MARGIN_END: i32 = 48;

/// Tighter line-number geometry for two-column mode: the numbers sit closer to
/// the text so each narrow column keeps more room for the verse line itself
/// (the widest real Shakespeare dialogue line is ~63 chars and must not wrap).
/// `MARGIN_END_TWO_COL` is the padding between the number and the card/column
/// edge; the gap between the dialogue text and the number is the view's right
/// margin, set separately (smaller) so the number reads as part of that line.
pub const LINE_NUMBER_WIDTH_TWO_COL: i32 = 32;
pub const LINE_NUMBER_MARGIN_END_TWO_COL: i32 = 10;
/// Gap between the last char of a dialogue line and its line number in
/// two-column mode (the text view's right margin when the number gutter is on).
pub const LINE_NUMBER_TEXT_GAP_TWO_COL: i32 = 2;

/// Gap between the left column's outer line numbers and the sign column to
/// their right, in two-column mode (the left number renderer's margin_end).
pub const LINE_NUMBER_LEFT_GAP_TWO_COL: i32 = 8;

pub fn setup_line_number_gutter(
    view: &View,
    line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
    dim_color: &str,
    font_family: &str,
    font_size_pt: u32,
    width: i32,
    margin_end: i32,
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Right);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(0);
    renderer.set_xalign(0.0);
    renderer.set_yalign(0.5);
    renderer.set_size_request(width, -1);
    renderer.set_margin_end(margin_end);
    gutter.insert(&renderer, 0);

    let color = dim_color.to_string();
    let face = font_family.to_string();
    let pango_size = (font_size_pt as f32 * 0.7) as u32 * 1024;
    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        let idx = line as usize;
        let nums = line_numbers.borrow();
        let show = idx < nums.len()
            && nums[idx].is_some_and(|n| n % 5 == 0);
        if show {
            let n = nums[idx].unwrap();
            text_renderer.set_markup(&format!(
                "<span foreground=\"{}\" face=\"{}\" size=\"{}\">{}</span>",
                color, face, pango_size, n,
            ));
        } else {
            text_renderer.set_markup("");
        }
    });

    renderer
}

pub fn remove_line_number_renderer(view: &View, renderer: &sourceview5::GutterRendererText) {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Right);
    gutter.remove(renderer);
}

/// Like `setup_line_number_gutter`, but inserts the numbers into the LEFT
/// gutter, right-aligned and at the outermost position (left of the sign
/// column). Used for the left column in two-column mode so the line numbers sit
/// on the card's outer edge — like a book's facing-page foliation — and the
/// verse text shifts inward toward the divider.
///
/// `margin_end` is the gap between the numbers and the next gutter element (the
/// sign column / text). The caller must enlarge the view's `left_margin` by
/// `width + margin_end` so the numbers have room without pushing the text right.
pub fn setup_line_number_gutter_left(
    view: &View,
    line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
    dim_color: &str,
    font_family: &str,
    font_size_pt: u32,
    width: i32,
    margin_end: i32,
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(0);
    renderer.set_xalign(1.0);
    renderer.set_yalign(0.5);
    renderer.set_size_request(width, -1);
    renderer.set_margin_end(margin_end);
    // Position 0 = outermost (leftmost). The sign column inserts at 0 later, so
    // inserting here at 0 keeps numbers outside it as long as we set up numbers
    // first; if signs already exist they were inserted at 0 and this pushes them
    // right — re-inserting numbers at 0 still lands them leftmost.
    gutter.insert(&renderer, 0);

    let color = dim_color.to_string();
    let face = font_family.to_string();
    let pango_size = (font_size_pt as f32 * 0.7) as u32 * 1024;
    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        let idx = line as usize;
        let nums = line_numbers.borrow();
        let show = idx < nums.len() && nums[idx].is_some_and(|n| n % 5 == 0);
        if show {
            let n = nums[idx].unwrap();
            text_renderer.set_markup(&format!(
                "<span foreground=\"{}\" face=\"{}\" size=\"{}\">{}</span>",
                color, face, pango_size, n,
            ));
        } else {
            text_renderer.set_markup("");
        }
    });

    renderer
}

/// Remove a line-number renderer that was inserted into the LEFT gutter.
pub fn remove_line_number_renderer_left(view: &View, renderer: &sourceview5::GutterRendererText) {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    gutter.remove(renderer);
}

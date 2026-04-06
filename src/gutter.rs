use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;
use sourceview5::View;


/// Set up a gutter renderer that shows signs for timestamped lines.
///
/// The renderer is inserted into the left gutter. On each visible line,
/// `query-data` fires and we set text based on the `has_timestamp` vec
/// and the `visible` toggle. Chapter lines show a filled diamond (◆).
///
/// Returns the renderer so the caller can store it for later removal.
pub fn setup_timestamp_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    has_timestamp: Rc<RefCell<Vec<bool>>>,
    is_chapter: Rc<RefCell<Vec<bool>>>,
    a_line: Rc<Cell<Option<usize>>>,
    b_line: Rc<Cell<Option<usize>>>,
    left_margin: i32,
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
    gutter.insert(&renderer, 0);

    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        if !visible.get() {
            text_renderer.set_text("");
            return;
        }
        let idx = line as usize;
        let ch = is_chapter.borrow();
        let is_ch = idx < ch.len() && ch[idx];
        let ts = has_timestamp.borrow();
        if is_ch {
            text_renderer.set_text("\u{25A0}"); // ■
        } else if idx < ts.len() && ts[idx] {
            if a_line.get() == Some(idx) {
                text_renderer.set_text("\u{25D0}"); // ◐
            } else if b_line.get() == Some(idx) {
                text_renderer.set_text("\u{25D1}"); // ◑
            } else {
                text_renderer.set_text("\u{2022}"); // •
            }
        } else {
            text_renderer.set_text("");
        }
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

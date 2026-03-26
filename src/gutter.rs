use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;
use sourceview5::{Buffer as SrcBuffer, View};

const MARK_TIMESTAMP: &str = "timestamp";

/// Place sourceview5::Mark on each timestamped line.
pub fn place_timestamp_marks(buffer: &SrcBuffer, has_timestamp: &[bool]) {
    // Clear old timestamp marks
    let start = buffer.start_iter();
    let end = buffer.end_iter();
    buffer.remove_source_marks(&start, &end, Some(MARK_TIMESTAMP));

    for (i, &has_ts) in has_timestamp.iter().enumerate() {
        if !has_ts {
            continue;
        }
        if let Some(iter) = buffer.iter_at_line(i as i32) {
            let mark_name = format!("ts-{}", i);
            buffer.create_source_mark(Some(&mark_name), MARK_TIMESTAMP, &iter);
        }
    }
}

/// Set up a gutter renderer that shows "│" for timestamped lines.
///
/// The renderer is inserted into the left gutter. On each visible line,
/// `query-data` fires and we set text based on the `has_timestamp` vec
/// and the `visible` toggle.
///
/// Returns the renderer so the caller can store it for later removal.
pub fn setup_timestamp_gutter(
    view: &View,
    visible: Rc<Cell<bool>>,
    has_timestamp: Vec<bool>,
    a_line: Rc<Cell<Option<usize>>>,
    b_line: Rc<Cell<Option<usize>>>,
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(24); // left padding matching original gutter
    renderer.set_yalign(0.5); // center vertically within line
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
        if idx < has_timestamp.len() && has_timestamp[idx] {
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
) -> Vec<ChunkPos> {
    let mut positions = vec![ChunkPos::None; lines.len()];

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
) -> sourceview5::GutterRendererText {
    let positions = build_chunk_positions(chunks, lines);
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

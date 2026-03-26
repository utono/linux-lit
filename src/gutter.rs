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
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Left);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(2);
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
            text_renderer.set_text("\u{2502}"); // │
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

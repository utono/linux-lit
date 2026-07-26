//! The underline entry point for a syntax gloss.
//!
//! The request-building, dispatch and rendering all live in
//! `crate::input::visual::action_syntax_gloss` now — a syntax gloss is an
//! ordinary gloss, so it goes through the same context/persist/render path as
//! every other gloss type. What remains here is the one thing that path does
//! NOT do: resolve the words underlined with `-`/`_` to the sentence that
//! contains them, and from there to the work lines that sentence sits in.

use crate::app::AppState;
use gtk4::prelude::TextBufferExt;
use std::cell::RefCell;
use std::rc::Rc;

/// Build a syntax gloss for the sentence containing the currently underlined
/// words (`-` / `_` then `Return`).
///
/// Window: the cursor's buffer line plus one either side. That covers a
/// sentence spanning a verse break without risking a whole-chapter scan.
///
/// The window is joined from BUFFER text, not `work.lines[..].text`, because
/// `collect_ranges` are char offsets into the BUFFER line (see
/// `extract_buffer_line_words`). Phase B's inline italics delete `_`
/// delimiters from the buffer, so on a work with italics the buffer and DB
/// strings differ in length and the offsets do not transfer.
///
/// The sentence span is used only to CONFIRM there is a sentence to analyze
/// and to log it. The gloss itself is keyed to the window's work lines: a
/// gloss is stored against citations, and a citation names a whole line, so a
/// sub-line span has nothing to key by. Selecting the lines the sentence sits
/// in is the same passage granularity the visual-mode action uses.
pub fn syntax_gloss_for_underlined(state_rc: &Rc<RefCell<AppState>>) {
    let selected_lines: Vec<crate::db::models::Line> = {
        let state = state_rc.borrow();

        let ranges: Vec<(usize, usize)> =
            crate::input::actions::word_copy::active_underline(&state).to_vec();
        if ranges.is_empty() {
            return;
        }

        let cursor = state.current_line;
        let last_line = (state.buffer.line_count().max(1) as usize) - 1;
        let first = cursor.saturating_sub(1);
        let last = (cursor + 1).min(last_line);

        // Join the window from buffer text, recording where the cursor's own
        // line starts so the underline offsets can be rebased into it.
        let mut window = String::new();
        let mut cursor_line_offset = 0usize;
        for bl in first..=last {
            if bl == cursor {
                cursor_line_offset = window.chars().count();
            }
            let start = match state.buffer.iter_at_line(bl as i32) {
                Some(it) => it,
                None => continue,
            };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            window.push_str(state.buffer.text(&start, &end, false).as_str());
            if bl != last {
                window.push('\n');
            }
        }

        let rebased: Vec<(usize, usize)> = ranges
            .iter()
            .map(|&(s, e)| (s + cursor_line_offset, e + cursor_line_offset))
            .collect();

        let span = match crate::input::sentence::sentence_span(&window, &rebased) {
            Some(sp) => sp,
            None => return,
        };

        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let lines: Vec<crate::db::models::Line> = (first..=last)
            .filter_map(|bl| {
                state
                    .work_line_for_buffer(bl)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        crate::logging::log(&format!(
            "SYNTAX_UNDERLINE: {} range(s) -> span {}..{} over {} line(s)",
            ranges.len(),
            span.0,
            span.1,
            lines.len()
        ));
        lines
    };

    crate::input::visual::syntax_gloss_for_lines(state_rc, selected_lines);
}

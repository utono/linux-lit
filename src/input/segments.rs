//! Cursor-segment context for the chat panel: the cursor's paragraph/speech
//! plus up to n neighbor segments on each side, truncated at section
//! (chapter/scene) starts and buffer edges.

use crate::app::AppState;

#[derive(Clone)]
pub(crate) struct SegmentContext {
    /// Segment texts in buffer order (cursor's segment included).
    pub segments: Vec<String>,
    /// Index of the cursor's segment within `segments`.
    pub cursor_index: usize,
    /// The work lines of the CURSOR block only — used at save time for
    /// citations (`build_context_for_type`) and `<speaker>/<verse>` markup
    /// (`build_source_header`).
    pub cursor_lines: Vec<crate::db::models::Line>,
    pub div1: i64,
    pub div2: i64,
}

/// Collect up to `n` neighbor blocks on each side of `cursor_block` (pure).
/// `block_at(line)` returns the block containing `line` (None on boundary
/// lines). Walking upward stops at the buffer edge or once the current block
/// STARTS a section (can't cross further up); walking downward stops before a
/// block whose first line starts a section (don't cross into the next one).
/// Returns all blocks in buffer order, cursor block included.
pub(crate) fn collect_neighbor_blocks(
    line_count: usize,
    cursor_block: (usize, usize),
    n: usize,
    block_at: impl Fn(usize) -> Option<(usize, usize)>,
    is_section_start: impl Fn(usize) -> bool,
) -> Vec<(usize, usize)> {
    let mut blocks = vec![cursor_block];
    // Upward.
    let mut added = 0;
    let mut cur = cursor_block;
    while added < n && cur.0 > 0 && !is_section_start(cur.0) {
        let mut l = cur.0 - 1;
        let prev = loop {
            if let Some(b) = block_at(l) {
                break Some(b);
            }
            if l == 0 {
                break None;
            }
            l -= 1;
        };
        match prev {
            Some(b) => {
                blocks.insert(0, b);
                cur = b;
                added += 1;
            }
            None => break,
        }
    }
    // Downward.
    let mut added = 0;
    let mut cur = cursor_block;
    while added < n && cur.1 + 1 < line_count {
        let mut l = cur.1 + 1;
        let next = loop {
            if let Some(b) = block_at(l) {
                break Some(b);
            }
            l += 1;
            if l >= line_count {
                break None;
            }
        };
        match next {
            Some(b) if !is_section_start(b.0) => {
                blocks.push(b);
                cur = b;
                added += 1;
            }
            _ => break,
        }
    }
    blocks
}

/// Assemble the chat user message: work header, the consecutive segments with
/// the cursor's segment marked, and the reader's question (pure).
pub(crate) fn chat_user_message(
    genre: &str,
    title: &str,
    author: &str,
    unit_label: &str,
    scene_label: &str,
    segments: &[String],
    cursor_index: usize,
    question: &str,
) -> String {
    let mut ctx = String::new();
    for (i, seg) in segments.iter().enumerate() {
        if i == cursor_index {
            ctx.push_str("[READER'S CURSOR SEGMENT]\n");
        }
        ctx.push_str(seg);
        ctx.push_str("\n\n");
    }
    format!(
        "Work type: {}\nWork: {} by {}\n{}: {}\n\nContext (consecutive segments; the reader's cursor segment is marked):\n{}Reader's question:\n{}",
        genre, title, author, unit_label, scene_label, ctx, question,
    )
}

/// A context holding ONLY the buffer lines `[start, end]` as a single segment —
/// the reader's visual selection, verbatim. Used by the chat panel's pinned
/// passage (`Tab` from V-mode): the chat then sends exactly what was
/// highlighted, with NO neighbor segments, for every question in the session.
///
/// Shaped as a one-segment `SegmentContext` (`cursor_index = 0`) so the whole
/// downstream path — `chat_user_message`, the source header, the citations —
/// works unchanged; it simply has nothing to add around the passage.
/// None when there is no work or the range maps to no work lines.
pub(crate) fn selection_context(
    state: &AppState,
    start: usize,
    end: usize,
) -> Option<SegmentContext> {
    let work = state.current_work.as_ref()?;
    let text = (start..=end)
        .map(|l| crate::input::viewport::buffer_line_text(&state.buffer, l))
        .collect::<Vec<_>>()
        .join("\n");
    let cursor_lines: Vec<crate::db::models::Line> = (start..=end)
        .filter_map(|bl| {
            state
                .work_line_for_buffer(bl)
                .and_then(|wi| work.lines.get(wi).cloned())
        })
        .collect();
    if cursor_lines.is_empty() {
        return None;
    }
    let (div1, div2) = cursor_lines
        .first()
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    Some(SegmentContext { segments: vec![text], cursor_index: 0, cursor_lines, div1, div2 })
}

/// The cursor's segment ±`n` neighbors, resolved against the live buffer.
/// None when there is no work or the cursor sits on a boundary/unmapped line.
pub(crate) fn segment_context(state: &AppState, n: usize) -> Option<SegmentContext> {
    let cursor_block = crate::input::visual::cursor_block_bounds(state)?;
    let line_count = state.effective_line_count();
    let blocks = collect_neighbor_blocks(
        line_count,
        cursor_block,
        n,
        |l| crate::input::visual::block_bounds_at(state, l),
        |l| state.is_section_start(l),
    );
    let cursor_index = blocks.iter().position(|b| *b == cursor_block).unwrap_or(0);
    let segments: Vec<String> = blocks
        .iter()
        .map(|&(s, e)| {
            (s..=e)
                .map(|l| crate::input::viewport::buffer_line_text(&state.buffer, l))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    let work = state.current_work.as_ref()?;
    let cursor_lines: Vec<crate::db::models::Line> = (cursor_block.0..=cursor_block.1)
        .filter_map(|bl| {
            state
                .work_line_for_buffer(bl)
                .and_then(|wi| work.lines.get(wi).cloned())
        })
        .collect();
    let (div1, div2) = cursor_lines
        .first()
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    Some(SegmentContext { segments, cursor_index, cursor_lines, div1, div2 })
}

#[cfg(test)]
mod tests {
    use super::{chat_user_message, collect_neighbor_blocks};

    /// Blocks laid out as [0..1] [3..4] [6..7] [9..10] [12..13] with blank
    /// boundary lines between; section starts at the given lines.
    fn harness(section_starts: &[usize]) -> (usize, impl Fn(usize) -> Option<(usize, usize)> + '_, impl Fn(usize) -> bool + '_) {
        let blocks = [(0usize, 1usize), (3, 4), (6, 7), (9, 10), (12, 13)];
        let block_at = move |l: usize| blocks.iter().copied().find(|&(s, e)| l >= s && l <= e);
        let is_start = move |l: usize| section_starts.contains(&l);
        (14, block_at, is_start)
    }

    #[test]
    fn collects_n_neighbors_each_side() {
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (6, 7), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn truncates_at_buffer_edges() {
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (0, 1), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7)]);
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (12, 13), 2, block_at, is_start);
        assert_eq!(got, vec![(6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn does_not_cross_section_start_downward() {
        // Block (9..10) starts a new section: walking down from (3..4) may
        // include (6..7) but must stop before (9..10).
        let (count, block_at, is_start) = harness(&[9]);
        let got = collect_neighbor_blocks(count, (3, 4), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7)]);
    }

    #[test]
    fn does_not_cross_section_start_upward() {
        // Block (6..7) starts a section: walking up from (6..7) stops
        // immediately (its own start is a section start).
        let (count, block_at, is_start) = harness(&[6]);
        let got = collect_neighbor_blocks(count, (6, 7), 2, block_at, is_start);
        assert_eq!(got, vec![(6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn user_message_marks_cursor_segment() {
        let segs = vec!["before".to_string(), "here".to_string(), "after".to_string()];
        let msg = chat_user_message(
            "novel", "Bleak House", "Charles Dickens", "Chapter", "Chapter 7",
            &segs, 1, "Why the fog?",
        );
        assert!(msg.contains("Work type: novel"));
        assert!(msg.contains("Chapter: Chapter 7"));
        assert!(msg.contains("[READER'S CURSOR SEGMENT]\nhere"));
        assert!(!msg.contains("[READER'S CURSOR SEGMENT]\nbefore"));
        assert!(msg.trim_end().ends_with("Reader's question:\nWhy the fog?"));
    }
}

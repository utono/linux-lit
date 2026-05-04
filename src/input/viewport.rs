use gtk4::prelude::*;

use crate::app::AppState;
use super::scroll::BASE_BOTTOM_MARGIN;

// ---------------------------------------------------------------------------
// VisibleRange — canonical "what's on screen" measurement
// ---------------------------------------------------------------------------

/// Result of a single height-summing walk over the buffer starting at a page
/// top: which line was the last to fully fit, the total pixel height consumed
/// by lines [page_top, last_fit], and the count of lines included.
///
/// Mirrors what foliate-js `getVisibleRange` returns (paginator.js:94-151) —
/// a single source of truth for "what's on screen right now" that all four
/// previous callers (`last_fully_visible_line`, `is_line_fully_visible`,
/// `update_bottom_clip`, `lines_per_page`) project from.
///
/// Convention: when the buffer is empty or no line fits, `count == 0` and
/// `total_height == 0`. `last_fit` is then equal to `page_top` but should be
/// treated as meaningless by callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisibleRange {
    pub(crate) last_fit: usize,
    pub(crate) total_height: i32,
    pub(crate) count: usize,
}

/// Walk the buffer from `page_top`, summing line heights against the viewport's
/// `usable_height` (caller computes that from `widget_height - descender_guard
/// - bottom_margin`). Returns the largest range that fully fits.
///
/// Caller-specific short-circuits (`loading_work`, `widget_height <= 0`, empty
/// buffer) stay in the callers — they're not part of this kernel.
///
/// Pure: given a sorted vec of viewport-page top line indices, return the
/// 1-indexed page that contains `target_line`. Empty index returns 1.
/// `target_line` past the last page-top returns `tops.len()`.
///
/// Mirrors what foliate-js paginator.js does in O(log n) via index lookup
/// (`atStart`/`atEnd` use page indices, paginator.js:1050-1054), replacing
/// linux-lit's previous O(n²) replay-from-line-0 walk.
pub(crate) fn page_for_line_in_index(tops: &[usize], target_line: usize) -> usize {
    if tops.is_empty() {
        return 1;
    }
    // partition_point returns the index of the first element > target_line.
    // Because tops[0] is always 0 (the first page), partition_point >= 1 for
    // any target_line >= 0 — that's exactly the page number we want.
    tops.partition_point(|&t| t <= target_line).max(1)
}

/// Mirrors `getVisibleRange` in foliate-js paginator.js (lines 94-151) in
/// purpose: one canonical visibility computation. Future foliate reads of that
/// function map directly to this one.
pub(crate) fn visible_range(
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    page_top: usize,
    line_count: usize,
    usable_height: i32,
) -> VisibleRange {
    let mut total_height: i32 = 0;
    let mut count: usize = 0;
    let mut last_fit: usize = page_top;
    for i in page_top..line_count {
        let Some(iter) = buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = text_view.line_yrange(&iter);
        if total_height + h > usable_height {
            break;
        }
        total_height += h;
        last_fit = i;
        count += 1;
    }
    VisibleRange { last_fit, total_height, count }
}

// ---------------------------------------------------------------------------
// Trim helpers — pure + GTK-bound
// ---------------------------------------------------------------------------

/// Trim trailing dangling-context lines (speakers, blanks, stage directions)
/// from a `VisibleRange` so the page never ends on a line that exists only
/// to introduce or annotate the dialogue that should follow it. Pure variant
/// separated for unit testability — the GTK-bound `trim_trailing_speakers`
/// wraps this with line_types and line_yrange calls.
///
/// Stops at `page_top` so the trim never deletes the page top itself.
pub(crate) fn trim_trailing_speakers_pure<F, H>(
    mut range: VisibleRange,
    page_top: usize,
    is_dangling_context: F,
    line_height: H,
) -> VisibleRange
where
    F: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    while range.last_fit > page_top && is_dangling_context(range.last_fit) {
        range.total_height -= line_height(range.last_fit);
        range.last_fit -= 1;
        range.count = range.count.saturating_sub(1);
    }
    range
}

/// GTK-bound wrapper for `trim_trailing_speakers_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`; reads heights via
/// `text_view.line_yrange`. Stage directions are treated as dangling-context
/// just like speakers — neither should be the last line of a page (they
/// preface the dialogue that follows, not summarize what came before).
pub(crate) fn trim_trailing_speakers(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    use crate::db::line_types;
    let is_dangling_context = |i: usize| -> bool {
        let text = {
            let Some(start) = buffer.iter_at_line(i as i32) else { return false };
            let mut end = start;
            if !end.ends_line() { end.forward_to_line_end(); }
            buffer.text(&start, &end, false).to_string()
        };
        line_types::is_speaker(&text)
            || line_types::is_blank(&text)
            || line_types::is_stage_direction(&text)
    };
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_trailing_speakers_pure(range, page_top, is_dangling_context, line_height)
}

/// Pure: given closures that classify line kinds, find the start of the
/// "block" containing `last_fit`. A block is a multi-line stage-direction
/// run (any work) or a multi-line verse stanza (non-prose works only).
///
/// Returns `last_fit` unchanged when `last_fit` is not in a recognized block,
/// or when the block is just one line (no backup needed).
///
/// Stops at `page_top` so the trim never deletes the page top itself.
///
/// Mirrors foliate-js's per-element visibility rule (paginator.js:104-106) —
/// block atomicity instead of per-line visibility.
pub(crate) fn block_start_for_line_pure<B, S, D, L>(
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
) -> usize
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
{
    // Stage takes precedence: a stage-direction line inside a non-prose
    // work is a stage block, never a verse line.
    if is_stage(last_fit) {
        let mut start = last_fit;
        while start > page_top && is_stage(start - 1) {
            start -= 1;
        }
        // Single-line "block" — not multi-line, no backup needed.
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    // Verse stanza: only in non-prose works, only on dialogue lines.
    if !is_prose && is_dialogue(last_fit) {
        let mut start = last_fit;
        while start > page_top {
            let prev = start - 1;
            if is_blank(prev) || is_speaker(prev) || is_stage(prev) {
                break;
            }
            start -= 1;
        }
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    last_fit
}

/// Pure: trim a `VisibleRange` so the last fitting line isn't mid-block.
/// Backs `last_fit` up to the line before block-start when the block fully
/// fits; returns the range unchanged when the block doesn't fit at all
/// (`block_start <= page_top` — overflow fallback policy from F9 spec).
///
/// `line_height` closure provides per-line heights for `total_height` accounting.
pub(crate) fn trim_block_atoms_pure<B, S, D, L, H>(
    range: VisibleRange,
    page_top: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
    line_height: &H,
) -> VisibleRange
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
{
    if range.count == 0 || range.last_fit == page_top {
        return range;
    }
    let block_start = block_start_for_line_pure(
        page_top, range.last_fit, is_prose,
        is_blank, is_speaker, is_stage, is_dialogue,
    );
    if block_start == range.last_fit {
        return range; // not in a block
    }
    if block_start <= page_top {
        return range; // overflow: block extends to (or past) page_top
    }
    // Only trim if the block actually continues past last_fit (i.e., we're
    // splitting it mid-block). If the next line is OUTSIDE the same block
    // (a blank, speaker, stage transition, or end of buffer), the block
    // ends at last_fit and there's nothing to atomicize — leave as-is.
    let continues = if is_stage(range.last_fit) {
        // Stage block: continues iff next line is also stage.
        is_stage(range.last_fit + 1)
    } else if !is_prose && is_dialogue(range.last_fit) {
        // Verse stanza: continues iff next line is dialogue (not blank/
        // speaker/stage which would close the stanza).
        let next = range.last_fit + 1;
        !is_blank(next) && !is_speaker(next) && !is_stage(next) && is_dialogue(next)
    } else {
        false
    };
    if !continues {
        return range; // block ends here; nothing to atomicize
    }
    // Drop lines [block_start, range.last_fit] from the range.
    let mut new_total_height = range.total_height;
    for i in block_start..=range.last_fit {
        new_total_height -= line_height(i);
    }
    let new_last_fit = block_start - 1;
    let new_count = new_last_fit - page_top + 1;
    // Overflow guard: if trimming would leave half or fewer lines visible,
    // the block is too tall to fit on one page (e.g., a verse stanza longer
    // than the viewport sitting just below a speaker). Keep the per-line
    // split — better to show a useful page that splits mid-block than a
    // useless 1-line page that just shows the speaker. This is the F9
    // best-effort policy from the spec ("atomic if it fits, split if not").
    if new_count * 2 <= range.count {
        return range;
    }
    VisibleRange {
        last_fit: new_last_fit,
        total_height: new_total_height,
        count: new_count,
    }
}

/// GTK-bound wrapper for `block_start_for_line_pure`. Reads line text via
/// `buffer` and classifies via `crate::db::line_types`.
///
/// Most callers want trim_visible_range instead.
#[allow(dead_code)]
pub(crate) fn block_start_for_line(
    buffer: &sourceview5::Buffer,
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
) -> usize {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    block_start_for_line_pure(page_top, last_fit, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue)
}

/// GTK-bound wrapper for `trim_block_atoms_pure`. Reads line text and heights
/// from `text_view`/`buffer`. `is_prose` is the work-type flag (true for novel/
/// essay/etc., false for plays and poetry).
///
/// Most callers want trim_visible_range instead.
pub(crate) fn trim_block_atoms(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    use crate::db::line_types;
    let line_text = |i: usize| -> String {
        let Some(start) = buffer.iter_at_line(i as i32) else { return String::new() };
        let mut end = start;
        if !end.ends_line() { end.forward_to_line_end(); }
        buffer.text(&start, &end, false).to_string()
    };
    let is_blank = |i: usize| line_types::is_blank(&line_text(i));
    let is_speaker = |i: usize| line_types::is_speaker(&line_text(i));
    let is_stage = |i: usize| line_types::is_stage_direction(&line_text(i));
    let is_dialogue = |i: usize| line_types::is_dialogue(&line_text(i), is_prose);
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_block_atoms_pure(range, page_top, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height)
}

/// Scan (page_top, last_fit] for a line that starts a new chapter or scene
/// (detected from buffer text via `is_act_scene_marker` or `is_separator`).
/// If found, clamp last_fit to the line before it so the new section starts
/// on the next page. Uses text-based detection rather than DB flags so it
/// works uniformly for prose, verse, and plays.
fn clamp_at_section_break(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    use crate::db::line_types;
    if range.count <= 1 {
        return range;
    }
    // Scan for the first section break strictly after page_top.
    let mut break_line = None;
    for i in (page_top + 1)..=range.last_fit {
        let text = buffer_line_text(buffer, i);
        let trimmed = text.trim();
        if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
            break_line = Some(i);
            break;
        }
    }
    let break_line = match break_line {
        Some(b) => b,
        None => return range,
    };
    let clamped_last = break_line.saturating_sub(1);
    if clamped_last < page_top {
        return range;
    }
    // No minimum-content guard here: even a page with just 3 lines before
    // a scene break is correct — it's the end of the previous scene. The
    // `clamped_last < page_top` check above already prevents truly empty
    // pages (break at page_top + 1 with page_top being blank).
    // Recompute total_height by walking line heights up to clamped_last.
    let mut total = 0i32;
    for i in page_top..=clamped_last {
        if let Some(iter) = buffer.iter_at_line(i as i32) {
            let (_y, h) = text_view.line_yrange(&iter);
            total += h;
        }
    }
    VisibleRange {
        last_fit: clamped_last,
        total_height: total,
        count: clamped_last - page_top + 1,
    }
}

/// Canonical composition: apply section-break clamping, then
/// `trim_trailing_speakers`, then `trim_block_atoms`,
/// then `trim_trailing_speakers` again on the result. All callers that compute a
/// visible range for "what's on this page" should go through this wrapper so all
/// trims fire in the right order.
///
/// Order matters:
/// 0. Section-break clamp first — ensures a new chapter/scene starts at the top
///    of the next page rather than appearing partway down the current page.
/// 1. Speaker trim first removes a dangling speaker at the bottom.
/// 2. Block trim sees the new `last_fit` and decides whether THAT line is
///    mid-block; if so, backs up to the line before block-start.
/// 3. Speaker trim AGAIN — block trim usually leaves `last_fit` on a speaker
///    (the line that introduced the now-removed block), which is itself
///    dangling-context. Without this second pass, is_line_fully_visible would
///    treat a fully-visible dialogue line as off-page just because the trim
///    chain reported `last_fit` on the speaker above it.
pub(crate) fn trim_visible_range(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
) -> VisibleRange {
    // Pre-pass: clamp at the first section break (act/scene marker or
    // separator) strictly inside the range so chapters/scenes always start
    // at the top of the next page.
    let r = clamp_at_section_break(range, page_top, text_view, buffer);
    let r = trim_trailing_speakers(r, page_top, text_view, buffer);
    let r = trim_block_atoms(r, page_top, text_view, buffer, is_prose);
    trim_trailing_speakers(r, page_top, text_view, buffer)
}

// ---------------------------------------------------------------------------
// Line-level helpers
// ---------------------------------------------------------------------------

/// Get the text content of a buffer line.
pub(crate) fn buffer_line_text(buffer: &sourceview5::Buffer, line: usize) -> String {
    let Some(start) = buffer.iter_at_line(line as i32) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}

/// Check if a buffer line is blank (empty or whitespace only).
pub(crate) fn is_blank_buffer_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    let text = buffer_line_text(buffer, line);
    text.trim().is_empty()
}

/// Check if a buffer line is a dialogue line (not blank, speaker, stage direction, or marker).
pub(crate) fn is_dialogue_line(buffer: &sourceview5::Buffer, line: usize) -> bool {
    use crate::db::line_types;
    let text = buffer_line_text(buffer, line);
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !line_types::is_speaker(trimmed)
        && !line_types::is_stage_direction(trimmed)
        && !line_types::is_act_scene_marker(trimmed)
        && !line_types::is_separator(trimmed)
}

/// Find the next dialogue line after `current`. Skips translation lines.
pub(crate) fn next_dialogue_line(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    current: usize,
    line_count: usize,
) -> Option<usize> {
    let mut i = current + 1;
    while i < line_count {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i)
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the previous dialogue line before `current`. Skips translation lines.
pub(crate) fn prev_dialogue_line(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    current: usize,
) -> Option<usize> {
    if current == 0 {
        return None;
    }
    let mut i = current - 1;
    loop {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i)
        {
            return Some(i);
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// Find the next dialogue line at or after `from`.
pub(crate) fn next_dialogue_from(buffer: &sourceview5::Buffer, from: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    for i in from..line_count {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            return i;
        }
    }
    from
}

/// Find the last dialogue line in the range [from, from+count).
pub(crate) fn last_dialogue_in_page(buffer: &sourceview5::Buffer, from: usize, count: usize, line_count: usize) -> usize {
    use crate::db::line_types;
    let end = (from + count).min(line_count);
    let mut last = from;
    for i in from..end {
        let text = buffer_line_text(buffer, i);
        if !line_types::is_blank(&text) && line_types::is_dialogue(&text, false) {
            last = i;
        }
    }
    last
}

/// Given a dialogue line that will be the top of a page, back up over any
/// non-dialogue preamble immediately preceding it: blanks, speakers, stage
/// directions. Stops at (and includes) the first act/scene marker or
/// separator — those are section boundaries and should be the page top,
/// not backed past into the previous section's content.
pub(crate) fn back_up_for_speaker(buffer: &sourceview5::Buffer, line: usize) -> usize {
    use crate::db::line_types;
    let mut top = line;
    while top > 0 {
        let prev = buffer_line_text(buffer, top - 1);
        let trimmed = prev.trim();
        if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
            top -= 1;
            break;
        }
        if trimmed.is_empty()
            || line_types::is_speaker(trimmed)
            || line_types::is_stage_direction(trimmed)
        {
            top -= 1;
        } else {
            break;
        }
    }
    top
}

/// Find the best page-top for a forward page turn targeting `target_line`.
/// Backs up over any non-dialogue content (blanks, speakers, stage directions,
/// scene markers) immediately preceding the target so transition context is
/// visible at the top of the new page rather than dropped between pages.
pub(crate) fn page_turn_top(buffer: &sourceview5::Buffer, target_line: usize) -> usize {
    back_up_for_speaker(buffer, target_line)
}

/// Find the page-top for a chapter jump: walk backward from `target_line` to
/// the nearest act/scene marker so the viewport starts on the scene title.
/// Falls back to `page_turn_top` if no marker is found within `MAX_LOOKBACK`.
pub(crate) fn chapter_page_top(buffer: &sourceview5::Buffer, target_line: usize) -> usize {
    use crate::db::line_types;
    const MAX_LOOKBACK: usize = 50;
    let lookback_floor = target_line.saturating_sub(MAX_LOOKBACK);
    let mut i = target_line;
    while i > lookback_floor {
        let text = buffer_line_text(buffer, i - 1);
        if line_types::is_act_scene_marker(text.trim()) {
            return i - 1;
        }
        i -= 1;
    }
    page_turn_top(buffer, target_line)
}

/// Earliest line whose y-coordinate is reachable by `vadjustment.set_value`
/// (i.e., y <= upper - page_size). When `proposed_top` is already reachable,
/// returns it unchanged. Otherwise walks backward to find the last line that
/// the scroll can actually position at the viewport top. Used by page_forward
/// and scroll_after_jump_forward to avoid setting a page_top that GTK clamps.
pub(crate) fn clamp_page_top_to_scroll_ceiling(state: &AppState, proposed_top: usize) -> usize {
    let adj = state.scrolled_window.vadjustment();
    let max_value = (adj.upper() - adj.page_size()).max(0.0);
    if let Some(iter) = state.buffer.iter_at_line(proposed_top as i32) {
        let (y, _) = state.text_view.line_yrange(&iter);
        if (y as f64) <= max_value {
            return proposed_top;
        }
    }
    for l in (0..proposed_top).rev() {
        if let Some(it) = state.buffer.iter_at_line(l as i32) {
            let (ly, _) = state.text_view.line_yrange(&it);
            if (ly as f64) <= max_value {
                return l;
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Page boundary computation
// ---------------------------------------------------------------------------

/// Raw last visible line without trims. Used by playback sync to decide
/// when the cursor has moved past what's actually rendered on screen.
/// Trims are for page-boundary placement; sync needs the physical boundary.
pub(crate) fn last_raw_visible_line(state: &AppState, top: usize) -> usize {
    if let Some(cached) = state.last_visible_range.get() {
        if cached.count > 0 {
            return cached.last_fit;
        }
    }
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    range.last_fit
}

/// Find the last buffer line that fits within the viewport starting from
/// `top`, matching the bottom clip calculation exactly. A line is included
/// only if its full height fits in the remaining usable space (widget height
/// minus descender guard). This ensures page_forward doesn't count clipped
/// lines as "seen". Trailing speaker names and blanks are trimmed so a
/// dangling speaker at the bottom doesn't count as "visible" content.
pub(crate) fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    let is_prose = state.is_prose();
    // Trim because this function feeds page-boundary placement decisions
    // (next_page_top): we don't want to put a partial verse stanza or
    // dangling speaker at the BOTTOM of the new page.
    let trimmed = trim_visible_range(range, top, &state.text_view, &state.buffer, is_prose);
    trimmed.last_fit
}

/// Result of stepping forward one page from `top`: the new page-top
/// (after backing up for a speaker) and the next dialogue line that
/// would become `state.current_line`. Both equal `line_count` when there
/// is no further dialogue.
#[derive(Clone, Copy)]
pub(crate) struct NextPage {
    /// Page-top of the page that follows `top` — what `set_page` is called
    /// with. Equals `line_count` if past the last page.
    pub(crate) new_top: usize,
    /// First dialogue line on the next page — what `state.current_line`
    /// should become. Equals `line_count` if past the last page.
    pub(crate) next_dialogue: usize,
}

/// Compute the next-page boundary from `top`, using the same logic as
/// `page_forward`. Returns `{ line_count, line_count }` when there is no
/// further dialogue. Pure — does not mutate `state`.
pub(crate) fn next_page_top(state: &AppState, top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    if line_count == 0 || top >= line_count {
        return NextPage { new_top: line_count, next_dialogue: line_count };
    }
    let last_visible = last_fully_visible_line(state, top);
    let last = last_dialogue_in_page(
        &state.buffer,
        top,
        last_visible.saturating_sub(top) + 1,
        line_count,
    );
    let next_dialogue = next_dialogue_from(&state.buffer, last + 1, line_count);
    if next_dialogue >= line_count {
        return NextPage { new_top: line_count, next_dialogue: line_count };
    }
    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
    NextPage { new_top, next_dialogue }
}

/// Backward mirror of `next_page_top`. Returns the page boundary immediately
/// before `current_top`, with the same `back_up_for_speaker` + `next_dialogue`
/// post-processing the forward path uses.
///
/// Three-tier lookup:
/// 1. F8 `page_tops` cache — `binary_search` for the fast path (O(log n)).
/// 2. Cold-start linear walk from line 0 looking for the page whose
///    `next_page_top` equals `current_top`. O(page_count) one-time cost.
/// 3. Lpp approximation — pathological case where `current_top` is not on any
///    forward-walkable boundary (e.g., user resumed at an arbitrary line via
///    concordance). Preserves the historical fallback rather than refusing to
///    move.
///
/// Mirrors foliate-js's `atStart`/`atEnd` page-index lookups
/// (paginator.js:1050-1054) — exact previous boundary instead of approximation.
pub(crate) fn prev_page_top(state: &AppState, current_top: usize) -> NextPage {
    let line_count = state.effective_line_count();
    if current_top == 0 || line_count == 0 {
        return NextPage { new_top: 0, next_dialogue: 0 };
    }

    // Tier 1: F8 cache fast path.
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            if let Ok(idx) = tops.binary_search(&current_top) {
                if idx > 0 {
                    let prev_top = tops[idx - 1];
                    let next_dialogue = next_dialogue_from(&state.buffer, prev_top, line_count);
                    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
                    return NextPage { new_top, next_dialogue };
                }
            }
        }
    }

    // Tier 2: cold-start linear walk from 0 looking for the page whose
    // next_page_top equals current_top.
    let mut top: usize = 0;
    while top < current_top {
        let next = next_page_top(state, top).new_top;
        if next == current_top {
            let next_dialogue = next_dialogue_from(&state.buffer, top, line_count);
            let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
            return NextPage { new_top, next_dialogue };
        }
        if next <= top {
            break; // safety: no progress
        }
        top = next;
    }

    // Tier 3: lpp approximation — current_top is not on any forward-walkable
    // boundary. Preserve historical behavior rather than refusing to move.
    let lpp = lines_per_page(state).max(1);
    let approx = current_top.saturating_sub(lpp);
    let next_dialogue = next_dialogue_from(&state.buffer, approx, line_count);
    let new_top = back_up_for_speaker(&state.buffer, next_dialogue);
    NextPage { new_top, next_dialogue }
}

/// Build the page_tops index by walking next_page_top from line 0 to the end
/// of the work. Result is the same as repeatedly calling next_page_top from
/// line 0; cost is O(line_count) once instead of O(line_count²) on every
/// overlay-label refresh.
fn build_page_tops(state: &AppState) -> Vec<usize> {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return Vec::new();
    }
    let mut tops = vec![0usize];
    let mut top: usize = 0;
    while top < line_count {
        let next = next_page_top(state, top).new_top;
        if next <= top || next >= line_count {
            break;
        }
        tops.push(next);
        top = next;
    }
    tops
}

/// Return the 1-indexed viewport page that contains `target_line`. Reads
/// from the page_tops cache; builds it on first need.
///
/// If the text_view hasn't been laid out yet (height ≤ 0), build returns a
/// degenerate `[0]` index that would lock the label to "page 1". We refuse
/// to cache that — return 1 directly and leave the cache empty so the next
/// call after layout completes triggers a real build.
pub fn viewport_page_for_line(state: &AppState, target_line: usize) -> usize {
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            return page_for_line_in_index(tops, target_line);
        }
    }
    // Don't build (and cache) before layout — next_page_top returns garbage
    // when widget_height ≤ 0 and the resulting [0] index would stick.
    if state.text_view.height() <= 0 {
        return 1;
    }
    let tops = build_page_tops(state);
    let page = page_for_line_in_index(&tops, target_line);
    *state.page_tops.borrow_mut() = Some(tops);
    page
}

/// Drop the page_tops cache. Called when font/size changes invalidate page
/// boundaries (resnap_page) and when a new work loads (display_work).
pub fn invalidate_page_tops(state: &AppState) {
    *state.page_tops.borrow_mut() = None;
}

// ---------------------------------------------------------------------------
// Visibility checks
// ---------------------------------------------------------------------------

/// Check if a line is on screen at all (no padding requirement).
/// Used by playback sync to avoid premature page turns.
pub fn is_line_on_screen(state: &AppState, line: usize) -> bool {
    is_line_fully_visible(state, line)
}

/// Check whether a buffer line is fully visible in the viewport.
/// Sums line heights from page_top against widget_height minus a descender
/// guard and the text_view bottom margin (which GTK reserves for padding
/// and is not available for text rendering).
pub(crate) fn is_line_fully_visible(state: &AppState, line: usize) -> bool {
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top_line {
        return false;
    }
    // F4: fast path — consult the cache (raw range — every line genuinely
    // rendered on screen, no F9 trim applied because "is line N drawn?" is
    // a different question from "where should the next page boundary land?").
    if let Some(cached) = state.last_visible_range.get() {
        return line <= cached.last_fit && cached.count > 0;
    }
    // Cold-start fallback: recompute the raw range. No trim — see comment
    // above for why visibility-checks use raw, not trimmed.
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return true;
    }
    let descender_guard = descender_guard_px(&state.text_view, state.page_top_line);
    let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
    let line_count = state.effective_line_count();
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        state.page_top_line,
        line_count,
        usable_height,
    );
    line <= range.last_fit && range.count > 0
}

/// Count how many buffer lines are fully visible starting from `page_top_line`.
/// Returns a calibrated estimate (35) during work load when GTK layout is
/// invalid, and a small fallback (15) for empty or past-end buffers.
pub(crate) fn lines_per_page(state: &AppState) -> usize {
    if state.loading_work.get() {
        return 35;
    }

    let line_count = state.effective_line_count();
    let start = state.page_top_line;

    if line_count == 0 || start >= line_count {
        return 15;
    }

    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return 15;
    }

    let descender_guard = descender_guard_px(&state.text_view, start);
    let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        start,
        line_count,
        usable_height,
    );
    range.count.max(1)
}

/// Pixel descent of the active font, queried from Pango. Mirrors foliate-js's
/// approach (paginator.js:83-91) of measuring the engine rather than estimating
/// from line height — fixes mixed-font-size pages where the bottom line uses a
/// different font than the page top (translations smaller, chapter titles
/// larger).
///
/// `_page_top` is unused but kept in the signature so the four callers (which
/// pass it from their local context) don't need to change.
///
/// Reads the active font from the `font-size` TextTag (which `reapply_font`
/// updates synchronously) rather than the widget's Pango context (which only
/// reflects font changes after GTK applies the next CSS pass). This avoids a
/// one-frame race where descender stayed clipped briefly after font cycling.
///
/// Returns the descent in pixels, with a small safety floor (4 px) and ceiling
/// (24 px) to prevent absurd values from a missing/broken font from corrupting
/// the visible-range calculation.
pub(crate) fn descender_guard_px(text_view: &sourceview5::View, _page_top: usize) -> i32 {
    use gtk4::prelude::{TextTagExt, TextBufferExt, WidgetExt};
    let ctx = text_view.pango_context();

    // Prefer the explicit font from the `font-size` tag we set in reapply_font.
    // This avoids the GTK CSS-application race where pango_context().metrics()
    // returns the OLD font's descent for one frame after a font change.
    let font_desc = text_view
        .buffer()
        .tag_table()
        .lookup("font-size")
        .and_then(|tag| tag.font_desc());

    let metrics = ctx.metrics(font_desc.as_ref(), None);
    let descent_px = metrics.descent() / pango::SCALE;
    descent_px.clamp(4, 24)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod visible_range_helpers_tests {
    use super::{VisibleRange, trim_trailing_speakers_pure};

    fn line_classifier(speakers_or_blanks: &[usize]) -> impl Fn(usize) -> bool + '_ {
        move |i| speakers_or_blanks.contains(&i)
    }

    fn line_height(_i: usize) -> i32 {
        20
    }

    #[test]
    fn trim_with_no_trailing_speaker_is_identity() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 6);
    }

    #[test]
    fn trim_drops_one_trailing_speaker() {
        let range = VisibleRange { last_fit: 5, total_height: 100, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 80);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_drops_speaker_with_preceding_blanks() {
        // Lines 3 (blank), 4 (blank), 5 (speaker) — all trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.total_height, 60);
        assert_eq!(trimmed.count, 3);
    }

    #[test]
    fn trim_stops_at_dialogue_line() {
        // Line 5 is speaker, line 4 is dialogue (not in classifier), line 3 is blank.
        // Trim removes line 5 only — line 4 is dialogue, blocks further trim.
        let range = VisibleRange { last_fit: 5, total_height: 120, count: 6 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[3, 5]), // 4 is dialogue
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 4);
        assert_eq!(trimmed.total_height, 100);
        assert_eq!(trimmed.count, 5);
    }

    #[test]
    fn trim_does_not_cross_page_top() {
        // Every line is a speaker, but page_top is 3 — trim must not delete the
        // page top itself (would leave an empty page).
        let range = VisibleRange { last_fit: 5, total_height: 60, count: 3 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            3,
            &line_classifier(&[3, 4, 5]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 3, "must leave page_top in place");
        assert!(trimmed.total_height > 0);
        assert!(trimmed.count >= 1);
    }

    #[test]
    fn trim_with_empty_range_is_noop() {
        // last_fit == page_top, count == 1 — nothing to trim.
        let range = VisibleRange { last_fit: 0, total_height: 20, count: 1 };
        let trimmed = trim_trailing_speakers_pure(
            range,
            0,
            &line_classifier(&[0]),
            &line_height,
        );
        assert_eq!(trimmed.last_fit, 0);
        assert_eq!(trimmed.total_height, 20);
        assert_eq!(trimmed.count, 1);
    }
}

#[cfg(test)]
mod page_tops_tests {
    use super::page_for_line_in_index;

    #[test]
    fn page_for_line_returns_1_for_empty_index() {
        let tops: Vec<usize> = vec![];
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 100), 1);
    }

    #[test]
    fn page_for_line_returns_1_for_first_page() {
        let tops = vec![0, 35, 70, 105]; // page 1 starts at 0, page 2 at 35, etc.
        assert_eq!(page_for_line_in_index(&tops, 0), 1);
        assert_eq!(page_for_line_in_index(&tops, 10), 1);
        assert_eq!(page_for_line_in_index(&tops, 34), 1);
    }

    #[test]
    fn page_for_line_returns_2_for_second_page() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
        assert_eq!(page_for_line_in_index(&tops, 69), 2);
    }

    #[test]
    fn page_for_line_handles_target_past_end() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(page_for_line_in_index(&tops, 200), 4);
    }

    #[test]
    fn page_for_line_exact_top_match() {
        let tops = vec![0, 35, 70];
        // line 35 is the START of page 2 — partition_point gives index 2,
        // page = 2.
        assert_eq!(page_for_line_in_index(&tops, 35), 2);
    }
}

#[cfg(test)]
mod prev_page_top_tests {
    // Pure tests against the page_tops index lookup. The cold-walk and
    // lpp-fallback paths require GTK and are exercised in manual verification.
    use super::page_for_line_in_index;

    /// Pure helper mirroring prev_page_top's binary_search fast path,
    /// extracted for unit testing without a full AppState.
    fn prev_top_via_index(tops: &[usize], current_top: usize) -> Option<usize> {
        if current_top == 0 {
            return Some(0);
        }
        match tops.binary_search(&current_top) {
            Ok(idx) if idx > 0 => Some(tops[idx - 1]),
            _ => None,
        }
    }

    #[test]
    fn prev_top_returns_zero_for_current_zero() {
        let tops = vec![0, 35, 70];
        assert_eq!(prev_top_via_index(&tops, 0), Some(0));
    }

    #[test]
    fn prev_top_finds_previous_boundary() {
        let tops = vec![0, 35, 70, 105];
        assert_eq!(prev_top_via_index(&tops, 70), Some(35));
        assert_eq!(prev_top_via_index(&tops, 35), Some(0));
        assert_eq!(prev_top_via_index(&tops, 105), Some(70));
    }

    #[test]
    fn prev_top_returns_none_for_off_boundary_target() {
        let tops = vec![0, 35, 70];
        // 40 is not a page top; binary_search returns Err — caller falls
        // back to cold-walk or lpp.
        assert_eq!(prev_top_via_index(&tops, 40), None);
    }

    #[test]
    fn page_for_line_after_prev_top_is_consistent() {
        // Sanity: if prev_page_top returns prev_top, page_for_line_in_index
        // for any line in [prev_top, current_top) should return the page
        // index of prev_top. This is the bidirectional symmetry property.
        let tops = vec![0, 35, 70, 105];
        let prev = prev_top_via_index(&tops, 70).unwrap();
        assert_eq!(prev, 35);
        // Line 50 is on the page that starts at 35 — page 2.
        assert_eq!(page_for_line_in_index(&tops, 50), 2);
    }
}

#[cfg(test)]
mod block_atom_tests {
    use super::{VisibleRange, block_start_for_line_pure, trim_block_atoms_pure};

    /// Build classifiers for a synthetic line array.
    /// `kinds` maps line index → 'b' (blank), 's' (speaker), 'd' (stage dir),
    /// 'l' (dialogue line).
    fn classifiers(kinds: &[char]) -> (
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
        impl Fn(usize) -> bool + '_,
    ) {
        let is_blank = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'b');
        let is_speaker = move |i: usize| kinds.get(i).map_or(false, |c| *c == 's');
        let is_stage = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'd');
        let is_dialogue = move |i: usize| kinds.get(i).map_or(false, |c| *c == 'l');
        (is_blank, is_speaker, is_stage, is_dialogue)
    }

    #[test]
    fn block_start_in_3line_stage_direction_returns_first_dir_line() {
        // Lines: 0=speaker, 1=dir, 2=dir, 3=dir
        let kinds = ['s', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-block), page_top=0, is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "should back up to first stage-direction line");
    }

    #[test]
    fn block_start_at_block_start_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=dir
        let kinds = ['s', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (at block start), page_top=0
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "no backup when last_fit is already block start");
    }

    #[test]
    fn block_start_in_verse_stanza_non_prose_returns_stanza_start() {
        // Lines: 0=speaker, 1=l, 2=l, 3=l, 4=blank
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-stanza), is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "verse stanza in non-prose work backs up to stanza start");
    }

    #[test]
    fn block_start_in_verse_stanza_prose_returns_unchanged() {
        // Same lines, but is_prose=true — rule does not apply.
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, true, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 3, "verse stanza rule skipped for prose works");
    }

    #[test]
    fn block_start_single_line_stage_direction_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=l
        let kinds = ['s', 'd', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (single dir line)
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "single-line stage direction is not multi-line — no backup");
    }

    #[test]
    fn block_start_stops_at_speaker() {
        // Lines: 0=blank, 1=speaker, 2=l, 3=l (stanza bounded above by speaker)
        let kinds = ['b', 's', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3, page_top=0
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at speaker line (returns first dialogue line)");
    }

    #[test]
    fn block_start_stops_at_blank() {
        // Lines: 0=l, 1=blank, 2=l, 3=l (stanza bounded above by blank)
        let kinds = ['l', 'b', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 2, "stanza backup stops at blank line (returns first dialogue line after blank)");
    }

    #[test]
    fn block_start_in_verse_stanza_bounded_above_by_stage_direction() {
        // Lines: 0=stage, 1=l, 2=l (stanza after a stage direction)
        let kinds = ['d', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 2, false, &is_blank, &is_speaker, &is_stage, &is_dialogue);
        assert_eq!(start, 1, "stanza backup stops at stage direction (returns first dialogue line after stage)");
    }

    #[test]
    fn trim_block_atoms_block_fully_fits_reduces_count() {
        // Block at lines 3-5; visible range stops at 4 (mid-block — line 5
        // would render off-screen). Trim backs up to before block start.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        // Lines 0=speaker, 1=l, 2=l, 3=d, 4=d, 5=d (stage block extends past last_fit)
        let kinds = ['s', 'l', 'l', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        // Block starts at 3 and continues past last_fit (line 5 is also stage);
        // new last_fit = 2; new count = 3 (lines 0,1,2).
        assert_eq!(trimmed.last_fit, 2);
        assert_eq!(trimmed.count, 3);
        assert_eq!(trimmed.total_height, 60); // dropped lines 3 and 4 at 20px each
    }

    #[test]
    fn trim_block_atoms_block_ends_at_last_fit_unchanged() {
        // Block at lines 3-4 ends exactly at last_fit (line 5 is dialogue,
        // outside the stage block). No mid-block split — leave range as-is.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        let kinds = ['s', 'l', 'l', 'd', 'd', 'l']; // line 5 is dialogue, ends the stage block
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 4, "block ends at last_fit; no trim");
        assert_eq!(trimmed.count, 5);
        assert_eq!(trimmed.total_height, 100);
    }

    #[test]
    fn trim_block_atoms_verse_stanza_ends_at_last_fit_unchanged() {
        // 2-line verse stanza (lines 3-4) ends at last_fit; line 5 is the
        // next speaker, closing the stanza. No trim — stanza fully fits.
        // This is the bug case from manual verification: pressing j with
        // cursor on the previous speaker's last dialogue should not
        // trigger a page-turn that removes a fully-visible 2-line stanza.
        let range = VisibleRange { last_fit: 4, total_height: 100, count: 5 };
        let kinds = ['s', 'l', 'l', 'l', 'l', 's']; // last_fit=4 dialogue, line 5 speaker closes stanza
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 4, "stanza ends at last_fit; no trim");
        assert_eq!(trimmed.count, 5);
        assert_eq!(trimmed.total_height, 100);
    }

    #[test]
    fn trim_block_atoms_block_doesnt_fit_returns_unchanged() {
        // Whole page IS the block — block_start (== 0) <= page_top (== 0).
        let range = VisibleRange { last_fit: 3, total_height: 80, count: 4 };
        let kinds = ['d', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 3);
        assert_eq!(trimmed.count, 4);
        assert_eq!(trimmed.total_height, 80);
    }

    #[test]
    fn trim_block_atoms_block_too_tall_keeps_per_line_split() {
        // Page-top is a speaker; verse stanza extends from line 1 down past
        // the visible area. block_start = 1 (just below page_top), block_end
        // = 9 (last_fit). Trimming would leave only line 0 (the speaker)
        // visible — useless. Overflow guard fires (new_count=1, range.count=10,
        // 1 * 2 <= 10 → keep per-line split).
        let range = VisibleRange { last_fit: 9, total_height: 200, count: 10 };
        let kinds = ['s', 'l', 'l', 'l', 'l', 'l', 'l', 'l', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 9, "block too tall: keep original last_fit");
        assert_eq!(trimmed.count, 10);
        assert_eq!(trimmed.total_height, 200);
    }

    #[test]
    fn trim_block_atoms_empty_range_unchanged() {
        let range = VisibleRange { last_fit: 0, total_height: 0, count: 0 };
        let kinds: [char; 0] = [];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.count, 0);
    }

    #[test]
    fn trim_block_atoms_at_page_top_unchanged() {
        // last_fit equals page_top — trim is no-op (already minimal).
        let range = VisibleRange { last_fit: 5, total_height: 20, count: 1 };
        let kinds = ['l', 'l', 'l', 'l', 'l', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 5, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.count, 1);
    }
}

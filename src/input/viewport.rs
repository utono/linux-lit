use gtk4::prelude::*;

use crate::app::AppState;
use super::scroll::SINGLE_COLUMN_BOTTOM_MARGIN;

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

/// Forward sub-line paging step for an OVER-TALL prose paragraph (one buffer line
/// taller than the viewport). Given the current pixel `offset` already scrolled
/// past the paragraph's top, the paragraph's full wrapped height `para_h`, and the
/// `usable` viewport height, return:
/// - `Some(next_offset)` — the paragraph still has rows below the fold, so the
///   next `x` scrolls down by one viewport WITHIN the same buffer line; or
/// - `None` — the paragraph is exhausted (its bottom is at/above the fold), so the
///   next `x` advances to the next buffer line (offset resets to 0).
///
/// Pure (no GTK) so it is unit-testable. The GTK caller snaps `offset + usable`
/// DOWN to a real visual-row top before using it; snapping only reduces the step,
/// which stays `> offset` and `< para_h`, so row coverage is preserved.
pub(crate) fn overtall_next_offset(offset: i32, para_h: i32, usable: i32) -> Option<i32> {
    let usable = usable.max(1);
    if para_h - offset > usable {
        Some(offset + usable)
    } else {
        None
    }
}

/// Pure fill decision for prose visual-row paging. Given the viewport's
/// absolute top pixel `y0`, the document's total pixel height `total`, and
/// the `usable` viewport height: the RAW next boundary pixel, or None when
/// the current page already shows the document tail.
pub(crate) fn prose_raw_next_boundary(y0: i32, total: i32, usable: i32) -> Option<i32> {
    let usable = usable.max(1);
    if total - y0 > usable {
        Some(y0 + usable)
    } else {
        None
    }
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

/// Minimal trailing trim for the LEFT column of a two-column spread: back up
/// only off trailing BARE SPEAKER NAMES and trailing BLANKS (a speaker must
/// lead its dialogue in the right column; blanks are invisible). Unlike
/// `trim_trailing_speakers`, this does NOT treat a trailing stage direction as
/// dangling — a stage direction at the bottom of the left column is fine and
/// trimming it would underfill the column. Maximizes left-column fill.
pub(crate) fn trim_trailing_speaker_only(
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
        line_types::is_speaker(&text) || line_types::is_blank(&text)
    };
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_trailing_speakers_pure(range, page_top, is_dangling_context, line_height)
}

/// If `range.last_fit` falls partway through a multi-line stage-direction
/// `[...]` block (the block's closing `]` is below `last_fit`), back the range
/// up to the line before the block starts so the whole stage direction stays
/// together (moves to the top of the right column). No-op when the range ends
/// on a complete line or a self-closed stage direction. Never crosses
/// `page_top`.
pub(crate) fn back_up_off_partial_stage_direction(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> VisibleRange {
    let last = range.last_fit;
    if last <= page_top {
        return range;
    }
    // Is last_fit inside a stage-direction block whose `]` is NOT yet seen by
    // last_fit? Scan from the block's start (an opening `[`) forward: if the
    // closing `]` is strictly below last_fit, the block is cut by the split.
    if !is_inside_stage_direction(buffer, last) {
        return range;
    }
    // The line itself closing the bracket means the block is complete here.
    let last_text = buffer_line_text(buffer, last);
    if last_text.trim_end().ends_with(']') {
        return range;
    }
    // Find the start of the stage-direction block (the line with the opening
    // `[`), scanning back from last_fit.
    let mut block_start = last;
    while block_start > page_top {
        let t = buffer_line_text(buffer, block_start);
        if t.trim_start().starts_with('[') {
            break;
        }
        block_start -= 1;
    }
    // Back up to the line before the block. If that would empty the page
    // (block starts at page_top), leave the range as-is — better an orphan
    // than a blank column.
    if block_start <= page_top {
        return range;
    }
    let new_last = block_start - 1;
    let mut total = 0i32;
    for i in page_top..=new_last {
        let Some(iter) = buffer.iter_at_line(i as i32) else { continue };
        let (_y, h) = text_view.line_yrange(&iter);
        total += h;
    }
    VisibleRange {
        last_fit: new_last,
        total_height: total,
        count: new_last - page_top + 1,
    }
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
pub(crate) fn block_start_for_line_pure<B, S, D, L, N>(
    page_top: usize,
    last_fit: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
    is_stanza_number: &N,
) -> usize
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
    N: Fn(usize) -> bool,
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
    // Only atomize if the block is bounded by a speaker or stanza number
    // (plays, numbered verse). Plain blank-delimited verse paragraphs
    // (continuous epics like the Odyssey) are allowed to split.
    if !is_prose && is_dialogue(last_fit) {
        let mut start = last_fit;
        while start > page_top {
            let prev = start - 1;
            if is_blank(prev) || is_speaker(prev) || is_stage(prev) {
                break;
            }
            start -= 1;
        }
        // Check what bounds the block above: speaker or stanza number
        // means this is a structured block worth keeping atomic.
        let bounded_by_speaker = start > 0 && is_speaker(start - 1);
        let has_number_above = start > page_top + 1
            && is_blank(start - 1)
            && is_stanza_number(start - 2);
        if has_number_above {
            start -= 2;
        } else if !bounded_by_speaker {
            return last_fit;
        }
        if start == last_fit {
            return last_fit;
        }
        return start;
    }

    // Standalone stanza number: if last_fit is a stanza number itself,
    // include the blank + verse lines below it as part of the block check.
    if !is_prose && is_stanza_number(last_fit) {
        return last_fit;
    }

    last_fit
}

/// Pure: trim a `VisibleRange` so the last fitting line isn't mid-block.
/// Backs `last_fit` up to the line before block-start when the block fully
/// fits; returns the range unchanged when the block doesn't fit at all
/// (`block_start <= page_top` — overflow fallback policy from F9 spec).
///
/// `line_height` closure provides per-line heights for `total_height` accounting.
pub(crate) fn trim_block_atoms_pure<B, S, D, L, H, N>(
    range: VisibleRange,
    page_top: usize,
    is_prose: bool,
    is_blank: &B,
    is_speaker: &S,
    is_stage: &D,
    is_dialogue: &L,
    line_height: &H,
    is_stanza_number: &N,
) -> VisibleRange
where
    B: Fn(usize) -> bool,
    S: Fn(usize) -> bool,
    D: Fn(usize) -> bool,
    L: Fn(usize) -> bool,
    H: Fn(usize) -> i32,
    N: Fn(usize) -> bool,
{
    if range.count == 0 || range.last_fit == page_top {
        return range;
    }
    let block_start = block_start_for_line_pure(
        page_top, range.last_fit, is_prose,
        is_blank, is_speaker, is_stage, is_dialogue, is_stanza_number,
    );
    // Standalone stanza number at bottom of page: trim it (the stanza body
    // follows below). block_start == last_fit for a single-line stanza number,
    // so we handle it before the "not in a block" early return.
    let is_trailing_stanza_num = !is_prose && is_stanza_number(range.last_fit);
    if block_start == range.last_fit && !is_trailing_stanza_num {
        return range; // not in a block
    }
    if block_start <= page_top && !is_trailing_stanza_num {
        return range; // overflow: block extends to (or past) page_top
    }
    // Only trim if the block actually continues past last_fit (i.e., we're
    // splitting it mid-block). If the next line is OUTSIDE the same block
    // (a blank, speaker, stage transition, or end of buffer), the block
    // ends at last_fit and there's nothing to atomicize — leave as-is.
    let continues = if is_trailing_stanza_num {
        true
    } else if is_stage(range.last_fit) {
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
    // For a trailing stanza number, block_start is the stanza number itself.
    let effective_start = if is_trailing_stanza_num { range.last_fit } else { block_start };
    // Drop lines [effective_start, range.last_fit] from the range.
    let mut new_total_height = range.total_height;
    for i in effective_start..=range.last_fit {
        new_total_height -= line_height(i);
    }
    let new_last_fit = effective_start - 1;
    let new_count = new_last_fit - page_top + 1;
    // Overflow guard: if trimming would leave the page less than half full
    // by line count, the block is too tall to fit on one page. Keep the
    // per-line split rather than showing a useless stub page.
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
    lookup: StageLookup,
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
    let is_stage = |i: usize| is_stage_db_first(i, &line_text(i), lookup);
    let is_dialogue = |i: usize| is_dialogue_db_first(i, &line_text(i), is_prose, lookup);
    let is_stanza_number = |i: usize| line_types::is_stanza_number(&line_text(i));
    block_start_for_line_pure(page_top, last_fit, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue, &is_stanza_number)
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
    lookup: StageLookup,
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
    let is_stage = |i: usize| is_stage_db_first(i, &line_text(i), lookup);
    let is_dialogue = |i: usize| is_dialogue_db_first(i, &line_text(i), is_prose, lookup);
    let is_stanza_number = |i: usize| line_types::is_stanza_number(&line_text(i));
    let line_height = |i: usize| -> i32 {
        let Some(iter) = buffer.iter_at_line(i as i32) else { return 0 };
        let (_y, h) = text_view.line_yrange(&iter);
        h
    };
    trim_block_atoms_pure(range, page_top, is_prose,
        &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &is_stanza_number)
}

/// Maps a buffer line index to its mapped DB `sub_line` (0 = spoken, >0 = stage
/// direction), or `None` when the buffer line has no mapped work line.
pub(crate) type StageLookup<'a> = &'a dyn Fn(usize) -> Option<i64>;

/// The always-`None` lookup: forces pure regex classification. Used by callers
/// with no `AppState`/`LineMap` in scope (tests, mid-load, no-coverage works).
pub(crate) fn no_stage_lookup() -> StageLookup<'static> {
    &|_| None
}

/// Stage-direction check: prefer the mapped DB row (`sub_line > 0`), else regex.
pub(crate) fn is_stage_db_first(line_index: usize, text: &str, lookup: StageLookup) -> bool {
    use crate::db::line_types;
    match lookup(line_index) {
        Some(sub_line) => sub_line > 0,
        None => line_types::is_stage_direction(text),
    }
}

/// Dialogue check: a mapped stage row (`sub_line > 0`) is never dialogue; a
/// mapped spoken row falls through to the text heuristic (it still distinguishes
/// speaker/separator/blank); an unmapped line uses the text heuristic entirely.
pub(crate) fn is_dialogue_db_first(
    line_index: usize,
    text: &str,
    is_prose: bool,
    lookup: StageLookup,
) -> bool {
    use crate::db::line_types;
    if let Some(sub_line) = lookup(line_index) {
        if sub_line > 0 {
            return false; // stage direction: never dialogue
        }
    }
    line_types::is_dialogue(text, is_prose)
}

/// Scan (page_top, last_fit] for a line that STARTS a new scene/section and, if
/// found, clamp last_fit to the line before it so the new section opens the next
/// page.
///
/// The boundary is the authoritative `is_break` predicate built from the DB's
/// `(div1,div2)` columns (`AppState::section_starts`). With an authoritative
/// boundary, the old "skip my own opening heading, but clamp a later one"
/// problem dissolves: a page that OPENS at a boundary has `is_break(page_top) ==
/// true` and we scan from `page_top + 1`, so it never self-clamps; a LATER
/// boundary inside the range still clamps. No `[They exit.]` header-skip
/// bridging (the AWW y-GAP).
///
/// `is_break` is `None` only mid-load (before the line map exists) — then we
/// fall back to the legacy text heuristic (`is_act_scene_marker`/`is_separator`)
/// with its header-skip, so an early reveal still paginates sanely.
/// Build the authoritative section-boundary closure over a `section_starts`
/// slice: `is_break(line) == section_starts[line]`. Returned as a concrete
/// closure so callers can pass `Some(&f)` as `Option<&dyn Fn(usize)->bool>`.
pub(crate) fn section_break_fn(section_starts: &[bool]) -> impl Fn(usize) -> bool + '_ {
    move |line: usize| section_starts.get(line).copied().unwrap_or(false)
}

fn clamp_at_section_break(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_break: Option<&dyn Fn(usize) -> bool>,
) -> VisibleRange {
    if range.count <= 1 {
        return range;
    }
    let break_line = match is_break {
        Some(is_break) => {
            // Authoritative path: first boundary strictly after page_top.
            (page_top + 1..=range.last_fit).find(|&i| is_break(i))
        }
        None => clamp_break_line_from_text(buffer, page_top, range.last_fit),
    };
    let break_line = match break_line {
        Some(b) => b,
        None => return range,
    };
    let clamped_last = break_line.saturating_sub(1);
    if clamped_last < page_top {
        return range;
    }
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

/// Legacy text-inference fallback for `clamp_at_section_break`, used only before
/// the line map (and its `section_starts` bitmap) is built. Skips the page's
/// own opening header block (consecutive markers/separators/blanks/stage
/// directions) then returns the first act/scene marker or separator after it.
fn clamp_break_line_from_text(
    buffer: &sourceview5::Buffer,
    page_top: usize,
    last_fit: usize,
) -> Option<usize> {
    use crate::db::line_types;
    let mut scan_start = page_top + 1;
    while scan_start <= last_fit {
        let trimmed = buffer_line_text(buffer, scan_start);
        let trimmed = trimmed.trim();
        if line_types::is_act_scene_marker(trimmed)
            || line_types::is_separator(trimmed)
            || trimmed.is_empty()
            || line_types::is_stage_direction(trimmed)
        {
            scan_start += 1;
        } else {
            break;
        }
    }
    (scan_start..=last_fit).find(|&i| {
        let trimmed = buffer_line_text(buffer, i);
        let trimmed = trimmed.trim();
        line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed)
    })
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
    is_break: Option<&dyn Fn(usize) -> bool>,
) -> VisibleRange {
    trim_visible_range_opts(range, page_top, text_view, buffer, is_prose, false, is_break, no_stage_lookup())
}

/// Like `trim_visible_range`, but `relax_underfill` raises the fill-guard
/// threshold so a column left even mildly underfilled by block-atom trimming
/// reverts to the per-line split (i.e. lets the block split across the
/// boundary). Used by the RIGHT column of a two-column spread: there, pushing a
/// block off the bottom doesn't move it to a fresh full page — it leaves a
/// visible gap mid-spread, so splitting the block to fill the column reads
/// better than the gap. Single-column callers pass `false` and keep block
/// atomicity (the pushed block fills the next full page, so a moderate
/// underfill on the current page is fine).
pub(crate) fn trim_visible_range_opts(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
    relax_underfill: bool,
    is_break: Option<&dyn Fn(usize) -> bool>,
    lookup: StageLookup,
) -> VisibleRange {
    let r = clamp_at_section_break(range, page_top, text_view, buffer, is_break);
    let r = trim_trailing_speakers(r, page_top, text_view, buffer);
    let r2 = r;
    let r = trim_block_atoms(r, page_top, text_view, buffer, is_prose, lookup);
    let r = trim_trailing_speakers(r, page_top, text_view, buffer);
    // Viewport fill guard: if block-atom trim + speaker trim left the column
    // under-full, the removed block was too large relative to the remaining
    // space. Revert to the pre-block-atom state (r2) — the per-line split —
    // which still has section-break clamping and the initial speaker trim.
    //
    // Single column: revert only when badly underfilled (< 2/3), because
    // pushing the block to the next full page is the intended page-turn and a
    // moderate gap is acceptable.
    //
    // Right column (relax_underfill): revert at a much higher threshold so the
    // block splits to fill the column instead of leaving a mid-spread gap.
    if r.last_fit != r2.last_fit && should_revert_underfill(r.total_height, text_view.height(), relax_underfill) {
        return r2;
    }
    r
}

/// Pure fill-guard decision: should a block-atom trim be reverted (i.e. let the
/// block split) because it left the column under-full? Returns true when the
/// trimmed result's pixel height falls below the minimum fill fraction of the
/// viewport. `relax` raises that fraction (right column: 9/10) vs the default
/// (single column: 2/3). A non-positive `widget_height` (layout not ready)
/// never reverts.
pub(crate) fn should_revert_underfill(total_height: i32, widget_height: i32, relax: bool) -> bool {
    if widget_height <= 0 {
        return false;
    }
    let (num, den) = if relax { (9, 10) } else { (2, 3) };
    total_height * den < widget_height * num
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

/// Check if a buffer line is inside a multi-line stage direction `[...]` block.
/// Scans backward (up to 10 lines) looking for an unclosed `[` opener.
pub(crate) fn is_inside_stage_direction(buffer: &sourceview5::Buffer, line: usize) -> bool {
    let text = buffer_line_text(buffer, line);
    let trimmed = text.trim();
    if crate::db::line_types::is_stage_direction(trimmed) {
        return true;
    }
    let start = line.saturating_sub(20);
    for i in (start..line).rev() {
        let prev = buffer_line_text(buffer, i);
        let prev_trimmed = prev.trim();
        if prev_trimmed.ends_with(']') {
            return false;
        }
        if prev_trimmed.starts_with('[') && !prev_trimmed.ends_with(']') {
            return true;
        }
    }
    false
}

/// Check if a buffer line is a dialogue line (not blank, speaker, stage direction, or marker).
///
/// `is_prose` makes the classifier prose-aware: on prose works the play-shaped
/// text heuristics (speaker / stage-direction / act-scene-marker / stanza) do not
/// apply — an ALL-CAPS heading ("CHAPTER I"), a bracketed editorial note, or a
/// shouted line is ordinary prose, so every non-blank, non-separator line counts
/// as dialogue. A DB-mapped stage row (`sub_line > 0`) is still never dialogue in
/// either mode. Verse/play works pass `is_prose=false` and keep the full filters.
pub(crate) fn is_dialogue_line(
    buffer: &sourceview5::Buffer,
    line: usize,
    is_prose: bool,
    lookup: StageLookup,
) -> bool {
    use crate::db::line_types;
    // DB-first: a mapped stage row (sub_line > 0) is never dialogue.
    if let Some(sub_line) = lookup(line) {
        if sub_line > 0 {
            return false;
        }
    }
    let text = buffer_line_text(buffer, line);
    let trimmed = text.trim();
    if is_prose {
        // Prose: only blank and explicit separators are non-dialogue.
        return !trimmed.is_empty() && !line_types::is_separator(trimmed);
    }
    let next = buffer_line_text(buffer, line + 1);
    !trimmed.is_empty()
        && !line_types::is_speaker(trimmed)
        && !line_types::is_stage_direction(trimmed)
        && !line_types::is_act_scene_marker(trimmed)
        && !line_types::is_separator(trimmed)
        && !line_types::is_stanza_number(trimmed)
        && !line_types::is_title_above_separator(&text, &next)
        && !is_inside_stage_direction(buffer, line)
}

/// Find the next dialogue line after `current`. Skips translation lines.
pub(crate) fn next_dialogue_line(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    current: usize,
    line_count: usize,
    is_prose: bool,
    lookup: StageLookup,
) -> Option<usize> {
    let mut i = current + 1;
    while i < line_count {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i, is_prose, lookup)
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
    is_prose: bool,
    lookup: StageLookup,
) -> Option<usize> {
    if current == 0 {
        return None;
    }
    let mut i = current - 1;
    loop {
        if !translation_lines.get(i).copied().unwrap_or(false)
            && is_dialogue_line(buffer, i, is_prose, lookup)
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
pub(crate) fn next_dialogue_from(buffer: &sourceview5::Buffer, from: usize, line_count: usize, is_prose: bool, lookup: StageLookup) -> usize {
    for i in from..line_count {
        if is_dialogue_line(buffer, i, is_prose, lookup) {
            return i;
        }
    }
    line_count
}

/// Find the last dialogue line in the range [from, from+count).
pub(crate) fn last_dialogue_in_page(buffer: &sourceview5::Buffer, from: usize, count: usize, line_count: usize, is_prose: bool, lookup: StageLookup) -> usize {
    let end = (from + count).min(line_count);
    let mut last = from;
    for i in from..end {
        if is_dialogue_line(buffer, i, is_prose, lookup) {
            last = i;
        }
    }
    last
}

/// Given a dialogue line that will be the top of a page, back up over any
/// non-dialogue preamble immediately preceding it: blanks, speakers, stage
/// directions, and the full header block (act/scene markers, separators,
/// and blanks between them). The page top lands on the earliest header line
/// so the reader sees "ACT 1 / Scene 1" instead of a bare separator.
pub(crate) fn back_up_for_speaker(
    buffer: &sourceview5::Buffer,
    line: usize,
    is_break: Option<&dyn Fn(usize) -> bool>,
) -> usize {
    use crate::db::line_types;
    // Authoritative-boundary classifiers: `is_break` (DB div columns) when
    // available, else the legacy text heuristic (mid-load fallback).
    let is_section = |idx: usize| -> bool {
        match is_break {
            Some(f) => f(idx),
            None => {
                let t = buffer_line_text(buffer, idx);
                let t = t.trim();
                line_types::is_act_scene_marker(t) || line_types::is_separator(t)
            }
        }
    };
    // If the target line IS a section boundary (its own opening heading), the
    // page top is that line — do not back up below it onto the previous
    // section's trailing blank. Without this, a one-section-per-page work
    // (sonnet_sequence: heading "2" at the section start) lands page_top on the
    // blank above the heading, producing a blank page between sonnets.
    if is_break.is_some() && is_section(line) {
        return line;
    }
    let mut top = line;
    while top > 0 {
        // Authoritative path: if the line just above is a section boundary
        // (the first chrome line of its run), the page top IS that boundary —
        // stop there. No header-block consume loop: the bitmap marks exactly
        // one line per boundary, and nothing above it in the run is marked.
        if is_break.is_some() && is_section(top - 1) {
            top -= 1;
            break;
        }
        let prev = buffer_line_text(buffer, top - 1);
        let trimmed = prev.trim();
        // Legacy fallback path: consume the whole stacked header block (marker /
        // separator / blanks). Only reached when `is_break` is None.
        if is_break.is_none()
            && (line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed))
        {
            top -= 1;
            while top > 0 {
                let above = buffer_line_text(buffer, top - 1);
                let above_trimmed = above.trim();
                if line_types::is_act_scene_marker(above_trimmed)
                    || line_types::is_separator(above_trimmed)
                    || above_trimmed.is_empty()
                {
                    top -= 1;
                } else {
                    break;
                }
            }
            break;
        }
        // With the authoritative bitmap, a marker/separator that is NOT a
        // boundary line (e.g. the lower chrome lines of a run, or a `=====`
        // separator) is still non-dialogue preamble — back up over it so the
        // page top lands on the boundary line above, not partway down the title.
        let is_chrome_text = is_break.is_some()
            && (line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed));
        // Only back up over a stage direction if it's part of an entrance
        // block (preceded by blank/speaker/stage/header), not a post-dialogue
        // action like "[Countess exits.]" or "[Sings.]".
        let is_entrance_stage_dir = line_types::is_stage_direction(trimmed) && {
            if top >= 2 {
                let above = buffer_line_text(buffer, top - 2);
                let above_t = above.trim();
                above_t.is_empty()
                    || line_types::is_speaker(above_t)
                    || line_types::is_stage_direction(above_t)
                    || line_types::is_act_scene_marker(above_t)
                    || line_types::is_separator(above_t)
            } else {
                true
            }
        };
        if trimmed.is_empty()
            || line_types::is_speaker(trimmed)
            || is_chrome_text
            || is_entrance_stage_dir
            || is_inside_stage_direction(buffer, top - 1)
        {
            top -= 1;
        } else {
            break;
        }
    }
    top
}

/// `state`-taking convenience wrapper for `back_up_for_speaker` that builds the
/// authoritative section-boundary closure from `state.section_starts()`. Use
/// this from GTK call sites; the bare `back_up_for_speaker` stays for the pure
/// test mirror and internal helpers that already hold an `is_break`.
pub(crate) fn back_up_for_speaker_state(state: &AppState, line: usize) -> usize {
    let sec = state.section_starts();
    let bf = sec.map(section_break_fn);
    let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);
    back_up_for_speaker(&state.buffer, line, is_break)
}

/// `state`-taking convenience wrapper for `page_turn_top`.
pub(crate) fn page_turn_top_state(state: &AppState, target_line: usize) -> usize {
    back_up_for_speaker_state(state, target_line)
}

/// `state`-taking convenience wrapper for `is_first_dialogue_of_division`.
pub(crate) fn is_first_dialogue_of_division_state(
    state: &AppState,
    translation_lines: &[bool],
    line: usize,
) -> bool {
    let sec = state.section_starts();
    let bf = sec.map(section_break_fn);
    let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);
    is_first_dialogue_of_division(&state.buffer, translation_lines, line, is_break)
}

/// `state`-taking convenience wrapper for `division_header_top`.
pub(crate) fn division_header_top_state(state: &AppState, line: usize) -> usize {
    let sec = state.section_starts();
    let bf = sec.map(section_break_fn);
    let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);
    division_header_top(&state.buffer, line, is_break)
}


/// Returns true when `line` is the first dialogue line of a scene — i.e.
/// walking backward hits a scene marker or separator before any dialogue.
pub(crate) fn is_first_dialogue_of_division(
    buffer: &sourceview5::Buffer,
    translation_lines: &[bool],
    line: usize,
    is_break: Option<&dyn Fn(usize) -> bool>,
) -> bool {
    use crate::db::line_types;
    if line == 0 {
        return false;
    }
    // Authoritative path: `line` is the first dialogue of a scene iff a section
    // boundary sits between it and the previous dialogue line (i.e. everything
    // back to a boundary is non-dialogue preamble).
    let is_section = |idx: usize| -> bool {
        match is_break {
            Some(f) => f(idx),
            None => {
                let t = buffer_line_text(buffer, idx);
                let t = t.trim();
                line_types::is_act_scene_marker(t) || line_types::is_separator(t)
            }
        }
    };
    let mut i = line;
    // The boundary may be marked on `line` itself (the chrome line) when called
    // on a page top; treat that as a scene start too.
    if is_break.is_some() && is_section(line) {
        return true;
    }
    while i > 0 {
        i -= 1;
        if is_section(i) {
            return true;
        }
        let text = buffer_line_text(buffer, i);
        let t = text.trim();
        if t.is_empty()
            || line_types::is_speaker(t)
            || line_types::is_stage_direction(t)
            || translation_lines.get(i).copied().unwrap_or(false)
        {
            continue;
        }
        return false;
    }
    false
}

/// Walk backward from any line within a scene to find the top of the scene
/// header block (scene marker, separator, blanks above it). Unlike
/// `back_up_for_speaker`, this crosses dialogue lines to reach the header.
pub(crate) fn division_header_top(
    buffer: &sourceview5::Buffer,
    line: usize,
    is_break: Option<&dyn Fn(usize) -> bool>,
) -> usize {
    use crate::db::line_types;
    // Authoritative path: the scene header IS the boundary line at or before
    // `line` (the first chrome line of this scene's run). Walk back to it.
    if let Some(f) = is_break {
        if f(line) {
            return line;
        }
        let mut i = line;
        while i > 0 {
            i -= 1;
            if f(i) {
                return i;
            }
        }
        // No boundary found above (work opens mid-buffer without one) — fall
        // back to the speaker back-up so we still land on a sane page top.
        return back_up_for_speaker(buffer, line, is_break);
    }
    // Legacy text-inference fallback (mid-load).
    let mut marker = line;
    let mut i = line;
    while i > 0 {
        i -= 1;
        let text = buffer_line_text(buffer, i);
        let t = text.trim();
        if line_types::is_act_scene_marker(t) || line_types::is_separator(t) {
            marker = i;
            continue;
        }
        if marker != line {
            break;
        }
    }
    if marker == line {
        return back_up_for_speaker(buffer, line, is_break);
    }
    while marker > 0 {
        let text = buffer_line_text(buffer, marker - 1);
        let t = text.trim();
        if line_types::is_act_scene_marker(t) || line_types::is_separator(t) || t.is_empty() {
            marker -= 1;
        } else {
            break;
        }
    }
    marker
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
    if state.column_count() == 2 {
        // Table mode renders the stored spread; the check must read the same
        // boundary or sync's past-the-end test never matches the screen.
        if let Some(end) = crate::input::page_table::table_end_for_top(state, top) {
            return end;
        }
        return column_split(state, top).page_end;
    }
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
    let usable_height = widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    range.last_fit
}

/// Result of splitting a page into two columns. Lines `[page_top .. split-1]`
/// fill the left column; `[split .. page_end]` fill the right column;
/// `next_page_top` is the first line of the following page (== line_count when
/// this is the last page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnSplit {
    pub(crate) split: usize,
    pub(crate) page_end: usize,
    pub(crate) next_page_top: usize,
}

/// Find the last buffer line that fits within the viewport starting from
/// `top`, matching the bottom clip calculation exactly. A line is included
/// only if its full height fits in the remaining usable space (widget height
/// minus descender guard). This ensures page_forward doesn't count clipped
/// lines as "seen". Trailing speaker names and blanks are trimmed so a
/// dangling speaker at the bottom doesn't count as "visible" content.
pub(crate) fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    if state.column_count() == 2 {
        // Table mode renders the stored spread; see table_end_for_top.
        if let Some(end) = crate::input::page_table::table_end_for_top(state, top) {
            return end;
        }
        return column_split(state, top).page_end;
    }
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let mut usable_height = widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN;
    let is_prose = state.is_prose();
    // Single-column prose row-fill: a page may start mid-paragraph
    // (`page_top_offset > 0`) on ANY line — over-tall or normal-height. When
    // asked about the CURRENT page top, `visible_range` charges `top` its FULL
    // wrapped height, but the leading `page_top_offset` pixels are above the
    // viewport, so the top line's remaining height is what competes for the
    // budget. Add the offset to `usable_height` (equivalent to reducing the
    // first line's charged height by it — the same adjustment
    // `is_line_start_visible` makes) so the walk counts the right number of
    // lines. Two-column already returned above; non-prose keeps offset 0.
    if is_prose && top == state.page_top.line() && state.page_top.offset() > 0 {
        usable_height += state.page_top.offset();
    }
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    // Trim because this function feeds page-boundary placement decisions
    // (next_page_top): we don't want to put a partial verse stanza or
    // dangling speaker at the BOTTOM of the new page.
    let sec = state.section_starts();
    let bf = sec.map(section_break_fn);
    let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);
    let stage_lookup = |bi: usize| {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let trimmed = trim_visible_range_opts(
        range, top, &state.text_view, &state.buffer, is_prose, false, is_break, &stage_lookup,
    );
    trimmed.last_fit
}

/// GTK-bound two-column split: measures pixel heights per column against the
/// left view (`state.text_view`) and right view (`state.right_view`), which
/// share one buffer. Returns where the right column starts (`split`), where the
/// page ends (`page_end`), and the next page top. Single-column callers should
/// not use this.
pub(crate) fn column_split(state: &AppState, page_top: usize) -> ColumnSplit {
    let line_count = state.effective_line_count();
    if line_count == 0 || page_top >= line_count {
        return ColumnSplit { split: page_top, page_end: page_top, next_page_top: line_count };
    }
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let is_prose = state.is_prose();
    // Authoritative section-boundary predicate (DB div columns), shared by both
    // columns and the "right column begins a new scene" check below.
    let sec = state.section_starts();
    let bf = sec.map(section_break_fn);
    let is_break = bf.as_ref().map(|f| f as &dyn Fn(usize) -> bool);

    // Left column. Unlike a single-column page, a dialogue/verse block (or a
    // stage direction) that doesn't fully fit at the bottom is NOT a problem
    // here — it simply continues at the top of the right column. So fill the
    // left column maximally and trim only what would look broken at the split:
    // a section break, and a trailing BARE SPEAKER NAME or trailing blanks (a
    // speaker must lead its dialogue in the right column). We deliberately do
    // NOT trim stage directions or run the block-atom trim, both of which
    // otherwise leave the left column badly underfilled.
    let left_h = state.text_view.height();
    let left = if left_h > 0 {
        let guard = descender_guard_px(&state.text_view, page_top);
        let usable = left_h - guard - super::scroll::TWO_COLUMN_BOTTOM_MARGIN;
        // Keep multi-line stage directions atomic across the split: if the
        // left column would end partway through a `[...]` block (the next line
        // continues the same unclosed bracket), back up to before the block so
        // the whole stage direction moves to the top of the right column
        // instead of being orphaned ("[They put Bassianus' body in the pit and"
        // on the left, "exit, carrying off Lavinia.]" on the right).
        let r = visible_range(&state.text_view, &state.buffer, page_top, line_count, usable);
        let r = clamp_at_section_break(r, page_top, &state.text_view, &state.buffer, is_break);
        let r = trim_trailing_speaker_only(r, page_top, &state.text_view, &state.buffer);
        back_up_off_partial_stage_direction(r, page_top, &state.text_view, &state.buffer)
    } else {
        // Layout not ready — degenerate single-line range so we don't panic.
        visible_range(&state.text_view, &state.buffer, page_top, line_count, 1)
    };
    let split = (left.last_fit + 1).min(line_count);
    if split >= line_count || left.count == 0 {
        return ColumnSplit { split, page_end: left.last_fit, next_page_top: line_count };
    }

    // If the RIGHT column would BEGIN with a new section — i.e. skipping leading
    // blanks/exit stage-directions from `split` lands on an ACT/SCENE marker or
    // separator — then the previous scene has ENDED within the left column and
    // the right column would be an entirely new scene. End the page here so the
    // new scene starts the NEXT spread (the "stop at scene break" model): the
    // left column shows the scene's tail, the right column is empty, and
    // page_end runs through the trailing exit/blank lines up to (not into) the
    // marker. This is what makes `y` from a scene's first page land on this page
    // EXACTLY (split == division_marker == that page's next_page_top). (The genuine
    // final section — EPILOGUE — is exempt: it has nowhere to be pushed, and
    // last_page_top/G expect it to fill the right column; detected by the section
    // running to the end of the work.)
    {
        use crate::db::line_types;
        // Skip leading blanks / exit stage-directions from `split`.
        let mut hi = split;
        while hi < line_count {
            let t = buffer_line_text(&state.buffer, hi);
            let t = t.trim();
            if t.is_empty() || line_types::is_stage_direction(t) {
                hi += 1;
            } else {
                break;
            }
        }
        // Does the right column OPEN on a section boundary? Authoritative path:
        // `is_break(hi)`. Mid-load fallback: legacy marker/separator text check.
        let at_break = hi < line_count
            && match is_break {
                Some(is_break) => is_break(hi),
                None => {
                    let t = buffer_line_text(&state.buffer, hi);
                    let t = t.trim();
                    line_types::is_act_scene_marker(t) || line_types::is_separator(t)
                }
            };
        // Exempt the work's FINAL trailing section (EPILOGUE): it has nowhere to
        // be pushed, and last_page_top/G expect it in the right column. Detected
        // by "no further section boundary after `hi`" (it is the last section).
        let is_final_section = at_break
            && match is_break {
                Some(is_break) => !((hi + 1)..line_count).any(is_break),
                None => !((hi + 1)..line_count).any(|i| {
                    let t = buffer_line_text(&state.buffer, i);
                    line_types::is_act_scene_marker(t.trim())
                }),
            };
        // Anthology works pack excerpts continuously: each excerpt is its own
        // tiny section, so the play "stop at scene break" model would waste the
        // right column on every excerpt (one-excerpt-per-spread). Skip it so the
        // right column simply begins the next excerpt and both columns fill.
        if at_break && !is_final_section && !state.is_anthology() {
            // FIRST-SPREAD MIRROR OF THE EPILOGUE RULE. On the very first spread
            // (`page_top == 0`), a short COMPLETE opening section (Prologue,
            // Induction, Chorus, or a brief opening scene) that ends at the first
            // section boundary `hi` would otherwise sit alone in the LEFT column
            // with an empty right. Instead place it in the RIGHT column and leave
            // the LEFT empty — the start-of-document mirror of the EPILOGUE
            // short-tail rule (which fills the right column rather than leaving a
            // lonely left). Requires the whole section `[0, hi)` to fit the RIGHT
            // view's height; otherwise fall through to the normal empty-right end.
            // Tiling is preserved: `next_page_top` stays `hi` (the marker), exactly
            // as the empty-right branch below — only the visual placement within
            // this first spread changes, never a spread boundary.
            if page_top == 0 {
                let right_h = state.right_view.height();
                let fits = if right_h > 0 {
                    let guard = descender_guard_px(&state.right_view, 0);
                    let usable = right_h - guard - super::scroll::TWO_COLUMN_BOTTOM_MARGIN;
                    let r = visible_range(&state.right_view, &state.buffer, 0, line_count, usable);
                    // The whole opening section [0, hi) fits the right column.
                    r.last_fit + 1 >= hi
                } else {
                    false
                };
                if fits {
                    // split = 0 → empty left column; right view renders [0, hi).
                    let page_end = hi.saturating_sub(1);
                    return ColumnSplit { split: 0, page_end, next_page_top: hi };
                }
            }
            // The right column would begin a NEW (non-final) scene → end the page
            // here. Left column shows the scene's tail (through the exit/blanks at
            // split..hi-1); the marker at `hi` starts the next spread. `split` is
            // unchanged so the right column simply renders nothing past page_end.
            let page_end = hi.saturating_sub(1).max(split.saturating_sub(1));
            return ColumnSplit { split, page_end, next_page_top: hi };
        }
    }

    // Anthology packing: the left column clamps at the end of one excerpt, so
    // `split` lands on the trailing blank lines BEFORE the next excerpt's title.
    // Starting the right column there makes its own section-break clamp stop
    // after a single blank (right.count == 1, a visually empty column — the bug).
    // Advance `split` past those blanks to the next section start so the right
    // column BEGINS the next excerpt and fills with it. (Plays keep the
    // "stop at scene break" model above and never reach here at a break.)
    let mut split = split;
    if state.is_anthology() {
        let mut hi = split;
        while hi < line_count && !state.is_section_start(hi) {
            let t = buffer_line_text(&state.buffer, hi);
            if t.trim().is_empty() {
                hi += 1;
            } else {
                break;
            }
        }
        if hi < line_count && hi > split && state.is_section_start(hi) {
            split = hi;
        }
    }

    // Right column (measure against the right view).
    let right_h = state.right_view.height().max(left_h);
    let right = if right_h > 0 {
        let guard = descender_guard_px(&state.right_view, split);
        let usable = right_h - guard - super::scroll::TWO_COLUMN_BOTTOM_MARGIN;
        let r = visible_range(&state.right_view, &state.buffer, split, line_count, usable);
        // Clamp the right column at a section break so a new ACT/SCENE starts the
        // NEXT spread rather than appearing in this column's interior. This makes
        // pages tile at scene boundaries: a page that ENDS a scene stops at the
        // scene's last line, so `y` from the next scene's first page lands here
        // exactly (no gap, no overlap into the scene the reader just left).
        // EXCEPTION: if clamping would EMPTY the right column AND the section is
        // the work's FINAL one (the unclamped range already reaches the end —
        // e.g. a trailing EPILOGUE), do NOT clamp: that tail is this spread's
        // right-column content, not a mid-column intrusion. (Without the
        // exception, `would_empty_right_column` treats the canonical final spread
        // as empty and G/last_page_top skip it.)
        let clamped = clamp_at_section_break(r, split, &state.right_view, &state.buffer, is_break);
        let r = if clamped.last_fit < split && r.last_fit + 1 >= line_count {
            r
        } else {
            clamped
        };
        // Right column is the bottom of the spread: relax the underfill guard
        // so a too-tall block splits across the boundary to fill the column
        // rather than leaving a mid-spread gap.
        trim_visible_range_opts(r, split, &state.right_view, &state.buffer, is_prose, true, is_break, &stage_lookup)
    } else {
        visible_range(&state.right_view, &state.buffer, split, line_count, 1)
    };
    // The page ends at `right.last_fit` (trimmed to the last DIALOGUE line). The
    // lines immediately after it may be a pure non-dialogue tail — a scene's
    // trailing `[They exit.]` / blank that the trim correctly dropped. Such a
    // tail is NOT a page of its own, so the next page top must skip it to the
    // next real page (the next dialogue, backed up to its scene heading).
    // Without this, `prev_page_top` would tile a tiny dialogue-less tail spread
    // (the AWW Scene-1→2 underfill). Only skip when everything between is
    // non-dialogue; otherwise advance one line as before.
    let raw_next = (right.last_fit + 1).min(line_count);
    let next_dlg = next_dialogue_from(&state.buffer, raw_next, line_count, state.is_prose(), &stage_lookup);
    let next_top = if next_dlg > raw_next
        && next_dlg < line_count
        && (raw_next..next_dlg).all(|l| !is_dialogue_line(&state.buffer, l, state.is_prose(), &stage_lookup))
    {
        // Back the next page's top up to the next dialogue's scene heading so the
        // page tiles at the boundary instead of mid-tail. BUT this back-up must
        // never move the next top to or before THIS page's top: when a scene's
        // header + entrance fills a spread but its first spoken line doesn't fit
        // (H8 1.4 at a short viewport — `Scene 4` / `=====` / `[Trumpets…]` /
        // `WOLSEY` fill the page, the first dialogue is pushed off), the next
        // dialogue's heading IS this page's own boundary, so backing it up lands
        // on `page_top` and the forward chain stalls (`next_top == page_top`) —
        // jump_to_end/G then strand at this page mid-work. A forward step must
        // advance: if the backed-up top doesn't clear `page_top`, fall back to
        // `raw_next` (just past page_end), which always progresses.
        let backed = back_up_for_speaker_state(state, next_dlg);
        if backed > page_top { backed } else { raw_next }
    } else {
        raw_next
    };
    ColumnSplit { split, page_end: right.last_fit, next_page_top: next_top }
}

/// True when a two-column spread starting at `top` would have an EMPTY right
/// column — i.e. all remaining content fits in the left column, so the right
/// column would render blank. Used to suppress a forward page turn onto the
/// work's short tail (e.g. a lone EPILOGUE): the tail should stay as the right
/// column of the prior spread rather than becoming a lonely left column.
pub(crate) fn would_empty_right_column(state: &AppState, top: usize) -> bool {
    let line_count = state.effective_line_count();
    if top >= line_count {
        return true;
    }
    let cs = column_split(state, top);
    // No right column when the split is at/after the end, or the right range is
    // empty (page_end falls before split).
    cs.split >= line_count || cs.page_end < cs.split
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
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };
    let last_visible = last_fully_visible_line(state, top);
    // A SECTION boundary (chapter/scene) must BEGIN a page — never appear part
    // way down one. If a section start falls strictly inside this page, end the
    // page there so the next page opens on the heading. Authoritative metadata
    // via `is_section_start` (the `(div1,div2)` bitmap), never inferred from the
    // text.
    //
    // Only the single-column path needs this: `column_split` already clamps at
    // breaks for the two-column reader, and `page_table`/`prose_pages` bake the
    // boundary into the stored spreads. This is the LIVE engine, which prose
    // falls back to whenever the pinned table is dropped — e.g. cycling the font
    // changes the layout fingerprint, and Bleak House then rendered
    // "CHAPTER II / In Fashion" mid-page.
    for line in (top + 1)..=last_visible.min(line_count.saturating_sub(1)) {
        if state.is_section_start(line) {
            return NextPage { new_top: line, next_dialogue: line };
        }
    }
    let last = last_dialogue_in_page(
        &state.buffer,
        top,
        last_visible.saturating_sub(top) + 1,
        line_count,
        state.is_prose(),
        &stage_lookup,
    );
    let next_dialogue = next_dialogue_from(&state.buffer, last + 1, line_count, state.is_prose(), &stage_lookup);
    if next_dialogue >= line_count {
        return NextPage { new_top: line_count, next_dialogue: line_count };
    }
    // Back the next dialogue up to its scene heading so the next page tiles at the
    // boundary. A forward step MUST advance, though: when a scene's header +
    // entrance fills the spread but its first spoken line is pushed off (H8 1.4 at
    // a short viewport — `Scene 4` / `=====` / `[Trumpets…]` / `WOLSEY` fill the
    // page, the dialogue doesn't fit), the next dialogue's heading IS this page's
    // own boundary, so the back-up lands on `top` and the forward chain stalls
    // (`new_top == top`). `last_page_top`/G walk this chain and would strand
    // mid-work. If the backed-up top doesn't clear `top`, advance to just past the
    // visible page instead. (Mirrors the same guard in `column_split`.)
    let backed = back_up_for_speaker_state(state, next_dialogue);
    let new_top = if backed > top { backed } else { (last_visible + 1).min(line_count) };
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
    let stage_lookup = |bi: usize| -> Option<i64> {
        state.work_line_for_buffer(bi)
            .and_then(|wi| state.current_work.as_ref()?.lines.get(wi))
            .map(|l| l.sub_line)
    };

    // Tier 1: F8 cache fast path.
    {
        let cached = state.page_tops.borrow();
        if let Some(tops) = cached.as_ref() {
            if let Ok(idx) = tops.binary_search(&current_top) {
                if idx > 0 {
                    // Return the cached boundary VERBATIM (it tiles); don't re-derive
                    // via back_up_for_speaker (the y-GAP shift). See Tier 2 below.
                    let prev_top = tops[idx - 1];
                    let next_dialogue = next_dialogue_from(&state.buffer, prev_top, line_count, state.is_prose(), &stage_lookup);
                    return NextPage { new_top: prev_top, next_dialogue };
                }
            }
        }
    }

    // Tier 2: walk the forward chain from 0. Track the boundary that tiles into
    // current_top: the page `top` whose forward boundary REACHES current_top
    // (next_page_top(top) >= current_top). When current_top is itself a natural
    // boundary, that's the page with next == current_top. When current_top was
    // pulled forward off the natural chain (the EPILOGUE final spread), the
    // correct predecessor is the natural page whose forward boundary lands ON OR
    // PAST current_top — paging forward from it reaches current_top's content,
    // so paging back lands a full page earlier (no overlap, no barely-moved page).
    // CRITICAL: return the page `top` we land on VERBATIM as `new_top` — it is a
    // real forward-chain boundary, so `next_page_top(top)` tiles exactly into
    // `current_top`. Do NOT re-derive `new_top` via
    // `back_up_for_speaker(next_dialogue_from(top))`: that shifts the top off the
    // boundary by the speaker back-up, leaving a ~3-line GAP (forward ends a page
    // before a speaker; the re-derivation lands after it) — the dominant
    // `y GAP delta=3` bug. `next_dialogue` is only the cursor hint.
    // Walk the forward chain looking for the page that tiles into current_top:
    // the boundary `b` whose forward page ENDS exactly at current_top
    // (`next_page_top(b) == current_top`). If the chain skips current_top (it was
    // reached by a scene-jump / y / startup-snap, not forward paging), there is no
    // exact match — then return the LAST boundary whose forward page does not
    // overshoot (`next_page_top(b) <= current_top`); its page runs up toward
    // current_top with the smallest possible gap, and crucially never OVERLAPS
    // (returning a boundary whose `next > current_top` would show lines twice).
    let mut top: usize = 0;          // candidate to return (best so far)
    let mut probe: usize = 0;        // walk cursor
    // Use the SAME forward boundary the renderer and the page-tiling invariant
    // use: in two columns that is `column_split(probe).next_page_top` (the right
    // column's `last_fit + 1`), which can differ from the single-column
    // `next_page_top()` by a trailing speaker block — that mismatch was the
    // residual 3-line `y GAP` (backward tiled against next_page_top while the
    // page actually rendered to column_split's boundary).
    let two_col = state.column_count() == 2;
    let fwd_boundary = |b: usize| -> usize {
        if two_col { column_split(state, b).next_page_top } else { next_page_top(state, b).new_top }
    };
    loop {
        let next = fwd_boundary(probe);
        if next == current_top {
            // Exact tile — `probe` is the page directly before current_top.
            let next_dialogue = next_dialogue_from(&state.buffer, probe, line_count, state.is_prose(), &stage_lookup);
            return NextPage { new_top: probe, next_dialogue };
        }
        if next > current_top || next <= probe {
            // `probe`'s page overshoots current_top (or no forward progress) — it
            // would OVERLAP. Keep the last non-overshooting candidate (`top`).
            // With the right-column section clamp restored, scene-start tops are
            // now real boundaries (`column_split(prev).next_page_top == division_top`),
            // so the `next == current_top` exact-tile case above handles them and
            // we rarely reach here with a gap.
            break;
        }
        // `next <= current_top - 1`: `probe`'s page ends before current_top, so
        // `probe` is a valid non-overlapping candidate. Record it and advance.
        top = probe;
        probe = next;
    }
    // `top` is the latest boundary whose page does not overshoot current_top
    // (worst case 0), and showing it skips no dialogue.
    let next_dialogue = next_dialogue_from(&state.buffer, top, line_count, state.is_prose(), &stage_lookup);
    NextPage { new_top: top, next_dialogue }
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

/// The canonical page-top whose spread CONTAINS `target_line` — i.e. the
/// `next_page_top` boundary you'd reach by paging forward to `target_line`. This
/// is the spread that actually DISPLAYS the line (in either column), as opposed
/// to `current_line - 1` which forces the line to the top-left and renders a
/// different, often near-empty spread. Used to reconstruct the saved reading
/// spread on resume: only the cursor line is persisted, so the page boundary
/// must be recomputed, and it must be the one that shows the cursor where the
/// user left it (e.g. the right column of a two-column play spread).
///
/// Returns 0 before layout (height ≤ 0), when the index can't be built.
pub fn page_top_containing(state: &AppState, target_line: usize) -> usize {
    let page = viewport_page_for_line(state, target_line); // 1-indexed
    let cached = state.page_tops.borrow();
    match cached.as_ref() {
        Some(tops) if !tops.is_empty() => {
            // page_for_line_in_index returns ≥1; tops[page-1] is the boundary.
            tops[(page - 1).min(tops.len() - 1)]
        }
        _ => 0,
    }
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
    if line < state.page_top.line() {
        return false;
    }
    if state.column_count() == 2 {
        // Table mode renders the stored spread; see table_end_for_top.
        let end = crate::input::page_table::table_end_for_top(state, state.page_top.line())
            .unwrap_or_else(|| column_split(state, state.page_top.line()).page_end);
        return line >= state.page_top.line() && line <= end;
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
    let descender_guard = descender_guard_px(&state.text_view, state.page_top.line());
    let usable_height = widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN;
    let line_count = state.effective_line_count();
    let range = visible_range(
        &state.text_view,
        &state.buffer,
        state.page_top.line(),
        line_count,
        usable_height,
    );
    line <= range.last_fit && range.count > 0
}

/// True when `line`'s FIRST visual row is inside the current viewport.
///
/// `is_line_fully_visible` charges a buffer line its FULL wrapped height via
/// `visible_range`, which is right for "did this line finish rendering on the
/// current page" but wrong for an over-tall prose paragraph: such a paragraph
/// starting exactly at `page_top_line` has height greater than `usable_height`,
/// so `visible_range` stops with `count == 0` on its very first line even
/// though the paragraph's opening row IS on screen — `is_line_fully_visible`
/// would (incorrectly) say "not visible" and trigger a redundant page turn.
///
/// This does NOT mean any pixel-band overlap counts as "visible": the paged
/// bottom clip (`update_bottom_clip`) hides everything past the walked
/// `visible_range` total wholesale — a line whose top pixel happens to fall
/// before `y0 + usable_height` but after the real accumulated content height
/// is clipped away entirely, not partially shown. So this mirrors
/// `visible_range`'s own walk (same kernel `is_line_fully_visible` uses),
/// with one adjustment: the first line's charged height is reduced by
/// `page_top_offset` (the part of an over-tall paragraph already consumed by
/// an earlier page), so a later page of the SAME straddling paragraph still
/// correctly counts only its remaining height.
pub(crate) fn is_line_start_visible(state: &AppState, line: usize) -> bool {
    if state.loading_work.get() {
        return true;
    }
    if line < state.page_top.line() {
        return false;
    }
    if line == state.page_top.line() {
        // The paragraph's own first visual row is only ON this page when the
        // page top offset is 0 — a nonzero offset means this page begins
        // partway down the SAME over-tall paragraph, i.e. the paragraph's
        // start rendered on an earlier page.
        return state.page_top.offset() == 0;
    }
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return true;
    }
    let guard = descender_guard_px(&state.text_view, state.page_top.line());
    let usable = widget_height - guard - SINGLE_COLUMN_BOTTOM_MARGIN;
    let line_count = state.effective_line_count();
    // Walk from page_top exactly like `visible_range`'s own break condition
    // (`total + h > usable` => stop, this line and everything after is
    // clipped away wholesale — the renderer never partially reveals a line
    // that doesn't fit), except the first line's charged height is reduced by
    // the already-consumed offset so a later page of the SAME straddling
    // paragraph only counts its remaining height.
    let mut total: i32 = 0;
    for i in state.page_top.line()..line_count {
        let Some(iter) = state.buffer.iter_at_line(i as i32) else { break };
        let (_y, h) = state.text_view.line_yrange(&iter);
        let charged = if i == state.page_top.line() {
            (h - state.page_top.offset()).max(0)
        } else {
            h
        };
        if total + charged > usable {
            return false; // `i` (and `line`, if not yet reached) is clipped
        }
        total += charged;
        if i == line {
            return true;
        }
    }
    false
}

/// Count how many buffer lines are fully visible starting from `page_top_line`.
/// Returns a calibrated estimate (35) during work load when GTK layout is
/// invalid, and a small fallback (15) for empty or past-end buffers.
pub(crate) fn lines_per_page(state: &AppState) -> usize {
    if state.loading_work.get() {
        return 35;
    }

    let line_count = state.effective_line_count();
    let start = state.page_top.line();

    if line_count == 0 || start >= line_count {
        return 15;
    }

    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return 15;
    }

    let descender_guard = descender_guard_px(&state.text_view, start);
    let usable_height = widget_height - descender_guard - SINGLE_COLUMN_BOTTOM_MARGIN;
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

/// The active font's internal leading at the TOP of a line: how far the tallest
/// realistic glyph ink sits below the line box's logical top, measured by
/// laying out a probe of the tallest glyphs (brackets, quotes, ascenders,
/// accented caps) and comparing Pango's ink vs logical extents. This is the
/// strip at the top of every line that is guaranteed blank even with
/// `pixels_above_lines = 0` (the verse/tight-spacing case) — the paged bottom
/// clip may drop its top edge into the NEXT line's box by at most this much
/// without revealing the next line's ascenders. Same font source as
/// `descender_guard_px` (the `font-size` tag) for the same CSS-race reason.
/// Clamped to `[0, 12]`; a failed probe returns 0 (no reveal — the safe side).
pub(crate) fn ascent_ink_gap_px(text_view: &sourceview5::View) -> i32 {
    use gtk4::prelude::{TextTagExt, TextBufferExt, WidgetExt};
    let ctx = text_view.pango_context();
    let font_desc = text_view
        .buffer()
        .tag_table()
        .lookup("font-size")
        .and_then(|tag| tag.font_desc());
    let layout = pango::Layout::new(&ctx);
    if let Some(fd) = font_desc.as_ref() {
        layout.set_font_description(Some(fd));
    }
    // Tallest glyphs that realistically start a line: brackets (stage
    // directions), quotes, full ascenders, accented capitals. If the actual
    // next line opens with something shorter, the real blank strip is larger —
    // erring toward less reveal, never more.
    layout.set_text("[\"'bdfhklÉÀ|]?!");
    let (ink, logical) = layout.extents();
    ((ink.y() - logical.y()) / pango::SCALE).clamp(0, 12)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod underfill_guard_tests {
    use super::should_revert_underfill;

    #[test]
    fn never_reverts_when_layout_not_ready() {
        assert!(!should_revert_underfill(100, 0, false));
        assert!(!should_revert_underfill(100, -1, true));
    }

    #[test]
    fn single_column_keeps_moderate_underfill() {
        // 70% full: above the 2/3 single-column threshold → keep the trim
        // (block moves to the next full page, current page-turn intended).
        let widget = 1000;
        assert!(!should_revert_underfill(700, widget, false));
        // 60% full: below 2/3 → revert to per-line split.
        assert!(should_revert_underfill(600, widget, false));
    }

    #[test]
    fn right_column_reverts_at_moderate_underfill() {
        // The Image #4 case: right column left ~70% full by an atomic block.
        // Single column tolerates this; the right column (relax) must revert
        // so the block splits across the boundary and fills the column.
        let widget = 1000;
        assert!(should_revert_underfill(700, widget, true)); // 70% < 90% → split
        assert!(should_revert_underfill(890, widget, true)); // 89% < 90% → split
        assert!(!should_revert_underfill(900, widget, true)); // 90% → full, keep
        assert!(!should_revert_underfill(950, widget, true)); // 95% → keep
    }
}

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

    fn no_stanza_numbers(_i: usize) -> bool { false }

    #[test]
    fn block_start_in_3line_stage_direction_returns_first_dir_line() {
        // Lines: 0=speaker, 1=dir, 2=dir, 3=dir
        let kinds = ['s', 'd', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-block), page_top=0, is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 1, "should back up to first stage-direction line");
    }

    #[test]
    fn block_start_at_block_start_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=dir
        let kinds = ['s', 'd', 'd'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (at block start), page_top=0
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 1, "no backup when last_fit is already block start");
    }

    #[test]
    fn block_start_in_verse_stanza_non_prose_returns_stanza_start() {
        // Lines: 0=speaker, 1=l, 2=l, 3=l, 4=blank
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3 (mid-stanza), is_prose=false
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 1, "verse stanza in non-prose work backs up to stanza start");
    }

    #[test]
    fn block_start_in_verse_stanza_prose_returns_unchanged() {
        // Same lines, but is_prose=true — rule does not apply.
        let kinds = ['s', 'l', 'l', 'l', 'b'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, true, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 3, "verse stanza rule skipped for prose works");
    }

    #[test]
    fn block_start_single_line_stage_direction_returns_unchanged() {
        // Lines: 0=speaker, 1=dir, 2=l
        let kinds = ['s', 'd', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=1 (single dir line)
        let start = block_start_for_line_pure(0, 1, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 1, "single-line stage direction is not multi-line — no backup");
    }

    #[test]
    fn block_start_stops_at_speaker() {
        // Lines: 0=blank, 1=speaker, 2=l, 3=l (stanza bounded above by speaker)
        let kinds = ['b', 's', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        // last_fit=3, page_top=0
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 2, "stanza backup stops at speaker line (returns first dialogue line)");
    }

    #[test]
    fn block_start_blank_bounded_stanza_is_allowed_to_split() {
        // Lines: 0=l, 1=blank, 2=l, 3=l (stanza bounded above by blank).
        // A plain blank-delimited verse paragraph (continuous epics like the
        // Odyssey) is NOT kept atomic — it may split to fill the viewport, so
        // block_start returns last_fit unchanged.
        let kinds = ['l', 'b', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 3, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 3, "blank-bounded verse stanza is allowed to split (returns last_fit)");
    }

    #[test]
    fn block_start_stage_bounded_stanza_is_allowed_to_split() {
        // Lines: 0=stage, 1=l, 2=l (stanza after a stage direction).
        // Not bounded by a speaker or stanza number, so it is not a structured
        // block worth keeping atomic — it may split (returns last_fit).
        let kinds = ['d', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let start = block_start_for_line_pure(0, 2, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &no_stanza_numbers);
        assert_eq!(start, 2, "stage-bounded verse stanza is allowed to split (returns last_fit)");
    }

    #[test]
    fn trim_block_atoms_block_fully_fits_reduces_count() {
        // Page with 15 lines; stage block at lines 13-15 split mid-block at
        // line 14. Trim removes 2 lines (~60px from ~450px = 13% removed).
        let range = VisibleRange { last_fit: 14, total_height: 450, count: 15 };
        // Lines 0-12 = dialogue, 13-15 = stage direction block
        let mut kinds = vec!['l'; 13];
        kinds.extend_from_slice(&['d', 'd', 'd']);
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |_i: usize| 30;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
        );
        assert_eq!(trimmed.last_fit, 12);
        assert_eq!(trimmed.count, 13);
        assert_eq!(trimmed.total_height, 390); // dropped lines 13 and 14 at 30px each
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
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
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
        );
        assert_eq!(trimmed.last_fit, 5);
        assert_eq!(trimmed.count, 1);
    }

    #[test]
    fn block_start_includes_stanza_number_above_verse() {
        // Buffer: 0=blank, 1=stanza_num, 2=blank, 3=l, 4=l, 5=l
        let kinds = ['b', 'n', 'b', 'l', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let is_sn = |i: usize| kinds.get(i).map_or(false, |c| *c == 'n');
        let start = block_start_for_line_pure(0, 5, false, &is_blank, &is_speaker, &is_stage, &is_dialogue, &is_sn);
        assert_eq!(start, 1, "block should extend back to include stanza number");
    }

    #[test]
    fn trim_block_atoms_trailing_stanza_number_is_trimmed() {
        // Buffer: 0=l, 1=l, 2=l, 3=l, 4=l, 5=l, 6=l, 7=blank, 8=stanza_num
        // last_fit=8 (stanza number at bottom of page after trim_trailing_speakers
        // stripped trailing blank). The stanza number should be trimmed.
        let kinds = ['l', 'l', 'l', 'l', 'l', 'l', 'l', 'b', 'n'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let is_sn = |i: usize| kinds.get(i).map_or(false, |c| *c == 'n');
        let range = VisibleRange { last_fit: 8, total_height: 180, count: 9 };
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &is_sn,
        );
        assert_eq!(trimmed.last_fit, 7, "stanza number should be trimmed from bottom");
        assert_eq!(trimmed.count, 8);
    }

    #[test]
    fn trim_block_atoms_small_block_is_trimmed() {
        // Block at lines 4-5 (2 dialogue lines after speaker at 3); visible
        // range stops at 5 (mid-block — line 6+ off-screen). Removing 2 of 6
        // lines is within the line-count guard (new_count=4, 4*2=8 > 6), so
        // the pure function trims. The viewport-relative guard in
        // trim_visible_range handles underfill at the GTK layer.
        let kinds = ['s', 'l', 'b', 's', 'l', 'l', 'l', 'l'];
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let line_height = |i: usize| if i <= 2 { 10 } else { 40 };
        let range = VisibleRange { last_fit: 5, total_height: 150, count: 6 };
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &no_stanza_numbers,
        );
        assert_eq!(trimmed.last_fit, 3, "block trimmed to before block start");
        assert_eq!(trimmed.total_height, 70); // kept lines 0-3: 10+10+10+40
    }

    #[test]
    fn trim_block_atoms_mid_stanza_with_number_backs_up_to_before_number() {
        // Realistic page: 20 lines of prior content, then stanza_num + blank + verse.
        // Buffer: 0..19=l, 20=blank, 21=stanza_num, 22=blank, 23=l, 24=l, 25=l, 26=l
        // last_fit=24 (mid-stanza), stanza continues at 25+.
        // Block should include stanza_num (21) + blank (22) + verse (23,24).
        // new_last_fit = 20 (blank before stanza number).
        let mut kinds = vec!['l'; 20];
        kinds.extend_from_slice(&['b', 'n', 'b', 'l', 'l', 'l', 'l']);
        let (is_blank, is_speaker, is_stage, is_dialogue) = classifiers(&kinds);
        let is_sn = |i: usize| kinds.get(i).map_or(false, |c| *c == 'n');
        let range = VisibleRange { last_fit: 24, total_height: 500, count: 25 };
        let line_height = |_i: usize| 20;
        let trimmed = trim_block_atoms_pure(
            range, 0, false,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &is_sn,
        );
        assert_eq!(trimmed.last_fit, 20, "should back up past stanza number");
        assert_eq!(trimmed.count, 21);
    }
}

#[cfg(test)]
mod headless_pagination_tests {
    use crate::db::line_types;
    use super::{ColumnSplit, VisibleRange, trim_block_atoms_pure};

    fn clean_text_file(path: &str) -> Vec<String> {
        let contents = std::fs::read_to_string(path).expect("failed to read text file");
        let file_lines: Vec<String> = contents.lines().map(String::from).collect();
        let mut result: Vec<String> = Vec::with_capacity(file_lines.len());
        for (i, line) in file_lines.iter().enumerate() {
            if line_types::is_blank(line) {
                let next_non_blank = file_lines[i + 1..]
                    .iter()
                    .find(|l| !line_types::is_blank(l));
                if let Some(next) = next_non_blank {
                    if line_types::is_speaker(next) {
                        continue;
                    }
                }
            }
            if let Some(stripped) = line.strip_prefix("## ") {
                result.push(stripped.to_string());
            } else {
                result.push(line.clone());
            }
        }
        result
    }

    fn clamp_at_section_break_pure(
        lines: &[String], page_top: usize, last_fit: usize,
    ) -> usize {
        let mut scan_start = page_top + 1;
        while scan_start <= last_fit {
            let trimmed = lines[scan_start].trim();
            if line_types::is_act_scene_marker(trimmed)
                || line_types::is_separator(trimmed)
                || trimmed.is_empty()
                || line_types::is_stage_direction(trimmed)
            {
                scan_start += 1;
            } else {
                break;
            }
        }
        for i in scan_start..=last_fit {
            let trimmed = lines[i].trim();
            if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
                let clamped = i.saturating_sub(1);
                if clamped >= page_top {
                    return clamped;
                }
            }
        }
        last_fit
    }

    /// Bitmap-driven mirror of the AUTHORITATIVE `clamp_at_section_break` path
    /// (the `Some(is_break)` branch): clamp to the line before the first section
    /// boundary strictly after `page_top`. No header-skip — a boundary AT
    /// `page_top` is the page's own opening (we scan from `page_top + 1`).
    fn clamp_at_section_break_bitmap(
        section_starts: &[bool], page_top: usize, last_fit: usize,
    ) -> usize {
        match (page_top + 1..=last_fit).find(|&i| section_starts[i]) {
            Some(b) => b.saturating_sub(1).max(page_top),
            None => last_fit,
        }
    }

    // --- clamp_at_section_break behavior contract -------------------------------
    // These encode the TWO requirements that fought each other at runtime so any
    // future change is verified here, not on a 45-min fuzz sweep:
    //   (A) A page that OPENS with a scene heading must keep its own stacked
    //       title (ACT / === / Scene / === / [Enter …]) and NOT self-clamp to a
    //       tiny page.
    //   (B) A page whose visible range ENDS with a new scene (dialogue, then a
    //       trailing [X exits.]/blank, then an ACT/SCENE marker) must clamp BEFORE
    //       that marker so the new scene starts the next spread (clean `y` tiling).
    // `clamp_at_section_break_pure` mirrors the real `clamp_at_section_break`
    // line-classification exactly; keep them in lockstep.

    fn play_lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn clamp_a_page_opening_on_act_heading_does_not_self_clamp() {
        // Page starts on "ACT 1" with the stacked title; the rest is dialogue.
        let lines = play_lines(&[
            "ACT 1", "=====", "Scene 1", "=======", "[Enter the King.]",
            "KING", "So shaken as we are, so wan with care,",
            "Find we a time for frighted peace to pant",
            "And breathe short-winded accents of new broils",
        ]);
        let last_fit = lines.len() - 1;
        // The only markers/separators are in the opening header block, which must
        // be skipped — so NO clamp: the page fills to last_fit.
        assert_eq!(
            clamp_at_section_break_pure(&lines, 0, last_fit), last_fit,
            "a page opening on ACT 1 must keep its whole title + dialogue, not self-clamp"
        );
    }

    #[test]
    fn clamp_before_a_new_division_that_follows_dialogue() {
        // Right-column-style page: starts mid-scene on dialogue, the scene ENDS a
        // few lines in (dialogue, exit, blank), then the NEXT scene heading.
        let lines = play_lines(&[
            "What I can help thee to thou shalt not miss.",  // 0 dialogue (page_top)
            "Be gone tomorrow, and be sure of this:",        // 1 dialogue
            "",                                              // 2 blank
            "[They exit.]",                                  // 3 stage dir
            "",                                              // 4 blank
            "Scene 2",                                       // 5 NEW scene marker
            "=======",                                       // 6 separator
            "[Enter Lafew.]",                                // 7 stage dir
            "LAFEW",                                         // 8 speaker
            "But I will attend his Majesty's command.",      // 9 dialogue
        ]);
        let last_fit = lines.len() - 1;
        // Must clamp to line 4 (the line BEFORE the Scene 2 marker at 5).
        assert_eq!(
            clamp_at_section_break_pure(&lines, 0, last_fit), 4,
            "a scene ending mid-page must clamp before the next scene's marker"
        );
    }

    // FIXED (the AWW 25-line `y GAP`). The EXACT real pattern: the right column's
    // first line is dialogue, IMMEDIATELY followed by blank / [They exit.] /
    // blank / ACT 2 — no dialogue between. The OLD text heuristic's header-skip
    // (blanks + stage directions + markers from page_top+1) ran straight through
    // the ACT 2 marker and skipped it, so nothing clamped and the right column
    // ran into the next act → `y` from the next scene skipped this scene's tail.
    //
    // The fix is the AUTHORITATIVE boundary bitmap (DB `(div1,div2)`): the
    // boundary is marked on the ACT 2 chrome line (here, line 4) and the clamp is
    // simply "the line before the first boundary strictly after page_top". No
    // header-skip to bridge over, so the marker is no longer skipped. The
    // [They exit.] / blank at 1..3 belong to the ending scene and stay on the
    // page → clamp at 3 (ACT 2 is at 4).
    #[test]
    fn clamp_when_split_dialogue_is_immediately_followed_by_exit_then_marker() {
        // Boundary bitmap: ACT 2 (index 4) is the start of the new (div1,div2)
        // run; everything before it is the ending scene.
        let section_starts = [
            false, // 0 dialogue (page_top, in the ending scene)
            false, // 1 blank
            false, // 2 [They exit.] (ending scene's)
            false, // 3 blank
            true,  // 4 ACT 2  <-- authoritative boundary
            false, // 5 =====
            false, // 6 Scene 1
            false, // 7 [Enter the Duke.]
            false, // 8 second-act dialogue
        ];
        let last_fit = section_starts.len() - 1;
        assert_eq!(
            clamp_at_section_break_bitmap(&section_starts, 0, last_fit), 3,
            "dialogue at split, then exit/blank, then ACT boundary → clamp before the marker"
        );
    }

    #[test]
    fn clamp_bitmap_page_opening_on_boundary_does_not_self_clamp() {
        // A page that OPENS on a boundary (page_top is the ACT chrome line) must
        // keep its whole title + dialogue: the scan starts at page_top+1, so the
        // boundary AT page_top never triggers a clamp.
        let section_starts = [
            true,  // 0 ACT 1 (page_top — this page's own opening)
            false, // 1 =====
            false, // 2 Scene 1
            false, // 3 dialogue
            false, // 4 dialogue
        ];
        assert_eq!(
            clamp_at_section_break_bitmap(&section_starts, 0, 4), 4,
            "a page opening on its own boundary keeps its whole title + dialogue"
        );
    }

    #[test]
    fn clamp_bitmap_later_boundary_inside_range_clamps() {
        // Opening boundary at page_top (skipped), a later boundary at 6 clamps.
        let section_starts = [
            true,  // 0 ACT 1 (page_top)
            false, // 1 =====
            false, // 2 Scene 1
            false, // 3 dialogue
            false, // 4 dialogue
            false, // 5 [Exit.]
            true,  // 6 Scene 2  <-- later boundary clamps here
            false, // 7 dialogue
        ];
        assert_eq!(
            clamp_at_section_break_bitmap(&section_starts, 0, 7), 5,
            "opening boundary skipped; later Scene 2 boundary clamps at 6-1=5"
        );
    }

    #[test]
    fn clamp_keeps_the_divisions_own_trailing_exit_and_blank() {
        // The clamp target is the marker line; the trailing [exit]/blanks belong
        // to the ENDING scene and stay on this page (clamp = marker - 1).
        let lines = play_lines(&[
            "Now is the winter of our discontent",  // 0 dialogue
            "Made glorious summer by this sun of York.", // 1 dialogue
            "[Exit.]",                              // 2 stage dir (this scene's)
            "Scene 2",                              // 3 next scene marker
            "More dialogue here.",                  // 4
        ]);
        assert_eq!(
            clamp_at_section_break_pure(&lines, 0, 4), 2,
            "trailing exit stays with the ending scene; clamp at marker-1"
        );
    }

    #[test]
    fn clamp_no_marker_in_range_returns_last_fit() {
        // Pure dialogue page — nothing to clamp.
        let lines = play_lines(&[
            "A line of dialogue.", "Another line.", "And a third.",
        ]);
        assert_eq!(clamp_at_section_break_pure(&lines, 0, 2), 2);
    }

    #[test]
    fn clamp_page_opening_on_act_then_later_division_clamps_at_the_later_one() {
        // Page opens on ACT 1 (header skipped), then has real dialogue, then a
        // Scene 2 marker further down: the LATER marker must still clamp.
        let lines = play_lines(&[
            "ACT 1", "=====", "Scene 1", "=======",  // 0-3 opening header (skipped)
            "First scene dialogue.",                  // 4 dialogue
            "More of scene one.",                     // 5 dialogue
            "Scene 2",                                // 6 a real later boundary
            "Second scene dialogue.",                 // 7
        ]);
        assert_eq!(
            clamp_at_section_break_pure(&lines, 0, 7), 5,
            "the opening header is skipped, but a later Scene 2 still clamps (at 6-1=5)"
        );
    }

    fn trim_trailing_pure(lines: &[String], page_top: usize, mut last_fit: usize) -> usize {
        while last_fit > page_top {
            let text = &lines[last_fit];
            let trimmed = text.trim();
            if line_types::is_speaker(trimmed)
                || line_types::is_blank(trimmed)
                || line_types::is_stage_direction(trimmed)
            {
                last_fit -= 1;
            } else {
                break;
            }
        }
        last_fit
    }

    fn trim_block_atoms_text(
        lines: &[String], page_top: usize, last_fit: usize,
        is_prose: bool, _line_count: usize,
    ) -> usize {
        let is_blank = |i: usize| lines.get(i).map_or(true, |l| line_types::is_blank(l));
        let is_speaker = |i: usize| lines.get(i).map_or(false, |l| line_types::is_speaker(l));
        let is_stage = |i: usize| lines.get(i).map_or(false, |l| line_types::is_stage_direction(l));
        let is_dialogue = |i: usize| lines.get(i).map_or(false, |l| line_types::is_dialogue(l, is_prose));
        let is_sn = |i: usize| lines.get(i).map_or(false, |l| line_types::is_stanza_number(l));
        let line_height = |_i: usize| 20i32;
        let count = last_fit - page_top + 1;
        let range = VisibleRange {
            last_fit,
            total_height: count as i32 * 20,
            count,
        };
        let trimmed = trim_block_atoms_pure(
            range, page_top, is_prose,
            &is_blank, &is_speaker, &is_stage, &is_dialogue, &line_height, &is_sn,
        );
        trimmed.last_fit
    }

    fn trim_visible_range_pure(
        lines: &[String], page_top: usize, raw_last_fit: usize,
        is_prose: bool,
    ) -> usize {
        let r = clamp_at_section_break_pure(lines, page_top, raw_last_fit);
        let r = trim_trailing_pure(lines, page_top, r);
        let r = trim_block_atoms_text(lines, page_top, r, is_prose, lines.len());
        trim_trailing_pure(lines, page_top, r)
    }

    fn next_dialogue_from_text(lines: &[String], from: usize, _is_prose: bool) -> usize {
        for i in from..lines.len() {
            let text = &lines[i];
            let trimmed = text.trim();
            if !trimmed.is_empty()
                && !line_types::is_speaker(trimmed)
                && !line_types::is_stage_direction(trimmed)
                && !line_types::is_act_scene_marker(trimmed)
                && !line_types::is_separator(trimmed)
            {
                return i;
            }
        }
        lines.len()
    }

    fn last_dialogue_in_range(lines: &[String], from: usize, to: usize, _is_prose: bool) -> usize {
        let mut last = from;
        for i in from..=to {
            let text = &lines[i];
            let trimmed = text.trim();
            if !trimmed.is_empty()
                && !line_types::is_speaker(trimmed)
                && !line_types::is_stage_direction(trimmed)
                && !line_types::is_act_scene_marker(trimmed)
                && !line_types::is_separator(trimmed)
            {
                last = i;
            }
        }
        last
    }

    fn back_up_for_speaker_text(lines: &[String], line: usize) -> usize {
        let mut top = line;
        while top > 0 {
            let trimmed = lines[top - 1].trim();
            if line_types::is_act_scene_marker(trimmed) || line_types::is_separator(trimmed) {
                top -= 1;
                while top > 0 {
                    let above = lines[top - 1].trim();
                    if line_types::is_act_scene_marker(above)
                        || line_types::is_separator(above)
                        || above.is_empty()
                    {
                        top -= 1;
                    } else {
                        break;
                    }
                }
                break;
            }
            let is_entrance_stage_dir = line_types::is_stage_direction(trimmed) && {
                if top >= 2 {
                    let above_t = lines[top - 2].trim();
                    above_t.is_empty()
                        || line_types::is_speaker(above_t)
                        || line_types::is_stage_direction(above_t)
                        || line_types::is_act_scene_marker(above_t)
                        || line_types::is_separator(above_t)
                } else {
                    true
                }
            };
            if trimmed.is_empty()
                || line_types::is_speaker(trimmed)
                || is_entrance_stage_dir
            {
                top -= 1;
            } else {
                break;
            }
        }
        top
    }

    struct PageResult {
        page_top: usize,
    }

    fn page_forward(
        lines: &[String], page_top: usize, lines_per_page: usize, is_prose: bool,
    ) -> Option<PageResult> {
        let line_count = lines.len();
        let raw_last_fit = (page_top + lines_per_page - 1).min(line_count - 1);
        let last_visible = trim_visible_range_pure(lines, page_top, raw_last_fit, is_prose);
        let last_dialogue = last_dialogue_in_range(lines, page_top, last_visible, is_prose);
        let next_dialogue = next_dialogue_from_text(lines, last_dialogue + 1, is_prose);
        if next_dialogue >= line_count {
            return None;
        }
        let new_top = back_up_for_speaker_text(lines, next_dialogue);
        Some(PageResult {
            page_top: new_top,
        })
    }

    fn validate_page_top(lines: &[String], page_top: usize, page_num: usize, is_prose: bool) -> Vec<String> {
        let mut errors = Vec::new();
        let text = &lines[page_top];
        let trimmed = text.trim();

        if line_types::is_stage_direction(trimmed) && page_top > 0 {
            let has_context = (page_top.saturating_sub(2)..page_top).any(|i| {
                let t = lines[i].trim();
                line_types::is_speaker(t) || line_types::is_stage_direction(t)
            });
            if !has_context {
                errors.push(format!(
                    "page {}: top line {} is dangling stage direction '{}' without speaker context",
                    page_num, page_top, trimmed
                ));
            }
        }

        // Orphaned verse line: only in stanza-numbered regions.
        // A dialogue line at page_top whose preceding line is also dialogue
        // means we're mid-stanza — the page should start at the stanza boundary.
        // Only check if a stanza number exists ABOVE page_top (we're inside
        // a numbered section, not in a prose introduction before it).
        if !is_prose && page_top > 0
            && line_types::is_dialogue(trimmed, false)
            && !line_types::is_stanza_number(trimmed)
        {
            let in_stanza_region = (0..page_top).rev()
                .take(200)
                .any(|i| line_types::is_stanza_number(&lines[i]));
            if in_stanza_region {
                let prev = lines[page_top - 1].trim();
                if line_types::is_dialogue(prev, false)
                    && !line_types::is_stanza_number(prev)
                {
                    let trunc = |s: &str| -> String {
                        s.chars().take(50).collect()
                    };
                    errors.push(format!(
                        "page {}: top line {} is orphaned mid-stanza verse '{}' (prev: '{}')",
                        page_num, page_top, trunc(trimmed), trunc(prev)
                    ));
                }
            }
        }

        errors
    }

    fn validate_page_bottom(lines: &[String], last_visible: usize, page_num: usize) -> Vec<String> {
        let mut errors = Vec::new();
        let text = &lines[last_visible];
        let trimmed = text.trim();

        if line_types::is_speaker(trimmed) {
            errors.push(format!(
                "page {}: bottom line {} is dangling speaker '{}'",
                page_num, last_visible, trimmed
            ));
        }

        errors
    }

    struct PaginationResult {
        pages: usize,
        errors: Vec<String>,
    }

    fn run_pagination_test(path: &str, is_prose: bool, lines_per_page: usize) -> PaginationResult {
        let lines = clean_text_file(path);
        assert!(!lines.is_empty(), "text file is empty: {}", path);

        let mut page_tops: Vec<usize> = Vec::new();
        let mut all_errors: Vec<String> = Vec::new();

        let mut page_top = 0usize;
        let mut page_num = 1usize;
        loop {
            page_tops.push(page_top);

            let raw_last_fit = (page_top + lines_per_page - 1).min(lines.len() - 1);
            let last_visible = trim_visible_range_pure(&lines, page_top, raw_last_fit, is_prose);

            all_errors.extend(validate_page_top(&lines, page_top, page_num, is_prose));
            all_errors.extend(validate_page_bottom(&lines, last_visible, page_num));

            match page_forward(&lines, page_top, lines_per_page, is_prose) {
                Some(result) => {
                    assert!(
                        result.page_top > page_top,
                        "{}: page {} did not advance: page_top stayed at {}",
                        path, page_num, page_top
                    );
                    page_top = result.page_top;
                    page_num += 1;
                }
                None => break,
            }
        }

        let fwd_page_count = page_num;

        for (i, &pt) in page_tops.iter().enumerate().rev() {
            let raw_last_fit = (pt + lines_per_page - 1).min(lines.len() - 1);
            let last_visible = trim_visible_range_pure(&lines, pt, raw_last_fit, is_prose);
            all_errors.extend(validate_page_top(&lines, pt, i + 1, is_prose));
            all_errors.extend(validate_page_bottom(&lines, last_visible, i + 1));
        }

        PaginationResult { pages: fwd_page_count, errors: all_errors }
    }

    fn discover_works(author_path_fragment: &str) -> Vec<(String, String, String)> {
        let db_path = std::path::Path::new(
            &std::env::var("HOME").unwrap_or_else(|_| "/home/mlj".to_string())
        ).join("utono/litdb/data/lit.db");
        if !db_path.exists() {
            return Vec::new();
        }
        let conn = rusqlite::Connection::open(&db_path).expect("failed to open lit.db");
        let mut stmt = conn.prepare(
            "SELECT abbrev, work_type, text_file FROM works \
             WHERE text_file LIKE ?1 AND text_file IS NOT NULL AND text_file != '' \
             ORDER BY title"
        ).expect("failed to prepare query");
        let pattern = format!("%{}%", author_path_fragment);
        let rows = stmt.query_map([&pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }).expect("query failed");
        rows.filter_map(|r| r.ok())
            .filter(|(_, _, path)| std::path::Path::new(path).exists())
            .collect()
    }

    fn run_author_pagination(author_path_fragment: &str, lines_per_page: usize) {
        let works = discover_works(author_path_fragment);
        if works.is_empty() {
            eprintln!("SKIP: no works found for '{}'", author_path_fragment);
            return;
        }

        let mut total_pages = 0usize;
        let mut total_errors = 0usize;
        let mut failure_summary: Vec<String> = Vec::new();

        for (abbrev, work_type, path) in &works {
            let is_prose = line_types::is_prose_work(work_type);
            let result = run_pagination_test(path, is_prose, lines_per_page);
            total_pages += result.pages;
            if !result.errors.is_empty() {
                total_errors += result.errors.len();
                failure_summary.push(format!(
                    "{} ({}, {} pages, {} errors):\n    {}",
                    abbrev, work_type, result.pages, result.errors.len(),
                    result.errors.join("\n    ")
                ));
            }
        }

        eprintln!(
            "pagination test: {} works, {} total pages, {} errors (lpp={})",
            works.len(), total_pages, total_errors, lines_per_page
        );

        if !failure_summary.is_empty() {
            panic!(
                "pagination failures ({}/{} works, {} errors, lpp={}):\n\n{}",
                failure_summary.len(), works.len(), total_errors, lines_per_page,
                failure_summary.join("\n\n")
            );
        }
    }

    #[test]
    fn shakespeare_pagination_35lpp() {
        run_author_pagination("shakespeare-william", 35);
    }

    #[test]
    fn shakespeare_pagination_25lpp() {
        run_author_pagination("shakespeare-william", 25);
    }

    #[test]
    fn shakespeare_pagination_45lpp() {
        run_author_pagination("shakespeare-william", 45);
    }

    #[test]
    fn chaucer_pagination_35lpp() {
        run_author_pagination("chaucer-geoffrey", 35);
    }

    #[test]
    fn chaucer_pagination_25lpp() {
        run_author_pagination("chaucer-geoffrey", 25);
    }

    #[test]
    fn chaucer_pagination_45lpp() {
        run_author_pagination("chaucer-geoffrey", 45);
    }

    #[test]
    fn db_first_classifiers_prefer_db_then_regex() {
        let stage = |_: usize| Some(2i64);   // mapped stage row
        let spoken = |_: usize| Some(0i64);  // mapped spoken row
        let unmapped = |_: usize| None;

        // is_stage: mapped sub_line>0 => stage regardless of text.
        assert!(super::is_stage_db_first(0, "anything", &stage));
        assert!(!super::is_stage_db_first(0, "[looks bracketed]", &spoken));
        // unmapped => regex fallback.
        assert!(super::is_stage_db_first(0, "[To Jourdain.]", &unmapped));
        assert!(!super::is_stage_db_first(0, "Lay hands.", &unmapped));

        // is_dialogue: mapped stage row => NOT dialogue; mapped spoken => regex on text.
        assert!(!super::is_dialogue_db_first(0, "[To Jourdain.]", false, &stage));
        assert!(super::is_dialogue_db_first(0, "Lay hands.", false, &spoken));
        // unmapped => regex fallback.
        assert!(super::is_dialogue_db_first(0, "Lay hands.", false, &unmapped));
        assert!(!super::is_dialogue_db_first(0, "[To Jourdain.]", false, &unmapped));
    }

    /// Pure two-column split over a slice of line texts. `col_lines` is how many
    /// lines fit in ONE column. Reuses `trim_visible_range_pure` so neither column
    /// ends on a dangling speaker / stage direction / split stanza, matching the
    /// single-column page-boundary rules.
    fn column_split_pure(
        lines: &[String],
        page_top: usize,
        col_lines: usize,
        is_prose: bool,
    ) -> ColumnSplit {
        let line_count = lines.len();
        if line_count == 0 || page_top >= line_count {
            return ColumnSplit { split: page_top, page_end: page_top, next_page_top: line_count };
        }
        let left_raw = (page_top + col_lines - 1).min(line_count - 1);
        let left_last = trim_visible_range_pure(lines, page_top, left_raw, is_prose);
        let split = (left_last + 1).min(line_count);
        if split >= line_count {
            return ColumnSplit { split, page_end: left_last, next_page_top: line_count };
        }
        let right_raw = (split + col_lines - 1).min(line_count - 1);
        let right_last = trim_visible_range_pure(lines, split, right_raw, is_prose);
        let next_top = (right_last + 1).min(line_count);
        ColumnSplit { split, page_end: right_last, next_page_top: next_top }
    }

    /// Like `column_split_pure` but, given a parallel `is_translation` slice, never
    /// lets the left/right split fall between a source line and its immediately-
    /// following translation line - the pair moves together to the right column.
    fn column_split_pure_tr(
        lines: &[String],
        is_translation: &[bool],
        page_top: usize,
        col_lines: usize,
        is_prose: bool,
    ) -> ColumnSplit {
        let mut cs = column_split_pure(lines, page_top, col_lines, is_prose);
        // If the right column would START on a translation line, that translation's
        // source is the last line of the left column - back the split up by one so
        // the source moves with its translation.
        while cs.split > page_top + 1
            && is_translation.get(cs.split).copied().unwrap_or(false)
        {
            cs.split -= 1;
        }
        cs
    }

    fn col_lines(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_falls_after_left_column_capacity() {
        // 10 dialogue lines, each column holds 3 -> left [0..2], right [3..5],
        // next page starts at 6.
        let l = col_lines(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let split = column_split_pure(&l, 0, 3, true);
        assert_eq!(split.split, 3, "right column starts at line 3");
        assert_eq!(split.page_end, 5, "page ends at line 5 (right col last)");
        assert_eq!(split.next_page_top, 6, "next page starts at line 6");
    }

    #[test]
    fn split_clamps_at_end_of_text() {
        // 4 lines, columns hold 3 -> left [0..2], right [3..3], end of text.
        let l = col_lines(&["a", "b", "c", "d"]);
        let split = column_split_pure(&l, 0, 3, true);
        assert_eq!(split.split, 3);
        assert_eq!(split.page_end, 3);
        assert_eq!(split.next_page_top, 4); // == line_count -> at end
    }

    #[test]
    fn left_column_does_not_end_on_dangling_speaker() {
        // Capacity 3 but line 2 is a speaker -> left column trims to [0..1],
        // the speaker moves to the right column with its dialogue.
        let l = col_lines(&["First line.", "Second line.", "HAMLET", "To be.", "Or not.", "End."]);
        let split = column_split_pure(&l, 0, 3, false);
        assert_eq!(split.split, 2, "speaker pushed to right column");
    }

    #[test]
    fn split_keeps_source_and_translation_together() {
        // line 1 is the translation of line 0; line 3 translation of line 2.
        // Capacity 3 would put split after line 2 (a source line) leaving its
        // translation (line 3) orphaned at the right column top - so the split
        // must back up to keep the pair together: split = 2 -> left [0..1],
        // right starts at the source line 2 with its translation 3.
        let l = col_lines(&["src0", "tr0", "src1", "tr1", "src2", "tr2"]);
        let is_trans = vec![false, true, false, true, false, true];
        let split = column_split_pure_tr(&l, &is_trans, 0, 3, false);
        // left column ends on a translation line (1), not splitting pair (2,3)
        assert_eq!(split.split, 2);
    }
}

#[cfg(test)]
mod overtall_offset_tests {
    use super::overtall_next_offset;

    #[test]
    fn steps_within_an_overtall_paragraph_then_stops() {
        // Paragraph 1170px tall, viewport usable 1067px (the real Bleak House case).
        // Offset 0: 1170 - 0 = 1170 > 1067 -> advance within (step to 1067).
        assert_eq!(overtall_next_offset(0, 1170, 1067), Some(1067));
        // Offset 1067: 1170 - 1067 = 103 <= 1067 -> exhausted, advance to next line.
        assert_eq!(overtall_next_offset(1067, 1170, 1067), None);
    }

    #[test]
    fn normal_height_paragraph_never_steps_within() {
        // A 122px paragraph fits the viewport: 122 - 0 = 122 <= 1067 -> None.
        assert_eq!(overtall_next_offset(0, 122, 1067), None);
    }

    #[test]
    fn very_tall_paragraph_steps_multiple_times_covering_all_rows() {
        // 3 viewports tall: must take 2 within-steps then exhaust on the 3rd.
        let para = 3200;
        let usable = 1000;
        let mut off = 0;
        let mut steps = vec![off];
        while let Some(next) = overtall_next_offset(off, para, usable) {
            assert!(next > off, "offset must advance");
            assert!(next < para, "offset must stay inside the paragraph");
            off = next;
            steps.push(off);
        }
        // Coverage: the last offset + usable must reach or pass the paragraph end,
        // so no row is left below the fold of the final page.
        assert!(off + usable >= para, "last page must cover the paragraph end: off={off} usable={usable} para={para}");
        // 3200/1000 -> offsets 0, 1000, 2000 (2200 left = 1200 > 1000 -> 2000 still steps? )
        // 3200-2000=1200>1000 -> step to 3000; 3200-3000=200<=1000 -> stop. So 0,1000,2000,3000.
        assert_eq!(steps, vec![0, 1000, 2000, 3000]);
    }

    #[test]
    fn zero_usable_is_safe() {
        // Degenerate viewport must not divide-by-zero or loop forever.
        assert_eq!(overtall_next_offset(0, 100, 0), Some(1));
    }
}

#[cfg(test)]
mod prose_raw_next_boundary_tests {
    use super::prose_raw_next_boundary;

    #[test]
    fn prose_raw_next_boundary_fills_then_stops() {
        // 1000px doc, 300px viewport: boundaries at 300, 600, 900, then None
        // (at y0=900 only 100px remain — the last page).
        assert_eq!(prose_raw_next_boundary(0, 1000, 300), Some(300));
        assert_eq!(prose_raw_next_boundary(600, 1000, 300), Some(900));
        assert_eq!(prose_raw_next_boundary(900, 1000, 300), None);
        assert_eq!(prose_raw_next_boundary(700, 1000, 300), None);
    }
}

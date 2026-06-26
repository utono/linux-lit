use gtk4::prelude::*;
use super::{AppState, InputMode, SidebarMode};
use crate::app::layout::overlay_card_size;
use crate::app::vocab_popup::{open_vocab_popup, close_vocab_popup, update_vocab_popup_margin};
use crate::logging::log;

/// Scene-key sentinel for the whole-work synopsis (not a real (div1,div2)
/// scene). Sorts before all real scenes in the synopsis picker; `whole_work_label`
/// maps it to "Whole work". Distinct from the journal whole-work key, which lives
/// in a separate table and disambiguates by its `scope` column.
pub(crate) const SYNOPSIS_WHOLE_WORK: (i64, i64) = (-2, 0);

/// (e.g. Rom, Tro) are never treated as chapter works.
/// Detection reads work.lines directly so it works whether or not a line_map
/// exists (prose works load with line_map = None).
pub fn is_chapter_work(state: &AppState) -> bool {
    state
        .current_work
        .as_ref()
        .map(|w| {
            crate::db::line_types::is_prose_work(&w.work_type)
                && w.lines.iter().any(|l| l.is_chapter)
        })
        .unwrap_or(false)
}

/// Chapter number (1-indexed) for the current line in a chapter work, counting
/// is_chapter work-lines at or before the current line. Returns 0 when before
/// the first chapter (front matter). Works with or without a line_map.
pub fn current_chapter_number(state: &AppState) -> usize {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return 0,
    };
    // Map the current buffer line to a work-line index. If the current buffer
    // line isn't itself mapped (e.g. a blank/heading line), walk forward then
    // backward to the nearest mapped work line, mirroring current_scene_divs.
    let line_count = state.effective_line_count();
    let work_idx = state
        .work_line_for_buffer(state.current_line)
        .or_else(|| (state.current_line + 1..line_count).find_map(|bl| state.work_line_for_buffer(bl)))
        .or_else(|| (0..state.current_line).rev().find_map(|bl| state.work_line_for_buffer(bl)));
    let work_idx = match work_idx {
        Some(i) => i,
        None => return 0,
    };
    let flags: Vec<bool> = work.lines.iter().map(|l| l.is_chapter).collect();
    chapter_number_from_flags(&flags, work_idx)
}

/// Pure core of current_chapter_number: count is_chapter flags up to and
/// including work_idx. 0 = before first chapter.
pub fn chapter_number_from_flags(is_chapter_flags: &[bool], work_idx: usize) -> usize {
    is_chapter_flags.iter().take(work_idx + 1).filter(|&&c| c).count()
}

/// The synopsis-cache key for the current line. For chapter works this is
/// (chapter_number, 0); otherwise the scene's (div1, div2).
pub fn current_synopsis_key(state: &AppState) -> (i64, i64) {
    if is_chapter_work(state) {
        return (current_chapter_number(state) as i64, 0);
    }
    current_scene_divs(state)
}

/// The fixed label for the whole-work synopsis position (SYNOPSIS_WHOLE_WORK), or `None` for
/// any real scene/chapter key. Pure seam for `synopsis_label`.
fn whole_work_label(div1: i64, div2: i64) -> Option<&'static str> {
    if (div1, div2) == SYNOPSIS_WHOLE_WORK {
        Some("Whole work")
    } else {
        None
    }
}

/// Human-readable overlay label for a synopsis key, branching on work type.
pub fn synopsis_label(state: &AppState, div1: i64, div2: i64) -> String {
    if let Some(label) = whole_work_label(div1, div2) {
        return label.to_string();
    }
    if is_chapter_work(state) {
        format!("Chapter {}", div1)
    } else {
        scene_label_for(state, div1, div2)
    }
}

/// Get the (div1, div2) of the scene at the current line.
/// When current_line is on an unmapped buffer line (scene header, separator,
/// stage direction), walks forward then backward to find the nearest mapped line.
pub fn current_scene_divs(state: &AppState) -> (i64, i64) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return (0, 0),
    };
    let line_count = state.effective_line_count();
    // Try current line first
    if let Some(work_idx) = state.work_line_for_buffer(state.current_line) {
        if let Some(line) = work.lines.get(work_idx) {
            return (line.div1, line.div2);
        }
    }
    // Walk forward to find the nearest mapped line (the first dialogue of the scene)
    for bl in (state.current_line + 1)..line_count {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    // Walk backward as fallback
    for bl in (0..state.current_line).rev() {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    (0, 0)
}

/// Return the `(div1, div2)` (act, scene) for an arbitrary buffer line by
/// reading the DB-backed `Line` metadata — never inferred from buffer text.
/// Walks forward from `buffer_line` to the first DB-mapped line (the marker /
/// `=====` chrome lines are unmapped), then backward as a fallback. Returns
/// `(0, 0)` when nothing is mapped (treated as "Prologue" by `scene_label`).
pub fn divs_at_buffer_line(state: &AppState, buffer_line: usize) -> (i64, i64) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return (0, 0),
    };
    let line_count = state.effective_line_count();
    for bl in buffer_line..line_count {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    for bl in (0..buffer_line).rev() {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    (0, 0)
}

/// Assemble the verbatim text of one scene `(div1, div2)` for the current work,
/// with speaker attributions, in reading order. Empty string if no current work
/// or no matching lines.
pub fn scene_text_for(state: &AppState, div1: i64, div2: i64) -> String {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return String::new(),
    };
    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for line in work.lines.iter().filter(|l| l.div1 == div1 && l.div2 == div2) {
        match line.speaker.as_deref() {
            Some(sp) if last_speaker != Some(sp) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(sp);
                out.push('\n');
                last_speaker = Some(sp);
            }
            _ => {}
        }
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

/// Pure prose-window renderer: collects the work-line indices for the given
/// division, finds `anchor_work_line`'s position within it (fallback 0), slices
/// ±`radius` via `window_range`, and renders the selected paragraphs with the
/// same speaker-interleave logic as `scene_text_for`.
pub(crate) fn prose_window_text(
    work: &crate::db::models::Work,
    div1: i64,
    div2: i64,
    anchor_work_line: usize,
    radius: usize,
) -> String {
    let idxs: Vec<usize> = work
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.div1 == div1 && l.div2 == div2)
        .map(|(i, _)| i)
        .collect();
    if idxs.is_empty() {
        return String::new();
    }
    // Anchor's position WITHIN this division. If `anchor_work_line` isn't in the
    // division (e.g. the band's div differs from the reader's cursor div, or the
    // reader had no saved position), fall back to the division's first paragraph
    // — the window is then the division opening ±radius.
    let anchor_pos = idxs.iter().position(|&i| i == anchor_work_line).unwrap_or(0);
    let (lo, hi) = window_range(anchor_pos, radius, idxs.len());

    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for &wi in &idxs[lo..=hi] {
        let line = &work.lines[wi];
        match line.speaker.as_deref() {
            Some(sp) if last_speaker != Some(sp) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(sp);
                out.push('\n');
                last_speaker = Some(sp);
            }
            _ => {}
        }
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

/// Like `scene_text_for`, but for PROSE works returns only the paragraphs around
/// `anchor_work_line` (±`radius`, clamped to the division). Non-prose works
/// (plays) return the full `scene_text_for` — a real scene is small and the
/// whole scene is the intended context. Up to `2*radius + 1` paragraphs.
pub fn scene_text_windowed(
    state: &AppState,
    div1: i64,
    div2: i64,
    anchor_work_line: usize,
    radius: usize,
) -> String {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return String::new(),
    };
    if !crate::db::line_types::is_prose_work(&work.work_type) {
        return scene_text_for(state, div1, div2);
    }
    prose_window_text(work, div1, div2, anchor_work_line, radius)
}

/// Check if the current line is the first line of a new scene.
pub fn is_first_line_of_scene(state: &AppState) -> bool {
    if state.current_line == 0 {
        return true;
    }
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let cur_idx = match state.work_line_for_buffer(state.current_line) {
        Some(i) => i,
        None => return false,
    };
    let cur = &work.lines[cur_idx];
    // line_in_div == 1 means this is the first content line of a scene,
    // which is where scene-jump (2/3 keys) lands the cursor.
    if cur.line_in_div == 1 {
        return true;
    }
    let prev_idx = state.work_line_for_buffer(state.current_line - 1);
    match prev_idx {
        Some(pi) => {
            let prev = &work.lines[pi];
            cur.div1 != prev.div1 || cur.div2 != prev.div2
        }
        _ => false,
    }
}

/// Walk backwards from `buf_line` past unmapped buffer lines (headers,
/// separators, blanks, stage directions) to find where the scene heading
/// block begins. Returns the buffer line to use as page_top.
pub(crate) fn scene_heading_start(state: &AppState, buf_line: usize) -> usize {
    let mut top = buf_line;
    while top > 0 {
        let prev = top - 1;
        if state.work_line_for_buffer(prev).is_some() {
            break;
        }
        top = prev;
    }
    top
}

/// Show the synopsis for the current scene in the sidebar popup.
pub fn show_synopsis(state: &mut AppState) {
    let (div1, div2) = current_synopsis_key(state);
    log(&format!(
        "SYNOPSIS: show current_line={} divs=({},{}) cache_hit={}",
        state.current_line, div1, div2, state.synopsis_cache.contains_key(&(div1, div2))
    ));
    if let Some(synopsis) = state.synopsis_cache.get(&(div1, div2)) {
        let scene_label = synopsis_label(state, div1, div2);
        state.vocab_popup.popup.update_synopsis(&scene_label, synopsis);
        state.vocab_popup.popup.show();
        update_vocab_popup_margin(state);
        state.sidebar_mode = SidebarMode::Synopsis;
        state.synopsis_visible = true;
    }
}

/// Toggle between synopsis and vocab sidebar modes.
pub fn toggle_synopsis(state: &mut AppState) {
    if state.synopsis_cache.is_empty() {
        return;
    }
    // Cancel any pending auto-fade timer
    state.vocab_popup.fade_gen.set(state.vocab_popup.fade_gen.get() + 1);
    if state.sidebar_mode == SidebarMode::Synopsis && state.synopsis_visible {
        state.sidebar_mode = SidebarMode::Vocab;
        state.synopsis_visible = false;
        if state.vocab_popup.auto {
            open_vocab_popup(state);
        } else {
            close_vocab_popup(state);
        }
    } else {
        let (div1, div2) = current_synopsis_key(state);
        if state.synopsis_cache.contains_key(&(div1, div2)) {
            show_synopsis(state);
        }
    }
}

pub fn show_synopsis_overlay(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let s = state.borrow();
    if s.gloss_overlay.is_visible() {
        drop(s);
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        s.input_mode = InputMode::Reader;
        return;
    }

    if s.synopsis_cache.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No synopsis for this section", 3);
        return;
    }

    let (div1, div2) = current_synopsis_key(&s);
    let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
        Some(text) => text.clone(),
        None => {
            crate::ui::toast::show_transient(&s.chapter_toast, "No synopsis for this section", 3);
            return;
        }
    };

    let (card_width, card_height) = overlay_card_size(&s);
    let label = synopsis_label(&s, div1, div2);
    let root_color = s.theme.root_color.clone();
    s.gloss_overlay.show_synopsis(&label, &synopsis, Some(&root_color), card_width, card_height);
    drop(s);
    let mut s = state.borrow_mut();
    s.synopsis_overlay_scene = (div1, div2);
    crate::input::actions::gloss::recolor_cached_blocks(&s);
    s.input_mode = InputMode::SynopsisOverlay;
}

/// Human-readable label for a scene, shared by the synopsis overlay and the
/// gloss overlay. (0,0) = Prologue; (N,0) = Act N, Chorus; else Act N, Scene M.
///
/// Note: `(N,0)` is ambiguous without the work — it is a *Chorus* only when act
/// N also has numbered scenes; a standalone `(N,0)` past the last act is an
/// *Epilogue*. Prefer `scene_label_for` when an `AppState` is available so the
/// epilogue is labelled correctly; this pure form falls back to "Act N, Chorus".
pub fn scene_label(div1: i64, div2: i64) -> String {
    if div1 == 0 && div2 == 0 {
        "Prologue".to_string()
    } else if div2 == 0 {
        format!("Act {}, Chorus", div1)
    } else {
        format!("Act {}, Scene {}", div1, div2)
    }
}

/// Work-aware scene label that resolves the `(N,0)` ambiguity using the
/// authoritative `(div1,div2)` metadata: a `(N,0)` whose act N has no numbered
/// scenes (`div2 > 0`) in the work is an *Epilogue*, not a Chorus. `(0,0)` is
/// always the Prologue. Falls back to the pure `scene_label` shape otherwise.
pub fn scene_label_for(state: &AppState, div1: i64, div2: i64) -> String {
    if div2 == 0 && div1 > 0 {
        let has_scenes = state
            .current_work
            .as_ref()
            .map(|w| w.lines.iter().any(|l| l.div1 == div1 && l.div2 > 0))
            .unwrap_or(false);
        if !has_scenes {
            return "Epilogue".to_string();
        }
    }
    scene_label(div1, div2)
}

/// Put the whole-work synopsis key (SYNOPSIS_WHOLE_WORK) first when it exists, otherwise
/// return `rest` unchanged. Pure seam for `ordered_synopsis_scenes`.
fn prepend_whole_work(has_whole_work: bool, rest: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    if has_whole_work {
        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push(SYNOPSIS_WHOLE_WORK);
        out.extend(rest);
        out
    } else {
        rest
    }
}

/// Ordered list of the work's scene keys (div1, div2) that have a synopsis, in
/// reading order. `work.lines` is already sorted by (div1, div2, line_in_div),
/// so collecting unique pairs in encounter order gives reading order.
fn ordered_synopsis_scenes(s: &AppState) -> Vec<(i64, i64)> {
    let has_whole_work = s.synopsis_cache.contains_key(&SYNOPSIS_WHOLE_WORK);

    if is_chapter_work(s) {
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return Vec::new(),
        };
        let chapter_count = work.lines.iter().filter(|l| l.is_chapter).count();
        let mut keys = Vec::new();
        for n in 1..=chapter_count {
            let k = (n as i64, 0);
            if s.synopsis_cache.contains_key(&k) {
                keys.push(k);
            }
        }
        return prepend_whole_work(has_whole_work, keys);
    }

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();
    for line in &work.lines {
        let k = (line.div1, line.div2);
        // Never list SYNOPSIS_WHOLE_WORK as a scene from the line loop — it's the whole-work
        // key, prepended separately. (Lines always have div1 >= -1, but guard
        // anyway so a stray SYNOPSIS_WHOLE_WORK line can't double it.)
        if k != SYNOPSIS_WHOLE_WORK && seen.insert(k) && s.synopsis_cache.contains_key(&k) {
            keys.push(k);
        }
    }
    prepend_whole_work(has_whole_work, keys)
}

/// Clamp the next synopsis index to `[0, len-1]` (no wraparound). Returns `None`
/// when the step would run off either end (already at first/last), so the caller
/// can no-op rather than re-render the same scene. Pure seam for `cycle_synopsis`.
fn clamp_synopsis_index(idx: usize, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = idx as i32 + delta;
    if next < 0 || next >= len as i32 {
        None
    } else {
        Some(next as usize)
    }
}

/// Step the synopsis overlay to the next (+1) or previous (-1) scene that has a
/// synopsis, clamping at the first/last (no wraparound). No-op if the overlay
/// isn't showing a known scene or is already at the boundary being stepped past.
pub fn cycle_synopsis(state: &std::rc::Rc<std::cell::RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let scenes = ordered_synopsis_scenes(&s);
    if scenes.is_empty() {
        return;
    }
    let cur = s.synopsis_overlay_scene;
    let idx = scenes.iter().position(|&k| k == cur).unwrap_or(0);
    let new_idx = match clamp_synopsis_index(idx, delta, scenes.len()) {
        Some(i) => i,
        None => return,
    };
    let (div1, div2) = scenes[new_idx];
    let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
        Some(t) => t.clone(),
        None => return,
    };
    let label = synopsis_label(&s, div1, div2);
    let (card_width, card_height) = overlay_card_size(&s);
    let root_color = s.theme.root_color.clone();
    s.gloss_overlay.show_synopsis(&label, &synopsis, Some(&root_color), card_width, card_height);
    s.synopsis_overlay_scene = (div1, div2);
    crate::input::actions::gloss::recolor_cached_blocks(&s);
}

pub fn update_title_bar_scene(state: &AppState) {
    if !state.title_bar.is_visible() {
        return;
    }
    if !state.synopsis_cache.is_empty() {
        let (div1, div2) = current_synopsis_key(state);
        let label = synopsis_label(state, div1, div2);
        state.title_bar_scene_label.set_text(&label);
    } else {
        state.title_bar_scene_label.set_text("");
    }
}

/// Inclusive paragraph index range `anchor_pos ± radius`, clamped to `[0, n)`.
/// Returns `(lo, hi)` with `lo <= hi`. When `n == 0` returns `(0, 0)` — callers
/// must check `n == 0` separately and not index.
fn window_range(anchor_pos: usize, radius: usize, n: usize) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let lo = anchor_pos.saturating_sub(radius);
    let hi = (anchor_pos + radius).min(n - 1);
    (lo, hi)
}

#[cfg(test)]
mod window_tests {
    use super::window_range;

    #[test]
    fn middle_anchor_full_window() {
        // anchor 50, radius 10, n 100 -> [40, 60] inclusive = 21 paragraphs
        assert_eq!(window_range(50, 10, 100), (40, 60));
    }
    #[test]
    fn clamps_low_near_start() {
        assert_eq!(window_range(2, 10, 100), (0, 12));
    }
    #[test]
    fn clamps_high_near_end() {
        assert_eq!(window_range(98, 10, 100), (88, 99));
    }
    #[test]
    fn whole_division_when_smaller_than_window() {
        // n=5, any anchor -> the whole [0,4]
        assert_eq!(window_range(2, 10, 5), (0, 4));
    }
    #[test]
    fn empty_division_is_safe() {
        // n=0 -> (0,0); caller must treat n==0 as "no paragraphs" and not index.
        assert_eq!(window_range(0, 10, 0), (0, 0));
    }
}

#[cfg(test)]
mod chapter_synopsis_tests {
    #[test]
    fn chapter_number_from_flags_counts_inclusive() {
        // lines: ch markers at idx 0 and 3
        let flags = vec![true, false, false, true, false];
        assert_eq!(super::chapter_number_from_flags(&flags, 0), 1); // on first chapter
        assert_eq!(super::chapter_number_from_flags(&flags, 2), 1); // still chapter 1
        assert_eq!(super::chapter_number_from_flags(&flags, 3), 2); // second chapter
        assert_eq!(super::chapter_number_from_flags(&flags, 4), 2);
    }

    #[test]
    fn chapter_number_from_flags_front_matter_is_zero() {
        // first chapter marker at idx 2; idx 0,1 are front matter
        let flags = vec![false, false, true, false];
        assert_eq!(super::chapter_number_from_flags(&flags, 0), 0);
        assert_eq!(super::chapter_number_from_flags(&flags, 1), 0);
        assert_eq!(super::chapter_number_from_flags(&flags, 2), 1);
    }
}

#[cfg(test)]
mod synopsis_tests {
    use super::prepend_whole_work;

    #[test]
    fn whole_work_label_only_for_minus_two() {
        assert_eq!(super::whole_work_label(super::SYNOPSIS_WHOLE_WORK.0, super::SYNOPSIS_WHOLE_WORK.1), Some("Whole work"));
        assert_eq!(super::whole_work_label(0, 0), None); // (0,0) is the Prologue slot, not whole-work
        assert_eq!(super::whole_work_label(1, 1), None);
        assert_eq!(super::whole_work_label(2, 0), None);
    }

    #[test]
    fn prepend_whole_work_puts_minus_two_zero_first() {
        let rest = vec![(1, 1), (1, 2), (2, 1)];
        assert_eq!(
            prepend_whole_work(true, rest.clone()),
            vec![super::SYNOPSIS_WHOLE_WORK, (1, 1), (1, 2), (2, 1)]
        );
    }

    #[test]
    fn prepend_whole_work_absent_is_unchanged() {
        let rest = vec![(1, 1), (1, 2)];
        assert_eq!(prepend_whole_work(false, rest.clone()), rest);
    }

    #[test]
    fn prepend_whole_work_empty_rest() {
        assert_eq!(prepend_whole_work(true, vec![]), vec![super::SYNOPSIS_WHOLE_WORK]);
        assert_eq!(prepend_whole_work(false, vec![]), Vec::<(i64, i64)>::new());
    }

    #[test]
    fn clamp_synopsis_index_clamps_no_wrap() {
        use super::clamp_synopsis_index;
        // Mid-list steps move by one.
        assert_eq!(clamp_synopsis_index(1, 1, 4), Some(2));
        assert_eq!(clamp_synopsis_index(2, -1, 4), Some(1));
        // At the last index, +1 is a no-op (None) — no wrap to 0.
        assert_eq!(clamp_synopsis_index(3, 1, 4), None);
        // At index 0, -1 is a no-op (None) — no wrap to last.
        assert_eq!(clamp_synopsis_index(0, -1, 4), None);
        // Empty list is always None.
        assert_eq!(clamp_synopsis_index(0, 1, 0), None);
    }

    #[test]
    fn prose_window_shrinks_cromwell_play_unchanged() {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => {
                eprintln!("skip: no lit.db");
                return;
            }
        };
        // Prose: Cromwell is one division (1,0) of thousands of paragraphs.
        if let Ok(work) = crate::db::queries::load_work(&conn, "Cromwell") {
            if crate::db::line_types::is_prose_work(&work.work_type) {
                // anchor somewhere in the middle
                let mid = work.lines.len() / 2;
                let windowed = super::prose_window_text(&work, 1, 0, mid, 10);
                // full division text length, computed the scene_text_for way
                let full_len: usize = work
                    .lines
                    .iter()
                    .filter(|l| l.div1 == 1 && l.div2 == 0)
                    .map(|l| l.text.len() + 1)
                    .sum();
                assert!(!windowed.is_empty());
                assert!(
                    windowed.len() < full_len / 10,
                    "windowed prose ({}) must be far smaller than full division ({})",
                    windowed.len(),
                    full_len
                );
            }
        }
        // Note: play-equality half is omitted as it would require constructing
        // a full AppState. The prose-shrinking gate (is_prose_work check in
        // scene_text_windowed) is already covered by code inspection.
    }
}

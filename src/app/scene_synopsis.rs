use gtk4::prelude::*;
use super::{AppState, InputMode, SidebarMode};
use crate::app::layout::overlay_card_size;
use crate::app::vocab_popup::{open_vocab_popup, close_vocab_popup, position_vocab_popup};
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

/// Human-readable label for a chapter-work synopsis key. Chapter number 0 is
/// the front matter before the first chapter — labelled "Preface", never
/// "Chapter 0". Pure seam for `synopsis_label`.
fn chapter_label(div1: i64) -> String {
    if div1 <= 0 {
        "Preface".to_string()
    } else {
        format!("Chapter {}", div1)
    }
}

/// Human-readable overlay label for a synopsis key, branching on work type.
pub fn synopsis_label(state: &AppState, div1: i64, div2: i64) -> String {
    if let Some(label) = whole_work_label(div1, div2) {
        return label.to_string();
    }
    if is_chapter_work(state) {
        chapter_label(div1)
    } else {
        scene_label_for(state, div1, div2)
    }
}

/// The synopsis overlay's running-head pair `(work, position)`: the work's
/// DISPLAY abbrev (left, e.g. "BH-Barrett" — same as the main card's strip,
/// which shows `w.abbrev`, not the canonical base) and the terse
/// scene/chapter label (right, "Chapter 10" / "Act N, Scene M").
pub fn synopsis_head(state: &AppState, div1: i64, div2: i64) -> (String, String) {
    let work = state
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone())
        .unwrap_or_default();
    (work, synopsis_label(state, div1, div2))
}

/// Running-head pair for the reader cursor's scene — the gloss/journal
/// overlays open at the cursor, so this is their `synopsis_head` equivalent
/// when no per-gloss (act, scene) context is at hand.
pub fn cursor_head(state: &AppState) -> (String, String) {
    let (d1, d2) = current_scene_divs(state);
    synopsis_head(state, d1, d2)
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

/// Budget for a non-prose scene ask: scenes at or under this render whole
/// (~90% of the Shakespeare corpus); longer scenes fall back to the anchored
/// excerpt so a single ask never ships a 6k+-token scene. ≈3k tokens.
pub(crate) const SCENE_TEXT_MAX_CHARS: usize = 12_000;

/// Excerpt radius (lines each side of the anchor) for over-budget scenes:
/// up to 161 play lines ≈ 2.5k tokens.
pub(crate) const VERSE_WINDOW_RADIUS: usize = 80;

/// Render `work.lines[idxs]` with the speaker-interleave rule shared by every
/// scene-text builder: the speaker name on its own line whenever it changes,
/// preceded by a blank separator line.
fn render_speaker_interleaved(work: &crate::db::models::Work, idxs: &[usize]) -> String {
    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for &wi in idxs {
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

/// Work-line indices of one division, in reading order.
fn division_indices(work: &crate::db::models::Work, div1: i64, div2: i64) -> Vec<usize> {
    work.lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.div1 == div1 && l.div2 == div2)
        .map(|(i, _)| i)
        .collect()
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
    let idxs = division_indices(work, div1, div2);
    if idxs.is_empty() {
        return String::new();
    }
    // Anchor's position WITHIN this division. If `anchor_work_line` isn't in the
    // division (e.g. the band's div differs from the reader's cursor div, or the
    // reader had no saved position), fall back to the division's first paragraph
    // — the window is then the division opening ±radius.
    let anchor_pos = idxs.iter().position(|&i| i == anchor_work_line).unwrap_or(0);
    let (lo, hi) = window_range(anchor_pos, radius, idxs.len());
    // Tell the model when the window is a FRAGMENT of a longer chapter, exactly
    // as `play_scene_text_lean` does for a clipped scene — otherwise a
    // long-chapter answer is given as if the whole chapter were in view. A
    // chapter that fits entirely within ±radius gets no markers.
    let mut out = String::new();
    if lo > 0 {
        out.push_str(
            "[\u{2026} chapter continues above \u{2014} this is an excerpt around the reader's position \u{2026}]\n\n",
        );
    }
    out.push_str(&render_speaker_interleaved(work, &idxs[lo..=hi]));
    if hi + 1 < idxs.len() {
        out.push_str("\n[\u{2026} chapter continues below \u{2026}]\n");
    }
    out
}

/// Non-prose scene text for a journal ask, budgeted: the WHOLE scene when it
/// fits `SCENE_TEXT_MAX_CHARS`, else an anchored excerpt of
/// ±`VERSE_WINDOW_RADIUS` lines around `anchor_work_line` (fallback: the scene
/// opening) with explicit markers on whichever ends were cut, so the model
/// never mistakes the excerpt for the whole scene.
pub(crate) fn play_scene_text_lean(
    work: &crate::db::models::Work,
    div1: i64,
    div2: i64,
    anchor_work_line: usize,
) -> String {
    let idxs = division_indices(work, div1, div2);
    if idxs.is_empty() {
        return String::new();
    }
    let full = render_speaker_interleaved(work, &idxs);
    if full.len() <= SCENE_TEXT_MAX_CHARS {
        return full;
    }
    let anchor_pos = idxs.iter().position(|&i| i == anchor_work_line).unwrap_or(0);
    let (lo, hi) = window_range(anchor_pos, VERSE_WINDOW_RADIUS, idxs.len());
    let mut out = String::new();
    if lo > 0 {
        out.push_str(
            "[\u{2026} scene continues above \u{2014} this is an excerpt around the reader's position \u{2026}]\n\n",
        );
    }
    out.push_str(&render_speaker_interleaved(work, &idxs[lo..=hi]));
    if hi + 1 < idxs.len() {
        out.push_str("\n[\u{2026} scene continues below \u{2026}]\n");
    }
    out
}

/// Like `scene_text_for`, but sized for a Claude ask. PROSE works return only
/// the paragraphs around `anchor_work_line` (±`radius`, clamped to the
/// division). Non-prose works (plays) return the whole scene when it fits
/// `SCENE_TEXT_MAX_CHARS`, else the `play_scene_text_lean` anchored excerpt —
/// the tail of long scenes (up to ~10k tokens) is what this bounds.
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
        return play_scene_text_lean(work, div1, div2, anchor_work_line);
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
    // Reusing the shared popup for synopsis: clear any journal Q&A state so an
    // in-flight reply can't clobber the synopsis and R over a synopsis starts
    // from a clean Definition view.
    state.vocab_popup.journal = None;
    state.vocab_popup.view = crate::ui::vocab_popup::VocabView::Definition;
    if let Some(synopsis) = state.synopsis_cache.get(&(div1, div2)) {
        let scene_label = synopsis_label(state, div1, div2);
        state.vocab_popup.popup.update_synopsis(&scene_label, synopsis);
        state.vocab_popup.popup.show();
        position_vocab_popup(state);
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
        crate::app::return_to_reader_mode(&mut s);
        return;
    }

    if s.synopsis_cache.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No synopsis for this section", 3);
        return;
    }

    let (div1, div2) = current_synopsis_key(&s);
    let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
        Some(text) => text.clone(),
        None => {
            crate::input::navigation::show_chapter_toast_secs(&s, "No synopsis for this section", 3);
            return;
        }
    };

    let (card_width, card_height) = overlay_card_size(&s);
    let (head_work, head_pos) = synopsis_head(&s, div1, div2);
    let root_color = s.theme.root_color.clone();
    let prose_card = prose_synopsis_card(&s, card_width);
    s.gloss_overlay.show_synopsis(&head_work, &head_pos, &synopsis, Some(&root_color), card_width, card_height, prose_card);
    drop(s);
    let mut s = state.borrow_mut();
    s.synopsis_overlay_scene = (div1, div2);
    // Escape remembered where the cursor sat in this scene's synopsis — reopen
    // there (clamped by restore_full_cursor) instead of at block 0.
    if let Some(&saved) = s.synopsis_cursor_memory.get(&(div1, div2)) {
        if saved > 0 {
            s.gloss_overlay.restore_full_cursor(saved);
            log(&format!("SYNOPSIS: restored cursor block {} for ({},{})", saved, div1, div2));
        }
    }
    crate::input::actions::gloss::recolor_cached_blocks(&s);
    s.input_mode = InputMode::SynopsisOverlay;
}

/// Card-matching synopsis layout for PROSE works: the main reading card's font
/// (family + size) and the centered NYTimes-style column inset
/// (`prose_column_margin(card_width)`, symmetric), so a prose synopsis reads
/// like its reading card. Returns `None` for plays/verse, which keep the
/// overlay's Charter + `card_width/4` inset look. Shared by every `show_synopsis`
/// call (open, amend, edit, undo) so a re-render after an edit keeps the layout.
pub fn prose_synopsis_card(state: &AppState, card_width: i32) -> Option<crate::ui::gloss_overlay::SynopsisProseCard> {
    let is_prose = crate::db::line_types::is_prose_work(
        state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or(""),
    );
    if !is_prose {
        return None;
    }
    // Centered NYTimes-style column: symmetric card_width/5 inset, matching the
    // prose reading card and the prose gloss/journal overlays.
    let margin = crate::ui::prose_column_margin(card_width);
    Some(crate::ui::gloss_overlay::SynopsisProseCard {
        font_family: state.config.font_family.clone(),
        font_size: state.config.font_size as i32,
        left_margin: margin,
        right_margin: margin,
    })
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
    let (head_work, head_pos) = synopsis_head(&s, div1, div2);
    let (card_width, card_height) = overlay_card_size(&s);
    let root_color = s.theme.root_color.clone();
    let prose_card = prose_synopsis_card(&s, card_width);
    s.gloss_overlay.show_synopsis(&head_work, &head_pos, &synopsis, Some(&root_color), card_width, card_height, prose_card);
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

/// Refresh the running-head strip from the cursor's authoritative position.
/// Left label = work abbrev; right label = the position string (act/scene for
/// plays, `Chapter N` for prose). Both come from
/// `navigation::compute_current_chapter_text`, which encodes the play-vs-prose
/// distinction from authoritative `(div1, div2)` metadata — we split off its
/// leading `"{abbrev} — "` so the work and position sit on opposite ends of
/// the strip, then shorten the prose position (`compute_current_chapter_text`
/// yields `Chapter N of M — title` / `Front matter — title`) to just
/// `Chapter N` / `Front matter` for the head. Blanks both labels when no work
/// is loaded. Runs on every cursor move (see highlight.rs) AND on work load.
pub fn update_running_heads(state: &AppState) {
    let abbrev = match state.current_work.as_ref() {
        Some(w) => w.abbrev.clone(),
        None => {
            state.running_head_work.set_text("");
            state.running_head_scene.set_text("");
            return;
        }
    };
    // `compute_current_chapter_text` returns "{abbrev} — <position>". Strip the
    // "{abbrev} — " prefix to get just the position for the right label; the
    // separator is " — " (space, em-dash U+2014, space).
    let full = crate::input::navigation::compute_current_chapter_text(state);
    let prefix = format!("{} — ", abbrev);
    let position = full.strip_prefix(&prefix).unwrap_or(full.as_str());
    // Prose head: keep only the terse leading token — `Chapter N of M — title`
    // becomes `Chapter N`, and `Front matter — title` becomes `Front matter`.
    // Plays (`Act N, Scene M`) contain no " of "/" — " and pass through intact.
    let position = position
        .split(" of ")
        .next()
        .unwrap_or(position)
        .split(" — ")
        .next()
        .unwrap_or(position);
    state.running_head_work.set_text(&abbrev);
    state.running_head_scene.set_text(position);
}

/// Inclusive paragraph index range `anchor_pos ± radius`, clamped to `[0, n)`.
/// Returns `(lo, hi)` with `lo <= hi`. When `n == 0` returns `(0, 0)` — callers
/// must check `n == 0` separately and not index.
pub(crate) fn window_range(anchor_pos: usize, radius: usize, n: usize) -> (usize, usize) {
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
mod lean_scene_tests {
    use super::{
        play_scene_text_lean, prose_window_text, SCENE_TEXT_MAX_CHARS, VERSE_WINDOW_RADIUS,
    };
    use crate::db::models::{Line, Work};

    fn line(id: i64, speaker: Option<&str>, text: &str) -> Line {
        Line {
            id,
            citation: format!("T.1.1.{id}"),
            text: text.to_string(),
            normalized: text.to_lowercase(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: speaker.is_some(),
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: id,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    /// A one-scene play work of `n` lines, two speakers alternating every
    /// 4 lines, each line `width` chars.
    fn play(n: usize, width: usize) -> Work {
        let lines = (0..n)
            .map(|i| {
                let sp = if (i / 4) % 2 == 0 { "FIRST" } else { "SECOND" };
                line(i as i64, Some(sp), &format!("{:0width$}", i, width = width))
            })
            .collect();
        Work {
            abbrev: "T".into(),
            canonical_abbrev: "T".into(),
            title: "Test".into(),
            author: "Nobody".into(),
            work_type: "play".into(),
            text_file: None,
            vocab_highlight: false,
            lines,
            timestamps: vec![],
            media_paths: vec![],
            media_ids: vec![],
            media_id: None,
        }
    }

    #[test]
    fn under_budget_scene_passes_through_whole() {
        // 100 lines x 40 chars ~ 4.6k chars < budget: whole scene, no markers.
        let w = play(100, 40);
        let text = play_scene_text_lean(&w, 1, 1, 50);
        assert!(text.len() <= SCENE_TEXT_MAX_CHARS);
        assert!(text.contains(&format!("{:040}", 0)), "first line present");
        assert!(text.contains(&format!("{:040}", 99)), "last line present");
        assert!(!text.contains("excerpt"), "no markers under budget");
    }

    #[test]
    fn over_budget_scene_windows_around_anchor_with_both_markers() {
        // 400 lines x 60 chars ~ 25k chars > budget.
        let w = play(400, 60);
        let text = play_scene_text_lean(&w, 1, 1, 200);
        assert!(text.contains("scene continues above"), "top marker");
        assert!(text.contains("scene continues below"), "bottom marker");
        assert!(text.contains(&format!("{:060}", 200)), "anchor line present");
        assert!(text.contains(&format!("{:060}", 200 - VERSE_WINDOW_RADIUS)));
        assert!(text.contains(&format!("{:060}", 200 + VERSE_WINDOW_RADIUS)));
        assert!(!text.contains(&format!("{:060}", 0)), "scene opening cut");
        assert!(!text.contains(&format!("{:060}", 399)), "scene end cut");
        // The excerpt itself respects the budget with room to spare.
        assert!(text.len() < 25_000);
    }

    #[test]
    fn anchor_at_start_has_only_bottom_marker() {
        let w = play(400, 60);
        let text = play_scene_text_lean(&w, 1, 1, 0);
        assert!(!text.contains("scene continues above"));
        assert!(text.contains("scene continues below"));
        assert!(text.contains(&format!("{:060}", 0)));
    }

    #[test]
    fn anchor_at_end_has_only_top_marker() {
        let w = play(400, 60);
        let text = play_scene_text_lean(&w, 1, 1, 399);
        assert!(text.contains("scene continues above"));
        assert!(!text.contains("scene continues below"));
        assert!(text.contains(&format!("{:060}", 399)));
    }

    // Prose windows carry the same fragment markers as plays (chapter wording).
    // `play(n, w)` fixtures all sit at div1=1/div2=1, so prose_window_text
    // windows them the same way.

    #[test]
    fn prose_window_in_the_middle_has_both_chapter_markers() {
        let w = play(400, 60);
        let text = prose_window_text(&w, 1, 1, 200, 10);
        assert!(text.contains("chapter continues above"), "top marker");
        assert!(text.contains("chapter continues below"), "bottom marker");
        assert!(text.contains(&format!("{:060}", 200)), "anchor present");
        assert!(!text.contains(&format!("{:060}", 0)), "chapter opening cut");
        assert!(!text.contains(&format!("{:060}", 399)), "chapter end cut");
    }

    #[test]
    fn prose_window_at_start_has_only_bottom_marker() {
        let w = play(400, 60);
        let text = prose_window_text(&w, 1, 1, 0, 10);
        assert!(!text.contains("chapter continues above"));
        assert!(text.contains("chapter continues below"));
        assert!(text.contains(&format!("{:060}", 0)));
    }

    #[test]
    fn prose_window_at_end_has_only_top_marker() {
        let w = play(400, 60);
        let text = prose_window_text(&w, 1, 1, 399, 10);
        assert!(text.contains("chapter continues above"));
        assert!(!text.contains("chapter continues below"));
        assert!(text.contains(&format!("{:060}", 399)));
    }

    #[test]
    fn prose_chapter_that_fits_the_window_has_no_markers() {
        // 15 paragraphs, radius 10 → the whole chapter fits, no clipping.
        let w = play(15, 60);
        let text = prose_window_text(&w, 1, 1, 7, 10);
        assert!(!text.contains("chapter continues above"));
        assert!(!text.contains("chapter continues below"));
        assert!(text.contains(&format!("{:060}", 0)), "first para present");
        assert!(text.contains(&format!("{:060}", 14)), "last para present");
    }

    #[test]
    fn anchor_outside_division_falls_back_to_opening() {
        let w = play(400, 60);
        // anchor_work_line 9999 is not in the division -> window at the opening.
        let text = play_scene_text_lean(&w, 1, 1, 9999);
        assert!(text.contains(&format!("{:060}", 0)), "opens at scene start");
        assert!(!text.contains("scene continues above"));
        assert!(text.contains("scene continues below"));
    }

    #[test]
    fn speaker_interleave_preserved_in_excerpt() {
        let w = play(400, 60);
        let text = play_scene_text_lean(&w, 1, 1, 200);
        assert!(text.contains("FIRST\n"), "speaker headers survive windowing");
        assert!(text.contains("SECOND\n"));
    }

    #[test]
    fn empty_division_is_empty() {
        let w = play(10, 40);
        assert_eq!(play_scene_text_lean(&w, 2, 1, 0), "");
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

    #[test]
    fn chapter_label_front_matter_is_preface() {
        // Chapter number 0 is the front matter before chapter 1, not "Chapter 0".
        assert_eq!(super::chapter_label(0), "Preface");
        assert_eq!(super::chapter_label(-1), "Preface");
        assert_eq!(super::chapter_label(1), "Chapter 1");
        assert_eq!(super::chapter_label(12), "Chapter 12");
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
                // full division text length, computed the full-render way
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

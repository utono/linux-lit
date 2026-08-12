use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::ui::gloss_block::BlockKind;

/// The three READER-FACING gloss types: the ones a passage can be glossed as
/// from plain reading (teacher-generic, inner-monologue, reader-gloss).
/// Deliberately EXCLUDES `syntax-gloss`, which is a separate stop of its own
/// in the `\` segment-overlay cycle — mixing it into this set would let the
/// GLOSS stop of the cycle land on a syntax-gloss, collapsing the two stops
/// the review confirmed must stay distinct. Use this set for anything that
/// should behave as if syntax-gloss doesn't exist (e.g. `try_open_gloss_at_cursor`,
/// the cycle's GLOSS stop).
const READER_GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];

/// Every gloss type the user could plausibly have been looking at, including
/// `syntax-gloss`. Use this set for anything that reopens "whatever gloss was
/// last shown" without caring which stop of the cycle it belonged to (e.g.
/// `open_last_gloss` — Ctrl+Shift+g must find a syntax-gloss it just recorded,
/// or the passage is unfindable and the bind goes dead).
const ANY_GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss", "syntax-gloss"];

/// Jump the reader cursor to the first dialogue line of the glossed passage's
/// source text (located by matching the gloss's first source line, then
/// advanced to the first `is_dialogue` line at or after it). Returns true if
/// it jumped.
///
/// Resolves by citation tuple `(div1,div2,line_in_div)` first (unique, avoids
/// landing on the wrong occurrence of a repeated source line); falls back to
/// text match for citationless (`.txt`-only) glosses.
///
/// Returns `false` if the current gloss context, work, or matching line can't
/// be resolved, so the caller can restore the saved page instead.
pub(crate) fn jump_to_gloss_source_start(s: &mut AppState) -> bool {
    let start_idx = match gloss_passage_start_idx(s) {
        Some(i) => i,
        None => return false,
    };
    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let work_idx = work.lines[start_idx..]
        .iter()
        .position(|l| l.is_dialogue)
        .map(|off| start_idx + off)
        .unwrap_or(start_idx);
    jump_to_work_idx(s, work_idx)
}

/// Work-line index where the glossed passage's source text starts, or None
/// when the gloss context/work/line can't be resolved.
///
/// `start_citation` is `ABBR.div1.div2.line_in_div`; the gloss strips any
/// `-Amb` suffix from the abbrev, so match on the numeric tail rather than the
/// full citation string. -Amb editions render the canonical parity-numbered
/// .txt (verified 2026-06-25; base and -Amb share text_file and
/// (div1,div2,line_in_div)). Resolve by the citation tuple first — it is
/// unique, so a repeated source line can't land on the wrong occurrence. Text
/// match is the citationless (.txt-only) fallback.
fn gloss_passage_start_idx(s: &AppState) -> Option<usize> {
    let ctx = s.gloss_context.as_ref()?;
    let work = s.current_work.as_ref()?;
    let target = crate::app::parse_citation(&ctx.start_citation);
    let by_citation = target
        .and_then(|t| work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t));
    by_citation.or_else(|| {
        let first_src = ctx.source_text.lines().next().map(str::trim).unwrap_or("");
        if first_src.is_empty() {
            None
        } else {
            work.lines.iter().position(|l| l.text.trim() == first_src)
        }
    })
}

/// Land the reader on `work_idx`: resolve it to a buffer line through the
/// line map, then jump. Use jump_to_line, not center-on-cursor: when the
/// source passage opens a scene (e.g. H8 Porter at (5,3,1)), naive centering
/// lets the scene-break clamp pull the spread back to the PREVIOUS scene,
/// leaving the cursor off-page. jump_to_line lands on the canonical spread
/// for the line in EReader mode (the same page paging through the work would
/// show).
fn jump_to_work_idx(s: &mut AppState, work_idx: usize) -> bool {
    let buf_idx = if let Some(ref lm) = s.line_map {
        match lm.work_to_buffer.get(work_idx) {
            Some(&bi) => bi,
            None => return false,
        }
    } else {
        work_idx
    };
    crate::input::navigation::jump_to_line(s, buf_idx);
    true
}

/// Escape-close landing for a MOVED overlay cursor: resolve the overlay's
/// selected block to its governing SOURCE excerpt (the block itself when the
/// cursor sits on source verse; the nearest source block ABOVE when it sits
/// on an explication paragraph) and land the reader on that excerpt's first
/// line. Occurrence-counted across the gloss's source blocks so a repeated
/// line (refrain) resolves by position, not first-match. Returns false when
/// the block or its line can't be resolved, so the caller can fall back to
/// the passage-start jump / saved-page restore.
pub(crate) fn jump_to_gloss_cursor_source(s: &mut AppState, kind: BlockKind, index: i32) -> bool {
    let gloss_text = match s.gloss_list.get(s.gloss_index) {
        Some(g) => g.gloss_text.clone(),
        None => return false,
    };
    let blocks = crate::ui::gloss_block::gloss_blocks(&gloss_text);
    let pos = match blocks.iter().position(|b| b.kind == kind && b.index == index) {
        Some(p) => p,
        None => return false,
    };
    let src_pos = match blocks[..=pos].iter().rposition(|b| b.kind == BlockKind::Source) {
        Some(p) => p,
        None => return false,
    };
    let needle = match blocks[src_pos]
        .display
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
    {
        Some(l) => l.to_string(),
        None => return false,
    };
    // Which occurrence of `needle` (among the gloss's source lines, in
    // document order) opens this block.
    let mut remaining = blocks[..src_pos]
        .iter()
        .filter(|b| b.kind == BlockKind::Source)
        .flat_map(|b| b.display.lines())
        .filter(|l| l.trim() == needle)
        .count();

    let start_idx = match gloss_passage_start_idx(s) {
        Some(i) => i,
        None => return false,
    };
    // Bound the scan to the passage's span (+ slack for the speaker/stage rows
    // the gloss's segment lines don't carry).
    let span = s
        .gloss_context
        .as_ref()
        .map(|c| c.source_text.lines().count())
        .unwrap_or(0)
        + 8;
    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let mut target = None;
    for (off, l) in work.lines[start_idx..].iter().take(span).enumerate() {
        if l.text.trim() == needle {
            if remaining == 0 {
                target = Some(start_idx + off);
                break;
            }
            remaining -= 1;
        }
    }
    let hit = match target {
        Some(t) => t,
        None => return false,
    };
    // A block can open on a stage direction; keep the reader cursor on
    // dialogue, same as the passage-start jump.
    let work_idx = work.lines[hit..]
        .iter()
        .position(|l| l.is_dialogue)
        .map(|off| hit + off)
        .unwrap_or(hit);
    jump_to_work_idx(s, work_idx)
}

pub(crate) fn navigate_gloss_passage(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    // Space source loop: nav-stop (keep the loop mpv for a quick re-space on
    // the new passage; the main player stays paused).
    crate::input::actions::chat::chat_loop_stop(&mut s);
    // Moving to a different passage invalidates any diff-highlight from a
    // custom-prompt rewrite on the passage we're leaving (Task 7).
    s.gloss_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;

    let work_abbrev = match &s.gloss_context {
        Some(ctx) => ctx.work_abbrev.clone(),
        None => return,
    };

    // Navigate only between passages that have a gloss of the type currently on
    // screen, and only show that type. The displayed type is authoritative on
    // `gloss_list[gloss_index]` (gloss_context.gloss_type can lag after Alt+n/p
    // within-passage cycling); fall back to gloss_context if the list is empty.
    let cur_type = s
        .gloss_list
        .get(s.gloss_index)
        .map(|g| g.gloss_type.clone())
        .or_else(|| s.gloss_context.as_ref().map(|c| c.gloss_type.clone()));
    let cur_type = match cur_type {
        Some(t) => t,
        None => return,
    };

    // Locate where we are now (by start citation) so we can step within this work.
    let cur_start = match &s.gloss_context {
        Some(ctx) => ctx.start_citation.clone(),
        None => return,
    };

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };

    // Rebuild the passage list filtered to the current type every time: the
    // filter type can change between calls (when the user switches types via
    // Alt+n/p), so the stored 3-type `gloss_passages` can't be reused here.
    let passages =
        crate::db::queries::find_glossed_passages(&conn, &work_abbrev, &[cur_type.as_str()])
            .unwrap_or_default();
    if passages.is_empty() {
        return;
    }

    // Locate the current passage by START citation only. The displayed gloss's
    // own end_citation can differ from the passage's (glosses sharing a start
    // may span to different ends — see the footer-citation note in
    // open_gloss_overlay), so matching start AND end could miss and silently
    // fall back to index 0 — which made Ctrl+n at the last passage "cycle" to
    // the second. Start is the passage key in this list. If even that misses,
    // abort rather than fall back to 0 (the spurious-jump bug).
    let cur_idx = match passages.iter().position(|p| p.start_citation == cur_start) {
        Some(i) => i,
        None => return,
    };

    // Clamp at the ends rather than wrapping: Ctrl+p stops at the first passage
    // of this type, Ctrl+n stops at the last.
    let len = passages.len();
    let target = cur_idx as i32 + delta;
    let new_idx = target.clamp(0, len as i32 - 1) as usize;
    if new_idx == cur_idx {
        return;
    }

    let passage = passages[new_idx].clone();
    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        &[cur_type.as_str()],
    )
    .unwrap_or_default();
    if all_glosses.is_empty() {
        return;
    }

    // Audio is already stopped by the Ctrl+n/p handler before this is called.
    let from_picker = s.gloss_opened_from_picker;
    open_gloss_overlay(
        &mut s,
        passages,
        new_idx,
        passage,
        all_glosses,
        from_picker,
        Some(&cur_type),
        false,
    );
}

pub(crate) fn navigate_gloss(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    // Moving to a different gloss within the passage invalidates any
    // diff-highlight from a custom-prompt rewrite on the gloss we're leaving
    // (Task 7).
    s.gloss_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    let len = s.gloss_list.len();
    if len == 0 {
        return;
    }
    let new_idx = ((s.gloss_index as i32 + delta).rem_euclid(len as i32)) as usize;
    if new_idx == s.gloss_index {
        return;
    }
    s.gloss_index = new_idx;
    s.gloss_active_voice = 0;
    // Footer cites the DISPLAYED gloss's own passage span (glosses in this list
    // share a start_citation but may have different end_citations).
    render_gloss_row(&mut s, new_idx);
}

/// `/` in the gloss overlay: open the reader `search_bar` to type a regex to
/// search the CURRENT gloss buffer. Mirrors `journal::open_overlay_search`, but
/// records the gloss overlay as the search origin so the shared
/// `OverlaySearchInput` handler routes Return/Escape back here.
///
/// BORROW SAFETY: `search_bar.show()` synchronously calls `entry.set_text("")`
/// and `grab_focus()`; the bar's Entry has NO `changed`/signal handler that
/// re-enters `state`, so this is safe under a short borrow. Still, scope the
/// widget borrow and set the fields in a fresh borrow (consistent with journal).
pub(crate) fn open_overlay_search(state: &Rc<RefCell<AppState>>) {
    {
        let s = state.borrow();
        s.search_bar.show();
    }
    let mut s = state.borrow_mut();
    s.overlay_search_origin = crate::app::InputMode::GlossOverlay;
    s.input_mode = crate::app::InputMode::OverlaySearchInput;
}

/// Return in the `/` bar (gloss origin): read the typed regex, hide the bar,
/// return to the gloss overlay, and set the pattern on the gloss buffer. Empty
/// query is a no-op (search stays whatever it was). Stores the pattern as the
/// gloss MRU. Mirrors `journal::confirm_overlay_search`.
///
/// BORROW SAFETY: read `query()` and `hide()` under scoped borrows dropped
/// before the mutating `borrow_mut`, so no `&s` getter borrow is held across the
/// `set_search_matches` write to `s.gloss_search`.
pub(crate) fn confirm_overlay_search(state: &Rc<RefCell<AppState>>) {
    let query = {
        let s = state.borrow();
        s.search_bar.query()
    };
    {
        let s = state.borrow();
        s.search_bar.hide();
    }
    state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
    let pattern = query.trim();
    if pattern.is_empty() {
        return;
    }
    let mut s = state.borrow_mut();
    // Collect over the WHOLE gloss (every page), not just the rendered buffer, so
    // a match on another page of a paginated gloss is found. Offsets are into the
    // whole-gloss rendered basis (`whole_entry_text` == `full_rendered_gloss_text`,
    // the same basis `page_char_span` measures); `jump_to_whole_offset` turns to
    // the match's page and `set_search_matches` paints whatever falls on it.
    let text = s.gloss_overlay.whole_entry_text();
    let search = crate::input::overlay_search::OverlaySearch {
        pattern: pattern.to_string(),
        matches: crate::input::overlay_search::collect(&text, pattern),
        current: 0,
    };
    if search.matches.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_NO_MATCHES, 2);
    } else if let Some((off, _)) = search.matches.first() {
        s.gloss_overlay.jump_to_whole_offset(*off as usize);
    }
    s.gloss_overlay.set_search_matches(&search);
    s.gloss_last_pattern = Some(pattern.to_string());
    s.gloss_search = Some(search);
}

/// n / N in the gloss overlay: step matches within the current gloss buffer. If
/// no live search but an MRU pattern exists, revive it first (post-Escape n/N).
/// Mirrors `journal::step_overlay_search`.
///
/// BORROW SAFETY: one `borrow_mut` held throughout. `s.gloss_search` (mutable)
/// and `s.gloss_overlay` (getter, immutable) alias `s`, so the tags are cloned +
/// the buffer taken into locals FIRST; the mutable step of `search.current`
/// happens in a scoped block; then `apply` is called on the locals with
/// `search.as_ref()` — no getter borrow overlaps the mutable use.
pub(crate) fn step_overlay_search(state: &Rc<RefCell<AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    if s.gloss_search.is_none() {
        // Revive the MRU pattern (post-Escape n/N). Collect over the WHOLE gloss
        // so revived stepping also crosses page boundaries.
        let Some(pat) = s.gloss_last_pattern.clone() else {
            return;
        };
        let text = s.gloss_overlay.whole_entry_text();
        let search = crate::input::overlay_search::OverlaySearch {
            pattern: pat.clone(),
            matches: crate::input::overlay_search::collect(&text, &pat),
            current: 0,
        };
        if search.matches.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_NO_MATCHES, 2);
            return;
        }
        if let Some((off, _)) = search.matches.first() {
            s.gloss_overlay.jump_to_whole_offset(*off as usize);
        }
        s.gloss_overlay.set_search_matches(&search);
        s.gloss_search = Some(search);
    }
    let scroll_to = {
        let search = s.gloss_search.as_mut().unwrap();
        match crate::input::overlay_search::step(search.current, search.matches.len(), forward) {
            Some(next) => {
                search.current = next;
                search.matches.get(next).map(|(a, _)| *a)
            }
            None => None,
        }
    };
    // Jump to the match's page (whole-body offset) and re-paint the highlights on
    // whatever page is now shown. Clone is a cheap snapshot to satisfy the borrow
    // checker (set_search_matches takes &self while s.gloss_search is borrowed).
    if let Some(off) = scroll_to {
        let search = s.gloss_search.clone().unwrap();
        s.gloss_overlay.set_search_matches(&search);
        s.gloss_overlay.jump_to_whole_offset(off as usize);
    } else if let Some(search) = s.gloss_search.clone() {
        s.gloss_overlay.set_search_matches(&search);
    }
}

/// Clear the active gloss overlay search (Escape). Keeps `gloss_last_pattern`
/// for MRU revival. Returns `true` when it cleared a live search (caller then
/// stays in the overlay), `false` when there was none (caller falls to the
/// existing close). Mirrors `journal::clear_overlay_search`.
///
/// BORROW SAFETY: single `borrow_mut`; tags cloned + buffer taken into locals
/// before `clear`, so no getter borrow overlaps writing `s.gloss_search`.
pub(crate) fn clear_overlay_search(state: &Rc<RefCell<AppState>>) -> bool {
    let mut s = state.borrow_mut();
    if s.gloss_search.is_none() {
        return false;
    }
    s.gloss_overlay.clear_search_tags();
    s.gloss_search = None;
    true
}

pub(crate) fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
    let copied = {
        let s = state.borrow();
        s.gloss_list.get(s.gloss_index).map(|gloss| {
            // Copy the id prefaced with a label so a paste self-identifies.
            let copied = format!("Gloss ID: {}", gloss.gloss_id);
            crate::ui::copy_to_clipboard(&copied);
            crate::logging::log(&format!("GLOSS: copied \"{}\" to clipboard", copied));
            copied
        })
    };
    // Toast AFTER dropping the borrow — show_tts_toast re-borrows state. Mirrors
    // the journal overlay's `c` (copy id) toast.
    if let Some(copied) = copied {
        show_tts_toast(state, &format!("Copied {}", copied));
    }
}

/// Delete a gloss row plus its cached TTS audio: the DB row (`delete_gloss`),
/// its audio rows (`delete_gloss_audio`), and the on-disk mp3 dir. Returns
/// `(audio_rows, mp3_files)` for the caller's verification toast. Shared by
/// the gloss overlay's `D` and the chat panel's `D` so the two purge paths
/// cannot drift. `work_abbrev: None` skips the on-disk dir (no context to
/// locate it) — the DB purge still runs.
pub(crate) fn purge_gloss_data(work_abbrev: Option<&str>, gloss_id: i64) -> (usize, usize) {
    let mut audio_rows = 0usize;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::delete_gloss(&conn, gloss_id);
        audio_rows = crate::db::queries::delete_gloss_audio(&conn, gloss_id).unwrap_or(0);
    }
    let mut mp3_files = 0usize;
    if let Some(abbrev) = work_abbrev {
        let dir = gloss_audio_dir(abbrev, gloss_id);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            mp3_files = entries
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("mp3"))
                .count();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    (audio_rows, mp3_files)
}

pub(crate) fn delete_current_gloss(state_rc: &Rc<RefCell<AppState>>) {
    // Phase 1: delete under a mutable borrow and gather counts for the
    // verification pill (audio rows purged + .mp3 files removed). The borrow is
    // released before toasting, since show_tts_toast borrows state again.
    let toast_msg;
    {
        let mut s = state_rc.borrow_mut();
        let idx = s.gloss_index;
        let Some(gloss) = s.gloss_list.get(idx) else { return };
        let gloss_id = gloss.gloss_id;

        let abbrev = s.gloss_context.as_ref().map(|c| c.work_abbrev.clone());
        let (audio_rows, mp3_files) = purge_gloss_data(abbrev.as_deref(), gloss_id);

        crate::logging::log(&format!(
            "GLOSS: deleted gloss {} ({} audio rows, {} mp3 files)",
            gloss_id, audio_rows, mp3_files
        ));
        toast_msg = format!(
            "Deleted gloss {} · {} mp3{}",
            gloss_id,
            mp3_files,
            if mp3_files == 1 { "" } else { "s" }
        );

        s.gloss_list.remove(idx);

        if s.gloss_list.is_empty() {
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
        } else {
            s.gloss_index = idx.min(s.gloss_list.len() - 1);
            s.gloss_active_voice = 0;
            let new_idx = s.gloss_index;
            // Footer cites the now-displayed gloss's own passage span.
            render_gloss_row(&mut s, new_idx);
        }

        // The gloss row was deleted from the DB above, so the glossed-passage set
        // changed. Recompute the main-card reader-gloss tint from the REMAINING
        // glossed passages — otherwise the just-deleted passage's lines stay
        // tinted (the "coloring persists after delete" bug). This clears the tag
        // buffer-wide and re-derives, so it also corrects the now-fully-unglossed
        // case when the last gloss on a passage is removed.
        crate::app::apply_reader_gloss_highlighting(&mut s);
    }
    // Phase 2: verification pill (borrow released above). Shown whether or not
    // the overlay closed, so the user always gets confirmation.
    show_tts_toast(state_rc, &toast_msg);
}

/// Open the "Delete …? y / Esc" confirmation over `origin`'s overlay. Records
/// `origin` (gloss overlay, journal overlay, or chat transcript) so `y` runs
/// the right delete and returns to the right mode; the dialog label names
/// what will be deleted. No-op when there is nothing to delete for that
/// overlay. Mirrors `show_undo_confirmation`.
pub(crate) fn show_delete_confirmation(
    state_rc: &Rc<RefCell<AppState>>,
    origin: crate::app::InputMode,
) {
    // Resolve the label + bail with no dialog if there is nothing to delete.
    // For the ChatTranscript origin, also capture the (view kind, row id)
    // target NOW — at dialog-open time — so it survives async state mutation
    // between `D` and `y` (see `AppState::delete_confirm_target`).
    let mut target: Option<(crate::input::actions::chat::PanelView, i64)> = None;
    let title = {
        let s = state_rc.borrow();
        match origin {
            crate::app::InputMode::GlossOverlay => match s.gloss_list.get(s.gloss_index) {
                Some(g) => format!("Delete gloss {}?", g.gloss_id),
                None => return,
            },
            crate::app::InputMode::JournalOverlay => {
                if s.journal.pages.is_empty() {
                    return;
                }
                "Delete this Q&A?".to_string()
            }
            crate::app::InputMode::ChatTranscript => {
                use crate::input::actions::chat::PanelView;
                match s.chat.view {
                    PanelView::Gloss => match s.chat.gloss_list.get(s.chat.gloss_index) {
                        Some(g) => {
                            target = Some((PanelView::Gloss, g.gloss_id));
                            format!("Delete gloss {}?", g.gloss_id)
                        }
                        None => return,
                    },
                    PanelView::Journal => {
                        match s.chat.journal_list.get(s.chat.journal_cursor) {
                            Some(p) => {
                                target = Some((PanelView::Journal, p.id));
                                format!("Delete journal {}?", p.id)
                            }
                            None => return,
                        }
                    }
                    // No deletable item is displayed in Question view — the
                    // dialog never opens there (the panel's D is view-gated
                    // here, not at the bind).
                    PanelView::Question => return,
                }
            }
            _ => return,
        }
    };

    // The dialog must stack above EVERYTHING, including the chat panel. The
    // action popup lives on the INNER (corpus_search_popup) overlay, but the
    // chat panel is a child of the window-level OUTER overlay, which draws
    // over the whole inner stack — a dialog added to the popup's immediate
    // parent renders UNDER the panel. Climb to the outermost Overlay
    // ancestor instead; every origin's dialog is centered the same way there.
    let overlay_parent = {
        let s = state_rc.borrow();
        let mut widget = s.action_popup_widget.container.parent();
        let mut outermost: Option<gtk4::Overlay> = None;
        while let Some(w) = widget {
            if let Ok(o) = w.clone().downcast::<gtk4::Overlay>() {
                outermost = Some(o);
            }
            widget = w.parent();
        }
        outermost
    };
    let overlay_parent = match overlay_parent {
        Some(o) => o,
        None => return,
    };

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(400);
    container.add_css_class("amend-dialog");

    let label = gtk4::Label::new(Some(&title));
    label.add_css_class("amend-title");
    label.set_halign(gtk4::Align::Center);
    container.append(&label);

    let hint = gtk4::Label::new(Some("y = confirm  \u{00b7}  Esc = cancel"));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    let mut s = state_rc.borrow_mut();
    s.delete_confirm_container = Some(container.downgrade());
    s.delete_confirm_overlay = Some(overlay_parent.downgrade());
    s.delete_confirm_origin = Some(origin);
    s.delete_confirm_target = target;
    s.input_mode = crate::app::InputMode::DeleteConfirm;
}

pub(crate) fn close_delete_confirmation(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (s.delete_confirm_container.take(), s.delete_confirm_overlay.take()) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    // Return to the overlay the `D` was pressed in (gloss or journal), defaulting
    // to the gloss overlay for safety if the origin was somehow not recorded.
    s.input_mode = s.delete_confirm_origin.take().unwrap_or(crate::app::InputMode::GlossOverlay);
    // Cancel/close clears the captured target too — `y`'s handler reads it
    // BEFORE calling this function, so this only matters for the Escape/`n`
    // path (defensive: no target should be read after this point either way).
    s.delete_confirm_target = None;
}

/// Open the "Undo last edit? y / Esc" confirmation over `origin`'s overlay (the
/// gloss / synopsis / journal overlay that the `u` key was pressed in). Records
/// `origin` so `y` runs the right overlay's undo and returns to the right mode.
/// No-op (toast) when there is no pending edit to undo for that overlay. Mirrors
/// `show_delete_confirmation`'s centered amend-dialog box.
pub(crate) fn show_undo_confirmation(
    state_rc: &Rc<RefCell<AppState>>,
    origin: crate::app::InputMode,
) {
    // Bail with a toast if there is nothing to undo for the originating overlay.
    let has_undo = {
        let s = state_rc.borrow();
        match origin {
            crate::app::InputMode::GlossOverlay => s.gloss_undo.is_some(),
            crate::app::InputMode::SynopsisOverlay => s.synopsis_undo.is_some(),
            crate::app::InputMode::JournalOverlay => s.journal_undo.is_some(),
            _ => false,
        }
    };
    if !has_undo {
        show_tts_toast(state_rc, "Nothing to undo");
        return;
    }

    let overlay_parent = {
        let s = state_rc.borrow();
        s.action_popup_widget.container.parent()
    };
    let overlay_parent = match overlay_parent.and_then(|p| p.downcast::<gtk4::Overlay>().ok()) {
        Some(o) => o,
        None => return,
    };

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(400);
    container.add_css_class("amend-dialog");

    let label = gtk4::Label::new(Some("Undo last edit?"));
    label.add_css_class("amend-title");
    label.set_halign(gtk4::Align::Center);
    container.append(&label);

    let hint = gtk4::Label::new(Some("y = confirm  \u{00b7}  Esc = cancel"));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    let mut s = state_rc.borrow_mut();
    s.undo_confirm_container = Some(container.downgrade());
    s.undo_confirm_overlay = Some(overlay_parent.downgrade());
    s.undo_confirm_origin = Some(origin);
    s.input_mode = crate::app::InputMode::UndoConfirm;
}

/// Tear down the undo confirmation box and return to the originating overlay
/// mode (or Reader if the origin was somehow lost). Clears the origin marker.
pub(crate) fn close_undo_confirmation(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (s.undo_confirm_container.take(), s.undo_confirm_overlay.take()) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    s.input_mode = s.undo_confirm_origin.take().unwrap_or(crate::app::InputMode::Reader);
}

fn show_prompt_dialog(state_rc: &Rc<RefCell<AppState>>, mode: crate::app::GlossPromptMode) {
    let (is_inner_monologue, is_reader_gloss, is_edit) = {
        let s = state_rc.borrow();
        let gloss_type = s.gloss_context.as_ref()
            .map(|ctx| ctx.gloss_type.as_str().to_string())
            .unwrap_or_default();
        (
            gloss_type == "inner-monologue",
            gloss_type == "reader-gloss",
            mode == crate::app::GlossPromptMode::Edit,
        )
    };
    let is_fix_ipa = mode == crate::app::GlossPromptMode::FixIpa;
    let is_passage_qa = mode == crate::app::GlossPromptMode::PassageQa;

    let title_text = if is_passage_qa {
        // Gloss-overlay Ctrl+a: journal passage Q&A (typed here, answered in the
        // journal overlay). Matches the journal card's wording.
        "Ask a question about this passage"
    } else if is_fix_ipa {
        "FIX IPA — word /IPA/  OR  word <hint>"
    } else if is_edit {
        // Mirrors the journal overlay's rewrite card: the input is an
        // INSTRUCTION for the rewrite (often posed as questions the rewrite
        // should answer), not replacement text.
        "Rewrite instruction — questions welcome"
    } else if is_inner_monologue {
        "Inner monologue passage"
    } else if is_reader_gloss {
        "Ask a question about the passage"
    } else {
        "Ask a question about the passage"
    };
    let hint_text = if is_fix_ipa {
        "e.g. `daily /\u{02c8}de\u{026a}li/` or `daily hard a`  \u{00b7}  Ctrl+Enter submit"
    } else if is_inner_monologue {
        "Paste lines from another work  \u{00b7}  Ctrl+Enter submit"
    } else {
        "Ctrl+Enter submit"
    };
    // Centered how-to watermark over the empty input (mirrors the journal Q&A
    // rewrite box). Only the Edit card offers the default-prompt rewrite, so
    // only it carries a legend; the others opt out with "".
    let legend_text = if is_edit {
        "Ctrl+Enter with NO instruction\nrewrites the gloss afresh under the default prompt."
    } else {
        ""
    };

    // Stack the input as a card below the open gloss (same widget the synopsis
    // "ask" flow uses) instead of a separate floating dialog. The gloss card
    // stays visible above it; `gloss_prompt_mode` routes the eventual submit.
    {
        let mut s = state_rc.borrow_mut();
        s.gloss_prompt_mode = mode;
        // Ctrl+Tab focus toggle: a freshly opened ask card always starts focused.
        s.ask_card_focus = true;
    }
    {
        let s = state_rc.borrow();
        s.gloss_overlay.open_ask_card_with(
            title_text,
            hint_text,
            legend_text,
            &s.theme.cursor_bg,
            &s.theme.cursor_fg,
        );
    }
}

pub(crate) fn show_edit_dialog(state_rc: &Rc<RefCell<AppState>>) {
    show_prompt_dialog(state_rc, crate::app::GlossPromptMode::Edit);
}

/// Open the fix-IPA input card for the cursor's source verse. No-op (toast) off
/// a source block. (Formerly the gloss-overlay `i` key — that bind was removed;
/// kept so the FixIpa flow can be rebound later without reconstruction.)
#[allow(dead_code)]
pub(crate) fn open_fix_ipa_prompt(state_rc: &Rc<RefCell<AppState>>) {
    if source_block_index(state_rc).is_none() {
        return; // not a source block — `source_block_index` toasted already
    }
    show_prompt_dialog(state_rc, crate::app::GlossPromptMode::FixIpa);
}

/// Gloss-overlay `i` submit: parse `word [/IPA/ | hint]` and fix the word's OP
/// IPA in the cursor's source verse, splice it into `gloss_text`, persist,
/// drop the source block's cached audio, patch the in-memory gloss, and
/// re-synthesize + play. Two paths: a literal `/IPA/` (applied directly, no
/// LLM) and a plain hint (LLM resolves the IPA, then applies the same way).
pub(crate) fn fix_word_ipa(state_rc: &Rc<RefCell<AppState>>, input: &str) {
    // 1. Parse `word <rest>`.
    let trimmed = input.trim();
    let (word, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((w, r)) => (w.trim().to_string(), r.trim().to_string()),
        None => {
            show_tts_toast(state_rc, "Usage: word /IPA/  or  word <hint>");
            return;
        }
    };
    if word.is_empty() || rest.is_empty() {
        show_tts_toast(state_rc, "Usage: word /IPA/  or  word <hint>");
        return;
    }

    // 2. Resolve the cursor's source block under a single borrow, then drop it
    //    before any toast / async spawn. On a non-Source block, a missing gloss,
    //    or a block index not present in the parsed gloss, toast and bail.
    enum Resolve {
        Ok {
            gloss_index_pos: usize,
            gloss_id: i64,
            block_index: i32,
            gloss_text: String,
        },
        NotSource,
        NoGloss,
    }
    let resolved = {
        let s = state_rc.borrow();
        match s.gloss_overlay.current_block() {
            Some((BlockKind::Source, block_index)) => {
                let gloss_index_pos = s.gloss_index;
                match s.gloss_list.get(gloss_index_pos) {
                    Some(g) => {
                        let gloss_text = g.gloss_text.clone();
                        let blocks = crate::ui::gloss_block::gloss_blocks(&gloss_text);
                        if blocks
                            .iter()
                            .any(|b| b.kind == BlockKind::Source && b.index == block_index)
                        {
                            Resolve::Ok {
                                gloss_index_pos,
                                gloss_id: g.gloss_id,
                                block_index,
                                gloss_text: gloss_text.clone(),
                            }
                        } else {
                            Resolve::NotSource
                        }
                    }
                    None => Resolve::NoGloss,
                }
            }
            _ => Resolve::NotSource,
        }
    };

    let (gloss_index_pos, gloss_id, block_index, gloss_text) = match resolved {
        Resolve::Ok {
            gloss_index_pos,
            gloss_id,
            block_index,
            gloss_text,
        } => (gloss_index_pos, gloss_id, block_index, gloss_text),
        Resolve::NotSource => {
            show_tts_toast(state_rc, "Source verse only");
            return;
        }
        Resolve::NoGloss => {
            show_tts_toast(state_rc, "No gloss");
            return;
        }
    };

    // 3. Literal `/IPA/` -> apply directly; plain hint -> ask the LLM first.
    if crate::ui::gloss_ipa::contains_ipa_span(&rest) {
        let new_ipa = first_ipa_span(&rest);
        apply_ipa_fix(
            state_rc,
            gloss_index_pos,
            gloss_id,
            block_index,
            &gloss_text,
            &word,
            &new_ipa,
        );
    } else {
        request_ipa_then_apply(
            state_rc,
            gloss_index_pos,
            gloss_id,
            block_index,
            gloss_text,
            word,
            rest,
        );
    }
}

/// The first inline IPA span (with slashes) in `s`, e.g. "daily /ˈdeɪli/" ->
/// "/ˈdeɪli/". Falls back to the trimmed input if no span (callers only use
/// this when `contains_ipa_span` is true).
fn first_ipa_span(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '/') {
                let close = i + 1 + rel;
                let inner = &chars[i + 1..close];
                if !inner.is_empty() && inner.iter().any(|&c| !c.is_ascii_alphabetic()) {
                    return chars[i..=close].iter().collect();
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    s.trim().to_string()
}

/// Shared back half for both the literal and hint paths: swap `word`'s IPA in
/// the source block, splice the rewritten block into `gloss_text`, persist via
/// `update_gloss`, delete the block's cached audio (DB rows + files), patch the
/// in-memory gloss so `play_block_tts` reads the corrected verse, then
/// re-synthesize + play (pausing MPV first). Holds no borrow across the toast /
/// play calls.
#[allow(clippy::too_many_arguments)]
fn apply_ipa_fix(
    state_rc: &Rc<RefCell<AppState>>,
    gloss_index_pos: usize,
    gloss_id: i64,
    block_index: i32,
    gloss_text: &str,
    word: &str,
    new_ipa: &str,
) {
    // Splice the rewritten IPA into the TAGGED gloss_text, scoped to this source
    // block's `<segment>` span. Operating on block.text directly was a no-op for
    // multi-line verse: block.text joins verse lines with '\n' (tags stripped),
    // but gloss_text separates them with `</segment>\n<segment>`, so block.text is
    // not a substring of gloss_text for any 2+ line block.
    let new_gloss_text = match crate::ui::gloss_block::replace_word_ipa_in_source_block(
        gloss_text,
        block_index,
        word,
        new_ipa,
    ) {
        Some(t) => t,
        None => {
            state_rc.borrow().gloss_overlay.stop_loading();
            show_tts_toast(state_rc, &format!("No IPA for {}", word));
            return;
        }
    };

    // Persist the corrected gloss and invalidate this block's cached audio.
    // Stamp with the configured model — the IPA fix that produced this revised
    // text was generated by it (see open_fix_ipa_prompt's call_claude_with_prompt).
    let model = state_rc.borrow().config.claude_model.clone();
    let removed: Vec<String> = match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            let _ = crate::db::queries::update_gloss(&conn, gloss_id, &new_gloss_text, &model);
            crate::db::queries::delete_gloss_audio_block(
                &conn,
                gloss_id,
                "source",
                block_index as i64,
            )
            .unwrap_or_default()
        }
        Err(_) => {
            state_rc.borrow().gloss_overlay.stop_loading();
            show_tts_toast(state_rc, "Could not save IPA fix");
            return;
        }
    };
    for p in &removed {
        let _ = std::fs::remove_file(p);
    }

    // Patch the in-memory gloss BEFORE play so play_block_tts re-synthesizes the
    // corrected verse, not the stale IPA. Re-render the gloss card under the same
    // borrow: the hint/LLM path called `show_loading()` (which clears the cursor
    // `blocks` and shows a "Glossing..." spinner), so without this the corrected
    // verse would play but the card would stay stuck on the loading screen with
    // no navigable blocks. Re-rendering also refreshes the displayed inline IPA
    // on both paths.
    {
        let mut s = state_rc.borrow_mut();
        if let Some(g) = s.gloss_list.get_mut(gloss_index_pos) {
            g.gloss_text = new_gloss_text;
        }
        if let (Some(ctx), Some(gloss)) =
            (s.gloss_context.as_ref(), s.gloss_list.get(gloss_index_pos))
        {
            let (cw, h) = crate::app::layout::overlay_card_size(&s);
            let pairs = ctx.source_line_pairs();
            let gloss_text = gloss.gloss_text.clone();
            let source_text = ctx.source_text.clone();
            let root_color = s.theme.root_color.clone();
            let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
            s.gloss_overlay.show_gloss_with_color(
                &source_text,
                &gloss_text,
                cw,
                h,
                Some(&root_color),
                &pairs,
                (&head.0, &head.1),
            );
            s.gloss_overlay
                .set_position(gloss_index_pos, s.gloss_list.len());
            s.gloss_overlay
                .set_citation(&ctx.start_citation, &ctx.end_citation);
            recolor_cached_blocks(&s);
        }
    }
    crate::log_fmt!(
        "GLOSS: fixed IPA for '{}' in gloss {} source block {} -> {}",
        word,
        gloss_id,
        block_index,
        new_ipa
    );

    // Re-synthesize + play (pauses MPV first). No borrow held here.
    play_source_tts_pausing_mpv(state_rc, block_index);
}

/// Hint/LLM path: ask Claude for the OP IPA of `word` given `hint`, then apply
/// the result via `apply_ipa_fix`. Mirrors `edit_gloss`'s async shape
/// (`glib::spawn_future_local` + `tokio_handle.spawn` + `Ok(Ok(_))`). Captures
/// owned strings into the closure; the apply call runs after the await with no
/// outstanding borrow.
#[allow(clippy::too_many_arguments)]
fn request_ipa_then_apply(
    state_rc: &Rc<RefCell<AppState>>,
    gloss_index_pos: usize,
    gloss_id: i64,
    block_index: i32,
    gloss_text: String,
    word: String,
    hint: String,
) {
    let (model, tokio_handle) = {
        let s = state_rc.borrow();
        (s.config.claude_model.clone(), s.tokio_handle.clone())
    };

    state_rc.borrow().gloss_overlay.show_loading();

    let state_for_result = Rc::clone(state_rc);
    let user_msg = format!("word: {}\nhint: {}", word, hint);

    glib::spawn_future_local(async move {
        let system_prompt = crate::gloss::FIX_IPA_PROMPT.to_string();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(&system_prompt, &user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(reply)) => {
                if !crate::ui::gloss_ipa::contains_ipa_span(&reply) {
                    state_for_result.borrow().gloss_overlay.stop_loading();
                    show_tts_toast(&state_for_result, "Could not get IPA");
                    return;
                }
                let new_ipa = first_ipa_span(&reply);
                apply_ipa_fix(
                    &state_for_result,
                    gloss_index_pos,
                    gloss_id,
                    block_index,
                    &gloss_text,
                    &word,
                    &new_ipa,
                );
            }
            _ => {
                state_for_result.borrow().gloss_overlay.stop_loading();
                show_tts_toast(&state_for_result, "Could not get IPA");
            }
        }
    });
}

/// Render the gloss row at `new_idx` into the overlay. Shared by
/// `navigate_gloss` and `delete_current_gloss` (their render blocks were
/// byte-identical). Clones the strings that must outlive the `gloss_list`
/// borrow so `gloss_overlay` can be mutably borrowed in the same call.
pub(crate) fn render_gloss_row(s: &mut AppState, new_idx: usize) {
    let gloss = &s.gloss_list[new_idx];
    let gloss_start = gloss.start_citation.clone();
    let gloss_end = gloss.end_citation.clone();
    let gloss_text = gloss.gloss_text.clone();
    let ctx = s.gloss_context.as_ref().unwrap();
    let source_text = ctx.source_text.clone();
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let head = crate::app::division_synopsis::synopsis_head(s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &source_text, &gloss_text, cw, h,
        Some(&s.theme.root_color), &pairs, (&head.0, &head.1),
    );
    s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
    s.gloss_overlay.set_citation(&gloss_start, &gloss_end);
    recolor_cached_blocks(s);
    // Re-apply any active overlay search so a `/`-typed pattern keeps
    // highlighting across gloss/passage stepping. Re-collect against the NEW
    // gloss's WHOLE rendered text (every page), not the rendered buffer, so a
    // match on a later page is found too, then store + paint the current page.
    if s.gloss_search.is_some() {
        let text = s.gloss_overlay.whole_entry_text();
        if let Some(search) = s.gloss_search.as_mut() {
            search.matches = crate::input::overlay_search::collect(&text, &search.pattern);
            if search.current >= search.matches.len() {
                search.current = search.matches.len().saturating_sub(1);
            }
        }
        let search = s.gloss_search.clone().unwrap();
        s.gloss_overlay.set_search_matches(&search);
    }
    // Show the diff vs this gloss's last stored revision (or clear if none), so
    // landing on a gloss always highlights what its last rewrite changed.
    refresh_gloss_diff_highlight(s, new_idx);
}

/// Paint the diff between the gloss at `idx` and its most recent stored revision
/// (RAW markup), rendered to text so offsets match the displayed buffer. Clears
/// the highlight when the gloss has no revision history. Mirrors journal's
/// `refresh_entry_diff_highlight`; survives page turns via the overlay's per-page
/// re-apply.
pub(crate) fn refresh_gloss_diff_highlight(s: &mut AppState, idx: usize) {
    let Some(gloss) = s.gloss_list.get(idx) else {
        s.gloss_overlay.clear_rewrite_diff();
        return;
    };
    let (gloss_id, current_markup) = (gloss.gloss_id, gloss.gloss_text.clone());
    let latest = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::list_revisions(&conn, "gloss", gloss_id).ok())
        .and_then(|revs| revs.into_iter().last());
    let Some(prev) = latest else {
        s.gloss_overlay.clear_rewrite_diff();
        return;
    };
    let prev_rendered =
        crate::ui::gloss_overlay::GlossOverlay::full_rendered_gloss_text(&prev.body);
    let new_rendered =
        crate::ui::gloss_overlay::GlossOverlay::full_rendered_gloss_text(&current_markup);
    let ranges = crate::input::rewrite_diff::changed_ranges(&prev_rendered, &new_rendered);
    s.gloss_overlay.apply_rewrite_diff(&ranges);
}

/// Persist a freshly composed gloss, reload the start-citation gloss list,
/// select the new row, and render it into the gloss overlay. Shared by
/// `add_gloss` and `edit_gloss` (their success bodies were byte-identical here).
fn persist_and_render_gloss(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: &crate::gloss::GlossContext,
    full_gloss: &str,
    gloss_type: &str,
    model_for_db: &str,
    log_msg: &str,
) {
    let mut new_gloss_id: i64 = -1;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Ok(id) = crate::db::queries::save_gloss(
            &conn, &ctx.hash, &ctx.work_abbrev,
            &ctx.start_citation, &ctx.end_citation,
            ctx.act, ctx.scene, &ctx.speaker,
            &ctx.source_text, full_gloss,
            gloss_type, model_for_db,
        ) {
            new_gloss_id = id;
        }
    }

    let all = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).ok()
        })
        .unwrap_or_default();

    let new_idx = all.iter().position(|g| g.gloss_id == new_gloss_id).unwrap_or(0);

    let mut s = state_rc.borrow_mut();
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, full_gloss, cw, h,
        Some(&s.theme.root_color), &pairs, (&head.0, &head.1),
    );
    s.gloss_overlay.set_position(new_idx, all.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_list = all;
    s.gloss_index = new_idx;
    s.gloss_active_voice = 0;
    recolor_cached_blocks(&s);
    // Refresh the MAIN-CARD reader-gloss tint from the now-saved passages, so a
    // newly created reader-gloss colors its lines immediately — mirrors the
    // delete path (delete_current_gloss). Without this the tint only appeared
    // after the overlay was closed (whose close path runs the same recompute).
    // The overlay STAYS OPEN here, so we recompute directly rather than via
    // return_to_reader_mode (which would wrongly switch to Reader mode). A non
    // reader-gloss type adds no reader-gloss passage, so the re-derive is a no-op
    // (the buffer-wide tag clear at the top still runs, harmlessly).
    crate::app::apply_reader_gloss_highlighting(&mut s);
    crate::logging::log(log_msg);
}

/// Re-gloss the CURRENT gloss row IN PLACE (the `E` / edit flow): overwrite its
/// `gloss_text` via `update_gloss` (no new row), invalidate its cached TTS audio
/// (DB rows + on-disk mp3 dir, since the whole text changed), patch the
/// in-memory `gloss_list[gloss_index]`, and re-render the card at the SAME index.
/// Mirrors `apply_ipa_fix`'s in-place update path, but for the whole gloss rather
/// than one source block. Unlike `persist_and_render_gloss`, it does NOT insert a
/// new gloss or shift the position.
fn update_and_render_gloss_in_place(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: &crate::gloss::GlossContext,
    gloss_index: usize,
    gloss_id: i64,
    full_gloss: &str,
    model_for_db: &str,
    log_msg: &str,
    diff: Option<(&str, Option<&str>)>, // (prev_rendered_text, custom_prompt)
) {
    // Capture the PRE-rewrite RAW gloss text (the durable revision body) before
    // the in-memory row is overwritten below.
    let prev_raw = state_rc
        .borrow()
        .gloss_list
        .get(gloss_index)
        .map(|g| g.gloss_text.clone());

    // Persist the rewritten text in place and purge this gloss's cached audio
    // (every block's verse/answer may have changed, so drop all of it).
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let (Some(prev), Some((_, prompt))) = (prev_raw.as_ref(), diff) {
            if let Err(e) = crate::db::journal::append_revision(
                &conn, "gloss", gloss_id, None, prev, model_for_db, prompt,
            ) {
                crate::logging::log(&format!("REVISION: append failed: {}", e));
            }
        }
        let _ = crate::db::queries::update_gloss(&conn, gloss_id, full_gloss, model_for_db);
        let _ = crate::db::queries::delete_gloss_audio(&conn, gloss_id);
    }
    let _ = std::fs::remove_dir_all(gloss_audio_dir(&ctx.work_abbrev, gloss_id));

    let mut s = state_rc.borrow_mut();
    // Snapshot the pre-edit text for single-level undo (`u`) BEFORE overwriting
    // the in-memory row. Keyed by gloss_id so `u` can restore the exact row.
    if let Some(g) = s.gloss_list.get(gloss_index) {
        s.gloss_undo = Some((gloss_id, g.gloss_text.clone()));
    }
    // Patch the in-memory row so play_block_tts reads the rewritten text.
    if let Some(g) = s.gloss_list.get_mut(gloss_index) {
        g.gloss_text = full_gloss.to_string();
    }
    s.gloss_active_voice = 0;
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, full_gloss, cw, h,
        Some(&s.theme.root_color), &pairs, (&head.0, &head.1),
    );
    s.gloss_overlay.set_position(gloss_index, s.gloss_list.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_index = gloss_index;
    if let Some((prev_rendered, _)) = diff {
        // Diff against the WHOLE gloss's rendered text, NOT the current buffer:
        // long glosses paginate, and the buffer holds only page 1, so a
        // buffer-based diff would miss every change on page 2+. `prev_rendered`
        // is computed in the same full-render basis by the caller.
        let new_rendered =
            crate::ui::gloss_overlay::GlossOverlay::full_rendered_gloss_text(full_gloss);
        let ranges = crate::input::rewrite_diff::changed_ranges(prev_rendered, &new_rendered);
        s.gloss_overlay.apply_rewrite_diff(&ranges);
    }
    recolor_cached_blocks(&s);
    // The gloss text changed but the glossed-passage SET did not, so the
    // main-card reader-gloss tint is unchanged; recompute anyway for parity with
    // persist_and_render_gloss (harmless no-op for non reader-gloss types).
    crate::app::apply_reader_gloss_highlighting(&mut s);
    crate::logging::log(log_msg);
}

/// `e` in the gloss overlay: enter the in-place modal vim editor on the current
/// gloss's RAW markup (`gloss_list[gloss_index].gloss_text`). Toasts and bails
/// when there is no current gloss. The save path (`vim_save`) writes the buffer
/// back via `update_and_render_gloss_in_place` — no Claude.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    // Resolve the raw markup + cursor colors under a mutable borrow, then enter
    // the editor. `show_tts_toast` re-borrows `state`, so the no-gloss toast is
    // emitted only after the borrow is dropped.
    let entered = {
        let mut s = state.borrow_mut();
        let idx = s.gloss_index;
        match s.gloss_list.get(idx) {
            Some(g) => {
                let raw = g.gloss_text.clone();
                let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
                s.gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
                s.input_mode = crate::app::InputMode::GlossEdit;
                true
            }
            None => false,
        }
    };
    if !entered {
        show_tts_toast(state, "No gloss to edit");
    }
}

/// Save the gloss vim-editor buffer's raw markup to lit.db as-is (no Claude) via
/// `update_and_render_gloss_in_place` (which also snapshots `gloss_undo`, purges
/// cached audio, patches the in-memory row, and re-renders the colored display).
/// `:w` (quit=false) stays in the editor and re-seeds the dirty baseline; `:wq`
/// (quit=true) exits to the gloss overlay.
pub(crate) fn vim_save(state: &Rc<RefCell<AppState>>, quit: bool) {
    let raw = state.borrow().gloss_overlay.edit_buffer_text();
    let raw = raw.trim_end().to_string();
    let (ctx, idx, gloss_id, model) = {
        let s = state.borrow();
        let ctx = match &s.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let idx = s.gloss_index;
        let gloss_id = match s.gloss_list.get(idx) {
            Some(g) => g.gloss_id,
            None => return,
        };
        (ctx, idx, gloss_id, s.config.claude_model.clone())
    };
    update_and_render_gloss_in_place(
        state, &ctx, idx, gloss_id, &raw, &model,
        &format!("GLOSS: hand-edited gloss {} in place (vim)", gloss_id),
        None,
    );
    if quit {
        let mut s = state.borrow_mut();
        s.gloss_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED, 2);
    } else {
        // `update_and_render_gloss_in_place` re-rendered the COLORED read display
        // and does not know about the editor, so the editor view is now gone.
        // Re-open the editor on the just-saved raw text and re-seed the dirty
        // baseline so the user stays in mono-edit mode with a clean buffer.
        let (fill, fg) = {
            let s = state.borrow();
            (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone())
        };
        {
            let mut s = state.borrow_mut();
            s.gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
            s.gloss_overlay.reseed_edit_buffer(&raw);
            s.input_mode = crate::app::InputMode::GlossEdit;
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED_IN_OVERLAY, 2);
        }
    }
}

/// Leave the gloss vim editor. With unsaved changes and not `force`, warn and
/// STAY (`:q` refused on a modified buffer; `:q!` forces). Re-renders the colored
/// gloss display (from the unchanged stored gloss) on exit.
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().gloss_overlay.edit_is_dirty();
    if dirty && !force {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), "Unsaved changes \u{2014} :w to save, :q! to discard", 3);
        return;
    }
    // Drop the editor, then re-render the STORED (un-edited) gloss in its colored
    // form via the render-only `render_gloss_row` helper. Unlike
    // `update_and_render_gloss_in_place`, it does NOT re-persist the text or purge
    // cached audio — so a no-op cancel keeps the synthesized gloss audio. The
    // reader-gloss tint is untouched (the gloss text + glossed-passage set never
    // changed), so no `apply_reader_gloss_highlighting` is needed.
    let mut s = state.borrow_mut();
    s.gloss_overlay.exit_edit_buffer();
    let idx = s.gloss_index;
    if s.gloss_context.is_some() && s.gloss_list.get(idx).is_some() {
        render_gloss_row(&mut s, idx);
    }
    s.input_mode = crate::app::InputMode::GlossOverlay;
    crate::logging::log("GLOSS: vim edit cancelled, re-rendered stored gloss");
}

/// Ctrl+r in the gloss overlay read view: open the ask-Claude rewrite (edit)
/// prompt for the displayed gloss — entering the `e` editor first is unnecessary
/// (mirrors journal `begin_rewrite`; the vim editor's `R` was dropped
/// 2026-07-22). Opens in INSERT: a rewrite instruction is always typed fresh,
/// so skip vim-NORMAL (fed through the engine so the mirror and `-- INSERT --`
/// hint stay truthful).
pub(crate) fn begin_rewrite(state: &Rc<RefCell<AppState>>) {
    show_edit_dialog(state);
    let _ = state
        .borrow()
        .gloss_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}

/// Undo the last `e` gloss edit (single-level): restore the snapshot in
/// `gloss_undo` to the row it came from via `update_gloss`, purge that gloss's
/// cached audio (the text reverted), patch the in-memory row if it is the
/// currently-displayed gloss, re-render, and clear the snapshot. Toasts and
/// bails when there is nothing to undo, the work/gloss can't be resolved, or the
/// snapshot's gloss is no longer the one on screen. Called by the `u` undo
/// confirmation (`y`).
pub(crate) fn undo_gloss_edit(state_rc: &Rc<RefCell<AppState>>) {
    let snapshot = state_rc.borrow().gloss_undo.clone();
    let (gloss_id, original) = match snapshot {
        Some(snap) => snap,
        None => {
            show_tts_toast(state_rc, "Nothing to undo");
            return;
        }
    };

    // Resolve the displayed gloss's context + index; the undo only applies to the
    // gloss currently on screen (single-level, same row that `e` just edited).
    let (ctx, gloss_index) = {
        let s = state_rc.borrow();
        match (s.gloss_context.clone(), s.gloss_list.get(s.gloss_index)) {
            (Some(ctx), Some(g)) if g.gloss_id == gloss_id => (ctx, s.gloss_index),
            _ => {
                drop(s);
                show_tts_toast(state_rc, "Nothing to undo");
                return;
            }
        }
    };

    let model = state_rc.borrow().config.claude_model.clone();
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::queries::update_gloss(&conn, gloss_id, &original, &model);
        let _ = crate::db::queries::delete_gloss_audio(&conn, gloss_id);
    }
    let _ = std::fs::remove_dir_all(gloss_audio_dir(&ctx.work_abbrev, gloss_id));

    let mut s = state_rc.borrow_mut();
    if let Some(g) = s.gloss_list.get_mut(gloss_index) {
        g.gloss_text = original.clone();
    }
    s.gloss_undo = None;
    s.gloss_active_voice = 0;
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &original, cw, h,
        Some(&s.theme.root_color), &pairs, (&head.0, &head.1),
    );
    s.gloss_overlay.set_position(gloss_index, s.gloss_list.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_index = gloss_index;
    recolor_cached_blocks(&s);
    crate::app::apply_reader_gloss_highlighting(&mut s);
    crate::logging::log(&format!("GLOSS: undid edit of gloss {}", gloss_id));
}

/// Persist a freshly composed async-Claude gloss, reload the start-citation
/// gloss list, select the new row, render it into the gloss overlay, reinstall
/// `gloss_context`, and call `record_last_gloss`. Shared by the four async
/// Claude-call render tails (action_reader_gloss, action_gloss_with_claude,
/// run_pending_inner_monologue_blocking, ask_claude/gloss-from-journal).
///
/// `text` is the text to persist and render (callers that pre-process the raw
/// Claude response, e.g. `verify_echo_citations`, pass the processed form here).
pub(crate) fn persist_render_install_gloss(
    s: &mut AppState,
    ctx: crate::gloss::GlossContext,
    text: &str,
    gloss_type: &str,
    model_for_db: &str,
    log_msg: &str,
) {
    let mut new_gloss_id: i64 = -1;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Ok(id) = crate::db::queries::save_gloss(
            &conn,
            &ctx.hash,
            &ctx.work_abbrev,
            &ctx.start_citation,
            &ctx.end_citation,
            ctx.act,
            ctx.scene,
            &ctx.speaker,
            &ctx.source_text,
            text,
            gloss_type,
            model_for_db,
        ) {
            new_gloss_id = id;
        }
    }

    // The reload list must always include the type just persisted, or the
    // `position()` lookup below finds nothing and falls through to
    // `unwrap_or(0)`, leaving gloss_list/gloss_index/the overlay's position
    // counter describing a different set than the gloss text on screen.
    let mut reload_types = vec!["teacher-generic", "inner-monologue", "reader-gloss"];
    if !reload_types.contains(&gloss_type) {
        reload_types.push(gloss_type);
    }
    let all = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &reload_types,
            ).ok()
        })
        .unwrap_or_default();

    let new_idx = all.iter().position(|g| g.gloss_id == new_gloss_id).unwrap_or(0);

    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(&ctx.source_text, text, cw, h, Some(&s.theme.root_color), &pairs, (&head.0, &head.1));
    // This install path re-populates the buffer but does NOT run
    // `recolor_cached_blocks` (unlike the display sites), so tint vocab here.
    if s.vocab_highlight_visible {
        s.gloss_overlay.apply_vocab_tags(&s.vocab_words);
    }
    s.gloss_overlay.set_position(new_idx, all.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_list = all;
    s.gloss_index = new_idx;
    s.gloss_context = Some(ctx);
    s.record_last_gloss(gloss_type);
    crate::logging::log(log_msg);
}

pub(crate) fn add_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
    let (ctx, model) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        (ctx, state.config.claude_model.clone())
    };

    // Show the passage being reglossed on the loading card (same single-column
    // `<speaker>`/`<segment>` formatting as the gloss result), not a bare
    // "Glossing…" label.
    {
        let s = state_rc.borrow();
        let (cw, h) = crate::app::layout::overlay_card_size(&s);
        s.gloss_overlay.show_glossing(
            &ctx.passage_doc(), cw, h, Some(&s.theme.root_color), Some(&ctx.gloss_type),
        );
    }

    let prompt_owned = prompt.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
            (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let neighbors = crate::gloss::neighbors_for_ctx(&ctx);
            crate::logging::log(&format!(
                "GLOSS NEIGHBORS: {} neighbor(s) for {}-{}",
                neighbors.len(), ctx.start_citation, ctx.end_citation
            ));
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None, &neighbors);
            (crate::gloss::reader_gloss_question_prompt(&ctx.work_type), msg, "reader-gloss")
        }
        _ => {
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None, &[]);
            (crate::gloss::USER_QUESTION_PROMPT.as_str(), msg, "teacher-generic")
        }
    };

    let gloss_type_owned = gloss_type_str.to_string();

    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt.to_string(),
        user_msg,
        model,
        move |st, gloss_text| {
            let verified_text = if is_inner_monologue {
                crate::gloss::verify_echo_citations(
                    &gloss_text, &ctx.work_abbrev, ctx.act, ctx.scene,
                )
            } else {
                gloss_text.clone()
            };
            let full_gloss = if is_inner_monologue {
                format!("<gloss>Inner voice from:</gloss>\n\n{}\n\n{}", prompt_owned, verified_text)
            } else {
                format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, verified_text)
            };
            persist_and_render_gloss(
                st, &ctx, &full_gloss, &gloss_type_owned, &model_for_db,
                &format!("GLOSS: added new {} gloss", gloss_type_owned),
            );
        },
        |st, msg| {
            st.borrow().gloss_overlay.show(msg, "");
        },
    );
}

pub(crate) fn edit_gloss(state_rc: &Rc<RefCell<AppState>>, pasted_lines: &str) {
    let (ctx, existing_gloss_text, model, gloss_index, gloss_id) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let idx = state.gloss_index;
        let (existing, gloss_id) = match state.gloss_list.get(idx) {
            Some(g) => (g.gloss_text.clone(), g.gloss_id),
            // No current gloss to edit in place — nothing to do.
            None => return,
        };
        (ctx, existing, state.config.claude_model.clone(), idx, gloss_id)
    };
    // Capture the WHOLE OLD gloss's RENDERED text before it is overwritten, so the
    // post-rewrite diff highlight can compare old-rendered vs new-rendered across
    // the FULL gloss (not just the visible page-1 buffer — long glosses paginate).
    // Same full-render basis the new side uses in update_and_render_gloss_in_place.
    let prev_rendered =
        crate::ui::gloss_overlay::GlossOverlay::full_rendered_gloss_text(&existing_gloss_text);

    // Show the passage being reglossed on the loading card (same single-column
    // `<speaker>`/`<segment>` formatting as the gloss result), not a bare
    // "Glossing…" label.
    {
        let s = state_rc.borrow();
        let (cw, h) = crate::app::layout::overlay_card_size(&s);
        s.gloss_overlay.show_glossing(
            &ctx.passage_doc(), cw, h, Some(&s.theme.root_color), Some(&ctx.gloss_type),
        );
    }

    let pasted_owned = pasted_lines.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::INNER_MONOLOGUE_EDIT_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let mut msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            let neighbors = crate::gloss::neighbors_for_ctx(&ctx);
            crate::logging::log(&format!(
                "GLOSS NEIGHBORS: {} neighbor(s) for {}-{}",
                neighbors.len(), ctx.start_citation, ctx.end_citation
            ));
            if let Some(block) = crate::gloss::neighbor_block(&neighbors) {
                msg.push_str("\n\n");
                msg.push_str(&block);
            }
            (crate::gloss::reader_gloss_edit_prompt(&ctx.work_type), msg, "reader-gloss")
        }
        _ => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::EDIT_GLOSS_PROMPT.as_str(), msg, "teacher-generic")
        }
    };

    let gloss_type_owned = gloss_type_str.to_string();

    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt.to_string(),
        user_msg,
        model,
        move |st, gloss_text| {
            let verified_text = if is_inner_monologue {
                crate::gloss::verify_echo_citations(
                    &gloss_text, &ctx.work_abbrev, ctx.act, ctx.scene,
                )
            } else {
                gloss_text.clone()
            };
            // Persist ONLY the model's rewritten gloss — no provenance header.
            // The user's rewrite prompt (`pasted_owned`) was already delivered to
            // Claude via `build_edit_gloss_message` (as USER-PROVIDED LINES); it
            // is transient editing metadata, not gloss content, so it must not be
            // saved into the gloss body in lit.db. (Earlier code prepended a
            // `<gloss>Edit context: …</gloss>` header, which both polluted the
            // stored gloss and — when stored untagged — vanished from the
            // overlay; gloss 21784.)
            update_and_render_gloss_in_place(
                st, &ctx, gloss_index, gloss_id, &verified_text, &model_for_db,
                &format!("GLOSS: edited {} gloss {} in place", gloss_type_owned, gloss_id),
                Some((&prev_rendered, Some(&pasted_owned))),
            );
        },
        |st, msg| {
            st.borrow().gloss_overlay.show(msg, "");
        },
    );
}

/// Stop all gloss audio so only one source can ever play: pause MPV media and
/// stop the rodio TTS player. Called on cursor moves in the GLOSS/SYNOPSIS
/// overlays, whose source blocks play the work's own media — there, moving off
/// a playing block should silence it before the user starts the next one. All
/// calls are harmless no-ops when nothing is playing.
///
/// The JOURNAL overlay's nav binds deliberately use `stop_overlay_tts_only`
/// instead: its blocks are a quoted passage plus a Q&A, not media anchors, so
/// paging through it must not pause the reader's audiobook.
pub(crate) fn stop_all_gloss_audio(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.tts.stop();
    // Space source loop counts as gloss audio too — nav-stop it so a block
    // TTS never plays over the looping source passage.
    crate::input::actions::chat::chat_loop_stop(&mut s);
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
}

/// Stop only the OVERLAY's own audio — the rodio TTS player and the Space
/// source loop — leaving the main card's MPV playback alone.
///
/// For overlay NAVIGATION binds. Moving the block cursor is not a playback
/// command: an overlay is something the reader opens while the audiobook plays,
/// and paging through it silently paused their book (`stop_all_gloss_audio`
/// sends `MpvCommand::Pause`). Only the binds that themselves produce audio —
/// `a` and Space, which synthesize TTS — should touch playback, and they manage
/// it through their own paths.
pub(crate) fn stop_overlay_tts_only(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.tts.stop();
    crate::input::actions::chat::chat_loop_stop(&mut s);
}

/// Space in the gloss overlay: toggle play/pause for the cursor's block.
/// - If TTS is playing -> stop it.
/// - Source block + media playing -> pause; else seek to the block start + play.
/// - Explication block (or source with no media) -> play its TTS (cached).
pub(crate) fn read_current_block(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        if s.tts.is_playing() {
            s.tts.stop();
            return;
        }
    }

    let (kind, index) = match resolve_cursor_block(state_rc) {
        Some(t) => t,
        None => return,
    };

    // Source block: Space toggles media play/pause.
    if kind == BlockKind::Source {
        let (connected, playing, seek) = source_media_state(state_rc, index);
        if connected && playing {
            // Currently playing -> pause. (Next Space restarts from block start.)
            let _ = state_rc.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
            crate::log_fmt!("GLOSS: source block {} -> pause", index);
            return;
        }
        if let Some(start) = seek {
            let _ = state_rc
                .borrow()
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::ResumeAndSeek(start));
            crate::log_fmt!("GLOSS: source block {} -> media seek {}", index, start);
            return;
        }
        // No media available -> fall through to TTS.
    }

    play_block_tts(state_rc, kind, index);
}

/// `a` in the gloss overlay: ALWAYS begin playback of the cursor's block from
/// its start (no pause-toggle). Source block + media -> seek to start + play;
/// otherwise -> play the block's TTS (cached).
pub(crate) fn begin_current_block(state_rc: &Rc<RefCell<AppState>>) {
    // Stop any current audio first so we never overlap.
    stop_all_gloss_audio(state_rc);

    let (kind, index) = match resolve_cursor_block(state_rc) {
        Some(t) => t,
        None => return,
    };

    if kind == BlockKind::Source {
        let (connected, _playing, seek) = source_media_state(state_rc, index);
        if connected {
            if let Some(start) = seek {
                let _ = state_rc
                    .borrow()
                    .cmd_tx
                    .try_send(crate::mpv::MpvCommand::ResumeAndSeek(start));
                crate::log_fmt!("GLOSS: source block {} -> begin media seek {}", index, start);
                return;
            }
        }
        // No media available -> fall through to TTS.
    }

    play_block_tts(state_rc, kind, index);
}

/// Resolve the cursor's current block as `(kind, index)`, toasting "Nothing to
/// read" and returning None when the card has no blocks. The borrow is dropped
/// before the toast call.
fn resolve_cursor_block(state_rc: &Rc<RefCell<AppState>>) -> Option<(BlockKind, i32)> {
    let block_opt = state_rc.borrow().gloss_overlay.current_block();
    match block_opt {
        Some(t) => Some(t),
        None => {
            show_tts_toast(state_rc, "Nothing to read");
            None
        }
    }
}

/// `(mpv_connected, mpv_playing, Some(start))` for a source block's media, where
/// `start` is the first timestamped line's start time (None if no timing / not
/// connected).
fn source_media_state(state_rc: &Rc<RefCell<AppState>>, index: i32) -> (bool, bool, Option<f64>) {
    let s = state_rc.borrow();
    let seek = if s.mpv_connected {
        source_block_seek_time(&s, index)
    } else {
        None
    };
    (s.mpv_connected, s.mpv_playing, seek)
}

/// Cycle which associated voice is active for the current gloss (wraps). Toasts
/// the now-active voice id; no-op toast if the gloss has no associated voices.
/// Does NOT auto-play — the next Space uses the new active voice.
pub(crate) fn cycle_active_voice(state_rc: &Rc<RefCell<AppState>>) {
    let gloss_id = {
        let s = state_rc.borrow();
        match s.gloss_list.get(s.gloss_index) {
            Some(g) => g.gloss_id,
            None => return,
        }
    };
    let voices = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::queries::get_gloss_voices(&conn, gloss_id),
        Err(_) => Vec::new(),
    };
    if voices.is_empty() {
        show_tts_toast(state_rc, "No voices associated — default in use");
        return;
    }
    let next = {
        let mut s = state_rc.borrow_mut();
        s.gloss_active_voice = (s.gloss_active_voice + 1) % voices.len();
        s.gloss_active_voice
    };
    show_tts_toast(state_rc, &format!("Voice: {}", voices[next].0));
}

/// Play a block's TTS audio: cache hit -> play the stored MP3; miss ->
/// synthesize via ElevenLabs (async), cache it, and play. `kind`/`index`
/// identify the block; the filename stem is `<index>` (explication) or
/// `source-<index>` (source).
/// Resolve the (voice_id, model_id) a gloss block plays in: the active per-gloss
/// override voice if the gloss has associated voices (clamped to
/// `active_voice`), else the fixed overlay narrator
/// (`OVERLAY_NARRATOR_VOICE_ID` — one voice for all gloss/synopsis TTS).
/// Shared by `play_block_tts` and the cached-audio recolor check so both look at
/// the same mp3. Mirrors the inline logic at the former call site.
pub(crate) fn gloss_block_voice(
    conn: &rusqlite::Connection,
    gloss_id: i64,
    _work_abbrev: &str,
    _speaker: &str,
    _kind: BlockKind,
    active_voice: usize,
) -> (String, String) {
    let voices = crate::db::queries::get_gloss_voices(conn, gloss_id);
    if !voices.is_empty() {
        let i = active_voice.min(voices.len() - 1);
        (voices[i].0.clone(), voices[i].1.clone())
    } else {
        (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID.to_string(),
            crate::elevenlabs::OP_MODEL_ID.to_string(),
        )
    }
}

/// Accent color for cached (already-synthesized) gloss/synopsis blocks:
/// Sienna family, picked per card polarity (`Theme.is_light`) — deliberately
/// NOT the theme's `cursor_bg` (a love-red that clashed against the cream
/// paper and read like an error). The old single `#2d5570` slate technically
/// cleared the contrast floors on light cards but sat at only 1.56:1 vs body
/// ink — synthesized blocks looked uncolored — and failed the 4.5:1 bg floor
/// outright on dark cards. Both variants clear every floor for their polarity
/// (colorscheme harness, 2026-07-22). Promote to a `Theme` field if/when
/// palettes need their own cached accent.
const CACHED_BLOCK_ACCENT_LIGHT_CARD: &str = "#a0522d";
const CACHED_BLOCK_ACCENT_DARK_CARD: &str = "#cc8850";

/// The cached-audio accent for the current theme's card polarity.
fn cached_block_accent(s: &AppState) -> &'static str {
    if s.theme.is_light {
        CACHED_BLOCK_ACCENT_LIGHT_CARD
    } else {
        CACHED_BLOCK_ACCENT_DARK_CARD
    }
}

/// Re-apply accent coloring to every block of the currently-open gloss OR
/// synopsis overlay whose mp3 is cached. Mode is taken from `s.input_mode`
/// (the authoritative overlay discriminator) — NOT from `gloss_context`, which
/// is never cleared and so lingers `Some` after a gloss is closed; keying mode
/// off it would mis-route synopsis coloring into the gloss branch. UI-only side
/// effect; DB errors degrade to "uncached" (no color). Call with `s` already
/// borrowed (the display sites) — see `recolor_cached_blocks_rc` for the
/// borrow-and-call wrapper used by async synth completions.
pub(crate) fn recolor_cached_blocks(s: &AppState) {
    // Gloss mode: only when the gloss overlay is the active one.
    if s.input_mode == crate::app::InputMode::GlossOverlay {
        let (Some(ctx), Some(gloss)) =
            (s.gloss_context.as_ref(), s.gloss_list.get(s.gloss_index))
        else {
            return;
        };
        let gloss_id = gloss.gloss_id;
        let work_abbrev = ctx.work_abbrev.clone();
        let speaker = ctx.speaker.clone();
        let active = s.gloss_active_voice;
        let accent = cached_block_accent(s).to_string();
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => return,
        };
        crate::log_fmt!(
            "RECOLOR: gloss {} active_voice={} blocks recoloring",
            gloss_id, active
        );
        s.gloss_overlay.color_audio_blocks(&accent, move |kind, index| {
            let kind_str = match kind {
                BlockKind::Source => "source",
                BlockKind::Explication => "explication",
            };
            let (vid, _mid) =
                gloss_block_voice(&conn, gloss_id, &work_abbrev, &speaker, *kind, active);
            for vid_try in [vid.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
                if let Ok(Some(path)) = crate::db::queries::find_gloss_audio(
                    &conn, gloss_id, kind_str, index as i64, vid_try,
                ) {
                    if std::path::Path::new(&path).exists() {
                        crate::log_fmt!(
                            "RECOLOR: {}#{} CACHED (voice {}) -> color",
                            kind_str, index, vid_try
                        );
                        return true;
                    }
                }
            }
            crate::log_fmt!("RECOLOR: {}#{} not cached (voice {})", kind_str, index, vid);
            false
        });
        if s.vocab_highlight_visible {
            s.gloss_overlay.apply_vocab_tags(&s.vocab_words);
        }
        return;
    }

    // Synopsis mode. Key by the BASE abbrev (matching `play_synopsis_block` /
    // `synth_all_synopsis_blocks`) so synopsis audio is shared across editions
    // (`2H6`/`2H6-Amb`) the same way the synopsis TEXT is — `synopsis_cache` is
    // itself loaded under the base abbrev, so the audio key must match it.
    let (div1, div2) = s.synopsis_overlay_division;
    let work_abbrev = match s.current_work.as_ref() {
        Some(w) => w.canonical_abbrev.clone(),
        None => return,
    };
    let voice_id = crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID.to_string();
    let accent = cached_block_accent(s).to_string();
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    s.gloss_overlay.color_audio_blocks(&accent, move |_kind, index| {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) = crate::db::queries::find_synopsis_audio(
                &conn, &work_abbrev, div1, div2, index as i64, vid_try,
            ) {
                if std::path::Path::new(&path).exists() {
                    return true;
                }
            }
        }
        false
    });
    if s.vocab_highlight_visible {
        s.gloss_overlay.apply_vocab_tags(&s.vocab_words);
    }
}

/// Borrow `state` and recolor. For async synth-completion sites that hold an
/// `Rc<RefCell<AppState>>` and must not already hold a borrow.
pub(crate) fn recolor_cached_blocks_rc(state: &Rc<RefCell<AppState>>) {
    recolor_cached_blocks(&state.borrow());
}

/// Color the journal Q&A overlay's current-page paragraphs whose TTS MP3 is
/// cached, with the same accent the gloss/synopsis overlays use. Keyed by the
/// page's `journal_entries.id` + the paragraph's FULL index + the fixed
/// overlay narrator (the journal always synthesizes in that voice). Called on
/// every page render and after a synth completes. No-op when not in the journal
/// overlay or no current page.
pub(crate) fn recolor_journal_cached_blocks(s: &AppState) {
    if s.input_mode != crate::app::InputMode::JournalOverlay {
        return;
    }
    // Vocab tint mirrors the gloss overlay: this is the shared post-render hook
    // (render_current + every j/k/x/y/g/G nav path routes through here), so tint
    // the freshly rendered buffer whenever the vocab surface is visible.
    if s.vocab_highlight_visible {
        s.journal_overlay.apply_vocab_tags(&s.vocab_words);
    }
    let entry_id = match s.journal.pages.get(s.journal.page_index) {
        Some(p) => p.id,
        None => return,
    };
    // MUST match the voice the journal synth paths write under, or a cached
    // paragraph reads back as uncached and loses its accent.
    let voice_id = crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID.to_string();
    let accent = cached_block_accent(s).to_string();
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    s.journal_overlay.color_cached_blocks(&accent, move |full_index| {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) =
                crate::db::queries::find_journal_audio(&conn, entry_id, full_index as i64, vid_try)
            {
                if std::path::Path::new(&path).exists() {
                    return true;
                }
            }
        }
        false
    });
}

/// Borrow-then-recolor wrapper for the journal, for async synth-completion sites.
pub(crate) fn recolor_journal_cached_blocks_rc(state: &Rc<RefCell<AppState>>) {
    recolor_journal_cached_blocks(&state.borrow());
}

fn play_block_tts(state_rc: &Rc<RefCell<AppState>>, kind: BlockKind, index: i32) {
    let kind_str = match kind {
        BlockKind::Source => "source",
        BlockKind::Explication => "explication",
    };
    let (gloss_id, work_abbrev, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let gloss = match s.gloss_list.get(s.gloss_index) {
            Some(g) => g,
            None => return,
        };
        let gloss_id = gloss.gloss_id;
        let (work_abbrev, speaker) = match &s.gloss_context {
            Some(ctx) => (ctx.work_abbrev.clone(), ctx.speaker.clone()),
            None => return,
        };
        let blocks = crate::ui::gloss_block::gloss_blocks(&gloss.gloss_text);
        let text = match blocks.iter().find(|b| b.kind == kind && b.index == index) {
            Some(b) => b.text.clone(),
            None => return,
        };
        let is_verse = kind == BlockKind::Source;
        // Per-gloss voice override: if the gloss has associated voices, play the
        // active one (gloss_active_voice index, clamped). Else the fixed overlay
        // narrator.
        let (vid, mid): (String, String) = match crate::db::queries::open_db() {
            Ok(conn) => gloss_block_voice(
                &conn, gloss_id, &work_abbrev, &speaker, kind, s.gloss_active_voice,
            ),
            Err(_) => (
                crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID.to_string(),
                crate::elevenlabs::OP_MODEL_ID.to_string(),
            ),
        };
        crate::log_fmt!(
            "TTS: voice -> {} (gloss {}, {})",
            vid, gloss_id, if is_verse { "verse" } else { "prose" }
        );
        (
            gloss_id,
            work_abbrev,
            text,
            vid,
            mid,
            s.tokio_handle.clone(),
        )
    };

    // TTS form: the prompt appends `/IPA/` after the word it annotates
    // (`take /tɛːk/`); ElevenLabs v3 would otherwise voice BOTH the word and the
    // IPA (the doubling). Replace each `word /IPA/` pair with just `/IPA/` so the
    // word is spoken once, in OP. Display/storage keep the word (see strip_ipa).
    let text = crate::ui::gloss_ipa::ipa_for_tts(&text);

    // Filename stem includes a short voice tag so each voice's audio for a block
    // is a distinct file (voice ids are alphanumeric, filesystem-safe).
    let voice_tag: String = voice_id.chars().take(12).collect();
    let stem = match kind {
        BlockKind::Source => format!("source-{}-{}", index, voice_tag),
        BlockKind::Explication => format!("{}-{}", index, voice_tag),
    };

    // Cache hit? Try the selected voice first; then the Alice fallback voice
    // (a block whose preferred voice 402'd was cached under Alice — without this
    // second lookup it would re-synthesize and re-hit the paywall every play).
    if let Ok(conn) = crate::db::queries::open_db() {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) =
                crate::db::queries::find_gloss_audio(&conn, gloss_id, kind_str, index as i64, vid_try)
            {
                if std::path::Path::new(&path).exists() {
                    state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                    return;
                }
            }
        }
    }

    // Miss: synthesize asynchronously. Keep the pill up until playback begins
    // (synthesis can exceed the 3s auto-dismiss); it is hidden just before
    // play_file below, or replaced by an error toast on a failure path.
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        // Try the preferred voice; on `paid_plan_required` fall back to Alice
        // (a free premade voice), keeping the user's preference unchanged.
        let result = synth_via(&tokio_handle, &text, &voice_id, &model_id).await;
        let (bytes, used_voice, used_model) = match result {
            Ok(bytes) => (bytes, voice_id.clone(), model_id.clone()),
            Err(crate::elevenlabs::ElevenLabsError::PaidPlanRequired)
                if voice_id != crate::elevenlabs::ALICE_VOICE_ID =>
            {
                crate::log_fmt!(
                    "TTS: voice {} needs a paid plan — falling back to Alice",
                    voice_id
                );
                show_tts_toast(&state_for_result, "Voice needs a paid plan — using Alice");
                let alice_voice = crate::elevenlabs::ALICE_VOICE_ID.to_string();
                let alice_model = crate::elevenlabs::ALICE_MODEL_ID.to_string();
                match synth_via(&tokio_handle, &text, &alice_voice, &alice_model).await {
                    Ok(bytes) => (bytes, alice_voice, alice_model),
                    Err(e) => {
                        crate::log_fmt!("TTS: Alice fallback failed: {}", e);
                        show_tts_toast(&state_for_result, &e.to_string());
                        return;
                    }
                }
            }
            Err(e) => {
                crate::log_fmt!("TTS: synth error: {}", e);
                show_tts_toast(&state_for_result, &e.to_string());
                return;
            }
        };

        // Persist the bytes and play, caching under the voice that actually
        // produced them (Alice on a fallback, not the rejected preferred voice).
        let dir = gloss_audio_dir(&work_abbrev, gloss_id);
        let path = dir.join(format!("{}.mp3", stem));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            crate::log_fmt!("TTS: mkdir {} failed: {}", dir.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            crate::log_fmt!("TTS: write {} failed: {}", path.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            if let Err(e) = crate::db::queries::save_gloss_audio(
                &conn,
                gloss_id,
                kind_str,
                index as i64,
                &path.to_string_lossy(),
                &used_voice,
                &used_model,
            ) {
                crate::log_fmt!("TTS: save_gloss_audio failed: {}", e);
            }
        }
        // Block is now cached — recolor the open overlay so it shows the accent.
        recolor_cached_blocks_rc(&state_for_result);
        // Playback begins now — dismiss the persistent "Synthesizing…" pill.
        hide_tts_toast(&state_for_result);
        // Cursor may have moved during the await — see the journal path. Compare
        // the KIND too: source and explication blocks number independently, so
        // index alone would match the wrong block. The clip is cached either
        // way, so the next press is a cache hit.
        let still_current = should_play_after_synth(
            (kind, index),
            state_for_result.borrow().gloss_overlay.current_block(),
        );
        if !still_current {
            crate::log_fmt!(
                "TTS: synthesized gloss {} {} {} — cursor moved, not playing",
                gloss_id, kind_str, index
            );
            return;
        }
        state_for_result.borrow().tts.play_file(&path);
        crate::log_fmt!(
            "TTS: synthesized gloss {} {} {} (voice {})",
            gloss_id,
            kind_str,
            index,
            used_voice
        );
    });
}

/// Shift+Space (gloss overlay): synthesize ALL prose (Explication) blocks of the
/// open gloss to cached MP3s in the fixed plain-prose voice. Cache-only (no
/// playback). Shows a persistent "Synthesizing…" toast; stops on the first
/// error and shows it. Skips blocks already cached. Re-entrant-safe via
/// AppState.tts_batch_running.
pub(crate) fn synth_all_prose_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (gloss_id, work_abbrev, blocks, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let gloss = match s.gloss_list.get(s.gloss_index) {
            Some(g) => g,
            None => return,
        };
        let gloss_id = gloss.gloss_id;
        let work_abbrev = match &s.gloss_context {
            Some(ctx) => ctx.work_abbrev.clone(),
            None => return,
        };
        let prose: Vec<(i32, String)> =
            crate::ui::gloss_block::gloss_blocks(&gloss.gloss_text)
                .into_iter()
                .filter(|b| b.kind == BlockKind::Explication)
                .map(|b| (b.index, b.text))
                .collect();
        if prose.is_empty() {
            return;
        }
        // Explication prose is read by the fixed overlay narrator. Single-block
        // synth resolves the same voice via gloss_block_voice, and the
        // cached-audio recolor check looks under that narrator — so the batch
        // MUST cache under it too, or its mp3s land under a different voice id
        // and neither playback-cache-hit nor the recolor existence check will
        // find them.
        let (vid, mid) = (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID,
            crate::elevenlabs::OP_MODEL_ID,
        );
        (gloss_id, work_abbrev, prose, vid.to_string(), mid.to_string(), s.tokio_handle.clone())
    };

    state_rc.borrow().tts_batch_running.set(true);
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        for (index, raw) in &blocks {
            // Skip if already cached for this voice.
            if let Ok(conn) = crate::db::queries::open_db() {
                if let Ok(Some(path)) = crate::db::queries::find_gloss_audio(
                    &conn, gloss_id, "explication", *index as i64, &voice_id,
                ) {
                    if std::path::Path::new(&path).exists() {
                        continue;
                    }
                }
            }
            let tts_text = crate::ui::gloss_ipa::ipa_for_tts(raw);
            let bytes = match synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await {
                Ok(b) => b,
                Err(e) => {
                    crate::log_fmt!("BATCH: gloss synth error at block {}: {}", index, e);
                    show_tts_toast(&state_for_result, &format!("Synthesis failed: {}", e));
                    state_for_result.borrow().tts_batch_running.set(false);
                    return;
                }
            };
            let dir = gloss_audio_dir(&work_abbrev, gloss_id);
            let voice_tag: String = voice_id.chars().take(12).collect();
            let path = dir.join(format!("{}-{}.mp3", index, voice_tag));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                crate::log_fmt!("BATCH: mkdir {} failed: {}", dir.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                crate::log_fmt!("BATCH: write {} failed: {}", path.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::queries::save_gloss_audio(
                    &conn, gloss_id, "explication", *index as i64,
                    &path.to_string_lossy(), &voice_id, &model_id,
                );
            }
            // This block is now cached — color it in the open overlay now.
            recolor_cached_blocks_rc(&state_for_result);
        }
        hide_tts_toast(&state_for_result);
        state_for_result.borrow().tts_batch_running.set(false);
        crate::log_fmt!("BATCH: synthesized {} gloss prose blocks", blocks.len());
    });
}

/// Shift+Space (synopsis overlay): synthesize the tiered gist/précis/account
/// paragraphs of the open scene to cached MP3s in the overlay narrator voice —
/// the metadata front matter (Location:, Characters:, ...) is skipped
/// (`synopsis_tier_blocks`); an untiered synopsis synthesizes all paragraphs.
/// Cache-only. Persistent toast; stop on first error. Re-entrant-safe via
/// tts_batch_running.
pub(crate) fn synth_all_synopsis_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (work_abbrev, div1, div2, blocks, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let (div1, div2) = s.synopsis_overlay_division;
        let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        let prose: Vec<(i32, String)> = crate::ui::gloss_block::synopsis_tier_blocks(&synopsis)
            .into_iter()
            .map(|b| (b.index, b.text))
            .collect();
        if prose.is_empty() {
            return;
        }
        let (vid, mid) = (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID,
            crate::elevenlabs::OP_MODEL_ID,
        );
        (work_abbrev, div1, div2, prose, vid.to_string(), mid.to_string(), s.tokio_handle.clone())
    };

    state_rc.borrow().tts_batch_running.set(true);
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        for (index, raw) in &blocks {
            if let Ok(conn) = crate::db::queries::open_db() {
                if let Ok(Some(path)) = crate::db::queries::find_synopsis_audio(
                    &conn, &work_abbrev, div1, div2, *index as i64, &voice_id,
                ) {
                    if std::path::Path::new(&path).exists() {
                        continue;
                    }
                }
            }
            let tts_text = crate::ui::gloss_ipa::ipa_for_tts(raw);
            let bytes = match synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await {
                Ok(b) => b,
                Err(e) => {
                    crate::log_fmt!("BATCH: synopsis synth error at para {}: {}", index, e);
                    show_tts_toast(&state_for_result, &format!("Synthesis failed: {}", e));
                    state_for_result.borrow().tts_batch_running.set(false);
                    return;
                }
            };
            let dir = synopsis_audio_dir(&work_abbrev, div1, div2);
            let voice_tag: String = voice_id.chars().take(12).collect();
            let path = dir.join(format!("{}-{}.mp3", index, voice_tag));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                crate::log_fmt!("BATCH: mkdir {} failed: {}", dir.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                crate::log_fmt!("BATCH: write {} failed: {}", path.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::migrations::ensure_synopsis_audio_table(&conn);
                let _ = crate::db::queries::save_synopsis_audio(
                    &conn, &work_abbrev, div1, div2, *index as i64,
                    &path.to_string_lossy(), &voice_id, &model_id,
                );
            }
            // This paragraph is now cached — color it in the open overlay now.
            recolor_cached_blocks_rc(&state_for_result);
        }
        hide_tts_toast(&state_for_result);
        state_for_result.borrow().tts_batch_running.set(false);
        crate::log_fmt!("BATCH: synthesized {} synopsis paragraphs", blocks.len());
    });
}

/// Play the synopsis cursor paragraph's TTS: cache hit -> play the stored MP3;
/// miss -> synthesize via ElevenLabs (async), cache under `synopsis_audio`, and
/// play. Mirrors `play_block_tts` but for synopsis paragraphs (which have no
/// source media). Shares the EXACT cache path/key with
/// `synth_all_synopsis_blocks` so a Space-synth and a Shift+Space-batch reuse the
/// same MP3 files and DB rows. Does NOT touch `tts_batch_running` (batch-only).
fn play_synopsis_block(state_rc: &Rc<RefCell<AppState>>, index: i32) {
    let (work_abbrev, div1, div2, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let (div1, div2) = s.synopsis_overlay_division;
        let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        let text = match crate::ui::gloss_block::synopsis_blocks(&synopsis)
            .into_iter()
            .find(|b| b.index == index)
        {
            Some(b) => b.text,
            None => return,
        };
        let (vid, mid) = (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID,
            crate::elevenlabs::OP_MODEL_ID,
        );
        (
            work_abbrev,
            div1,
            div2,
            text,
            vid.to_string(),
            mid.to_string(),
            s.tokio_handle.clone(),
        )
    };

    // Cache hit? Try the selected voice first; then the Alice fallback voice
    // (a paragraph whose preferred voice 402'd was cached under Alice — without
    // this second lookup it would re-synthesize and re-hit the paywall).
    if let Ok(conn) = crate::db::queries::open_db() {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) = crate::db::queries::find_synopsis_audio(
                &conn,
                &work_abbrev,
                div1,
                div2,
                index as i64,
                vid_try,
            ) {
                if std::path::Path::new(&path).exists() {
                    state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                    return;
                }
            }
        }
    }

    // TTS form: rewrite `word /IPA/` pairs to just `/IPA/` (no-op on synopsis
    // text, which carries no IPA, but applied for consistency with other paths).
    let tts_text = crate::ui::gloss_ipa::ipa_for_tts(&text);

    // Miss: synthesize asynchronously. Keep the pill up until playback begins.
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        // Try the preferred voice; on `paid_plan_required` fall back to Alice.
        let result = synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await;
        let (bytes, used_voice, used_model) = match result {
            Ok(bytes) => (bytes, voice_id.clone(), model_id.clone()),
            Err(crate::elevenlabs::ElevenLabsError::PaidPlanRequired)
                if voice_id != crate::elevenlabs::ALICE_VOICE_ID =>
            {
                crate::log_fmt!(
                    "SYNOPSIS TTS: voice {} needs a paid plan — falling back to Alice",
                    voice_id
                );
                show_tts_toast(&state_for_result, "Voice needs a paid plan — using Alice");
                let alice_voice = crate::elevenlabs::ALICE_VOICE_ID.to_string();
                let alice_model = crate::elevenlabs::ALICE_MODEL_ID.to_string();
                match synth_via(&tokio_handle, &tts_text, &alice_voice, &alice_model).await {
                    Ok(bytes) => (bytes, alice_voice, alice_model),
                    Err(e) => {
                        crate::log_fmt!("SYNOPSIS TTS: Alice fallback failed: {}", e);
                        show_tts_toast(&state_for_result, &e.to_string());
                        return;
                    }
                }
            }
            Err(e) => {
                crate::log_fmt!("SYNOPSIS TTS: synth error: {}", e);
                show_tts_toast(&state_for_result, &e.to_string());
                return;
            }
        };

        // Persist the bytes and play, caching under the voice that actually
        // produced them (Alice on a fallback, not the rejected preferred voice).
        // The filename tag uses `used_voice` so the row's voice_id and the
        // filename stem agree — exactly as `play_block_tts` does.
        let used_tag: String = used_voice.chars().take(12).collect();
        let dir = synopsis_audio_dir(&work_abbrev, div1, div2);
        let path = dir.join(format!("{}-{}.mp3", index, used_tag));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            crate::log_fmt!("SYNOPSIS TTS: mkdir {} failed: {}", dir.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            crate::log_fmt!("SYNOPSIS TTS: write {} failed: {}", path.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::migrations::ensure_synopsis_audio_table(&conn);
            let _ = crate::db::queries::save_synopsis_audio(
                &conn,
                &work_abbrev,
                div1,
                div2,
                index as i64,
                &path.to_string_lossy(),
                &used_voice,
                &used_model,
            );
        }
        // Paragraph is now cached — recolor the open overlay so it shows the accent.
        recolor_cached_blocks_rc(&state_for_result);
        // Playback begins now — dismiss the persistent "Synthesizing…" pill.
        hide_tts_toast(&state_for_result);
        // Cursor may have moved during the await — see the journal path. Play
        // only if this is still the cursor's block; the clip is cached either
        // way, so the next press is a cache hit.
        let still_current = should_play_after_synth(
            index,
            state_for_result.borrow().gloss_overlay.current_block().map(|(_k, i)| i),
        );
        if !still_current {
            crate::log_fmt!(
                "SYNOPSIS TTS: synthesized {} {}-{} para {} — cursor moved, not playing",
                work_abbrev, div1, div2, index
            );
            return;
        }
        state_for_result.borrow().tts.play_file(&path);
        crate::log_fmt!(
            "SYNOPSIS TTS: synthesized {} {}-{} para {} (voice {})",
            work_abbrev,
            div1,
            div2,
            index,
            used_voice
        );
    });
}

/// Spacebar in the synopsis overlay: if TTS is playing, stop it; otherwise play
/// the cursor paragraph's TTS (cache hit plays the stored MP3, miss synthesizes
/// then plays). The synopsis overlay's blocks are all Explication paragraphs with
/// no source media, so there is no media-toggle branch — purely TTS play/stop.
pub(crate) fn read_current_synopsis_block(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        if s.tts.is_playing() {
            s.tts.stop();
            return;
        }
    }
    let index = match state_rc.borrow().gloss_overlay.current_block() {
        Some((_kind, index)) => index,
        None => return,
    };
    play_synopsis_block(state_rc, index);
}

/// `a` in the synopsis overlay: ALWAYS begin playback of the cursor's paragraph
/// from its start (no pause-toggle), mirroring the gloss-overlay `a`
/// (`begin_current_block`). Stops any current audio first, then plays the
/// paragraph's TTS — cache hit plays the stored MP3, miss synthesizes via
/// ElevenLabs then plays (both handled by `play_synopsis_block`).
pub(crate) fn begin_current_synopsis_block(state_rc: &Rc<RefCell<AppState>>) {
    // Stop any current audio first so we never overlap.
    stop_all_gloss_audio(state_rc);

    let index = match state_rc.borrow().gloss_overlay.current_block() {
        Some((_kind, index)) => index,
        None => return,
    };
    play_synopsis_block(state_rc, index);
}

// ── Journal Q&A overlay TTS ────────────────────────────────────────────────
//
// The journal Q&A overlay (src/ui/journal_overlay.rs) is plain prose, exactly
// like the synopsis overlay, so its TTS mirrors `play_synopsis_block`: the
// shared `OVERLAY_NARRATOR_VOICE_ID`, the Alice paywall fallback, and a
// per-paragraph MP3 cache.
// The only differences are the cache key (the `journal_entries.id` + paragraph
// index, not work/div) and the audio dir (`~/Music/journal/`). It lives in
// gloss.rs alongside the synopsis path so both reuse the private `synth_via` and
// toast helpers.

/// Synthesize (or play cached) the cursor paragraph of the open journal Q&A
/// page, keyed by the page's `journal_entries.id` so the cached MP3 follows the
/// entry. The borrow is dropped before any await; a miss synthesizes via
/// ElevenLabs and persists the bytes + a `journal_audio` row.
fn play_journal_block(state_rc: &Rc<RefCell<AppState>>, index: i32) {
    let (entry_id, work_abbrev, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        // Each bail below LOGS: a silent return here presents as "the key does
        // nothing", which cost a full diagnosis session when the Markdown-block
        // migration left every block's text empty and tripped the `text` guard.
        let entry_id = match s.journal.pages.get(s.journal.page_index) {
            Some(p) => p.id,
            None => {
                crate::log_fmt!(
                    "JOURNAL TTS: no entry at page_index={} (pages={})",
                    s.journal.page_index,
                    s.journal.pages.len()
                );
                return;
            }
        };
        let text = match s.journal_overlay.current_block_text() {
            Some(t) if !t.trim().is_empty() => t,
            other => {
                crate::log_fmt!(
                    "JOURNAL TTS: no block text at index={} (entry={}, {})",
                    index,
                    entry_id,
                    if other.is_none() { "no blocks" } else { "empty" }
                );
                return;
            }
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => {
                crate::log_fmt!("JOURNAL TTS: no current_work (entry={})", entry_id);
                return;
            }
        };
        // Journal Q&A reads in the shared overlay narrator — the same single
        // voice the gloss and synopsis overlays use (2026-07-30; was the
        // plain-prose default, Benedick).
        let (vid, mid) = (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID,
            crate::elevenlabs::OP_MODEL_ID,
        );
        (
            entry_id,
            work_abbrev,
            text,
            vid.to_string(),
            mid.to_string(),
            s.tokio_handle.clone(),
        )
    };

    // Cache hit? Try the selected voice first; then the Alice fallback voice
    // (a paragraph whose preferred voice 402'd was cached under Alice).
    if let Ok(conn) = crate::db::queries::open_db() {
        for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
            if let Ok(Some(path)) =
                crate::db::queries::find_journal_audio(&conn, entry_id, index as i64, vid_try)
            {
                if std::path::Path::new(&path).exists() {
                    crate::log_fmt!(
                        "JOURNAL TTS: cache hit entry {} para {} ({})",
                        entry_id,
                        index,
                        path
                    );
                    state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                    return;
                }
            }
        }
    }

    // TTS form: rewrite `word /IPA/` pairs to just `/IPA/` (a no-op on journal
    // prose, which carries no IPA, but applied for consistency with other paths).
    let tts_text = crate::ui::gloss_ipa::ipa_for_tts(&text);

    // Miss: synthesize asynchronously. Keep the pill up until playback begins.
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        // Try the preferred voice; on `paid_plan_required` fall back to Alice.
        let result = synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await;
        let (bytes, used_voice, used_model) = match result {
            Ok(bytes) => (bytes, voice_id.clone(), model_id.clone()),
            Err(crate::elevenlabs::ElevenLabsError::PaidPlanRequired)
                if voice_id != crate::elevenlabs::ALICE_VOICE_ID =>
            {
                crate::log_fmt!(
                    "JOURNAL TTS: voice {} needs a paid plan — falling back to Alice",
                    voice_id
                );
                show_tts_toast(&state_for_result, "Voice needs a paid plan — using Alice");
                let alice_voice = crate::elevenlabs::ALICE_VOICE_ID.to_string();
                let alice_model = crate::elevenlabs::ALICE_MODEL_ID.to_string();
                match synth_via(&tokio_handle, &tts_text, &alice_voice, &alice_model).await {
                    Ok(bytes) => (bytes, alice_voice, alice_model),
                    Err(e) => {
                        crate::log_fmt!("JOURNAL TTS: Alice fallback failed: {}", e);
                        show_tts_toast(&state_for_result, &e.to_string());
                        return;
                    }
                }
            }
            Err(e) => {
                crate::log_fmt!("JOURNAL TTS: synth error: {}", e);
                show_tts_toast(&state_for_result, &e.to_string());
                return;
            }
        };

        // Persist the bytes and play, caching under the voice that actually
        // produced them (Alice on a fallback, not the rejected preferred voice).
        let used_tag: String = used_voice.chars().take(12).collect();
        let dir = journal_audio_dir(&work_abbrev, entry_id);
        let path = dir.join(format!("{}-{}.mp3", index, used_tag));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            crate::log_fmt!("JOURNAL TTS: mkdir {} failed: {}", dir.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            crate::log_fmt!("JOURNAL TTS: write {} failed: {}", path.display(), e);
            show_tts_toast(&state_for_result, "Could not save audio");
            return;
        }
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::migrations::ensure_journal_audio_table(&conn);
            let _ = crate::db::queries::save_journal_audio(
                &conn,
                entry_id,
                index as i64,
                &path.to_string_lossy(),
                &used_voice,
                &used_model,
            );
        }
        // The paragraph is now cached — recolor the open page so it shows the
        // cached-block accent (mirrors the gloss/synopsis synth path).
        recolor_journal_cached_blocks_rc(&state_for_result);
        // Playback begins now — dismiss the persistent "Synthesizing…" pill.
        hide_tts_toast(&state_for_result);
        // The cursor may have MOVED during the await (synthesis takes seconds,
        // and j/k/x/y stay live throughout). Play only if this block is still
        // the cursor's; otherwise the reader hears a paragraph they navigated
        // away from. The clip is already cached and persisted above, so the
        // work is not wasted — the next `a`/Space on this block is a cache hit.
        let still_current = should_play_after_synth(
            index as usize,
            state_for_result.borrow().journal_overlay.current_full_block_index(),
        );
        if !still_current {
            crate::log_fmt!(
                "JOURNAL TTS: synthesized entry {} para {} — cursor moved, not playing",
                entry_id, index
            );
            return;
        }
        state_for_result.borrow().tts.play_file(&path);
        crate::log_fmt!(
            "JOURNAL TTS: synthesized entry {} para {} (voice {})",
            entry_id, index, used_voice
        );
    });
}

/// `a` in the journal Q&A overlay: if TTS is playing, stop it; otherwise
/// play the cursor paragraph's TTS (cache hit plays the stored MP3, miss
/// synthesizes then plays). Mirrors `read_current_synopsis_block`. The cache
/// key is the FULL paragraph index (page-local would repeat across pages).
pub(crate) fn read_current_journal_block(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        // Stop-toggle, but only while actually SOUNDING. `is_playing` reports a
        // loaded clip and stays true while Space has it paused; without the
        // `is_paused` guard, `a` on a paused clip would silently discard it
        // instead of restarting the block — an apparent dead key.
        if s.tts.is_playing() && !s.tts.is_paused() {
            s.tts.stop();
            return;
        }
    }
    let index = match state_rc.borrow().journal_overlay.current_full_block_index() {
        Some(i) => i as i32,
        None => return,
    };
    // `a` is a PLAYBACK bind, so it may touch the main card's media: pause MPV
    // so the block's TTS is not spoken over the audiobook. (Space does the same
    // via `stop_all_gloss_audio`.) The journal's NAV binds deliberately do not —
    // see `stop_overlay_tts_only`.
    stop_all_gloss_audio(state_rc);
    play_journal_block(state_rc, index);
}

/// Should a just-synthesized clip actually play? Only when the block it was
/// synthesized for is STILL the cursor's block.
///
/// Synthesis is asynchronous and takes seconds, during which every navigation
/// bind stays live. Without this check the reader hears a paragraph they
/// already navigated away from. The clip is written to the cache and the DB
/// before this point either way, so suppressing playback wastes nothing — the
/// next press on that block is a cache hit.
///
/// Compares the WHOLE cursor identity, not just an index: the gloss overlay
/// numbers source and explication blocks independently, so equal indices of
/// different kinds are different blocks.
pub(crate) fn should_play_after_synth<T: PartialEq>(synthesized: T, cursor_now: Option<T>) -> bool {
    cursor_now == Some(synthesized)
}

/// What Space should do to the TTS player, given its current state. Pure so the
/// transport logic is testable: under `LIT_HEADLESS_TEST` the player is built
/// with no audio device, so `is_playing`/`is_paused` are always false and a
/// cage-driven Space can only ever exercise the `Start` arm.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum SpaceTransport {
    Resume,
    Pause,
    Start,
}

/// `is_playing` reports a LOADED clip and stays true while paused, so `paused`
/// must be tested FIRST or the toggle only ever pauses and never resumes.
pub(crate) fn space_transport(playing: bool, paused: bool) -> SpaceTransport {
    if paused {
        SpaceTransport::Resume
    } else if playing {
        SpaceTransport::Pause
    } else {
        SpaceTransport::Start
    }
}

/// Space in the journal Q&A overlay: TOGGLE play/pause on the rodio TTS player.
///
/// - A clip paused -> resume it in place.
/// - A clip playing -> pause it in place.
/// - Nothing loaded -> start the cursor paragraph's TTS from its beginning.
///
/// Deliberately NOT the same as `a` (`read_current_journal_block`), which always
/// (re)starts the cursor block from the top. Space is the transport control;
/// `a` is "read this block". They used to do the same thing, which left the
/// overlay with no way to pause a clip mid-sentence and pick it back up.
///
/// Diverges from `begin_current_synopsis_block`, which this once mirrored.
pub(crate) fn begin_current_journal_block(state_rc: &Rc<RefCell<AppState>>) {
    // PAUSE/RESUME the rodio player when a clip is loaded — Space is a transport
    // control, not a second "play from the start" (that is `a`). `is_playing`
    // reports a LOADED clip, so it stays true while paused; check `is_paused`
    // first or the toggle only ever pauses.
    {
        let s = state_rc.borrow();
        match space_transport(s.tts.is_playing(), s.tts.is_paused()) {
            SpaceTransport::Resume => {
                s.tts.resume();
                crate::log_fmt!("JOURNAL TTS: space -> resume");
                return;
            }
            SpaceTransport::Pause => {
                s.tts.pause();
                crate::log_fmt!("JOURNAL TTS: space -> pause");
                return;
            }
            SpaceTransport::Start => {}
        }
    }
    // Nothing loaded: start the cursor block from its beginning.
    stop_all_gloss_audio(state_rc);
    let index = match state_rc.borrow().journal_overlay.current_full_block_index() {
        Some(i) => i as i32,
        None => {
            crate::log_fmt!("JOURNAL TTS: no full block index (no blocks)");
            return;
        }
    };
    play_journal_block(state_rc, index);
}

/// Shift+Space in the journal Q&A overlay: batch-synthesize every Q&A
/// paragraph of the displayed entry that has no cached MP3 yet (cache-only
/// skip). The leading prepended SOURCE paragraphs are excluded — the source is
/// the work's own text, not Q&A prose (cursor Space still reads one
/// explicitly). Mirrors `synth_all_synopsis_blocks`, keyed by `(entry_id, full
/// paragraph index, voice)` — the SAME cache path/key as `play_journal_block`,
/// so a Space-synth and a batch reuse the same MP3 files and DB rows. Journal
/// Q&A reads in the shared overlay narrator.
pub(crate) fn synth_all_journal_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (entry_id, work_abbrev, blocks, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let entry_id = match s.journal.pages.get(s.journal.page_index) {
            Some(p) => p.id,
            None => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        let source_count = s.journal_overlay.source_paragraph_count();
        let blocks: Vec<(i32, String)> = s
            .journal_overlay
            .all_paragraph_texts()
            .into_iter()
            .enumerate()
            .filter(|(i, t)| *i >= source_count && !t.trim().is_empty())
            .map(|(i, t)| (i as i32, t))
            .collect();
        if blocks.is_empty() {
            return;
        }
        // Same narrator as the cursor-Space path, so a batch and a single
        // synth share one cache key.
        let (vid, mid) = (
            crate::elevenlabs::OVERLAY_NARRATOR_VOICE_ID,
            crate::elevenlabs::OP_MODEL_ID,
        );
        (entry_id, work_abbrev, blocks, vid.to_string(), mid.to_string(), s.tokio_handle.clone())
    };

    state_rc.borrow().tts_batch_running.set(true);
    show_persistent_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        for (index, raw) in &blocks {
            if let Ok(conn) = crate::db::queries::open_db() {
                let mut cached = false;
                for vid_try in [voice_id.as_str(), crate::elevenlabs::ALICE_VOICE_ID] {
                    if let Ok(Some(path)) = crate::db::queries::find_journal_audio(
                        &conn, entry_id, *index as i64, vid_try,
                    ) {
                        if std::path::Path::new(&path).exists() {
                            cached = true;
                            break;
                        }
                    }
                }
                if cached {
                    continue;
                }
            }
            let tts_text = crate::ui::gloss_ipa::ipa_for_tts(raw);
            let bytes = match synth_via(&tokio_handle, &tts_text, &voice_id, &model_id).await {
                Ok(b) => b,
                Err(e) => {
                    crate::log_fmt!("BATCH: journal synth error at para {}: {}", index, e);
                    show_tts_toast(&state_for_result, &format!("Synthesis failed: {}", e));
                    state_for_result.borrow().tts_batch_running.set(false);
                    return;
                }
            };
            let dir = journal_audio_dir(&work_abbrev, entry_id);
            let voice_tag: String = voice_id.chars().take(12).collect();
            let path = dir.join(format!("{}-{}.mp3", index, voice_tag));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                crate::log_fmt!("BATCH: mkdir {} failed: {}", dir.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                crate::log_fmt!("BATCH: write {} failed: {}", path.display(), e);
                show_tts_toast(&state_for_result, "Could not save audio");
                state_for_result.borrow().tts_batch_running.set(false);
                return;
            }
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::migrations::ensure_journal_audio_table(&conn);
                let _ = crate::db::queries::save_journal_audio(
                    &conn, entry_id, *index as i64,
                    &path.to_string_lossy(), &voice_id, &model_id,
                );
            }
            // This paragraph is now cached — color it in the open overlay now.
            recolor_journal_cached_blocks_rc(&state_for_result);
        }
        hide_tts_toast(&state_for_result);
        state_for_result.borrow().tts_batch_running.set(false);
        crate::log_fmt!("BATCH: synthesized {} journal paragraphs", blocks.len());
    });
}

/// Play a Source block's synthesized (ElevenLabs) MP3 in the gloss's active /
/// default voice, FIRST pausing the MPV recording so the two audio streams do
/// not overlap. Cache hit -> play the stored MP3; miss -> synthesize then play
/// (both handled by `play_block_tts`). Used by the gloss-overlay `r` key and by
/// the `R` picker-confirm path. MPV is paused exactly once here, immediately
/// before playback; it is never resumed by this path (the user resumes the
/// recording with `space`).
pub(crate) fn play_source_tts_pausing_mpv(state_rc: &Rc<RefCell<AppState>>, index: i32) {
    {
        let s = state_rc.borrow();
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
    }
    play_block_tts(state_rc, BlockKind::Source, index);
}

/// The current cursor block's index if it is a Source block; otherwise toast
/// "Source verse only" and return None. (The `r`/`R` synthesized-voice keys act
/// only on the source verse, where the accent bar sits to the left of the
/// source text.)
fn source_block_index(state_rc: &Rc<RefCell<AppState>>) -> Option<i32> {
    let block = state_rc.borrow().gloss_overlay.current_block();
    match block {
        Some((BlockKind::Source, index)) => Some(index),
        _ => {
            show_tts_toast(state_rc, "Source verse only");
            None
        }
    }
}

/// Gloss-overlay `R` (shift+r): open the voice picker for the Source block's
/// synthesized reading. If the TTS sink is already playing, stop it (MPV stays
/// paused) — same stop semantics as `r`. Otherwise open the picker in
/// `GlossPlay` mode; confirming sets the picked voice as the gloss's active
/// voice and plays the verse (pausing MPV first, via the GlossPlay confirm
/// path). `R` is the ONLY key that opens the picker. No-op (toast) off a Source
/// block.
pub(crate) fn pick_source_voice(state_rc: &Rc<RefCell<AppState>>) {
    // Stop-if-playing FIRST (same as `r`), before the Source gate.
    if state_rc.borrow().tts.is_playing() {
        state_rc.borrow().tts.stop();
        return;
    }
    if source_block_index(state_rc).is_none() {
        return;
    }
    crate::input::actions::settings::open_voice_picker(
        state_rc,
        crate::app::VoicePickerOrigin::GlossPlay,
    );
}

/// Run one ElevenLabs synthesis on the Tokio runtime, flattening the
/// `JoinError` into an `ElevenLabsError` so callers match a single error type.
async fn synth_via(
    tokio_handle: &tokio::runtime::Handle,
    text: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<Vec<u8>, crate::elevenlabs::ElevenLabsError> {
    let text = text.to_string();
    let voice = voice_id.to_string();
    let model = model_id.to_string();
    match tokio_handle
        .spawn(async move { crate::elevenlabs::synthesize(&text, &voice, &model).await })
        .await
    {
        Ok(inner) => inner,
        Err(e) => Err(crate::elevenlabs::ElevenLabsError::ApiError(format!(
            "tokio join error: {}",
            e
        ))),
    }
}

/// Resolve a source block's first-line start time from the current work's line
/// timestamps. Returns None if no current work, no matching block, or no
/// matched verse line carries a timestamp.
fn source_block_seek_time(s: &AppState, index: i32) -> Option<f64> {
    let gloss = s.gloss_list.get(s.gloss_index)?;
    let blocks = crate::ui::gloss_block::gloss_blocks(&gloss.gloss_text);
    let block = blocks
        .iter()
        .find(|b| b.kind == BlockKind::Source && b.index == index)?;
    let work = s.current_work.as_ref()?;

    // Citation-first (authoritative; -Amb editions are parity-numbered now).
    let start = crate::app::parse_citation(&gloss.start_citation)
        .and_then(|cit| {
            let lines: Vec<(i64, i64, i64, Option<f64>)> = work.lines.iter()
                .map(|l| (l.div1, l.div2, l.line_in_div, l.timestamp.map(|t| t.start)))
                .collect();
            start_time_for_citation(cit, &lines)
        })
        // Fallback: citationless/.txt-only works — match the verse text.
        // Match on `display` (IPA-stripped) text: work line text has no `/IPA/`,
        // so the raw `block.text` would never match an IPA-bearing verse line.
        // Seek a `SEEK_PREROLL` (0.2s) before the line start, matching every other
        // line-seek in the app (search / concordance / echoes), so `a`/`space` begin
        // just ahead of the first word rather than clipping its onset.
        .or_else(|| {
            let work_pairs: Vec<(String, Option<f64>)> = work.lines.iter()
                .map(|l| (l.text.clone(), l.timestamp.map(|t| t.start)))
                .collect();
            first_source_start_time(&block.display, &work_pairs)
        })?;
    Some(crate::input::navigation::preroll_seek_time(start))
}

/// `~/Music/glosses/<work-abbrev>/<gloss-id>/`
fn gloss_audio_dir(work_abbrev: &str, gloss_id: i64) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Music")
        .join("glosses")
        .join(work_abbrev)
        .join(gloss_id.to_string())
}

/// `~/Music/synopses/<work-abbrev>/<div1>-<div2>/`
fn synopsis_audio_dir(work_abbrev: &str, div1: i64, div2: i64) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Music")
        .join("synopses")
        .join(work_abbrev)
        .join(format!("{}-{}", div1, div2))
}

/// `~/Music/journal/<work-abbrev>/<entry-id>/`
fn journal_audio_dir(work_abbrev: &str, entry_id: i64) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Music")
        .join("journal")
        .join(work_abbrev)
        .join(entry_id.to_string())
}

/// Toast helper exposed for the voice-picker confirm path (settings.rs) to
/// report gloss-voice association from the gloss overlay.
pub(crate) fn voice_picker_toast(state_rc: &Rc<RefCell<AppState>>, verb: &str, name: &str) {
    show_tts_toast(state_rc, &format!("{}: {}", verb, name));
}

fn show_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    crate::input::navigation::show_chapter_toast_secs(&state_rc.borrow(), msg, 3);
}

/// Show a toast that stays up until something explicitly replaces it (another
/// `show_tts_toast`, which re-arms the 3s dismiss) or `hide_tts_toast`. Used for
/// "Synthesizing…", which must persist until playback begins — ElevenLabs often
/// takes longer than the 3s auto-dismiss, so a timed toast would vanish mid-synth.
/// Routes through the gen-counted chapter-toast system: the old direct
/// set_text/set_visible left a previously-armed 3s hide timer live, which took
/// this pill down mid-synthesis.
fn show_persistent_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    crate::input::navigation::show_chapter_toast_persistent(&state_rc.borrow(), msg);
}

/// Hide the toast pill immediately (used to dismiss the persistent "Synthesizing…"
/// toast the moment audio starts playing). Gen-counted + pill-restoring.
fn hide_tts_toast(state_rc: &Rc<RefCell<AppState>>) {
    crate::input::navigation::hide_chapter_toast(&state_rc.borrow());
}

/// True when the cursor's `(div1, div2, line_in_div)` triple falls within the
/// inclusive `[start, end]` citation range of a glossed passage. Rust tuple
/// ordering compares lexicographically, which matches citation ordering.
fn passage_covers(start: (i64, i64, i64), end: (i64, i64, i64), cur: (i64, i64, i64)) -> bool {
    start <= cur && cur <= end
}

/// Inclusive `(start_buf, end_buf)` buffer-line span of the reader-gloss
/// passage covering the cursor line, or `None` when the cursor line has no
/// covering reader-gloss (or no work / DB). reader-gloss ONLY — the chat
/// panel's gloss flow is the reader-gloss flow. Used by the reader-mode `-`
/// bind (`reader_gloss_chat_at_cursor`) to stage a transient selection over
/// the gloss's authored passage without the user entering visual mode.
pub(crate) fn reader_gloss_passage_at_cursor(s: &AppState) -> Option<(usize, usize)> {
    let work = s.current_work.as_ref()?;
    // Glosses are keyed by canonical_abbrev (the -BBC/-Amb lookup rule).
    let abbrev = work.canonical_abbrev.clone();
    let wl = s.work_line_for_buffer(s.current_line)?;
    let line = work.lines.get(wl)?;
    let cur = (line.div1, line.div2, line.line_in_div);

    let conn = crate::db::queries::open_db().ok()?;
    let passages =
        crate::db::queries::find_glossed_passages(&conn, &abbrev, &["reader-gloss"])
            .unwrap_or_default();

    let passage = passages.into_iter().find(|p| {
        match (
            crate::app::parse_citation(&p.start_citation),
            crate::app::parse_citation(&p.end_citation),
        ) {
            (Some(start), Some(end)) => passage_covers(start, end, cur),
            _ => false,
        }
    })?;

    // Map the passage's start/end citations to work-line indices, then to
    // buffer lines through the line map (jump_to_gloss_source_start's pattern).
    let start_t = crate::app::parse_citation(&passage.start_citation)?;
    let end_t = crate::app::parse_citation(&passage.end_citation)?;
    // A citation triple can match several work rows: the spoken line
    // (`sub_line == 0`) and any stage directions sharing its `line_in_div`
    // (`sub_line > 0`), which can sort ahead of it. A citation denotes the
    // spoken line, so prefer the `sub_line == 0` row; fall back to any match.
    let work_idx_for = |t: (i64, i64, i64)| -> Option<usize> {
        work.lines
            .iter()
            .position(|l| (l.div1, l.div2, l.line_in_div) == t && l.sub_line == 0)
            .or_else(|| {
                work.lines
                    .iter()
                    .position(|l| (l.div1, l.div2, l.line_in_div) == t)
            })
    };
    let start_wi = work_idx_for(start_t)?;
    let end_wi = work_idx_for(end_t)?;

    let a = s.buffer_line_for_work(start_wi)?;
    let b = s.buffer_line_for_work(end_wi)?;
    Some((a.min(b), a.max(b)))
}

/// Open the gloss overlay for a resolved passage and its glosses, wiring up all
/// the `gloss_*` state, painting the card, and coloring already-synthesized
/// blocks. Shared by the cursor open path (`toggle_overlay`) and the gloss
/// picker confirm path so they cannot drift — a missing `recolor_cached_blocks`
/// here was a real bug (cached blocks uncolored only when opened via the picker).
///
/// Caller responsibilities (done identically by both sites before this call):
/// set `gloss_return_pos`, and hold the `&mut AppState` borrow. `all_glosses`
/// must be non-empty. `from_picker` controls the Escape return path.
/// Pick the starting index into a gloss list for a desired gloss type.
/// Returns the index of the first gloss whose type matches `desired`, or 0
/// when `desired` is None or no gloss of that type is present.
fn start_gloss_idx(types: &[impl AsRef<str>], desired: Option<&str>) -> usize {
    desired
        .and_then(|d| types.iter().position(|t| t.as_ref() == d))
        .unwrap_or(0)
}

pub(crate) fn open_gloss_overlay(
    s: &mut AppState,
    passages: Vec<crate::db::queries::GlossedPassage>,
    passage_index: usize,
    passage: crate::db::queries::GlossedPassage,
    all_glosses: Vec<crate::db::queries::SavedGloss>,
    from_picker: bool,
    desired_type: Option<&str>,
    entry_open: bool,
) {
    let types: Vec<&str> = all_glosses.iter().map(|g| g.gloss_type.as_str()).collect();
    let idx = start_gloss_idx(&types, desired_type);
    open_gloss_overlay_at(
        s,
        passages,
        passage_index,
        passage,
        all_glosses,
        from_picker,
        idx,
        entry_open,
    );
}

/// `open_gloss_overlay` with the starting index given outright instead of
/// derived from a desired gloss TYPE. The vocab entry path picks its index by
/// cursor position, which `desired_type` cannot express (every candidate is
/// the same type). `start_idx` is clamped, so a stale index cannot panic.
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_gloss_overlay_at(
    s: &mut AppState,
    passages: Vec<crate::db::queries::GlossedPassage>,
    passage_index: usize,
    passage: crate::db::queries::GlossedPassage,
    all_glosses: Vec<crate::db::queries::SavedGloss>,
    from_picker: bool,
    start_idx: usize,
    entry_open: bool,
) {
    let idx = start_idx.min(all_glosses.len().saturating_sub(1));

    let work_title = s
        .current_work
        .as_ref()
        .map(|w| w.title.clone())
        .unwrap_or_default();
    let work_type = s
        .current_work
        .as_ref()
        .map(|w| w.work_type.clone())
        .unwrap_or_default();
    let ctx = crate::gloss::GlossContext {
        work_abbrev: passage.work_abbrev,
        work_title,
        start_citation: passage.start_citation,
        end_citation: passage.end_citation,
        act: passage.act,
        scene: passage.scene,
        speaker: passage.speaker,
        source_text: passage.source_text,
        source_line_numbers: Vec::new(),
        hash: String::new(),
        gloss_type: all_glosses[idx].gloss_type.clone(),
        work_type,
    };

    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let source_lines: Vec<(String, i64)> = Vec::new();
    let head = crate::app::division_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text,
        &all_glosses[idx].gloss_text,
        cw,
        h,
        Some(&s.theme.root_color),
        &source_lines,
        (&head.0, &head.1),
    );
    s.gloss_overlay.set_position(idx, all_glosses.len());
    // Footer cites the DISPLAYED gloss's own passage span, not the group-wide
    // ctx (glosses sharing a start_citation may span to different end_citations).
    s.gloss_overlay
        .set_citation(&all_glosses[idx].start_citation, &all_glosses[idx].end_citation);

    // Original opens from the reader stamp the entry passage so the Escape
    // close can tell a peek (restore the saved page) from a traversal (jump
    // to the shown passage's source). Traversal re-opens leave the stamp
    // untouched, so stepping away and back counts as the entry passage again.
    if entry_open {
        s.gloss_entry_citation = Some(ctx.start_citation.clone());
    }

    let shown_type = all_glosses[idx].gloss_type.clone();
    s.gloss_passages = passages;
    s.gloss_passage_index = passage_index;
    s.gloss_list = all_glosses;
    s.gloss_index = idx;
    s.gloss_active_voice = 0;
    s.gloss_context = Some(ctx);
    s.gloss_opened_from_picker = from_picker;
    // input_mode MUST be set before recolor: recolor_cached_blocks selects the
    // gloss vs synopsis branch off it and no-ops otherwise.
    s.input_mode = crate::app::InputMode::GlossOverlay;
    recolor_cached_blocks(s);

    // Re-apply / clear the overlay-search highlight for the NEWLY shown gloss.
    // Cross-passage Ctrl+n/p keeps `gloss_search` active but shows a different
    // gloss, so re-collect the pattern against this gloss's whole rendered text
    // (every page). When no search is active, clear the overlay's stored spans so
    // a prior passage's matches don't re-paint on this render.
    if s.gloss_search.is_some() {
        let text = s.gloss_overlay.whole_entry_text();
        if let Some(search) = s.gloss_search.as_mut() {
            search.matches = crate::input::overlay_search::collect(&text, &search.pattern);
            if search.current >= search.matches.len() {
                search.current = search.matches.len().saturating_sub(1);
            }
        }
        let search = s.gloss_search.clone().unwrap();
        s.gloss_overlay.set_search_matches(&search);
    } else {
        s.gloss_overlay.clear_search_tags();
    }

    // Stamp the most-recent reference from the gloss now displayed.
    s.record_last_gloss(&shown_type);
}

/// Close the gloss overlay and return to the reader, landing the cursor on the
/// glossed passage's source line (falling back to the pre-open page). This is
/// the overlay's Escape close — the only close key under the Escape-only
/// policy; `n`/Ctrl+g/Ctrl+j are consumed no-ops in this overlay.
pub(crate) fn close_gloss_to_reader(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Capture the overlay's block-cursor selection BEFORE hide/cleanup: a
    // moved cursor re-lands the reader on that block's source line below.
    let cursor_block = s.gloss_overlay.current_block();
    let cursor_moved = !s.gloss_overlay.cursor_on_first_block();
    // Closing the overlay must not leave a stale diff-highlight for the next
    // open session (Task 7).
    s.gloss_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    s.tts.stop();
    // Space source loop: full teardown (quit the loop mpv, restore the main
    // player) — leaving the overlay ends the loop.
    crate::input::actions::chat::chat_loop_teardown(&mut s);
    s.gloss_overlay.hide();
    // Ctrl+Tab focus toggle: closing the overlay resets ask-card focus.
    s.ask_card_focus = true;
    s.gloss_opened_from_picker = false;
    // Drop any overlay search + MRU so neither leaks into the next gloss overlay
    // session. Clear the overlay's stored whole-body match spans too, or the next
    // open's render_gloss_page would re-paint them. Mirrors the journal overlay's
    // close-branch cleanup.
    s.gloss_overlay.clear_search_tags();
    s.gloss_search = None;
    s.gloss_last_pattern = None;
    crate::app::return_to_reader_mode(&mut s);
    // Still showing the passage the overlay opened on from the reader, with
    // the block cursor never moved off the first stop? Then this is a
    // peek-and-Escape: restore the exact saved reading position — closing
    // must not re-frame the page the reader left (the "Escape repaginates"
    // bug). Mirrors the journal overlay's entry_page_id check in
    // journal::toggle_overlay. A MOVED block cursor instead lands the reader
    // on the source excerpt the cursor was reading (its governing source
    // block's first line); in-overlay traversal to a DIFFERENT passage with
    // an unmoved cursor falls back to that passage's source start.
    let entry = s.gloss_entry_citation.take();
    let on_entry_passage = entry.is_some()
        && s.gloss_context.as_ref().map(|c| c.start_citation.as_str()) == entry.as_deref();
    let jumped = if let Some((kind, index)) = cursor_block.filter(|_| cursor_moved) {
        jump_to_gloss_cursor_source(&mut s, kind, index)
            || (!on_entry_passage && jump_to_gloss_source_start(&mut s))
    } else if on_entry_passage {
        false
    } else {
        jump_to_gloss_source_start(&mut s)
    };
    let saved = s.gloss_return_pos.take();
    if !jumped {
        crate::app::restore_saved_position_resnap(&mut s, saved);
    }
}

/// Ctrl+a in the gloss overlay: cross-create a journal Q&A for the gloss's
/// source passage, mirroring the reader's visual-mode Ctrl+a ask. Closes the
/// overlay through the canonical close (TTS/loop teardown, search cleanup,
/// reader lands on the source passage) and opens the journal passage ask card
/// with the `<speaker>/<verse>/<stage>` markup.
/// The current gloss passage as `(div1, div2, start_citation, end_citation,
/// source_text)`, preferring the exact start..end citation range and falling
/// back to the whole scene. `None` when there is no gloss context / current
/// work. Shared by the journal-handoff path and the in-overlay float path.
fn gloss_passage_args(
    state: &Rc<RefCell<AppState>>,
) -> Option<(i64, i64, String, String, String)> {
    let s = state.borrow();
    let ctx = s.gloss_context.as_ref()?;
    let work = s.current_work.as_ref()?;
    let selected_lines: Vec<crate::db::models::Line> = match (
        crate::app::parse_citation(&ctx.start_citation),
        crate::app::parse_citation(&ctx.end_citation),
    ) {
        (Some((sd1, sd2, s_lid)), Some((_, _, e_lid))) => work
            .lines
            .iter()
            .filter(|l| {
                l.div1 == sd1 && l.div2 == sd2 && l.line_in_div >= s_lid && l.line_in_div <= e_lid
            })
            .cloned()
            .collect(),
        _ => work
            .lines
            .iter()
            .filter(|l| l.div1 == ctx.act && l.div2 == ctx.scene)
            .cloned()
            .collect(),
    };
    let markup =
        crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);
    Some((
        ctx.act,
        ctx.scene,
        ctx.start_citation.clone(),
        ctx.end_citation.clone(),
        markup,
    ))
}

/// Gloss-overlay Ctrl+a: open the journal passage Q&A in the gloss overlay's
/// OWN floated ask card (gloss commentary stays left, ask floats right) instead
/// of closing to the journal overlay. Sets the journal band + pending_passage
/// so a submit runs the journal passage flow; the gloss overlay is NOT closed.
pub(crate) fn open_passage_qa_float(state: &Rc<RefCell<AppState>>) {
    let Some((div1, div2, start, end, source_text)) = gloss_passage_args(state) else {
        return;
    };
    {
        let mut s = state.borrow_mut();
        // Mirror begin_passage_ask's state setup (journal.rs:1590-1599) so the
        // eventual ask_claude reads the right band + pending_passage — but do
        // NOT open the journal overlay or switch input_mode (we stay in the
        // gloss overlay; its ask-card intercept routes the typed keys).
        s.journal.return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
        s.journal.entry_page_id = None;
        s.journal.prompt_mode = crate::app::JournalPromptMode::Ask;
        let band = crate::app::JournalBand::Passage { div1, div2, start, end };
        s.journal.pending_passage =
            Some(crate::input::actions::journal::PendingPassage {
                source_text,
                band: band.clone(),
            });
        s.journal_band = band;
        s.journal.page_index = 0;
    }
    // Open the gloss overlay's floated ask card in PassageQa mode, then INSERT.
    show_prompt_dialog(state, crate::app::GlossPromptMode::PassageQa);
    let _ = state
        .borrow()
        .gloss_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}

/// Open the gloss overlay for the cursor line (reader Ctrl+g /
/// `Action::ToggleGlossOverlay`, and the Ctrl+Tab reopen). Open-only since
/// the Escape-only close policy: the overlay closes via Escape
/// (`close_gloss_to_reader`), never by re-pressing the toggle.
pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    open_gloss_at_cursor(state);
}

/// Open the gloss overlay for the passage covering the reader cursor line
/// (the open half of `toggle_overlay`, shared with the `\` segment-overlay
/// cycle). Toasts "No gloss on this line" and opens nothing when no glossed
/// passage covers the cursor. Saves `gloss_return_pos` from the current
/// reader position so Escape/cycle-advance can restore it.
pub(crate) fn open_gloss_at_cursor(state: &Rc<RefCell<AppState>>) {
    if !try_open_gloss_at_cursor(state) {
        show_tts_toast(state, "No gloss on this line");
    }
}

/// Whether a gloss of any of `gloss_types` covers the reader cursor line —
/// the same lookup `try_open_gloss_at_cursor` / `try_open_syntax_gloss_at_cursor`
/// perform, WITHOUT opening anything or touching overlay state.
///
/// The `\` overlay cycle (`overlay_cycle::advance`) needs this: it must know
/// whether the next stop can open BEFORE it tears down the current overlay,
/// so that a lap with no reachable stop can leave the current overlay up and
/// toast instead of dumping the reader out of an overlay it can't replace.
/// Whether a `gloss_context`'s work agrees with the work actually loaded.
///
/// `gloss_context` is set when a gloss opens and is NOT cleared when the
/// overlay closes, so it outlives its work unless a work switch clears it
/// (`display_work_at_with_prepared`). This is the belt to that suspenders: the
/// probe below queries with `ctx.work_abbrev` while the opener queries the
/// current work, and a disagreement makes the probe answer about a work the
/// opener will never look in. Pure so the rule is testable.
fn displayed_span_is_current_work(current_abbrev: Option<&str>, ctx_abbrev: &str) -> bool {
    current_abbrev == Some(ctx_abbrev)
}

pub(crate) fn gloss_covers_cursor(state: &Rc<RefCell<AppState>>, gloss_types: &[&str]) -> bool {
    // When an overlay is already open, the question the cycle is really asking
    // is "does another stop cover the passage I am LOOKING AT", not "…the
    // single anchor line". Those differ whenever the stops sit on passages of
    // different widths — which is the norm: a reader-gloss spans a whole speech
    // (Ant.5.2.424-437) while a syntax gloss spans the one sentence it was
    // created from (424-425). Anchoring on line 437 found no syntax gloss and
    // reported "nothing else to cycle to" even though the displayed passage
    // plainly has one. Test SPAN OVERLAP against the displayed passage first.
    let displayed_span = {
        let s = state.borrow();
        // Only trust the displayed span when its work IS the loaded work. A
        // stale context from a previous work would send this branch querying
        // the OLD abbrev and RETURN from it, while the opener queries the
        // current work — probe true, open nothing, reader with no toast.
        let current = s.current_work.as_ref().map(|w| w.canonical_abbrev.as_str());
        s.gloss_context
            .as_ref()
            .filter(|ctx| displayed_span_is_current_work(current, &ctx.work_abbrev))
            .and_then(|ctx| {
                let start = crate::app::parse_citation(&ctx.start_citation)?;
                let end = crate::app::parse_citation(&ctx.end_citation)?;
                Some((ctx.work_abbrev.clone(), start, end))
            })
    };
    if let Some((abbrev, dstart, dend)) = displayed_span {
        if let Ok(conn) = crate::db::queries::open_db() {
            let passages =
                crate::db::queries::find_glossed_passages(&conn, &abbrev, gloss_types)
                    .unwrap_or_default();
            return passages.iter().any(|p| {
                match (
                    crate::app::parse_citation(&p.start_citation),
                    crate::app::parse_citation(&p.end_citation),
                ) {
                    // Inclusive overlap of [dstart, dend] and [start, end].
                    (Some(start), Some(end)) => start <= dend && dstart <= end,
                    _ => false,
                }
            });
        }
    }

    let (work_abbrev, cur_triple) = {
        let s = state.borrow();
        let Some(work) = s.current_work.as_ref() else {
            return false;
        };
        // Resolve from the LAP ANCHOR, not `current_line`: opening the gloss
        // stop moves the cursor to the END of the glossed passage, so probing
        // the live cursor asks "is there a syntax gloss at line 437?" when the
        // lap started at 424. Passages of different widths (a reader-gloss over
        // a whole speech vs a syntax gloss over one sentence) then never match.
        // `gloss_return_pos`/`journal.return_pos` hold the position the lap was
        // anchored to; fall back to the cursor when no overlay is open.
        let anchor = s
            .gloss_return_pos
            .or(s.journal.return_pos)
            .map(|(line, _, _)| line)
            .unwrap_or(s.current_line);
        let Some(wl) = s.work_line_for_buffer(anchor) else {
            return false;
        };
        let Some(line) = work.lines.get(wl) else {
            return false;
        };
        (
            work.canonical_abbrev.clone(),
            (line.div1, line.div2, line.line_in_div),
        )
    };

    let Ok(conn) = crate::db::queries::open_db() else {
        return false;
    };
    crate::db::queries::find_glossed_passages(&conn, &work_abbrev, gloss_types)
        .unwrap_or_default()
        .iter()
        .any(|p| {
            match (
                crate::app::parse_citation(&p.start_citation),
                crate::app::parse_citation(&p.end_citation),
            ) {
                (Some(start), Some(end)) => passage_covers(start, end, cur_triple),
                _ => false,
            }
        })
}

/// Gloss types of the cycle's GLOSS stop (reader-facing types, excluding
/// syntax-gloss, which is its own stop). Exposed for `overlay_cycle`'s
/// availability probe.
pub(crate) const CYCLE_GLOSS_TYPES: &[&str] = READER_GLOSS_TYPES;

/// Gloss type of the cycle's SYNTAX stop.
pub(crate) const CYCLE_SYNTAX_TYPES: &[&str] = &["syntax-gloss"];

/// The open half of `open_gloss_at_cursor` without the miss toast: returns
/// false (opening nothing) when no glossed passage covers the cursor, so the
/// prose `-` path can fall through to background glossing instead of toasting.
pub(crate) fn try_open_gloss_at_cursor(state: &Rc<RefCell<AppState>>) -> bool {
    // Reader-facing types only: this is the cycle's GLOSS stop, and a
    // syntax-gloss is its own separate stop (`try_open_syntax_gloss_at_cursor`
    // below) — mixing it in here would collapse the two stops.
    const GLOSS_TYPES: &[&str] = READER_GLOSS_TYPES;

    // Resolve the cursor line -> its (work abbrev, (div1, div2, line_in_div)).
    let (work_abbrev, cur_triple) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return false,
        };
        let wl = match s.work_line_for_buffer(s.current_line) {
            Some(wl) => wl,
            None => return false,
        };
        let line = match work.lines.get(wl) {
            Some(l) => l,
            None => return false,
        };
        (
            // Glosses are STORED under the canonical base abbrev, so look them
            // up the same way — every gloss path (save, overlay, picker, tint)
            // uses `Work.canonical_abbrev` or a variant edition misses its own
            // glosses (the recurring `-BBC`/`-Amb` lookup-mismatch bug class).
            work.canonical_abbrev.clone(),
            (line.div1, line.div2, line.line_in_div),
        )
    };

    // Load every glossed passage for this work and find the one covering the
    // cursor line. All read-only DB work happens before any state mutation.
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();

    let covering = passages.iter().enumerate().find(|(_, p)| {
        match (crate::app::parse_citation(&p.start_citation), crate::app::parse_citation(&p.end_citation)) {
            (Some(start), Some(end)) => passage_covers(start, end, cur_triple),
            _ => false,
        }
    });
    let (passage_index, passage) = match covering {
        Some((i, p)) => (i, p.clone()),
        None => return false,
    };

    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();

    if all_glosses.is_empty() {
        return false;
    }

    // All resolution done; mutate state and open the overlay under one borrow.
    let mut s = state.borrow_mut();
    // Remember the reader page so Escape returns here.
    s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    // Opened from the reader cursor, not the picker (from_picker = false): Escape
    // uses the saved reader page, not the picker return path.
    open_gloss_overlay(&mut s, passages, passage_index, passage, all_glosses, false, None, true);
    true
}

/// Open the gloss overlay filtered to `vocab-word`, for the passage covering
/// the reader cursor line (reader Ctrl+Shift+g / `Action::ShowVocabGloss`).
///
/// A vocab-word gloss is PER-OCCURRENCE, not per-word: all 12 in `LoJ` gloss
/// the single word "solicitude", each attached to a different passage
/// (`LoJ.1.2207`, `LoJ.4.14043`, …). So the cursor line — not the word under
/// it — is what selects which gloss opens, and the bind fires anywhere inside
/// the glossed span rather than only on the word token.
///
/// Deliberately does NOT carry the displayed-passage overlap fallback that
/// `try_open_syntax_gloss_at_cursor` has below. That fallback exists so the
/// `\` cycle can reach a narrow syntax passage while a wider reader-gloss
/// overlay is up; this bind opens straight from the reader cursor, where a
/// strict cursor-line match is the predictable rule. Wire that fallback in
/// only if vocab ever becomes a `\` stop.
///
/// Returns false (opening nothing) when no vocab-word gloss covers the cursor;
/// `open_vocab_gloss_at_cursor` turns that into a toast.
pub(crate) fn try_open_vocab_gloss_at_cursor(state: &Rc<RefCell<AppState>>) -> bool {
    const GLOSS_TYPES: &[&str] = &["vocab-word"];

    let (work_abbrev, cur_triple) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return false,
        };
        let wl = match s.work_line_for_buffer(s.current_line) {
            Some(wl) => wl,
            None => return false,
        };
        let line = match work.lines.get(wl) {
            Some(l) => l,
            None => return false,
        };
        (
            // Glosses are STORED under the canonical base abbrev — look them up
            // the same way or a variant edition (-Amb/-BBC) misses its own.
            work.canonical_abbrev.clone(),
            (line.div1, line.div2, line.line_in_div),
        )
    };

    // All read-only DB work happens before any state mutation.
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();

    let covering = passages.iter().enumerate().find(|(_, p)| {
        match (crate::app::parse_citation(&p.start_citation), crate::app::parse_citation(&p.end_citation)) {
            (Some(start), Some(end)) => passage_covers(start, end, cur_triple),
            _ => false,
        }
    });
    let (passage_index, passage) = match covering {
        Some((i, p)) => (i, p.clone()),
        None => return false,
    };

    // Paragraph scope first: vocab glosses on the cursor's OWN line, gathered
    // across passages. A prose passage spans two paragraphs and a paragraph's
    // words are split between sibling passages, so passage scope is wrong in
    // both directions. Falls back to passage scope when the line yields
    // nothing -- verse, and any row whose word could not be uniquely located,
    // have NULL line_in_div.
    let (div1, div2, line_in_div) = cur_triple;
    let by_line = crate::db::queries::find_vocab_glosses_by_line(
        &conn, &work_abbrev, div1, div2, line_in_div,
    )
    .unwrap_or_default();

    let (all_glosses, headwords) = if by_line.is_empty() {
        let g = crate::db::queries::find_glosses_by_start(
            &conn, &passage.work_abbrev, &passage.start_citation, GLOSS_TYPES,
        )
        .unwrap_or_default();
        let h = crate::db::queries::find_vocab_headwords_by_start(
            &conn, &passage.work_abbrev, &passage.start_citation,
        )
        .unwrap_or_default();
        (g, h)
    } else {
        let h = crate::db::queries::find_vocab_headwords_by_line(
            &conn, &work_abbrev, div1, div2, line_in_div,
        )
        .unwrap_or_default();
        (by_line, h)
    };

    if all_glosses.is_empty() {
        return false;
    }

    let mut s = state.borrow_mut();

    // Land on the vocab word at the CURSOR rather than the segment's first: a
    // vocab passage spans several reader lines, and opening at index 0 ignored
    // which line the reader was actually on. Falls back to index 0 (now the
    // segment's first word by text order) when the cursor line has no glossed
    // word — the passage can start mid-line.
    let start_idx = nearest_vocab_gloss_idx(&s, &headwords).unwrap_or(0);

    // Remember the reader page so Escape returns here.
    s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    open_gloss_overlay_at(
        &mut s,
        passages,
        passage_index,
        passage,
        all_glosses,
        false,
        start_idx,
        true,
    );
    true
}

/// Index into the vocab gloss list of the word the reader cursor is on, or
/// None when no glossed word sits on the cursor line.
///
/// The reader cursor is a LINE, not a character position (`AppState` has
/// `current_line` and no column; navigation and the cursor highlight are both
/// line-oriented), so "nearest the cursor" can only mean the cursor's own
/// line. A vocab passage spans several lines, and opening at the passage's
/// first word ignored which of those lines the reader was on — that is the
/// part this fixes. Within one line the earliest glossed word wins, matching
/// the left-to-right reading order the list is now sorted by.
///
/// `vocab_matches` is already built for the highlighter, keyed by BUFFER line
/// — the same space as `current_line` — so this needs no extra query.
/// Matching is case-insensitive: the stored headword is the lemma
/// ("conceive") while the text may be capitalized at a sentence start.
fn nearest_vocab_gloss_idx(s: &AppState, headwords: &[String]) -> Option<usize> {
    if headwords.is_empty() {
        return None;
    }
    // Collect then scan in column order: the earliest word on the line may be
    // highlighted without having a gloss in THIS passage (the passage can start
    // mid-line, and vocab highlighting is independent of gloss existence), so
    // fall through to the next one rather than giving up on the first miss.
    let mut on_line: Vec<&crate::app::VocabMatch> = s
        .vocab_matches
        .iter()
        .filter(|m| m.line_index == s.current_line)
        .collect();
    on_line.sort_by_key(|m| m.char_start);
    on_line.iter().find_map(|m| {
        headwords
            .iter()
            .position(|h| h.eq_ignore_ascii_case(&m.word))
    })
}

/// Reader Ctrl+Shift+g: open the cursor line's vocab-word gloss, or toast when
/// there is none. The toast fires AFTER `try_open_vocab_gloss_at_cursor` has
/// dropped its borrow — `show_tts_toast` borrows state again.
pub(crate) fn open_vocab_gloss_at_cursor(state: &Rc<RefCell<AppState>>) {
    if !try_open_vocab_gloss_at_cursor(state) {
        show_tts_toast(state, "No vocab gloss on this line");
    }
}

/// Open the gloss overlay filtered to `syntax-gloss`, for the passage
/// covering the reader cursor line — the syntax stop of the `\` segment-
/// overlay cycle (`cycle_from_journal` in `overlay_cycle.rs`). A parallel of
/// `try_open_gloss_at_cursor` scoped to one gloss type instead of the three
/// reader-facing ones: a syntax gloss is created from an explicit selection
/// (visual-mode "Syntax" or the `-`/`_` underline path), never from the
/// cursor line alone, so this can only ever show one that already exists.
/// Returns false (opening nothing) when no syntax-gloss covers the cursor.
pub(crate) fn try_open_syntax_gloss_at_cursor(state: &Rc<RefCell<AppState>>) -> bool {
    const GLOSS_TYPES: &[&str] = &["syntax-gloss"];

    let (work_abbrev, cur_triple) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return false,
        };
        let wl = match s.work_line_for_buffer(s.current_line) {
            Some(wl) => wl,
            None => return false,
        };
        let line = match work.lines.get(wl) {
            Some(l) => l,
            None => return false,
        };
        (
            work.canonical_abbrev.clone(),
            (line.div1, line.div2, line.line_in_div),
        )
    };

    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();

    let covering = passages.iter().enumerate().find(|(_, p)| {
        match (crate::app::parse_citation(&p.start_citation), crate::app::parse_citation(&p.end_citation)) {
            (Some(start), Some(end)) => passage_covers(start, end, cur_triple),
            _ => false,
        }
    });
    // Cursor-line miss: fall back to a passage OVERLAPPING the displayed one.
    // The `\` cycle reaches this stop from another overlay whose passage is
    // usually WIDER (a reader-gloss over a whole speech vs a syntax gloss over
    // one sentence), so the anchor line often sits outside the narrower span
    // even though both describe the same passage — the "`\` won't cycle to the
    // syntax gloss" bug. `gloss_covers_cursor` probes with this same rule;
    // probe and open MUST agree or the cycle advances into an empty stop.
    let covering = covering.map(|(i, p)| (i, p.clone())).or_else(|| {
        let s = state.borrow();
        let ctx = s.gloss_context.as_ref()?;
        let dstart = crate::app::parse_citation(&ctx.start_citation)?;
        let dend = crate::app::parse_citation(&ctx.end_citation)?;
        passages
            .iter()
            .enumerate()
            .find(|(_, p)| {
                match (
                    crate::app::parse_citation(&p.start_citation),
                    crate::app::parse_citation(&p.end_citation),
                ) {
                    (Some(start), Some(end)) => start <= dend && dstart <= end,
                    _ => false,
                }
            })
            .map(|(i, p)| (i, p.clone()))
    });
    let (passage_index, passage) = match covering {
        Some((i, p)) => (i, p),
        None => return false,
    };

    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();

    if all_glosses.is_empty() {
        return false;
    }

    let mut s = state.borrow_mut();
    s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    open_gloss_overlay(&mut s, passages, passage_index, passage, all_glosses, false, None, true);
    true
}

/// Prose `-` (`ReaderGlossChatAtCursor` routed here by
/// `visual::reader_gloss_chat_at_cursor`): open the gloss overlay on the
/// gloss covering the cursor. When none exists, gloss the cursor paragraph in
/// the BACKGROUND — reading continues under a "Glossing…" toast, and the
/// overlay opens on the new gloss when the reply lands (unlike visual-mode
/// gloss, which opens the overlay immediately on its loading card).
pub(crate) fn prose_gloss_overlay_at_cursor(state: &Rc<RefCell<AppState>>) {
    if try_open_gloss_at_cursor(state) {
        return;
    }
    background_gloss_cursor_segment(state);
}

/// Gloss the cursor's paragraph block as a reader-gloss without opening any
/// surface, then open the gloss overlay on the saved gloss. Guarded by
/// `AppState.prose_gloss_pending` so a second `-` mid-flight cannot double-fire
/// the paid API call.
fn background_gloss_cursor_segment(state_rc: &Rc<RefCell<AppState>>) {
    let prepared = {
        let s = state_rc.borrow();
        let block = crate::input::visual::cursor_block_bounds(&s);
        let ctx = block.and_then(|(start, end)| {
            let work = s.current_work.as_ref()?;
            let lines: Vec<crate::db::models::Line> = (start..=end)
                .filter_map(|bl| {
                    s.work_line_for_buffer(bl)
                        .and_then(|wi| work.lines.get(wi).cloned())
                })
                .collect();
            crate::gloss::build_context_for_type(work, &lines, "reader-gloss")
        });
        match ctx {
            Some(c) => Some((c, s.config.claude_model.clone())),
            None => None,
        }
    };
    let Some((ctx, model)) = prepared else {
        show_tts_toast(state_rc, "Nothing to gloss here");
        return;
    };
    background_gloss_request(state_rc, ctx, model);
}

/// Prose V-mode `-` (`visual::action_reader_gloss_chat`'s prose branch): the
/// selection's reader-gloss ctx, already built and visual mode already exited.
/// Cache hit opens the overlay on the stored gloss (no API spend, mirroring
/// the chat path's `-` cache check); miss goes to the background request.
pub(crate) fn prose_gloss_selection(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: crate::gloss::GlossContext,
    model: String,
) {
    let cached = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_glosses_by_start(
                &conn,
                &ctx.work_abbrev,
                &ctx.start_citation,
                &["reader-gloss"],
            )
            .ok()
        })
        .map(|g| !g.is_empty())
        .unwrap_or(false);
    if cached {
        open_gloss_overlay_by_start(state_rc, &ctx.work_abbrev, &ctx.start_citation);
        return;
    }
    background_gloss_request(state_rc, ctx, model);
}

/// Fire the reader-gloss request for `ctx` with no surface open — "Glossing…"
/// toast, background call, overlay opened on the saved gloss by the completion
/// handler. Shared by the prose cursor-paragraph and selection paths.
fn background_gloss_request(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: crate::gloss::GlossContext,
    model: String,
) {
    let hold_gen = {
        let s = state_rc.borrow();
        // A second `-` mid-flight: the held "Glossing…" toast is already up —
        // re-toasting would steal its generation and expire it early.
        if s.prose_gloss_pending.get() {
            return;
        }
        s.prose_gloss_pending.set(true);
        // Held (no expiry) for the whole round-trip; the completion handlers
        // release it (overlay open) or supersede it (failure/ready toasts).
        crate::input::navigation::show_chapter_toast_hold(&s, "Glossing\u{2026}")
    };

    let neighbors = crate::gloss::neighbors_for_ctx(&ctx);
    crate::logging::log(&format!(
        "PROSE-GLOSS: background glossing {}-{} ({} neighbor(s))",
        ctx.start_citation, ctx.end_citation, neighbors.len()
    ));
    let user_msg = crate::gloss::build_user_message(&ctx, None, None, &neighbors);

    let model_for_db = model.clone();
    let ctx_ok = ctx.clone();
    let on_success = move |sr: &Rc<RefCell<AppState>>, reply: String| {
        let open = {
            let mut s = sr.borrow_mut();
            s.prose_gloss_pending.set(false);
            // Persists the row, reloads the chat-side gloss cache, and re-derives
            // the glossed-line tint so the paragraph colors immediately.
            let saved =
                crate::input::actions::chat::save_reader_gloss(&mut s, &ctx_ok, &reply, &model_for_db);
            if saved.is_none() {
                crate::input::navigation::show_chapter_toast_secs(&s, "Gloss not saved", 3);
                return;
            }
            // Only yank the user into the overlay from plain reading in the
            // same work; anywhere else (another mode, another work), announce
            // and let Ctrl+g open it.
            let same_work = s
                .current_work
                .as_ref()
                .map(|w| w.canonical_abbrev.as_str() == ctx_ok.work_abbrev)
                .unwrap_or(false);
            if s.input_mode == crate::app::InputMode::Reader && same_work {
                // Drop the held "Glossing…" toast in the same breath as the
                // overlay opens (the failure/ready toasts supersede it via
                // their own generation bump instead).
                crate::input::navigation::release_chapter_toast_hold(&s, hold_gen);
                true
            } else {
                crate::input::navigation::show_chapter_toast_secs(&s, "Gloss ready", 3);
                false
            }
        };
        if open {
            open_gloss_overlay_by_start(sr, &ctx_ok.work_abbrev, &ctx_ok.start_citation);
        }
    };
    let on_error = move |sr: &Rc<RefCell<AppState>>, e: &str| {
        let s = sr.borrow();
        s.prose_gloss_pending.set(false);
        crate::input::navigation::show_chapter_toast_secs(&s, "Gloss failed", 3);
        crate::logging::log(&format!("PROSE-GLOSS: API error: {}", e));
    };

    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::reader_gloss_prompt(&ctx.work_type).to_string(),
        user_msg,
        model,
        on_success,
        on_error,
    );
}

/// Open the gloss overlay on the passage keyed by `start_citation` (the
/// background-gloss completion path — the cursor may have moved since the
/// request fired, so resolve by the glossed passage's own key, not the
/// cursor). Newest-first ordering makes index 0 the just-saved reader-gloss.
fn open_gloss_overlay_by_start(
    state: &Rc<RefCell<AppState>>,
    work_abbrev: &str,
    start_citation: &str,
) {
    // ANY type, including syntax-gloss. This was READER_GLOSS_TYPES on the
    // assumption that both callers are the background reader-gloss path — that
    // assumption was WRONG, and the failure was silent and total: the gloss
    // PICKER lists syntax glosses (its own query includes them), and selecting
    // one lands here. With syntax-gloss excluded, `all_glosses` came back empty
    // and this function returned without opening anything — while the caller
    // had already flipped `input_mode` back to Reader. The user was left in the
    // reader staring at the STALE overlay still painted from before, with every
    // keypress going to the reader behind it. Reported 2026-07-26; the log shows
    // `Return` in mode=GlossPicker producing no output at all, then the next key
    // arriving in mode=Reader.
    //
    // Excluding a type here can only ever silently drop an open, never protect
    // anything: the caller has already resolved WHICH gloss it wants.
    const GLOSS_TYPES: &[&str] = ANY_GLOSS_TYPES;
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();
    let Some(idx) = passages.iter().position(|p| p.start_citation == start_citation) else {
        // Same trap as the empty-glosses branch below: returning bare here
        // strands a stale overlay over a reader that is already taking the
        // keys. This is the branch the 2026-07-26 picker bug actually hit,
        // since a GLOSS_TYPES list without syntax-gloss makes `passages` miss
        // the citation entirely.
        let s = state.borrow();
        s.gloss_overlay.hide();
        crate::logging::log(&format!(
            "GLOSS_OPEN: no glossed passage at {start_citation} — nothing opened"
        ));
        return;
    };
    let passage = passages[idx].clone();
    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();
    if all_glosses.is_empty() {
        // Belt-and-braces after the GLOSS_TYPES fix above: a bare `return` here
        // is what made that bug invisible AND unrecoverable. The caller has
        // already set input_mode away from the picker, so returning without
        // opening leaves a STALE overlay painted over a reader that is silently
        // receiving the keys. Hide it and say so in the log.
        let s = state.borrow();
        s.gloss_overlay.hide();
        crate::logging::log(&format!(
            "GLOSS_OPEN: no gloss found for {start_citation} — nothing opened"
        ));
        return;
    }

    let mut s = state.borrow_mut();
    s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    open_gloss_overlay(
        &mut s,
        passages,
        idx,
        passage,
        all_glosses,
        false,
        Some("reader-gloss"),
        true,
    );
    crate::logging::log("PROSE-GLOSS: opened overlay on background gloss");
}

/// Reopen whichever toggleable overlay (gloss/journal) was last open
/// (`AppState.last_overlay`, recorded at every close via
/// `return_to_reader_mode`). Reader-only: overlays close via Escape alone,
/// so this no longer doubles as an in-overlay close. Toasts when nothing is
/// remembered. Bound to Ctrl+Tab (`ToggleLastOverlay`).
pub(crate) fn toggle_last_overlay(state: &Rc<RefCell<AppState>>) {
    use crate::app::{InputMode, LastOverlay};
    let (mode, last) = {
        let s = state.borrow();
        (s.input_mode, s.last_overlay)
    };
    if mode != InputMode::Reader {
        return;
    }
    match last {
        Some(LastOverlay::Gloss) => toggle_overlay(state),
        Some(LastOverlay::Journal) => crate::input::actions::journal::toggle_overlay(state),
        None => show_tts_toast(state, "No overlay to reopen"),
    }
}

/// Reopen the gloss overlay on the most-recently-viewed gloss for the current
/// work (persisted in `config.last_gloss`), restored to the gloss type that was
/// last shown. Toasts "No recent gloss" when there is no usable reference
/// (none recorded, passage gone, or no glosses remain).
pub(crate) fn open_last_gloss(state: &Rc<RefCell<AppState>>) {
    // ANY gloss type, including syntax-gloss: `config.last_gloss` records
    // whatever gloss type the user actually viewed (visual.rs and
    // persist_render_install_gloss both call record_last_gloss for
    // syntax-gloss too), so the reload set here must be able to find it back
    // — a narrower set would record-then-never-find a syntax-gloss reference,
    // going dead or (via `find_glosses_by_start`'s unwrap_or(0) below)
    // silently reopening a different gloss type than the one recorded.
    const GLOSS_TYPES: &[&str] = ANY_GLOSS_TYPES;

    // Resolve current work + the stored reference, under a shared borrow.
    let (work_abbrev, start_citation, desired_type) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => {
                drop(s);
                show_tts_toast(state, "No recent gloss");
                return;
            }
        };
        // `last_gloss` is KEYED by the canonical base abbrev (record_last_gloss
        // writes `ctx.work_abbrev` = `Work.canonical_abbrev`), and glosses are
        // STORED under it too — read with the same key. Same bug/fix class as
        // toggle_overlay above.
        let abbrev = work.canonical_abbrev.clone();
        match s.config.last_gloss.get(&abbrev) {
            Some(lg) => (abbrev, lg.start_citation.clone(), lg.gloss_type.clone()),
            None => {
                drop(s);
                show_tts_toast(state, "No recent gloss");
                return;
            }
        }
    };

    // Read-only DB work before any mutation (same pattern as toggle_overlay).
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => {
            show_tts_toast(state, "No recent gloss");
            return;
        }
    };
    let passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, GLOSS_TYPES)
        .unwrap_or_default();

    let found = passages
        .iter()
        .enumerate()
        .find(|(_, p)| p.start_citation == start_citation);
    let (passage_index, passage) = match found {
        Some((i, p)) => (i, p.clone()),
        None => {
            // Stale reference: passage deleted or work re-imported.
            show_tts_toast(state, "No recent gloss");
            return;
        }
    };

    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();
    if all_glosses.is_empty() {
        show_tts_toast(state, "No recent gloss");
        return;
    }

    let mut s = state.borrow_mut();
    // Remember the reader page so Escape returns here (from_picker = false).
    s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    open_gloss_overlay(
        &mut s,
        passages,
        passage_index,
        passage,
        all_glosses,
        false,
        Some(&desired_type),
        true,
    );
}

/// Close the stacked gloss add/edit input card and return focus to the gloss.
/// The reader stays in `InputMode::GlossOverlay` throughout (the card lives
/// inside the gloss overlay, like the synopsis ask card).
pub(crate) fn close_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().gloss_overlay.close_ask_card();
    // Ctrl+Tab focus toggle: closing the ask card always resets focus + dim so
    // no stale state leaks into the next open.
    let mut s = state.borrow_mut();
    s.ask_card_focus = true;
    s.gloss_overlay.clear_focus_dim();
}

#[cfg(test)]
mod start_gloss_idx_tests {
    use super::start_gloss_idx;

    #[test]
    fn matches_requested_type() {
        let types = ["teacher-generic", "reader-gloss", "inner-monologue"];
        assert_eq!(start_gloss_idx(&types, Some("reader-gloss")), 1);
        assert_eq!(start_gloss_idx(&types, Some("inner-monologue")), 2);
    }

    #[test]
    fn falls_back_to_zero_when_type_absent() {
        let types = ["teacher-generic"];
        assert_eq!(start_gloss_idx(&types, Some("reader-gloss")), 0);
    }

    #[test]
    fn falls_back_to_zero_when_none_requested() {
        let types = ["teacher-generic", "reader-gloss"];
        assert_eq!(start_gloss_idx(&types, None), 0);
    }
}

/// Substitute for an empty `R` (Edit) instruction: Ctrl+Enter with no text
/// regenerates the gloss afresh under the default prompt, mirroring the journal
/// Q&A rewrite (`journal::submit_prompt`). Without this, an empty edit message
/// would leave the reader-gloss-edit prompt's "additional lines" framing
/// unanswered.
const EDIT_GLOSS_DEFAULT_INSTRUCTION: &str =
    "No further instruction was given; rewrite this gloss afresh under the \
     standard reader-gloss guidance, grounded in the same passage as before.";

/// Submit the stacked gloss input card: read its text, close it, and route to
/// `add_gloss` / `edit_gloss` by the active prompt mode.
///
/// Empty input is a no-op for Add/FixIpa (nothing to ask). For Edit (`R`),
/// empty input means "Ctrl+Enter with no prompt" — regenerate the gloss with
/// the default instruction rather than doing nothing.
pub(crate) fn submit_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    let (prompt, mode) = {
        let s = state.borrow();
        (s.gloss_overlay.take_ask_text(), s.gloss_prompt_mode)
    };
    close_gloss_prompt(state);
    let is_empty = prompt.trim().is_empty();
    match mode {
        crate::app::GlossPromptMode::Add if is_empty => {}
        crate::app::GlossPromptMode::Add => add_gloss(state, &prompt),
        crate::app::GlossPromptMode::Edit if is_empty => {
            edit_gloss(state, EDIT_GLOSS_DEFAULT_INSTRUCTION)
        }
        crate::app::GlossPromptMode::Edit => edit_gloss(state, &prompt),
        crate::app::GlossPromptMode::FixIpa if is_empty => {}
        crate::app::GlossPromptMode::FixIpa => fix_word_ipa(state, &prompt),
        // Empty passage question → stay in the gloss overlay (nothing to ask);
        // close_gloss_prompt (called above) already hid the float.
        crate::app::GlossPromptMode::PassageQa if is_empty => {}
        // Non-empty → close the gloss overlay and run the journal passage flow.
        // The band + pending_passage were set in open_passage_qa_float, so
        // submit_passage_question's ask_claude reads them and lands the answer
        // in the journal overlay (today's post-submit behavior).
        crate::app::GlossPromptMode::PassageQa => {
            close_gloss_to_reader(state);
            crate::input::actions::journal::submit_passage_question(state, &prompt);
        }
    }
}

/// Start time of the work line whose citation == `cit`. Pure + testable.
fn start_time_for_citation(
    cit: (i64, i64, i64),
    lines: &[(i64, i64, i64, Option<f64>)], // (div1, div2, line_in_div, start)
) -> Option<f64> {
    lines.iter()
        .find(|(d1, d2, l, _)| (*d1, *d2, *l) == cit)
        .and_then(|(_, _, _, start)| *start)
}

/// Given a source block's verse text (one quoted line per `\n`) and the work's
/// lines as `(text, Option<start_seconds>)`, return the start time of the FIRST
/// verse line (in block order) that matches a work line carrying a timestamp.
/// Matching is exact on trimmed text. None if no matched line has timing.
/// Citationless fallback — use `start_time_for_citation` as the primary path.
fn first_source_start_time(verses: &str, work: &[(String, Option<f64>)]) -> Option<f64> {
    for verse in verses.lines() {
        let needle = verse.trim();
        if needle.is_empty() {
            continue;
        }
        for (text, start) in work {
            if text.trim() == needle {
                if let Some(s) = start {
                    return Some(*s);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod displayed_span_guard_tests {
    use super::displayed_span_is_current_work;

    /// The `\` cycle's probe reads the DISPLAYED passage out of `gloss_context`
    /// and queries with `ctx.work_abbrev`, while the opener queries the CURRENT
    /// work. `gloss_context` is never cleared on a work switch, so after one the
    /// probe could answer about the OLD work: it returns true, `advance()` tears
    /// the overlay down, the opener finds nothing in the new work, and the user
    /// lands in the reader with no toast.
    #[test]
    fn stale_context_from_another_work_is_rejected() {
        assert!(!displayed_span_is_current_work(Some("Ant"), "Cym"));
    }

    #[test]
    fn context_matching_the_current_work_is_accepted() {
        assert!(displayed_span_is_current_work(Some("Cym"), "Cym"));
    }

    /// No work loaded — there is nothing the displayed span can agree with, so
    /// the probe must not trust it.
    #[test]
    fn absent_current_work_is_rejected() {
        assert!(!displayed_span_is_current_work(None, "Cym"));
    }
}

#[cfg(test)]
mod passage_cover_tests {
    use super::passage_covers;

    #[test]
    fn covers_inclusive_endpoints_and_interior() {
        let start = (1, 2, 10);
        let end = (1, 2, 20);
        assert!(passage_covers(start, end, (1, 2, 10))); // start endpoint
        assert!(passage_covers(start, end, (1, 2, 20))); // end endpoint
        assert!(passage_covers(start, end, (1, 2, 15))); // interior
    }

    #[test]
    fn excludes_outside_the_range() {
        let start = (1, 2, 10);
        let end = (1, 2, 20);
        assert!(!passage_covers(start, end, (1, 2, 9)));  // before start
        assert!(!passage_covers(start, end, (1, 2, 21))); // after end
    }

    #[test]
    fn spans_div_boundaries_lexicographically() {
        // Passage from 1.2.50 through 2.1.3 covers everything between.
        let start = (1, 2, 50);
        let end = (2, 1, 3);
        assert!(passage_covers(start, end, (1, 3, 1))); // later scene, same act
        assert!(passage_covers(start, end, (2, 1, 1))); // next act, within end
        assert!(!passage_covers(start, end, (2, 1, 4))); // past end line
        assert!(!passage_covers(start, end, (1, 2, 49))); // before start line
    }
}

#[cfg(test)]
mod source_timing_tests {
    use super::*;

    #[test]
    fn first_timed_matching_line_wins() {
        let work: Vec<(String, Option<f64>)> = vec![
            ("Ah, my good Lord of Winchester, I thank you.".into(), None),
            ("You are always my good friend.".into(), Some(12.5)),
            ("I shall both find your Lordship judge and juror,".into(), Some(15.0)),
        ];
        let verses = "Ah, my good Lord of Winchester, I thank you.\nYou are always my good friend.";
        assert_eq!(first_source_start_time(verses, &work), Some(12.5));
    }

    #[test]
    fn none_when_no_match_has_timing() {
        let work: Vec<(String, Option<f64>)> = vec![
            ("Ah, my good Lord of Winchester, I thank you.".into(), None),
        ];
        let verses = "Ah, my good Lord of Winchester, I thank you.";
        assert_eq!(first_source_start_time(verses, &work), None);
    }

    #[test]
    fn none_when_no_text_match() {
        let work: Vec<(String, Option<f64>)> = vec![("Unrelated line.".into(), Some(1.0))];
        let verses = "Ah, my good Lord of Winchester, I thank you.";
        assert_eq!(first_source_start_time(verses, &work), None);
    }

    #[test]
    fn seek_resolves_by_citation_not_first_text_match() {
        // verse "Let him shun castles" appears twice; citation points at the 2nd.
        // helper signature defined in Step 3.
        let lines = vec![
            (1i64,4i64,37i64, Some(2484.0)), // first occurrence
            (1,4,71, Some(2620.0)),          // re-read (citation target)
        ];
        let got = start_time_for_citation((1,4,71), &lines);
        assert_eq!(got, Some(2620.0));
    }

    #[test]
    fn start_time_for_citation_none_when_unresolved() {
        // Two None paths both fall through to the text fallback in the caller:
        let lines = vec![
            (1i64, 4i64, 37i64, Some(2484.0)),
            (1, 4, 71, None), // citation found, but no timestamp
        ];
        // citation not present in the work
        assert_eq!(start_time_for_citation((9, 9, 9), &lines), None);
        // citation present but its line carries no start time
        assert_eq!(start_time_for_citation((1, 4, 71), &lines), None);
    }

    /// Regression: an IPA-bearing source block must still resolve a seek time.
    /// The work line text (from the DB) has NO `/IPA/`, so the seek matcher
    /// must compare the STRIPPED (`display`) verse text — the raw `text` (with
    /// `/IPA/`) never matches and would silently break MPV seeking.
    #[test]
    fn ipa_stripped_display_matches_work_line() {
        let work: Vec<(String, Option<f64>)> =
            vec![("To be, or not to be".into(), Some(42.0))];

        // RAW block text (what TTS uses) carries inline IPA and does NOT match
        // the IPA-free work line — this is exactly the bug if `.text` is used.
        let raw = "To /biː/ or not to /biː/";
        assert_eq!(
            first_source_start_time(raw, &work),
            None,
            "raw IPA text must not match the IPA-free work line"
        );

        // DISPLAY block text (IPA stripped, what the seek matcher now passes)
        // DOES match and resolves the time.
        let display = "To be, or not to be";
        assert_eq!(first_source_start_time(display, &work), Some(42.0));
    }
}

#[cfg(test)]
mod should_play_after_synth_tests {
    use super::should_play_after_synth;
    use crate::ui::gloss_block::BlockKind;

    #[test]
    fn plays_when_the_cursor_never_moved() {
        assert!(should_play_after_synth(3usize, Some(3usize)));
    }

    /// THE reported case: the reader navigates away while ElevenLabs is still
    /// synthesizing, and the clip must not start on a paragraph they left.
    #[test]
    fn stays_silent_when_the_cursor_moved_away() {
        assert!(!should_play_after_synth(3usize, Some(4usize)));
    }

    /// The overlay closed (or the page emptied) during the await.
    #[test]
    fn stays_silent_when_there_is_no_cursor_block() {
        assert!(!should_play_after_synth(3usize, None::<usize>));
    }

    /// The gloss overlay numbers Source and Explication blocks independently,
    /// so index alone is not an identity — comparing only the index would play
    /// explication 2's audio while the cursor sits on source 2.
    #[test]
    fn kind_is_part_of_the_identity() {
        assert!(should_play_after_synth(
            (BlockKind::Explication, 2),
            Some((BlockKind::Explication, 2))
        ));
        assert!(!should_play_after_synth(
            (BlockKind::Explication, 2),
            Some((BlockKind::Source, 2))
        ));
    }
}

#[cfg(test)]
mod space_transport_tests {
    use super::{space_transport, SpaceTransport};

    /// The ordering trap. `TtsPlayer::is_playing` reports a LOADED clip, so it
    /// is true while the clip is paused. Testing `playing` before `paused`
    /// makes Space pause an already-paused clip forever — it can never resume.
    #[test]
    fn a_paused_clip_resumes_even_though_it_reports_playing() {
        assert_eq!(
            space_transport(true, true),
            SpaceTransport::Resume,
            "paused must win over playing, or Space never resumes"
        );
    }

    #[test]
    fn a_sounding_clip_pauses() {
        assert_eq!(space_transport(true, false), SpaceTransport::Pause);
    }

    /// Nothing loaded: Space falls back to starting the cursor block. This is
    /// the ONLY arm a headless cage run can reach (no audio device there), so
    /// the two above are covered here or nowhere.
    #[test]
    fn an_idle_player_starts_the_cursor_block() {
        assert_eq!(space_transport(false, false), SpaceTransport::Start);
    }

    /// Space must never be a no-op: every state maps to an action.
    #[test]
    fn every_state_maps_to_an_action() {
        for playing in [false, true] {
            for paused in [false, true] {
                let _: SpaceTransport = space_transport(playing, paused);
            }
        }
    }
}

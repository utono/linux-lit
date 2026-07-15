use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::ui::gloss_block::BlockKind;

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
    let (start_citation, source_text) = match &s.gloss_context {
        Some(ctx) => (ctx.start_citation.clone(), ctx.source_text.clone()),
        None => return false,
    };

    // start_citation is `ABBR.div1.div2.line_in_div`; the gloss strips any
    // `-Amb` suffix from the abbrev, so match on the numeric tail rather than
    // the full citation string.
    let target = crate::app::parse_citation(&start_citation);

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };

    // -Amb editions now render the canonical parity-numbered .txt (verified
    // 2026-06-25; base and -Amb share text_file and (div1,div2,line_in_div)).
    // Resolve by the citation tuple first — it is unique, so a repeated source
    // line can't land on the wrong occurrence. Text match is the citationless
    // (.txt-only) fallback.
    let by_citation = target.and_then(|t| {
        work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t)
    });
    let first_src = source_text.lines().next().map(str::trim).unwrap_or("");
    let start_idx = match by_citation.or_else(|| {
        if first_src.is_empty() {
            None
        } else {
            work.lines.iter().position(|l| l.text.trim() == first_src)
        }
    }) {
        Some(i) => i,
        None => return false,
    };
    let work_idx = work.lines[start_idx..]
        .iter()
        .position(|l| l.is_dialogue)
        .map(|off| start_idx + off)
        .unwrap_or(start_idx);

    // Resolve the work index to a buffer line through the line map.
    let buf_idx = if let Some(ref lm) = s.line_map {
        match lm.work_to_buffer.get(work_idx) {
            Some(&bi) => bi,
            None => return false,
        }
    } else {
        work_idx
    };

    // Use jump_to_line, not center-on-cursor: when the source passage opens a
    // scene (e.g. H8 Porter at (5,3,1)), naive centering lets the scene-break
    // clamp pull the spread back to the PREVIOUS scene, leaving the cursor
    // off-page. jump_to_line lands on the canonical spread for the line in
    // EReader mode (the same page paging through the work would show).
    crate::input::navigation::jump_to_line(s, buf_idx);
    true
}

pub(crate) fn navigate_gloss_passage(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    // Moving to a different passage invalidates any diff-highlight from a
    // custom-prompt rewrite on the passage we're leaving (Task 7).
    s.gloss_overlay.clear_rewrite_diff();

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
    );
}

pub(crate) fn navigate_gloss(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    // Moving to a different gloss within the passage invalidates any
    // diff-highlight from a custom-prompt rewrite on the gloss we're leaving
    // (Task 7).
    s.gloss_overlay.clear_rewrite_diff();
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
/// before the mutating `borrow_mut`. The tags are cloned into locals and the
/// buffer taken by value BEFORE building/applying, so no `&s` getter borrow is
/// held across the `set_from_text` write to `s.gloss_search`.
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
    let buffer = s.gloss_overlay.buffer();
    let tag = s.gloss_overlay.search_tag().clone();
    let ctag = s.gloss_overlay.search_current_tag().clone();
    let search = crate::input::overlay_search::set_from_text(&buffer, &tag, &ctag, pattern);
    if search.matches.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No matches", 2);
    } else if let Some((off, _)) = search.matches.first() {
        s.gloss_overlay.scroll_to_char_offset(*off);
    }
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
        // Revive the MRU pattern (post-Escape n/N).
        let Some(pat) = s.gloss_last_pattern.clone() else {
            return;
        };
        let buffer = s.gloss_overlay.buffer();
        let tag = s.gloss_overlay.search_tag().clone();
        let ctag = s.gloss_overlay.search_current_tag().clone();
        let search = crate::input::overlay_search::set_from_text(&buffer, &tag, &ctag, &pat);
        if search.matches.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, "No matches", 2);
            return;
        }
        s.gloss_search = Some(search);
    }
    let buffer = s.gloss_overlay.buffer();
    let tag = s.gloss_overlay.search_tag().clone();
    let ctag = s.gloss_overlay.search_current_tag().clone();
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
    if let Some(search) = s.gloss_search.as_ref() {
        crate::input::overlay_search::apply(&buffer, &tag, &ctag, search);
    }
    if let Some(off) = scroll_to {
        // Move the accent bar to the block holding the match, then scroll.
        s.gloss_overlay.cursor_to_char_offset(off);
        s.gloss_overlay.scroll_to_char_offset(off);
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
    let buffer = s.gloss_overlay.buffer();
    let tag = s.gloss_overlay.search_tag().clone();
    let ctag = s.gloss_overlay.search_current_tag().clone();
    crate::input::overlay_search::clear(&buffer, &tag, &ctag);
    s.gloss_search = None;
    true
}

pub(crate) fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
    let copied = {
        let s = state.borrow();
        s.gloss_list.get(s.gloss_index).map(|gloss| {
            // Copy the id prefaced with a label so a paste self-identifies.
            let copied = format!("Gloss ID: {}", gloss.gloss_id);
            let _ = std::process::Command::new("wl-copy").arg(&copied).spawn();
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

        let mut audio_rows = 0usize;
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::delete_gloss(&conn, gloss_id);
            audio_rows = crate::db::queries::delete_gloss_audio(&conn, gloss_id).unwrap_or(0);
        }
        // Count .mp3 files in the gloss's audio dir before removing it, so the
        // pill verifies the on-disk files actually went too.
        let mut mp3_files = 0usize;
        if let Some(ctx) = s.gloss_context.as_ref() {
            let dir = gloss_audio_dir(&ctx.work_abbrev, gloss_id);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                mp3_files = entries
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("mp3"))
                    .count();
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
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
/// `origin` (gloss vs journal) so `y` runs the right delete and returns to the
/// right mode; the dialog label names what will be deleted. No-op when there is
/// nothing to delete for that overlay. Mirrors `show_undo_confirmation`.
pub(crate) fn show_delete_confirmation(
    state_rc: &Rc<RefCell<AppState>>,
    origin: crate::app::InputMode,
) {
    // Resolve the label + bail with no dialog if there is nothing to delete.
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
            _ => return,
        }
    };

    let overlay_parent = {
        let s = state_rc.borrow();
        s.action_popup_widget.container.parent()
    };
    let overlay_parent = match overlay_parent {
        Some(p) => p.downcast::<gtk4::Overlay>().ok(),
        None => None,
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

    let title_text = if is_fix_ipa {
        "FIX IPA — word /IPA/  OR  word <hint>"
    } else if is_edit {
        "Edit gloss"
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
    state_rc.borrow_mut().gloss_prompt_mode = mode;
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
    // block's `<verse>` span. Operating on block.text directly was a no-op for
    // multi-line verse: block.text joins verse lines with '\n' (tags stripped),
    // but gloss_text separates them with `</verse>\n<verse>`, so block.text is
    // not a substring of gloss_text for any 2+ line block.
    let new_gloss_text = match crate::ui::gloss_block::replace_word_ipa_in_source_block(
        gloss_text,
        block_index,
        word,
        new_ipa,
    ) {
        Some(t) => t,
        None => {
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
            s.gloss_overlay.show_gloss_with_color(
                &source_text,
                &gloss_text,
                cw,
                h,
                Some(&root_color),
                &pairs,
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
                show_tts_toast(&state_for_result, "Could not get IPA");
            }
        }
    });
}

/// Render the gloss row at `new_idx` into the overlay. Shared by
/// `navigate_gloss` and `delete_current_gloss` (their render blocks were
/// byte-identical). Clones the strings that must outlive the `gloss_list`
/// borrow so `gloss_overlay` can be mutably borrowed in the same call.
fn render_gloss_row(s: &mut AppState, new_idx: usize) {
    let gloss = &s.gloss_list[new_idx];
    let gloss_start = gloss.start_citation.clone();
    let gloss_end = gloss.end_citation.clone();
    let gloss_text = gloss.gloss_text.clone();
    let ctx = s.gloss_context.as_ref().unwrap();
    let source_text = ctx.source_text.clone();
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(
        &source_text, &gloss_text, cw, h,
        Some(&s.theme.root_color), &pairs,
    );
    s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
    s.gloss_overlay.set_citation(&gloss_start, &gloss_end);
    recolor_cached_blocks(s);
    // Re-apply any active overlay search so a `/`-typed pattern keeps
    // highlighting across gloss/passage stepping (same locals-first borrow
    // discipline as journal's render_current: tags cloned + buffer taken into
    // locals BEFORE borrowing `s.gloss_search` mutably, so no `s.gloss_overlay`
    // getter borrow overlaps the mutable use — `reapply` re-collects spans
    // against the new buffer text and re-tags).
    if s.gloss_search.is_some() {
        let buffer = s.gloss_overlay.buffer();
        let tag = s.gloss_overlay.search_tag().clone();
        let ctag = s.gloss_overlay.search_current_tag().clone();
        let search = s.gloss_search.as_mut().unwrap();
        crate::input::overlay_search::reapply(&buffer, &tag, &ctag, search);
    }
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
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, full_gloss, cw, h,
        Some(&s.theme.root_color), &pairs,
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
            let _ = crate::db::journal::append_revision(
                &conn, "gloss", gloss_id, None, prev, model_for_db, prompt,
            );
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
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, full_gloss, cw, h,
        Some(&s.theme.root_color), &pairs,
    );
    s.gloss_overlay.set_position(gloss_index, s.gloss_list.len());
    s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
    s.gloss_index = gloss_index;
    if let Some((prev_rendered, _)) = diff {
        let new_rendered = s.gloss_overlay.buffer_text_for_diff();
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
        crate::input::navigation::show_chapter_toast_secs(&s, "Saved", 2);
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
            crate::input::navigation::show_chapter_toast_secs(&s, "Saved (:q to exit)", 2);
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

/// `R` in the gloss vim editor: leave the editor and open the existing ask-Claude
/// rewrite (edit) card so an AI rewrite is reachable without switching surfaces.
/// Mirrors journal `vim_open_rewrite`. The hand-edits in the buffer are discarded
/// (the rewrite operates on the stored gloss); `R` is "ask AI", distinct from
/// `:w` "save my edit".
pub(crate) fn vim_open_rewrite(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    {
        let mut s = state.borrow_mut();
        s.gloss_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }
    // Open the existing ask-Claude edit dialog (GlossPromptMode::Edit).
    begin_rewrite(state);
}

/// `R` in the gloss overlay (read view OR via the vim editor's `R`): open the
/// ask-Claude rewrite (edit) prompt for the displayed gloss. Directly reachable
/// from the read view — entering the `e` editor first is unnecessary (mirrors
/// journal `begin_rewrite`). Opens in INSERT: a rewrite instruction is always
/// typed fresh, so skip vim-NORMAL (fed through the engine so the mirror and
/// `-- INSERT --` hint stay truthful).
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
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &original, cw, h,
        Some(&s.theme.root_color), &pairs,
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

    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(&ctx.source_text, text, cw, h, Some(&s.theme.root_color), &pairs);
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
    // `<speaker>`/`<verse>` formatting as the gloss result), not a bare
    // "Glossing…" label.
    {
        let s = state_rc.borrow();
        let (cw, h) = crate::app::layout::overlay_card_size(&s);
        s.gloss_overlay.show_glossing(&ctx.passage_doc(), cw, h, Some(&s.theme.root_color));
    }

    let prompt_owned = prompt.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
            (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
            (crate::gloss::READER_GLOSS_QUESTION_PROMPT.as_str(), msg, "reader-gloss")
        }
        _ => {
            let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
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
    // Capture the on-screen RENDERED gloss text before it is overwritten, so the
    // post-rewrite diff highlight can compare old-rendered vs new-rendered.
    let prev_rendered = state_rc.borrow().gloss_overlay.buffer_text_for_diff();

    // Show the passage being reglossed on the loading card (same single-column
    // `<speaker>`/`<verse>` formatting as the gloss result), not a bare
    // "Glossing…" label.
    {
        let s = state_rc.borrow();
        let (cw, h) = crate::app::layout::overlay_card_size(&s);
        s.gloss_overlay.show_glossing(&ctx.passage_doc(), cw, h, Some(&s.theme.root_color));
    }

    let pasted_owned = pasted_lines.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = match ctx.gloss_type.as_str() {
        "inner-monologue" => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::INNER_MONOLOGUE_EDIT_PROMPT.as_str(), msg, "inner-monologue")
        }
        "reader-gloss" => {
            let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
            (crate::gloss::READER_GLOSS_EDIT_PROMPT.as_str(), msg, "reader-gloss")
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
/// stop the rodio TTS player. Called on every cursor move (j/k/gg/G) so moving
/// off a playing block silences it before the user starts the next one. Both
/// calls are harmless no-ops when nothing is playing.
pub(crate) fn stop_all_gloss_audio(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    s.tts.stop();
    let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
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
/// `active_voice`), else the age-aware default by kind (verse->OP, prose->plain).
/// Shared by `play_block_tts` and the cached-audio recolor check so both look at
/// the same mp3. Mirrors the inline logic at the former call site.
pub(crate) fn gloss_block_voice(
    conn: &rusqlite::Connection,
    gloss_id: i64,
    work_abbrev: &str,
    speaker: &str,
    kind: BlockKind,
    active_voice: usize,
) -> (String, String) {
    let is_verse = kind == BlockKind::Source;
    let voices = crate::db::queries::get_gloss_voices(conn, gloss_id);
    if !voices.is_empty() {
        let i = active_voice.min(voices.len() - 1);
        (voices[i].0.clone(), voices[i].1.clone())
    } else {
        crate::db::queries::resolve_default_voice(conn, work_abbrev, speaker, is_verse)
    }
}

/// Accent color for cached (already-synthesized) gloss/synopsis blocks:
/// deep slate blue — deliberately NOT the theme's `cursor_bg` (a love-red that
/// clashed against the cream paper and read like an error). A flat constant for
/// now: no theme exposes a dedicated slate accent role, so promote this to a
/// `Theme` field if/when other palettes need their own cached accent.
const CACHED_BLOCK_ACCENT: &str = "#2d5570";

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
        let accent = CACHED_BLOCK_ACCENT.to_string();
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
        return;
    }

    // Synopsis mode. Key by the BASE abbrev (matching `play_synopsis_block` /
    // `synth_all_synopsis_blocks`) so synopsis audio is shared across editions
    // (`2H6`/`2H6-Amb`) the same way the synopsis TEXT is — `synopsis_cache` is
    // itself loaded under the base abbrev, so the audio key must match it.
    let (div1, div2) = s.synopsis_overlay_scene;
    let work_abbrev = match s.current_work.as_ref() {
        Some(w) => w.canonical_abbrev.clone(),
        None => return,
    };
    let (voice_id, _mid) =
        crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
    let voice_id = voice_id.to_string();
    let accent = CACHED_BLOCK_ACCENT.to_string();
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
}

/// Borrow `state` and recolor. For async synth-completion sites that hold an
/// `Rc<RefCell<AppState>>` and must not already hold a borrow.
pub(crate) fn recolor_cached_blocks_rc(state: &Rc<RefCell<AppState>>) {
    recolor_cached_blocks(&state.borrow());
}

/// Color the journal Q&A overlay's current-page paragraphs whose TTS MP3 is
/// cached, with the same accent the gloss/synopsis overlays use. Keyed by the
/// page's `journal_entries.id` + the paragraph's FULL index + the fixed
/// plain-prose voice (the journal always synthesizes in that voice). Called on
/// every page render and after a synth completes. No-op when not in the journal
/// overlay or no current page.
pub(crate) fn recolor_journal_cached_blocks(s: &AppState) {
    if s.input_mode != crate::app::InputMode::JournalOverlay {
        return;
    }
    let entry_id = match s.journal.pages.get(s.journal.page_index) {
        Some(p) => p.id,
        None => return,
    };
    let (voice_id, _mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
    let voice_id = voice_id.to_string();
    let accent = CACHED_BLOCK_ACCENT.to_string();
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
        // active one (gloss_active_voice index, clamped). Else fall back to the
        // age-aware character default (verse->OP, prose->plain).
        let (vid, mid): (String, String) = match crate::db::queries::open_db() {
            Ok(conn) => gloss_block_voice(
                &conn, gloss_id, &work_abbrev, &speaker, kind, s.gloss_active_voice,
            ),
            Err(_) => {
                let (v, m) =
                    crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, is_verse);
                (v.to_string(), m.to_string())
            }
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
        // Explication prose is always read by Eleanor (see resolve_default_voice:
        // "All prose is read by Eleanor"). Single-block synth resolves the same
        // voice via gloss_block_voice, and the cached-audio recolor check looks
        // under Eleanor — so the batch MUST cache under Eleanor too, or its
        // mp3s land under a different voice id and neither playback-cache-hit nor
        // the recolor existence check will find them.
        let (vid, mid) = (
            crate::elevenlabs::DEFAULT_FEMALE_VOICE_ID,
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

/// Shift+Space (synopsis overlay): synthesize ALL synopsis paragraphs of the
/// open scene to cached MP3s in the fixed plain-prose voice. Cache-only.
/// Persistent toast; stop on first error. Re-entrant-safe via tts_batch_running.
pub(crate) fn synth_all_synopsis_blocks(state_rc: &Rc<RefCell<AppState>>) {
    if state_rc.borrow().tts_batch_running.get() {
        return;
    }
    let (work_abbrev, div1, div2, blocks, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let synopsis = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        let prose: Vec<(i32, String)> = crate::ui::gloss_block::synopsis_blocks(&synopsis)
            .into_iter()
            .map(|b| (b.index, b.text))
            .collect();
        if prose.is_empty() {
            return;
        }
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
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
                let _ = crate::db::queries::ensure_synopsis_audio_table(&conn);
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
        let (div1, div2) = s.synopsis_overlay_scene;
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
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
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
            let _ = crate::db::queries::ensure_synopsis_audio_table(&conn);
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
// like the synopsis overlay, so its TTS mirrors `play_synopsis_block`: the fixed
// plain-prose voice, the Alice paywall fallback, and a per-paragraph MP3 cache.
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
        let entry_id = match s.journal.pages.get(s.journal.page_index) {
            Some(p) => p.id,
            None => return,
        };
        let text = match s.journal_overlay.current_block_text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return,
        };
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        // Journal Q&A is plain English prose -> the fixed plain-prose voice.
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
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
            let _ = crate::db::queries::ensure_journal_audio_table(&conn);
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
        state_for_result.borrow().tts.play_file(&path);
        crate::log_fmt!(
            "JOURNAL TTS: synthesized entry {} para {} (voice {})",
            entry_id, index, used_voice
        );
    });
}

/// Space/Tab in the journal Q&A overlay: if TTS is playing, stop it; otherwise
/// play the cursor paragraph's TTS (cache hit plays the stored MP3, miss
/// synthesizes then plays). Mirrors `read_current_synopsis_block`.
pub(crate) fn read_current_journal_block(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        if s.tts.is_playing() {
            s.tts.stop();
            return;
        }
    }
    let index = match state_rc.borrow().journal_overlay.current_block_index() {
        Some(i) => i as i32,
        None => return,
    };
    play_journal_block(state_rc, index);
}

/// `s` in the journal Q&A overlay: ALWAYS begin playback of the cursor's
/// paragraph from its start (no pause-toggle). Stops any current audio first,
/// then plays the paragraph's TTS. Mirrors `begin_current_synopsis_block`.
pub(crate) fn begin_current_journal_block(state_rc: &Rc<RefCell<AppState>>) {
    stop_all_gloss_audio(state_rc);
    let index = match state_rc.borrow().journal_overlay.current_block_index() {
        Some(i) => i as i32,
        None => return,
    };
    play_journal_block(state_rc, index);
}

/// The cursor journal block's CACHED TTS MP3, if one exists on disk (selected
/// voice first, then the Alice paywall-fallback voice). Never synthesizes.
fn find_cached_journal_block_audio(
    state_rc: &Rc<RefCell<AppState>>,
    index: i32,
) -> Option<std::path::PathBuf> {
    let entry_id = {
        let s = state_rc.borrow();
        s.journal.pages.get(s.journal.page_index).map(|p| p.id)?
    };
    let (vid, _) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
    let conn = crate::db::queries::open_db().ok()?;
    for vid_try in [vid, crate::elevenlabs::ALICE_VOICE_ID] {
        if let Ok(Some(path)) =
            crate::db::queries::find_journal_audio(&conn, entry_id, index as i64, vid_try)
        {
            let p = std::path::PathBuf::from(&path);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// `a` in the journal Q&A overlay: toggle play/pause of the block's TTS.
/// A playing clip pauses in place; a paused clip resumes; nothing loaded ->
/// start the cursor block ONLY if its TTS MP3 is already cached (no
/// synthesis — Space owns that). Toasts when the block has no cached audio.
pub(crate) fn toggle_pause_current_journal_block(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        if s.tts.is_paused() {
            s.tts.resume();
            return;
        }
        if s.tts.is_playing() {
            s.tts.pause();
            return;
        }
    }
    let index = match state_rc.borrow().journal_overlay.current_block_index() {
        Some(i) => i as i32,
        None => return,
    };
    match find_cached_journal_block_audio(state_rc, index) {
        Some(path) => {
            stop_all_gloss_audio(state_rc);
            state_rc.borrow().tts.play_file(&path);
        }
        None => show_tts_toast(state_rc, "No TTS audio for this block (Space synthesizes)"),
    }
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

/// Gloss-overlay `r`: play/stop the Source block's synthesized MP3 in the
/// gloss's ACTIVE voice (or the age-aware default voice when the gloss has no
/// associated voices) — the ElevenLabs/`TtsPlayer` channel, NOT the MPV
/// recording (`space`/`a`). Toggle: if the TTS sink is playing, stop it (MPV
/// stays paused; the user resumes the recording with `space`); else pause MPV
/// and play (cache hit -> play; miss -> synthesize then play). No picker. No-op
/// (toast) off a Source block.
pub(crate) fn toggle_source_tts(state_rc: &Rc<RefCell<AppState>>) {
    // Stop-if-playing FIRST (like `read_current_block`), before the Source gate:
    // a press while the synthesized audio plays always stops it. MPV stays paused.
    if state_rc.borrow().tts.is_playing() {
        state_rc.borrow().tts.stop();
        return;
    }
    let index = match source_block_index(state_rc) {
        Some(i) => i,
        None => return,
    };
    play_source_tts_pausing_mpv(state_rc, index);
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
fn show_persistent_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    let s = state_rc.borrow();
    s.chapter_toast.set_text(msg);
    s.chapter_toast.set_visible(true);
}

/// Hide the toast pill immediately (used to dismiss the persistent "Synthesizing…"
/// toast the moment audio starts playing).
fn hide_tts_toast(state_rc: &Rc<RefCell<AppState>>) {
    state_rc.borrow().chapter_toast.set_visible(false);
}

/// True when the cursor's `(div1, div2, line_in_div)` triple falls within the
/// inclusive `[start, end]` citation range of a glossed passage. Rust tuple
/// ordering compares lexicographically, which matches citation ordering.
fn passage_covers(start: (i64, i64, i64), end: (i64, i64, i64), cur: (i64, i64, i64)) -> bool {
    start <= cur && cur <= end
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
) {
    let types: Vec<&str> = all_glosses.iter().map(|g| g.gloss_type.as_str()).collect();
    let idx = start_gloss_idx(&types, desired_type);

    let work_title = s
        .current_work
        .as_ref()
        .map(|w| w.title.clone())
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
    };

    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let source_lines: Vec<(String, i64)> = Vec::new();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text,
        &all_glosses[idx].gloss_text,
        cw,
        h,
        Some(&s.theme.root_color),
        &source_lines,
    );
    s.gloss_overlay.set_position(idx, all_glosses.len());
    // Footer cites the DISPLAYED gloss's own passage span, not the group-wide
    // ctx (glosses sharing a start_citation may span to different end_citations).
    s.gloss_overlay
        .set_citation(&all_glosses[idx].start_citation, &all_glosses[idx].end_citation);

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

    // Stamp the most-recent reference from the gloss now displayed.
    s.record_last_gloss(&shown_type);
}

/// Close the gloss overlay and return to the reader, landing the cursor on the
/// glossed passage's source line (falling back to the pre-open page). This is
/// the overlay's Escape close — the only close key under the Escape-only
/// policy; `n`/Ctrl+g/Ctrl+j are consumed no-ops in this overlay.
pub(crate) fn close_gloss_to_reader(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Closing the overlay must not leave a stale diff-highlight for the next
    // open session (Task 7).
    s.gloss_overlay.clear_rewrite_diff();
    s.tts.stop();
    s.gloss_overlay.hide();
    s.gloss_opened_from_picker = false;
    // Drop any overlay search + MRU so neither leaks into the next gloss overlay
    // session (the buffer is re-rendered on the next open regardless). Mirrors
    // the journal overlay's close-branch cleanup.
    s.gloss_search = None;
    s.gloss_last_pattern = None;
    crate::app::return_to_reader_mode(&mut s);
    let jumped = jump_to_gloss_source_start(&mut s);
    let saved = s.gloss_return_pos.take();
    if !jumped {
        crate::app::restore_saved_position_resnap(&mut s, saved);
    }
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
    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];

    // Resolve the cursor line -> its (work abbrev, (div1, div2, line_in_div)).
    let (work_abbrev, cur_triple) = {
        let s = state.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => {
                drop(s);
                show_tts_toast(state, "No gloss on this line");
                return;
            }
        };
        let wl = match s.work_line_for_buffer(s.current_line) {
            Some(wl) => wl,
            None => {
                drop(s);
                show_tts_toast(state, "No gloss on this line");
                return;
            }
        };
        let line = match work.lines.get(wl) {
            Some(l) => l,
            None => {
                drop(s);
                show_tts_toast(state, "No gloss on this line");
                return;
            }
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
        Err(_) => {
            show_tts_toast(state, "No gloss on this line");
            return;
        }
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
        None => {
            show_tts_toast(state, "No gloss on this line");
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
        show_tts_toast(state, "No gloss on this line");
        return;
    }

    // All resolution done; mutate state and open the overlay under one borrow.
    let mut s = state.borrow_mut();
    // Remember the reader page so Escape returns here.
    s.gloss_return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    // Opened from the reader cursor, not the picker (from_picker = false): Escape
    // uses the saved reader page, not the picker return path.
    open_gloss_overlay(&mut s, passages, passage_index, passage, all_glosses, false, None);
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
    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];

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
    s.gloss_return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    open_gloss_overlay(
        &mut s,
        passages,
        passage_index,
        passage,
        all_glosses,
        false,
        Some(&desired_type),
    );
}

/// Close the stacked gloss add/edit input card and return focus to the gloss.
/// The reader stays in `InputMode::GlossOverlay` throughout (the card lives
/// inside the gloss overlay, like the synopsis ask card).
pub(crate) fn close_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().gloss_overlay.close_ask_card();
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

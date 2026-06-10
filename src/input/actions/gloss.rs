use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;
use crate::ui::gloss_overlay::BlockKind;

/// Jump the reader cursor to the first dialogue line of the glossed passage's
/// source text (the line `start_citation` points at, advanced to the first
/// `is_dialogue` line at or after it). Returns true if it jumped.
///
/// Falls back to `false` if the current gloss context, work, or matching line
/// can't be resolved, so the caller can restore the saved page instead.
pub(crate) fn jump_to_gloss_source_start(s: &mut AppState) -> bool {
    let start_citation = match &s.gloss_context {
        Some(ctx) => ctx.start_citation.clone(),
        None => return false,
    };

    // start_citation is `ABBR.div1.div2.line_in_div`; the gloss strips any
    // `-Amb` suffix from the abbrev, so match on the numeric tail rather than
    // the full citation string to stay robust across Ambrose works.
    let cite_tail = |cite: &str| -> Option<(i64, i64, i64)> {
        let mut parts = cite.rsplitn(4, '.');
        let lid = parts.next()?.parse().ok()?;
        let d2 = parts.next()?.parse().ok()?;
        let d1 = parts.next()?.parse().ok()?;
        Some((d1, d2, lid))
    };
    let target = match cite_tail(&start_citation) {
        Some(t) => t,
        None => return false,
    };

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    // First work-line whose (div1,div2,line_in_div) matches the citation, then
    // the first dialogue line at or after it.
    let start_idx = match work
        .lines
        .iter()
        .position(|l| (l.div1, l.div2, l.line_in_div) == target)
    {
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

    let work_abbrev = match &s.gloss_context {
        Some(ctx) => ctx.work_abbrev.clone(),
        None => return,
    };

    if s.gloss_passages.is_empty() {
        if let Ok(conn) = crate::db::queries::open_db() {
            s.gloss_passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev, &["teacher-generic", "inner-monologue"])
                .unwrap_or_default();
        }
        if s.gloss_passages.is_empty() {
            return;
        }
        if let Some(ctx) = &s.gloss_context {
            s.gloss_passage_index = s.gloss_passages.iter()
                .position(|p| p.start_citation == ctx.start_citation && p.end_citation == ctx.end_citation)
                .unwrap_or(0);
        }
    }

    let len = s.gloss_passages.len();
    // Clamp at the ends rather than wrapping: Alt+p stops at the work's earliest
    // gloss (index 0), Alt+n stops at its last (index len-1).
    let target = s.gloss_passage_index as i32 + delta;
    let new_idx = target.clamp(0, len as i32 - 1) as usize;
    if new_idx == s.gloss_passage_index {
        return;
    }
    s.gloss_passage_index = new_idx;

    let passage = s.gloss_passages[new_idx].clone();

    let all_glosses = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_all_glosses(
                &conn, &passage.work_abbrev, &passage.start_citation, &passage.end_citation,
                &["teacher-generic", "inner-monologue"],
            ).ok()
        })
        .unwrap_or_default();

    if all_glosses.is_empty() {
        return;
    }

    let source_lines: Vec<(String, i64)> = Vec::new();

    let gloss_type = all_glosses[0].gloss_type.clone();
    let work_title = s.current_work.as_ref().map(|w| w.title.clone()).unwrap_or_default();
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
        gloss_type,
    };

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let gloss_text = &all_glosses[0].gloss_text;
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, gloss_text, cw, h,
        Some(&s.theme.root_color), &source_lines,
    );
    s.gloss_overlay.set_position(0, all_glosses.len());
    s.gloss_list = all_glosses;
    s.gloss_index = 0;
    s.gloss_active_voice = 0;
    s.gloss_context = Some(ctx);
}

pub(crate) fn navigate_gloss(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
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
    let gloss = &s.gloss_list[new_idx];
    let ctx = s.gloss_context.as_ref().unwrap();
    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &gloss.gloss_text, cw, h,
        Some(&s.theme.root_color), &pairs,
    );
    s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
}

pub(crate) fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    if let Some(gloss) = s.gloss_list.get(s.gloss_index) {
        let id = gloss.gloss_id.to_string();
        let _ = std::process::Command::new("wl-copy")
            .arg(&id)
            .spawn();
        crate::logging::log(&format!("GLOSS: copied id {} to clipboard", id));
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
            let gloss = &s.gloss_list[new_idx];
            let ctx = s.gloss_context.as_ref().unwrap();
            let cw = s.content_hbox.width();
            let h = s.content_hbox.height();
            let pairs = ctx.source_line_pairs();
            s.gloss_overlay.show_gloss_with_color(
                &ctx.source_text, &gloss.gloss_text, cw, h,
                Some(&s.theme.root_color), &pairs,
            );
            s.gloss_overlay.set_position(new_idx, s.gloss_list.len());
        }
    }
    // Phase 2: verification pill (borrow released above). Shown whether or not
    // the overlay closed, so the user always gets confirmation.
    show_tts_toast(state_rc, &toast_msg);
}

pub(crate) fn show_delete_confirmation(state_rc: &Rc<RefCell<AppState>>) {
    let gloss_id = {
        let s = state_rc.borrow();
        match s.gloss_list.get(s.gloss_index) {
            Some(g) => g.gloss_id,
            None => return,
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

    let label = gtk4::Label::new(Some(&format!("Delete gloss {}?", gloss_id)));
    label.add_css_class("amend-title");
    label.set_halign(gtk4::Align::Start);
    container.append(&label);

    let hint = gtk4::Label::new(Some("y = confirm  \u{00b7}  Esc = cancel"));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    let mut s = state_rc.borrow_mut();
    s.delete_confirm_container = Some(container.downgrade());
    s.delete_confirm_overlay = Some(overlay_parent.downgrade());
    s.input_mode = crate::app::InputMode::DeleteConfirm;
}

pub(crate) fn close_delete_confirmation(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (s.delete_confirm_container.take(), s.delete_confirm_overlay.take()) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    s.input_mode = crate::app::InputMode::GlossOverlay;
}

fn show_prompt_dialog(state_rc: &Rc<RefCell<AppState>>, mode: crate::app::GlossPromptMode) {
    let (is_inner_monologue, is_edit) = {
        let s = state_rc.borrow();
        let im = s.gloss_context.as_ref()
            .map(|ctx| ctx.gloss_type == "inner-monologue")
            .unwrap_or(false);
        (im, mode == crate::app::GlossPromptMode::Edit)
    };
    let is_fix_ipa = mode == crate::app::GlossPromptMode::FixIpa;

    let title_text = if is_fix_ipa {
        "FIX IPA — word /IPA/  OR  word <hint>"
    } else if is_edit {
        "EDIT GLOSS — PASTE SUBTEXT LINES"
    } else if is_inner_monologue {
        "INNER MONOLOGUE PASSAGE"
    } else {
        "GLOSS PROMPT"
    };
    let hint_text = if is_fix_ipa {
        "e.g. `daily /\u{02c8}de\u{026a}li/` or `daily hard a`  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel"
    } else if is_edit {
        "Paste lines for subtext  \u{00b7}  Tab switch  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel"
    } else if is_inner_monologue {
        "Paste lines from another work  \u{00b7}  Tab switch  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel"
    } else {
        "Tab switch  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel"
    };

    // Stack the input as a card below the open gloss (same widget the synopsis
    // "ask" flow uses) instead of a separate floating dialog. The gloss card
    // stays visible above it; `gloss_prompt_mode` routes the eventual submit.
    state_rc.borrow_mut().gloss_prompt_mode = mode;
    state_rc.borrow().gloss_overlay.open_ask_card_with(title_text, hint_text);
}

pub(crate) fn show_amend_dialog(state_rc: &Rc<RefCell<AppState>>) {
    show_prompt_dialog(state_rc, crate::app::GlossPromptMode::Add);
}

pub(crate) fn show_edit_dialog(state_rc: &Rc<RefCell<AppState>>) {
    show_prompt_dialog(state_rc, crate::app::GlossPromptMode::Edit);
}

/// Gloss-overlay `i`: open the fix-IPA input card for the cursor's source verse.
/// No-op (toast) off a source block.
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
                        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss_text);
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
    if crate::ui::gloss_overlay::contains_ipa_span(&rest) {
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
    let new_gloss_text = match crate::ui::gloss_overlay::replace_word_ipa_in_source_block(
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
    let removed: Vec<String> = match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            let _ = crate::db::queries::update_gloss(&conn, gloss_id, &new_gloss_text);
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
            let cw = s.content_hbox.width();
            let h = s.content_hbox.height();
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
                if !crate::ui::gloss_overlay::contains_ipa_span(&reply) {
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

pub(crate) fn add_gloss(state_rc: &Rc<RefCell<AppState>>, prompt: &str) {
    let (ctx, model, tokio_handle) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone())
    };

    state_rc.borrow().gloss_overlay.show_loading();

    let prompt_owned = prompt.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = if is_inner_monologue {
        let msg = crate::gloss::build_inner_monologue_add_message(&ctx, &prompt_owned);
        (crate::gloss::INNER_MONOLOGUE_ADD_PROMPT, msg, "inner-monologue")
    } else {
        let msg = crate::gloss::build_user_message(&ctx, Some(&prompt_owned), None);
        (crate::gloss::USER_QUESTION_PROMPT, msg, "teacher-generic")
    };

    let state_for_result = Rc::clone(state_rc);
    let gloss_type_owned = gloss_type_str.to_string();

    glib::spawn_future_local(async move {
        let system_prompt = system_prompt.to_string();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &system_prompt, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let verified_text = if is_inner_monologue {
                    crate::gloss::verify_echo_citations(&gloss_text, &ctx.work_abbrev)
                } else {
                    gloss_text.clone()
                };
                let full_gloss = if is_inner_monologue {
                    format!("<gloss>Inner voice from:</gloss>\n\n{}\n\n{}", prompt_owned, verified_text)
                } else {
                    format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, verified_text)
                };
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &full_gloss,
                        &gloss_type_owned,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                            &[gloss_type_owned.as_str()],
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &full_gloss, cw, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                s.gloss_active_voice = 0;
                crate::logging::log(&format!("GLOSS: added new {} gloss", gloss_type_owned));
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: add error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
}

pub(crate) fn edit_gloss(state_rc: &Rc<RefCell<AppState>>, pasted_lines: &str) {
    let (ctx, existing_gloss_text, model, tokio_handle) = {
        let state = state_rc.borrow();
        let ctx = match &state.gloss_context {
            Some(c) => c.clone(),
            None => return,
        };
        let existing = state.gloss_list.get(state.gloss_index)
            .map(|g| g.gloss_text.clone())
            .unwrap_or_default();
        (ctx, existing, state.config.claude_model.clone(), state.tokio_handle.clone())
    };

    state_rc.borrow().gloss_overlay.show_loading();

    let pasted_owned = pasted_lines.to_string();
    let is_inner_monologue = ctx.gloss_type == "inner-monologue";

    let (system_prompt, user_msg, gloss_type_str) = if is_inner_monologue {
        let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
        (crate::gloss::INNER_MONOLOGUE_EDIT_PROMPT, msg, "inner-monologue")
    } else {
        let msg = crate::gloss::build_edit_gloss_message(&ctx, &existing_gloss_text, &pasted_owned);
        (crate::gloss::EDIT_GLOSS_PROMPT, msg, "teacher-generic")
    };

    let state_for_result = Rc::clone(state_rc);
    let gloss_type_owned = gloss_type_str.to_string();

    glib::spawn_future_local(async move {
        let system_prompt = system_prompt.to_string();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &system_prompt, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let verified_text = if is_inner_monologue {
                    crate::gloss::verify_echo_citations(&gloss_text, &ctx.work_abbrev)
                } else {
                    gloss_text.clone()
                };
                let full_gloss = if is_inner_monologue {
                    format!("<gloss>Re-glossed with:</gloss>\n\n{}\n\n{}", pasted_owned, verified_text)
                } else {
                    format!("<gloss>Edit context:</gloss>\n\n{}\n\n{}", pasted_owned, verified_text)
                };
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &full_gloss,
                        &gloss_type_owned,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                            &[gloss_type_owned.as_str()],
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &full_gloss, cw, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                s.gloss_active_voice = 0;
                crate::logging::log(&format!("GLOSS: edited {} gloss (added new)", gloss_type_owned));
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: edit error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
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

/// Re-apply accent coloring to every block of the currently-open gloss OR
/// synopsis overlay whose mp3 is cached. Detects mode from which context is set:
/// a live `gloss_context` + non-empty `gloss_list` means gloss mode; otherwise
/// fall through to synopsis mode keyed by `synopsis_overlay_scene`. UI-only side
/// effect; DB errors degrade to "uncached" (no color). Call with `s` already
/// borrowed (the display sites) — see `recolor_cached_blocks_rc` for the
/// borrow-and-call wrapper used by async synth completions.
pub(crate) fn recolor_cached_blocks(s: &AppState) {
    // Gloss mode.
    if let (Some(ctx), Some(gloss)) =
        (s.gloss_context.as_ref(), s.gloss_list.get(s.gloss_index))
    {
        let gloss_id = gloss.gloss_id;
        let work_abbrev = ctx.work_abbrev.clone();
        let speaker = ctx.speaker.clone();
        let active = s.gloss_active_voice;
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => return,
        };
        s.gloss_overlay.color_audio_blocks(move |kind, index| {
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
                        return true;
                    }
                }
            }
            false
        });
        return;
    }

    // Synopsis mode. Key by the work's plain abbrev — matching
    // `play_synopsis_block` / `synth_all_synopsis_blocks`, which write/read
    // synopsis audio under `w.abbrev` (NOT base-normalized) — so the existence
    // check finds the same files those synth paths wrote.
    let (div1, div2) = s.synopsis_overlay_scene;
    let work_abbrev = match s.current_work.as_ref() {
        Some(w) => w.abbrev.clone(),
        None => return,
    };
    let (voice_id, _mid) =
        crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
    let voice_id = voice_id.to_string();
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    s.gloss_overlay.color_audio_blocks(move |_kind, index| {
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
        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
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
    let text = crate::ui::gloss_overlay::ipa_for_tts(&text);

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
            crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text)
                .into_iter()
                .filter(|b| b.kind == BlockKind::Explication)
                .map(|b| (b.index, b.text))
                .collect();
        if prose.is_empty() {
            return;
        }
        let (vid, mid) = crate::elevenlabs::voice_for(crate::elevenlabs::Gender::Unknown, false);
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
            let tts_text = crate::ui::gloss_overlay::ipa_for_tts(raw);
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
            Some(w) => w.abbrev.clone(),
            None => return,
        };
        let prose: Vec<(i32, String)> = crate::ui::gloss_overlay::synopsis_blocks(&synopsis)
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
            let tts_text = crate::ui::gloss_overlay::ipa_for_tts(raw);
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
            Some(w) => w.abbrev.clone(),
            None => return,
        };
        let text = match crate::ui::gloss_overlay::synopsis_blocks(&synopsis)
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
    let tts_text = crate::ui::gloss_overlay::ipa_for_tts(&text);

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
    let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
    let block = blocks
        .iter()
        .find(|b| b.kind == BlockKind::Source && b.index == index)?;
    let work = s.current_work.as_ref()?;
    let work_pairs: Vec<(String, Option<f64>)> = work
        .lines
        .iter()
        .map(|l| (l.text.clone(), l.timestamp.map(|t| t.start)))
        .collect();
    // Match on `display` (IPA-stripped) text: work line text has no `/IPA/`,
    // so the raw `block.text` would never match an IPA-bearing verse line.
    // Seek a `SEEK_PREROLL` (0.2s) before the line start, matching every other
    // line-seek in the app (search / concordance / echoes), so `a`/`space` begin
    // just ahead of the first word rather than clipping its onset.
    let start = first_source_start_time(&block.display, &work_pairs)?;
    Some((start - crate::input::navigation::SEEK_PREROLL).max(0.0))
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

/// Toast helper exposed for the voice-picker confirm path (settings.rs) to
/// report gloss-voice association from the gloss overlay.
pub(crate) fn voice_picker_toast(state_rc: &Rc<RefCell<AppState>>, verb: &str, name: &str) {
    show_tts_toast(state_rc, &format!("{}: {}", verb, name));
}

fn show_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    let s = state_rc.borrow();
    s.chapter_toast.set_text(msg);
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
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

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == crate::app::InputMode::GlossOverlay {
        let mut s = state.borrow_mut();
        s.tts.stop();
        s.gloss_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        // Restore the page the user was on before toggling the gloss open.
        if let Some((line, top)) = s.gloss_return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
            crate::input::scroll::resnap_page(&mut s);
            crate::input::highlight::update_highlight(&mut s);
        }
        return;
    }

    const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue"];

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
            crate::app::base_work_abbrev(&work.abbrev).to_string(),
            (line.div1, line.div2, line.line_in_div),
        )
    };

    // Decode the tail of a citation (`ABBR.div1.div2.line_in_div`) into a triple.
    let cite_tail = |cite: &str| -> Option<(i64, i64, i64)> {
        let mut parts = cite.rsplitn(4, '.');
        let lid = parts.next()?.parse().ok()?;
        let d2 = parts.next()?.parse().ok()?;
        let d1 = parts.next()?.parse().ok()?;
        Some((d1, d2, lid))
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
        match (cite_tail(&p.start_citation), cite_tail(&p.end_citation)) {
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

    let all_glosses = crate::db::queries::find_all_glosses(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        &passage.end_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();

    if all_glosses.is_empty() {
        show_tts_toast(state, "No gloss on this line");
        return;
    }

    // All resolution done; mutate state and open the overlay under one borrow.
    let mut s = state.borrow_mut();
    let work_title = s.current_work.as_ref().map(|w| w.title.clone()).unwrap_or_default();
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
        gloss_type: all_glosses[0].gloss_type.clone(),
    };

    // Remember the reader page so Escape returns here.
    s.gloss_return_pos = Some((s.current_line, s.page_top_line));

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let source_lines: Vec<(String, i64)> = Vec::new();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &all_glosses[0].gloss_text, cw, h,
        Some(&s.theme.root_color), &source_lines,
    );
    s.gloss_overlay.set_position(0, all_glosses.len());

    s.gloss_passages = passages;
    s.gloss_passage_index = passage_index;
    s.gloss_list = all_glosses;
    s.gloss_index = 0;
    s.gloss_active_voice = 0;
    s.gloss_context = Some(ctx);
    // Opened from the reader cursor, not the picker, so Escape uses the
    // saved reader page (gloss_return_pos), not the picker return path.
    s.gloss_opened_from_picker = false;
    s.input_mode = crate::app::InputMode::GlossOverlay;
}

/// Close the stacked gloss add/edit input card and return focus to the gloss.
/// The reader stays in `InputMode::GlossOverlay` throughout (the card lives
/// inside the gloss overlay, like the synopsis ask card).
pub(crate) fn close_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().gloss_overlay.close_ask_card();
}

/// Submit the stacked gloss input card: read its text, close it, and route to
/// `add_gloss` / `edit_gloss` by the active prompt mode. No-op on empty input.
pub(crate) fn submit_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    let (prompt, mode) = {
        let s = state.borrow();
        (s.gloss_overlay.take_ask_text(), s.gloss_prompt_mode)
    };
    close_gloss_prompt(state);
    if prompt.trim().is_empty() {
        return;
    }
    match mode {
        crate::app::GlossPromptMode::Add => add_gloss(state, &prompt),
        crate::app::GlossPromptMode::Edit => edit_gloss(state, &prompt),
        crate::app::GlossPromptMode::FixIpa => fix_word_ipa(state, &prompt),
    }
}

/// Given a source block's verse text (one quoted line per `\n`) and the work's
/// lines as `(text, Option<start_seconds>)`, return the start time of the FIRST
/// verse line (in block order) that matches a work line carrying a timestamp.
/// Matching is exact on trimmed text. None if no matched line has timing.
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

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
    let mut s = state_rc.borrow_mut();
    let idx = s.gloss_index;
    if let Some(gloss) = s.gloss_list.get(idx) {
        let gloss_id = gloss.gloss_id;
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::delete_gloss(&conn, gloss_id);
            let _ = crate::db::queries::delete_gloss_audio(&conn, gloss_id);
        }
        if let Some(ctx) = s.gloss_context.as_ref() {
            let dir = gloss_audio_dir(&ctx.work_abbrev, gloss_id);
            let _ = std::fs::remove_dir_all(&dir);
        }
        crate::logging::log(&format!("GLOSS: deleted gloss {}", gloss_id));
        s.gloss_list.remove(idx);

        if s.gloss_list.is_empty() {
            s.gloss_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            return;
        }

        s.gloss_index = idx.min(s.gloss_list.len() - 1);
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

    let title_text = if is_edit {
        "EDIT GLOSS — PASTE SUBTEXT LINES"
    } else if is_inner_monologue {
        "INNER MONOLOGUE PASSAGE"
    } else {
        "GLOSS PROMPT"
    };
    let hint_text = if is_edit {
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

/// Play a block's TTS audio: cache hit -> play the stored MP3; miss ->
/// synthesize via ElevenLabs (async), cache it, and play. `kind`/`index`
/// identify the block; the filename stem is `<index>` (explication) or
/// `source-<index>` (source).
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
        let work_abbrev = match &s.gloss_context {
            Some(ctx) => ctx.work_abbrev.clone(),
            None => return,
        };
        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
        let text = match blocks.iter().find(|b| b.kind == kind && b.index == index) {
            Some(b) => b.text.clone(),
            None => return,
        };
        (
            gloss_id,
            work_abbrev,
            text,
            s.config.elevenlabs_voice_id.clone(),
            s.config.elevenlabs_model_id.clone(),
            s.tokio_handle.clone(),
        )
    };

    // Filename stem: explication uses "<index>", source uses "source-<index>".
    let stem = match kind {
        BlockKind::Source => format!("source-{}", index),
        BlockKind::Explication => format!("{}", index),
    };

    // Cache hit?
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Ok(Some(path)) =
            crate::db::queries::find_gloss_audio(&conn, gloss_id, kind_str, index as i64)
        {
            if std::path::Path::new(&path).exists() {
                state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                return;
            }
        }
    }

    // Miss: synthesize asynchronously.
    show_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let voice = voice_id.clone();
        let model = model_id.clone();
        let result = tokio_handle
            .spawn(async move { crate::elevenlabs::synthesize(&text, &voice, &model).await })
            .await;

        match result {
            Ok(Ok(bytes)) => {
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
                        &voice_id,
                        &model_id,
                    ) {
                        crate::log_fmt!("TTS: save_gloss_audio failed: {}", e);
                    }
                }
                state_for_result.borrow().tts.play_file(&path);
                crate::log_fmt!("TTS: synthesized gloss {} {} {}", gloss_id, kind_str, index);
            }
            Ok(Err(e)) => {
                crate::log_fmt!("TTS: synth error: {}", e);
                show_tts_toast(&state_for_result, &e.to_string());
            }
            Err(e) => {
                crate::log_fmt!("TTS: tokio join error: {}", e);
            }
        }
    });
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
    first_source_start_time(&block.text, &work_pairs)
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

fn show_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    let s = state_rc.borrow();
    s.chapter_toast.set_text(msg);
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
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
    let has_gloss = !state.borrow().gloss_list.is_empty();
    if has_gloss {
        let s = state.borrow();
        let idx = s.gloss_index;
        let gloss = &s.gloss_list[idx];
        let ctx = s.gloss_context.as_ref().unwrap();
        let cw = s.content_hbox.width();
        let h = s.content_hbox.height();
        let pairs = ctx.source_line_pairs();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, &gloss.gloss_text, cw, h,
            Some(&s.theme.root_color), &pairs,
        );
        s.gloss_overlay.set_position(idx, s.gloss_list.len());
        drop(s);
        let mut s = state.borrow_mut();
        // Remember the current page so toggling/Escape returns here.
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }
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
}

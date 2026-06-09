//! Synopsis amend flow: from the synopsis overlay, `A` opens a question prompt;
//! the answer is sent to Claude which augments (not replaces) the scene synopsis
//! with an explanation, the result is shown and persisted to scene_synopses.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

/// System prompt for the synopsis amend call. The model keeps the existing
/// synopsis intact and weaves in an explanation answering the reader's question.
/// Output is paragraph-tagged: each paragraph wrapped in <p>...</p> (the synopsis
/// card renders one <p> per visible paragraph). The model must split the synopsis
/// into a few readable paragraphs even if the input was a single block.
const SYNOPSIS_AMEND_PROMPT: &str = "\
You are a Shakespeare scholar helping a reader understand a scene. You will be \
given a play, an act and scene, the current plot synopsis for that scene, and a \
reader's question about it. Rewrite the synopsis so that it KEEPS all of the \
existing content and wording as much as possible, but weaves in a clear, \
concise explanation that answers the reader's question. Do not drop any plot \
points already present. Do not add a heading, preamble, or commentary about \
what you changed.\n\n\
FORMAT: Return the revised synopsis split into 2-4 readable paragraphs, each \
wrapped in <p>...</p> tags, like:\n\
<p>First paragraph of the synopsis.</p>\n\
<p>Second paragraph that continues the action.</p>\n\
Break paragraphs at natural shifts in the scene (a new entrance, a turn in the \
action, a change of subject). If the input synopsis already had <p> tags, keep a \
similar paragraph structure. Output ONLY the <p>-tagged paragraphs, nothing else.";

/// Open the stacked "ask" card below the synopsis card. The synopsis card
/// shrinks to make room and `Tab` toggles focus between the two; the input
/// receives typed characters while it holds focus. No-op if already open.
pub(crate) fn show_amend_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    if s.gloss_overlay.ask_is_open() {
        return;
    }
    // Amend the scene currently displayed in the overlay (which n/p may have
    // moved away from the cursor's scene).
    let scene = s.synopsis_overlay_scene;
    s.gloss_overlay.open_ask_card();
    drop(s);
    state_rc.borrow_mut().synopsis_amend_scene = scene;
}

/// Close the ask card and return focus to the synopsis card (mode stays
/// `SynopsisOverlay`).
pub(crate) fn close_amend_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().gloss_overlay.close_ask_card();
}

/// Submit the ask card: read its text, close it, and (if non-empty) kick off the
/// async synopsis amend. Called on Ctrl+Enter.
pub(crate) fn submit_amend_prompt(state: &Rc<RefCell<AppState>>) {
    let question = state.borrow().gloss_overlay.take_ask_text();
    close_amend_prompt(state);
    if !question.trim().is_empty() {
        amend_synopsis(state, &question);
    }
}

/// Send the question + current synopsis to Claude, then show and persist the
/// augmented synopsis. Mirrors the gloss add async pattern.
pub(crate) fn amend_synopsis(state_rc: &Rc<RefCell<AppState>>, question: &str) {
    let (div1, div2) = state_rc.borrow().synopsis_amend_scene;

    let (work_title, work_abbrev, original, model, tokio_handle, label) = {
        let s = state_rc.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        let abbrev = crate::app::base_work_abbrev(&work.abbrev).to_string();
        let original = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let label = crate::app::synopsis_label(&s, div1, div2);
        (
            work.title.clone(),
            abbrev,
            original,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
            label,
        )
    };
    let user_msg = format!(
        "Play: {}\n{}\n\nCurrent synopsis:\n{}\n\n---\nReader's question: {}",
        work_title, label, original, question,
    );

    state_rc.borrow().gloss_overlay.show_loading();

    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::claude::send_message(SYNOPSIS_AMEND_PROMPT, &user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(revised)) => {
                let revised = revised.trim().to_string();
                // Persist (upsert) to lit.db.
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if let Err(e) =
                        crate::db::queries::save_synopsis(&conn, &work_abbrev, div1, div2, &revised)
                    {
                        crate::logging::log(&format!("SYNOPSIS: save error: {}", e));
                    }
                }
                let mut s = state_for_result.borrow_mut();
                // Remember the pre-amend text so `U` can revert this edit.
                s.synopsis_undo = Some(((div1, div2), original.clone()));
                s.synopsis_cache.insert((div1, div2), revised.clone());
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                s.gloss_overlay.show_synopsis(&label, &revised, cw, h);
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                crate::logging::log(&format!(
                    "SYNOPSIS: amended {} ({},{})",
                    work_abbrev, div1, div2
                ));
            }
            Ok(Err(e)) => {
                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                s.gloss_overlay
                    .show_synopsis(&label, &format!("Error: {}", e), cw, h);
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                crate::logging::log(&format!("SYNOPSIS: amend error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("SYNOPSIS: tokio join error: {}", e));
            }
        }
    });
}

/// Revert the most recent `A` amendment: restore the pre-amend synopsis text in
/// the cache and in lit.db, and redisplay it. No-op (with a toast) if there is
/// nothing to undo. Single-level — only the last amendment can be undone.
pub(crate) fn undo_amend(state_rc: &Rc<RefCell<AppState>>) {
    let undo = state_rc.borrow().synopsis_undo.clone();
    let ((div1, div2), original) = match undo {
        Some(u) => u,
        None => {
            let s = state_rc.borrow();
            s.chapter_toast.set_text("Nothing to undo");
            s.chapter_toast.set_visible(true);
            let toast = s.chapter_toast.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                toast.set_visible(false);
            });
            return;
        }
    };

    let work_abbrev = {
        let s = state_rc.borrow();
        s.current_work
            .as_ref()
            .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
    };
    if let Some(abbrev) = work_abbrev {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            if let Err(e) =
                crate::db::queries::save_synopsis(&conn, &abbrev, div1, div2, &original)
            {
                crate::logging::log(&format!("SYNOPSIS: undo save error: {}", e));
            }
        }
    }

    let mut s = state_rc.borrow_mut();
    s.synopsis_cache.insert((div1, div2), original.clone());
    s.synopsis_undo = None;
    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let label = crate::app::synopsis_label(&s, div1, div2);
    s.gloss_overlay.show_synopsis(&label, &original, cw, h);
    crate::logging::log(&format!("SYNOPSIS: undid amend ({},{})", div1, div2));
}


/// Ctrl+g in the synopsis card: open the gloss overlay for the whole work, with
/// the glosses for the currently-displayed scene shown first. Mirrors the state
/// setup that `navigate_gloss_passage` relies on, so Ctrl+n/p (within a passage)
/// and Alt+n/p (across passages) work afterwards.
pub(crate) fn open_work_glosses(state_rc: &Rc<RefCell<AppState>>) {
    let (div1, div2) = state_rc.borrow().synopsis_overlay_scene;
    let work_abbrev = {
        let s = state_rc.borrow();
        match s.current_work.as_ref() {
            Some(w) => crate::app::base_work_abbrev(&w.abbrev).to_string(),
            None => return,
        }
    };

    // Load every glossed passage for the work (reading order), then rotate so the
    // current scene's passages come first.
    let mut passages = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::queries::find_glossed_passages(
            &conn, &work_abbrev, &["teacher-generic", "inner-monologue"],
        ).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if passages.is_empty() {
        let s = state_rc.borrow();
        s.chapter_toast.set_text("No glosses for this work");
        s.chapter_toast.set_visible(true);
        let toast = s.chapter_toast.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            toast.set_visible(false);
        });
        return;
    }
    // Stable partition: current-scene passages first, the rest after, each group
    // keeping its reading order.
    let (mut here, rest): (Vec<_>, Vec<_>) = passages
        .drain(..)
        .partition(|p| p.act == div1 && p.scene == div2);
    here.extend(rest);
    let passages = here;

    let first = passages[0].clone();
    let all_glosses = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_all_glosses(
                &conn, &first.work_abbrev, &first.start_citation, &first.end_citation,
                &["teacher-generic", "inner-monologue"],
            ).ok()
        })
        .unwrap_or_default();
    if all_glosses.is_empty() {
        return;
    }

    let mut s = state_rc.borrow_mut();
    let work_title = s.current_work.as_ref().map(|w| w.title.clone()).unwrap_or_default();
    let gloss_type = all_glosses[0].gloss_type.clone();
    let ctx = crate::gloss::GlossContext {
        work_abbrev: first.work_abbrev.clone(),
        work_title,
        start_citation: first.start_citation.clone(),
        end_citation: first.end_citation.clone(),
        act: first.act,
        scene: first.scene,
        speaker: first.speaker.clone(),
        source_text: first.source_text.clone(),
        source_line_numbers: Vec::new(),
        hash: String::new(),
        gloss_type,
    };

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let gloss_text = all_glosses[0].gloss_text.clone();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &gloss_text, cw, h, Some(&s.theme.root_color), &[],
    );
    s.gloss_overlay.set_position(0, all_glosses.len());
    s.gloss_list = all_glosses;
    s.gloss_index = 0;
    s.gloss_active_voice = 0;
    s.gloss_passages = passages;
    s.gloss_passage_index = 0;
    s.gloss_context = Some(ctx);
    // Remember the reader page so Escape returns here (unless an earlier step,
    // e.g. a picker, already recorded a position to return to).
    if s.gloss_return_pos.is_none() {
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
    }
    s.input_mode = crate::app::InputMode::GlossOverlay;
    crate::logging::log(&format!("SYNOPSIS: opened work glosses, scene ({},{}) first", div1, div2));
}

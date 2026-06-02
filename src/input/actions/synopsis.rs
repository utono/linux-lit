//! Synopsis amend flow: from the synopsis overlay, `A` opens a question prompt;
//! the answer is sent to Claude which augments (not replaces) the scene synopsis
//! with an explanation, the result is shown and persisted to scene_synopses.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

/// System prompt for the synopsis amend call. The model keeps the existing
/// synopsis intact and weaves in an explanation answering the reader's question.
const SYNOPSIS_AMEND_PROMPT: &str = "\
You are a Shakespeare scholar helping a reader understand a scene. You will be \
given a play, an act and scene, the current plot synopsis for that scene, and a \
reader's question about it. Rewrite the synopsis so that it KEEPS all of the \
existing content and wording as much as possible, but weaves in a clear, \
concise explanation that answers the reader's question. Do not drop any plot \
points already present. Do not add a heading, preamble, or commentary about \
what you changed. Return only the revised synopsis as a single flowing prose \
paragraph (or the same number of paragraphs as the original).";

/// Open the question input dialog over the synopsis overlay. Reuses the gloss
/// prompt weakref fields (gloss and synopsis prompts are never open together).
pub(crate) fn show_amend_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let overlay_parent = {
        let s = state_rc.borrow();
        s.action_popup_widget.container.parent()
    };
    let overlay_parent = match overlay_parent.and_then(|p| p.downcast::<gtk4::Overlay>().ok()) {
        Some(o) => o,
        None => return,
    };

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(600);
    container.add_css_class("amend-dialog");

    let title = gtk4::Label::new(Some("ASK ABOUT THIS SCENE"));
    title.add_css_class("amend-title");
    title.set_halign(gtk4::Align::Start);
    container.append(&title);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_min_content_height(120);
    scrolled.set_margin_start(22);
    scrolled.set_margin_end(22);
    scrolled.set_margin_top(8);
    scrolled.set_margin_bottom(8);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_top_margin(8);
    text_view.set_bottom_margin(8);
    text_view.set_left_margin(4);
    text_view.set_right_margin(4);
    text_view.add_css_class("amend-text");
    scrolled.set_child(Some(&text_view));
    container.append(&scrolled);

    let hint = gtk4::Label::new(Some(
        "Ask a question; the synopsis will be expanded to answer it  \u{00b7}  Ctrl+Enter submit  \u{00b7}  Esc cancel",
    ));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    {
        let mut s = state_rc.borrow_mut();
        // Remember which scene the answer should amend.
        s.synopsis_amend_scene = crate::app::current_scene_divs(&s);
        s.gloss_prompt_container = Some(container.downgrade());
        s.gloss_prompt_overlay = Some(overlay_parent.downgrade());
        s.gloss_prompt_textview = Some(text_view.downgrade());
        s.input_mode = crate::app::InputMode::SynopsisPrompt;
    }

    text_view.grab_focus();
}

/// Remove the prompt dialog and return to the synopsis overlay.
pub(crate) fn close_amend_prompt(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (s.gloss_prompt_container.take(), s.gloss_prompt_overlay.take()) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    s.gloss_prompt_textview = None;
    s.input_mode = crate::app::InputMode::SynopsisOverlay;
}

/// Send the question + current synopsis to Claude, then show and persist the
/// augmented synopsis. Mirrors the gloss add async pattern.
pub(crate) fn amend_synopsis(state_rc: &Rc<RefCell<AppState>>, question: &str) {
    let (div1, div2) = state_rc.borrow().synopsis_amend_scene;

    let (work_title, work_abbrev, original, model, tokio_handle) = {
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
        (
            work.title.clone(),
            abbrev,
            original,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
        )
    };

    let scene_label = scene_label(div1, div2);
    let user_msg = format!(
        "Play: {}\n{}\n\nCurrent synopsis:\n{}\n\n---\nReader's question: {}",
        work_title, scene_label, original, question,
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
                let h = s.scrolled_window.height();
                s.gloss_overlay.show_synopsis(&scene_label, &revised, h);
                s.input_mode = crate::app::InputMode::SynopsisOverlay;
                crate::logging::log(&format!(
                    "SYNOPSIS: amended {} ({},{})",
                    work_abbrev, div1, div2
                ));
            }
            Ok(Err(e)) => {
                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                s.gloss_overlay
                    .show_synopsis(&scene_label, &format!("Error: {}", e), h);
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
    let h = s.scrolled_window.height();
    let label = scene_label(div1, div2);
    s.gloss_overlay.show_synopsis(&label, &original, h);
    crate::logging::log(&format!("SYNOPSIS: undid amend ({},{})", div1, div2));
}

fn scene_label(div1: i64, div2: i64) -> String {
    if div1 == 0 && div2 == 0 {
        "Prologue".to_string()
    } else if div2 == 0 {
        format!("Act {}, Chorus", div1)
    } else {
        format!("Act {}, Scene {}", div1, div2)
    }
}

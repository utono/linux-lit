use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::AppState;

pub(crate) fn navigate_gloss_passage(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();

    let work_abbrev = match &s.gloss_context {
        Some(ctx) => ctx.work_abbrev.clone(),
        None => return,
    };

    if s.gloss_passages.is_empty() {
        if let Ok(conn) = crate::db::queries::open_db() {
            s.gloss_passages = crate::db::queries::find_glossed_passages(&conn, &work_abbrev)
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
    let new_idx = ((s.gloss_passage_index as i32 + delta).rem_euclid(len as i32)) as usize;
    if new_idx == s.gloss_passage_index && len > 1 {
        return;
    }
    s.gloss_passage_index = new_idx;

    let passage = s.gloss_passages[new_idx].clone();

    let all_glosses = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_all_glosses(
                &conn, &passage.work_abbrev, &passage.start_citation, &passage.end_citation,
            ).ok()
        })
        .unwrap_or_default();

    if all_glosses.is_empty() {
        return;
    }

    let source_lines: Vec<(String, i64)> = Vec::new();

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
    };

    let h = s.scrolled_window.height();
    let gloss_text = &all_glosses[0].gloss_text;
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, gloss_text, h,
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
    let h = s.scrolled_window.height();
    let pairs = ctx.source_line_pairs();
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &gloss.gloss_text, h,
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
        let h = s.scrolled_window.height();
        let pairs = ctx.source_line_pairs();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, &gloss.gloss_text, h,
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

pub(crate) fn show_amend_dialog(state_rc: &Rc<RefCell<AppState>>) {
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

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_halign(gtk4::Align::Center);
    container.set_valign(gtk4::Align::Center);
    container.set_width_request(600);
    container.add_css_class("amend-dialog");

    let title = gtk4::Label::new(Some("GLOSS PROMPT"));
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

    let hint = gtk4::Label::new(Some("Ctrl+Enter submit  \u{00b7}  Esc cancel"));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_prompt_container = Some(container.downgrade());
        s.gloss_prompt_overlay = Some(overlay_parent.downgrade());
        s.gloss_prompt_textview = Some(text_view.downgrade());
        s.input_mode = crate::app::InputMode::GlossPrompt;
    }

    text_view.grab_focus();
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
    let user_msg = crate::gloss::build_user_message(
        &ctx, Some(&prompt_owned), None,
    );
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    crate::gloss::USER_QUESTION_PROMPT, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let full_gloss = format!("<gloss>Q: {}</gloss>\n\n{}", prompt_owned, gloss_text);
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss(
                        &conn, &ctx.hash, &ctx.work_abbrev,
                        &ctx.start_citation, &ctx.end_citation,
                        ctx.act, ctx.scene, &ctx.speaker,
                        &ctx.source_text, &full_gloss,
                    );
                }

                let all = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::queries::find_all_glosses(
                            &conn, &ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation,
                        ).ok()
                    })
                    .unwrap_or_default();

                let mut s = state_for_result.borrow_mut();
                let h = s.scrolled_window.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &full_gloss, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(0, all.len());
                s.gloss_list = all;
                s.gloss_index = 0;
                crate::logging::log("GLOSS: added new gloss");
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

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == crate::app::InputMode::GlossOverlay {
        let mut s = state.borrow_mut();
        s.gloss_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        return;
    }
    let has_gloss = !state.borrow().gloss_list.is_empty();
    if has_gloss {
        let s = state.borrow();
        let idx = s.gloss_index;
        let gloss = &s.gloss_list[idx];
        let ctx = s.gloss_context.as_ref().unwrap();
        let h = s.scrolled_window.height();
        let pairs = ctx.source_line_pairs();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, &gloss.gloss_text, h,
            Some(&s.theme.root_color), &pairs,
        );
        s.gloss_overlay.set_position(idx, s.gloss_list.len());
        drop(s);
        state.borrow_mut().input_mode = crate::app::InputMode::GlossOverlay;
    }
}

pub(crate) fn close_gloss_prompt(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (s.gloss_prompt_container.take(), s.gloss_prompt_overlay.take()) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    s.gloss_prompt_textview = None;
    s.input_mode = crate::app::InputMode::GlossOverlay;
}

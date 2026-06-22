use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Load the current band's pages from the DB into `journal_pages`, clamp the
/// index, and render the current page (or the empty-band card).
fn render_current(s: &mut AppState) {
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();

    let conn = crate::db::queries::open_db().ok();
    let (pages, scene_title) = match s.journal_band {
        JournalBand::Work => {
            let pages = conn
                .and_then(|c| crate::db::journal::find_work_pages(&c, &work_abbrev).ok())
                .unwrap_or_default();
            let title = format!(
                "{} — whole work",
                s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
            );
            (pages, title)
        }
        JournalBand::Scene(d1, d2) => {
            let pages = conn
                .and_then(|c| crate::db::journal::find_journal_pages(&c, &work_abbrev, d1, d2).ok())
                .unwrap_or_default();
            let title = format!(
                "{} — {}",
                s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
                crate::app::synopsis_label(s, d1, d2),
            );
            (pages, title)
        }
    };

    let count = pages.len();
    if count == 0 {
        s.journal_page_index = 0;
    } else if s.journal_page_index >= count {
        s.journal_page_index = count - 1;
    }

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let (q, a) = if count == 0 {
        (String::new(), String::new())
    } else {
        let p = &pages[s.journal_page_index];
        (p.question.clone(), p.answer.clone())
    };
    s.journal_overlay
        .show_page(&scene_title, s.journal_page_index, count, &q, &a, cw, h);
    s.journal_pages = pages;
}

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        s.input_mode = InputMode::Reader;
        if let Some((line, top)) = s.journal_return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
            crate::input::scroll::resnap_page(&mut s);
            crate::input::highlight::update_highlight(&mut s);
        }
        return;
    }

    let mut s = state.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    s.journal_return_pos = Some((s.current_line, s.page_top_line));
    let (d1, d2) = crate::app::current_scene_divs(&s);
    s.journal_band = JournalBand::Scene(d1, d2);
    s.journal_page_index = 0;
    s.input_mode = InputMode::JournalOverlay;
    render_current(&mut s);
}

pub(crate) fn close_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        toggle_overlay(state);
    }
}

/// Flip pages within the current scene (clamped, no wrap).
pub(crate) fn nav_page(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let count = s.journal_pages.len();
    if count == 0 {
        return;
    }
    let cur = s.journal_page_index as i64;
    let next = (cur + delta as i64).clamp(0, count as i64 - 1) as usize;
    if next != s.journal_page_index {
        s.journal_page_index = next;
        render_current(&mut s);
    }
}

/// Jump to the next/prev scene that has pages (skips empty scenes). Lands on
/// that scene's first page. From the Work band, delta>0 lands on the first
/// scene with pages, delta<0 on the last (the Work band sorts before scenes).
pub(crate) fn nav_scene(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();
    let scenes = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_journal_scenes(&conn, &work_abbrev).ok())
        .unwrap_or_default();
    if scenes.is_empty() {
        return;
    }

    let target_idx: i64 = match s.journal_band {
        // From the Work band, enter the scene list at the appropriate end.
        JournalBand::Work => {
            if delta > 0 { 0 } else { scenes.len() as i64 - 1 }
        }
        JournalBand::Scene(d1, d2) => {
            match scenes.iter().position(|&sc| sc == (d1, d2)) {
                Some(i) => (i as i64 + delta as i64).clamp(0, scenes.len() as i64 - 1),
                None => {
                    if delta > 0 { 0 } else { scenes.len() as i64 - 1 }
                }
            }
        }
    };

    let target = JournalBand::Scene(scenes[target_idx as usize].0, scenes[target_idx as usize].1);
    if target != s.journal_band {
        s.journal_band = target;
        s.journal_page_index = 0;
        render_current(&mut s);
    }
}

/// Switch to the Work band (whole-work pages) and render it.
pub(crate) fn nav_to_work_band(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal_band == JournalBand::Work {
        return;
    }
    s.journal_band = JournalBand::Work;
    s.journal_page_index = 0;
    render_current(&mut s);
}

pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal_prompt_mode = JournalPromptMode::Ask;
    let title = match s.journal_band {
        JournalBand::Work => "Ask a question about the whole work",
        JournalBand::Scene(_, _) => "Ask a question about this scene",
    };
    s.journal_overlay
        .open_ask_card(title, "Ctrl+Enter to ask · Esc to cancel");
}

pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal_pages.is_empty() {
        return;
    }
    s.journal_prompt_mode = JournalPromptMode::Edit;
    s.journal_overlay
        .open_ask_card("Edit: ask a new question for this page", "Ctrl+Enter · Esc");
}

pub(crate) fn close_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().journal_overlay.close_ask_card();
}

pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let (question, mode) = {
        let s = state.borrow();
        (s.journal_overlay.take_ask_text(), s.journal_prompt_mode)
    };
    close_prompt(state);
    if question.trim().is_empty() {
        return;
    }
    ask_claude(state, &question, mode);
}

fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str, mode: JournalPromptMode) {
    let (work_title, work_author, work_abbrev, band, scene_text, model, tokio_handle) = {
        let s = state_rc.borrow();
        let band = s.journal_band;
        let (title, author, abbrev) = match s.current_work.as_ref() {
            Some(w) => (
                w.title.clone(),
                w.author.clone(),
                crate::app::base_work_abbrev(&w.abbrev).to_string(),
            ),
            None => return,
        };
        let scene_text = match band {
            JournalBand::Work => String::new(),
            JournalBand::Scene(d1, d2) => crate::app::scene_text_for(&s, d1, d2),
        };
        (
            title,
            author,
            abbrev,
            band,
            scene_text,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
        )
    };

    state_rc.borrow().journal_overlay.show_loading();

    let edit_id: i64 = if mode == JournalPromptMode::Edit {
        let s = state_rc.borrow();
        s.journal_pages
            .get(s.journal_page_index)
            .map(|p| p.id)
            .unwrap_or(-1)
    } else {
        -1
    };

    let user_msg = match band {
        JournalBand::Work => format!(
            "Work: {} by {}\n\nReader's question about the play as a whole:\n{}",
            work_title, work_author, question,
        ),
        JournalBand::Scene(d1, d2) => format!(
            "Work: {} by {}\nScene: {}\n\nScene text:\n{}\n\nReader's question:\n{}",
            work_title,
            work_author,
            crate::app::scene_label(d1, d2),
            scene_text,
            question,
        ),
    };
    let question_owned = question.to_string();
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &crate::gloss::JOURNAL_QA_PROMPT,
                    &user_msg,
                    &model,
                )
                .await
            })
            .await;

        match result {
            Ok(Ok(answer)) => {
                // For a save, the scope and (div1,div2) come from the band.
                let (scope, sdiv1, sdiv2) = match band {
                    JournalBand::Work => ("work", -1_i64, -1_i64),
                    JournalBand::Scene(d1, d2) => ("scene", d1, d2),
                };
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let write_result = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                        crate::db::journal::update_journal_page(
                            &conn, edit_id, &question_owned, &answer, &model_for_db,
                        )
                    } else {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev, sdiv1, sdiv2, &question_owned, &answer,
                            &model_for_db, scope,
                        )
                        .map(|_| ())
                    };
                    if let Err(e) = write_result {
                        crate::logging::log(&format!("JOURNAL: db write failed: {}", e));
                    }
                }
                let pages = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| match band {
                        JournalBand::Work => {
                            crate::db::journal::find_work_pages(&conn, &work_abbrev).ok()
                        }
                        JournalBand::Scene(d1, d2) => {
                            crate::db::journal::find_journal_pages(&conn, &work_abbrev, d1, d2).ok()
                        }
                    })
                    .unwrap_or_default();
                let new_index = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                    pages.iter().position(|p| p.id == edit_id).unwrap_or(0)
                } else {
                    pages.len().saturating_sub(1)
                };
                let mut s = state_for_result.borrow_mut();
                s.journal_band = band;
                s.journal_page_index = new_index;
                render_current(&mut s);
                crate::logging::log("JOURNAL: saved page");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.journal_overlay.show_message(&format!("Error: {}", e));
                crate::logging::log(&format!("JOURNAL: claude error: {}", e));
            }
            Err(e) => {
                let s = state_for_result.borrow();
                s.journal_overlay.show_message("Internal error — try again.");
                crate::logging::log(&format!("JOURNAL: tokio join error: {}", e));
            }
        }
    });
}

/// Open the Q&A picker over the journal overlay. Lists every page in the work
/// (work pages first, then scene pages by scene), each by creation time. Empty
/// journal -> toast, stay in the overlay.
pub(crate) fn open_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();
    let pages = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_all_pages_ordered(&conn, &work_abbrev).ok())
        .unwrap_or_default();

    if pages.is_empty() {
        s.chapter_toast.set_text("No journal pages yet — press a to ask");
        s.chapter_toast.set_visible(true);
        let toast = s.chapter_toast.clone();
        gtk4::glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            toast.set_visible(false);
        });
        return;
    }

    let rows: Vec<crate::ui::journal_picker::JournalRow> = pages
        .iter()
        .map(|p| {
            let band = if p.div1 < 0 {
                JournalBand::Work
            } else {
                JournalBand::Scene(p.div1, p.div2)
            };
            let scene_label = match band {
                JournalBand::Work => "whole work".to_string(),
                JournalBand::Scene(d1, d2) => crate::app::synopsis_label(&s, d1, d2),
            };
            let prefix: String = p.question.chars().take(80).collect();
            crate::ui::journal_picker::JournalRow {
                id: p.id,
                band,
                question_prefix: prefix,
                scene_label,
            }
        })
        .collect();

    s.journal_picker.set_items(rows);
    s.journal_picker.show();
    s.input_mode = InputMode::JournalPicker;
}

/// Confirm the picker selection: switch the journal overlay to the chosen page's
/// band, land on that exact page (matched by id within the band), hide the
/// picker, return to the journal overlay.
pub(crate) fn confirm_picker(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().journal_picker.selected_index();
    let mut s = state.borrow_mut();
    s.journal_picker.hide();
    s.input_mode = InputMode::JournalOverlay;

    let Some(idx) = selected else {
        // Nothing selected — just return to the overlay, re-render current band.
        render_current(&mut s);
        return;
    };
    let (band, target_id) = {
        let row = &s.journal_picker.items[idx];
        (row.band, row.id)
    };

    s.journal_band = band;
    s.journal_page_index = 0;
    render_current(&mut s); // loads the band's pages into s.journal_pages
    if let Some(pos) = s.journal_pages.iter().position(|p| p.id == target_id) {
        s.journal_page_index = pos;
        render_current(&mut s);
    }
}

pub(crate) fn delete_current(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal_pages.is_empty() {
        return;
    }
    let id = s.journal_pages[s.journal_page_index].id;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::delete_journal_page(&conn, id);
    }
    if s.journal_page_index > 0 {
        s.journal_page_index -= 1;
    }
    render_current(&mut s);
}

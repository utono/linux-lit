use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Passage context captured from a visual selection, held until the ask-card
/// submit fires (at which point `ask_claude` reads it and clears it).
/// The passage coordinates (div1/div2/start/end) live in `JournalBand::Passage`;
/// only `source_text` (the `<speaker>/<verse>` markup) needs separate storage.
pub struct PendingPassage {
    pub source_text: String,
}

/// Grouped state for the journal feature (band pages + viewer index + the
/// return-to-reader position + the add/edit prompt mode). Was four flat
/// `journal_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (pure-tier cluster).
pub struct JournalState {
    pub pages: Vec<crate::db::journal::JournalPage>,
    pub page_index: usize,
    pub return_pos: Option<(usize, usize)>,
    pub prompt_mode: JournalPromptMode,
    /// Set by `action_journal_qa` before opening the ask card; read and
    /// consumed by `ask_claude` when the band is `Passage`.
    pub pending_passage: Option<PendingPassage>,
}

/// Footer-left text identifying the current page: `<abbrev> <act>.<scene>` for a
/// scene page, `<abbrev> · whole work` for a whole-work page.
fn footer_left_text(abbrev: &str, band: JournalBand) -> String {
    match band {
        JournalBand::Work => format!("{} \u{00b7} whole work", abbrev),
        JournalBand::Scene(d1, d2) => format!("{} {}.{}", abbrev, d1, d2),
        JournalBand::Passage { div1, div2, .. } => format!("{} {}.{} passage", abbrev, div1, div2),
    }
}

/// Load the current band's pages from the DB into `journal.pages`, clamp the
/// index, and render the current page (or the empty-band card).
pub(crate) fn render_current(s: &mut AppState) {
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();

    let conn = crate::db::queries::open_db().ok();
    let (pages, scene_title) = match s.journal_band.clone() {
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
                crate::app::scene_synopsis::synopsis_label(s, d1, d2),
            );
            (pages, title)
        }
        JournalBand::Passage { start, end, .. } => {
            let pages = conn
                .and_then(|c| crate::db::journal::find_passage_pages(&c, &work_abbrev, &start, &end).ok())
                .unwrap_or_default();
            let title = format!(
                "{} — passage",
                s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
            );
            (pages, title)
        }
    };

    let count = pages.len();
    if count == 0 {
        s.journal.page_index = 0;
    } else if s.journal.page_index >= count {
        s.journal.page_index = count - 1;
    }

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let footer_left = footer_left_text(&work_abbrev, s.journal_band.clone());

    // Passage pages with source_text use the verse renderer; everything else
    // uses the plain show_page path.
    let current_page = if count == 0 {
        None
    } else {
        Some(&pages[s.journal.page_index])
    };

    let is_passage_with_source = matches!(s.journal_band, JournalBand::Passage { .. })
        && current_page.is_some_and(|p| p.source_text.is_some());

    if is_passage_with_source {
        let p = current_page.unwrap();
        let source_text = p.source_text.as_deref().unwrap_or("");
        s.journal_overlay.show_passage_page(
            &footer_left,
            s.journal.page_index,
            count,
            p.start_citation.as_deref(),
            p.end_citation.as_deref(),
            source_text,
            &p.question,
            &p.answer,
            cw,
            h,
        );
    } else {
        let (q, a) = current_page
            .map(|p| (p.question.clone(), p.answer.clone()))
            .unwrap_or_default();
        s.journal_overlay
            .show_page(&scene_title, &footer_left, s.journal.page_index, count, &q, &a, cw, h);
    }

    s.journal.pages = pages;
}

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        // Recolor the main card BEFORE update_highlight (which re-applies the tint
        // for reader_gloss_lines), so a reader-gloss created/edited in the overlay
        // colors immediately on return.
        crate::app::return_to_reader_mode(&mut s);
        if let Some((line, top)) = s.journal.return_pos.take() {
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
    s.journal.return_pos = Some((s.current_line, s.page_top_line));
    let (d1, d2) = crate::app::scene_synopsis::current_scene_divs(&s);
    s.journal_band = JournalBand::Scene(d1, d2);
    s.journal.page_index = 0;
    s.input_mode = InputMode::JournalOverlay;
    render_current(&mut s);
}

pub(crate) fn close_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        toggle_overlay(state);
    }
}

/// Flip pages within the current band (clamped, no wrap).
pub(crate) fn nav_page(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let count = s.journal.pages.len();
    if count == 0 {
        return;
    }
    let cur = s.journal.page_index as i64;
    let next = (cur + delta as i64).clamp(0, count as i64 - 1) as usize;
    if next != s.journal.page_index {
        s.journal.page_index = next;
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

    let target_idx: i64 = match s.journal_band.clone() {
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
        JournalBand::Passage { .. } => return, // passage band nav is out of scope
    };

    let target = JournalBand::Scene(scenes[target_idx as usize].0, scenes[target_idx as usize].1);
    if target != s.journal_band {
        s.journal_band = target;
        s.journal.page_index = 0;
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
    s.journal.page_index = 0;
    render_current(&mut s);
}

/// Set up the journal overlay for a passage Q&A and open the ask card.
///
/// Called both from `action_journal_qa` (visual selection path) and from the
/// gloss overlay's `J` key (gloss-context path). The caller has already
/// exited visual mode / closed any conflicting overlay and set `return_pos`.
///
/// - Sets `journal.pending_passage` with the `<speaker>/<verse>` markup.
/// - Sets `journal_band` to `Passage { div1, div2, start, end }`.
/// - Sets `input_mode` to `JournalOverlay` and renders the current page list.
/// - Opens the ask card titled "Ask about this passage".
pub(crate) fn begin_passage_ask(
    state: &Rc<RefCell<AppState>>,
    div1: i64,
    div2: i64,
    start: String,
    end: String,
    source_text: String,
) {
    let mut s = state.borrow_mut();
    s.journal.return_pos = Some((s.current_line, s.page_top_line));
    s.journal.prompt_mode = JournalPromptMode::Ask;
    s.journal.pending_passage = Some(PendingPassage { source_text });
    s.journal_band = JournalBand::Passage { div1, div2, start, end };
    s.journal.page_index = 0;
    s.input_mode = crate::app::InputMode::JournalOverlay;
    render_current(&mut s);
    s.journal_overlay
        .open_ask_card("Ask about this passage", "Tab switch  \u{00b7}  Ctrl+Enter submit");
}

pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal.prompt_mode = JournalPromptMode::Ask;
    let title = match s.journal_band {
        JournalBand::Work => "Ask a question about the whole work",
        JournalBand::Scene(_, _) => "Ask a question about this scene",
        JournalBand::Passage { .. } => "Ask a question about this passage",
    };
    s.journal_overlay
        .open_ask_card(title, "Tab switch  \u{00b7}  Ctrl+Enter submit");
}

pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.pages.is_empty() {
        return;
    }
    s.journal.prompt_mode = JournalPromptMode::Edit;
    s.journal_overlay
        .open_ask_card(
            "Edit: ask a new question for this page",
            "Tab switch  \u{00b7}  Ctrl+Enter submit",
        );
}

pub(crate) fn close_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().journal_overlay.close_ask_card();
}

pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let (question, mode) = {
        let s = state.borrow();
        (s.journal_overlay.take_ask_text(), s.journal.prompt_mode)
    };
    close_prompt(state);
    if question.trim().is_empty() {
        return;
    }
    ask_claude(state, &question, mode);
}

fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str, mode: JournalPromptMode) {
    let (work_title, work_author, work_abbrev, band, scene_text, model) = {
        let s = state_rc.borrow();
        let band = s.journal_band.clone();
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
            JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_text_for(&s, d1, d2),
            JournalBand::Passage { div1, div2, .. } => {
                crate::app::scene_synopsis::scene_text_for(&s, div1, div2)
            }
        };
        (
            title,
            author,
            abbrev,
            band,
            scene_text,
            s.config.claude_model.clone(),
        )
    };

    state_rc.borrow().journal_overlay.show_loading(question);

    let edit_id: i64 = if mode == JournalPromptMode::Edit {
        let s = state_rc.borrow();
        s.journal.pages
            .get(s.journal.page_index)
            .map(|p| p.id)
            .unwrap_or(-1)
    } else {
        -1
    };

    // For a Passage band, consume pending_passage (take it so the Option is
    // cleared after use — defensive hygiene; the guard above makes it
    // harmless but the field should not linger after it is read).
    let passage_source_text: String = if matches!(band, JournalBand::Passage { .. }) {
        state_rc
            .borrow_mut()
            .journal
            .pending_passage
            .take()
            .map(|pp| pp.source_text)
            .unwrap_or_default()
    } else {
        String::new()
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
            crate::app::scene_synopsis::scene_label(d1, d2),
            scene_text,
            question,
        ),
        JournalBand::Passage { div1, div2, .. } => format!(
            "Work: {} by {}\nScene: {}\n\nScene text:\n{}\n\nPassage:\n{}\n\nReader's question:\n{}",
            work_title,
            work_author,
            crate::app::scene_synopsis::scene_label(div1, div2),
            scene_text,
            passage_source_text,
            question,
        ),
    };
    let question_owned = question.to_string();
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::JOURNAL_QA_PROMPT.to_string(),
        user_msg,
        model,
        move |st, answer| {
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let write_result = match (&band, mode == JournalPromptMode::Edit && edit_id >= 0) {
                    (_, true) => {
                        crate::db::journal::update_journal_page(
                            &conn, edit_id, &question_owned, &answer, &model_for_db,
                        )
                    }
                    (JournalBand::Work, false) => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev,
                            crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1,
                            &question_owned, &answer, &model_for_db, "work",
                        )
                        .map(|_| ())
                    }
                    (JournalBand::Scene(d1, d2), false) => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev, *d1, *d2,
                            &question_owned, &answer, &model_for_db, "scene",
                        )
                        .map(|_| ())
                    }
                    (JournalBand::Passage { div1, div2, start, end }, false) => {
                        crate::db::journal::save_passage_page(
                            &conn, &work_abbrev, *div1, *div2, start, end,
                            &passage_source_text, &question_owned, &answer, &model_for_db,
                        )
                        .map(|_| ())
                    }
                };
                if let Err(e) = write_result {
                    crate::logging::log(&format!("JOURNAL: db write failed: {}", e));
                }
            }
            let pages = crate::db::queries::open_db()
                .ok()
                .and_then(|conn| match &band {
                    JournalBand::Work => {
                        crate::db::journal::find_work_pages(&conn, &work_abbrev).ok()
                    }
                    JournalBand::Scene(d1, d2) => {
                        crate::db::journal::find_journal_pages(&conn, &work_abbrev, *d1, *d2).ok()
                    }
                    JournalBand::Passage { start, end, .. } => {
                        crate::db::journal::find_passage_pages(&conn, &work_abbrev, start, end).ok()
                    }
                })
                .unwrap_or_default();
            let new_index = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                pages.iter().position(|p| p.id == edit_id).unwrap_or(0)
            } else {
                pages.len().saturating_sub(1)
            };
            let mut s = st.borrow_mut();
            s.journal_band = band.clone();
            s.journal.page_index = new_index;
            render_current(&mut s);
            crate::logging::log("JOURNAL: saved page");
        },
        move |st, msg| {
            st.borrow().journal_overlay.show_message(msg);
        },
    );
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
        crate::ui::toast::show_transient(&s.chapter_toast, "No journal pages yet — press A to ask", 3);
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
            let scene_label = match &band {
                JournalBand::Work => "whole work".to_string(),
                JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::synopsis_label(&s, *d1, *d2),
                JournalBand::Passage { div1, div2, .. } => {
                    format!("{}.{} passage", div1, div2)
                }
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
        (row.band.clone(), row.id)
    };

    s.journal_band = band;
    s.journal.page_index = 0;
    render_current(&mut s); // loads the band's pages into s.journal.pages
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == target_id) {
        s.journal.page_index = pos;
        render_current(&mut s);
    }
}

pub(crate) fn delete_current(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.pages.is_empty() {
        return;
    }
    let id = s.journal.pages[s.journal.page_index].id;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::delete_journal_page(&conn, id);
    }
    if s.journal.page_index > 0 {
        s.journal.page_index -= 1;
    }
    render_current(&mut s);
}

/// Alt+g in the journal overlay: create a reader-gloss for the current passage
/// page's source text.
///
/// Requires the current page to be a passage page (JournalBand::Passage with a
/// stored source_text). If the current page is not a passage page, shows a toast
/// and returns. Otherwise looks up the work lines for the citation range, builds
/// a GlossContext, and triggers the reader-gloss creation flow (same as
/// action_reader_gloss from a visual selection: cache-hit shows the gloss
/// immediately; cache-miss calls Claude and saves).
pub(crate) fn action_gloss_from_journal_passage(state: &Rc<RefCell<AppState>>) {
    // Phase 1: check we are on a passage page and gather what we need.
    let (ctx, model, tokio_handle, all_glosses, passage_doc) = {
        let s = state.borrow();

        // Must be in a Passage band.
        let (div1, div2, start_cit, end_cit) = match &s.journal_band {
            JournalBand::Passage { div1, div2, start, end } => {
                (*div1, *div2, start.clone(), end.clone())
            }
            _ => {
                crate::ui::toast::show_transient(&s.chapter_toast, "Not a passage page", 2);
                return;
            }
        };

        // Must have a source_text on the current page.
        match s.journal.pages.get(s.journal.page_index) {
            Some(p) if p.source_text.is_some() => {}
            _ => {
                crate::ui::toast::show_transient(&s.chapter_toast, "Not a passage page", 2);
                return;
            }
        }

        // Look up the actual work lines for the citation range so we can build a
        // proper GlossContext (with plain-text source_text and line numbers).
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };

        let start_triple = crate::app::parse_citation(&start_cit);
        let end_triple = crate::app::parse_citation(&end_cit);

        // Filter the work lines to those in [start_citation, end_citation].
        // Match primarily on (div1, div2, line_in_div) tuples; fall back to
        // the full set of lines in the passage's (div1, div2) if parsing fails.
        let selected_lines: Vec<crate::db::models::Line> = match (start_triple, end_triple) {
            (Some((sd1, sd2, s_lid)), Some((ed1, ed2, e_lid))) => {
                work.lines
                    .iter()
                    .filter(|l| {
                        let t = (l.div1, l.div2, l.line_in_div);
                        t >= (sd1, sd2, s_lid) && t <= (ed1, ed2, e_lid)
                    })
                    .cloned()
                    .collect()
            }
            _ => {
                // Citation parse failed; collect all lines in (div1, div2).
                work.lines
                    .iter()
                    .filter(|l| l.div1 == div1 && l.div2 == div2)
                    .cloned()
                    .collect()
            }
        };

        if selected_lines.is_empty() {
            crate::ui::toast::show_transient(&s.chapter_toast, "Could not locate passage lines", 2);
            return;
        }

        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(c) => c,
            None => return,
        };

        let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx, s.config.claude_model.clone(), s.tokio_handle.clone(), all_glosses, passage_doc)
    };

    // Phase 2: transition from journal overlay to gloss overlay.
    {
        let mut s = state.borrow_mut();
        // Close the journal overlay and restore reader mode so the gloss overlay
        // opens cleanly (gloss overlay saves/restores its own return position).
        s.journal_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        if let Some((line, top)) = s.journal.return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
        }
    }

    // Phase 3: cache hit — show existing gloss immediately.
    let own_idx = all_glosses.iter().position(|g| g.gloss_type == "reader-gloss");
    if let Some(idx) = own_idx {
        let mut s = state.borrow_mut();
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = s.content_hbox.height();
        s.gloss_overlay.show_gloss_with_color(
            &ctx.source_text, gloss_text, card_width, card_height,
            Some(&s.theme.root_color), &pairs,
        );
        s.gloss_overlay.set_position(idx, all_glosses.len());
        s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
        s.gloss_list = all_glosses;
        s.gloss_index = idx;
        s.gloss_context = Some(ctx);
        s.record_last_gloss("reader-gloss");
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("GLOSS-FROM-JOURNAL: showing cached reader-gloss");
        return;
    }

    // Phase 4: cache miss — show loading card and call Claude.
    {
        let mut s = state.borrow_mut();
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
        s.gloss_original_text = Some(ctx.source_text.clone());
        let cw = s.content_hbox.width();
        let h = s.content_hbox.height();
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }

    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    let state_for_result = std::rc::Rc::clone(state);

    gtk4::glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &crate::gloss::READER_GLOSS_PROMPT, &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
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
                        &gloss_text,
                        "reader-gloss",
                        &model_for_db,
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

                let mut s = state_for_result.borrow_mut();
                let cw = s.content_hbox.width();
                let h = s.content_hbox.height();
                let pairs = ctx.source_line_pairs();
                s.gloss_overlay.show_gloss_with_color(
                    &ctx.source_text, &gloss_text, cw, h,
                    Some(&s.theme.root_color), &pairs,
                );
                s.gloss_overlay.set_position(new_idx, all.len());
                s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
                s.gloss_list = all;
                s.gloss_index = new_idx;
                s.gloss_context = Some(ctx);
                s.record_last_gloss("reader-gloss");
                crate::logging::log("GLOSS-FROM-JOURNAL: generated and saved new reader-gloss");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS-FROM-JOURNAL: API error: {}", e));
            }
            Err(e) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show("Internal error \u{2014} try again.", "");
                crate::logging::log(&format!("GLOSS-FROM-JOURNAL: tokio join error: {}", e));
            }
        }
    });
}

/// View the gloss overlay for the passage cited by the current journal page
/// (Ctrl+g in the journal overlay). The current page must be a passage page
/// (JournalBand::Passage with source_text). If so, parses start_citation and
/// calls find_glosses_by_start; on a hit, closes the journal overlay and opens
/// the gloss overlay on that passage. Toasts on failure or non-passage page.
pub(crate) fn view_gloss_from_journal(state: &Rc<RefCell<AppState>>) {
    // Phase 1: gather what we need while holding the borrow.
    let (work_abbrev, start_cit) = {
        let s = state.borrow();

        // Must be on a passage-band page with source_text.
        let start_cit = match s.journal.pages.get(s.journal.page_index) {
            Some(p) if p.source_text.is_some() => {
                match &p.start_citation {
                    Some(c) => c.clone(),
                    None => {
                        crate::ui::toast::show_transient(
                            &s.chapter_toast, "Not a passage page", 2,
                        );
                        return;
                    }
                }
            }
            _ => {
                crate::ui::toast::show_transient(
                    &s.chapter_toast, "Not a passage page", 2,
                );
                return;
            }
        };

        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => crate::app::base_work_abbrev(&w.abbrev).to_string(),
            None => return,
        };

        (work_abbrev, start_cit)
    };

    const TYPES: &[&str] = &["reader-gloss", "teacher-generic", "inner-monologue"];

    // Phase 2: look up the gloss list and the full passage metadata.
    let (all_glosses, passage) = {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(_) => {
                let s = state.borrow();
                crate::ui::toast::show_transient(
                    &s.chapter_toast, "No gloss for this passage", 3,
                );
                return;
            }
        };

        let all_glosses = crate::db::queries::find_glosses_by_start(
            &conn, &work_abbrev, &start_cit, TYPES,
        )
        .unwrap_or_default();

        if all_glosses.is_empty() {
            let s = state.borrow();
            crate::ui::toast::show_transient(
                &s.chapter_toast, "No gloss for this passage", 3,
            );
            return;
        }

        let passage = match crate::db::queries::find_glossed_passage_by_start(
            &conn, &work_abbrev, &start_cit, TYPES,
        )
        .unwrap_or(None)
        {
            Some(p) => p,
            None => {
                let s = state.borrow();
                crate::ui::toast::show_transient(
                    &s.chapter_toast, "No gloss for this passage", 3,
                );
                return;
            }
        };

        (all_glosses, passage)
    };

    // Phase 3: close the journal overlay and restore reader position.
    {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        if let Some((line, top)) = s.journal.return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
        }
        // Save gloss return position so Escape in the gloss overlay returns here.
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
    }

    // Phase 4: open the gloss overlay on the passage.
    crate::input::actions::gloss::open_gloss_overlay(
        &mut state.borrow_mut(),
        vec![passage.clone()],
        0,
        passage,
        all_glosses,
        false,
        Some("reader-gloss"),
    );
    crate::logging::log("VIEW-GLOSS-FROM-JOURNAL: opened gloss overlay from journal passage page");
}

/// View the journal passage pages for the gloss currently shown in the gloss
/// overlay (Ctrl+j in the gloss overlay). Reads gloss_context citations and
/// calls find_passage_pages; if pages exist, closes the gloss overlay and opens
/// the journal overlay in the Passage band on the first page. Toasts on failure.
pub(crate) fn view_journal_from_gloss(state: &Rc<RefCell<AppState>>) {
    // Phase 1: gather citations from gloss_context.
    let (work_abbrev, start_cit, end_cit, div1, div2) = {
        let s = state.borrow();
        let ctx = match s.gloss_context.as_ref() {
            Some(c) => c,
            None => {
                crate::ui::toast::show_transient(
                    &s.chapter_toast, "No journal page for this passage", 3,
                );
                return;
            }
        };
        let work_abbrev = crate::app::base_work_abbrev(
            s.current_work.as_ref().map(|w| w.abbrev.as_str()).unwrap_or(""),
        )
        .to_string();
        (
            work_abbrev,
            ctx.start_citation.clone(),
            ctx.end_citation.clone(),
            ctx.act,
            ctx.scene,
        )
    };

    // Phase 2: look up passage pages.
    let pages = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::journal::find_passage_pages(&conn, &work_abbrev, &start_cit, &end_cit).ok()
        })
        .unwrap_or_default();

    if pages.is_empty() {
        let s = state.borrow();
        crate::ui::toast::show_transient(
            &s.chapter_toast, "No journal page for this passage", 3,
        );
        return;
    }

    // Phase 3: close the gloss overlay and open the journal overlay.
    {
        let mut s = state.borrow_mut();
        s.tts.stop();
        s.gloss_overlay.hide();
        // Restore the saved position so journal return_pos is coherent.
        if let Some((line, top)) = s.gloss_return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
        }
        s.input_mode = crate::app::InputMode::Reader;
    }

    {
        let mut s = state.borrow_mut();
        s.journal.return_pos = Some((s.current_line, s.page_top_line));
        s.journal_band = JournalBand::Passage {
            div1,
            div2,
            start: start_cit,
            end: end_cit,
        };
        s.journal.page_index = 0;
        s.input_mode = InputMode::JournalOverlay;
        render_current(&mut s);
    }
    crate::logging::log("VIEW-JOURNAL-FROM-GLOSS: opened journal passage band from gloss overlay");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_left_scene_shows_abbrev_act_scene() {
        assert_eq!(footer_left_text("2H6", JournalBand::Scene(1, 4)), "2H6 1.4");
    }

    #[test]
    fn footer_left_work_shows_whole_work() {
        assert_eq!(footer_left_text("2H6", JournalBand::Work), "2H6 \u{00b7} whole work");
    }
}

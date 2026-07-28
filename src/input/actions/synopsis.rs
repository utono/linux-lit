//! Synopsis revision flows: from the synopsis overlay, `A` opens a question
//! prompt (augment/explain) and `E` opens an edit prompt (structural rewrite).
//! Both send the instruction + current synopsis to Claude, persist the result
//! to scene_synopses, and support single-level undo with `U`.

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

/// Open the stacked "edit" card below the synopsis card (same widget as the ask
/// card, edit framing). On Ctrl+Enter the typed instruction is sent to Claude
/// with the structural-editor prompt. No-op if a card is already open.
pub(crate) fn show_edit_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    if s.gloss_overlay.ask_is_open() {
        return;
    }
    let scene = s.synopsis_overlay_scene;
    s.gloss_overlay.open_ask_card_with(
        "Edit this scene",
        "Describe the edit (split/merge paragraphs, reword, reorder)  \u{00b7}  Ctrl+Enter submit",
        "",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    drop(s);
    let mut s = state_rc.borrow_mut();
    s.synopsis_amend_scene = scene;
    s.synopsis_prompt_kind = crate::app::SynopsisPromptKind::Edit;
}

/// Close the ask card and return focus to the synopsis card (mode stays
/// `SynopsisOverlay`).
pub(crate) fn close_amend_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().gloss_overlay.close_ask_card();
}

/// Submit the ask card: read its text, close it, and (if non-empty) kick off the
/// async synopsis amend or edit. Called on Ctrl+Enter.
pub(crate) fn submit_amend_prompt(state: &Rc<RefCell<AppState>>) {
    let text = state.borrow().gloss_overlay.take_ask_text();
    let kind = state.borrow().synopsis_prompt_kind;
    close_amend_prompt(state);
    if text.trim().is_empty() {
        return;
    }
    match kind {
        crate::app::SynopsisPromptKind::Ask => amend_synopsis(state, &text),
        crate::app::SynopsisPromptKind::Edit => edit_synopsis(state, &text),
    }
}

/// System prompt for the synopsis EDIT call. Unlike the amend prompt (which
/// answers a reader's question by weaving an explanation in), this one applies
/// the reader's edit instruction literally — split/merge paragraphs, reword,
/// tighten, reorder — while keeping the scene's facts accurate. It returns the
/// FULL revised synopsis (not a diff), in the same <p>-tagged format the
/// synopsis card renders.
const SYNOPSIS_EDIT_PROMPT: &str = "\
You are a careful editor revising a Shakespeare scene synopsis. You will be \
given a play, an act and scene, the current synopsis for that scene, and an \
edit instruction from the reader. Apply the edit instruction faithfully and \
literally (for example: split or merge paragraphs, reword a sentence, tighten \
or expand, reorder events). Preserve the factual accuracy of the scene — do \
not invent events that are not already implied by the current synopsis, and do \
not drop plot points unless the instruction tells you to. Do not add a heading, \
preamble, or commentary about what you changed.\n\n\
FORMAT: Return the FULL revised synopsis split into readable paragraphs, each \
wrapped in <p>...</p> tags, like:\n\
<p>First paragraph of the synopsis.</p>\n\
<p>Second paragraph that continues the action.</p>\n\
Output ONLY the <p>-tagged paragraphs, nothing else.";

/// Send the instruction + current synopsis to Claude, then show and persist the
/// revised synopsis. Shared by the `A` amend flow and the `E` edit flow; the
/// caller supplies the prompt key / fallback prompt / log verb. Mirrors the
/// gloss add async pattern.
fn run_synopsis_revision(
    state_rc: &Rc<RefCell<AppState>>,
    instruction: &str,
    prompt_key: &'static str,
    fallback_prompt: &'static str,
    log_verb: &'static str,
) {
    let (div1, div2) = state_rc.borrow().synopsis_amend_scene;

    let (work_title, work_abbrev, head_work, original, model, label) = {
        let s = state_rc.borrow();
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return,
        };
        let abbrev = work.canonical_abbrev.clone();
        let original = match s.synopsis_cache.get(&(div1, div2)) {
            Some(t) => t.clone(),
            None => return,
        };
        let label = crate::app::scene_synopsis::synopsis_label(&s, div1, div2);
        (
            work.title.clone(),
            abbrev,
            work.abbrev.clone(),
            original,
            s.config.claude_model.clone(),
            label,
        )
    };
    let user_msg = format!(
        "Play: \"{}\"\n{}\n\nCurrent synopsis:\n{}\n\n---\nReader's request: {}",
        work_title, label, original, instruction,
    );

    state_rc.borrow().gloss_overlay.show_loading();

    let system_prompt = crate::db::prompts::active_prompt(prompt_key)
        .unwrap_or_else(|| fallback_prompt.to_string());

    let model_for_db = model.clone();
    let label_err = label.clone();
    let head_work_err = head_work.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt,
        user_msg,
        model,
        move |st, revised| {
            let revised = revised.trim().to_string();
            // Persist (upsert) to lit.db, stamping the authoring model.
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                if let Err(e) = crate::db::queries::save_synopsis(
                    &conn, &work_abbrev, div1, div2, &revised, &model_for_db,
                ) {
                    crate::logging::log(&format!("SYNOPSIS: save error: {}", e));
                }
            }
            let mut s = st.borrow_mut();
            // Remember the pre-revision text so `U` can revert this edit.
            s.synopsis_undo = Some(((div1, div2), original.clone()));
            s.synopsis_cache.insert((div1, div2), revised.clone());
            let cw = s.content_hbox.width();
            let h = crate::app::layout::overlay_card_height(&s);
            let root_color = s.theme.root_color.clone();
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
            s.gloss_overlay.show_synopsis(&head_work, &label, &revised, Some(&root_color), cw, h, prose_card);
            s.synopsis_overlay_scene = (div1, div2);
            crate::input::actions::gloss::recolor_cached_blocks(&s);
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
            crate::logging::log(&format!(
                "SYNOPSIS: {} {} ({},{})",
                log_verb, work_abbrev, div1, div2
            ));
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            let cw = s.content_hbox.width();
            let h = crate::app::layout::overlay_card_height(&s);
            let root_color = s.theme.root_color.clone();
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
            s.gloss_overlay.show_synopsis(&head_work_err, &label_err, msg, Some(&root_color), cw, h, prose_card);
            s.synopsis_overlay_scene = (div1, div2);
            crate::input::actions::gloss::recolor_cached_blocks(&s);
            s.input_mode = crate::app::InputMode::SynopsisOverlay;
        },
    );
}

/// Send the question + current synopsis to Claude (augment/explain). `A` path.
pub(crate) fn amend_synopsis(state_rc: &Rc<RefCell<AppState>>, question: &str) {
    run_synopsis_revision(
        state_rc,
        question,
        "synopsis.amend",
        SYNOPSIS_AMEND_PROMPT,
        "amended",
    );
}

/// Send the instruction + current synopsis to Claude (literal edit). `E` path.
pub(crate) fn edit_synopsis(state_rc: &Rc<RefCell<AppState>>, instruction: &str) {
    run_synopsis_revision(
        state_rc,
        instruction,
        "synopsis.edit",
        SYNOPSIS_EDIT_PROMPT,
        "edited",
    );
}

/// Revert the most recent revision (`A` ask or `E` edit): restore the pre-edit
/// synopsis text in the cache and in lit.db, and redisplay it. No-op (with a
/// toast) if there is nothing to undo. Single-level — only the last revision
/// can be undone.
pub(crate) fn undo_amend(state_rc: &Rc<RefCell<AppState>>) {
    let undo = state_rc.borrow().synopsis_undo.clone();
    let ((div1, div2), original) = match undo {
        Some(u) => u,
        None => {
            crate::input::navigation::show_chapter_toast_secs(&state_rc.borrow(), "Nothing to undo", 2);
            return;
        }
    };

    let work_abbrev = {
        let s = state_rc.borrow();
        s.current_work
            .as_ref()
            .map(|w| w.canonical_abbrev.clone())
    };
    if let Some(abbrev) = work_abbrev {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            // Undo restores the pre-amend text only; leave claude_model as the
            // row's existing value (that model authored the text being restored).
            if let Err(e) =
                crate::db::queries::restore_synopsis_text(&conn, &abbrev, div1, div2, &original)
            {
                crate::logging::log(&format!("SYNOPSIS: undo save error: {}", e));
            }
        }
    }

    let mut s = state_rc.borrow_mut();
    s.synopsis_cache.insert((div1, div2), original.clone());
    s.synopsis_undo = None;
    let cw = s.content_hbox.width();
    let h = crate::app::layout::overlay_card_height(&s);
    let (head_work, head_pos) = crate::app::scene_synopsis::synopsis_head(&s, div1, div2);
    let root_color = s.theme.root_color.clone();
    let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
    s.gloss_overlay.show_synopsis(&head_work, &head_pos, &original, Some(&root_color), cw, h, prose_card);
    s.synopsis_overlay_scene = (div1, div2);
    crate::input::actions::gloss::recolor_cached_blocks(&s);
    crate::logging::log(&format!("SYNOPSIS: undid amend ({},{})", div1, div2));
}

/// `c` in the synopsis overlay: copy a troubleshooting payload for the current
/// scene's synopsis to the clipboard — work abbrev, work_type, scene/chapter
/// key, `scene_synopses.id`, authoring model, and the litdb `api_prompts` key
/// that generates this kind of synopsis (`synopsis.batch` for plays,
/// `synopsis.chapter-tiered` for prose). Enough to run the litdb `synopses`
/// skill's queries without re-deriving anything. When no row exists yet the
/// scene context is still copied (a missing row needs the same key to debug).
pub(crate) fn copy_synopsis_id(state: &Rc<RefCell<AppState>>) {
    let lookup = {
        let s = state.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let label = crate::app::scene_synopsis::synopsis_label(&s, div1, div2);
        s.current_work.as_ref().map(|w| {
            (
                w.canonical_abbrev.clone(),
                w.title.clone(),
                w.work_type.clone(),
                div1,
                div2,
                label,
            )
        })
    };
    let Some((abbrev, title, work_type, div1, div2, label)) = lookup else { return };

    let info = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::synopsis_debug_info(&conn, &abbrev, div1, div2).ok()
        })
        .flatten();

    let prompt_key = if work_type == "play" {
        "synopsis.batch"
    } else {
        "synopsis.chapter-tiered"
    };
    let (id_line, model_line, tiers_line) = match &info {
        Some(i) => (
            i.id.to_string(),
            i.claude_model.clone().unwrap_or_else(|| "unset".into()),
            if i.has_tiers { "present" } else { "absent" }.to_string(),
        ),
        None => ("none (no scene_synopses row)".into(), "n/a".into(), "n/a".into()),
    };
    let payload = format!(
        "work_abbrev: {}\n\
         title: {}\n\
         work_type: {}\n\
         scene: {} (div1={}, div2={})\n\
         synopsis_id: {}\n\
         claude_model: {}\n\
         tiered gist/precis/account: {}\n\
         batch prompt key (api_prompts): {}\n\
         in-app revision prompt keys: synopsis.amend / synopsis.edit\n\
         table: scene_synopses in ~/utono/litdb/data/lit.db\n\
         skill: ~/utono/litdb/.claude/skills/synopses\n",
        abbrev, title, work_type, label, div1, div2, id_line, model_line, tiers_line, prompt_key,
    );
    crate::ui::copy_to_clipboard(&payload);
    let msg = match info {
        Some(i) => {
            crate::logging::log(&format!(
                "SYNOPSIS: copied debug info (id {}) to clipboard",
                i.id
            ));
            format!("Copied synopsis debug info (id {})", i.id)
        }
        None => {
            crate::logging::log("SYNOPSIS: copied debug info (no row) to clipboard");
            "Copied synopsis debug info (no row)".to_string()
        }
    };
    crate::input::navigation::show_chapter_toast_secs(&state.borrow(), &msg, 2);
}

/// `e` in the synopsis overlay: enter the in-place modal vim editor on the
/// current scene's RAW synopsis text (`synopsis_cache[(div1,div2)]`). Uses the
/// SAME `GlossOverlay` editor as gloss-edit; the save path (`vim_save`) branches
/// to the synopsis persistence. No-op + toast if no cached synopsis.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let (div1, div2) = s.synopsis_overlay_scene;
    let raw = match s.synopsis_cache.get(&(div1, div2)) {
        Some(t) => t.clone(),
        None => {
            crate::input::navigation::show_chapter_toast_secs(&s, "No synopsis to edit", 2);
            return;
        }
    };
    let (fill, fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.gloss_overlay.enter_edit_buffer(&raw, &fill, &fg);
    s.input_mode = crate::app::InputMode::GlossEdit;
}

/// Re-render the synopsis card for `(div1,div2)` from `text` (the colored/formatted
/// display). Mirrors the render block in `run_synopsis_revision`'s success
/// callback. Caller holds no borrow.
fn render_synopsis(state: &Rc<RefCell<AppState>>, div1: i64, div2: i64, text: &str) {
    let s = state.borrow_mut();
    let (head_work, head_pos) = crate::app::scene_synopsis::synopsis_head(&s, div1, div2);
    let cw = s.content_hbox.width();
    let h = crate::app::layout::overlay_card_height(&s);
    let root_color = s.theme.root_color.clone();
    let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
    s.gloss_overlay
        .show_synopsis(&head_work, &head_pos, text, Some(&root_color), cw, h, prose_card);
    crate::input::actions::gloss::recolor_cached_blocks(&s);
}

/// Save the synopsis vim-editor buffer's raw text to lit.db as-is (no Claude) via
/// `save_synopsis` (upsert), snapshot `synopsis_undo`, update `synopsis_cache`,
/// and re-render the colored card. `:w` stays + re-seeds; `:wq` exits.
pub(crate) fn vim_save(state: &Rc<RefCell<AppState>>, quit: bool) {
    let raw = state.borrow().gloss_overlay.edit_buffer_text();
    let raw = raw.trim_end().to_string();
    let (div1, div2, abbrev, model, original) = {
        let s = state.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        };
        let original = s
            .synopsis_cache
            .get(&(div1, div2))
            .cloned()
            .unwrap_or_default();
        (div1, div2, abbrev, s.config.claude_model.clone(), original)
    };
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Err(e) = crate::db::queries::save_synopsis(&conn, &abbrev, div1, div2, &raw, &model) {
            crate::logging::log(&format!("SYNOPSIS: vim save error: {}", e));
        }
    }
    {
        let mut s = state.borrow_mut();
        s.synopsis_undo = Some(((div1, div2), original));
        s.synopsis_cache.insert((div1, div2), raw.clone());
    }
    if quit {
        state.borrow().gloss_overlay.exit_edit_buffer();
        render_synopsis(state, div1, div2, &raw);
        let mut s = state.borrow_mut();
        s.input_mode = crate::app::InputMode::SynopsisOverlay;
        crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED, 2);
    } else {
        state.borrow().gloss_overlay.reseed_edit_buffer(&raw);
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), crate::input::navigation::TOAST_SAVED_IN_OVERLAY, 2);
    }
}

/// Leave the synopsis vim editor. Warn + STAY on a dirty buffer unless `force`.
/// Re-renders the stored (un-edited) synopsis on exit.
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().gloss_overlay.edit_is_dirty();
    if dirty && !force {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), "Unsaved changes \u{2014} :w to save, :q! to discard", 3);
        return;
    }
    let (div1, div2, stored) = {
        let s = state.borrow();
        let (div1, div2) = s.synopsis_overlay_scene;
        let stored = s
            .synopsis_cache
            .get(&(div1, div2))
            .cloned()
            .unwrap_or_default();
        (div1, div2, stored)
    };
    state.borrow().gloss_overlay.exit_edit_buffer();
    render_synopsis(state, div1, div2, &stored);
    state.borrow_mut().input_mode = crate::app::InputMode::SynopsisOverlay;
}

/// `R` in the synopsis overlay read view: open the ask-Claude synopsis rewrite
/// prompt — entering the `e` editor first is unnecessary (mirrors journal/gloss
/// `begin_rewrite`; the vim editor's `R` was dropped 2026-07-22). Opens in
/// INSERT: a rewrite instruction is always typed fresh, so skip vim-NORMAL
/// (fed through the engine so the mirror and `-- INSERT --` hint stay
/// truthful).
pub(crate) fn begin_rewrite(state: &Rc<RefCell<AppState>>) {
    show_edit_prompt(state);
    let _ = state
        .borrow()
        .gloss_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}

/// Alt+g in the synopsis card: open the gloss overlay for the whole work, with
/// the glosses for the currently-displayed scene shown first. Mirrors the state
/// setup that `navigate_gloss_passage` relies on, so Ctrl+n/p (across passages
/// having a gloss of the current type) and Alt+n/p (within a passage) work
/// afterwards.
pub(crate) fn open_work_glosses(state_rc: &Rc<RefCell<AppState>>) {
    let (div1, div2) = state_rc.borrow().synopsis_overlay_scene;
    let work_abbrev = {
        let s = state_rc.borrow();
        match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
            None => return,
        }
    };

    // Load every glossed passage for the work (reading order), then rotate so the
    // current scene's passages come first.
    let mut passages = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::queries::find_glossed_passages(
            &conn, &work_abbrev, &["teacher-generic", "inner-monologue", "reader-gloss"],
        ).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if passages.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&state_rc.borrow(), "No glosses for this work", 3);
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
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).ok()
        })
        .unwrap_or_default();
    if all_glosses.is_empty() {
        return;
    }

    let mut s = state_rc.borrow_mut();
    let work_title = s.current_work.as_ref().map(|w| w.title.clone()).unwrap_or_default();
    let work_type = s.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
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
        work_type,
    };

    let cw = s.content_hbox.width();
    let h = crate::app::layout::overlay_card_height(&s);
    let gloss_text = all_glosses[0].gloss_text.clone();
    let head = crate::app::scene_synopsis::synopsis_head(&s, ctx.act, ctx.scene);
    s.gloss_overlay.show_gloss_with_color(
        &ctx.source_text, &gloss_text, cw, h, Some(&s.theme.root_color), &[], (&head.0, &head.1),
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
        s.gloss_return_pos = Some((s.current_line, s.page_top.line(), s.page_top.offset()));
    }
    s.input_mode = crate::app::InputMode::GlossOverlay;
    crate::logging::log(&format!("SYNOPSIS: opened work glosses, scene ({},{}) first", div1, div2));
}

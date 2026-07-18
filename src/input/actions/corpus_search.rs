//! Ctrl+f cross-corpus regex search popup: open (load both corpora), and the
//! select handler that jumps to the chosen entry with the match highlighted
//! (n/N step within the entry). No new engine: `select` wires the existing
//! cross-work load (concordance.rs), overlay-open (gloss/journal), and
//! overlay-search seed (overlay_search) paths together.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::EditableExt;

use crate::app::{AppState, InputMode};
use crate::input::corpus_search::{Corpus, CorpusHit};

/// The three gloss types a passage's gloss list is built from (mirrors
/// `open_gloss_at_cursor` / the gloss picker). Corpus-search glosses are all
/// `reader-gloss`, but load the full set so positioning on the target id works
/// even when a passage carries other gloss types too.
const GLOSS_TYPES: &[&str] = &["teacher-generic", "inner-monologue", "reader-gloss"];

/// Open the popup. Loads both corpora once, defaults corpus to the opening
/// context's kind (gloss/journal overlay) or the last-used corpus otherwise.
pub fn open(state: &Rc<RefCell<AppState>>) {
    let (journal, gloss) = {
        let conn = crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
        let j = crate::db::journal::list_all_journal_rows(&conn).unwrap_or_default();
        let g = crate::db::queries::list_all_gloss_rows(&conn).unwrap_or_default();
        (j, g)
    };
    let mut s = state.borrow_mut();
    let corpus = match s.input_mode {
        InputMode::GlossOverlay => Corpus::Gloss,
        InputMode::JournalOverlay => Corpus::Journal,
        _ => s.last_corpus,
    };
    // Remember where to return on Escape.
    s.corpus_search_return_mode = s.input_mode;
    s.corpus_search_popup.set_rows(journal, gloss);
    s.corpus_search_popup.set_corpus(corpus);
    s.corpus_search_popup.search_entry().set_text(""); // emits changed (guarded)
    s.corpus_search_popup.populate_list("");
    s.corpus_search_popup.show();
    s.input_mode = InputMode::CorpusSearch;
}

/// Enter on a result: hide the popup, load the hit's work if it isn't current,
/// open the matching overlay on that entry, and seed the overlay's `/` search
/// with the popup pattern so the match highlights (n/N step within the entry).
///
/// Cross-work loads follow the concordance CROSS-WORK template (async
/// `load_work` + `display_work_at_with_prepared`); the overlay open re-uses the
/// gloss cursor-open assembly (glosses) and the term-filter single-entry render
/// (journal). Empty list → no-op; a failed load toasts and stays put.
pub fn select(state: &Rc<RefCell<AppState>>) {
    let (hit, pattern) = {
        let s = state.borrow();
        let Some(hit) = s.corpus_search_popup.selected_hit() else {
            // Empty list / no selection: close the popup and restore the mode we
            // opened from, exactly like Escape.
            drop(s);
            let mut s = state.borrow_mut();
            s.corpus_search_popup.hide();
            s.input_mode = s.corpus_search_return_mode;
            return;
        };
        (hit, s.corpus_search_popup.search_entry().text().to_string())
    };

    // Close the popup and record the chosen corpus for the next open's default.
    let current_canonical = {
        let mut s = state.borrow_mut();
        s.corpus_search_popup.hide();
        s.last_corpus = hit.corpus;
        s.current_work.as_ref().map(|w| w.canonical_abbrev.clone())
    };

    // Corpus rows are stored under the canonical (variant-base) abbrev, so
    // `hit.work_abbrev` is already canonical — compare it to the current work's
    // canonical abbrev to decide whether a cross-work load is needed.
    let same_work = current_canonical.as_deref() == Some(hit.work_abbrev.as_str());

    if same_work {
        open_hit(state, &hit, &pattern);
        return;
    }

    // Cross-work: load the hit's work off the reader thread, then open the
    // overlay on the entry (mirrors concordance.rs CROSS-WORK jump).
    let abbrev = hit.work_abbrev.clone();
    let abbrev_for_load = abbrev.clone();
    let state_clone = Rc::clone(state);
    let handle = state.borrow().tokio_handle.clone();
    glib::spawn_future_local(async move {
        let result = handle
            .spawn_blocking(move || {
                let conn =
                    crate::db::queries::open_db().expect(crate::db::queries::OPEN_DB_PANIC_MSG);
                let work = crate::db::queries::load_work(&conn, &abbrev_for_load)?;
                let prepared = crate::app::text_prep::prepare_text_for_display(&work);
                Ok::<_, rusqlite::Error>((work, prepared))
            })
            .await;
        match result {
            Ok(Ok((work, prepared))) => {
                {
                    let mut s = state_clone.borrow_mut();
                    s.skip_mpv_discovery = true;
                    crate::app::clear_display(&mut s);
                    // No cursor target: the reader is behind the overlay; the
                    // overlay's Escape restores the pre-jump reader position.
                    crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                }
                open_hit(&state_clone, &hit, &pattern);
            }
            _ => {
                let s = state_clone.borrow();
                crate::input::navigation::show_chapter_toast_secs(
                    &s,
                    &format!("Could not load {}", abbrev),
                    3,
                );
            }
        }
    });
}

/// Open the overlay for `hit` (its work must already be the current work) and
/// seed the overlay `/` search with `pattern`. Dispatches on the hit's corpus.
fn open_hit(state: &Rc<RefCell<AppState>>, hit: &CorpusHit, pattern: &str) {
    match hit.corpus {
        Corpus::Gloss => open_gloss_hit(state, hit.entry_id, pattern),
        Corpus::Journal => open_journal_hit(state, hit.entry_id, pattern),
    }
}

/// Open the gloss overlay on gloss `gloss_id` and seed the overlay search.
/// Mirrors `open_gloss_at_cursor`'s assembly (find_glossed_passages +
/// find_glosses_by_start + open_gloss_overlay), but resolves the passage from
/// the gloss id rather than the cursor line.
fn open_gloss_hit(state: &Rc<RefCell<AppState>>, gloss_id: i64, pattern: &str) {
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c,
        Err(_) => {
            let s = state.borrow();
            crate::input::navigation::show_chapter_toast_secs(&s, "Could not open gloss", 3);
            return;
        }
    };
    // The passage that owns this gloss (work_abbrev + start_citation + source).
    let passage = match crate::db::queries::find_gloss_passage_by_id(&conn, gloss_id) {
        Ok(Some(p)) => p,
        _ => {
            let s = state.borrow();
            crate::input::navigation::show_chapter_toast_secs(&s, "Gloss no longer exists", 3);
            return;
        }
    };

    // The full list of glossed passages for this work, and this passage's index
    // within it (open_gloss_overlay wants both, for Ctrl+n/p passage stepping).
    let passages =
        crate::db::queries::find_glossed_passages(&conn, &passage.work_abbrev, GLOSS_TYPES)
            .unwrap_or_default();
    let passage_index = passages
        .iter()
        .position(|p| p.passage_id == passage.passage_id)
        .unwrap_or(0);

    // Every gloss anchored to this passage's start line (the gloss overlay's
    // Ctrl+n/p list within a passage), so we can land on the searched gloss id.
    let all_glosses = crate::db::queries::find_glosses_by_start(
        &conn,
        &passage.work_abbrev,
        &passage.start_citation,
        GLOSS_TYPES,
    )
    .unwrap_or_default();
    if all_glosses.is_empty() {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "Gloss no longer exists", 3);
        return;
    }
    let target_idx = all_glosses
        .iter()
        .position(|g| g.gloss_id == gloss_id)
        .unwrap_or(0);

    let mut s = state.borrow_mut();
    // No saved reader position to restore beyond whatever the load left; keep
    // the current reader page so Escape lands sensibly.
    s.gloss_return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    // Open the overlay. from_picker=true so Escape returns to the reader (the
    // popup is gone), not a hidden overlay.
    crate::input::actions::gloss::open_gloss_overlay(
        &mut s,
        passages,
        passage_index,
        passage,
        all_glosses,
        true,
        Some("reader-gloss"),
    );
    // open_gloss_overlay lands on its default gloss for the passage (the
    // reader-gloss); if the searched gloss is a different row in this passage,
    // re-render on the exact target id.
    if s.gloss_index != target_idx {
        s.gloss_index = target_idx;
        s.gloss_active_voice = 0;
        crate::input::actions::gloss::render_gloss_row(&mut s, target_idx);
    }

    // Seed the overlay `/` search so the match highlights and n/N step within
    // this gloss. Collect against the WHOLE rendered gloss text (every page).
    let text = s.gloss_overlay.whole_entry_text();
    let search = crate::input::overlay_search::OverlaySearch {
        pattern: pattern.to_string(),
        matches: crate::input::overlay_search::collect(&text, pattern),
        current: 0,
    };
    s.gloss_overlay.set_search_matches(&search);
    s.gloss_last_pattern = Some(pattern.to_string());
    s.gloss_search = Some(search);
    s.input_mode = InputMode::GlossOverlay;
}

/// Open the journal overlay on entry `entry_id` and seed the overlay search.
/// Mirrors the term-filter single-entry render (`activate_filter` +
/// `render_filtered_match`): the entry is shown via a one-item `JournalFilter`,
/// which paints it through `show_page` WITHOUT switching `journal_band` /
/// `current_work` — the same cross-work-display trick term browse uses.
fn open_journal_hit(state: &Rc<RefCell<AppState>>, entry_id: i64, pattern: &str) {
    let m = match crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_page_by_id(&conn, entry_id).ok().flatten())
    {
        Some(m) => m,
        None => {
            let s = state.borrow();
            crate::input::navigation::show_chapter_toast_secs(&s, "Entry no longer exists", 3);
            return;
        }
    };

    let mut s = state.borrow_mut();
    // Save the reader position so Escape returns there.
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    // A one-match filter renders exactly this entry via render_filtered_match.
    // The "term" is the search pattern so the footer orients the reader.
    s.journal.filter = Some(crate::input::actions::journal::JournalFilter {
        term: pattern.to_string(),
        matches: vec![m],
        pos: 0,
    });
    s.input_mode = InputMode::JournalOverlay;
    crate::input::actions::journal::render_filtered_match(&mut s);

    // Seed the overlay `/` search against the just-rendered entry's whole text
    // so the match highlights and n/N step within the entry. render_filtered_match
    // re-applies s.journal.search on every render, so store it before/after — set
    // it now and repaint.
    let text = s.journal_overlay.whole_entry_text();
    let search = crate::input::overlay_search::OverlaySearch {
        pattern: pattern.to_string(),
        matches: crate::input::overlay_search::collect(&text, pattern),
        current: 0,
    };
    s.journal_overlay.set_search_matches(&search);
    s.journal.last_pattern = Some(pattern.to_string());
    s.journal.search = Some(search);
}

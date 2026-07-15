//! Read-only browsing of a journal/gloss entry's stored rewrite revisions
//! (Ctrl+Shift+n/p) and restore of the viewed version (Ctrl+Shift+r). Browsing
//! NEVER mutates the live entry (no DB write, no patch of `gloss_list` /
//! `journal.pages`); only `browse_restore` writes.
//!
//! The virtual position list is `[revisions[0], …, revisions[n-1], HEAD]` where
//! HEAD is the current live entry. `pos == revisions.len()` means HEAD.
//! `Ctrl+Shift+p` steps toward older (pos-1), `Ctrl+Shift+n` toward newer
//! (pos+1); both clamp at the ends. The rendered version's diff-highlight marks
//! `changed_ranges(body_at(pos-1), body_at(pos))` (nothing at pos==0).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::AppState;
use crate::db::journal::Revision;

/// An active read-only revision-browse session for one journal Q&A or gloss.
pub struct RewriteBrowse {
    /// "journal" | "gloss" — the `kind` key into `rewrite_revisions`.
    pub kind: &'static str,
    /// The live entry's id (journal page id / gloss id).
    pub entry_id: i64,
    /// Stored revisions, oldest → newest (HEAD is a synthetic extra position).
    pub revisions: Vec<Revision>,
    /// Position in `[0..=revisions.len()]`; `== revisions.len()` means HEAD.
    pub pos: usize,
    /// The live head's question (journal only; `None` for gloss).
    pub head_question: Option<String>,
    /// The live head's body: journal answer, or gloss RAW markup.
    pub head_body: String,
}

impl RewriteBrowse {
    /// Length of the virtual list (`revisions` + the synthetic HEAD slot).
    fn virtual_len(&self) -> usize {
        self.revisions.len() + 1
    }

    /// True when `pos` points at the synthetic HEAD (the live entry).
    fn is_head(&self) -> bool {
        self.pos == self.revisions.len()
    }

    /// Body at virtual position `i` (`revisions.len()` → HEAD).
    fn body_at(&self, i: usize) -> &str {
        if i == self.revisions.len() {
            &self.head_body
        } else {
            &self.revisions[i].body
        }
    }

    /// Question at virtual position `i` (journal). `None` at HEAD falls back to
    /// `head_question`; a revision's stored question falls back to head if the
    /// row stored no question.
    fn question_at(&self, i: usize) -> Option<&str> {
        if i == self.revisions.len() {
            self.head_question.as_deref()
        } else {
            self.revisions[i]
                .question
                .as_deref()
                .or(self.head_question.as_deref())
        }
    }
}

/// Pure clamped step over the virtual list `[0..=len]` (len == revisions.len(),
/// so the valid range is `0..=len` with `len` being HEAD). `forward` moves
/// toward newer (higher index); both ends clamp. Returned unit-tested.
fn step_pos(pos: usize, len: usize, forward: bool) -> usize {
    if forward {
        (pos + 1).min(len)
    } else {
        pos.saturating_sub(1)
    }
}

/// Ctrl+Shift+n (`forward = true`, newer) / Ctrl+Shift+p (older). Lazily opens
/// the browse session on the first press (loading `list_revisions` for the
/// current entry), then steps and re-renders read-only. When browse ends at
/// HEAD after a step-from-HEAD forward (a no-op clamp) nothing changes.
pub fn browse_step(state: &Rc<RefCell<AppState>>, forward: bool) {
    // Lazily open the session if none is active.
    if state.borrow().rewrite_browse.is_none() {
        if !open_browse(state) {
            return; // toast already shown (no revisions / no entry)
        }
    }

    // Step within the virtual list (clamped).
    let new_pos = {
        let mut s = state.borrow_mut();
        let Some(b) = s.rewrite_browse.as_mut() else {
            return;
        };
        let len = b.revisions.len();
        b.pos = step_pos(b.pos, len, forward);
        b.pos
    };

    render_position(state, new_pos);
}

/// Ctrl+Shift+r: promote the currently-viewed revision to head. Only valid while
/// browsing and NOT on HEAD. Appends the current head as a new revision first
/// (so nothing is lost), writes the viewed body via the normal update fn, ends
/// the browse, and re-renders the live entry.
pub fn browse_restore(state: &Rc<RefCell<AppState>>) {
    // Gather everything needed under a short borrow.
    let plan = {
        let s = state.borrow();
        let Some(b) = s.rewrite_browse.as_ref() else {
            return; // not browsing: no toast, nothing user-meaningful happened
        };
        if b.is_head() {
            crate::input::navigation::show_chapter_toast_secs(&s, "Already on current version", 2);
            return;
        }
        let model = current_model(&s);
        Some((
            b.kind,
            b.entry_id,
            b.head_question.clone(),
            b.head_body.clone(),
            b.question_at(b.pos).map(|q| q.to_string()),
            b.body_at(b.pos).to_string(),
            model,
        ))
    };

    let Some((kind, entry_id, head_question, head_body, view_question, view_body, model)) = plan
    else {
        return;
    };

    // Append the CURRENT head as a revision, then write the viewed body live.
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::append_revision(
            &conn,
            kind,
            entry_id,
            head_question.as_deref(),
            &head_body,
            &model,
            None,
        );
        match kind {
            "journal" => {
                let q = view_question.unwrap_or_default();
                let _ =
                    crate::db::journal::update_journal_page(&conn, entry_id, &q, &view_body, &model);
            }
            "gloss" => {
                let _ = crate::db::queries::update_gloss(&conn, entry_id, &view_body, &model);
            }
            _ => {}
        }
    }

    // Gloss renders from the in-memory gloss_list (render_gloss_live reads
    // gloss_list[gloss_index]), so patch the restored body into memory too —
    // the journal path re-queries the DB in render_current and needs no patch.
    // Mirrors the live rewrite path (gloss.rs: gloss_list[idx].gloss_text = ...).
    if kind == "gloss" {
        let mut s = state.borrow_mut();
        let idx = s.gloss_index;
        if let Some(g) = s.gloss_list.get_mut(idx) {
            g.gloss_text = view_body.clone();
        }
    }

    // End browse and re-render the LIVE entry (which is now the restored body).
    state.borrow_mut().rewrite_browse = None;
    rerender_live(state);
    let s = state.borrow();
    crate::input::navigation::show_chapter_toast_secs(&s, "Restored", 2);
}

/// Open a browse session for the current overlay's displayed entry. Returns
/// false (with a toast) when there is no entry or no stored revisions.
fn open_browse(state: &Rc<RefCell<AppState>>) -> bool {
    let opened = {
        let s = state.borrow();
        match s.input_mode {
            crate::app::InputMode::GlossOverlay => open_browse_gloss(&s),
            crate::app::InputMode::JournalOverlay => open_browse_journal(&s),
            _ => None,
        }
    };
    let Some(browse) = opened else {
        // No current entry OR no revisions: toast and stay put.
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No revision history", 2);
        return false;
    };
    state.borrow_mut().rewrite_browse = Some(browse);
    true
}

/// Build a gloss browse session from the current `gloss_list[gloss_index]`.
fn open_browse_gloss(s: &AppState) -> Option<RewriteBrowse> {
    let gloss = s.gloss_list.get(s.gloss_index)?;
    let entry_id = gloss.gloss_id;
    let head_body = gloss.gloss_text.clone();
    let revisions = crate::db::queries::open_db()
        .ok()
        .and_then(|c| crate::db::journal::list_revisions(&c, "gloss", entry_id).ok())
        .unwrap_or_default();
    if revisions.is_empty() {
        return None;
    }
    let pos = revisions.len(); // HEAD
    Some(RewriteBrowse {
        kind: "gloss",
        entry_id,
        revisions,
        pos,
        head_question: None,
        head_body,
    })
}

/// Build a journal browse session from the displayed journal page.
fn open_browse_journal(s: &AppState) -> Option<RewriteBrowse> {
    let page = crate::input::actions::journal::displayed_journal_page(s)?;
    let entry_id = page.id;
    let revisions = crate::db::queries::open_db()
        .ok()
        .and_then(|c| crate::db::journal::list_revisions(&c, "journal", entry_id).ok())
        .unwrap_or_default();
    if revisions.is_empty() {
        return None;
    }
    let pos = revisions.len(); // HEAD
    Some(RewriteBrowse {
        kind: "journal",
        entry_id,
        revisions,
        pos,
        head_question: Some(page.question.clone()),
        head_body: page.answer.clone(),
    })
}

/// Render the version at virtual position `pos` read-only, with the diff-
/// highlight of `changed_ranges(body_at(pos-1), body_at(pos))`. At `pos == 0`
/// there is no predecessor, so the highlight is cleared. When `pos` is HEAD the
/// live entry is re-rendered normally (leaving browse intact so a further step
/// back re-enters history).
fn render_position(state: &Rc<RefCell<AppState>>, pos: usize) {
    // HEAD position: render the live entry (the current in-memory row).
    let (kind, is_head) = {
        let s = state.borrow();
        match s.rewrite_browse.as_ref() {
            Some(b) => (b.kind, b.is_head()),
            None => return,
        }
    };

    if is_head {
        rerender_live(state);
        show_cue(state);
        return;
    }

    match kind {
        "gloss" => render_gloss_position(state, pos),
        "journal" => render_journal_position(state, pos),
        _ => {}
    }
    show_cue(state);
}

/// Render a stored gloss revision's raw markup into the overlay WITHOUT touching
/// `gloss_list`. To diff two revisions we need the RENDERED buffer text of both
/// (gloss diffs are computed against rendered text, like the rewrite path): the
/// predecessor is rendered first to capture its text, then the target is
/// rendered and the diff applied.
fn render_gloss_position(state: &Rc<RefCell<AppState>>, pos: usize) {
    let s = state.borrow();
    let Some(b) = s.rewrite_browse.as_ref() else {
        return;
    };
    let Some(ctx) = s.gloss_context.as_ref() else {
        return;
    };
    let target_markup = b.body_at(pos).to_string();
    let prev_markup = if pos > 0 {
        Some(b.body_at(pos - 1).to_string())
    } else {
        None
    };
    let idx = s.gloss_index;
    let total = s.gloss_list.len();
    let source_text = ctx.source_text.clone();
    let start_citation = ctx.start_citation.clone();
    let end_citation = ctx.end_citation.clone();
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let pairs = ctx.source_line_pairs();
    let root = s.theme.root_color.clone();

    // Capture the predecessor's RENDERED text for the diff baseline.
    let prev_rendered = prev_markup.map(|m| {
        s.gloss_overlay
            .show_gloss_with_color(&source_text, &m, cw, h, Some(&root), &pairs);
        s.gloss_overlay.buffer_text_for_diff()
    });

    // Render the target revision (the version the user is viewing).
    s.gloss_overlay
        .show_gloss_with_color(&source_text, &target_markup, cw, h, Some(&root), &pairs);
    s.gloss_overlay.set_position(idx, total);
    s.gloss_overlay.set_citation(&start_citation, &end_citation);

    // Diff-highlight vs the predecessor (cleared when there is none).
    if let Some(prev) = prev_rendered {
        let new_rendered = s.gloss_overlay.buffer_text_for_diff();
        let ranges = crate::input::rewrite_diff::changed_ranges(&prev, &new_rendered);
        s.gloss_overlay.apply_rewrite_diff(&ranges);
    } else {
        s.gloss_overlay.clear_rewrite_diff();
    }
}

/// Render a stored journal revision (question + answer) into the overlay
/// read-only via `show_page`, WITHOUT mutating `journal.pages`. The diff is
/// `changed_ranges(prev_answer, answer)` shifted by the "Q: …\n\n" prefix, the
/// same offset the live rewrite path uses.
fn render_journal_position(state: &Rc<RefCell<AppState>>, pos: usize) {
    let s = state.borrow();
    let Some(b) = s.rewrite_browse.as_ref() else {
        return;
    };
    let question = b.question_at(pos).unwrap_or_default().to_string();
    let answer = b.body_at(pos).to_string();
    let prev_answer = if pos > 0 {
        Some(b.body_at(pos - 1).to_string())
    } else {
        None
    };
    // The overlay footer is position-only ("Q&A n of m"); keep it unchanged by
    // rendering at the current page index / count.
    let page_index = s.journal.page_index;
    let count = s.journal.pages.len().max(1);
    let (cw, h) = crate::app::layout::overlay_card_size(&s);

    s.journal_overlay
        .show_page("", page_index, count, &question, &answer, "qa", cw, h);

    if let Some(prev) = prev_answer {
        let base = journal_answer_prefix_chars(&question);
        let ranges: Vec<(i32, i32)> = crate::input::rewrite_diff::changed_ranges(&prev, &answer)
            .into_iter()
            .map(|(a, b)| (a + base, b + base))
            .collect();
        s.journal_overlay.apply_rewrite_diff(&ranges);
    } else {
        s.journal_overlay.clear_rewrite_diff();
    }
}

/// Char length of the "Q: …\n\n" prefix the journal body renders before the
/// answer (mirrors `journal::answer_prefix_chars`, which is private).
fn journal_answer_prefix_chars(question: &str) -> i32 {
    ("Q: ".chars().count() + question.chars().count() + 2) as i32
}

/// Re-render the LIVE entry (HEAD) so the user is back on the current row. Clears
/// any browse diff-highlight (the live rewrite path re-applies its own if one is
/// pending, but here we simply show the current row with no diff).
fn rerender_live(state: &Rc<RefCell<AppState>>) {
    let mode = state.borrow().input_mode;
    match mode {
        crate::app::InputMode::GlossOverlay => {
            let mut s = state.borrow_mut();
            s.gloss_overlay.clear_rewrite_diff();
            let idx = s.gloss_index;
            render_gloss_live(&mut s, idx);
        }
        crate::app::InputMode::JournalOverlay => {
            let mut s = state.borrow_mut();
            s.journal_overlay.clear_rewrite_diff();
            crate::input::actions::journal::render_current(&mut s);
        }
        _ => {}
    }
}

/// Render the live gloss row at `idx` (the in-memory `gloss_list` markup). A
/// small mirror of `gloss::render_gloss_row` that avoids re-querying the DB.
fn render_gloss_live(s: &mut AppState, idx: usize) {
    let Some(gloss) = s.gloss_list.get(idx) else {
        return;
    };
    let gloss_text = gloss.gloss_text.clone();
    let start_citation = gloss.start_citation.clone();
    let end_citation = gloss.end_citation.clone();
    let Some(ctx) = s.gloss_context.as_ref() else {
        return;
    };
    let source_text = ctx.source_text.clone();
    let (cw, h) = crate::app::layout::overlay_card_size(s);
    let pairs = ctx.source_line_pairs();
    let total = s.gloss_list.len();
    let root = s.theme.root_color.clone();
    s.gloss_overlay
        .show_gloss_with_color(&source_text, &gloss_text, cw, h, Some(&root), &pairs);
    s.gloss_overlay.set_position(idx, total);
    s.gloss_overlay.set_citation(&start_citation, &end_citation);
}

/// The model to attribute the head-preserving revision + restore write to.
fn current_model(s: &AppState) -> String {
    s.config.claude_model.clone()
}

/// Toast the current virtual position as "rev k/N" (HEAD shown as "current").
fn show_cue(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let Some(b) = s.rewrite_browse.as_ref() else {
        return;
    };
    let text = if b.is_head() {
        format!("current ({}/{})", b.virtual_len(), b.virtual_len())
    } else {
        // Human 1-based position within the virtual list.
        format!("rev {}/{}", b.pos + 1, b.virtual_len())
    };
    crate::input::navigation::show_chapter_toast_secs(&s, &text, 2);
}

#[cfg(test)]
mod tests {
    use super::step_pos;

    #[test]
    fn step_forward_clamps_at_head() {
        // len == 2 revisions → virtual positions 0,1,2(HEAD). Forward clamps at 2.
        assert_eq!(step_pos(0, 2, true), 1);
        assert_eq!(step_pos(1, 2, true), 2);
        assert_eq!(step_pos(2, 2, true), 2); // clamp at HEAD
    }

    #[test]
    fn step_backward_clamps_at_zero() {
        assert_eq!(step_pos(2, 2, false), 1);
        assert_eq!(step_pos(1, 2, false), 0);
        assert_eq!(step_pos(0, 2, false), 0); // clamp at oldest
    }

    #[test]
    fn zero_revisions_head_is_pos_zero_and_stuck() {
        // No revisions: virtual list is just [HEAD] at pos 0; both steps clamp.
        assert_eq!(step_pos(0, 0, true), 0);
        assert_eq!(step_pos(0, 0, false), 0);
    }
}

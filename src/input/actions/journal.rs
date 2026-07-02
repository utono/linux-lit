use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};
use crate::ui::journal_move_picker::MoveTargetRow;
use std::cell::RefCell;
use std::rc::Rc;

/// Capitalize the first character of `s` (ASCII), leaving the rest unchanged.
/// Used to turn a unit noun (`chapter`) into a user-message field label
/// (`Chapter:`). Empty input returns empty.
fn titlecase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Prose journal-Q&A context window radius (paragraphs each side of the
/// reader's anchor). Prose divisions can be the whole book, so cap the context.
const PROSE_CONTEXT_RADIUS: usize = 10;

/// The first spoken/stage line of a passage's `<speaker>/<verse>/<stage>` source
/// markup (as built by `build_source_header`), for the Q&A picker to show
/// instead of the question. Returns the inner text of the first `<verse>` or
/// `<stage>` element (speaker labels are chrome, skipped), or `None` if the
/// markup has no such line. Pure — unit-tested.
fn first_passage_line(source_markup: &str) -> Option<String> {
    for line in source_markup.lines() {
        let line = line.trim();
        for tag in ["verse", "stage"] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if let Some(rest) = line.strip_prefix(&open) {
                if let Some(inner) = rest.strip_suffix(&close) {
                    let inner = inner.trim();
                    if !inner.is_empty() {
                        return Some(inner.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Passage context captured from a visual selection, held until the ask-card
/// submit fires (at which point `ask_claude` reads it and clears it).
/// `band` is the Passage band the ask was started on: a cancelled ask leaves
/// the pending value behind (close_prompt keeps it so `r` can re-ask on the
/// same band), so every consumer MUST check `band` against the current
/// `journal_band` — otherwise a stale pending renders (or, worse, persists to
/// lit.db) as a DIFFERENT passage's source.
pub struct PendingPassage {
    pub source_text: String,
    pub band: JournalBand,
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
    /// True when the Q&A picker was opened from the READING CARD (Alt+j) rather
    /// than from inside the journal overlay (Ctrl+\). Set by
    /// `open_picker_from_reader`; consumed by the picker's confirm/escape paths so
    /// Escape returns to the reader (not a hidden journal overlay).
    pub picker_from_reader: bool,
    /// Set by `vim_open_rewrite` (the `R` key in the vim editor) before opening
    /// the ask card: `(page_id, question, answer)` of the CURRENT edit buffer.
    /// Read and consumed by `submit_prompt`, which sends it + the typed
    /// instruction to Claude as a rewrite. `None` for a normal create-ask.
    pub vim_rewrite: Option<(i64, String, String)>,
}

/// Resolve which band a stored journal page belongs to, for the Q&A picker. A
/// page is `Work` when its `div1 < 0` (the JOURNAL_WORK_DIV sentinel), and a
/// `Scene` otherwise — INCLUDING passage Q&As, which belong to their
/// `(div1, div2)` scene/chapter band. A passage page therefore resolves to the
/// same `Scene` band as the scene Q&As around it, so `confirm_picker` lands the
/// reader in the merged band (where `render_current` loads scene + passage pages
/// together via `find_scene_band_pages`) and finds the page by id. The picker's
/// "N.N passage" label is computed separately from the ROW's citations, not from
/// the band — see `open_picker`.
fn band_for_page(p: &crate::db::journal::JournalPage) -> JournalBand {
    if p.div1 < 0 {
        JournalBand::Work
    } else {
        JournalBand::Scene(p.div1, p.div2)
    }
}

/// Resolve the band to GROUND a rewrite of a page in (`rewrite_context`). Unlike
/// `band_for_page` (which folds a passage page into its Scene band for *viewing*
/// and *navigation*), this reconstructs the `Passage` band for a passage row so
/// the rewrite context keeps the passage-specific arm (which appends the passage
/// source text). A page is a passage row iff it carries start+end citations.
fn band_for_rewrite(p: &crate::db::journal::JournalPage) -> JournalBand {
    if p.div1 < 0 {
        JournalBand::Work
    } else if let (Some(start), Some(end)) = (p.start_citation.clone(), p.end_citation.clone()) {
        JournalBand::Passage { div1: p.div1, div2: p.div2, start, end }
    } else {
        JournalBand::Scene(p.div1, p.div2)
    }
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

/// Pure core of `move_target_rows`: given the work's unique scene keys in
/// reading order and the entry's current band, return the ordered list of
/// destination bands — whole work first, then each scene — with the current
/// band omitted. Labels are applied by `move_target_rows`.
fn target_bands(scenes: &[(i64, i64)], current: &JournalBand) -> Vec<JournalBand> {
    let mut out = Vec::with_capacity(scenes.len() + 1);
    if *current != JournalBand::Work {
        out.push(JournalBand::Work);
    }
    for &(d1, d2) in scenes {
        let band = JournalBand::Scene(d1, d2);
        if band != *current {
            out.push(band);
        }
    }
    out
}

/// Build the list of move targets for the current entry: every band it could be
/// moved to (whole work + every scene/chapter in the work), excluding its
/// current band. Scene keys come from `work.lines` (unique (div1,div2) in
/// reading order — the same source the synopsis picker uses), unfiltered, so
/// every scene is offered even if it has no Q&A yet. Labels via `synopsis_label`.
fn move_target_rows(s: &AppState, current: &JournalBand) -> Vec<MoveTargetRow> {
    let scenes: Vec<(i64, i64)> = match s.current_work.as_ref() {
        Some(work) => {
            let mut seen = std::collections::HashSet::new();
            let mut keys = Vec::new();
            for line in &work.lines {
                let k = (line.div1, line.div2);
                if seen.insert(k) {
                    keys.push(k);
                }
            }
            keys
        }
        None => Vec::new(),
    };

    target_bands(&scenes, current)
        .into_iter()
        .map(|band| {
            let label = match band {
                JournalBand::Work => "whole work".to_string(),
                JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::synopsis_label(s, d1, d2),
                // target_bands never yields Passage; map defensively.
                JournalBand::Passage { div1, div2, .. } => format!("{}.{} passage", div1, div2),
            };
            MoveTargetRow { band, label }
        })
        .collect()
}

/// Load the current band's pages from the DB into `journal.pages`, clamp the
/// index, and render the current page (or the empty-band card).
pub(crate) fn render_current(s: &mut AppState) {
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone())
        .unwrap_or_default();

    let conn = crate::db::queries::open_db().ok();
    // The overlay has no title header anymore (the footer identifies the work +
    // chapter), so each band only needs its page list.
    let pages = match s.journal_band.clone() {
        JournalBand::Work => conn
            .and_then(|c| crate::db::journal::find_work_pages(&c, &work_abbrev).ok())
            .unwrap_or_default(),
        JournalBand::Scene(d1, d2) => conn
            // A scene/chapter band holds BOTH its scene Q&As and the passage
            // Q&As anchored in the same (div1, div2) — `find_scene_band_pages`
            // merges them so Ctrl+n/p pages through all of a chapter's Q&As.
            .and_then(|c| crate::db::journal::find_scene_band_pages(&c, &work_abbrev, d1, d2).ok())
            .unwrap_or_default(),
        JournalBand::Passage { start, end, .. } => conn
            .and_then(|c| crate::db::journal::find_passage_pages(&c, &work_abbrev, &start, &end).ok())
            .unwrap_or_default(),
    };

    let count = pages.len();
    if count == 0 {
        s.journal.page_index = 0;
    } else if s.journal.page_index >= count {
        s.journal.page_index = count - 1;
    }

    // Use the authoritative main-card rect for BOTH dimensions. Reading
    // `content_hbox.width()` directly is wrong for prose works: long wrapped Q&A
    // paragraphs stretch the hbox past the card's `width_request`, so the journal
    // card spanned edge-to-edge for novels while plays (already wide) looked
    // correct. `overlay_card_size` mirrors what the reader's card actually shows.
    let (cw, h) = crate::app::layout::overlay_card_size(&s);
    let footer_left = footer_left_text(&work_abbrev, s.journal_band.clone());

    // A passage ask in flight (visual selection → Journal Q&A) on a band with
    // no stored pages yet: render the SELECTED passage source in place of the
    // bare "No pages yet — press r to ask." placeholder, so the reader sees the
    // text they are asking about while the ask card is open. `pending_passage`
    // is consumed by `ask_claude` on submit; stored pages render below as plain
    // Q&As (their source intentionally not reproduced). The band check is the
    // staleness guard: a CANCELLED ask leaves pending_passage behind (so `r`
    // can re-ask on its own band), and without it the stale source rendered on
    // any other empty Passage band (e.g. after D deletes that band's last Q&A).
    if count == 0
        && s.journal
            .pending_passage
            .as_ref()
            .is_some_and(|pp| pp.band == s.journal_band)
    {
        let doc = s
            .journal
            .pending_passage
            .as_ref()
            .map(|pp| pp.source_text.clone())
            .unwrap_or_default();
        s.journal_overlay.show_passage_source(&footer_left, &doc, cw, h);
        s.journal.pages = pages;
        return;
    }

    // Every Q&A — including passage pages — renders as a plain Q&A. The passage
    // source block is intentionally NOT shown: the highlighted source stays in
    // lit.db (`source_text` + citations) for provenance, but a Q&A's rendering
    // never reproduces the source. (Previously passage pages used a verse
    // renderer that printed the source above the answer.)
    let current_page = if count == 0 {
        None
    } else {
        Some(&pages[s.journal.page_index])
    };
    let (q, a) = current_page
        .map(|p| (p.question.clone(), p.answer.clone()))
        .unwrap_or_default();
    s.journal_overlay
        .show_page(&footer_left, s.journal.page_index, count, &q, &a, cw, h);

    s.journal.pages = pages;
    // Color any paragraphs whose TTS MP3 is already cached, like the gloss
    // overlay (must run after the page renders + s.journal.pages is set so the
    // entry id resolves).
    crate::input::actions::gloss::recolor_journal_cached_blocks(s);
}

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        // Recolor the main card BEFORE update_highlight (which re-applies the tint
        // for reader_gloss_lines), so a reader-gloss created/edited in the overlay
        // colors immediately on return.
        crate::app::return_to_reader_mode(&mut s);
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position_resnap(&mut s, pos);
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

/// Pure step+clamp for cross-band Q&A traversal: from flat index `pos`, move by
/// `delta`, clamped to `[0, len-1]`. Returns `None` when the list is empty or the
/// step would not move (already at the work's first/last Q&A — no wrap).
fn flat_step(pos: usize, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = (pos as i64 + delta as i64).clamp(0, len as i64 - 1) as usize;
    if next == pos {
        None
    } else {
        Some(next)
    }
}

/// Switch to `band`, load its pages, and land the viewer on the page with
/// `target_id` (matched by id after the band's pages load). Shared by the Q&A
/// picker confirm and the cross-band `Ctrl+n/p` traversal so both land a page
/// the same way.
fn land_on_page(s: &mut AppState, band: JournalBand, target_id: i64) {
    s.journal_band = band;
    s.journal.page_index = 0;
    render_current(s); // loads the band's pages into s.journal.pages
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == target_id) {
        s.journal.page_index = pos;
        render_current(s);
    }
}

/// Within the CURRENT band (already loaded by a preceding `render_current`),
/// point `page_index` at `target_id` and re-render. Unlike `land_on_page` this
/// does NOT switch bands — it is for after an in-place edit/rewrite, where the
/// band is unchanged but `update_journal_page` bumped the row's timestamp and the
/// band's `timestamp ASC` ordering moved the entry to a new index. A no-op if the
/// id isn't found (defensive). The caller must have rendered the band first.
fn land_on_current_band_id(s: &mut AppState, target_id: i64) {
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == target_id) {
        if pos != s.journal.page_index {
            s.journal.page_index = pos;
            render_current(s);
        }
    }
}

/// `Ctrl+n` / `Ctrl+p`: step through EVERY Q&A in the work, across bands, in the
/// same order the `Ctrl+\` picker uses (`find_all_pages_ordered`: whole-work
/// pages first, then by div1/div2, then timestamp/id; passage Q&As interleave in
/// their scene band). At the last page of a band, `Ctrl+n` rolls into the first
/// Q&A of the next band; `Ctrl+p` symmetrically. Clamped at the work's first /
/// last Q&A (no wrap). Was previously a within-band clamp.
pub(crate) fn nav_page(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let Some(cur_id) = s.journal.pages.get(s.journal.page_index).map(|p| p.id) else {
        return;
    };
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone())
        .unwrap_or_default();
    let all = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_all_pages_ordered(&conn, &work_abbrev).ok())
        .unwrap_or_default();
    let Some(pos) = all.iter().position(|p| p.id == cur_id) else {
        return;
    };
    let Some(next) = flat_step(pos, delta, all.len()) else {
        return; // already at the work's first/last Q&A
    };
    let target = &all[next];
    let band = band_for_page(target);
    let target_id = target.id;
    land_on_page(&mut s, band, target_id);
}

/// Jump to the next/prev scene that has pages (skips empty scenes). Lands on
/// that scene's first page. From the Work band, delta>0 lands on the first
/// scene with pages, delta<0 on the last (the Work band sorts before scenes).
pub(crate) fn nav_scene(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone())
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
/// - Opens the ask card titled "Ask a question about this passage".
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
    let band = JournalBand::Passage { div1, div2, start, end };
    s.journal.pending_passage = Some(PendingPassage {
        source_text,
        band: band.clone(),
    });
    s.journal_band = band;
    s.journal.page_index = 0;
    s.input_mode = crate::app::InputMode::JournalOverlay;
    render_current(&mut s);
    s.journal_overlay.open_ask_card(
        "Ask a question about this passage",
        "Ctrl+Enter submit",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
}

/// Set up the journal overlay for a SCENE (chapter) Q&A and open the ask card.
///
/// Called from the synopsis overlay's `A` key: the synopsis always displays one
/// scene/chapter, so its journal hand-off files the Q&A under that scene's band.
/// Mirrors `begin_passage_ask` but for a Scene band (no `pending_passage`,
/// since there is no passage source markup — the Q&A is scoped to the whole
/// scene/chapter). The caller has already closed the synopsis overlay and set
/// reader mode. The journal labels the band "scene" or "chapter" per work type.
pub(crate) fn begin_scene_ask(state: &Rc<RefCell<AppState>>, div1: i64, div2: i64) {
    let mut s = state.borrow_mut();
    s.journal.return_pos = Some((s.current_line, s.page_top_line));
    s.journal.prompt_mode = JournalPromptMode::Ask;
    s.journal_band = JournalBand::Scene(div1, div2);
    s.journal.page_index = 0;
    s.input_mode = crate::app::InputMode::JournalOverlay;
    render_current(&mut s);
    s.journal_overlay.open_ask_card(
        "Ask a question about this scene",
        "Ctrl+Enter submit",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
}

pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal.prompt_mode = JournalPromptMode::Ask;
    let title = match s.journal_band {
        JournalBand::Work => "Ask a question about the whole work",
        JournalBand::Scene(_, _) => "Ask a question about this scene",
        JournalBand::Passage { .. } => "Ask a question about this passage",
    };
    s.journal_overlay.open_ask_card(
        title,
        "Ctrl+Enter submit",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
}

/// Build the user message for a Ctrl+Enter rewrite. `context` is the band-aware
/// grounding block (work title/author, the band the Q&A now lives in, and the
/// relevant scene/passage text) — the same framing the original ask sends — so a
/// rewrite instruction that references the band (e.g. "now that this is in the
/// work band, broaden the opening") has the context it needs. The question, the
/// current answer, and the instruction follow in "revise this answer" shape.
fn rewrite_user_message(context: &str, question: &str, answer: &str, instruction: &str) -> String {
    format!(
        "{}\n\nOriginal question:\n{}\n\nCurrent answer:\n{}\n\nRevise the answer per this instruction (return only the revised answer):\n{}",
        context, question, answer, instruction,
    )
}

/// Assemble the band-aware grounding context for a journal Q&A, mirroring the
/// framing `ask_claude` sends. For Work the context is just the work header; for
/// Scene/Passage it adds the band label and the windowed scene text (and, for a
/// passage page, the stored source markup). `anchor_work_line` anchors the
/// windowed slice; `passage_source` is the editing page's own `source_text`
/// (empty for non-passage pages).
fn rewrite_context(
    s: &AppState,
    band: &JournalBand,
    work_type: &str,
    anchor_work_line: usize,
    passage_source: &str,
) -> String {
    let (title, author) = match s.current_work.as_ref() {
        Some(w) => (w.title.clone(), w.author.clone()),
        None => (String::new(), String::new()),
    };
    let (_genre, unit, _units) = crate::gloss::genre_unit(work_type);
    let unit_label = titlecase_first(unit);
    match band {
        JournalBand::Work => {
            format!("Work: {} by {}\nThis Q&A is filed under the WHOLE WORK (not a single scene).", title, author)
        }
        JournalBand::Scene(d1, d2) => {
            let scene_text = crate::app::scene_synopsis::scene_text_windowed(
                s, *d1, *d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            );
            format!(
                "Work: {} by {}\nThis Q&A is filed under: {}\n\n{} text:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*d1, *d2), unit_label, scene_text,
            )
        }
        JournalBand::Passage { div1, div2, .. } => {
            let scene_text = crate::app::scene_synopsis::scene_text_windowed(
                s, *div1, *div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            );
            format!(
                "Work: {} by {}\nThis Q&A is filed under a PASSAGE in {}\n\n{} text:\n{}\n\nPassage:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*div1, *div2),
                unit_label, scene_text, passage_source,
            )
        }
    }
}

/// `e` in the journal overlay: enter the in-place modal vim editor on the
/// current page's Q&A (replaces the old edit card). No-op if the band is empty.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let Some(page) = s.journal.pages.get(s.journal.page_index) else {
        return;
    };
    let (q, a) = (page.question.clone(), page.answer.clone());
    let (block_fill, block_fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.journal_overlay.enter_edit_buffer(&q, &a, &block_fill, &block_fg);
    s.input_mode = crate::app::InputMode::JournalEdit;
}

/// Save the current vim-editor buffer's Q&A to the DB as-is (no Claude), snapshot
/// the pre-edit Q&A for the reading-mode `u` undo, re-render, and land back on the
/// saved entry. If `quit`, leave the editor and return to the journal overlay.
/// Called for `:w` (quit=false) and `:wq` (quit=true).
pub(crate) fn vim_save(state: &Rc<RefCell<AppState>>, quit: bool) {
    let (question, answer) = state.borrow().journal_overlay.edit_buffer_qa();
    let q = question.trim().to_string();
    let a = answer.trim().to_string();
    {
        let mut s = state.borrow_mut();
        let undo_snap = s.journal.pages.get(s.journal.page_index).map(|page| {
            (page.id, page.question.clone(), page.answer.clone(), page.claude_model.clone())
        });
        let saved_id = s.journal.pages.get(s.journal.page_index).map(|page| {
            let (id, model) = (page.id, page.claude_model.clone());
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::journal::update_journal_page(&conn, id, &q, &a, &model);
                purge_journal_audio(&conn, id);
            }
            id
        });
        s.journal_undo = undo_snap;
        if quit {
            // Leave the editor, restore the read view, land on the saved entry.
            s.journal_overlay.exit_edit_buffer();
            s.input_mode = crate::app::InputMode::JournalOverlay;
            render_current(&mut s);
            if let Some(id) = saved_id {
                land_on_current_band_id(&mut s, id);
            }
            crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
        } else {
            // Stay in the editor; the buffer is now the saved baseline so the
            // dirty-check resets. Re-seed the seed to the just-saved buffer.
            crate::ui::toast::show_transient(&s.chapter_toast, "Saved (:q to exit)", 2);
        }
    }
    // After a non-quit `:w`, reset the editor's dirty baseline to the saved text
    // by re-entering with the saved Q&A (keeps the buffer, resets the seed).
    if !quit {
        let s = state.borrow();
        s.journal_overlay.reseed_edit_buffer(&q, &a);
    }
}

/// Leave the vim editor. With unsaved changes and not `force`, warn and STAY
/// (vim semantics: `:q` on a modified buffer is refused; `:q!` forces). Called
/// for `:q` (force=false), Esc-in-Normal (force=false), and `:q!` (force=true).
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().journal_overlay.edit_is_dirty();
    if dirty && !force {
        crate::ui::toast::show_transient(
            &state.borrow().chapter_toast,
            "Unsaved changes \u{2014} :w to save, :q! to discard",
            3,
        );
        return;
    }
    let mut s = state.borrow_mut();
    s.journal_overlay.exit_edit_buffer();
    s.input_mode = crate::app::InputMode::JournalOverlay;
    render_current(&mut s);
}

/// `R` in the vim editor: persist the current buffer, then open the ask card to
/// collect a rewrite instruction for Claude. Stashes the current `(id, q, a)` in
/// `journal.vim_rewrite` so `submit_prompt` sends them with the instruction.
pub(crate) fn vim_open_rewrite(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    let (question, answer) = state.borrow().journal_overlay.edit_buffer_qa();
    let q = question.trim().to_string();
    let a = answer.trim().to_string();
    let id = {
        let s = state.borrow();
        s.journal.pages.get(s.journal.page_index).map(|p| p.id)
    };
    let Some(id) = id else { return };
    {
        let mut s = state.borrow_mut();
        // Persist the current hand-edits and re-render the READ page first, so the
        // Q&A is visible (and clean) behind the rewrite prompt — and so Esc from
        // the prompt lands on the rendered page, not a blank or half-edited one.
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let model = s
                .journal
                .pages
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.claude_model.clone())
                .unwrap_or_default();
            let _ = crate::db::journal::update_journal_page(&conn, id, &q, &a, &model);
            purge_journal_audio(&conn, id);
        }
        // Leave the editor (the ask card uses the journal overlay's ask_host and
        // its own Tab/Ctrl+Enter/Esc intercept in handle_journal_key).
        s.journal_overlay.exit_edit_buffer();
        s.input_mode = crate::app::InputMode::JournalOverlay;
        render_current(&mut s);
        land_on_current_band_id(&mut s, id);
        s.journal.vim_rewrite = Some((id, q, a));
    }
    let s = state.borrow();
    s.journal_overlay.open_ask_card(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} Esc cancel",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
}

/// Undo the last `e` journal edit (single-level): restore the snapshot in
/// `journal_undo` (the pre-edit question/answer/model) to its page via
/// `update_journal_page`, purge that page's cached TTS, re-render the band, land
/// on the restored page, and clear the snapshot. Toasts and bails when there is
/// nothing to undo. Called by the `u` undo confirmation (`y`).
pub(crate) fn undo_journal_edit(state: &Rc<RefCell<AppState>>) {
    let snapshot = state.borrow().journal_undo.clone();
    let (id, question, answer, model) = match snapshot {
        Some(snap) => snap,
        None => {
            crate::ui::toast::show_transient(&state.borrow().chapter_toast, "Nothing to undo", 2);
            return;
        }
    };

    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Err(e) = crate::db::journal::update_journal_page(&conn, id, &question, &answer, &model) {
            crate::logging::log(&format!("JOURNAL: undo edit failed: {}", e));
        }
        // The answer reverted -> the cached per-paragraph TTS is stale.
        purge_journal_audio(&conn, id);
    }

    let mut s = state.borrow_mut();
    s.journal_undo = None;
    render_current(&mut s);
    // The restore bumped the row's timestamp; re-find it so the band's ordering
    // doesn't leave the view on a different page (see land_on_current_band_id).
    land_on_current_band_id(&mut s, id);
    crate::ui::toast::show_transient(&s.chapter_toast, "Undid edit", 2);
}

pub(crate) fn close_prompt(state: &Rc<RefCell<AppState>>) {
    // Discard any pending vim rewrite so a later create-ask isn't mistaken for a
    // rewrite (Esc out of the `R` prompt; the hand-edits were already saved).
    state.borrow_mut().journal.vim_rewrite = None;
    state.borrow().journal_overlay.close_ask_card();
}

pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let text = state.borrow().journal_overlay.take_ask_text();
    // If a vim-editor rewrite is pending (`R`), this ask text is a REWRITE
    // instruction for the stashed (id, q, a) — not a new-Q&A question.
    let rewrite = state.borrow_mut().journal.vim_rewrite.take();
    close_prompt(state);
    if let Some((id, question, answer)) = rewrite {
        let instruction = text.trim();
        if instruction.is_empty() {
            // Empty instruction → just save the hand-edits as-is.
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let model = state
                    .borrow()
                    .journal
                    .pages
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.claude_model.clone())
                    .unwrap_or_default();
                let _ = crate::db::journal::update_journal_page(
                    &conn, id, question.trim(), answer.trim(), &model,
                );
                purge_journal_audio(&conn, id);
            }
            let mut s = state.borrow_mut();
            render_current(&mut s);
            land_on_current_band_id(&mut s, id);
            crate::ui::toast::show_transient(&s.chapter_toast, "Saved as-is", 2);
            return;
        }
        rewrite_with_claude(state, id, &question, &answer, instruction);
        return;
    }
    if text.trim().is_empty() {
        return;
    }
    ask_claude(state, &text);
}

/// Send a journal Q&A `(question, answer)` plus a rewrite `instruction` to Claude,
/// save the revised answer to row `id`, and re-render. Factored from
/// `submit_edit_rewrite` so the vim editor's `R` path reuses the exact grounding
/// context + save + undo-snapshot behavior.
fn rewrite_with_claude(
    state: &Rc<RefCell<AppState>>,
    id: i64,
    question: &str,
    answer: &str,
    instruction: &str,
) {
    let (model, context, work_type, prev_question, prev_answer) = {
        let s = state.borrow();
        let Some(p) = s.journal.pages.iter().find(|p| p.id == id) else {
            return;
        };
        let model = if p.claude_model.is_empty() {
            s.config.claude_model.clone()
        } else {
            p.claude_model.clone()
        };
        let work_type = s
            .current_work
            .as_ref()
            .map(|w| w.work_type.clone())
            .unwrap_or_default();
        let passage_source = p.source_text.clone().unwrap_or_default();
        let band = band_for_rewrite(p);
        let anchor_work_line = s
            .journal
            .return_pos
            .and_then(|(buf, _top)| s.work_line_for_buffer(buf))
            .unwrap_or(0);
        let context = rewrite_context(&s, &band, &work_type, anchor_work_line, &passage_source);
        (model, context, work_type, p.question.clone(), p.answer.clone())
    };
    let question_owned = question.to_string();
    let model_for_db = model.clone();
    let user_msg = rewrite_user_message(&context, question, answer, instruction);

    crate::ui::toast::show_persistent(&state.borrow().chapter_toast, "Rewriting\u{2026}");

    crate::input::actions::claude_bridge::run_claude_request(
        state,
        crate::gloss::journal_qa_prompt(&work_type),
        user_msg,
        model,
        move |st, revised| {
            st.borrow_mut().journal_undo = Some((
                id,
                prev_question.clone(),
                prev_answer.clone(),
                model_for_db.clone(),
            ));
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                if let Err(e) = crate::db::journal::update_journal_page(
                    &conn, id, &question_owned, &revised, &model_for_db,
                ) {
                    crate::logging::log(&format!("JOURNAL: vim-rewrite save failed: {}", e));
                }
                purge_journal_audio(&conn, id);
            }
            let mut s = st.borrow_mut();
            render_current(&mut s);
            land_on_current_band_id(&mut s, id);
            crate::ui::toast::show_transient(&s.chapter_toast, "Rewritten", 2);
        },
        move |st, msg| {
            let s = st.borrow();
            crate::ui::toast::show_transient(&s.chapter_toast, msg, 4);
        },
    );
}

fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str) {
    let (work_title, work_author, work_abbrev, work_type, band, scene_text, model) = {
        let s = state_rc.borrow();
        let band = s.journal_band.clone();
        let (title, author, abbrev, work_type) = match s.current_work.as_ref() {
            Some(w) => (
                w.title.clone(),
                w.author.clone(),
                w.canonical_abbrev.clone(),
                w.work_type.clone(),
            ),
            None => return,
        };
        // Anchor on the reader's saved position (where the journal overlay was
        // opened from), mapped to a work line. Falls back to 0 (the division's
        // first paragraph) when unresolvable — scene_text_windowed clamps.
        let anchor_work_line = s
            .journal
            .return_pos
            .and_then(|(buf, _top)| s.work_line_for_buffer(buf))
            .unwrap_or(0);
        let scene_text = match band {
            JournalBand::Work => String::new(),
            JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_text_windowed(
                &s, d1, d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            ),
            JournalBand::Passage { div1, div2, .. } => {
                crate::app::scene_synopsis::scene_text_windowed(
                    &s, div1, div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
                )
            }
        };
        (
            title,
            author,
            abbrev,
            work_type,
            band,
            scene_text,
            s.config.claude_model.clone(),
        )
    };

    state_rc.borrow().journal_overlay.show_loading(question);

    // For a Passage band, consume pending_passage — but ONLY when it belongs
    // to THIS band. A cancelled ask leaves a stale pending behind; using it
    // here would embed (and persist via save_passage_page) a DIFFERENT
    // passage's source under this band. A stale mismatch is dropped (take())
    // either way so it cannot linger further.
    let passage_source_text: String = if matches!(band, JournalBand::Passage { .. }) {
        state_rc
            .borrow_mut()
            .journal
            .pending_passage
            .take()
            .filter(|pp| pp.band == band)
            .map(|pp| pp.source_text)
            .unwrap_or_default()
    } else {
        String::new()
    };

    let (genre, unit, _units) = crate::gloss::genre_unit(&work_type);
    let unit_label = titlecase_first(unit);
    let user_msg = match band {
        JournalBand::Work => format!(
            "Work type: {}\nWork: {} by {}\n\nReader's question about the {} as a whole:\n{}",
            genre, work_title, work_author, genre, question,
        ),
        JournalBand::Scene(d1, d2) => format!(
            "Work type: {}\nWork: {} by {}\n{}: {}\n\n{} text:\n{}\n\nReader's question:\n{}",
            genre,
            work_title,
            work_author,
            unit_label,
            crate::app::scene_synopsis::scene_label(d1, d2),
            unit_label,
            scene_text,
            question,
        ),
        JournalBand::Passage { div1, div2, .. } => format!(
            "Work type: {}\nWork: {} by {}\n{}: {}\n\n{} text:\n{}\n\nPassage:\n{}\n\nReader's question:\n{}",
            genre,
            work_title,
            work_author,
            unit_label,
            crate::app::scene_synopsis::scene_label(div1, div2),
            unit_label,
            scene_text,
            passage_source_text,
            question,
        ),
    };
    let question_owned = question.to_string();
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::journal_qa_prompt(&work_type),
        user_msg,
        model,
        move |st, answer| {
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let write_result = match &band {
                    JournalBand::Work => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev,
                            crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1,
                            &question_owned, &answer, &model_for_db, "work",
                        )
                        .map(|_| ())
                    }
                    JournalBand::Scene(d1, d2) => {
                        crate::db::journal::save_journal_page(
                            &conn, &work_abbrev, *d1, *d2,
                            &question_owned, &answer, &model_for_db, "scene",
                        )
                        .map(|_| ())
                    }
                    JournalBand::Passage { div1, div2, start, end } => {
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
            let new_index = pages.len().saturating_sub(1);
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

/// Build the Q&A picker rows for the current work, populate + show the picker,
/// and switch to `InputMode::JournalPicker`. Returns false (after a toast) when
/// the journal is empty, so the caller can leave state untouched. Shared by the
/// overlay (`open_picker`) and reader (`open_picker_from_reader`) entry points.
fn populate_and_show_picker(s: &mut AppState) -> bool {
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone())
        .unwrap_or_default();
    let pages = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_all_pages_ordered(&conn, &work_abbrev).ok())
        .unwrap_or_default();

    if pages.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No journal pages yet — press r to ask", 3);
        return false;
    }

    let rows: Vec<crate::ui::journal_picker::JournalRow> = pages
        .iter()
        .map(|p| {
            let band = band_for_page(p);
            // A passage Q&A resolves to its Scene band (so Enter lands in the
            // merged chapter band), but the picker still labels it "N.N passage"
            // — read passage-ness from the ROW's citations, not the band.
            let is_passage = p.start_citation.is_some() && p.end_citation.is_some();
            let scene_label = match &band {
                JournalBand::Work => "whole work".to_string(),
                JournalBand::Scene(d1, d2) if is_passage => format!("{}.{} passage", d1, d2),
                JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::synopsis_label(s, *d1, *d2),
                JournalBand::Passage { div1, div2, .. } => {
                    format!("{}.{} passage", div1, div2)
                }
            };
            // A passage Q&A shows the FIRST LINE of its passage (not the
            // question) so the picker reads like the text it's about; fall back to
            // the question if the source markup has no verse/stage line.
            let label_text = if is_passage {
                p.source_text
                    .as_deref()
                    .and_then(first_passage_line)
                    .unwrap_or_else(|| p.question.clone())
            } else {
                p.question.clone()
            };
            let prefix: String = label_text.chars().take(80).collect();
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
    true
}

/// Open the Q&A picker over the journal overlay (Ctrl+\). Lists every page in the
/// work (work pages first, then scene pages by scene), each by creation time.
/// Empty journal -> toast, stay in the overlay.
pub(crate) fn open_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal.picker_from_reader = false;
    populate_and_show_picker(&mut s);
}

/// Open the Q&A picker directly from the READING CARD (Alt+j), without first
/// opening the journal overlay. Records the reader return position and flags the
/// picker as reader-initiated so confirm reveals the overlay and Escape returns
/// to the reader. Empty journal -> toast, stay in the reader.
pub(crate) fn open_picker_from_reader(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    s.journal.return_pos = Some((s.current_line, s.page_top_line));
    s.journal.picker_from_reader = true;
    if !populate_and_show_picker(&mut s) {
        // Empty journal: nothing shown, drop the half-set reader-return state.
        s.journal.picker_from_reader = false;
        s.journal.return_pos = None;
    }
}

/// Confirm the picker selection: switch the journal overlay to the chosen page's
/// band, land on that exact page (matched by id within the band), hide the
/// picker, return to the journal overlay.
pub(crate) fn confirm_picker(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().journal_picker.selected_index();
    let mut s = state.borrow_mut();
    s.journal_picker.hide();
    s.input_mode = InputMode::JournalOverlay;
    // Confirming always reveals the overlay (land_on_page -> render_current ->
    // show_page), so the reader-initiated flag has done its job. Clear it but
    // KEEP return_pos — closing the overlay later (Esc) restores the reader.
    s.journal.picker_from_reader = false;

    let Some(idx) = selected else {
        // Nothing selected — just return to the overlay, re-render current band.
        render_current(&mut s);
        return;
    };
    let (band, target_id) = {
        let row = &s.journal_picker.items[idx];
        (row.band.clone(), row.id)
    };
    land_on_page(&mut s, band, target_id);
}

/// Open the "move this Q&A to another band" picker over the journal overlay.
/// Lists every band the current entry could move to (whole work + every
/// scene/chapter), excluding its current band. No-op with a toast if there is no
/// current page, or if the current band is a passage (passages are
/// citation-anchored and not movable).
pub(crate) fn open_move_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.pages.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No page to move", 2);
        return;
    }
    // Passage pages are citation-anchored and not movable. A passage page now
    // lives inside its Scene band, so guard on the current PAGE's citations
    // (not the band, which is Scene) — and also keep the band guard for the
    // transient ask-time Passage band.
    let on_passage_page = s
        .journal
        .pages
        .get(s.journal.page_index)
        .is_some_and(|p| p.start_citation.is_some() && p.end_citation.is_some());
    if on_passage_page || matches!(s.journal_band, JournalBand::Passage { .. }) {
        crate::ui::toast::show_transient(&s.chapter_toast, "Can't move a passage page", 2);
        return;
    }
    let rows = move_target_rows(&s, &s.journal_band.clone());
    if rows.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No other band to move to", 2);
        return;
    }
    s.journal_move_picker.set_items(rows);
    s.journal_move_picker.show();
    s.input_mode = InputMode::JournalMovePicker;
}

/// Confirm the move-picker selection: re-target the current entry to the chosen
/// band in lit.db, then follow it — switch the overlay to the destination band
/// and land on the moved entry (matched by id). Hides the picker and returns to
/// the journal overlay.
pub(crate) fn confirm_move_picker(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().journal_move_picker.selected_index();
    let mut s = state.borrow_mut();
    s.journal_move_picker.hide();
    s.input_mode = InputMode::JournalOverlay;

    let Some(idx) = selected else {
        render_current(&mut s);
        return;
    };

    // The destination band + label, and the current entry's id.
    let (dest_band, label) = {
        let row = &s.journal_move_picker.items[idx];
        (row.band.clone(), row.label.clone())
    };
    let Some(entry_id) = s.journal.pages.get(s.journal.page_index).map(|p| p.id) else {
        render_current(&mut s);
        return;
    };

    // Map the destination band to (scope, div1, div2).
    let (scope, d1, d2) = match &dest_band {
        JournalBand::Work => ("work", crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1),
        JournalBand::Scene(a, b) => ("scene", *a, *b),
        // open_move_picker excludes the passage band from targets; unreachable
        // in practice, but re-render-and-bail defensively rather than panic.
        JournalBand::Passage { .. } => {
            render_current(&mut s);
            return;
        }
    };

    let conn = match crate::db::queries::open_db_rw() {
        Ok(c) => c,
        Err(e) => {
            crate::logging::log(&format!("JOURNAL: move failed (open_db_rw): {}", e));
            render_current(&mut s);
            return;
        }
    };
    if let Err(e) = crate::db::journal::move_journal_page(&conn, entry_id, scope, d1, d2) {
        crate::logging::log(&format!("JOURNAL: move failed: {}", e));
        render_current(&mut s);
        return;
    }

    // Follow the entry: switch to the destination band and land on it.
    s.journal_band = dest_band;
    s.journal.page_index = 0;
    render_current(&mut s); // loads the destination band's pages
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == entry_id) {
        s.journal.page_index = pos;
        render_current(&mut s);
    }
    crate::ui::toast::show_transient(&s.chapter_toast, &format!("Moved to {}", label), 2);
    crate::logging::log("JOURNAL: moved page to new band");
}

/// Purge an entry's cached TTS MP3s (rows + files), since SQLite FK cascade is
/// not enabled app-wide. Called when an entry is DELETED and also when its answer
/// is EDITED/REWRITTEN — the cached per-paragraph audio no longer matches the new
/// text, so it must be dropped or Space would replay the stale take.
fn purge_journal_audio(conn: &rusqlite::Connection, id: i64) {
    if let Ok(paths) = crate::db::queries::delete_journal_audio(conn, id) {
        for p in paths {
            let _ = std::fs::remove_file(&p);
        }
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
        purge_journal_audio(&conn, id);
    }
    if s.journal.page_index > 0 {
        s.journal.page_index -= 1;
    }
    render_current(&mut s);
}

/// `c` in the journal overlay: copy the current Q&A entry's database row id to
/// the Wayland clipboard (via `wl-copy`) and confirm with a transient toast.
pub(crate) fn copy_current_id(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let Some(id) = s.journal.pages.get(s.journal.page_index).map(|p| p.id) else {
        return;
    };
    let _ = std::process::Command::new("wl-copy")
        .arg(id.to_string())
        .spawn();
    crate::ui::toast::show_transient(&s.chapter_toast, &format!("Copied id {}", id), 2);
    crate::logging::log(&format!("JOURNAL: copied id {}", id));
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

        // The current PAGE must be a passage page: it carries source_text plus a
        // start/end citation. (Read from the row, not the band — a passage page
        // now lives inside its Scene band alongside scene Q&As.)
        let (div1, div2, start_cit, end_cit) = match s.journal.pages.get(s.journal.page_index) {
            Some(p) => match (p.source_text.as_ref(), p.start_citation.clone(), p.end_citation.clone()) {
                (Some(_), Some(start), Some(end)) => (p.div1, p.div2, start, end),
                _ => {
                    crate::ui::toast::show_transient(&s.chapter_toast, "Not a passage page", 2);
                    return;
                }
            },
            None => {
                crate::ui::toast::show_transient(&s.chapter_toast, "Not a passage page", 2);
                return;
            }
        };

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
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position(&mut s, pos);
    }

    // Phase 3: cache hit — show existing gloss immediately.
    let own_idx = all_glosses.iter().position(|g| g.gloss_type == "reader-gloss");
    if let Some(idx) = own_idx {
        let mut s = state.borrow_mut();
        s.gloss_return_pos = Some((s.current_line, s.page_top_line));
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let (card_width, card_height) = crate::app::layout::overlay_card_size(&s);
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
        let (cw, h) = crate::app::layout::overlay_card_size(&s);
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
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &gloss_text, "reader-gloss", &model_for_db,
                    "GLOSS-FROM-JOURNAL: generated and saved new reader-gloss",
                );
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

        // Glosses are STORED under the canonical base abbrev, so look them up
        // the same way — every gloss path uses `Work.canonical_abbrev` or a
        // variant edition misses its own gloss and toasts "No gloss for this
        // passage" (the recurring lookup-mismatch bug class; see
        // project_gloss_lookup_normalize_abbrev).
        let work_abbrev = match s.current_work.as_ref() {
            Some(w) => w.canonical_abbrev.clone(),
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
        let pos = s.journal.return_pos.take();
        crate::app::restore_saved_position(&mut s, pos);
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
        let work_abbrev = s
            .current_work
            .as_ref()
            .map(|w| w.canonical_abbrev.clone())
            .unwrap_or_default();
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
        let pos = s.gloss_return_pos.take();
        crate::app::restore_saved_position(&mut s, pos);
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
    fn title_case_first_letter() {
        assert_eq!(super::titlecase_first("chapter"), "Chapter");
        assert_eq!(super::titlecase_first("scene"), "Scene");
        assert_eq!(super::titlecase_first(""), "");
    }

    #[test]
    fn first_passage_line_reads_first_verse() {
        let markup = "<speaker>FIRST GENTLEMAN</speaker>\n\
                      <verse>You do not meet a man but frowns. Our bloods</verse>\n\
                      <verse>No more obey the heavens than our courtiers'</verse>\n";
        assert_eq!(
            super::first_passage_line(markup).as_deref(),
            Some("You do not meet a man but frowns. Our bloods"),
        );
    }

    #[test]
    fn first_passage_line_uses_leading_stage_direction() {
        // A passage that opens on a stage direction shows that line.
        let markup = "<stage>[Enter two Gentlemen.]</stage>\n\
                      <speaker>FIRST GENTLEMAN</speaker>\n\
                      <verse>You do not meet a man but frowns.</verse>\n";
        assert_eq!(
            super::first_passage_line(markup).as_deref(),
            Some("[Enter two Gentlemen.]"),
        );
    }

    #[test]
    fn first_passage_line_none_without_verse_or_stage() {
        assert_eq!(super::first_passage_line("<speaker>KING</speaker>\n"), None);
        assert_eq!(super::first_passage_line(""), None);
    }

    #[test]
    fn flat_step_clamps_and_steps() {
        // Empty list -> no move.
        assert_eq!(super::flat_step(0, 1, 0), None);
        // Middle: steps forward and back.
        assert_eq!(super::flat_step(2, 1, 5), Some(3));
        assert_eq!(super::flat_step(2, -1, 5), Some(1));
        // Cross-band roll is just the next flat index — same call.
        assert_eq!(super::flat_step(0, 1, 3), Some(1));
        // Clamp at the ends -> no move (no wrap).
        assert_eq!(super::flat_step(4, 1, 5), None); // last + forward
        assert_eq!(super::flat_step(0, -1, 5), None); // first + back
        // Single-page work: never moves.
        assert_eq!(super::flat_step(0, 1, 1), None);
        assert_eq!(super::flat_step(0, -1, 1), None);
    }

    #[test]
    fn rewrite_user_message_includes_context_and_all_three_parts() {
        let msg = rewrite_user_message(
            "Work: Bleak House by Charles Dickens\nThis Q&A is filed under the WHOLE WORK (not a single scene).",
            "Who is Esther?",
            "She narrates half the book.",
            "Add her surname.",
        );
        // The band context is present and leads the message.
        assert!(msg.contains("WHOLE WORK"));
        assert!(msg.contains("Who is Esther?"));
        assert!(msg.contains("She narrates half the book."));
        assert!(msg.contains("Add her surname."));
        // Order: context, then question, then answer, then instruction.
        let c_pos = msg.find("WHOLE WORK").unwrap();
        let q_pos = msg.find("Who is Esther?").unwrap();
        let a_pos = msg.find("She narrates half the book.").unwrap();
        let i_pos = msg.find("Add her surname.").unwrap();
        assert!(c_pos < q_pos, "context should lead the message");
        assert!(q_pos < a_pos, "question before answer");
        assert!(i_pos > a_pos, "instruction should follow the current answer");
    }

    /// Build a `JournalPage` for band-classification tests.
    fn page(div1: i64, div2: i64, start: Option<&str>, end: Option<&str>) -> crate::db::journal::JournalPage {
        crate::db::journal::JournalPage {
            id: 1,
            div1,
            div2,
            question: "Q".into(),
            answer: "A".into(),
            claude_model: "m".into(),
            timestamp: "t".into(),
            start_citation: start.map(|s| s.to_string()),
            end_citation: end.map(|s| s.to_string()),
            source_text: None,
        }
    }

    #[test]
    fn band_for_page_classifies_work_and_scene_passages_share_scene_band() {
        // Work: div1 < 0 (the JOURNAL_WORK_DIV sentinel), no citations.
        assert_eq!(band_for_page(&page(-1, -1, None, None)), JournalBand::Work);
        // Scene: div1 >= 0, no citations.
        assert_eq!(band_for_page(&page(1, 0, None, None)), JournalBand::Scene(1, 0));
        // Passage: div1 >= 0 AND has citations -> the SAME Scene band as the
        // scene Q&As around it. A passage Q&A belongs to its scene/chapter band,
        // so the picker lands the reader in the merged band (render_current then
        // loads scene + passage pages together) and finds the page by id.
        assert_eq!(
            band_for_page(&page(1, 0, Some("BH.1.0.18"), Some("BH.1.0.18"))),
            JournalBand::Scene(1, 0),
        );
    }

    #[test]
    fn band_for_rewrite_reconstructs_passage_band_for_grounding() {
        // Viewing/navigation folds a passage page into its Scene band, but the
        // REWRITE context must keep the Passage band so it appends the passage
        // source. A scene page (no citations) still grounds as Scene.
        assert_eq!(band_for_rewrite(&page(-1, -1, None, None)), JournalBand::Work);
        assert_eq!(band_for_rewrite(&page(1, 0, None, None)), JournalBand::Scene(1, 0));
        assert_eq!(
            band_for_rewrite(&page(1, 0, Some("BH.1.0.18"), Some("BH.1.0.18"))),
            JournalBand::Passage { div1: 1, div2: 0, start: "BH.1.0.18".into(), end: "BH.1.0.18".into() },
        );
    }

    #[test]
    fn footer_left_scene_shows_abbrev_act_scene() {
        assert_eq!(footer_left_text("2H6", JournalBand::Scene(1, 4)), "2H6 1.4");
    }

    #[test]
    fn footer_left_work_shows_whole_work() {
        assert_eq!(footer_left_text("2H6", JournalBand::Work), "2H6 \u{00b7} whole work");
    }

    #[test]
    fn target_bands_exclude_current_and_lead_with_work() {
        // Pure core: given the unique (div1,div2) scene keys in reading order and
        // the current band, produce the ordered destination bands (work first,
        // current band omitted). Labels are applied separately by the caller.
        let scenes = vec![(1, 1), (1, 2), (3, 1)];

        // Current = Scene(1,2): work row first, then 1.1 and 3.1 (1.2 omitted).
        let bands = target_bands(&scenes, &JournalBand::Scene(1, 2));
        assert_eq!(
            bands,
            vec![JournalBand::Work, JournalBand::Scene(1, 1), JournalBand::Scene(3, 1)]
        );

        // Current = Work: work row omitted, all scenes listed.
        let bands = target_bands(&scenes, &JournalBand::Work);
        assert_eq!(
            bands,
            vec![JournalBand::Scene(1, 1), JournalBand::Scene(1, 2), JournalBand::Scene(3, 1)]
        );
    }
}

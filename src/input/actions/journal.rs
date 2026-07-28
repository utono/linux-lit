use crate::app::{AppState, InputMode, JournalBand, JournalPromptMode};
use crate::ui::journal_move_picker::MoveTargetRow;
use crate::ui::journal_overlay::JournalSource;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The current work's canonical abbrev, or `""` if no work is loaded. The
/// journal Q&A / picker paths key every DB read on this string; extracted so the
/// `.current_work.map(canonical_abbrev).unwrap_or_default()` idiom lives once
/// (audit #72).
fn current_work_abbrev(s: &AppState) -> String {
    s.current_work
        .as_ref()
        .map(|w| w.canonical_abbrev.clone())
        .unwrap_or_default()
}

/// The line the `\` lap is anchored to.
///
/// An open overlay has already moved the cursor to the end of its own passage,
/// so the live `current_line` is the wrong question to ask once a lap is under
/// way. `gloss_return_pos` / `journal.return_pos` hold the reader position the
/// lap started from; fall back to the cursor when no overlay is open.
///
/// Pure so both the probe and the open resolve identically — they disagreed
/// before 2026-07-27 and opened a different entry than they probed.
fn lap_anchor_line(
    gloss_return: Option<(usize, usize, i32)>,
    journal_return: Option<(usize, usize, i32)>,
    current_line: usize,
) -> usize {
    gloss_return
        .or(journal_return)
        .map(|(line, _, _)| line)
        .unwrap_or(current_line)
}

/// `lap_anchor_line` read off live state.
fn lap_anchor_for(s: &AppState) -> usize {
    lap_anchor_line(s.gloss_return_pos, s.journal.return_pos, s.current_line)
}

/// Take the synopsis-origin band, leaving None. Pure so the take-not-copy
/// contract is testable without an AppState.
fn take_synopsis_origin(marker: &mut Option<(i64, i64)>) -> Option<(i64, i64)> {
    marker.take()
}

/// Capitalize the first character of `s` (ASCII), leaving the rest unchanged.
/// Used to turn a unit noun (`chapter`) into a user-message field label
/// (`Chapter:`). Empty input returns empty. `pub(crate)` so the chat panel
/// derives the unit label identically to `ask_claude` when it shares
/// `build_qa_answer_message`.
pub(crate) fn titlecase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Prose journal-Q&A context window radius (paragraphs each side of the
/// reader's anchor). Prose divisions can be the whole book, so cap the context.
const PROSE_CONTEXT_RADIUS: usize = 10;

/// What an `R` rewrite is regenerating, solely so the "Rewriting …" toast can
/// say which part is in flight. `Answer` is the plain `a` path (and the vim-`R`
/// instruction path); `Question` is the `q` path (question reworded, answer
/// regenerated afresh); `Both` is the `b` path (question improved, then the
/// answer regenerated for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteTarget {
    Question,
    Answer,
    Both,
}

impl RewriteTarget {
    /// The persistent toast text shown while the rewrite call is in flight.
    fn toast(self) -> &'static str {
        match self {
            RewriteTarget::Question => "Rewriting question\u{2026}",
            RewriteTarget::Answer => "Rewriting answer\u{2026}",
            RewriteTarget::Both => "Rewriting question & answer\u{2026}",
        }
    }
}

/// Parse the improve-question reply: strip a markdown fence + trim. Returns
/// `original` when the reply is empty/whitespace so the question is never lost.
fn parse_improved_question(raw: &str, original: &str) -> String {
    let cleaned = crate::journal_tags::strip_code_fence(raw).trim().to_string();
    if cleaned.is_empty() {
        original.to_string()
    } else {
        cleaned
    }
}

/// Fallback prompt for improving a reader's journal question when the
/// `journal.improve-question` api_prompts row is absent. Returns ONLY the
/// improved question (one line, no preamble, no JSON).
const FALLBACK_IMPROVE_QUESTION_PROMPT: &str = "\
You are a literature tutor helping a student learn to ask sharper, more \
insightful questions about a text. Recast the student's question the way an \
expert reader would pose it, so the student absorbs how scholars frame \
inquiry. Two things to do at once:\n\
1. FRAME IT LIKE AN EXPERT — name the literary technique, device, rhetorical \
move, or term of art actually at stake, and anchor the question to the specific \
moment (speaker, line, or turn) it concerns, rather than asking in vague, \
general terms.\n\
2. DEEPEN THE INQUIRY — push a surface question toward the richer question \
underneath it: from \"what does this mean\" toward \"how does this work\" or \
\"why does it matter here\". Draw out the interpretive stakes the student is \
circling.\n\
PRESERVE what the student is genuinely curious about — do not swap in a \
different question, do not answer it, and do not add extra sub-questions. Even \
when the original is already grammatical, produce a genuinely sharper, more \
expert version rather than returning it unchanged.\n\
When the improved question uses a scholarly or technical term — a literary \
device, rhetorical figure, or term of art (e.g. conceit, ironize, metonymy, or \
a legal or prosodic term) — gloss it briefly in a parenthetical the first time, \
so the student learns the vocabulary as they read (e.g. \"ironize (turn against \
itself)\", \"conceit (an extended, ingenious metaphor)\").\n\
Set the title of any work the question cites in quotation marks — \
\"Paradise Lost\" — never in asterisks or any other italics markup, and \
never bare.\n\
{terms}\n\
Return ONLY the improved question as a single line of plain text — no preamble, \
no quotes, no markdown, no explanation.";

/// Fallback for the scene-terms extractor when the `journal.scene-terms`
/// api_prompts row is absent. Mirrors the master so a missing row does not
/// silently disable first-ask term grounding. Same `{"terms":[...]}` contract as
/// the extract-terms prompt, so `parse_terms` handles the reply unchanged.
const FALLBACK_SCENE_TERMS_PROMPT: &str = "\
You extract the substantive terms of art that a reader working through the\n\
following passage might want to ask about — legal, rhetorical, historical,\n\
prosodic, or theological terms a reader might later look up (e.g. \"fee simple\",\n\
\"anaphora\", \"recusant\").\n\
\n\
Do NOT include ordinary vocabulary, character names, or the work's title.\n\
Prefer the canonical phrasing of each term. Return AT MOST 8 terms.\n\
\n\
Return ONLY a JSON object (no markdown fences, no commentary) with exactly one\n\
key:\n\
\n\
{\"terms\": [\"term one\", \"term two\"]}\n\
\n\
If the passage has no such term, return {\"terms\": []}.";

/// The `{terms}` substitution for the improve-question prompt: a guidance
/// sentence naming the entry's key terms of art and telling Claude to keep them,
/// or the empty string when the entry has no tags (so the prompt reads cleanly
/// and behaves exactly as before this feature).
fn improve_terms_line(terms: &[String]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    format!(
        "The reader's question concerns these terms of art: {}. Preserve them \
         verbatim in your rewrite — keep each term's canonical phrasing, and do \
         not rename, gloss away, or drop any of them.",
        terms.join(", ")
    )
}

/// Improve a journal question's phrasing via Claude, then hand the improved
/// question (or the original on empty/error) to `on_done` on the main loop.
///
/// BORROW SAFETY: `config.claude_model` is read under a scoped borrow that
/// drops before `run_claude_request` (which itself borrows `state` for
/// `tokio_handle`) — mirrors `ask_claude`'s borrow discipline. `on_done` runs
/// later, inside `run_claude_request`'s on_success/on_error closures on the
/// main loop, receiving the `&Rc<RefCell<AppState>>` those closures are
/// handed.
/// Resolve candidate terms of art for a BRAND-NEW ask by extracting them from
/// the current scene text, then hand `(state, question, terms)` to `on_done`.
/// Empty scene text (Work/Author band, unresolvable position) or any extraction
/// error yields an empty term list — the ask then proceeds ungrounded, exactly
/// as before this feature.
///
/// BORROW SAFETY: scene text + model are read under one scoped borrow that drops
/// before `run_claude_request` (which re-borrows `state`), mirroring
/// `spawn_retag`. `on_done` runs later inside the request callbacks.
fn extract_scene_terms(
    state: &Rc<RefCell<AppState>>,
    question: String,
    on_done: impl Fn(&Rc<RefCell<AppState>>, String, Vec<String>) + 'static,
) {
    let (scene_text, model) = {
        let s = state.borrow();
        (current_scene_text(&s), s.config.tag_extract_model.clone())
    };
    if scene_text.trim().is_empty() {
        on_done(state, question, Vec::new());
        return;
    }
    let prompt = crate::db::prompts::active_prompt("journal.scene-terms")
        .unwrap_or_else(|| FALLBACK_SCENE_TERMS_PROMPT.to_string());
    // `on_done` is shared (not Clone) between the two callbacks; wrap in Rc.
    let on_done = Rc::new(on_done);
    let on_done_err = Rc::clone(&on_done);
    let q_ok = question.clone();
    let q_err = question;
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        scene_text, // the user message is the passage text to mine
        model,
        move |st, reply| {
            let terms = crate::journal_tags::parse_terms(&reply);
            on_done(st, q_ok.clone(), terms);
        },
        move |st, msg| {
            crate::logging::log(&format!("SCENE-TERMS: extract failed ({msg}); no grounding"));
            on_done_err(st, q_err.clone(), Vec::new());
        },
    );
}

pub(crate) fn improve_question(
    state: &Rc<RefCell<AppState>>,
    question: String,
    terms: &[String],
    on_done: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
) {
    let model = state.borrow().config.claude_model.clone();
    let prompt = crate::db::prompts::active_prompt("journal.improve-question")
        .unwrap_or_else(|| FALLBACK_IMPROVE_QUESTION_PROMPT.to_string())
        .replace("{terms}", &improve_terms_line(terms));
    let original = question.clone();
    let original_err = question.clone();
    // `on_done` is shared (not Clone) between the two callbacks, so wrap it in
    // an Rc rather than requiring callers to pass a Clone closure.
    let on_done = Rc::new(on_done);
    let on_done_err = Rc::clone(&on_done);
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        question, // the user message is the raw question
        model,
        move |st, reply| {
            let improved = parse_improved_question(&reply, &original);
            if improved == original {
                crate::logging::log(
                    "IMPROVE-Q: model returned the question unchanged (no reword)",
                );
            } else {
                crate::logging::log(&format!(
                    "IMPROVE-Q: reworded\n  from: {original}\n  to:   {improved}"
                ));
            }
            on_done(st, improved);
        },
        move |st, msg| {
            crate::logging::log(&format!("IMPROVE-Q: call failed ({msg}); keeping original"));
            on_done_err(st, original_err.clone());
        },
    );
}

/// The first spoken/stage line of a passage's `<speaker>/<segment>/<stage>` source
/// markup (as built by `build_source_header`), for the Q&A picker to show
/// instead of the question. Returns the inner text of the first `<segment>` or
/// `<stage>` element (speaker labels are chrome, skipped), or `None` if the
/// markup has no such line. Pure — unit-tested.
fn first_passage_line(source_markup: &str) -> Option<String> {
    for line in source_markup.lines() {
        let line = line.trim();
        for tag in ["segment", "stage"] {
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

/// Active term-browse filter: the term, the ordered cross-work match list, and
/// the current position within it. When set, `nav_page` walks these matches
/// instead of the current work's `find_all_pages_ordered`.
#[derive(Debug, Clone, Default)]
pub struct JournalFilter {
    pub term: String,
    pub matches: Vec<crate::db::journal::TermMatch>,
    pub pos: usize,
}

/// Grouped state for the journal feature (band pages + viewer index + the
/// return-to-reader position + the add/edit prompt mode). Was four flat
/// `journal_*` fields on AppState; grouped per the AppState god-struct
/// decomposition (pure-tier cluster).
pub struct JournalState {
    pub pages: Vec<crate::db::journal::JournalPage>,
    pub page_index: usize,
    pub return_pos: Option<(usize, usize, i32)>,
    pub prompt_mode: JournalPromptMode,
    /// Set by `action_journal_qa` before opening the ask card; read and
    /// consumed by `ask_claude` when the band is `Passage`.
    pub pending_passage: Option<PendingPassage>,
    /// True when the Q&A picker was opened from the READING CARD (Alt+j) rather
    /// than from inside the journal overlay (Alt+p). Set by
    /// `open_picker_from_reader`; consumed by the picker's confirm/escape paths so
    /// Escape returns to the reader (not a hidden journal overlay).
    pub picker_from_reader: bool,
    /// Set by `vim_open_rewrite` (the `R` key in the vim editor) before opening
    /// the ask card: `(page_id, question, answer, target)` of the CURRENT edit
    /// buffer. Read and consumed by `submit_prompt`, which sends it + the typed
    /// instruction to Claude as a rewrite. `target` selects the "Rewriting …"
    /// toast wording. `None` for a normal create-ask.
    pub vim_rewrite: Option<(i64, String, String, RewriteTarget)>,
    /// Id of the page the overlay OPENED on when entered from the reader
    /// (Ctrl+j). While the viewer is still on this page, closing restores the
    /// exact saved reading position instead of source-jumping — a peek at the
    /// current passage's Q&A must not re-frame the page. Traversing to any
    /// other page (Ctrl+n/p, picker) re-enables the source jump. `None` for
    /// opens that are themselves navigation (picker confirm, add flows).
    pub entry_page_id: Option<i64>,
    /// Active cross-work term-browse filter (see `JournalFilter`). When set,
    /// `nav_page` walks the filtered match list instead of the current work's
    /// `find_all_pages_ordered`. `None` outside term-browse.
    pub filter: Option<JournalFilter>,
    /// Live regex search over the CURRENT overlay entry's buffer (the `/` bind,
    /// n/N stepping). Its match spans are re-collected on every entry render
    /// (`reapply`), so it highlights the seeded/f-term across the stepped set.
    /// `None` when no search is active.
    pub search: Option<crate::input::overlay_search::OverlaySearch>,
    /// MRU search pattern for post-Escape n/N revival: cleared search drops
    /// `search` but keeps this, so the next n/N rebuilds the search from it.
    pub last_pattern: Option<String>,
    /// True when the term input was opened from the READING CARD (reader `f`)
    /// rather than inside the journal overlay. Consumed by the term-input Escape
    /// path so cancel returns to the reader, not the overlay. Mirrors
    /// `picker_from_reader`.
    pub term_input_from_reader: bool,
}

/// True when the term input, opened while in `mode`, should return to the reader
/// on cancel (opened from the reading card) rather than the journal overlay.
pub(crate) fn term_input_opened_from_reader(mode: crate::app::InputMode) -> bool {
    matches!(mode, crate::app::InputMode::Reader)
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
    if p.div1 == crate::app::JOURNAL_AUTHOR_DIV.0 && p.div2 == crate::app::JOURNAL_AUTHOR_DIV.1 {
        JournalBand::Author(String::new())
    } else if p.div1 < 0 {
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
    if p.div1 == crate::app::JOURNAL_AUTHOR_DIV.0 && p.div2 == crate::app::JOURNAL_AUTHOR_DIV.1 {
        JournalBand::Author(String::new())
    } else if p.div1 < 0 {
        JournalBand::Work
    } else if let (Some(start), Some(end)) = (p.start_citation.clone(), p.end_citation.clone()) {
        JournalBand::Passage { div1: p.div1, div2: p.div2, start, end }
    } else {
        JournalBand::Scene(p.div1, p.div2)
    }
}

/// Compact source citation for a journal passage, e.g. `— Cymbeline, 1.1.1–3`.
/// `title` is the work title; the div/line numbers come from parsing
/// `start_citation`/`end_citation` (`ABBR.div1.div2.line`). Collapses to a
/// single locator (`— Cymbeline, 1.1.5`) when start line == end line or no end
/// citation is given. Returns `None` when the start citation is absent or does
/// not parse (never fabricate a locator).
fn format_source_citation(
    title: &str,
    start_citation: Option<&str>,
    end_citation: Option<&str>,
) -> Option<String> {
    let (d1, d2, start_line) = crate::app::parse_citation(start_citation?)?;
    let end_line = end_citation
        .and_then(crate::app::parse_citation)
        .map(|(_, _, l)| l)
        .unwrap_or(start_line);
    let locator = if end_line > start_line {
        format!("{}.{}.{}\u{2013}{}", d1, d2, start_line, end_line)
    } else {
        format!("{}.{}.{}", d1, d2, start_line)
    };
    Some(format!("\u{2014} {}, {}", title, locator))
}

/// Inner text of the first `<TAG>…</TAG>` on `line`, or `None` if `line` is not
/// a single `<tag>text</tag>` element for one of `tags`. Whitespace-trimmed.
fn tag_inner<'a>(line: &'a str, tags: &[&str]) -> Option<&'a str> {
    let l = line.trim();
    for t in tags {
        let open = format!("<{}>", t);
        let close = format!("</{}>", t);
        if let Some(rest) = l.strip_prefix(&open) {
            if let Some(inner) = rest.strip_suffix(&close) {
                return Some(inner.trim());
            }
        }
    }
    None
}

/// Build the ordered source paragraphs to prepend above a passage Q&A:
/// `[speaker?, quote block(s), citation?]`. The speaker paragraph is dropped
/// when empty or `UNKNOWN` (prose works).
///
/// The quote lines (whether `<segment>`/`<stage>`-tagged — the display source is
/// rebuilt with per-line `<segment>` tags by `build_source_header` — or plain
/// untagged text) flush by `is_prose`:
///
/// - **Verse/play work** (`is_prose` false): all quote lines collapse into ONE
///   `\n`-joined paragraph so the overlay renders them at pure line-height
///   (consecutive lines, no blank line between) — matching the main card. Verse
///   lines of a speech belong together.
/// - **Prose work** (`is_prose` true): each quote line is a distinct PARAGRAPH
///   and becomes its own `paras` entry, so the overlay's `"\n\n"` join gives a
///   blank-line gap between them — a multi-paragraph prose passage reads as
///   separate paragraphs, not a run-together wall of text.
///
/// `is_prose` is the authoritative `works.work_type` signal (`is_prose_work`),
/// never inferred from the text. The quoted block(s) plus citation still get
/// blank-line gaps between the speaker, quote, and citation. (No trailing `———`
/// rule — the user dropped it; the citation plus the blank-line gap separate the
/// quote from the question on their own.) The returned `JournalSource` carries
/// `has_speaker`/`has_citation` explicitly so the overlay never has to sniff
/// paragraph roles from their text.
fn source_paragraphs(source_text: &str, citation: Option<&str>, is_prose: bool) -> JournalSource {
    let mut out: Vec<String> = Vec::new();
    let mut has_speaker = false;
    // Quote lines accumulate here; flushed by `is_prose` (one joined block for
    // verse, one paragraph each for prose) via `flush`.
    let mut verse: Vec<String> = Vec::new();
    // Flush accumulated quote lines into `out`: one `\n`-joined block on verse
    // works (line-height, matches the main card), one paragraph each on prose
    // works (blank-line gap between paragraphs). Called before a speaker label
    // and once at the end so a speaker never merges into the prior speech.
    let flush = |verse: &mut Vec<String>, out: &mut Vec<String>| {
        if verse.is_empty() {
            return;
        }
        if is_prose {
            out.extend(verse.drain(..));
        } else {
            out.push(verse.join("\n"));
            verse.clear();
        }
    };
    for raw in source_text.lines() {
        if let Some(sp) = tag_inner(raw, &["speaker"]) {
            if !sp.is_empty() && sp != "UNKNOWN" {
                // A speaker starts a new speech: flush the prior one first so it
                // doesn't merge across the label.
                flush(&mut verse, &mut out);
                out.push(sp.to_string());
                has_speaker = true;
            }
        } else if let Some(body) = tag_inner(raw, &["segment", "stage"]) {
            for seg in body.split('\n') {
                let seg = seg.trim();
                if !seg.is_empty() {
                    verse.push(seg.to_string());
                }
            }
        } else {
            // Untagged line: vocab Q&As store the cursor segment as PLAIN text
            // (no <speaker>/<segment> markup), so treat bare lines as quote lines
            // — otherwise a vocab entry renders only its citation.
            let seg = raw.trim();
            if !seg.is_empty() {
                verse.push(seg.to_string());
            }
        }
    }
    flush(&mut verse, &mut out);
    let has_citation = citation.is_some();
    if let Some(c) = citation {
        out.push(c.to_string());
    }
    JournalSource { paras: out, has_speaker, has_citation }
}

/// Footer-left text identifying the current page: `<abbrev> <act>.<scene>` for a
/// scene page, `<abbrev> · whole work` for a whole-work page.
fn footer_left_text(abbrev: &str, band: JournalBand) -> String {
    match band {
        JournalBand::Work => format!("{} \u{00b7} whole work", abbrev),
        JournalBand::Scene(d1, d2) => format!("{} {}.{}", abbrev, d1, d2),
        JournalBand::Passage { div1, div2, .. } => format!("{} {}.{} passage", abbrev, div1, div2),
        JournalBand::Author(name) => format!("{} \u{00b7} corpus", name),
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
                // target_bands never yields Passage or Author; map defensively.
                JournalBand::Passage { div1, div2, .. } => format!("{}.{} passage", div1, div2),
                JournalBand::Author(_) => String::new(),
            };
            MoveTargetRow { band, label }
        })
        .collect()
}

/// Query the current band's page list from the DB (no rendering, no state
/// writes). Shared by `render_current` and `land_on_page` — the latter needs
/// the list to locate a target id BEFORE the single render.
fn load_band_pages(s: &AppState) -> Vec<crate::db::journal::JournalPage> {
    let work_abbrev = current_work_abbrev(s);
    let conn = crate::db::queries::open_db().ok();
    // The overlay has no title header anymore (the footer identifies the work +
    // chapter), so each band only needs its page list.
    match s.journal_band.clone() {
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
        // Author band: the author name is stored in the variant (filled by the
        // caller after band_for_page returns Author(String::new())).
        JournalBand::Author(author_name) => conn
            .and_then(|c| crate::db::journal::find_author_pages(&c, &author_name).ok())
            .unwrap_or_default(),
    }
}

/// Load the current band's pages from the DB into `journal.pages`, clamp the
/// index, and render the current page (or the empty-band card).
pub(crate) fn render_current(s: &mut AppState) {
    let t_total = std::time::Instant::now();
    let work_abbrev = current_work_abbrev(s);

    let t_query = std::time::Instant::now();
    let pages = load_band_pages(s);

    let band_query_ms = t_query.elapsed().as_millis();

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
    // Prose works get the main reading card's tighter column (card/8) so the
    // overlay text margins equal the 1-col prose layout; see size_card.
    let is_prose = s
        .current_work
        .as_ref()
        .is_some_and(|w| crate::db::line_types::is_prose_work(&w.work_type));
    s.journal_overlay.set_prose_reading(is_prose);
    let footer_left = footer_left_text(&work_abbrev, s.journal_band.clone());

    // A passage ask in flight (visual selection → Journal Q&A): render the
    // SELECTED passage source behind the ask card, so the reader sees the text
    // they are asking about while the ask card is open — mirroring the gloss
    // "Glossing…" card. This runs whether or not the passage already has stored
    // Q&As: when it has none it replaces the bare "No pages yet — press r to
    // ask." placeholder; when it has some, it still shows the passage source
    // (not the existing Q&A) for the duration of the transient ask. The stored
    // pages are unaffected — `ask_claude` consumes `pending_passage` on submit
    // and normal Ctrl+n/p viewing has no matching pending, so it renders the
    // Q&As as plain pages. The `pp.band == journal_band` check is the staleness
    // guard: it fires ONLY while an ask card is open for THIS band. A CANCELLED
    // ask leaves pending_passage behind (so `r` can re-ask on its own band), and
    // the band check keeps the stale source off any other Passage band.
    if s.journal
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

    // A passage Q&A (source_text present) shows its quoted source — speaker,
    // verse, citation — as leading navigable paragraphs above the question.
    // Built here and passed to show_page, which prepends them to
    // all_paragraphs (page 0 only; apply_source_style styles them). Notes and
    // source-less entries pass None (unchanged plain Q&A).
    let current_page = if count == 0 {
        None
    } else {
        Some(&pages[s.journal.page_index])
    };
    let (q, a, kind) = current_page
        .map(|p| (p.question.clone(), p.answer.clone(), p.kind.clone()))
        .unwrap_or_else(|| (String::new(), String::new(), "qa".to_string()));

    let source_para = current_page.and_then(|p| {
        let src = p.source_text.as_deref().unwrap_or("").trim();
        if src.is_empty() {
            return None;
        }
        let title = s
            .current_work
            .as_ref()
            .map(|w| w.title.clone())
            .unwrap_or_default();
        let citation =
            format_source_citation(&title, p.start_citation.as_deref(), p.end_citation.as_deref());
        Some(source_paragraphs(src, citation.as_deref(), is_prose))
    });

    let head = crate::app::scene_synopsis::cursor_head(s);
    s.journal_overlay.set_running_head(&head.0, &head.1);
    let t_show = std::time::Instant::now();
    s.journal_overlay.show_page(
        &footer_left,
        s.journal.page_index,
        count,
        &q,
        &a,
        &kind,
        source_para,
        cw,
        h,
    );

    let show_page_ms = t_show.elapsed().as_millis();

    s.journal.pages = pages;
    // Color any paragraphs whose TTS MP3 is already cached, like the gloss
    // overlay (must run after the page renders + s.journal.pages is set so the
    // entry id resolves).
    let t_recolor = std::time::Instant::now();
    crate::input::actions::gloss::recolor_journal_cached_blocks(s);
    let recolor_ms = t_recolor.elapsed().as_millis();
    // Re-apply any active overlay search so a `/`-typed pattern keeps
    // highlighting across Ctrl+n/p band navigation. Re-collect against the NEW
    // entry's WHOLE-entry text (not the rendered buffer), so matches on later
    // pages are found too, then store + paint the current page.
    reapply_overlay_search_whole_entry(s);
    // Show the diff vs the entry's last stored revision (or clear if none), so
    // landing on an entry always highlights what its last rewrite changed.
    let t_diff = std::time::Instant::now();
    refresh_entry_diff_highlight(s);
    crate::log_fmt!(
        "JOURNAL-TIMING: band_query={}ms show_page={}ms recolor={}ms diff={}ms total={}ms",
        band_query_ms,
        show_page_ms,
        recolor_ms,
        t_diff.elapsed().as_millis(),
        t_total.elapsed().as_millis()
    );
}

/// Re-collect the active overlay search's pattern against the CURRENT entry's
/// whole-entry text (every page), store the whole-body matches on
/// `s.journal.search`, and paint whatever falls on the shown page via
/// `set_search_matches`. No-op when no search is active. Used after landing on a
/// new entry (band nav / filter step) so a `/`-typed or seeded pattern keeps
/// highlighting across the stepped set AND finds cross-page matches.
fn reapply_overlay_search_whole_entry(s: &mut AppState) {
    if s.journal.search.is_none() {
        return;
    }
    let text = s.journal_overlay.whole_entry_text();
    if let Some(search) = s.journal.search.as_mut() {
        search.matches = crate::input::overlay_search::collect(&text, &search.pattern);
        if search.current >= search.matches.len() {
            search.current = search.matches.len().saturating_sub(1);
        }
    }
    let search = s.journal.search.clone().unwrap();
    s.journal_overlay.set_search_matches(&search);
}

/// Render the filter's current match in the overlay WITHOUT switching
/// `journal_band` / `current_work`. Drives `show_page` directly with the
/// fetched entry (bypasses the band-driven `render_current` so an entry from
/// another work displays in place). Footer reads
/// "<abbrev> <div1>.<div2> · "<term>" · match N of M" — the searched term
/// orients the reader across cross-work matches. No-op if there is no active
/// filter or the position is out of range.
pub(crate) fn render_filtered_match(s: &mut AppState) {
    let Some(filter) = s.journal.filter.as_ref() else {
        return;
    };
    let Some(m) = filter.matches.get(filter.pos) else {
        return;
    };
    let p = m.page.clone();
    let work_abbrev = m.work_abbrev.clone();
    let footer_left = format!(
        "{} {}.{} \u{00b7} \u{201c}{}\u{201d} \u{00b7} match {} of {}",
        work_abbrev,
        p.div1,
        p.div2,
        filter.term,
        filter.pos + 1,
        filter.matches.len()
    );
    let (cw, h) = crate::app::layout::overlay_card_size(s);
    // Filtered view shows one entry at a time: page_index 0 of page_count 1.
    // No source block here — the term-browse filtered view is a distinct render
    // path from nav_page (kept scoped to the main viewer for now).
    // Head names the ENTRY's own work/position (the filtered view can surface
    // entries away from the cursor), matching the footer's citation.
    let head_pos = crate::app::scene_synopsis::synopsis_label(s, p.div1, p.div2);
    s.journal_overlay.set_running_head(&work_abbrev, &head_pos);
    s.journal_overlay
        .show_page(&footer_left, 0, 1, &p.question, &p.answer, &p.kind, None, cw, h);
    // Re-apply the overlay search against the just-rendered entry. For an
    // `f`-filtered entry no search is seeded (opens clean); for a Ctrl+f
    // corpus-search hit `open_journal_hit` seeds the `/` pattern AFTER this, so
    // its match still lights up. Re-collect against the whole-entry text (every
    // page) so a later-page match is found.
    reapply_overlay_search_whole_entry(s);
    // A filtered entry is always popup-opened (`f` term browse or the Ctrl+f
    // corpus search — the only setters of `journal.filter`), and those open the
    // entry CLEAN: no rewrite-diff highlight. Clear any stale diff rather than
    // painting one. (Normal band navigation goes through `render_current`, which
    // keeps the diff; the Ctrl+Shift+p revision browse paints its own.)
    s.journal_overlay.clear_rewrite_diff();
    // Vocab tint: the filtered render bypasses recolor_journal_cached_blocks
    // (the shared post-render vocab hook), so tint the filtered entry here.
    if s.vocab_highlight_visible {
        s.journal_overlay.apply_vocab_tags(&s.vocab_words);
    }
}

/// Activate a term filter: fetch matches, store filter state, render the
/// first match. Returns `false` (with a toast) if nothing matches.
pub(crate) fn activate_filter(state: &Rc<RefCell<AppState>>, term: &str) -> bool {
    let matches = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_pages_by_term(&conn, term).ok())
        .unwrap_or_default();
    let mut s = state.borrow_mut();
    if matches.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, &format!("No entries mention \u{201c}{}\u{201d}", term), 3);
        return false;
    }
    s.journal.filter = Some(JournalFilter {
        term: term.to_string(),
        matches,
        pos: 0,
    });
    render_filtered_match(&mut s);
    // The browsed term is deliberately NOT seeded as an overlay search: the
    // `f`-filtered entry opens clean (no highlight). The filter itself stays
    // active (`s.journal.filter`), so Ctrl+n/p still steps the subset and Escape
    // still jumps to the entry's Arkangel source. `/` remains available to search
    // within the open entry manually.
    true
}

/// Clear the active filter and return to the normal band view.
pub(crate) fn clear_filter(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.filter.is_none() {
        return;
    }
    s.journal.filter = None;
    // Clear any search seeded by the filter (e.g. the f-term). Drop the stored
    // whole-body match spans on the overlay too, or the next render_page would
    // re-paint them (the highlights survive page turns by re-tagging from
    // search_full). Keep last_pattern so post-clear n/N can still revive it.
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    render_current(&mut s); // restore the band the user was in
}

/// Escape from a journal entry shown under an active term filter: if it is a
/// PASSAGE entry (has a source citation), close the overlay + clear the filter,
/// load its `<work>-Arkangel` edition (base if none) with the Arkangel media,
/// and land the cursor on the entry's source first line. Returns `true` when it
/// handled a passage entry; `false` for a non-passage note (no citation) — the
/// caller then falls back to clear-filter + close-to-reader.
pub(crate) fn escape_filtered_entry_to_source(state: &Rc<RefCell<AppState>>) -> bool {
    // Gather the filtered entry's abbrev + citation + source under a short borrow.
    let (base_abbrev, start_citation, source_text, current_abbrev) = {
        let s = state.borrow();
        let Some(filter) = s.journal.filter.as_ref() else {
            return false;
        };
        let Some(m) = filter.matches.get(filter.pos) else {
            return false;
        };
        let Some(cite) = m.page.start_citation.clone() else {
            return false; // non-passage note: caller falls back
        };
        (
            m.work_abbrev.clone(),
            cite,
            m.page.source_text.clone().unwrap_or_default(),
            s.current_work.as_ref().map(|w| w.abbrev.clone()),
        )
    };

    // Leave the overlay: clear the filter + close to the reader BEFORE loading.
    // Both take `&Rc<RefCell<AppState>>` and re-borrow internally, so the gather
    // borrow above must already be dropped (it is).
    clear_filter(state);
    close_overlay(state);

    // Load the entry's Arkangel edition (base if none), then land the cursor on
    // its source first line — resolved against the freshly-loaded edition. The
    // shared loader handles the same-work skip, the MPV-media discovery, and the
    // error toast; `on_ready` just does the source-line jump (runs in both the
    // same-work and cross-work paths).
    let handle = state.borrow().tokio_handle.clone();
    crate::input::actions::pickers::load_arkangel_edition_then(
        state,
        &handle,
        base_abbrev,
        current_abbrev,
        move |state| {
            let mut s = state.borrow_mut();
            // Compute the buffer line under the immutable borrow of current_work
            // (which ends with the `and_then` closure), then do the mutable jump.
            let buf = s.current_work.as_ref().and_then(|work| {
                source_first_buffer_line(work, s.line_map.as_ref(), &start_citation, &source_text)
            });
            if let Some(bi) = buf {
                crate::input::navigation::jump_to_line(&mut s, bi);
            }
        },
    );
    true
}

/// Open the term input box (with distinct-tag suggestions) from inside the
/// overlay (the `f` key). The user types a term freely; existing tags are
/// suggested beneath.
///
/// BORROW SAFETY: `set_suggestions`/`show()` call `search_entry.set_text("")`,
/// which SYNCHRONOUSLY emits the entry's `changed` signal; that handler does
/// `state.borrow()` to re-filter. So we must NOT hold a `borrow_mut` across
/// those calls — do the widget work under a short-lived borrow that is dropped
/// before the signal can re-enter, then set `input_mode` in a fresh borrow.
/// (Holding the borrow across `set_text` is what caused the RefCell
/// non-unwinding panic in the GTK callback.)
pub(crate) fn open_term_input(state: &Rc<RefCell<AppState>>) {
    let from_reader = term_input_opened_from_reader(state.borrow().input_mode);
    state.borrow_mut().journal.term_input_from_reader = from_reader;
    let terms = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_distinct_terms(&conn).ok())
        .unwrap_or_default();
    {
        let mut s = state.borrow_mut();
        s.journal_term_input.set_suggestions(terms);
    }
    // Borrow dropped: `show()`'s set_text can now re-enter the `changed`
    // handler's `state.borrow()` without a double-borrow.
    state.borrow().journal_term_input.show();
    state.borrow_mut().input_mode = InputMode::JournalTermInput;
}

/// Confirm the entered term: hide the box, return to the overlay, then activate
/// the filter (lands on match 1). The term is the typed text (else the
/// highlighted suggestion); a freely-typed term reaches the FTS fallback even
/// with zero tags. Borrows are scoped so they are dropped before
/// `activate_filter` (which re-borrows `state` mutably) runs — otherwise a
/// RefCell double-borrow would panic at runtime.
pub(crate) fn confirm_term_input(state: &Rc<RefCell<AppState>>) {
    let term = {
        let s = state.borrow();
        s.journal_term_input.query_term()
    };
    {
        let s = state.borrow();
        s.journal_term_input.hide();
    }
    state.borrow_mut().input_mode = InputMode::JournalOverlay;
    if let Some(term) = term {
        activate_filter(state, &term);
    }
}

/// `/` in the journal overlay: open the reader `search_bar` to type a regex to
/// search the CURRENT overlay entry.
///
/// BORROW SAFETY: `search_bar.show()` synchronously calls `entry.set_text("")`
/// and `grab_focus()`. The bar's Entry has NO `changed`/signal handler that
/// re-enters `state`, so this is safe under a short borrow; still, scope the
/// widget borrow and set `input_mode` in a fresh borrow (consistent with
/// `open_term_input`).
pub(crate) fn open_overlay_search(state: &Rc<RefCell<AppState>>) {
    {
        let s = state.borrow();
        s.search_bar.show();
    }
    // Set the origin on EVERY open path — the field is sticky (gloss's open sets
    // it to GlossOverlay and nothing resets it), so relying on the init default
    // would route a journal `/` to the gloss overlay after any prior gloss
    // search. Both writes are plain fields (no signal), safe in one borrow.
    let mut s = state.borrow_mut();
    s.overlay_search_origin = InputMode::JournalOverlay;
    s.input_mode = InputMode::OverlaySearchInput;
}

/// Return in the `/` bar: read the typed regex, hide the bar, return to the
/// overlay, and set the pattern on the current overlay buffer. Empty query is a
/// no-op (search stays whatever it was). Stores the pattern as the MRU.
///
/// BORROW SAFETY: read `query()` and `hide()` under scoped borrows dropped
/// before the mutating `borrow_mut`, so no `&s` borrow is held across the
/// `set_search_matches` write.
pub(crate) fn confirm_overlay_search(state: &Rc<RefCell<AppState>>) {
    let query = {
        let s = state.borrow();
        s.search_bar.query()
    };
    {
        let s = state.borrow();
        s.search_bar.hide();
    }
    state.borrow_mut().input_mode = InputMode::JournalOverlay;
    let pattern = query.trim();
    if pattern.is_empty() {
        return;
    }
    let mut s = state.borrow_mut();
    // Collect over the WHOLE entry (every page), not just the rendered buffer, so
    // a match on another page of a paginated Q&A is found. Match offsets are into
    // the whole-entry char basis (`whole_entry_text` == the paragraph basis
    // `page_char_span` measures); `jump_to_whole_offset` turns to the match's page
    // and `set_search_matches` paints whatever falls on the shown page.
    let text = s.journal_overlay.whole_entry_text();
    let search = crate::input::overlay_search::OverlaySearch {
        pattern: pattern.to_string(),
        matches: crate::input::overlay_search::collect(&text, pattern),
        current: 0,
    };
    if search.matches.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_NO_MATCHES, 2);
    } else if let Some((off, _)) = search.matches.first() {
        // Turn to the page holding the first match before painting it.
        s.journal_overlay.jump_to_whole_offset(*off as usize);
    }
    s.journal_overlay.set_search_matches(&search);
    s.journal.last_pattern = Some(pattern.to_string());
    s.journal.search = Some(search);
}

/// n / N in the journal overlay: step matches within the current entry. If no
/// live search but an MRU pattern exists, revive it first (post-Escape n/N).
///
/// BORROW SAFETY: one `borrow_mut` held throughout. `s.journal.search`
/// (mutable) and `s.journal_overlay` (getter, immutable) alias `s`, so the
/// tags are cloned + the buffer taken into locals FIRST; the mutable step of
/// `search.current` happens in a scoped block; then `apply` is called on the
/// locals with `search.as_ref()` — no getter borrow overlaps the mutable use.
pub(crate) fn step_overlay_search(state: &Rc<RefCell<AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    if s.journal.search.is_none() {
        // Revive the MRU pattern (post-Escape n/N). Collect over the WHOLE entry
        // so revived stepping also crosses page boundaries.
        let Some(pat) = s.journal.last_pattern.clone() else {
            return;
        };
        let text = s.journal_overlay.whole_entry_text();
        let search = crate::input::overlay_search::OverlaySearch {
            pattern: pat.clone(),
            matches: crate::input::overlay_search::collect(&text, &pat),
            current: 0,
        };
        if search.matches.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_NO_MATCHES, 2);
            return;
        }
        if let Some((off, _)) = search.matches.first() {
            s.journal_overlay.jump_to_whole_offset(*off as usize);
        }
        s.journal_overlay.set_search_matches(&search);
        s.journal.search = Some(search);
    }
    let scroll_to = {
        let search = s.journal.search.as_mut().unwrap();
        match crate::input::overlay_search::step(search.current, search.matches.len(), forward) {
            Some(next) => {
                search.current = next;
                search.matches.get(next).map(|(a, _)| *a)
            }
            None => None,
        }
    };
    // Jump to the match's page (whole-body offset) and re-paint the highlights on
    // whatever page is now shown. The clone is a cheap snapshot to satisfy the
    // borrow checker (set_search_matches takes &self while s.journal.search is
    // borrowed).
    if let Some(off) = scroll_to {
        let search = s.journal.search.clone().unwrap();
        s.journal_overlay.set_search_matches(&search);
        // Turn to the match's page (updates cursor bar + scrolls it on-screen).
        s.journal_overlay.jump_to_whole_offset(off as usize);
    } else if let Some(search) = s.journal.search.clone() {
        // At an edge (no move): just re-assert the current highlight in place.
        s.journal_overlay.set_search_matches(&search);
    }
}

/// Clear the active overlay search (Escape). Keeps `last_pattern` for MRU
/// revival. Returns `true` when it cleared a live search (caller then stays in
/// the overlay), `false` when there was none (caller falls to filter/close).
///
/// BORROW SAFETY: single `borrow_mut`; tags cloned + buffer taken into locals
/// before `clear`, so no getter borrow overlaps writing `s.journal.search`.
pub(crate) fn clear_overlay_search(state: &Rc<RefCell<AppState>>) -> bool {
    let mut s = state.borrow_mut();
    if s.journal.search.is_none() {
        return false;
    }
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    true
}

/// From the journal source_text markup (`<speaker>…</speaker>\n<segment>text…`),
/// return the first bare CONTENT line (inside a `<segment>`/`<stage>` tag), tags
/// stripped and trimmed. Empty if there is no content line. Used only by the
/// citationless text-match fallback in `jump_to_journal_source_start` — citation
/// match is primary, so this is a rare `.txt`-only path.
fn first_plain_source_line(source_text: &str) -> String {
    for raw in source_text.lines() {
        let line = raw.trim();
        // Skip pure speaker headers and blank/tag-only lines.
        if line.is_empty() || line.starts_with("<speaker>") {
            continue;
        }
        // Strip a single leading/trailing tag pair (<segment>…</segment>, <stage>…).
        let stripped = line
            .trim_start_matches("<segment>")
            .trim_start_matches("<stage>")
            .trim_end_matches("</segment>")
            .trim_end_matches("</stage>")
            .trim();
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    String::new()
}

/// Buffer index of the first dialogue line of the passage at `start_citation`
/// within `work`. Primary match is the citation tuple `(div1,div2,line_in_div)`;
/// the fallback matches the first plain source line of `source_text` (which
/// carries `<speaker>/<segment>` markup) against line text. Advances to the first
/// `is_dialogue` line, then maps through `line_map.work_to_buffer`. `None` when
/// the citation/text doesn't resolve.
pub(crate) fn source_first_buffer_line(
    work: &crate::db::models::Work,
    line_map: Option<&crate::text_file_map::LineMap>,
    start_citation: &str,
    source_text: &str,
) -> Option<usize> {
    // start_citation is `ABBR.div1.div2.line_in_div`; match on the numeric tail
    // (the abbrev may carry an edition suffix), same as the gloss path.
    let target = crate::app::parse_citation(start_citation);

    // Citation tuple is unique → primary match; citationless text match is the
    // .txt-only fallback (source_text carries <speaker>/<segment> markup, so strip
    // to the first bare content line before comparing).
    let by_citation = target
        .and_then(|t| work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t));
    let first_src = first_plain_source_line(source_text);
    let start_idx = by_citation.or_else(|| {
        if first_src.is_empty() {
            None
        } else {
            work.lines.iter().position(|l| l.text.trim() == first_src)
        }
    })?;
    let work_idx = work.lines[start_idx..]
        .iter()
        .position(|l| l.is_dialogue)
        .map(|off| start_idx + off)
        .unwrap_or(start_idx);

    match line_map {
        Some(lm) => lm.work_to_buffer.get(work_idx).copied(),
        None => Some(work_idx),
    }
}

/// Land the cursor on the first dialogue line of the CURRENT journal page's
/// source passage, when that page is a passage page whose source resolves in the
/// current work. Returns `true` on a successful jump, `false` for scene/corpus
/// notes (no `start_citation`) or a passage from another work. Parallels
/// `gloss::jump_to_gloss_source_start`.
pub(crate) fn jump_to_journal_source_start(s: &mut AppState) -> bool {
    let (start_citation, source_text) = match s.journal.pages.get(s.journal.page_index) {
        // Only passage pages carry a source citation; scene/corpus notes don't.
        Some(p) => match &p.start_citation {
            Some(c) => (c.clone(), p.source_text.clone().unwrap_or_default()),
            None => return false,
        },
        None => return false,
    };

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };

    let buf_idx = match source_first_buffer_line(work, s.line_map.as_ref(), &start_citation, &source_text) {
        Some(i) => i,
        None => return false,
    };

    crate::input::navigation::jump_to_line(s, buf_idx);
    true
}

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        let mut s = state.borrow_mut();
        // Space source loop: full teardown (quit the loop mpv, restore the
        // main player) — leaving the overlay ends the loop.
        crate::input::actions::chat::chat_loop_teardown(&mut s);
        s.journal_overlay.hide();
        // Recolor the main card BEFORE update_highlight (which re-applies the tint
        // for reader_gloss_lines), so a reader-gloss created/edited in the overlay
        // colors immediately on return.
        crate::app::return_to_reader_mode(&mut s);
        // Land on the passage's source start when the viewer TRAVERSED to this
        // page (Ctrl+n/p, picker); when it still shows the page the overlay
        // opened on from the reader, restore the exact saved reading position —
        // a peek-and-Escape must not re-frame the page the reader left.
        // Covers Escape (routes through here).
        // Take return_pos/entry_page_id regardless so they don't leak into the
        // next open.
        let entry = s.journal.entry_page_id.take();
        let on_entry_page = entry.is_some()
            && s.journal.pages.get(s.journal.page_index).map(|p| p.id) == entry;
        let jumped = if on_entry_page {
            false
        } else {
            jump_to_journal_source_start(&mut s)
        };
        let pos = s.journal.return_pos.take();
        if !jumped {
            crate::app::restore_saved_position_resnap(&mut s, pos);
        }
        // Reset any active term filter so it never leaks into the next overlay
        // session: on the two-stage Esc the filter is already None here, but a
        // close that skips the first Esc (or a future close route) could leave
        // it Some, which would make the next open's first Ctrl+n/p walk the
        // stale match list and the first Esc "clear" a filter never set.
        s.journal.filter = None;
        // Also drop any overlay search + MRU so neither leaks into the next
        // overlay session. Clear the overlay's stored whole-body match spans too,
        // or the next open's render_page would re-paint them.
        s.journal_overlay.clear_search_tags();
        s.journal.search = None;
        s.journal.last_pattern = None;
        return;
    }

    open_journal_scene(state, JournalOpenScope::SegmentElseBand);
}

/// How far `open_journal_scene` may widen its search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum JournalOpenScope {
    /// The `\` overlay cycle: a `scope='passage'` entry covering the lap
    /// anchor, or nothing. A miss returns false silently — no toast, no state
    /// change — so `overlay_cycle::advance` can skip the stop.
    SegmentOnly,
    /// Ctrl+j: the segment entry if there is one, else the whole scene band.
    SegmentElseBand,
}

impl JournalOpenScope {
    /// Whether a segment miss may fall through to the chapter band.
    fn allows_band_fallback(self) -> bool {
        matches!(self, JournalOpenScope::SegmentElseBand)
    }
}

/// Whether the journal Q&A stop has anything to show for the cursor: a
/// `scope='passage'` entry whose citation span covers the lap anchor. Performed
/// WITHOUT opening the overlay or touching any state.
///
/// Matches `open_journal_scene(state, JournalOpenScope::SegmentOnly)` exactly —
/// same anchor, same query. The `\` overlay cycle probes with this before
/// tearing down the current overlay; see `gloss::gloss_covers_cursor`, whose
/// span-only shape this mirrors.
pub(crate) fn journal_has_content_at_cursor(state: &Rc<RefCell<AppState>>) -> bool {
    let s = state.borrow();
    if s.current_work.is_none() {
        return false;
    }
    let abbrev = current_work_abbrev(&s);
    let Ok(conn) = crate::db::queries::open_db() else {
        return false;
    };

    // SPAN-SCOPED ONLY (2026-07-27). The scene-band fallback that used to sit
    // here answered "does this CHAPTER have any Q&A" — a question with no
    // reference to the cursor — so `\` opened whichever entry sorted oldest in
    // the band. The `\` lap shows material about the segment under the cursor,
    // so the only hit that counts is a `scope='passage'` entry whose citation
    // span contains the anchor. `scope='scene'` entries carry no span and are
    // deliberately unreachable by `\`; Ctrl+j and the picker still reach them.
    let anchor = lap_anchor_for(&s);
    s.current_work
        .as_ref()
        .and_then(|w| s.work_line_for_buffer(anchor).and_then(|wi| w.lines.get(wi)))
        .map(|l| (l.div1, l.div2, l.line_in_div))
        .and_then(|(d1, d2, lid)| {
            crate::db::journal::find_journal_page_for_line(&conn, &abbrev, d1, d2, lid).ok()?
        })
        .is_some()
}

/// Open the journal Q&A stop for the cursor. Returns whether an overlay was
/// actually opened: false means nothing covers the cursor and the caller is
/// still in whatever mode it started in. `scope` controls how far a segment
/// miss may widen: `SegmentOnly` (the `\` overlay cycle, via
/// `overlay_cycle::advance`) returns false silently so the lap can SKIP an
/// empty journal stop instead of ending there; `SegmentElseBand` (Ctrl+j, via
/// `toggle_overlay`) falls through to the whole scene band and keeps the miss
/// toast this function emits when even the band is empty.
pub(crate) fn open_journal_scene(
    state: &Rc<RefCell<AppState>>,
    scope: JournalOpenScope,
) -> bool {
    // First: if the cursor line itself falls inside a passage Q&A's span, open
    // the overlay LANDED on that specific entry — regardless of which band the
    // picker groups it under. Resolved from the cursor's exact
    // `(div1, div2, line_in_div)` (not just the scene band), keyed by
    // `Work.canonical_abbrev` like every journal path. Falls through to the
    // scene-band / picker behavior below when the cursor isn't inside any
    // passage entry.
    let cursor_hit = {
        let s = state.borrow();
        if s.current_work.is_none() {
            return false;
        }
        let abbrev = current_work_abbrev(&s);
        // The lap anchor, not the live cursor — arriving here via `\` from the
        // gloss stop leaves the cursor at the END of the glossed passage, so
        // probing `current_line` asks about a different line than
        // `journal_has_content_at_cursor` just approved.
        let anchor = lap_anchor_for(&s);
        s.current_work
            .as_ref()
            .and_then(|w| {
                s.work_line_for_buffer(anchor)
                    .and_then(|wi| w.lines.get(wi))
            })
            .map(|l| (l.div1, l.div2, l.line_in_div))
            .and_then(|(d1, d2, lid)| {
                crate::db::queries::open_db().ok().and_then(|conn| {
                    crate::db::journal::find_journal_page_for_line(&conn, &abbrev, d1, d2, lid).ok()
                })?
            })
    };
    if let Some((pd1, pd2, entry_id)) = cursor_hit {
        let mut s = state.borrow_mut();
        // Same prior-session cleanup the scene-band open path performs, so a
        // stale filter/search never leaks into this entry-landed session.
        s.journal.filter = None;
        s.journal_overlay.clear_search_tags();
        s.journal.search = None;
        s.journal.last_pattern = None;
        s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
        s.input_mode = InputMode::JournalOverlay;
        // The passage entry is stored under its own scene band; land ON it by id.
        land_on_page(&mut s, JournalBand::Scene(pd1, pd2), entry_id);
        s.journal.entry_page_id = s.journal.pages.get(s.journal.page_index).map(|p| p.id);
        return true;
    }

    // The `\` cycle stops here: no passage Q&A covers the segment, so the stop
    // has nothing to show. Return silently — `overlay_cycle::advance` skips to
    // the next stop and owns the all-empty toast. Emitting the band path's
    // "No journal entry for this segment" toast here would fire on every lap
    // through a segment that simply has no Q&A of its own.
    if !scope.allows_band_fallback() {
        return false;
    }

    let (d1, d2, scene_empty) = {
        let s = state.borrow();
        if s.current_work.is_none() {
            return false;
        }
        let (d1, d2) = crate::app::scene_synopsis::current_scene_divs(&s);
        // Use the SAME query the Scene band renders with (find_scene_band_pages =
        // scene Q&As + passage Q&As in this (d1,d2)), so a scene that has only
        // passage entries is NOT treated as empty.
        let work_abbrev = current_work_abbrev(&s);
        let scene_pages = crate::db::queries::open_db()
            .ok()
            .and_then(|conn| {
                crate::db::journal::find_scene_band_pages(&conn, &work_abbrev, d1, d2).ok()
            })
            .unwrap_or_default();
        (d1, d2, scene_pages.is_empty())
    };

    // Scene band has no Q&A: toast and stay in the reader instead of landing
    // on a blank scene band or popping the work-wide picker (the picker keeps
    // its own dedicated bind).
    if scene_empty {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            "No journal entry for this segment",
            3,
        );
        return false;
    }

    let mut s = state.borrow_mut();
    // Defensive: never inherit a term filter from a prior session. toggle_overlay's
    // close branch already clears it, but the `\` overlay cycle (cycle_from_journal)
    // is a close route that bypasses toggle_overlay — resetting at every open covers
    // it regardless of how the previous session ended.
    s.journal.filter = None;
    // Drop the overlay's stored whole-body search spans too, so a prior session's
    // matches don't re-paint on this open's first render_page.
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    s.journal.last_pattern = None;
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.journal_band = JournalBand::Scene(d1, d2);
    s.journal.page_index = 0;
    s.input_mode = InputMode::JournalOverlay;
    render_current(&mut s);
    // Remember the page this reader-entered open landed on: while the viewer
    // is still on it, close restores the exact reading position (no source
    // jump). Must be read AFTER render_current loads the band's pages.
    s.journal.entry_page_id = s.journal.pages.get(s.journal.page_index).map(|p| p.id);
    true
}

pub(crate) fn close_overlay(state: &Rc<RefCell<AppState>>) {
    // Closing the overlay must not leave a stale diff-highlight for the next
    // open session (Task 7); a revision browse (Task 8) drops with it.
    state.borrow().journal_overlay.clear_rewrite_diff();
    state.borrow_mut().rewrite_browse = None;
    // A journal session that ends any other way must not leave an origin
    // marker for the NEXT journal open to inherit — `\` there would jump to a
    // synopsis instead of advancing the overlay cycle.
    state.borrow_mut().journal_from_synopsis = None;
    // Stop any cached-segment TTS still playing, so closing the overlay silences
    // it (mirrors close_gloss_to_reader's s.tts.stop()).
    state.borrow_mut().tts.stop();
    if state.borrow().input_mode == InputMode::JournalOverlay {
        toggle_overlay(state);
    }
}

/// Synopsis `\`: open the band's newest scene-scoped Q&A. Returns whether an
/// entry was opened; false means the band has none and the caller keeps the
/// synopsis open (this function emits the miss toast).
pub(crate) fn open_scene_qa_from_synopsis(state: &Rc<RefCell<AppState>>) -> bool {
    let (abbrev, div1, div2, unit) = {
        let s = state.borrow();
        if s.current_work.is_none() {
            return false;
        }
        let (d1, d2) = s.synopsis_overlay_scene;
        // "chapter" for prose, "scene" for plays — match the surface's wording.
        let unit = if crate::app::scene_synopsis::is_chapter_work(&s) {
            "chapter"
        } else {
            "scene"
        };
        (current_work_abbrev(&s), d1, d2, unit)
    };

    let page = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::journal::find_newest_scene_page(&conn, &abbrev, div1, div2).ok()
        })
        .flatten();

    let Some(page) = page else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("No journal entry for this {}", unit),
            3,
        );
        return false;
    };

    let mut s = state.borrow_mut();
    // Close the synopsis (it renders in the gloss overlay widget) before the
    // journal takes over.
    s.gloss_overlay.hide();
    // Same prior-session cleanup every journal open path performs, so a stale
    // filter/search never leaks into this session.
    s.journal.filter = None;
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    s.journal.last_pattern = None;
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.journal_from_synopsis = Some((div1, div2));
    s.input_mode = InputMode::JournalOverlay;
    let id = page.id;
    land_on_page(&mut s, JournalBand::Scene(div1, div2), id);
    s.journal.entry_page_id = s.journal.pages.get(s.journal.page_index).map(|p| p.id);
    true
}

/// Journal `\` when the session was entered from a synopsis: close the journal
/// and reopen that synopsis. Returns false when there is no origin marker, so
/// the caller falls through to the overlay cycle.
pub(crate) fn return_to_synopsis(state: &Rc<RefCell<AppState>>) -> bool {
    let origin = {
        let mut s = state.borrow_mut();
        take_synopsis_origin(&mut s.journal_from_synopsis)
    };
    let Some((div1, div2)) = origin else {
        return false;
    };
    {
        let mut s = state.borrow_mut();
        s.journal_overlay.clear_rewrite_diff();
        s.rewrite_browse = None;
        s.tts.stop();
        s.journal_overlay.hide();
        s.journal.entry_page_id.take();
        let pos = s.journal.return_pos.take();
        crate::app::return_to_reader_mode(&mut s);
        crate::app::restore_saved_position_resnap(&mut s, pos);
    }
    // Reopen the band we came from, not wherever the cursor now sits — the
    // journal session may have crossed a chapter/scene boundary (Ctrl+n/p
    // traversal, or a jump-to-source that moved return_pos), so recomputing
    // the band from the post-restore cursor would silently show a different
    // synopsis than the one this journal session was opened from.
    //
    // On cache miss (no synopsis for that band), we're already back in
    // reader mode (return_to_reader_mode above), so the caller's overlay-cycle
    // fallthrough would try to open the NEXT overlay in the cycle, not leave
    // the user stranded with nothing — report false rather than force-opening
    // whatever `current_synopsis_key` would land on.
    crate::app::scene_synopsis::show_synopsis_overlay_for(state, div1, div2)
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
    // Locate the target BEFORE rendering. The old flow fully rendered page 0
    // (pagination + TTS recolor + landing rewrite-diff) just to load the page
    // list, then re-rendered at the target — twice the work and a visible
    // flash of the wrong entry on every Ctrl+j entry-hit open.
    s.journal.page_index = load_band_pages(s)
        .iter()
        .position(|p| p.id == target_id)
        .unwrap_or(0);
    render_current(s);
}

/// Open the journal overlay from the reader landed on entry `entry_id` in its
/// `(div1, div2)` scene band — the same sequence as the reader's Ctrl+j
/// entry-hit path (prior-session cleanup, return_pos, entry_page_id peek
/// semantics). Used by the vocab Q&A flow to reveal a stored or freshly saved
/// entry.
pub(crate) fn open_overlay_at_entry(s: &mut AppState, div1: i64, div2: i64, entry_id: i64) {
    s.journal.filter = None;
    s.journal_overlay.clear_search_tags();
    s.journal.search = None;
    s.journal.last_pattern = None;
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.input_mode = InputMode::JournalOverlay;
    land_on_page(s, JournalBand::Scene(div1, div2), entry_id);
    s.journal.entry_page_id = s.journal.pages.get(s.journal.page_index).map(|p| p.id);
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

/// Paint the diff between the DISPLAYED journal entry and its most recent stored
/// revision, so opening/landing on an entry shows what its last rewrite changed
/// (persists until Escape; survives page turns via the overlay's per-page
/// re-apply). Clears the highlight when the entry has no revision history. Call
/// after landing on / rendering an entry. Safe to call with the overlay closed
/// (apply/clear are buffer ops; the tag simply isn't visible).
pub(crate) fn refresh_entry_diff_highlight(s: &mut AppState) {
    let Some(page) = displayed_journal_page(s) else {
        s.journal_overlay.clear_rewrite_diff();
        return;
    };
    let latest = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::list_revisions(&conn, "journal", page.id).ok())
        .and_then(|revs| revs.into_iter().last());
    let Some(prev) = latest else {
        s.journal_overlay.clear_rewrite_diff();
        return;
    };
    let base = answer_prefix_chars(&page.question);
    let ranges: Vec<(i32, i32)> = crate::input::rewrite_diff::changed_ranges(&prev.body, &page.answer)
        .into_iter()
        .map(|(a, b)| (a + base, b + base))
        .collect();
    s.journal_overlay.apply_rewrite_diff(&ranges);
}

/// `Ctrl+n` / `Ctrl+p`: step through EVERY Q&A in the work, across bands, in the
/// same order the `Alt+p` picker uses (`find_all_pages_ordered`: whole-work
/// pages first, then by div1/div2, then timestamp/id; passage Q&As interleave in
/// their scene band). At the last page of a band, `Ctrl+n` rolls into the first
/// Q&A of the next band; `Ctrl+p` symmetrically. Clamped at the work's first /
/// last Q&A (no wrap). Was previously a within-band clamp.
pub(crate) fn nav_page(state: &Rc<RefCell<AppState>>, delta: i32) {
    // Moving to a different Q&A page invalidates any diff-highlight from a
    // custom-prompt rewrite on the page we're leaving (Task 7); a revision
    // browse (Task 8) drops with it.
    state.borrow().journal_overlay.clear_rewrite_diff();
    state.borrow_mut().rewrite_browse = None;
    // Filtered subset walk: step within the term matches, render read-only,
    // and skip the unfiltered work-wide logic entirely.
    {
        let mut s = state.borrow_mut();
        if let Some(filter) = s.journal.filter.as_mut() {
            let len = filter.matches.len();
            if let Some(next) = flat_step(filter.pos, delta, len) {
                filter.pos = next;
                render_filtered_match(&mut s);
            }
            return;
        }
    }

    let mut s = state.borrow_mut();
    let Some(cur_id) = s.journal.pages.get(s.journal.page_index).map(|p| p.id) else {
        return;
    };
    let work_abbrev = current_work_abbrev(&s);
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
    let band = match band_for_page(target) {
        JournalBand::Author(_) => JournalBand::Author(
            s.current_work.as_ref().map(|w| w.author.clone()).unwrap_or_default(),
        ),
        other => other,
    };
    let target_id = target.id;
    land_on_page(&mut s, band, target_id);
}

/// Jump to the next/prev scene that has pages (skips empty scenes). Lands on
/// that scene's first page. From the Work band, delta>0 lands on the first
/// scene with pages, delta<0 on the last (the Work band sorts before scenes).
pub(crate) fn nav_scene(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    // Moving to a different scene invalidates any diff-highlight from a
    // custom-prompt rewrite on the entry we're leaving (Task 7).
    s.journal_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    let work_abbrev = current_work_abbrev(&s);
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
        JournalBand::Author(_) => return,       // author band is jump-only, not part of the walk
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
    // Switching bands invalidates any diff-highlight from a custom-prompt
    // rewrite on the entry we're leaving (Task 7).
    s.journal_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    if s.journal_band == JournalBand::Work {
        return;
    }
    s.journal_band = JournalBand::Work;
    s.journal.page_index = 0;
    render_current(&mut s);
    // Band jumps (Alt+s/w/a) browse Q&As fresh: drop the landed entry's
    // last-rewrite diff tint render_current just painted — that highlight is
    // for rewrite/restore landings, not band browsing.
    s.journal_overlay.clear_rewrite_diff();
}

/// Switch to the Scene band for the main card's current cursor line and render
/// it. Jump-only — a direct jump to `Scene(current_scene_divs)`, complementing
/// `Alt+n/p` (which only step through the scene list) and matching the band the
/// overlay first opens on. Lets the author/work band return to the reading
/// position's scene without closing and reopening the overlay.
pub(crate) fn nav_to_scene_band(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Switching bands invalidates any diff-highlight from a custom-prompt
    // rewrite on the entry we're leaving (Task 7).
    s.journal_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    if s.current_work.is_none() {
        return;
    }
    let (d1, d2) = crate::app::scene_synopsis::current_scene_divs(&s);
    if s.journal_band == JournalBand::Scene(d1, d2) {
        return;
    }
    s.journal_band = JournalBand::Scene(d1, d2);
    s.journal.page_index = 0;
    render_current(&mut s);
    // Band jumps browse fresh — no landed-entry rewrite-diff tint (see
    // nav_to_work_band).
    s.journal_overlay.clear_rewrite_diff();
}

/// Switch to the Author band (corpus-scope pages for the current work's author)
/// and render it. Jump-only — not part of the sequential scene/work band walk.
///
/// # nav_to_author_band verified via e2e (needs GTK AppState)
pub(crate) fn nav_to_author_band(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Switching bands invalidates any diff-highlight from a custom-prompt
    // rewrite on the entry we're leaving (Task 7).
    s.journal_overlay.clear_rewrite_diff();
    s.rewrite_browse = None;
    let author = s
        .current_work
        .as_ref()
        .map(|w| w.author.clone())
        .unwrap_or_default();
    if author.is_empty() {
        return;
    }
    if s.journal_band == JournalBand::Author(author.clone()) {
        return;
    }
    s.journal_band = JournalBand::Author(author);
    s.journal.page_index = 0;
    render_current(&mut s);
    // Band jumps browse fresh — no landed-entry rewrite-diff tint (see
    // nav_to_work_band).
    s.journal_overlay.clear_rewrite_diff();
}

/// Set up the journal overlay for a passage Q&A and open the ask card.
///
/// Called from the visual-selection path (`src/input/visual.rs`). The caller
/// has already exited visual mode / closed any conflicting overlay and set
/// `return_pos`.
///
/// - Sets `journal.pending_passage` with the `<speaker>/<segment>` markup.
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
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.journal.entry_page_id = None; // this open is itself navigation: close may source-jump
    s.journal.prompt_mode = JournalPromptMode::Ask;
    let band = JournalBand::Passage { div1, div2, start, end };
    s.journal.pending_passage = Some(PendingPassage {
        source_text,
        band: band.clone(),
    });
    s.journal_band = band;
    s.journal.page_index = 0;
    s.input_mode = crate::app::InputMode::JournalOverlay;
    // Ctrl+Tab focus toggle: a freshly opened ask card always starts focused.
    s.ask_card_focus = true;
    render_current(&mut s);
    s.journal_overlay.open_ask_card(
        "Ask a question about this passage",
        "Ctrl+Enter submit",
        "", // no legend: a new question has no answer, so we drop straight to INSERT
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // No existing answer (brand-new question) → auto-enter INSERT so the reader
    // types immediately.
    let _ = s
        .journal_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}

/// Ctrl+a in the journal overlay: open the ask card for a NEW Q&A, choosing the
/// band correctly whether or not a term/one-match filter is active.
///
/// With no filter, this is just `begin_ask` on the current band. With a filter
/// (recent-Q&A picker, Ctrl+f corpus hit, or an `f` term browse) the overlay is
/// showing an entry that may sit in a different band — or a different WORK — than
/// the origin cursor. Two cases:
///
/// - **Cross-work match** (`displayed_entry_is_cross_work`): the entry belongs to
///   a work OTHER than the one loaded, so `ask_claude`'s grounding (built from
///   `current_work`) would be the wrong work. There is no safe home band on
///   screen — swallow with the clear-filter toast, exactly as before.
/// - **Same-work match** (the recent-Q&A / corpus-hit single-entry cases, which
///   load the entry's edition first, and any `f`-match in the current work):
///   retarget `journal_band` to the DISPLAYED entry's own band (via
///   `band_for_rewrite`) so the new Q&A attaches to the entry the reader is
///   actually looking at, then ask.
pub(crate) fn begin_ask_or_filter_gate(state: &Rc<RefCell<AppState>>) {
    let filtered = state.borrow().journal.filter.is_some();
    if filtered {
        if displayed_entry_is_cross_work(&state.borrow()) {
            crate::input::navigation::show_chapter_toast_secs(
                &state.borrow(),
                "Clear the term filter (Esc) for this key",
                3,
            );
            return;
        }
        // Same-work filter match: point the ask at the displayed entry's own
        // band before asking, so the new Q&A lands on it (not the origin band
        // journal_band still holds under a filter). Then END the filter: the
        // reader is committing to this band, and leaving `journal.filter` set
        // would make the post-submit `render_current` / later nav diverge onto
        // the stale one-match browse view. The overlay stays on this entry (the
        // ask card opens over the current render); the answer save + render_current
        // then repaint the band normally.
        let band = {
            let s = state.borrow();
            displayed_journal_page(&s).map(|p| band_for_rewrite(&p))
        };
        let mut s = state.borrow_mut();
        if let Some(band) = band {
            s.journal_band = band;
        }
        s.journal.filter = None;
    }
    begin_ask(state);
}

pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal.prompt_mode = JournalPromptMode::Ask;
    // Ctrl+Tab focus toggle: a freshly opened ask card always starts focused.
    s.ask_card_focus = true;
    let title = match s.journal_band {
        JournalBand::Work => "Ask a question about the whole work",
        JournalBand::Scene(_, _) => "Ask a question about this scene",
        JournalBand::Passage { .. } => "Ask a question about this passage",
        JournalBand::Author(_) => "Ask a question about this author's corpus",
    };
    s.journal_overlay.open_ask_card(
        title,
        "Ctrl+Enter submit",
        "", // no legend: a new question has no answer, so we drop straight to INSERT
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // No existing answer (brand-new question) → auto-enter INSERT.
    let _ = s
        .journal_overlay
        .feed_ask_vim_key(crate::input::vim::VimKey::Char('i'));
}

/// Build the user message for a Ctrl+Enter rewrite. `context` is the band-aware
/// grounding block (work title/author, the band the Q&A now lives in, and the
/// relevant scene/passage text) — the same framing the original ask sends — so a
/// rewrite instruction that references the band (e.g. "now that this is in the
/// work band, broaden the opening") has the context it needs. The question, the
/// current answer, and the instruction follow in "revise this answer" shape.
///
/// A journal Q&A is often browsed (via the cross-work term filter) while reading
/// a DIFFERENT work than the one it was filed under. The rewrite must answer the
/// question as asked — about the work the QUESTION concerns — and must never open
/// with meta-commentary about which work is on screen. The `FOCUS` directive
/// suppresses the "the scene you're reading belongs to X, but your question is
/// about Y" preamble and stops the answer from drifting onto the on-screen work.
pub(crate) fn rewrite_user_message(context: &str, question: &str, answer: &str, instruction: &str) -> String {
    format!(
        "{}\n\nOriginal question:\n{}\n\nCurrent answer:\n{}\n\nFOCUS: Answer the question as it is asked — stay on the work, scene, and passage the QUESTION itself concerns. Do NOT switch to a different work, and do NOT open with any disambiguation or meta-commentary about which work the reader is currently viewing (no \"this passage belongs to X, but your question is about Y\"). Just give the revised answer.\n\nRevise the answer per this instruction (return only the revised answer):\n{}",
        context, question, answer, instruction,
    )
}

/// Grounding block for rewriting a CROSS-WORK filter entry (see
/// `displayed_entry_is_cross_work`): the reader is browsing this Q&A while
/// reading a different work, so we deliberately omit the on-screen work's
/// header and scene text. The entry's own stored passage source (if any) is the
/// only grounding; the question itself names its subject. Keeps the rewrite
/// anchored to what the Q&A is actually about, not what happens to be on screen.
fn cross_work_rewrite_context(passage_source: &str) -> String {
    let ps = passage_source.trim();
    if ps.is_empty() {
        "This Q&A stands on its own — answer the question as it is asked, about \
         the work and passage it concerns. Do not tie it to any other work."
            .to_string()
    } else {
        format!(
            "This Q&A concerns the following passage. Answer the question as it \
             is asked, about the work this passage belongs to — do not tie it to \
             any other work.\n\nPassage:\n{ps}"
        )
    }
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
            format!("Work: \"{}\" by {}\nThis Q&A is filed under the WHOLE WORK (not a single scene).", title, author)
        }
        JournalBand::Scene(d1, d2) => {
            let scene_text = crate::app::scene_synopsis::scene_text_windowed(
                s, *d1, *d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            );
            format!(
                "Work: \"{}\" by {}\nThis Q&A is filed under: {}\n\n{} text:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*d1, *d2), unit_label, scene_text,
            )
        }
        JournalBand::Passage { div1, div2, .. } => {
            let scene_text = crate::app::scene_synopsis::scene_text_windowed(
                s, *div1, *div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            );
            format!(
                "Work: \"{}\" by {}\nThis Q&A is filed under a PASSAGE in {}\n\n{} text:\n{}\n\nPassage:\n{}",
                title, author, crate::app::scene_synopsis::scene_label(*div1, *div2),
                unit_label, scene_text, passage_source,
            )
        }
        JournalBand::Author(author_name) => {
            format!("Author: {}\nThis Q&A is filed under the AUTHOR'S CORPUS (cross-work scope).", author_name)
        }
    }
}

/// `e` in the journal overlay: enter the in-place modal vim editor on the
/// current page's Q&A (replaces the old edit card). No-op if the band is empty.
pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Edit the DISPLAYED entry (filter match under `f`, else band page).
    let Some(page) = displayed_journal_page(&s) else {
        return;
    };
    let (q, a, kind) = (page.question.clone(), page.answer.clone(), page.kind.clone());
    let (block_fill, block_fg) = (s.theme.cursor_bg.clone(), s.theme.cursor_fg.clone());
    s.journal_overlay.enter_edit_buffer(&q, &a, &block_fill, &block_fg, &kind);
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
    let saved_id;
    {
        let mut s = state.borrow_mut();
        // Save the DISPLAYED entry (filter match under `f`, else band page).
        let disp = displayed_journal_page(&s);
        let undo_snap = disp.as_ref().map(|page| {
            (page.id, page.question.clone(), page.answer.clone(), page.claude_model.clone())
        });
        saved_id = disp.as_ref().map(|page| {
            let (id, model) = (page.id, page.claude_model.clone());
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ = crate::db::journal::update_journal_page(&conn, id, &q, &a, &model);
                purge_journal_audio(&conn, id);
            }
            id
        });
        s.journal_undo = undo_snap;
        // Under a filter, keep the in-memory match's answer in sync so the
        // filtered re-render shows the saved text.
        if let (Some(id), Some(filter)) = (saved_id, s.journal.filter.as_mut()) {
            if let Some(m) = filter.matches.get_mut(filter.pos) {
                if m.page.id == id {
                    m.page.question = q.clone();
                    m.page.answer = a.clone();
                }
            }
        }
        let in_filter = saved_id
            .and_then(|id| {
                s.journal
                    .filter
                    .as_ref()
                    .and_then(|f| f.matches.get(f.pos))
                    .map(|m| m.page.id == id)
            })
            .unwrap_or(false);
        if quit {
            // Leave the editor, restore the read view, land on the saved entry.
            s.journal_overlay.exit_edit_buffer();
            s.input_mode = crate::app::InputMode::JournalOverlay;
            if in_filter {
                render_filtered_match(&mut s);
            } else {
                render_current(&mut s);
                if let Some(id) = saved_id {
                    land_on_current_band_id(&mut s, id);
                }
            }
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED, 2);
        } else {
            // Stay in the editor; the buffer is now the saved baseline so the
            // dirty-check resets. Re-seed the seed to the just-saved buffer.
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED_IN_OVERLAY, 2);
        }
    }
    // After a non-quit `:w`, reset the editor's dirty baseline to the saved text
    // by re-entering with the saved Q&A (keeps the buffer, resets the seed).
    if !quit {
        let s = state.borrow();
        s.journal_overlay.reseed_edit_buffer(&q, &a);
    }
    // Background auto-tag the edited entry (all state borrows above are dropped).
    if let Some(id) = saved_id {
        spawn_retag(state, id, q.clone(), a.clone());
    }
}

/// Leave the vim editor. With unsaved changes and not `force`, warn and STAY
/// (vim semantics: `:q` on a modified buffer is refused; `:q!` forces). Called
/// for `:q` (force=false), Esc-in-Normal (force=false), and `:q!` (force=true).
pub(crate) fn vim_cancel(state: &Rc<RefCell<AppState>>, force: bool) {
    let dirty = state.borrow().journal_overlay.edit_is_dirty();
    if dirty && !force {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), "Unsaved changes \u{2014} :w to save, :q! to discard", 3);
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
        s.journal.vim_rewrite = Some((id, q, a, RewriteTarget::Answer));
    }
    let s = state.borrow();
    s.journal_overlay.open_ask_card(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} Esc cancel",
        "Ctrl+Enter with NO instruction\nrewrites the answer afresh under the default prompt.",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // Open in NORMAL (do NOT auto-enter INSERT) so the legend — which explains
    // that an empty Ctrl+Enter regenerates the answer — is readable before the
    // reader commits to typing an instruction. Press `i` to type; Ctrl+Enter on
    // the empty box regenerates.
}

/// `R` in the journal overlay (NOT the vim editor): open the ask card to collect
/// a rewrite instruction for the CURRENT Q&A page, sending it to Claude. Unlike
/// `vim_open_rewrite`, there is no edit buffer to persist first — the `(id, q, a)`
/// come straight from the displayed page. Stashes them in `journal.vim_rewrite`
/// so `submit_prompt` sends them with the instruction (same rewrite path as `R`
/// inside the editor). No-op (toast) on an empty band.
/// The journal entry currently DISPLAYED in the overlay: the active term-filter
/// match (`f`-opened cross-work entry) when a filter is set, else the current
/// band page. Rewrite/edit paths must read THIS, not `journal.pages[page_index]`
/// (which is the origin band and holds the wrong entry under a filter).
pub(crate) fn displayed_journal_page(s: &AppState) -> Option<crate::db::journal::JournalPage> {
    if let Some(filter) = s.journal.filter.as_ref() {
        return filter.matches.get(filter.pos).map(|m| m.page.clone());
    }
    s.journal.pages.get(s.journal.page_index).cloned()
}

/// True when the DISPLAYED entry belongs to a work OTHER than the one currently
/// being read — i.e. an `f`-opened cross-work term-filter match whose stored
/// `work_abbrev` differs from `current_work`'s. In that case the on-screen work's
/// scene text is the WRONG grounding for a rewrite (it would drag the answer onto
/// the work in front of the reader), so `rewrite_with_claude` grounds on the
/// entry's own passage source instead. Band pages (no filter) are always the
/// current work, so this is `false` for them.
fn displayed_entry_is_cross_work(s: &AppState) -> bool {
    let Some(filter) = s.journal.filter.as_ref() else {
        return false;
    };
    let Some(m) = filter.matches.get(filter.pos) else {
        return false;
    };
    let current = current_work_abbrev(s);
    !current.is_empty() && m.work_abbrev != current
}

pub(crate) fn begin_rewrite(state: &Rc<RefCell<AppState>>) {
    let page = {
        let s = state.borrow();
        displayed_journal_page(&s)
            .map(|p| (p.id, p.question.trim().to_string(), p.answer.trim().to_string()))
    };
    let Some((id, q, a)) = page else {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), crate::input::navigation::TOAST_NOTHING_TO_REWRITE, 2);
        return;
    };
    {
        let mut s = state.borrow_mut();
        s.journal.vim_rewrite = Some((id, q, a, RewriteTarget::Answer));
    }
    let s = state.borrow();
    s.journal_overlay.open_ask_card(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} Esc cancel",
        "Ctrl+Enter with NO instruction\nrewrites the answer afresh under the default prompt.",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // Open in NORMAL so the empty-Ctrl+Enter legend is readable first (see
    // vim_open_rewrite). Press `i` to type an instruction.
}

/// Body of `begin_rewrite` given an explicit `(id, q, a)`: stash the rewrite
/// tuple in `journal.vim_rewrite` and open the ask card in INSERT so
/// `submit_prompt` sends the typed instruction with this (id, q, a). Factored
/// so the `both` question-improve path opens the instruction card with the
/// IMPROVED question (not the stale on-screen one).
///
/// BORROW SAFETY: the `vim_rewrite` write is under a scoped `borrow_mut` that
/// drops before the read-only `borrow` used to open the ask card — mirrors
/// `begin_rewrite`.
pub(crate) fn begin_rewrite_with(state: &Rc<RefCell<AppState>>, id: i64, q: &str, a: &str) {
    {
        let mut s = state.borrow_mut();
        s.journal.vim_rewrite = Some((id, q.to_string(), a.to_string(), RewriteTarget::Both));
    }
    let s = state.borrow();
    s.journal_overlay.open_ask_card(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} Esc cancel",
        "Ctrl+Enter with NO instruction\nrewrites the answer afresh under the default prompt.",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    // Open in NORMAL so the empty-Ctrl+Enter legend is readable first.
}

/// `R` in the journal overlay: open the small target chooser
/// ("Rewrite: q question · a answer · b both · Esc cancel") and switch to
/// `InputMode::RewriteTargetChoice`. The single-key handler
/// (`handle_rewrite_target_key`) then routes to the answer/question/both path.
/// No-op (toast) when there is nothing displayed to rewrite. Mirrors
/// `show_delete_confirmation`'s centered amend-dialog box.
///
/// BORROW SAFETY: the "nothing to rewrite" check + the overlay-parent lookup are
/// scoped read borrows dropped before `add_overlay`; the state writes (the two
/// weakrefs + `input_mode`) happen in a single trailing `borrow_mut`, with no
/// widget call that re-enters `state` held across it.
pub(crate) fn open_rewrite_target(state: &Rc<RefCell<AppState>>) {
    // Nothing displayed → toast and stay in the overlay (no chooser).
    let has_entry = {
        let s = state.borrow();
        displayed_journal_page(&s).is_some()
    };
    if !has_entry {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), crate::input::navigation::TOAST_NOTHING_TO_REWRITE, 2);
        return;
    }

    // The chooser must stack above EVERYTHING, including the chat panel — the
    // panel-initiated `R` opens it while the chat panel is visible. The action
    // popup lives on the INNER (corpus_search_popup) overlay, but the chat panel
    // is a child of the window-level OUTER overlay, which draws over the whole
    // inner stack — a chooser added to the popup's immediate parent renders
    // UNDER the panel (the reported bug). Climb to the outermost Overlay
    // ancestor instead, exactly like `show_delete_confirmation`.
    let overlay_parent = {
        let s = state.borrow();
        let mut widget = s.action_popup_widget.container.parent();
        let mut outermost: Option<gtk4::Overlay> = None;
        while let Some(w) = widget {
            if let Ok(o) = w.clone().downcast::<gtk4::Overlay>() {
                outermost = Some(o);
            }
            widget = w.parent();
        }
        outermost
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

    let label = gtk4::Label::new(Some("Rewrite target"));
    label.add_css_class("amend-title");
    label.set_halign(gtk4::Align::Center);
    container.append(&label);

    let hint = gtk4::Label::new(Some(
        "q = question  \u{00b7}  a = answer  \u{00b7}  b = both  \u{00b7}  Esc = cancel",
    ));
    hint.add_css_class("amend-hint");
    hint.set_halign(gtk4::Align::Center);
    container.append(&hint);

    overlay_parent.add_overlay(&container);

    let mut s = state.borrow_mut();
    s.rewrite_target_container = Some(container.downgrade());
    s.rewrite_target_overlay = Some(overlay_parent.downgrade());
    s.input_mode = InputMode::RewriteTargetChoice;
}

/// Tear down the `R` target chooser box and return to the journal overlay.
///
/// BORROW SAFETY: single `borrow_mut`; `remove_overlay` operates on the taken
/// weakref upgrades (no `state` re-entry).
pub(crate) fn close_rewrite_target(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if let (Some(cw), Some(ow)) = (
        s.rewrite_target_container.take(),
        s.rewrite_target_overlay.take(),
    ) {
        if let (Some(c), Some(o)) = (cw.upgrade(), ow.upgrade()) {
            o.remove_overlay(&c);
        }
    }
    // Panel-initiated R (rewrite_return): a cancel (Esc) at the popup never
    // reaches the rewrite success closure, so restore the chat panel here. The
    // q/a/b dispatch paths ALSO call close_rewrite_target first, but they then
    // run the rewrite, whose success closure calls finish_panel_rewrite and
    // re-clears the flag — so setting ChatTranscript here is harmless for them
    // (immediately overwritten) and correct for the Esc path.
    if s.chat.rewrite_return {
        s.input_mode = InputMode::ChatTranscript;
        // Esc-cancel: no entry changed. If a rewrite is actually running, its
        // success closure will call finish_panel_rewrite and re-render; if this
        // was a cancel, we still owe a re-render + flag clear.
        // We cannot know here whether a rewrite will follow, so DON'T clear the
        // flag yet — the q/a/b handlers set it fresh, and the Esc arm clears it
        // explicitly (see keymap Task step 6). Just set the mode.
        return;
    }
    s.input_mode = InputMode::JournalOverlay;
}

/// `R` → question (`both == false`) or both (`both == true`): improve the
/// DISPLAYED entry's question via Claude, persist the improved question
/// immediately (answer unchanged), then either open the answer-rewrite
/// instruction card for the improved question (`both`) or regenerate the answer
/// afresh with a fixed reword instruction (question-only).
///
/// BORROW SAFETY: the displayed page is read under a scoped borrow that drops
/// before the toast; the toast takes another scoped borrow; `improve_question`
/// is then called with NO outer borrow held (it borrows `state` itself). Inside
/// the `on_done` closure: the `update_journal_page` write opens its own
/// `open_db_rw` conn and reads the model under a scoped `borrow` that drops
/// before `begin_rewrite_with`/`rewrite_with_claude` (each of which re-borrows
/// `state`) — no borrow is held across those calls.
pub(crate) fn rewrite_question_path(state: &Rc<RefCell<AppState>>, both: bool) {
    let page = {
        let s = state.borrow();
        displayed_journal_page(&s)
    };
    let Some(page) = page else {
        crate::input::navigation::show_chapter_toast_secs(&state.borrow(), crate::input::navigation::TOAST_NOTHING_TO_REWRITE, 2);
        return;
    };
    let id = page.id;
    let old_q = page.question.trim().to_string();
    let answer = page.answer.trim().to_string();
    // Capture the model up-front (like id/q/a) so a navigate during the async
    // improve-question round-trip can't stamp a different entry's model. Fall
    // back to the config model when the entry has none (legacy/unstamped rows),
    // mirroring rewrite_with_claude — else the immediate persist would write an
    // empty model string.
    let model = if page.claude_model.is_empty() {
        state.borrow().config.claude_model.clone()
    } else {
        page.claude_model.clone()
    };
    // Fetch the displayed entry's key terms up-front (like id/q/a/model) so a
    // navigate during the async improve round-trip can't cross entries. A fetch
    // failure yields no terms — the rewrite proceeds exactly as before.
    let terms = crate::db::queries::open_db_rw()
        .ok()
        .and_then(|conn| crate::db::journal::terms_for_entry(&conn, id).ok())
        .unwrap_or_default();

    crate::input::navigation::show_persistent_chapter_toast(&state.borrow(), "Improving question\u{2026}");

    improve_question(state, old_q, &terms, move |st, improved_q| {
        // Persist the improved question immediately with the unchanged answer,
        // reusing the entry's stored model (captured before the async call).
        {
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let _ =
                    crate::db::journal::update_journal_page(&conn, id, &improved_q, &answer, &model);
            }
        }
        // No borrow held here.
        if both {
            // Replace the persistent "Improving question…" toast (the else path
            // clears it via rewrite_with_claude's own toast; the both path opens
            // the instruction card, which does not, so dismiss it here) before
            // opening the answer-instruction card for the improved question.
            crate::input::navigation::show_chapter_toast_secs(&st.borrow(), "Question improved", 2);
            let to_panel = st.borrow().chat.rewrite_return;
            if to_panel {
                // Stash the (id, improved_q, answer, Both) tuple and open the
                // PANEL's instruction card (the overlay's is on a hidden widget).
                st.borrow_mut().journal.vim_rewrite =
                    Some((id, improved_q.clone(), answer.clone(), RewriteTarget::Both));
                crate::input::actions::chat::open_rewrite_instruction_input(&mut st.borrow_mut());
            } else {
                begin_rewrite_with(st, id, &improved_q, &answer);
            }
        } else {
            rewrite_with_claude(
                st,
                id,
                &improved_q,
                &answer,
                "The question was reworded for clarity; answer this (possibly reworded) question afresh, grounded as before.",
                RewriteTarget::Question,
            );
        }
    });
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
            crate::input::navigation::show_chapter_toast_secs(&state.borrow(), "Nothing to undo", 2);
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
    // Filter-aware: if the undone entry is the displayed cross-work filter match,
    // revert its in-memory q/a and re-render the filtered view (an `f`-opened
    // e/R edit is what set this snapshot). Otherwise the normal band re-render.
    let in_filter = s
        .journal
        .filter
        .as_ref()
        .and_then(|f| f.matches.get(f.pos))
        .map(|m| m.page.id == id)
        .unwrap_or(false);
    if in_filter {
        if let Some(filter) = s.journal.filter.as_mut() {
            if let Some(m) = filter.matches.get_mut(filter.pos) {
                m.page.question = question.clone();
                m.page.answer = answer.clone();
            }
        }
        render_filtered_match(&mut s);
    } else {
        render_current(&mut s);
        // The restore bumped the row's timestamp; re-find it so the band's
        // ordering doesn't leave the view on a different page.
        land_on_current_band_id(&mut s, id);
    }
    crate::input::navigation::show_chapter_toast_secs(&s, "Undid edit", 2);
}

pub(crate) fn close_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().journal_overlay.close_ask_card();
    let mut s = state.borrow_mut();
    // Discard any pending vim rewrite so a later create-ask isn't mistaken for a
    // rewrite (Esc out of the `R` prompt; the hand-edits were already saved).
    s.journal.vim_rewrite = None;
    // Ctrl+Tab focus toggle: closing the ask card always resets focus + dim so
    // no stale state leaks into the next open.
    s.ask_card_focus = true;
    s.journal_overlay.clear_focus_dim();
}

pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let text = state.borrow().journal_overlay.take_ask_text();
    // If a vim-editor rewrite is pending (`R`), this ask text is a REWRITE
    // instruction for the stashed (id, q, a) — not a new-Q&A question.
    let rewrite = state.borrow_mut().journal.vim_rewrite.take();
    close_prompt(state);
    if let Some((id, question, answer, target)) = rewrite {
        let instruction = text.trim();
        // An empty instruction means "no instructions — rerun with the existing
        // prompt": regenerate the answer afresh for the current (possibly
        // hand-edited) question, grounded as before. Any buffer hand-edits were
        // already persisted before the card opened (vim_open_rewrite) or come
        // straight from the displayed page (begin_rewrite*), so regenerating over
        // them loses nothing. The typed-instruction path below is unchanged.
        let instruction = if instruction.is_empty() {
            "No further instruction was given; answer this question afresh under the standard guidance, grounded as before."
        } else {
            instruction
        };
        rewrite_with_claude(state, id, &question, &answer, instruction, target);
        return;
    }
    submit_passage_question(state, &text);
}

/// A journal question with no non-whitespace content is not asked.
pub(crate) fn is_blank_question(text: &str) -> bool {
    text.trim().is_empty()
}

/// Run the new-Q&A ask chain for the current band/pending_passage: show the
/// loading card, derive scene terms, improve the phrasing, then call Claude.
/// Factored from `submit_prompt` so the gloss-overlay Ctrl+a passage-ask can
/// reuse the exact flow. `text` is the raw typed question. No-op if blank.
pub(crate) fn submit_passage_question(state: &Rc<RefCell<AppState>>, text: &str) {
    if is_blank_question(text) {
        return;
    }
    // Show the loading card immediately with the raw text so the UI isn't
    // dead during the improve-question round-trip; `ask_claude` re-shows it
    // with the improved phrasing once that call returns.
    {
        let s = state.borrow();
        let head = crate::app::scene_synopsis::cursor_head(&s);
        s.journal_overlay.set_running_head(&head.0, &head.1);
        s.journal_overlay.show_loading(text, "Refining question\u{2026}");
    }
    // A brand-new ask has no saved entry/tags yet — derive candidate terms from
    // the scene text first, then ground the phrasing on them.
    extract_scene_terms(state, text.to_string(), move |st, question, terms| {
        improve_question(st, question, &terms, move |st2, improved| {
            ask_claude(st2, &improved);
        });
    });
}

/// Char length of the "Q: …\n\n" prefix the journal body renders before the
/// answer, so answer-relative diff offsets can be shifted into buffer offsets.
/// Mirror the rendered body `format!("{}\n\n{}", prefix_question(question), answer)`
/// exactly, so an idempotent `prefix_question` (question already starting "Q:")
/// cannot desync the offset base. +2 for the "\n\n".
fn answer_prefix_chars(question: &str) -> i32 {
    (crate::ui::journal_overlay::prefix_question(question).chars().count() + 2) as i32
}

/// Send a journal Q&A `(question, answer)` plus a rewrite `instruction` to Claude,
/// save the revised answer to row `id`, and re-render. Factored from
/// `submit_edit_rewrite` so the vim editor's `R` path reuses the exact grounding
/// context + save + undo-snapshot behavior.
pub(crate) fn rewrite_with_claude(
    state: &Rc<RefCell<AppState>>,
    id: i64,
    question: &str,
    answer: &str,
    instruction: &str,
    target: RewriteTarget,
) {
    let (model, context, work_type, prev_question, prev_answer) = {
        let s = state.borrow();
        // The displayed entry (filter match under `f`, else band page) — NOT a
        // bare journal.pages lookup, which misses the cross-work filter entry.
        let Some(p) = displayed_journal_page(&s) else {
            return;
        };
        let p = &p;
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
        // A cross-work filter entry (browsing another work's Q&A while reading
        // this one) must NOT be grounded on the on-screen work's scene text —
        // that framing drags the rewritten answer onto the work in front of the
        // reader and prompts a "this belongs to a different work" preamble. Ground
        // it on its OWN stored passage source only; the question carries its own
        // subject. Same-work band pages keep the full band-aware grounding.
        let context = if displayed_entry_is_cross_work(&s) {
            cross_work_rewrite_context(&passage_source)
        } else {
            let band = band_for_rewrite(p);
            let anchor_work_line = s
                .journal
                .return_pos
                .and_then(|(buf, _top, _off)| s.work_line_for_buffer(buf))
                .unwrap_or(0);
            rewrite_context(&s, &band, &work_type, anchor_work_line, &passage_source)
        };
        (model, context, work_type, p.question.clone(), p.answer.clone())
    };
    let question_owned = question.to_string();
    let instruction_owned = instruction.to_string();
    let model_for_db = model.clone();
    let user_msg = rewrite_user_message(&context, question, answer, instruction);

    crate::input::navigation::show_persistent_chapter_toast(&state.borrow(), target.toast());

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
                if let Err(e) = crate::db::journal::append_revision(
                    &conn,
                    "journal",
                    id,
                    Some(&prev_question),
                    &prev_answer,
                    &model_for_db,
                    Some(&instruction_owned),
                ) {
                    crate::logging::log(&format!("REVISION: append failed: {}", e));
                }
                if let Err(e) = crate::db::journal::update_journal_page(
                    &conn, id, &question_owned, &revised, &model_for_db,
                ) {
                    crate::logging::log(&format!("JOURNAL: vim-rewrite save failed: {}", e));
                }
                purge_journal_audio(&conn, id);
            }
            // Background auto-tag the rewritten entry (rw conn above is dropped).
            spawn_retag(st, id, question_owned.clone(), revised.clone());
            let mut s = st.borrow_mut();
            // Under an active term filter the displayed entry is a cross-work
            // filter match, not a band page — update the in-memory match's answer
            // and re-render the filtered view (render_current would show the
            // wrong origin band). Otherwise the normal band re-render + land.
            let in_filter = s
                .journal
                .filter
                .as_ref()
                .and_then(|f| f.matches.get(f.pos))
                .map(|m| m.page.id == id)
                .unwrap_or(false);
            if in_filter {
                if let Some(filter) = s.journal.filter.as_mut() {
                    if let Some(m) = filter.matches.get_mut(filter.pos) {
                        // Sync BOTH q and a: the R→question path passes an
                        // improved question, so the in-memory match must carry
                        // it too, else render_filtered_match shows the stale old
                        // question with the new answer (the DB is already
                        // correct; this is display-only).
                        m.page.question = question_owned.clone();
                        m.page.answer = revised.clone();
                    }
                }
                render_filtered_match(&mut s);
            } else if s.chat.rewrite_return {
                // Panel-initiated R: re-render the chat panel, not the hidden
                // overlay. finish_panel_rewrite reloads journal_list, keeps the
                // cursor on entry `id`, restores ChatTranscript, clears the flag.
                crate::input::actions::chat::finish_panel_rewrite(&mut s, Some(id));
            } else {
                render_current(&mut s);
                land_on_current_band_id(&mut s, id);
            }
            // The diff-highlight is painted by refresh_entry_diff_highlight inside
            // the render above: the just-appended revision is now the entry's
            // latest, so it computes the same changed-words diff (prev vs revised).
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_REWRITTEN, 2);
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 4);
            // Panel-initiated R that errored: don't strand the panel in a stale
            // flag / wrong mode. Re-render the (unchanged) journal list and
            // restore ChatTranscript.
            if s.chat.rewrite_return {
                crate::input::actions::chat::finish_panel_rewrite(&mut s, None);
            }
        },
    );
}

/// Fire-and-forget: extract this entry's terms via Claude (the shared
/// `journal.extract-terms` prompt, on `tag_extract_model`) and replace its
/// auto-generated `journal_tags`. No-op when `auto_tag_journal` is off. On a
/// call error nothing is written (existing tags survive); only a successful
/// reply runs the replace. Text is captured by value so overlapping re-edits
/// each tag their own snapshot (last commit wins — correct).
pub(crate) fn spawn_retag(
    state: &Rc<RefCell<AppState>>,
    entry_id: i64,
    question: String,
    answer: String,
) {
    let (enabled, model) = {
        let s = state.borrow();
        (s.config.auto_tag_journal, s.config.tag_extract_model.clone())
    };
    if !enabled {
        return;
    }
    // The active DB prompt wins when present; otherwise fall back to the
    // hardcoded prompt (litdb's batch tagger does the same — the row has never
    // been seeded in the real lit.db), so the feature works unseeded.
    let prompt = crate::db::prompts::active_prompt("journal.extract-terms")
        .unwrap_or_else(|| crate::journal_tags::FALLBACK_EXTRACT_PROMPT.to_string());
    let user_msg = format!("Q: {question}\nA: {answer}");
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        user_msg,
        model,
        // on_success: parse + write. Own its own rw connection; never touches AppState.
        move |_state, reply| {
            let terms = crate::journal_tags::parse_terms(&reply);
            match crate::db::queries::open_db_rw() {
                Ok(conn) => {
                    if let Err(e) = crate::db::journal::replace_auto_tags(&conn, entry_id, &terms) {
                        crate::logging::log(&format!("AUTO_TAG: write failed for {entry_id}: {e}"));
                    } else {
                        crate::logging::log(&format!(
                            "AUTO_TAG: entry {entry_id} tagged with {} term(s)",
                            terms.len()
                        ));
                    }
                }
                Err(e) => crate::logging::log(&format!("AUTO_TAG: open_db_rw failed: {e}")),
            }
        },
        // on_error: write NOTHING — leave existing tags intact.
        move |_state, msg| {
            crate::logging::log(&format!("AUTO_TAG: extract call failed ({msg}); tags unchanged"));
        },
    );
}

/// The windowed scene/passage text for the current journal band, anchored on the
/// reader's saved position — the same context `ask_claude` sends to the answer
/// prompt. Empty for Work/Author bands and unresolvable positions. Factored so
/// the answer path and the first-ask term extractor build it identically.
/// `pub(crate)` so the chat panel can fetch the SAME scene text the journal
/// Passage band sends (the two surfaces build one shared answer message — see
/// `build_qa_answer_message`).
pub(crate) fn current_scene_text(s: &AppState) -> String {
    let anchor_work_line = s
        .journal
        .return_pos
        .and_then(|(buf, _top, _off)| s.work_line_for_buffer(buf))
        .unwrap_or(0);
    match &s.journal_band {
        JournalBand::Work => String::new(),
        JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_text_windowed(
            s, *d1, *d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
        ),
        JournalBand::Passage { div1, div2, .. } => crate::app::scene_synopsis::scene_text_windowed(
            s, *div1, *div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
        ),
        JournalBand::Author(_) => String::new(),
    }
}

/// Build the passage-context ANSWER-request user message for a journal band.
///
/// This is the ONE builder shared by BOTH surfaces that ask Claude a
/// passage-context question — the journal Q&A overlay (`ask_claude`) and the
/// chat panel (`submit_chat_prompt`, which always calls it with the Passage
/// band). Extracting it keeps the two prompts in sync by construction: any
/// future change to "what the journal sends" automatically applies to the chat
/// panel, with no drift. It is pure (no `AppState`), so it is unit-tested
/// per-band as the regression guard that pins each band's exact wording.
///
/// Per-band field usage (matches the arms verbatim):
/// - `Work`: genre, title, author, question.
/// - `Scene`: genre, title, author, unit_label, scene_label, scene_text, question.
/// - `Passage`: as Scene plus `passage_source`.
/// - `Author`: the band's OWN author name + question (title/author args ignored).
// One flat arg per message field, mirroring the format!() calls it replaces —
// grouping them into a struct would only add indirection at both call sites.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_qa_answer_message(
    band: &JournalBand,
    genre: &str,
    title: &str,
    author: &str,
    unit_label: &str,
    scene_label: &str,
    scene_text: &str,
    passage_source: &str,
    question: &str,
) -> String {
    match band {
        JournalBand::Work => format!(
            "Work type: {}\nWork: \"{}\" by {}\n\nReader's question about the {} as a whole:\n{}",
            genre, title, author, genre, question,
        ),
        JournalBand::Scene(_, _) => format!(
            "Work type: {}\nWork: \"{}\" by {}\n{}: {}\n\n{} text:\n{}\n\nReader's question:\n{}",
            genre,
            title,
            author,
            unit_label,
            scene_label,
            unit_label,
            scene_text,
            question,
        ),
        JournalBand::Passage { .. } => format!(
            "Work type: {}\nWork: \"{}\" by {}\n{}: {}\n\n{} text:\n{}\n\nPassage:\n{}\n\nReader's question:\n{}",
            genre,
            title,
            author,
            unit_label,
            scene_label,
            unit_label,
            scene_text,
            passage_source,
            question,
        ),
        // Author band: corpus-scope question, not tied to a specific work.
        JournalBand::Author(author_name) => format!(
            "Author: {}\n\nReader's question about the author's corpus:\n{}",
            author_name, question,
        ),
    }
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
        // opened from), mapped to a work line — factored so the first-ask term
        // extractor builds identical context. Empty for Work/Author bands.
        let scene_text = current_scene_text(&s);
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

    {
        let s = state_rc.borrow();
        let head = crate::app::scene_synopsis::cursor_head(&s);
        s.journal_overlay.set_running_head(&head.0, &head.1);
        s.journal_overlay.show_loading(question, "Answering\u{2026}");
    }

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
    // scene_label is band-relevant only for Scene/Passage; the builder ignores
    // it for Work/Author, so computing it unconditionally is harmless.
    let scene_label = match band {
        JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_label(d1, d2),
        JournalBand::Passage { div1, div2, .. } => {
            crate::app::scene_synopsis::scene_label(div1, div2)
        }
        _ => String::new(),
    };
    let user_msg = build_qa_answer_message(
        &band,
        genre,
        &work_title,
        &work_author,
        &unit_label,
        &scene_label,
        &scene_text,
        &passage_source_text,
        question,
    );
    let question_owned = question.to_string();
    let model_for_db = model.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        crate::gloss::journal_qa_prompt(&work_type),
        user_msg,
        model,
        move |st, answer| {
            let mut saved_id: Option<i64> = None;
            if let Ok(conn) = crate::db::queries::open_db_rw() {
                let write_result = match &band {
                    JournalBand::Work => crate::db::journal::save_journal_page(
                        &conn, &work_abbrev,
                        crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1,
                        &question_owned, &answer, &model_for_db, "work", "qa",
                    ),
                    JournalBand::Scene(d1, d2) => crate::db::journal::save_journal_page(
                        &conn, &work_abbrev, *d1, *d2,
                        &question_owned, &answer, &model_for_db, "scene", "qa",
                    ),
                    JournalBand::Passage { div1, div2, start, end } => {
                        crate::db::journal::save_passage_page(
                            &conn, &work_abbrev, *div1, *div2, start, end,
                            &passage_source_text, &question_owned, &answer, &model_for_db,
                        )
                    }
                    JournalBand::Author(_) => crate::db::journal::save_author_page(
                        &conn, &work_author,
                        &question_owned, &answer, &model_for_db, "qa",
                    ),
                };
                match write_result {
                    Ok(id) => saved_id = Some(id),
                    Err(e) => crate::logging::log(&format!("JOURNAL: db write failed: {}", e)),
                }
            }
            // Background auto-tag the new entry (borrows `st` briefly; the rw
            // connection above is already dropped).
            if let Some(id) = saved_id {
                spawn_retag(st, id, question_owned.clone(), answer.clone());
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
                    JournalBand::Author(author_name) => {
                        crate::db::journal::find_author_pages(&conn, author_name).ok()
                    }
                })
                .unwrap_or_default();
            let new_index = pages.len().saturating_sub(1);
            let mut s = st.borrow_mut();
            s.journal_band = band.clone();
            s.journal.page_index = new_index;
            // The rendered answer lives in the journal overlay, so make the
            // overlay the active input consumer. Without this the overlay is
            // visible but keys (j/k block-nav, comma/q, Escape) fall through to
            // whatever mode the ask flow left us in — Reader for the gloss-side
            // Ctrl+a passage-ask, which routes close_gloss_to_reader before
            // handing off here. (2026-07-23: fixes "overlay keys dead after
            // asking".)
            s.input_mode = crate::app::InputMode::JournalOverlay;
            render_current(&mut s);
            crate::logging::log("JOURNAL: saved page (mode=JournalOverlay)");
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
    let work_abbrev = current_work_abbrev(s);
    let pages = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_all_pages_ordered(&conn, &work_abbrev).ok())
        .unwrap_or_default();

    if pages.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No journal pages yet — press Ctrl+a to ask", 3);
        return false;
    }

    let rows: Vec<crate::ui::journal_picker::JournalRow> = pages
        .iter()
        .map(|p| {
            let band = match band_for_page(p) {
                JournalBand::Author(_) => JournalBand::Author(
                    s.current_work.as_ref().map(|w| w.author.clone()).unwrap_or_default(),
                ),
                other => other,
            };
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
                JournalBand::Author(author_name) => format!("{} corpus", author_name),
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

/// Open the Q&A picker over the journal overlay (Alt+p). Lists every page in the
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
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));
    s.journal.entry_page_id = None; // this open is itself navigation: close may source-jump
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

/// Open the cross-work "recent Q&A" jump-back picker from the reading card
/// (Ctrl+a): the last 15 journal entries across all works, newest-first. Empty
/// list shows an empty-state row (never a crash). Confirm loads the entry's
/// edition and opens the journal overlay on it; Escape returns to the reader.
pub(crate) fn open_recent_qa_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    // Save the reader position so Escape (and the post-open overlay's Escape)
    // returns there.
    s.journal.return_pos = Some((s.current_line, s.page_top_line, s.page_top_offset));

    let matches = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_recent_pages(&conn, 15).ok())
        .unwrap_or_default();

    let rows: Vec<crate::ui::recent_qa_picker::RecentQaRow> = matches
        .iter()
        .map(|m| {
            let p = &m.page;
            // A passage Q&A shows the FIRST LINE of its passage (like the
            // work-scoped picker); otherwise the question. Fall back to the
            // question when the source markup has no verse/stage line.
            let is_passage = p.start_citation.is_some() && p.end_citation.is_some();
            let label_text = if is_passage {
                p.source_text
                    .as_deref()
                    .and_then(first_passage_line)
                    .unwrap_or_else(|| p.question.clone())
            } else {
                p.question.clone()
            };
            let question_prefix: String = label_text.chars().take(80).collect();
            crate::ui::recent_qa_picker::RecentQaRow {
                id: p.id,
                work_abbrev: m.work_abbrev.clone(),
                work_label: m.work_abbrev.clone(),
                question_prefix,
            }
        })
        .collect();

    s.recent_qa_picker.set_items(rows);
    s.recent_qa_picker.show();
    s.input_mode = InputMode::RecentQaPicker;
}

/// Confirm the recent-Q&A picker selection: load the entry's edition (if it is
/// not the current work) and open the journal overlay on that exact entry,
/// reusing the shared cross-work open sequence (`load_arkangel_edition_then` ->
/// `open_journal_hit` -> `find_page_by_id` -> `render_filtered_match`). A missing
/// entry (deleted between list and confirm) toasts inside `open_journal_hit` and
/// stays in the reader.
pub(crate) fn confirm_recent_qa_picker(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected = state.borrow().recent_qa_picker.selected_index();

    // Capture the pick + current edition, hide the picker, drop to the reader
    // mode so the load path (which reveals the journal overlay via
    // open_journal_hit) starts from a clean state.
    let picked = {
        let mut s = state.borrow_mut();
        s.recent_qa_picker.hide();
        let picked = selected.and_then(|idx| {
            s.recent_qa_picker
                .items
                .get(idx)
                .map(|row| (row.id, row.work_abbrev.clone()))
        });
        if picked.is_none() {
            // Nothing selected (e.g. the empty-state row): return to the reader.
            s.journal.return_pos = None;
            crate::app::return_to_reader_mode(&mut s);
        } else {
            s.input_mode = InputMode::Reader;
        }
        picked
    };

    let Some((entry_id, base_abbrev)) = picked else {
        return;
    };

    let current_abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());

    // Load the entry's Arkangel edition (base if none), then open the journal
    // overlay on the entry with no seeded search pattern. The shared loader
    // handles the same-work skip, MPV-media discovery, and the error toast.
    crate::input::actions::pickers::load_arkangel_edition_then(
        state,
        tokio_handle,
        base_abbrev,
        current_abbrev,
        move |state| crate::input::actions::corpus_search::open_journal_hit(state, entry_id, ""),
    );
}

/// Open the "move this Q&A to another band" picker over the journal overlay.
/// Lists every band the current entry could move to (whole work + every
/// scene/chapter), excluding its current band. No-op with a toast if there is no
/// current page, or if the current band is a passage (passages are
/// citation-anchored and not movable).
pub(crate) fn open_move_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.pages.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No page to move", 2);
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
        crate::input::navigation::show_chapter_toast_secs(&s, "Can't move a passage page", 2);
        return;
    }
    let rows = move_target_rows(&s, &s.journal_band.clone());
    if rows.is_empty() {
        crate::input::navigation::show_chapter_toast_secs(&s, "No other band to move to", 2);
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
        JournalBand::Author(_) => ("author", crate::app::JOURNAL_AUTHOR_DIV.0, crate::app::JOURNAL_AUTHOR_DIV.1),
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
    crate::input::navigation::show_chapter_toast_secs(&s, &format!("Moved to {}", label), 2);
    crate::logging::log("JOURNAL: moved page to new band");
}

/// Purge an entry's cached TTS MP3s (rows + files), since SQLite FK cascade is
/// not enabled app-wide. Called when an entry is DELETED and also when its answer
/// is EDITED/REWRITTEN — the cached per-paragraph audio no longer matches the new
/// text, so it must be dropped or Space would replay the stale take.
pub(crate) fn purge_journal_audio(conn: &rusqlite::Connection, id: i64) {
    if let Ok(paths) = crate::db::queries::delete_journal_audio(conn, id) {
        for p in paths {
            let _ = std::fs::remove_file(&p);
        }
    }
}

pub(crate) fn delete_current(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    // Delete the DISPLAYED entry (filter match under `f`, else band page).
    let Some(id) = displayed_journal_page(&s).map(|p| p.id) else {
        return;
    };
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::delete_journal_page(&conn, id);
        purge_journal_audio(&conn, id);
    }
    // Under a filter, drop the deleted entry from the match list and re-render
    // the filtered view (next match, clamped); if it was the last match, clear
    // the filter and fall back to the band.
    if s.journal.filter.is_some() {
        let empty = {
            let filter = s.journal.filter.as_mut().unwrap();
            if filter.pos < filter.matches.len() {
                filter.matches.remove(filter.pos);
            }
            if filter.pos >= filter.matches.len() {
                filter.pos = filter.matches.len().saturating_sub(1);
            }
            filter.matches.is_empty()
        };
        if empty {
            s.journal.filter = None;
            render_current(&mut s);
        } else {
            render_filtered_match(&mut s);
        }
        return;
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
    // Displayed entry (filter match under `f`, else band page) — copy the id of
    // what's on screen, not the stale origin band.
    let Some(id) = displayed_journal_page(&s).map(|p| p.id) else {
        return;
    };
    // Copy the id prefaced with a label so a paste self-identifies.
    let copied = format!("Journal Q&A ID: {}", id);
    crate::ui::copy_to_clipboard(&copied);
    crate::input::navigation::show_chapter_toast_secs(&s, &format!("Copied {}", copied), 2);
    crate::logging::log(&format!("JOURNAL: copied \"{}\"", copied));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `\` cycle must never fall through to the chapter band; Ctrl+j must
    /// keep doing so. Guards the scope enum against being collapsed back into
    /// a bool or silently defaulted.
    #[test]
    fn only_segment_else_band_reaches_the_scene_band() {
        assert!(!JournalOpenScope::SegmentOnly.allows_band_fallback());
        assert!(JournalOpenScope::SegmentElseBand.allows_band_fallback());
    }

    #[test]
    fn blank_question_is_skipped() {
        assert!(is_blank_question("   \n\t "));
        assert!(is_blank_question(""));
        assert!(!is_blank_question("why the tub?"));
    }

    #[test]
    fn source_citation_range() {
        assert_eq!(
            format_source_citation("Cymbeline", Some("Cym.1.1.1"), Some("Cym.1.1.3")),
            Some("\u{2014} Cymbeline, 1.1.1\u{2013}3".to_string())
        );
    }

    #[test]
    fn source_citation_single_locator_collapses() {
        assert_eq!(
            format_source_citation("Cymbeline", Some("Cym.1.1.5"), Some("Cym.1.1.5")),
            Some("\u{2014} Cymbeline, 1.1.5".to_string())
        );
    }

    #[test]
    fn source_citation_missing_start_is_none() {
        assert_eq!(format_source_citation("Cymbeline", None, Some("Cym.1.1.3")), None);
        assert_eq!(
            format_source_citation("Cymbeline", Some("garbage"), Some("Cym.1.1.3")),
            None
        );
    }

    #[test]
    fn source_citation_missing_end_uses_start_only() {
        assert_eq!(
            format_source_citation("Cymbeline", Some("Cym.2.3.10"), None),
            Some("\u{2014} Cymbeline, 2.3.10".to_string())
        );
    }

    #[test]
    fn source_paragraphs_speaker_verse_citation() {
        let src = "<speaker>FIRST GENTLEMAN</speaker>\n\
                   <segment>You do not meet a man but frowns. Our bloods</segment>\n\
                   <segment>No more obey the heavens than our courtiers\u{2019}</segment>\n\
                   <segment>Still seem as does the King\u{2019}s.</segment>";
        let got = source_paragraphs(src, Some("\u{2014} Cymbeline, 1.1.1\u{2013}3"), false);
        assert_eq!(
            got.paras,
            vec![
                "FIRST GENTLEMAN".to_string(),
                "You do not meet a man but frowns. Our bloods\n\
                 No more obey the heavens than our courtiers\u{2019}\n\
                 Still seem as does the King\u{2019}s."
                    .to_string(),
                "\u{2014} Cymbeline, 1.1.1\u{2013}3".to_string(),
            ]
        );
        assert!(got.has_speaker);
        assert!(got.has_citation);
    }

    #[test]
    fn source_paragraphs_no_citation_omits_citation_para() {
        let src = "<speaker>KING</speaker>\n<segment>Now is the winter</segment>";
        let got = source_paragraphs(src, None, false);
        assert_eq!(
            got.paras,
            vec!["KING".to_string(), "Now is the winter".to_string()]
        );
        assert!(got.has_speaker);
        assert!(!got.has_citation);
    }

    #[test]
    fn source_paragraphs_speakerless_prose_drops_speaker() {
        let src = "<speaker>UNKNOWN</speaker>\n<segment>a prose line</segment>";
        // <segment> lines collapse to one block regardless of is_prose.
        let got = source_paragraphs(src, Some("\u{2014} Bleak House, 1.1.1"), true);
        assert_eq!(
            got.paras,
            vec![
                "a prose line".to_string(),
                "\u{2014} Bleak House, 1.1.1".to_string(),
            ]
        );
        // The regression this flag exists for: a speakerless prose quote must
        // NOT be mistaken for a speaker line (the old em-dash sniff shrank the
        // whole quote to the speaker tag's reduced scale).
        assert!(!got.has_speaker);
        assert!(got.has_citation);
    }

    #[test]
    fn source_paragraphs_prose_multiparagraph_splits_segments() {
        // The display source for a prose passage is rebuilt with per-line
        // `<segment>` tags (build_source_header) — three paragraphs, three tags.
        // On a prose work each becomes its OWN paragraph so the overlay renders a
        // blank-line gap between them — the A Tale of a Tub 0.0.5–7 Q&A the bug
        // was reported against.
        let src = "<segment>This infallibly convinced me that your Lordship was the person.</segment>\n\
                   <segment>In two days they brought me ten sheets of paper.</segment>\n\
                   <segment>If by altering the title I could make the same materials serve.</segment>";
        let got = source_paragraphs(src, Some("\u{2014} A Tale of a Tub, 0.0.5\u{2013}7"), true);
        assert_eq!(
            got.paras,
            vec![
                "This infallibly convinced me that your Lordship was the person.".to_string(),
                "In two days they brought me ten sheets of paper.".to_string(),
                "If by altering the title I could make the same materials serve.".to_string(),
                "\u{2014} A Tale of a Tub, 0.0.5\u{2013}7".to_string(),
            ]
        );
        assert!(!got.has_speaker);
        assert!(got.has_citation);
    }

    #[test]
    fn source_paragraphs_prose_plain_untagged_splits() {
        // Untagged plain prose (e.g. a vocab-sentence cursor segment stored raw):
        // still splits per line on a prose work.
        let src = "First paragraph of prose.\nSecond paragraph of prose.";
        let got = source_paragraphs(src, Some("\u{2014} Bleak House, 1.1.1"), true);
        assert_eq!(
            got.paras,
            vec![
                "First paragraph of prose.".to_string(),
                "Second paragraph of prose.".to_string(),
                "\u{2014} Bleak House, 1.1.1".to_string(),
            ]
        );
    }

    #[test]
    fn source_paragraphs_prose_single_line() {
        let src = "<segment>A single paragraph of prose.</segment>";
        let got = source_paragraphs(src, Some("\u{2014} Bleak House, 1.1.1"), true);
        assert_eq!(
            got.paras,
            vec![
                "A single paragraph of prose.".to_string(),
                "\u{2014} Bleak House, 1.1.1".to_string(),
            ]
        );
    }

    #[test]
    fn source_paragraphs_verse_segments_stay_one_block() {
        // The SAME per-line `<segment>` shape on a VERSE/play work: the lines are
        // verse lines of one speech, so they stay a single line-height block
        // (is_prose=false), never gapped — the existing Cymbeline behavior.
        let src = "<segment>You do not meet a man but frowns. Our bloods</segment>\n\
                   <segment>No more obey the heavens than our courtiers\u{2019}</segment>\n\
                   <segment>Still seem as does the King\u{2019}s.</segment>";
        let got = source_paragraphs(src, Some("\u{2014} Cymbeline, 1.1.1\u{2013}3"), false);
        assert_eq!(
            got.paras,
            vec![
                "You do not meet a man but frowns. Our bloods\n\
                 No more obey the heavens than our courtiers\u{2019}\n\
                 Still seem as does the King\u{2019}s."
                    .to_string(),
                "\u{2014} Cymbeline, 1.1.1\u{2013}3".to_string(),
            ]
        );
        assert!(!got.has_speaker);
        assert!(got.has_citation);
    }

    #[test]
    fn source_paragraphs_prose_speaker_flushes_before_label() {
        // Defensive: on a prose work with a speaker (rare), the prior speech
        // flushes to its own paragraphs BEFORE the speaker label, so lines never
        // merge across the label.
        let src = "<speaker>ESTHER</speaker>\n\
                   <segment>Line one of prose.</segment>\n\
                   <segment>Line two of prose.</segment>";
        let got = source_paragraphs(src, None, true);
        assert_eq!(
            got.paras,
            vec![
                "ESTHER".to_string(),
                "Line one of prose.".to_string(),
                "Line two of prose.".to_string(),
            ]
        );
        assert!(got.has_speaker);
    }

    #[test]
    fn term_input_from_reader_only_in_reader_mode() {
        use crate::app::InputMode;
        assert!(term_input_opened_from_reader(InputMode::Reader));
        assert!(!term_input_opened_from_reader(InputMode::JournalOverlay));
    }

    #[test]
    fn improve_terms_line_guidance_or_empty() {
        // empty -> empty string (prompt reads clean)
        assert_eq!(super::improve_terms_line(&[]), "");
        // terms -> guidance naming them, preserve-verbatim instruction present
        let line = super::improve_terms_line(&["fee simple".to_string(), "quibble".to_string()]);
        assert!(line.contains("fee simple, quibble"), "names terms: {line}");
        assert!(line.to_lowercase().contains("preserve"), "guidance present: {line}");
    }

    #[test]
    fn answer_prefix_chars_matches_rendered_body() {
        // Body is `format!("{}\n\n{}", prefix_question(question), answer)` where
        // prefix_question(q) = "Q: {q}" (journal_overlay.rs). The prefix length
        // must exactly match where the answer begins in that rendered string.
        let question = "What does Prospero mean here?";
        let answer = "He means the isle is full of noises.";
        let body = format!("Q: {}\n\n{}", question, answer);
        let base = super::answer_prefix_chars(question) as usize;
        assert_eq!(&body[..base], format!("Q: {}\n\n", question));
        assert_eq!(&body[base..], answer);
    }

    #[test]
    fn title_case_first_letter() {
        assert_eq!(super::titlecase_first("chapter"), "Chapter");
        assert_eq!(super::titlecase_first("scene"), "Scene");
        assert_eq!(super::titlecase_first(""), "");
    }

    // build_qa_answer_message is the ONE builder shared by the journal Q&A
    // overlay and the chat panel. These tests pin each band's EXACT wording:
    // if someone edits one band's format string, the matching test fails —
    // that is the regression guard that keeps the two surfaces in sync.

    #[test]
    fn qa_message_work_band() {
        let got = super::build_qa_answer_message(
            &JournalBand::Work,
            "play", "Cymbeline", "William Shakespeare",
            "IGNORED_UNIT", "IGNORED_SCENE", "IGNORED_SCENE_TEXT", "IGNORED_PASSAGE",
            "What is the play about?",
        );
        assert_eq!(
            got,
            "Work type: play\nWork: \"Cymbeline\" by William Shakespeare\n\n\
             Reader's question about the play as a whole:\n\
             What is the play about?",
        );
    }

    #[test]
    fn qa_message_scene_band() {
        let got = super::build_qa_answer_message(
            &JournalBand::Scene(3, 4),
            "play", "Cymbeline", "William Shakespeare",
            "Scene", "Act 3, Scene 4", "SCENE TEXT HERE", "IGNORED_PASSAGE",
            "Why does she weep?",
        );
        assert_eq!(
            got,
            "Work type: play\nWork: \"Cymbeline\" by William Shakespeare\n\
             Scene: Act 3, Scene 4\n\n\
             Scene text:\nSCENE TEXT HERE\n\n\
             Reader's question:\nWhy does she weep?",
        );
    }

    #[test]
    fn qa_message_passage_band() {
        let band = JournalBand::Passage {
            div1: 3,
            div2: 4,
            start: "3.4.1".to_string(),
            end: "3.4.9".to_string(),
        };
        let got = super::build_qa_answer_message(
            &band,
            "play", "Cymbeline", "William Shakespeare",
            "Scene", "Act 3, Scene 4",
            "FULL SCENE TEXT", "<speaker>IMOGEN</speaker>\n<segment>the passage</segment>",
            "What does she mean?",
        );
        assert_eq!(
            got,
            "Work type: play\nWork: \"Cymbeline\" by William Shakespeare\n\
             Scene: Act 3, Scene 4\n\n\
             Scene text:\nFULL SCENE TEXT\n\n\
             Passage:\n<speaker>IMOGEN</speaker>\n<segment>the passage</segment>\n\n\
             Reader's question:\nWhat does she mean?",
        );
    }

    #[test]
    fn qa_message_author_band() {
        let got = super::build_qa_answer_message(
            &JournalBand::Author("William Shakespeare".to_string()),
            "IGNORED_GENRE", "IGNORED_TITLE", "IGNORED_AUTHOR",
            "IGNORED_UNIT", "IGNORED_SCENE", "IGNORED_SCENE_TEXT", "IGNORED_PASSAGE",
            "What are his recurring themes?",
        );
        assert_eq!(
            got,
            "Author: William Shakespeare\n\n\
             Reader's question about the author's corpus:\n\
             What are his recurring themes?",
        );
    }

    #[test]
    fn qa_message_passage_band_is_what_both_surfaces_send() {
        // The chat panel now builds the SAME Passage-band message as the journal
        // overlay for a given (scene_text, passage, question). This test fixes
        // that shared output: both surfaces call build_qa_answer_message with the
        // Passage band, so this string is what BOTH now send on the wire.
        let scene_text = "First line of the scene.\nSecond line of the scene.";
        let passage = "<speaker>POSTHUMUS</speaker>\n<segment>Is there no way for men to be, but women</segment>";
        let question = "Is he being fair to women here?";
        let band = JournalBand::Passage {
            div1: 2,
            div2: 5,
            start: "2.5.1".to_string(),
            end: "2.5.2".to_string(),
        };
        let got = super::build_qa_answer_message(
            &band,
            "play", "Cymbeline", "William Shakespeare",
            "Scene", "Act 2, Scene 5",
            scene_text, passage, question,
        );
        let expected = format!(
            "Work type: play\nWork: \"Cymbeline\" by William Shakespeare\n\
             Scene: Act 2, Scene 5\n\n\
             Scene text:\n{}\n\n\
             Passage:\n{}\n\n\
             Reader's question:\n{}",
            scene_text, passage, question,
        );
        assert_eq!(got, expected);
    }

    #[test]
    fn first_passage_line_reads_first_verse() {
        let markup = "<speaker>FIRST GENTLEMAN</speaker>\n\
                      <segment>You do not meet a man but frowns. Our bloods</segment>\n\
                      <segment>No more obey the heavens than our courtiers'</segment>\n";
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
                      <segment>You do not meet a man but frowns.</segment>\n";
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

    /// Guard test (not fail-first — see task-2-report.md): locks the
    /// clamp-no-wrap semantics of the existing `flat_step` that the term-browse
    /// filter's `nav_page` branch relies on to walk a match subset. Already
    /// covered by `flat_step_clamps_and_steps`; this documents the specific
    /// 3-match subset shape the filter uses.
    #[test]
    fn filter_walk_uses_flat_step_over_match_list() {
        // A 3-match subset: stepping forward from 0 -> 1 -> 2 -> clamps (None at end).
        assert_eq!(flat_step(0, 1, 3), Some(1));
        assert_eq!(flat_step(1, 1, 3), Some(2));
        assert_eq!(flat_step(2, 1, 3), None); // at last match, no wrap
        assert_eq!(flat_step(0, -1, 3), None); // at first match, no wrap
        // empty subset never steps
        assert_eq!(flat_step(0, 1, 0), None);
    }

    #[test]
    fn rewrite_user_message_includes_context_and_all_three_parts() {
        let msg = rewrite_user_message(
            "Work: \"Bleak House\" by Charles Dickens\nThis Q&A is filed under the WHOLE WORK (not a single scene).",
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
        // The focus directive that suppresses cross-work mismatch preambles is
        // present, between the current answer and the instruction.
        let r_pos = msg.find("FOCUS:").expect("focus directive present");
        assert!(r_pos > a_pos && r_pos < i_pos, "focus directive sits after the answer, before the instruction");
    }

    #[test]
    fn rewrite_user_message_with_empty_instruction_is_still_well_formed() {
        // The "rewrite afresh under the default prompt" path (journal `R` and,
        // now, the chat panel's empty-Ctrl+Enter): an empty instruction must
        // still yield a coherent request — context + question + current answer
        // + the revise directive, just with no custom steering appended.
        let msg = rewrite_user_message(
            "Work: \"Bleak House\" by Charles Dickens",
            "Who is Esther?",
            "She narrates half the book.",
            "",
        );
        assert!(msg.contains("Bleak House"), "context present");
        assert!(msg.contains("Who is Esther?"), "question present");
        assert!(msg.contains("She narrates half the book."), "current answer present");
        assert!(msg.contains("return only the revised answer"), "revise directive present");
        // The message is non-trivial even with no instruction (not just the
        // instruction line collapsed to nothing).
        assert!(msg.len() > 100, "empty-instruction message is still substantive");
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
            kind: "qa".into(),
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
    fn band_for_page_classifies_author() {
        // An author page arrives with div1=div2=-2. band_for_page must classify it
        // as Author using the page's work_abbrev — but band_for_page only sees a
        // JournalPage (no work_abbrev field), so author classification keys on the
        // -2 sentinel and the Author name is supplied by the caller (render_current).
        // Here we assert the sentinel routes to the Work-vs-Author branch correctly.
        assert_eq!(band_for_page(&page(-2, -2, None, None)), JournalBand::Author(String::new()));
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

    #[test]
    fn improved_question_parse_strips_fence_and_falls_back() {
        // plain
        assert_eq!(
            parse_improved_question("What does 'fee simple' mean here?", "orig"),
            "What does 'fee simple' mean here?"
        );
        // fenced (model wrapped it)
        assert_eq!(
            parse_improved_question("```\nWhat is a fee simple?\n```", "orig"),
            "What is a fee simple?"
        );
        // empty / whitespace -> keep the original (never lose the question)
        assert_eq!(parse_improved_question("", "the original q"), "the original q");
        assert_eq!(parse_improved_question("   \n  ", "the original q"), "the original q");
    }

    fn source_test_line(id: i64, div1: i64, div2: i64, line_in_div: i64, is_dialogue: bool, text: &str) -> crate::db::models::Line {
        crate::db::models::Line {
            id,
            citation: format!("T.{div1}.{div2}.{line_in_div}"),
            text: text.to_string(),
            normalized: text.to_lowercase(),
            speaker: if is_dialogue { Some("FIRST".to_string()) } else { None },
            is_dialogue,
            timestamp: None,
            div1,
            div2,
            line_in_div,
            sub_line: 0,
            is_chapter: false,
            is_spoken: None,
            block_type: "prose".to_string(),
        }
    }

    fn source_test_work(lines: Vec<crate::db::models::Line>) -> crate::db::models::Work {
        crate::db::models::Work {
            abbrev: "Cym".into(),
            canonical_abbrev: "Cym".into(),
            title: "Test".into(),
            author: "Nobody".into(),
            work_type: "play".into(),
            text_file: None,
            vocab_highlight: false,
            lines,
            timestamps: vec![],
            media_paths: vec![],
            media_ids: vec![],
            media_id: None,
        }
    }

    #[test]
    fn source_first_buffer_line_resolves_citation() {
        // Scene heading (non-dialogue) at (5,5,1), then the first dialogue line
        // at (5,5,2). A citation matching the heading's tuple should advance to
        // the first dialogue line at-or-after it, then map through line_map.
        let work = source_test_work(vec![
            source_test_line(1, 5, 5, 1, false, "SCENE V."),
            source_test_line(2, 5, 5, 2, true, "Enter Posthumus."),
            source_test_line(3, 5, 5, 3, true, "Another line."),
        ]);

        // line_map: work index -> buffer index 1:1, offset by 10 to prove the
        // mapping (not the raw work index) is what gets returned.
        let line_map = crate::text_file_map::LineMap {
            buffer_to_work: vec![],
            work_to_buffer: vec![10, 11, 12],
            dialogue_buffer_lines: vec![],
            sentence_groups: vec![],
            chapter_breaks: vec![],
            section_starts: vec![],
        };

        // Citation resolves to the heading (work idx 0), advances to the first
        // dialogue line (work idx 1), maps to buffer idx 11.
        assert_eq!(
            source_first_buffer_line(&work, Some(&line_map), "Cym.5.5.1", ""),
            Some(11)
        );

        // Citation matching the dialogue line directly (work idx 1) maps to 11.
        assert_eq!(
            source_first_buffer_line(&work, Some(&line_map), "Cym.5.5.2", ""),
            Some(11)
        );

        // No line_map: returns the raw work index instead of a buffer index.
        assert_eq!(
            source_first_buffer_line(&work, None, "Cym.5.5.1", ""),
            Some(1)
        );

        // Unresolvable citation and empty source_text fallback -> None.
        assert_eq!(
            source_first_buffer_line(&work, Some(&line_map), "Cym.9.9.9", ""),
            None
        );
    }

    /// The `\` lap anchors on the position the lap STARTED from. Opening the
    /// gloss stop moves the cursor to the end of the glossed passage, so the
    /// journal stop must not probe the live cursor. Regression for the
    /// probe/open mismatch fixed 2026-07-27.
    #[test]
    fn lap_anchor_prefers_gloss_return_pos() {
        assert_eq!(lap_anchor_line(Some((424, 400, 0)), None, 437), 424);
    }

    #[test]
    fn lap_anchor_falls_back_to_journal_return_pos() {
        assert_eq!(lap_anchor_line(None, Some((910, 900, 0)), 979), 910);
    }

    #[test]
    fn lap_anchor_prefers_gloss_over_journal() {
        assert_eq!(lap_anchor_line(Some((424, 400, 0)), Some((910, 900, 0)), 979), 424);
    }

    #[test]
    fn lap_anchor_uses_cursor_when_no_overlay_open() {
        assert_eq!(lap_anchor_line(None, None, 979), 979);
    }

    /// The synopsis→journal hop records its origin band; the return hop TAKES
    /// it, so a later journal session opened any other way cannot inherit a
    /// stale marker and hijack `\`.
    #[test]
    fn synopsis_origin_is_taken_not_copied() {
        let mut marker = Some((10, 0));
        assert_eq!(take_synopsis_origin(&mut marker), Some((10, 0)));
        assert_eq!(marker, None, "the marker must be consumed");
        assert_eq!(take_synopsis_origin(&mut marker), None);
    }
}

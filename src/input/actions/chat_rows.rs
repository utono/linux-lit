//! Pure row-model core of the chat panel (audit #93): exchange → transcript-row
//! building, row/index arithmetic, journal-view rows, history-turn building,
//! consolidation, and revision parsing. No `AppState`, no GTK widgets —
//! everything here takes plain slices/primitives, which is what keeps it
//! unit-testable without constructing an `AppState`. The `&AppState` wrappers
//! (`transcript_rows`, the render fns, cursor handlers) stay in `chat.rs`.
//!
//! SYNC invariant (moved verbatim with the fns): the row set built here is the
//! single source of truth that BOTH the render path (`ui::chat_panel`) and the
//! pagination path (`ui::chat_pagination`) consume; cursor/row bookkeeping
//! breaks if any render path builds rows another way.

use super::chat::{Exchange, PanelView};

/// Wrap an exchange's answer as the right `TranscriptRow` variant: a
/// reader-gloss exchange (`push_gloss_exchange` always stores an empty
/// `question` — "the user asked nothing", see its doc comment) carries RAW
/// `<speaker>`/`<verse>`/`<gloss>` markup and must render through
/// `GlossAnswer` (typed rows, styled) rather than `Answer` (one plain label,
/// which would show the literal tags).
/// `reflow` (= `ChatState.source_is_prose`) re-flows `<verse>` bodies to one
/// line so prose source passages wrap at the panel width — see
/// `gloss_render::reflow_verse_markup`. It must be applied HERE, before
/// `widget_row_count` counts the row, so render, pagination, yank, and the
/// row cursor all see the same widget count.
pub(crate) fn answer_row(e: &Exchange, reflow: bool) -> crate::ui::chat_panel::TranscriptRow {
    use crate::ui::chat_panel::TranscriptRow as R;
    if e.question.is_empty() {
        R::GlossAnswer(gloss_answer_markup(&e.answer, reflow))
    } else {
        R::Answer(e.answer.clone())
    }
}

/// The markup a gloss-answer row renders from: verbatim for verse works,
/// `<verse>`-reflowed for prose. The single seam every `GlossAnswer`
/// constructor routes through.
pub(crate) fn gloss_answer_markup(answer: &str, reflow: bool) -> String {
    if reflow {
        crate::ui::gloss_render::reflow_verse_markup(answer)
    } else {
        answer.to_string()
    }
}

/// A `Question` row with the `Q: ` display label. Routes through
/// `journal_overlay::prefix_question` — the single source of that label — so
/// the prefix cannot drift across the render paths, and so a question already
/// beginning `Q:` is not double-prefixed (the raw `format!("Q: {}", …)` these
/// call sites used to inline had no such guard).
pub(crate) fn question_row(question: &str) -> crate::ui::chat_panel::TranscriptRow {
    crate::ui::chat_panel::TranscriptRow::Question(
        crate::ui::journal_overlay::prefix_question(question),
    )
}

/// How many actual WIDGETS (label rows in `transcript_box`) a single
/// `TranscriptRow` renders as. Every variant is one widget
/// (`chat_panel::rebuild_from_specs`'s `append_spec_label`) EXCEPT
/// `GlossAnswer`, which `chat_panel::row_widget_specs`/`gloss_answer_specs`
/// explodes into one label per `gloss_render::chat_gloss_rows` row
/// (speaker/verse/stage/gloss) — so the j/k row cursor, which must land on
/// those individual widgets, has to count in this same "widget space", not
/// `Vec<TranscriptRow>` space. Falls back to 1 for markup with no recognized
/// tags, matching `gloss_answer_specs`'s own plain-label fallback.
pub(crate) fn widget_row_count(row: &crate::ui::chat_panel::TranscriptRow) -> usize {
    use crate::ui::chat_panel::TranscriptRow as R;
    match row {
        R::GlossAnswer(markup) => {
            crate::ui::gloss_render::chat_gloss_rows(markup).len().max(1)
        }
        _ => 1,
    }
}

/// Whether `e` should render a `Q:` row at all. An auto-gloss exchange
/// (`push_gloss_exchange`) deliberately stores an empty `question` — "the
/// user asked nothing" — so a literal `▶ Q:` row with nothing after it reads
/// as a Q&A affordance on content that isn't Q&A (a reader-gloss). A
/// follow-up exchange asked with `a` always has a real question and keeps its
/// row.
pub(crate) fn has_question_row(e: &Exchange) -> bool {
    !e.question.is_empty()
}

/// Whether the exchange list's Q&A-bearing entries (per `has_question_row`)
/// number exactly one — i.e. whether the LAST push was the first real
/// follow-up question asked in this panel session (the auto-gloss exchange,
/// `exchanges[0]` with its empty question, never counts). Used right after
/// pushing a new Q&A exchange to decide whether to auto-save it: further
/// follow-ups (count > 1) still require `s`.
pub(crate) fn is_first_question_exchange(exchanges: &[Exchange]) -> bool {
    exchanges.iter().filter(|e| has_question_row(e)).count() == 1
}

/// Pure core of `transcript_rows` (no `AppState` — takes the exchange list
/// and exchange cursor directly), so the row-count/row_owner bookkeeping is
/// unit-testable without constructing an `AppState`.
pub(crate) fn build_transcript_rows(
    exchanges: &[Exchange],
    cursor: usize,
    reflow: bool,
) -> (Vec<crate::ui::chat_panel::TranscriptRow>, usize, Vec<usize>) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    let mut row_owner: Vec<usize> = Vec::new();
    let mut cursor_row = 0;
    let mut widget_row = 0usize; // running WIDGET-space row count
    let mut prev_chip: Option<&str> = None;
    for (i, e) in exchanges.iter().enumerate() {
        let is_cursor = i == cursor;
        let show_question = has_question_row(e);
        // An EMPTY chip renders no row: `gloss_chip` empties it for a lone
        // gloss, where the label added nothing the content didn't already say.
        let chip_is_new = !e.chip.is_empty() && prev_chip != Some(e.chip.as_str());
        if chip_is_new {
            rows.push(R::Chip(e.chip.clone()));
            row_owner.push(i);
            widget_row += 1;
        }
        prev_chip = Some(e.chip.as_str());
        // No `▸` glyph: j/k paints an accent bar on the cursor ROW, which says
        // the same thing in the same place the reading card says it. The
        // exchange cursor lands on the first row this exchange owns.
        if is_cursor {
            cursor_row = widget_row;
        }
        if show_question {
            rows.push(question_row(&e.question));
            row_owner.push(i);
            widget_row += 1;
        }
        let ans = answer_row(e, reflow);
        let ans_widgets = widget_row_count(&ans);
        rows.push(ans);
        for _ in 0..ans_widgets {
            row_owner.push(i);
        }
        widget_row += ans_widgets;
        if e.saved_id.is_some() {
            rows.push(R::SavedMark);
            row_owner.push(i);
            widget_row += 1;
        }
    }
    (rows, cursor_row, row_owner)
}

/// Pure row-builder for `PanelView::Question`: exactly ONE exchange's rows —
/// its `Q:` row (via `has_question_row`) plus one `Answer` row PER PARAGRAPH
/// of the answer (split via `split_answer_paragraphs`, same split the Journal
/// view uses) so the row cursor can traverse the answer and no single widget
/// outgrows a page. A reader-gloss exchange (empty question, raw markup)
/// renders as one `GlossAnswer` row (splitting it would break the markup
/// parse), with NO `Q:` row. No chip and no other exchange in either case.
/// This is `build_transcript_rows`'s per-exchange body run for a single `i == 0`
/// slice, factored out so the "one exchange -> its rows" shape is
/// unit-testable without constructing a whole transcript or an `AppState`.
/// The chip is deliberately omitted (matching `render_transcript_with_thinking`'s
/// existing "question names its own subject" reasoning) — a Question-view
/// render never shows the source-preview label the full Gloss transcript
/// does. A trailing `SavedMark` is appended if the exchange has been saved.
pub(crate) fn build_single_exchange_rows(
    e: &Exchange,
    reflow: bool,
) -> Vec<crate::ui::chat_panel::TranscriptRow> {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    if has_question_row(e) {
        rows.push(question_row(&e.question));
    }
    if e.question.is_empty() {
        // Reader-gloss exchange: raw <speaker>/<verse> markup renders as ONE
        // GlossAnswer row (see answer_row's doc comment) — splitting it would
        // break the markup parse.
        rows.push(R::GlossAnswer(gloss_answer_markup(&e.answer, reflow)));
    } else {
        // One Answer row per paragraph (same split journal_view_rows uses) so
        // the row cursor traverses the answer and no single widget outgrows a
        // page.
        for para in split_answer_paragraphs(&e.answer) {
            rows.push(R::Answer(para));
        }
    }
    if e.saved_id.is_some() {
        rows.push(R::SavedMark);
    }
    rows
}

/// Step an index by `delta`, wrapping at both ends. `len == 0` stays at 0
/// (guards the modulo).
pub(crate) fn wrap_index(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as i32;
    (((cur as i32 + delta) % n + n) % n) as usize
}

/// Pure flip, three-way: `Gloss -> Journal -> Gloss` (unchanged pair) and
/// `Question -> Gloss` — a shown Q&A is the non-gloss side of the toggle, so
/// `t` from it returns to the gloss, same direction `Journal` already goes.
/// There is deliberately no `Gloss -> Question`: `Question` is only ever
/// ENTERED by asking a follow-up (`submit_chat_prompt`), never by cycling
/// into it — `t` from Gloss always means "show the journal", matching the
/// existing bind before this view existed. Pulled out of `toggle_panel_view`
/// so the toggle direction is unit-testable without an `AppState`/
/// `Rc<RefCell<_>>`.
pub(crate) fn flip_view(v: PanelView) -> PanelView {
    match v {
        PanelView::Gloss => PanelView::Journal,
        PanelView::Journal => PanelView::Gloss,
        PanelView::Question => PanelView::Gloss,
    }
}

/// Split a saved answer into paragraph chunks (blank-line separated), each a
/// separate row so the panel cursor can traverse them. Never returns empty (an
/// empty answer yields one empty chunk so the entry still has an answer row).
pub(crate) fn split_answer_paragraphs(answer: &str) -> Vec<String> {
    let parts: Vec<String> = answer
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() { vec![String::new()] } else { parts }
}

/// Build the `Journal` view's transcript rows from a passage's saved journal
/// pages: each entry as a `Q:` row + one plain-prose `Answer` row per
/// paragraph of the answer (split on blank lines by `split_answer_paragraphs`,
/// never `GlossAnswer` — journal answers are prose, not `<speaker>`/`<verse>`
/// markup, even for entries saved from a gloss's follow-up question). Returns
/// the rows AND a parallel `row_owner` (widget index -> journal entry index),
/// mirroring the Gloss view's `build_transcript_rows`, so the accent bar can
/// traverse every emitted widget (Q and each answer paragraph) while `R`/save
/// still resolve the owning ENTRY. An empty list renders one `Answer`
/// placeholder row (with an empty `row_owner`, i.e. nothing landable) rather
/// than nothing, so a
/// passage with no journal history reads as "checked, none found" instead of
/// looking like a rendering bug. Pure (no `AppState`) so the row shape is
/// unit-testable without a DB or GTK.
pub(crate) fn journal_view_rows(
    pages: &[crate::db::journal::JournalPage],
) -> (Vec<crate::ui::chat_panel::TranscriptRow>, Vec<usize>) {
    use crate::ui::chat_panel::TranscriptRow as R;
    if pages.is_empty() {
        // Placeholder-only render: one non-landable Answer row, no owner.
        return (
            vec![R::Answer("No journal entries for this passage".to_string())],
            Vec::new(),
        );
    }
    let mut rows = Vec::with_capacity(pages.len() * 2);
    let mut row_owner: Vec<usize> = Vec::new();
    for (entry, p) in pages.iter().enumerate() {
        rows.push(question_row(&p.question));
        row_owner.push(entry);
        for para in split_answer_paragraphs(&p.answer) {
            rows.push(R::Answer(para));
            row_owner.push(entry);
        }
    }
    (rows, row_owner)
}

/// The first widget-row index owned by `journal_list` entry `entry`, i.e. its
/// `Q:` row, found in the parallel `row_owner` map `journal_view_rows` builds.
/// Each entry now emits a `Q:` row plus one `Answer` row per answer paragraph
/// (see `journal_view_rows`/`split_answer_paragraphs`), so entry width varies
/// and the old `entry*2` no longer holds — the accent bar anchors on this
/// row. Falls back to `0` when the entry isn't in `row_owner` (empty list /
/// stale cursor); the caller clamps `journal_cursor` first, so this is
/// defensive.
pub(crate) fn journal_entry_first_row(row_owner: &[usize], entry: usize) -> usize {
    row_owner.iter().position(|&e| e == entry).unwrap_or(0)
}

/// Clamp a Journal-view row cursor to a list of `len` entries: `[0, len-1]`, or
/// `0` for an empty list (which renders a single non-landable placeholder row).
pub(crate) fn clamp_journal_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        cursor.min(len - 1)
    }
}

/// Clean up in-memory references to a just-deleted journal row: clear any
/// exchange's `saved_id` that pointed at it (the exchange becomes re-savable
/// and its SavedMark disappears on the next render) and return the new
/// `revision_of` (cleared iff it pointed at the deleted row). Pure so the
/// dangling-reference contract is unit-testable without an `AppState`.
pub(crate) fn clear_deleted_journal_refs(
    exchanges: &mut [Exchange],
    revision_of: Option<i64>,
    deleted: i64,
) -> Option<i64> {
    for ex in exchanges.iter_mut() {
        if ex.saved_id == Some(deleted) {
            ex.saved_id = None;
        }
    }
    if revision_of == Some(deleted) { None } else { revision_of }
}

/// Widget-space "is this row a valid j/k landing spot" mask, one entry per
/// widget `chat_panel::rebuild_from_specs` would actually paint (same
/// granularity as `row_owner`/`widget_row_count`) — flat-mapped from
/// `chat_panel::row_widget_landable`, so it can never drift out of sync with
/// what the panel renders or with `row_widget_texts` (`y`'s copy source).
/// Only a `ChatGlossRowKind::Speaker` widget (e.g. "CYMBELINE") is `false`:
/// it isn't a line of dialogue, so it isn't a cursor stop — see Fix 2's brief
/// ("do not highlight speaker labels; only lines of dialogue").
pub(crate) fn landable_mask(rows: &[crate::ui::chat_panel::TranscriptRow]) -> Vec<bool> {
    rows.iter()
        .flat_map(crate::ui::chat_panel::row_widget_landable)
        .collect()
}

/// Step the row cursor by `delta`, skipping over unlandable (Speaker) widgets
/// entirely — `j`/`k` must never stop on one. Walks one step at a time in the
/// `delta` direction (delta may be ±1, the only magnitude j/k ever passes)
/// until it lands on a `true` entry or falls off the end. Returns `None`
/// when: `landable` is empty, NO entry is landable (nothing to move to —
/// e.g. a gloss that is somehow all speakers, Fix 2's no-landable-rows case),
/// or the walk falls off the boundary without finding a further landable row
/// (already at the first/last landable row — degrade to scrolling, the same
/// "no move -> None" contract `transcript_cursor_move` relies on).
///
/// The first LANDABLE widget row at or after `from` (widget-space), so a
/// caller that just computed an exchange's leading widget row
/// (`build_transcript_rows`' `cursor_row` — often a Speaker widget, since a
/// gloss block usually opens with one) can advance past it. Falls back to
/// `from` itself when nothing at or after it is landable (defensive; should
/// not happen for a real transcript, since a gloss with a Speaker row also
/// has at least one Verse/Gloss row) — never panics or returns an
/// out-of-range index.
pub(crate) fn first_landable_at_or_after(from: usize, landable: &[bool]) -> usize {
    (from..landable.len())
        .find(|&i| landable[i])
        .unwrap_or(from)
}

/// Pure range resolution: anchor/cursor in either order -> `(start, end)`
/// inclusive, widget-row space. Shared by `render_transcript` (highlight) and
/// `yank_transcript_selection` (copy) so both agree on exactly which rows are
/// "selected".
pub(crate) fn visual_selection_range(anchor: usize, cursor: usize) -> (usize, usize) {
    if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) }
}

/// `S` (or "consolidate") in the ask input: ask the model to merge the whole
/// transcript into ONE cohesive journal Q&A, save it as a passage journal
/// page, and pivot into the revision loop on the new entry (same landing as
/// `s` save, so Ctrl+Enter refines it further). The entry is filed under the
/// FIRST exchange's passage — the conversation's origin.
/// Consolidation reads the conversation transcript, which is otherwise
/// unbounded; keep the most recent exchanges — enough to cover a real
/// session, a cap only for marathon outliers. (The chat SEND window is
/// CHAT_HISTORY_TURNS = 6; consolidate gets double, since merging the
/// conversation is its whole point.)
const CONSOLIDATE_MAX_EXCHANGES: usize = 12;

/// The Q/A transcript for the consolidate prompt: the last
/// `CONSOLIDATE_MAX_EXCHANGES` exchanges, with an explicit omission marker
/// when older ones are dropped so the model knows it is merging a tail.
pub(crate) fn consolidate_transcript(exchanges: &[Exchange]) -> String {
    let skip = exchanges.len().saturating_sub(CONSOLIDATE_MAX_EXCHANGES);
    let mut transcript = String::new();
    if skip > 0 {
        transcript.push_str(&format!(
            "[\u{2026} {skip} earlier exchanges omitted \u{2026}]\n\n"
        ));
    }
    for e in &exchanges[skip..] {
        transcript.push_str("Q: ");
        transcript.push_str(&e.question);
        transcript.push_str("\nA: ");
        transcript.push_str(&e.answer);
        transcript.push_str("\n\n");
    }
    transcript
}

/// How many most-recent exchanges are sent as conversation history (each is
/// two turns). Older exchanges age out of the wire request — `S` consolidate
/// is the archival path for anything the model still needs to remember.
const CHAT_HISTORY_TURNS: usize = 6;

/// Question-only wire form for a turn whose passage context is already
/// present verbatim in an earlier turn of the same request.
pub(crate) fn same_passage_question(q: &str) -> String {
    format!("Reader's question (about the same passage context given above):\n{}", q)
}

/// Build the wire history from the last `CHAT_HISTORY_TURNS` exchanges,
/// deduping repeated passage context: within the window, an exchange whose
/// chip (cursor-segment fingerprint) matches the previous one is sent
/// question-only — its 5-segment context block is byte-identical to the one
/// already in the conversation. The first exchange in the window always
/// carries its full user_msg, so capping can never orphan a question from
/// its passage. Returns the turns plus the last exchange's chip (for the
/// current message's own dedupe check).
/// The user turn a question-less auto-gloss exchange stands in for, so history
/// never carries an empty user turn (which the API rejects). Grounded in the
/// passage the exchange still holds; falls back to a bare instruction if the
/// source is somehow empty, since the return must never itself be empty.
pub(crate) fn gloss_history_user_turn(e: &Exchange) -> String {
    let src = e.source_markup.trim();
    if src.is_empty() {
        "Provide a reader's gloss of the preceding passage.".to_string()
    } else {
        format!("Provide a reader's gloss of this passage:\n\n{src}")
    }
}

pub(crate) fn build_history_turns(
    exchanges: &[Exchange],
) -> (Vec<crate::claude::ChatTurn>, Option<String>) {
    let start = exchanges.len().saturating_sub(CHAT_HISTORY_TURNS);
    let mut turns = Vec::new();
    let mut prev_chip: Option<&str> = None;
    for e in &exchanges[start..] {
        let content = if prev_chip == Some(e.chip.as_str()) {
            same_passage_question(&e.question)
        } else {
            e.user_msg.clone()
        };
        // An auto-gloss exchange has an empty user_msg AND question (`-` asks
        // nothing), so `content` is empty here — and the API rejects a user
        // turn with empty content (HTTP 400 "messages must have non-empty
        // content"), which broke the FIRST follow-up asked with `a`. Synthesize
        // the request the gloss stood in for, grounded in the passage the
        // exchange still carries, so the assistant's gloss answer has a valid
        // antecedent instead of a blank one.
        let content = if content.trim().is_empty() {
            gloss_history_user_turn(e)
        } else {
            content
        };
        prev_chip = Some(e.chip.as_str());
        turns.push(crate::claude::ChatTurn { role: "user", content });
        turns.push(crate::claude::ChatTurn { role: "assistant", content: e.answer.clone() });
    }
    (turns, prev_chip.map(str::to_string))
}

/// Parse a revision reply of the form "Q: ...\nA: ..." (A may span
/// paragraphs). Falls back to (fallback_q, whole reply) when the format is
/// absent, so a format-ignoring model still yields a usable answer.
pub(crate) fn parse_revised_qa(reply: &str, fallback_q: &str) -> (String, String) {
    let trimmed = reply.trim();
    if let Some(rest) = trimmed.strip_prefix("Q:") {
        if let Some(a_pos) = rest.find("\nA:") {
            let q = rest[..a_pos].trim().to_string();
            let a = rest[a_pos + 3..].trim().to_string();
            if !q.is_empty() && !a.is_empty() {
                return (q, a);
            }
        }
    }
    (fallback_q.to_string(), trimmed.to_string())
}

#[cfg(test)]
mod question_row_tests {
    use super::{has_question_row, Exchange};

    fn ex(question: &str) -> Exchange {
        Exchange {
            question: question.to_string(),
            answer: String::new(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }
    }

    // An auto-gloss exchange (`push_gloss_exchange`) stores an empty
    // question — no `Q:` row (Bug 1: a bare "▶ Q:" is a Q&A affordance on
    // content that isn't Q&A).
    #[test]
    fn gloss_exchange_has_no_question_row() {
        assert!(!has_question_row(&ex("")));
    }

    // A follow-up exchange asked with `a` always carries a real question and
    // must keep its `Q:` row — this must NOT regress.
    #[test]
    fn followup_exchange_keeps_question_row() {
        assert!(has_question_row(&ex("What does this line mean?")));
    }
}

#[cfg(test)]
mod first_question_tests {
    use super::{is_first_question_exchange, Exchange};

    fn ex(question: &str) -> Exchange {
        Exchange {
            question: question.to_string(),
            answer: String::new(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }
    }

    // The auto-gloss exchange (exchanges[0], empty question) never counts —
    // an empty list, or a list with only the gloss, has zero Q&A exchanges,
    // not one.
    #[test]
    fn empty_list_is_not_the_first_question() {
        assert!(!is_first_question_exchange(&[]));
    }

    #[test]
    fn gloss_only_is_not_the_first_question() {
        assert!(!is_first_question_exchange(&[ex("")]));
    }

    // Gloss followed by the reader's first `a` question: exactly one Q&A
    // exchange — this is the auto-save trigger.
    #[test]
    fn gloss_then_one_question_is_first() {
        assert!(is_first_question_exchange(&[ex(""), ex("What does this mean?")]));
    }

    // No gloss at all (panel opened straight into `a`, if that's ever
    // possible): still the first Q&A on a single real question.
    #[test]
    fn single_question_with_no_gloss_is_first() {
        assert!(is_first_question_exchange(&[ex("What does this mean?")]));
    }

    // A second follow-up question must NOT re-trigger auto-save.
    #[test]
    fn second_question_is_not_first() {
        assert!(!is_first_question_exchange(&[
            ex(""),
            ex("First question?"),
            ex("Second question?"),
        ]));
    }
}

#[cfg(test)]
mod history_tests {
    use super::{build_history_turns, Exchange, CHAT_HISTORY_TURNS};

    fn ex(chip: &str, q: &str, user_msg: &str, a: &str) -> Exchange {
        Exchange {
            question: q.to_string(),
            answer: a.to_string(),
            chip: chip.to_string(),
            user_msg: user_msg.to_string(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }
    }

    fn gloss_ex(chip: &str, source: &str, a: &str) -> Exchange {
        // An auto-gloss exchange: empty question AND user_msg, but a real
        // source_markup and answer. This is the shape that sent an empty user
        // turn to the API (HTTP 400) before build_history_turns synthesized one.
        let mut e = ex(chip, "", "", a);
        e.source_markup = source.to_string();
        e
    }

    #[test]
    fn gloss_exchange_gets_a_synthesized_nonempty_user_turn() {
        // A gloss followed by a real question on the same passage: the gloss's
        // user turn must be non-empty (the bug: it was ""), and grounded in
        // the source it carries.
        let exchanges = [
            gloss_ex("chipA", "<verse>Stand by my side</verse>", "gloss answer"),
            ex("chipA", "What does it mean?", "What does it mean?", "a2"),
        ];
        let (turns, _last) = build_history_turns(&exchanges);
        assert_eq!(turns.len(), 4);
        assert!(!turns[0].content.trim().is_empty(), "gloss user turn was empty");
        assert!(turns[0].content.contains("Stand by my side"));
        assert_eq!(turns[1].content, "gloss answer");
    }

    #[test]
    fn gloss_exchange_with_empty_source_still_yields_nonempty_turn() {
        let exchanges = [gloss_ex("chipA", "", "gloss answer")];
        let (turns, _last) = build_history_turns(&exchanges);
        assert_eq!(turns.len(), 2);
        assert!(!turns[0].content.trim().is_empty());
    }

    #[test]
    fn same_chip_exchange_sends_question_only() {
        let exchanges = [
            ex("chipA", "q1", "FULL1", "a1"),
            ex("chipA", "q2", "FULL2", "a2"),
            ex("chipB", "q3", "FULL3", "a3"),
        ];
        let (turns, last_chip) = build_history_turns(&exchanges);
        assert_eq!(turns.len(), 6);
        assert_eq!(turns[0].content, "FULL1");
        // Same passage as the turn above: question only, no context block.
        assert!(turns[2].content.contains("same passage"));
        assert!(turns[2].content.ends_with("q2"));
        // New passage: full context returns.
        assert_eq!(turns[4].content, "FULL3");
        assert_eq!(last_chip.as_deref(), Some("chipB"));
    }

    #[test]
    fn history_caps_at_window_and_window_head_gets_full_context() {
        // 8 exchanges, all on the same passage: only the last 6 are sent,
        // and the first IN THE WINDOW must carry full context even though
        // its chip matches the (evicted) exchange before it.
        let exchanges: Vec<Exchange> = (0..8)
            .map(|i| ex("chipA", &format!("q{}", i), &format!("FULL{}", i), "a"))
            .collect();
        let (turns, _) = build_history_turns(&exchanges);
        assert_eq!(turns.len(), CHAT_HISTORY_TURNS * 2);
        assert_eq!(turns[0].content, "FULL2");
        for i in 1..CHAT_HISTORY_TURNS {
            assert!(turns[i * 2].content.contains("same passage"));
        }
    }

    #[test]
    fn empty_history_yields_no_turns_and_no_chip() {
        let (turns, last_chip) = build_history_turns(&[]);
        assert!(turns.is_empty());
        assert_eq!(last_chip, None);
    }
}

#[cfg(test)]
mod revision_tests {
    use super::parse_revised_qa;

    #[test]
    fn parses_q_and_multiparagraph_a() {
        let (q, a) = parse_revised_qa(
            "Q: Sharper question?\nA: First paragraph.\n\nSecond paragraph.",
            "old q",
        );
        assert_eq!(q, "Sharper question?");
        assert_eq!(a, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn falls_back_when_format_absent() {
        let (q, a) = parse_revised_qa("Just a plain revised answer.", "old q");
        assert_eq!(q, "old q");
        assert_eq!(a, "Just a plain revised answer.");
    }

    #[test]
    fn falls_back_when_a_missing() {
        let (q, a) = parse_revised_qa("Q: only a question", "old q");
        assert_eq!(q, "old q");
        assert_eq!(a, "Q: only a question");
    }
}

#[cfg(test)]
mod consolidate_tests {
    use super::*;

    fn exchange(i: usize) -> Exchange {
        Exchange {
            question: format!("Q{i}?"),
            answer: format!("A{i}."),
            chip: "1.1".into(),
            user_msg: String::new(),
            div1: 1,
            div2: 1,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }
    }

    #[test]
    fn short_conversation_transcribes_whole_with_no_marker() {
        let ex: Vec<Exchange> = (1..=5).map(exchange).collect();
        let t = consolidate_transcript(&ex);
        assert!(t.contains("Q1?") && t.contains("A5."));
        assert!(!t.contains("omitted"));
    }

    #[test]
    fn long_conversation_keeps_last_12_and_marks_omission() {
        let ex: Vec<Exchange> = (1..=15).map(exchange).collect();
        let t = consolidate_transcript(&ex);
        assert!(t.contains("3 earlier exchanges omitted"));
        assert!(!t.contains("Q3?"), "oldest exchanges dropped");
        assert!(t.contains("Q4?") && t.contains("Q15?"), "last 12 kept");
    }

    #[test]
    fn exactly_at_cap_has_no_marker() {
        let ex: Vec<Exchange> = (1..=12).map(exchange).collect();
        let t = consolidate_transcript(&ex);
        assert!(!t.contains("omitted"));
        assert!(t.contains("Q1?") && t.contains("Q12?"));
    }
}

#[cfg(test)]
mod gloss_cycle_tests {
    use super::*;

    #[test]
    fn forward_wraps_at_the_end() {
        assert_eq!(wrap_index(0, 1, 3), 1);
        assert_eq!(wrap_index(1, 1, 3), 2);
        assert_eq!(wrap_index(2, 1, 3), 0); // wraps
    }

    #[test]
    fn backward_wraps_at_the_start() {
        assert_eq!(wrap_index(2, -1, 3), 1);
        assert_eq!(wrap_index(1, -1, 3), 0);
        assert_eq!(wrap_index(0, -1, 3), 2); // wraps
    }

    #[test]
    fn single_gloss_stays_put() {
        assert_eq!(wrap_index(0, 1, 1), 0);
        assert_eq!(wrap_index(0, -1, 1), 0);
    }

    /// Guard against a % panic / underflow on an empty list.
    #[test]
    fn empty_list_stays_at_zero() {
        assert_eq!(wrap_index(0, 1, 0), 0);
        assert_eq!(wrap_index(0, -1, 0), 0);
    }
}

/// Fix 2 (do not highlight speaker labels; only lines of dialogue):
/// `first_landable_at_or_after` (`snap_row_cursor_to_exchange`'s helper) must
/// skip past a leading unlandable (Speaker) widget, stay put when already on
/// a landable one, and fall back to `from` itself rather than panicking or
/// running off the end when nothing at or after it is landable. House style
/// per `row_cursor_step_tests` above — pure index math, no `AppState`.
#[cfg(test)]
mod first_landable_at_or_after_tests {
    use super::first_landable_at_or_after;

    #[test]
    fn first_landable_finds_the_leading_speaker_and_advances() {
        let landable = [false, true, true];
        assert_eq!(first_landable_at_or_after(0, &landable), 1);
    }

    #[test]
    fn first_landable_stays_put_when_already_landable() {
        let landable = [false, true, true];
        assert_eq!(first_landable_at_or_after(1, &landable), 1);
    }

    /// Defensive fallback: nothing at or after `from` is landable — return
    /// `from` itself rather than panicking or running off the end.
    #[test]
    fn first_landable_falls_back_to_from_when_nothing_found() {
        let landable = [true, false, false];
        assert_eq!(first_landable_at_or_after(1, &landable), 1);
    }
}

/// `widget_row_count` / `build_transcript_rows`' `row_owner` map (CHANGE 1):
/// the row cursor moves in WIDGET space, which only diverges from
/// `Vec<TranscriptRow>` space at a `GlossAnswer` row (it explodes into one
/// widget per `gloss_render::chat_gloss_rows` row). These tests pin that
/// divergence down, and prove `row_owner` correctly maps every exploded
/// widget back to its owning exchange — the mechanism `s`/`▶`/Ctrl+n/p rely
/// on to stay correct once j/k can land mid-gloss.
#[cfg(test)]
mod row_cursor_widget_tests {
    use super::{build_transcript_rows, widget_row_count, Exchange};
    use crate::ui::chat_panel::TranscriptRow as R;

    fn plain_ex(chip: &str, question: &str, answer: &str) -> Exchange {
        Exchange {
            question: question.to_string(),
            answer: answer.to_string(),
            chip: chip.to_string(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }
    }

    fn gloss_ex(chip: &str, markup: &str) -> Exchange {
        // push_gloss_exchange always stores an empty question (see its doc
        // comment) — that's what routes answer_row to GlossAnswer.
        plain_ex(chip, "", markup)
    }

    #[test]
    fn plain_answer_is_one_widget() {
        assert_eq!(widget_row_count(&R::Answer("text".to_string())), 1);
        assert_eq!(widget_row_count(&R::Question("Q: x".to_string())), 1);
        assert_eq!(widget_row_count(&R::Chip("chip".to_string())), 1);
        assert_eq!(widget_row_count(&R::SavedMark), 1);
        assert_eq!(widget_row_count(&R::Thinking), 1);
    }

    #[test]
    fn gloss_answer_explodes_into_one_widget_per_typed_row() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side</verse>\n\
                       <gloss>Cymbeline honors him.</gloss>";
        // 3 rows: Speaker, Verse, Gloss (chat_gloss_rows_tests pins the same
        // split in gloss_render.rs).
        assert_eq!(widget_row_count(&R::GlossAnswer(markup.to_string())), 3);
    }

    // Markup with none of the recognized tags falls back to ONE widget
    // (gloss_answer_specs's own plain-label fallback) — must not panic on a
    // zero-length row_owner slice or silently vanish from the count.
    #[test]
    fn untagged_gloss_answer_falls_back_to_one_widget() {
        assert_eq!(widget_row_count(&R::GlossAnswer("no tags here".to_string())), 1);
    }

    #[test]
    fn row_owner_maps_every_widget_of_a_gloss_exchange_to_its_index() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side</verse>\n\
                       <gloss>Cymbeline honors him.</gloss>";
        let exchanges = vec![gloss_ex("chipA", markup)];
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 0, false);
        // Chip + exploded gloss (3 widgets) = 4 widget rows, all owned by
        // exchange 0. (This exchange carries an explicit chip; a real lone
        // gloss has an empty one and renders no chip row at all — see
        // `lone_gloss_renders_no_chip_row`.)
        assert_eq!(row_owner, vec![0, 0, 0, 0]);
        assert_eq!(row_owner.len(), 4);
        assert_eq!(rows.len(), 2); // Vec<TranscriptRow> space: Chip + GlossAnswer(1)
        // The cursor lands on the exchange's first CONTENT row, past the chip:
        // the accent bar belongs on the gloss, not on a "2 of 5" label.
        assert_eq!(cursor_row, 1);
    }

    /// A lone gloss's chip is empty (`gloss_chip`), so no chip row renders and
    /// the accent bar sits on the gloss's first line — the panel shows the
    /// gloss and nothing else.
    #[test]
    fn lone_gloss_renders_no_chip_row() {
        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side</verse>\n\
                       <gloss>Cymbeline honors him.</gloss>";
        let exchanges = vec![gloss_ex("", markup)];
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 0, false);
        assert_eq!(rows.len(), 1); // GlossAnswer only — no Chip
        assert_eq!(row_owner, vec![0, 0, 0]); // the 3 exploded gloss widgets
        assert_eq!(cursor_row, 0);
    }

    #[test]
    fn row_owner_distinguishes_two_plain_exchanges() {
        let exchanges = vec![
            plain_ex("chipA", "Q1?", "A1"),
            plain_ex("chipB", "Q2?", "A2"),
        ];
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 1, false);
        // Each exchange: Chip, Question, Answer = 3 widgets.
        assert_eq!(row_owner, vec![0, 0, 0, 1, 1, 1]);
        assert_eq!(rows.len(), 6);
        // Cursor (exchange 1) leads at its Question row (widget index 4).
        assert_eq!(cursor_row, 4);
    }

    #[test]
    fn row_owner_covers_saved_mark_widget() {
        let mut e = plain_ex("chipA", "Q1?", "A1");
        e.saved_id = Some(42);
        let (rows, _cursor_row, row_owner) = build_transcript_rows(&[e], 0, false);
        // Chip, Question, Answer, SavedMark = 4 widgets, all exchange 0.
        assert_eq!(row_owner, vec![0, 0, 0, 0]);
        assert_eq!(rows.len(), 4);
    }
}

/// `V`/`y` visual-selection range arithmetic (this feature): anchor/cursor
/// resolve to an inclusive `(start, end)` in EITHER order — `j`/`k` can
/// extend the live end above or below the anchor — and the resulting range
/// must slice the flattened widget-text list correctly, including the
/// single-row (no-selection) case `y` also uses. Pure index math, same house
/// style as `row_cursor_step_tests`/`row_cursor_widget_tests` above; the
/// actual `wl-copy` spawn and GTK CSS-class painting are not unit-testable
/// and are exercised only by manual/headless on-screen verification.
#[cfg(test)]
mod visual_selection_tests {
    use super::visual_selection_range;

    #[test]
    fn anchor_before_cursor_is_already_ordered() {
        assert_eq!(visual_selection_range(1, 4), (1, 4));
    }

    #[test]
    fn anchor_after_cursor_flips_to_ordered() {
        // j/k moved the live end (cursor) UP past the anchor — extend-up.
        assert_eq!(visual_selection_range(4, 1), (1, 4));
    }

    #[test]
    fn anchor_equals_cursor_is_a_single_row() {
        assert_eq!(visual_selection_range(2, 2), (2, 2));
    }

    #[test]
    fn anchor_at_zero_extends_down_to_last_row() {
        assert_eq!(visual_selection_range(0, 7), (0, 7));
    }

    /// Slice out of a flattened widget-text list, mirroring the clamp
    /// `yank_transcript_row_or_selection` applies before indexing
    /// (`start.min(n-1)`/`end.min(n-1)`) — guards a stale anchor pointing
    /// past a transcript that shrank without a render in between.
    #[test]
    fn range_slices_the_expected_rows() {
        let texts: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        let (start, end) = visual_selection_range(1, 3);
        assert_eq!(&texts[start..=end], &["b".to_string(), "c".to_string(), "d".to_string()]);
    }

    #[test]
    fn single_row_no_selection_slices_exactly_one() {
        let texts: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let cursor = 1usize;
        let (start, end) = (cursor, cursor); // yank's no-selection range
        assert_eq!(&texts[start..=end], &["b".to_string()]);
    }

    #[test]
    fn clamp_guards_a_stale_range_past_a_shrunk_list() {
        let texts: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let n = texts.len();
        let (raw_start, raw_end) = visual_selection_range(0, 5); // stale: transcript shrank
        let (start, end) = (raw_start.min(n - 1), raw_end.min(n - 1));
        assert_eq!(&texts[start..=end], &["a".to_string(), "b".to_string()]);
    }

    /// Fix 2's spanned-speaker decision, end to end: a `V` selection anchored
    /// on the last verse of one speaker's block, extended (via j landing on
    /// the NEXT block's first verse — never on the label in between, the same
    /// skip-unlandable-widgets contract `chat_pagination::step_cursor_paged`
    /// enforces in production) still yanks the spanned speaker label —
    /// `yank_transcript_row_or_selection` does NOT filter unlandable rows out
    /// of the slice, it only gates where the cursor/anchor can LAND (see that
    /// function's doc comment for the "why" — a pasted excerpt that silently
    /// dropped the speaker name would be worse than one that keeps it).
    #[test]
    fn v_selection_spanning_a_speaker_boundary_yanks_the_speaker_label() {
        use super::{build_transcript_rows, landable_mask, Exchange};
        use crate::ui::chat_panel::row_widget_texts;

        let markup = "<speaker>CYMBELINE</speaker>\n\
                       <verse>Stand by my side</verse>\n\
                       <speaker>BELARIUS</speaker>\n\
                       <verse>I will, my liege</verse>";
        let exchanges = vec![Exchange {
            question: String::new(), // routes to GlossAnswer, see answer_row
            answer: markup.to_string(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: None,
        }];
        let (rows, _cursor_row, _row_owner) = build_transcript_rows(&exchanges, 0, false);
        // Widget space: [Speaker(false), Verse(true), Speaker(false), Verse(true)].
        let landable = landable_mask(&rows);
        assert_eq!(landable, vec![false, true, false, true]);

        // V anchors on widget 1 (CYMBELINE's verse, the only place it COULD
        // land); j lands the live end on widget 3 (BELARIUS's verse),
        // skipping widget 2 (the BELARIUS label) as a landing spot — but the
        // SELECTION still spans widget 2. `step_next_landable` here is a
        // single-page stand-in for `chat_pagination::step_cursor_paged`'s
        // within-page walk, kept local to this test since the production
        // path additionally requires page bookkeeping this test doesn't need.
        fn step_next_landable(cur: usize, landable: &[bool]) -> Option<usize> {
            (cur + 1..landable.len()).find(|&i| landable[i])
        }
        let anchor = 1;
        let cursor = step_next_landable(anchor, &landable).unwrap();
        assert_eq!(cursor, 3);

        let all_texts: Vec<String> = rows.iter().flat_map(row_widget_texts).collect();
        let (start, end) = visual_selection_range(anchor, cursor);
        let yanked = all_texts[start..=end].join("\n");
        assert_eq!(yanked, "Stand by my side\nBELARIUS\nI will, my liege");
    }
}

/// `t`'s view-toggle logic (this feature): the pure flip direction and the
/// row-building for `PanelView::Journal` (a `JournalPage` list -> transcript
/// rows, including the empty-placeholder case). House style per
/// `gloss_cycle_tests`/`visual_selection_tests` above — pure functions, no
/// `AppState`/GTK. GTK painting (`render_journal_view`'s actual widget
/// rebuild) is not unit-testable and is exercised only by manual/headless
/// on-screen verification.
#[cfg(test)]
mod panel_view_toggle_tests {
    use super::{
        clamp_journal_cursor, flip_view, journal_entry_first_row, journal_view_rows,
        split_answer_paragraphs, PanelView,
    };
    use crate::db::journal::JournalPage;
    use crate::ui::chat_panel::TranscriptRow as R;

    #[test]
    fn flip_cycles_gloss_and_journal() {
        assert_eq!(flip_view(PanelView::Gloss), PanelView::Journal);
        assert_eq!(flip_view(PanelView::Journal), PanelView::Gloss);
        assert_eq!(flip_view(flip_view(PanelView::Gloss)), PanelView::Gloss);
    }

    /// The three-way requirement this feature adds: `t` from a shown Q&A
    /// (`Question`) must land on `Gloss` in ONE press — the non-gloss side of
    /// the toggle, same direction `Journal` already goes. There is no reverse
    /// arm (`Gloss -> Question`): `Question` is only ever entered by asking a
    /// follow-up, never by cycling into it (see `flip_view`'s doc comment).
    #[test]
    fn flip_from_question_reaches_gloss() {
        assert_eq!(flip_view(PanelView::Question), PanelView::Gloss);
    }

    #[test]
    fn default_view_is_gloss() {
        // The panel's existing behavior (unchanged) must be what a freshly
        // reset ChatState (Default::default(), e.g. close_chat_layout /
        // on_work_switched) shows.
        assert_eq!(PanelView::default(), PanelView::Gloss);
    }

    fn page(question: &str, answer: &str) -> JournalPage {
        JournalPage {
            id: 1,
            div1: 1,
            div2: 0,
            question: question.to_string(),
            answer: answer.to_string(),
            claude_model: String::new(),
            timestamp: String::new(),
            start_citation: Some("W.1.0.1".to_string()),
            end_citation: Some("W.1.0.1".to_string()),
            source_text: None,
            kind: "qa".to_string(),
        }
    }

    #[test]
    fn split_answer_paragraphs_by_blank_lines() {
        assert_eq!(split_answer_paragraphs("one\n\ntwo\n\nthree"),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]);
        // single paragraph → one chunk
        assert_eq!(split_answer_paragraphs("just one"), vec!["just one".to_string()]);
        // empty → one empty chunk (entry keeps an answer row)
        assert_eq!(split_answer_paragraphs("   "), vec![String::new()]);
    }

    #[test]
    fn empty_list_renders_a_placeholder_row() {
        let (rows, row_owner) = journal_view_rows(&[]);
        assert_eq!(rows.len(), 1);
        assert!(row_owner.is_empty()); // nothing landable in the placeholder
        match &rows[0] {
            R::Answer(text) => assert_eq!(text, "No journal entries for this passage"),
            other => panic!("expected a placeholder Answer row, got a different variant: {}",
                match other {
                    R::Question(_) => "Question",
                    R::GlossAnswer(_) => "GlossAnswer",
                    R::Chip(_) => "Chip",
                    R::Error(_) => "Error",
                    R::Thinking => "Thinking",
                    R::SavedMark => "SavedMark",
                    R::Answer(_) => unreachable!(),
                }),
        }
    }

    #[test]
    fn one_entry_renders_question_then_plain_answer() {
        let pages = vec![page("What does York mean?", "He is plotting.")];
        let (rows, row_owner) = journal_view_rows(&pages);
        // Single-paragraph answer → Q + one Answer row; both owned by entry 0.
        assert_eq!(rows.len(), 2);
        assert_eq!(row_owner, vec![0, 0]);
        match &rows[0] {
            R::Question(t) => assert_eq!(t, "Q: What does York mean?"),
            _ => panic!("row 0 must be a Question row"),
        }
        // Journal answers are prose: must render as a plain Answer row, never
        // GlossAnswer (which would try to parse <speaker>/<verse> markup out
        // of ordinary prose text).
        match &rows[1] {
            R::Answer(t) => assert_eq!(t, "He is plotting."),
            _ => panic!("row 1 must be a plain Answer row, not GlossAnswer"),
        }
    }

    #[test]
    fn multiple_entries_render_in_list_order_with_no_placeholder() {
        let pages = vec![
            page("Q1?", "A1."),
            page("Q2?", "A2."),
            page("Q3?", "A3."),
        ];
        let (rows, row_owner) = journal_view_rows(&pages);
        // Single-paragraph answers: Q/A pair per entry, no placeholder mixed in.
        assert_eq!(rows.len(), 6);
        assert_eq!(row_owner, vec![0, 0, 1, 1, 2, 2]);
        let texts: Vec<&str> = rows
            .iter()
            .map(|r| match r {
                R::Question(t) | R::Answer(t) => t.as_str(),
                _ => panic!("unexpected row kind in journal view"),
            })
            .collect();
        assert_eq!(
            texts,
            vec!["Q: Q1?", "A1.", "Q: Q2?", "A2.", "Q: Q3?", "A3."]
        );
    }

    /// The Task 3 shape change: a multi-paragraph answer is exploded into one
    /// `Answer` row per paragraph, so an entry is now `1 + n_paragraphs`
    /// widgets (not the old fixed 2), and every widget maps back to its entry
    /// through `row_owner` (widget -> entry). This is what lets the accent bar
    /// traverse the answer paragraphs instead of being stuck on the `Q:` row.
    #[test]
    fn entry_is_one_plus_n_paragraph_rows_with_owner_map() {
        // Entry 0: 2-paragraph answer → Q + 2 answer rows = 3 widgets.
        // Entry 1: 1-paragraph answer → Q + 1 answer row  = 2 widgets.
        let pages = vec![
            page("Q0?", "First para.\n\nSecond para."),
            page("Q1?", "Only para."),
        ];
        let (rows, row_owner) = journal_view_rows(&pages);
        assert_eq!(rows.len(), 5); // 3 + 2
        // Widget -> entry: entry 0 owns the first 3 widgets, entry 1 the next 2.
        assert_eq!(row_owner, vec![0, 0, 0, 1, 1]);
        let texts: Vec<&str> = rows
            .iter()
            .map(|r| match r {
                R::Question(t) | R::Answer(t) => t.as_str(),
                _ => panic!("unexpected row kind in journal view"),
            })
            .collect();
        assert_eq!(
            texts,
            vec!["Q: Q0?", "First para.", "Second para.", "Q: Q1?", "Only para."]
        );
        // journal_entry_first_row maps each entry to its leading (Q:) widget.
        assert_eq!(journal_entry_first_row(&row_owner, 0), 0);
        assert_eq!(journal_entry_first_row(&row_owner, 1), 3);
    }

    #[test]
    fn clamp_journal_cursor_bounds() {
        assert_eq!(clamp_journal_cursor(0, 0), 0); // empty list
        assert_eq!(clamp_journal_cursor(5, 0), 0); // empty list, stale cursor
        assert_eq!(clamp_journal_cursor(0, 3), 0);
        assert_eq!(clamp_journal_cursor(2, 3), 2);
        assert_eq!(clamp_journal_cursor(9, 3), 2); // clamps to len-1
    }

}

/// Question-view row shape (top-landing feature): a Q&A exchange renders as
/// `Q:` + one `Answer` row PER PARAGRAPH (same split the Journal view uses),
/// so the accent-bar cursor can traverse the answer and pagination never
/// produces a single oversized answer widget. A reader-gloss exchange (empty
/// question, raw markup) must stay one `GlossAnswer` row.
#[cfg(test)]
mod question_view_rows_tests {
    use super::{build_single_exchange_rows, Exchange};
    use crate::ui::chat_panel::TranscriptRow as R;

    fn ex(question: &str, answer: &str, saved: bool) -> Exchange {
        Exchange {
            question: question.to_string(),
            answer: answer.to_string(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id: if saved { Some(1) } else { None },
        }
    }

    #[test]
    fn qa_answer_splits_into_paragraph_rows() {
        let rows = build_single_exchange_rows(&ex("Why?", "one\n\ntwo\n\nthree", false), false);
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[0], R::Question(q) if q == "Q: Why?"));
        assert!(matches!(&rows[1], R::Answer(a) if a == "one"));
        assert!(matches!(&rows[2], R::Answer(a) if a == "two"));
        assert!(matches!(&rows[3], R::Answer(a) if a == "three"));
    }

    #[test]
    fn saved_mark_trails_the_paragraphs() {
        let rows = build_single_exchange_rows(&ex("Why?", "one\n\ntwo", true), false);
        assert_eq!(rows.len(), 4); // Q + 2 paragraphs + SavedMark
        assert!(matches!(rows.last(), Some(R::SavedMark)));
    }

    #[test]
    fn gloss_exchange_keeps_single_gloss_answer_row() {
        let rows = build_single_exchange_rows(
            &ex("", "<speaker>A</speaker>\n\n<verse>b</verse>", false),
            false,
        );
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], R::GlossAnswer(_)));
    }
}

/// `build_single_exchange_rows` — the `PanelView::Question` focused render's
/// pure seam (one exchange -> its rows, no gloss, no other exchange, no
/// chip). House style per `panel_view_toggle_tests`/`consolidate_tests`
/// above: pure functions, no `AppState`/GTK. The actual GTK paint
/// (`render_current_question`'s `render_paginated` call) is not unit-testable
/// and is exercised only by manual/headless on-screen verification.
#[cfg(test)]
mod question_view_tests {
    use super::{build_single_exchange_rows, Exchange};
    use crate::ui::chat_panel::TranscriptRow as R;

    fn qa_exchange(question: &str, answer: &str, saved_id: Option<i64>) -> Exchange {
        Exchange {
            question: question.to_string(),
            answer: answer.to_string(),
            chip: "Some source chip".to_string(),
            user_msg: String::new(),
            div1: 1,
            div2: 1,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id,
        }
    }

    #[test]
    fn follow_up_exchange_renders_question_then_plain_answer_no_chip() {
        let e = qa_exchange("What does York mean?", "He is plotting.", None);
        let rows = build_single_exchange_rows(&e, false);
        // Exactly 2 rows: Q + A. No Chip row, unlike the full transcript's
        // build_transcript_rows — a Question-view render never shows the
        // source-preview label (see the function's doc comment).
        assert_eq!(rows.len(), 2);
        match &rows[0] {
            R::Question(t) => assert_eq!(t, "Q: What does York mean?"),
            _ => panic!("row 0 must be a Question row"),
        }
        match &rows[1] {
            R::Answer(t) => assert_eq!(t, "He is plotting."),
            _ => panic!("row 1 must be a plain Answer row, not GlossAnswer"),
        }
    }

    /// A reader-gloss exchange (`push_gloss_exchange`'s empty-question
    /// convention — see `has_question_row`'s doc comment) must never reach
    /// `PanelView::Question` in practice (only `a`-submitted follow-ups do),
    /// but the row-builder itself is exercised here for completeness: no
    /// Question row, and the answer renders as GlossAnswer (raw markup), not
    /// a plain Answer label.
    #[test]
    fn gloss_shaped_exchange_omits_question_row_and_uses_gloss_answer() {
        let e = qa_exchange("", "<speaker>YORK</speaker><verse>Speak.</verse>", None);
        let rows = build_single_exchange_rows(&e, false);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            R::GlossAnswer(markup) => assert!(markup.contains("YORK")),
            _ => panic!("row 0 must be a GlossAnswer row for an empty-question exchange"),
        }
    }

    #[test]
    fn saved_exchange_appends_saved_mark() {
        let e = qa_exchange("Q?", "A.", Some(42));
        let rows = build_single_exchange_rows(&e, false);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0], R::Question(_)));
        assert!(matches!(rows[1], R::Answer(_)));
        assert!(matches!(rows[2], R::SavedMark));
    }
}

/// Deleting a journal row from the chat panel must not leave dangling
/// references: an exchange saved to that row regains `saved_id: None` (so
/// `s` can re-save it and the SavedMark disappears on the next render), and
/// a pending `revision_of` aimed at the deleted row is cleared so Ctrl+Enter
/// cannot retarget a nonexistent entry.
#[cfg(test)]
mod delete_refs_tests {
    use super::{clear_deleted_journal_refs, Exchange};

    fn ex(saved_id: Option<i64>) -> Exchange {
        Exchange {
            question: "q".to_string(),
            answer: "a".to_string(),
            chip: String::new(),
            user_msg: String::new(),
            div1: 0,
            div2: 0,
            start_citation: String::new(),
            end_citation: String::new(),
            source_markup: String::new(),
            saved_id,
        }
    }

    #[test]
    fn clears_matching_saved_id_only() {
        let mut exchanges = vec![ex(Some(45)), ex(Some(46)), ex(None)];
        let rev = clear_deleted_journal_refs(&mut exchanges, None, 45);
        assert_eq!(exchanges[0].saved_id, None);
        assert_eq!(exchanges[1].saved_id, Some(46));
        assert_eq!(exchanges[2].saved_id, None);
        assert_eq!(rev, None);
    }

    #[test]
    fn clears_revision_of_pointing_at_deleted() {
        let mut exchanges = vec![ex(Some(45))];
        assert_eq!(clear_deleted_journal_refs(&mut exchanges, Some(45), 45), None);
    }

    #[test]
    fn keeps_revision_of_pointing_elsewhere() {
        let mut exchanges = vec![ex(None)];
        assert_eq!(clear_deleted_journal_refs(&mut exchanges, Some(46), 45), Some(46));
    }
}

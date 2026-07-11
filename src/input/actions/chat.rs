//! Chat layout (Tab): left chat panel + right-pinned card. This task ships
//! the layout toggle only; the panel widget and conversation land in later
//! tasks of the chat-layout plan.

use crate::app::AppState;
use gtk4::prelude::WidgetExt;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum freed left space (px) required to open the chat layout.
const CHAT_MIN_PANEL_W: i32 = 500;

/// One question/answer turn in the chat transcript.
pub(crate) struct Exchange {
    pub question: String,
    pub answer: String,
    pub chip: String,
    pub user_msg: String,
    pub div1: i64,
    pub div2: i64,
    pub start_citation: String,
    pub end_citation: String,
    pub source_markup: String,
    pub saved_id: Option<i64>,
}

/// Chat-layout session state: the transcript of exchanges, the selected
/// exchange (for save/revision), and whether a request is in flight.
#[derive(Default)]
pub(crate) struct ChatState {
    pub exchanges: Vec<Exchange>,
    pub cursor: usize,
    pub revision_of: Option<i64>,
    pub pending: bool,
}

/// Re-apply the card margins for the current chat_layout_open value.
pub(crate) fn reapply_card_margins(s: &AppState) {
    let ww = s.window.width().max(0);
    crate::app::layout::apply_card_sizing(
        &s.content_hbox,
        ww,
        crate::app::layout::effective_column_width(s),
        s.column_count(),
        s.translations_visible,
        s.chat_layout_open,
    );
}

pub(crate) fn close_chat_layout(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat = Default::default();
    s.chat_panel.render_rows(&[]);
    s.chat_layout_open = false;
    reapply_card_margins(s);
    s.input_mode = crate::app::InputMode::Reader;
    s.chat_panel.hide();
    crate::logging::log("CHAT: layout closed");
}

/// Work switch with the panel open: history clears (context would be from
/// another work). The new work's card geometry may no longer leave enough
/// free space for the panel (works can pin different layouts), so it must be
/// re-gated — but NOT here: at this hook point (inside
/// `display_work_at_with_prepared`, before the rest of `display_work`'s
/// column/layout setup runs) `s.window.width()` can observe a transient,
/// not-yet-settled window size — e.g. the panel's OWN stale width_request
/// (sized for the old work's layout) plus the new work's wider two-column
/// card can together push GTK to grow the window past its true fixed
/// compositor width before it settles back down. Gating on that transient
/// width computes free space against a phantom, oversized window and wrongly
/// decides "stays open" when the settled geometry would say "close" —
/// leaving the panel visibly overlapping the new card.
///
/// So: release the panel's width hold immediately (it can't inflate the
/// window if it no longer asks for a fixed size), and defer the real
/// re-gate/resize to `regate_panel`, run from the resize tick once geometry
/// has settled (see `chat_regate_pending` in `app/mod.rs`).
pub(crate) fn on_work_switched(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat = Default::default();
    s.chat_panel.render_rows(&[]);
    s.chat_panel.size_to_natural();
    s.chat_regate_pending = true;
    crate::logging::log("CHAT: work switch — regate deferred");
}

/// Re-check the chat panel against the CURRENT settled geometry: close with
/// a toast when the freed left space is too tight (e.g. after switching to a
/// two-column play), else re-size the panel to the new card rect.
pub(crate) fn regate_panel(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        close_chat_layout(s);
        crate::ui::toast::show_transient(
            &s.chapter_toast,
            "No room for chat panel at this layout",
            3,
        );
        return;
    }
    size_panel(s);
    set_panel_header(s);
    crate::logging::log(&format!("CHAT: regate kept panel (free={}px)", free));
}

pub(crate) fn toggle_chat_layout(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat_layout_open {
        // Panel already open: Tab (from reader focus) cycles INTO the prompt;
        // closing is Ctrl+Tab's job (ToggleLastOverlay shadow).
        focus_prompt(&mut s);
        return;
    }
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(&s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        crate::ui::toast::show_transient(
            &s.chapter_toast,
            "No room for chat panel at this layout",
            3,
        );
        return;
    }
    s.chat_layout_open = true;
    reapply_card_margins(&s);
    size_panel(&s);
    s.chat_panel.show();
    crate::logging::log(&format!("CHAT: layout opened (free={}px)", free));
    focus_prompt(&mut s);
}

/// Chat layout: the panel's vim prompt gains input focus. Opens the ask-card
/// input (title/hint/theme colors) and sets the panel header for the current
/// cursor position.
///
/// Title/hint are chosen honestly by mode: while `s.chat.revision_of.is_some()`
/// every Ctrl+Enter routes to `submit_revision` and UPDATES the saved journal
/// row, so the input must say "Revise this entry", not "Ask about this
/// passage" (these strings must match `save_selected_exchange`'s revision
/// `open_input` call). There is deliberately no separate "exit revision mode"
/// action in v1 — the documented route is to close and reopen the panel
/// (Ctrl+Tab, then Tab), which resets `s.chat` (see `close_chat_layout`) and
/// returns to ask mode.
///
/// If the input is already open, reopening it (via `open_input`) would wipe
/// any typed draft (including an error-restored question) by reseeding the
/// vim engine. So: only call `open_input` when the input is not already open.
/// If it's already open and the mode-appropriate title differs from what's
/// showing, retitle it — but ONLY when the input is empty (a draft always
/// wins over a title refresh).
pub(crate) fn focus_prompt(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    let (title, hint) = prompt_title_hint(s);
    if !s.chat_panel.input_is_open() {
        s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg);
    } else if s.chat_panel.peek_input_text().trim().is_empty() {
        // No draft to lose: re-titling via open_input is safe (it also
        // reseeds the vim engine, but there's nothing in it to destroy).
        s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg);
    }
    set_panel_header(s);
}

/// The honest title/hint pair for the current chat mode (revision vs ask).
fn prompt_title_hint(s: &AppState) -> (&'static str, &'static str) {
    if s.chat.revision_of.is_some() {
        ("Revise this entry", "Ctrl+Enter send \u{b7} s update \u{b7} Tab cycle")
    } else {
        ("Ask about this passage", "Ctrl+Enter send \u{b7} Tab cycle")
    }
}

/// Chat layout: the transcript pane gains input focus (j/k move the exchange
/// cursor, s saves, Tab cycles to the reader, Ctrl+Tab closes).
pub(crate) fn focus_transcript(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatTranscript;
}

/// Chat layout: the reader pane gains input focus (full reader keys live;
/// the panel stays open and visible).
pub(crate) fn focus_reader(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::Reader;
}

/// Submit the chat prompt's current text as a new turn: builds the segment
/// context + gloss context for the cursor's passage, assembles the multi-turn
/// history from prior exchanges, and dispatches the Claude chat request.
pub(crate) fn submit_chat_prompt(state_rc: &Rc<RefCell<AppState>>) {
    // Revision mode: the prompt text is an instruction, not a question.
    if state_rc.borrow().chat.revision_of.is_some() {
        chat_revision::submit_revision(state_rc);
        return;
    }
    let (question, system, user_msg, turns, model, chip, meta) = {
        let s = state_rc.borrow();
        if s.chat.pending {
            crate::ui::toast::show_transient(&s.chapter_toast, "Waiting for the previous reply\u{2026}", 2);
            return;
        }
        // Resolve the passage context BEFORE consuming the input text: a
        // validation failure (no work / no passage at cursor) must leave the
        // typed question untouched for retry, not silently clear it.
        let Some(work) = s.current_work.as_ref() else { return };
        let Some(seg) = crate::input::segments::segment_context(&s, 2) else {
            crate::ui::toast::show_transient(&s.chapter_toast, "No passage at the cursor", 2);
            return;
        };
        let Some(gctx) = crate::gloss::build_context_for_type(work, &seg.cursor_lines, "reader-gloss") else {
            crate::ui::toast::show_transient(&s.chapter_toast, "No passage at the cursor", 2);
            return;
        };
        let question = s.chat_panel.take_input_text().trim().to_string();
        if question.is_empty() {
            return;
        }
        let source_markup =
            crate::input::actions::echoes::build_source_header(&seg.cursor_lines, &gctx.speaker);
        let (genre, unit, _units) = crate::gloss::genre_unit(&work.work_type);
        let scene = crate::app::scene_synopsis::scene_label_for(&s, seg.div1, seg.div2);
        let mut unit_label = unit.to_string();
        if let Some(c) = unit_label.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        let user_msg = crate::input::segments::chat_user_message(
            genre, &work.title, &work.author, &unit_label, &scene,
            &seg.segments, seg.cursor_index, &question,
        );
        // Prior turns: each exchange contributes its full user_msg (context
        // embedded, so history stays coherent as the cursor moves) + answer.
        let mut turns: Vec<crate::claude::ChatTurn> = Vec::new();
        for e in &s.chat.exchanges {
            turns.push(crate::claude::ChatTurn { role: "user", content: e.user_msg.clone() });
            turns.push(crate::claude::ChatTurn { role: "assistant", content: e.answer.clone() });
        }
        turns.push(crate::claude::ChatTurn { role: "user", content: user_msg.clone() });
        let chip: String = seg.segments[seg.cursor_index].chars().take(120).collect();
        let meta = (
            seg.div1,
            seg.div2,
            gctx.start_citation.clone(),
            gctx.end_citation.clone(),
            source_markup,
        );
        (
            question,
            crate::gloss::journal_qa_prompt(&work.work_type),
            user_msg,
            turns,
            s.config.claude_model.clone(),
            chip,
            meta,
        )
    };

    {
        let mut s = state_rc.borrow_mut();
        s.chat.pending = true;
        render_transcript_with_thinking(&s, &question, &chip);
    }

    let (div1, div2, start_citation, end_citation, source_markup) = meta;
    let question_ok = question.clone();
    let question_err = question;
    crate::input::actions::claude_bridge::run_claude_chat_request(
        state_rc,
        system,
        turns,
        model,
        move |st, answer| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            s.chat.exchanges.push(Exchange {
                question: question_ok.clone(),
                answer,
                chip: chip.clone(),
                user_msg: user_msg.clone(),
                div1,
                div2,
                start_citation: start_citation.clone(),
                end_citation: end_citation.clone(),
                source_markup: source_markup.clone(),
                saved_id: None,
            });
            s.chat.cursor = s.chat.exchanges.len() - 1;
            render_transcript(&s);
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            render_transcript_with_error(&s, msg);
            // Restore the failed question for retry.
            s.chat_panel.paste_input_text(&question_err);
        },
    );
}

fn transcript_rows(s: &AppState) -> Vec<crate::ui::chat_panel::TranscriptRow> {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    let mut prev_chip: Option<&str> = None;
    for (i, e) in s.chat.exchanges.iter().enumerate() {
        if prev_chip != Some(e.chip.as_str()) {
            rows.push(R::Chip(e.chip.clone()));
        }
        prev_chip = Some(e.chip.as_str());
        let marker = if i == s.chat.cursor { "\u{25b8} " } else { "" };
        rows.push(R::Question(format!("{}Q: {}", marker, e.question)));
        rows.push(R::Answer(e.answer.clone()));
        if e.saved_id.is_some() {
            rows.push(R::SavedMark);
        }
    }
    rows
}

pub(crate) fn render_transcript(s: &AppState) {
    s.chat_panel.render_rows(&transcript_rows(s));
}

fn render_transcript_with_thinking(s: &AppState, question: &str, chip: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = transcript_rows(s);
    rows.push(R::Chip(chip.to_string()));
    rows.push(R::Question(format!("Q: {}", question)));
    rows.push(R::Thinking);
    s.chat_panel.render_rows(&rows);
}

fn render_transcript_with_error(s: &AppState, msg: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = transcript_rows(s);
    rows.push(R::Error(msg.to_string()));
    s.chat_panel.render_rows(&rows);
}

/// Move the transcript exchange cursor by `delta`, clamped to bounds, and
/// re-render.
pub(crate) fn transcript_cursor_move(s: &mut AppState, delta: i32) {
    let n = s.chat.exchanges.len();
    if n == 0 {
        return;
    }
    let cur = s.chat.cursor as i32 + delta;
    s.chat.cursor = cur.clamp(0, n as i32 - 1) as usize;
    render_transcript(s);
}

/// `s` on the transcript: save the selected exchange as a passage journal
/// page, mark it, and pivot the panel into the revision loop on that entry.
pub(crate) fn save_selected_exchange(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if let Some(id) = s.chat.revision_of {
        // Already saved: `s` re-confirms (row is persisted on every
        // successful revision); just toast.
        let _ = id;
        crate::ui::toast::show_transient(&s.chapter_toast, "Entry is saved", 2);
        return;
    }
    let idx = s.chat.cursor;
    let Some(e) = s.chat.exchanges.get(idx) else { return };
    let Some(work) = s.current_work.as_ref() else { return };
    let abbrev = work.canonical_abbrev.clone();
    let model = s.config.claude_model.clone();
    let (q, a) = (e.question.clone(), e.answer.clone());
    let saved = crate::db::queries::open_db_rw().and_then(|conn| {
        crate::db::journal::save_passage_page(
            &conn, &abbrev, e.div1, e.div2,
            &e.start_citation, &e.end_citation, &e.source_markup,
            &e.question, &e.answer, &model,
        )
    });
    match saved {
        Ok(id) => {
            s.chat.exchanges[idx].saved_id = Some(id);
            s.chat.revision_of = Some(id);
            render_saved_entry(&s, &q, &a);
            let (title, hint) = prompt_title_hint(&s);
            s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg);
            s.input_mode = crate::app::InputMode::ChatPrompt;
            crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
            crate::logging::log(&format!("CHAT: saved exchange as journal page {}", id));
        }
        Err(err) => {
            crate::ui::toast::show_transient(&s.chapter_toast, "Save failed", 3);
            crate::logging::log(&format!("CHAT: save failed: {}", err));
        }
    }
}

/// Revision view: the panel content IS the saved entry (Q + A), no history.
pub(crate) fn render_saved_entry(s: &AppState, question: &str, answer: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    s.chat_panel.render_rows(&[
        R::SavedMark,
        R::Question(format!("Q: {}", question)),
        R::Answer(answer.to_string()),
    ]);
}

/// Size the panel to the freed left space at the card's height.
pub(crate) fn size_panel(s: &AppState) {
    let ww = s.window.width().max(0);
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    let end = crate::app::layout::CARD_OUTER_MARGIN;
    // left outer margin (24) + gap to the card (16)
    let w = ww - card_w - end - 24 - 16;
    s.chat_panel.size_to(w, card_h);
}

pub(crate) fn set_panel_header(s: &AppState) {
    let Some(w) = s.current_work.as_ref() else {
        return;
    };
    let (d1, d2) = s
        .work_line_for_buffer(s.current_line)
        .and_then(|wi| w.lines.get(wi))
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    let scene = crate::app::scene_synopsis::scene_label_for(s, d1, d2);
    s.chat_panel.set_header(&w.title, &w.author, &scene);
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

/// Ctrl+Enter revision loop: sends a rewrite instruction for the saved entry,
/// parses Claude's revised Q&A, and updates the same journal row in place.
pub(crate) mod chat_revision {
    use super::*;

    /// Ctrl+Enter in revision mode: the prompt text is an instruction to
    /// revise the saved entry. Empty instruction = no-op (hand edits are not
    /// a chat concern). Claude may rewrite both Q and A (fixed output format,
    /// parsed leniently by parse_revised_qa).
    pub(crate) fn submit_revision(state_rc: &Rc<RefCell<AppState>>) {
        let (id, q, a, context, instruction, model) = {
            let s = state_rc.borrow();
            let Some(id) = s.chat.revision_of else { return };
            let instruction = s.chat_panel.take_input_text().trim().to_string();
            if instruction.is_empty() {
                crate::ui::toast::show_transient(&s.chapter_toast, "Type a revision instruction", 2);
                return;
            }
            let Some(e) = s.chat.exchanges.iter().find(|e| e.saved_id == Some(id)) else {
                return;
            };
            let Some(work) = s.current_work.as_ref() else { return };
            let scene = crate::app::scene_synopsis::scene_label_for(&s, e.div1, e.div2);
            let context = format!(
                "Work: {} by {}\nThis Q&A is filed under a PASSAGE in {}\n\nPassage:\n{}\n\nReturn the revised Q&A in exactly this format:\nQ: <revised question>\nA: <revised answer>",
                work.title, work.author, scene, e.source_markup,
            );
            (
                id,
                e.question.clone(),
                e.answer.clone(),
                context,
                instruction,
                s.config.claude_model.clone(),
            )
        };
        let instruction_err = instruction.clone();
        let user_msg =
            crate::input::actions::journal::rewrite_user_message(&context, &q, &a, &instruction);
        let work_type = state_rc
            .borrow()
            .current_work
            .as_ref()
            .map(|w| w.work_type.clone())
            .unwrap_or_default();
        {
            let s = state_rc.borrow();
            crate::ui::toast::show_persistent(&s.chapter_toast, "Rewriting\u{2026}");
        }
        let model_for_db = model.clone();
        crate::input::actions::claude_bridge::run_claude_request(
            state_rc,
            crate::gloss::journal_qa_prompt(&work_type),
            user_msg,
            model,
            move |st, reply| {
                let mut s = st.borrow_mut();
                let (new_q, new_a) = super::parse_revised_qa(&reply, &q);
                if let Some(e) = s.chat.exchanges.iter_mut().find(|e| e.saved_id == Some(id)) {
                    e.question = new_q.clone();
                    e.answer = new_a.clone();
                }
                super::render_saved_entry(&s, &new_q, &new_a);
                // Persist immediately: the revision loop's `s` re-update path
                // also exists, but the design stores exactly the model's
                // latest output, so write it now.
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if let Err(err) = crate::db::journal::update_journal_page(
                        &conn, id, &new_q, &new_a, &model_for_db,
                    ) {
                        crate::logging::log(&format!("CHAT: revision save failed: {}", err));
                    }
                    crate::input::actions::journal::purge_journal_audio(&conn, id);
                }
                crate::ui::toast::show_transient(&s.chapter_toast, "Rewritten", 2);
            },
            move |st, msg| {
                let s = st.borrow_mut();
                crate::ui::toast::show_transient(&s.chapter_toast, msg, 4);
                // Restore the failed instruction for retry, mirroring
                // submit_chat_prompt's error path.
                s.chat_panel.paste_input_text(&instruction_err);
            },
        );
    }
}

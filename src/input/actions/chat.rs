//! Chat layout (Tab): left chat panel + right-pinned card. This task ships
//! the layout toggle only; the panel widget and conversation land in later
//! tasks of the chat-layout plan.

use crate::app::AppState;
use gtk4::prelude::WidgetExt;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum freed left space (px) required to open the chat layout.
const CHAT_MIN_PANEL_W: i32 = 500;

/// Where the open chat panel sits. Pinned = single-column layout (card pinned
/// right, panel in the freed left space). Float* = two-column layout (panel
/// overlays one reading column; the card is untouched).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChatPlacement {
    Pinned,
    FloatLeft,
    FloatRight,
}

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
    /// Passage PINNED by opening the panel with `Tab` from visual (`V`) mode:
    /// the reader's selection, verbatim, as a one-segment context. While set,
    /// EVERY question in the session sends exactly this passage as the source
    /// text instead of re-deriving the cursor's segment ±2 neighbors — so
    /// follow-ups keep discussing the same passage even if the cursor drifts.
    /// Cleared with the rest of ChatState when the panel closes.
    pub pinned_passage: Option<crate::input::segments::SegmentContext>,
    /// Stored reader-glosses for the pinned passage, newest first, as
    /// `find_glosses_by_start` orders them. A DIFFERENT axis from `exchanges`:
    /// these are lit.db rows (including earlier sessions'), where `exchanges`
    /// is this session's in-memory transcript. `Ctrl+n`/`Ctrl+p` moves over
    /// this list; `j`/`k` moves over `exchanges`. Never share `cursor`.
    pub gloss_list: Vec<crate::db::queries::SavedGloss>,
    /// Index into `gloss_list` of the gloss currently shown in exchange #1.
    pub gloss_index: usize,
    /// The pinned passage as a gloss context — what regloss re-sends and what
    /// a save needs for the `passages` row. Set when `-` opens the panel.
    pub gloss_ctx: Option<crate::gloss::GlossContext>,
}

/// Re-apply the card margins for the current chat placement. Only a PINNED
/// open panel pins the card right; float placements leave the card alone.
pub(crate) fn reapply_card_margins(s: &AppState) {
    let ww = s.window.width().max(0);
    crate::app::layout::apply_card_sizing(
        &s.content_hbox,
        ww,
        crate::app::layout::effective_column_width(s),
        s.column_count(),
        s.translations_visible,
        s.chat_pinned(),
    );
}

pub(crate) fn close_chat_layout(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    // A pinned passage (Tab from V-mode) keeps its selection tag painted for as
    // long as the pin lives — the pin dies with ChatState below, so the mark
    // goes with it. Harmless no-op when nothing was pinned (the reader never
    // leaves the tag applied outside visual mode).
    crate::input::visual::clear_selection_highlight(s);
    s.chat = Default::default();
    s.chat_panel.render_rows(&[]);
    s.chat_layout_open = false;
    s.chat_placement = ChatPlacement::Pinned;
    s.chat_panel.container.remove_css_class("chat-panel-float");
    s.chat_panel.container.set_margin_start(24);
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

/// Pure boundary test: is `line` rendered in the RIGHT column of a spread
/// whose right column starts at `split` and whose last line is `end`?
pub(crate) fn line_in_right_column(line: usize, split: Option<usize>, end: usize) -> bool {
    split.is_some_and(|sp| line >= sp && line <= end)
}

/// Which column holds the cursor on the CURRENT spread. Table mode reads the
/// stored spread (authoritative); live mode falls back to column_split. Both
/// are (div1,div2)-derived boundaries — never text inference. Shared with the
/// vocab popup's float placement (`app::vocab_popup::position_vocab_popup`).
pub(crate) fn cursor_in_right_column(s: &AppState) -> bool {
    let line = s.current_line;
    if let Some(table) = crate::input::page_table::active_page_table(s) {
        if let Some(sp) = crate::input::page_table::spread_for_top(&table, s.page_top_line) {
            return line_in_right_column(line, sp.split, sp.end);
        }
    }
    let cs = crate::input::viewport::column_split(s, s.page_top_line);
    // Live ColumnSplit encodes "no right column" as split > page_end
    // (see the table synthesis in scroll.rs); normalize to Option.
    let split = (cs.split <= cs.page_end).then_some(cs.split);
    line_in_right_column(line, split, cs.page_end)
}

/// The float side that does NOT cover the cursor's column.
fn float_side_for_cursor(s: &AppState) -> ChatPlacement {
    if cursor_in_right_column(s) {
        ChatPlacement::FloatLeft
    } else {
        ChatPlacement::FloatRight
    }
}

/// The float side for a SELECTED RANGE, not just the cursor.
///
/// A selection inside one column floats over the other column, as with the
/// cursor. A selection SPANNING both columns has no free column — either side
/// covers half the passage — so it floats LEFT by rule. Pure (no AppState) so
/// the column arithmetic is unit-testable.
fn placement_for_range(
    start: usize,
    end: usize,
    split: Option<usize>,
    page_end: usize,
) -> ChatPlacement {
    let start_right = line_in_right_column(start, split, page_end);
    let end_right = line_in_right_column(end, split, page_end);
    if start_right != end_right {
        return ChatPlacement::FloatLeft; // spans both columns
    }
    if start_right {
        ChatPlacement::FloatLeft
    } else {
        ChatPlacement::FloatRight
    }
}

/// `placement_for_range` against the CURRENT page geometry, reading the split
/// from the same two sources as `cursor_in_right_column`, in the same order:
/// the active page table's spread when in table mode, else the live
/// `viewport::column_split` with its `split > page_end` "no right column"
/// normalization.
fn placement_for_selection(s: &AppState, start: usize, end: usize) -> ChatPlacement {
    if s.column_count() != 2 {
        return ChatPlacement::FloatRight;
    }
    if let Some(table) = crate::input::page_table::active_page_table(s) {
        if let Some(sp) = crate::input::page_table::spread_for_top(&table, s.page_top_line) {
            return placement_for_range(start, end, sp.split, sp.end);
        }
    }
    let cs = crate::input::viewport::column_split(s, s.page_top_line);
    let split = (cs.split <= cs.page_end).then_some(cs.split);
    placement_for_range(start, end, split, cs.page_end)
}

/// Ctrl+l: flip a floating panel to the other column. No-op when closed or
/// pinned (single-column has no "other side").
pub(crate) fn flip_panel_side(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat_placement = match s.chat_placement {
        ChatPlacement::FloatLeft => ChatPlacement::FloatRight,
        ChatPlacement::FloatRight => ChatPlacement::FloatLeft,
        ChatPlacement::Pinned => return,
    };
    size_panel(s);
    crate::logging::log(&format!("CHAT: panel flipped ({:?})", s.chat_placement));
}

/// Re-check the chat panel against the CURRENT settled geometry, converting
/// the placement to match the new work's layout: a two-column target floats
/// the panel over a column (never closes); a single-column target pins the
/// card right again, or closes with a toast when the freed space is too
/// tight.
pub(crate) fn regate_panel(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    if s.column_count() == 2 {
        s.chat_placement = float_side_for_cursor(s);
        reapply_card_margins(s); // un-pin the card if we arrived from Pinned
        size_panel(s);
        set_panel_header(s);
        crate::logging::log(&format!(
            "CHAT: regate floated panel ({:?})",
            s.chat_placement
        ));
        return;
    }
    s.chat_placement = ChatPlacement::Pinned;
    s.chat_panel.container.remove_css_class("chat-panel-float");
    reapply_card_margins(s); // pin the card right again
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        close_chat_layout(s);
        crate::input::navigation::show_chapter_toast_secs(&s, "No room for chat panel at this layout", 3);
        return;
    }
    size_panel(s);
    set_panel_header(s);
    crate::logging::log(&format!("CHAT: regate kept panel (free={}px)", free));
}

/// `Tab` from visual (`V`) mode: open the chat panel PINNED to the selection.
/// The highlighted passage becomes the source text for every question in the
/// session (see `ChatState::pinned_passage`) — the chat sends exactly what was
/// highlighted, with no neighbor segments, instead of re-deriving the cursor's
/// segment ±2 each time.
///
/// Leaves visual MODE (so keys go to the chat, not the selection) but KEEPS the
/// passage visibly marked: the selection tag is re-applied over the pinned range
/// and lives exactly as long as the pin does — `close_chat_layout` clears both.
/// So the mark always shows precisely what the chat is discussing.
/// No-op when the selection maps to no work lines.
///
/// Returns `true` only when a new pin was actually installed into
/// `s.chat.pinned_passage`. Returns `false` on both early-return paths (no
/// selection at all; selection maps to no passage) — in neither case does
/// this touch any existing pin, so callers MUST NOT infer success from
/// `chat_layout_open` alone: a panel opened by a PREVIOUS call stays open
/// (and still pinned to the OLD passage) even when this call fails.
pub(crate) fn open_chat_pinned_to_selection(state_rc: &Rc<RefCell<AppState>>) -> bool {
    let picked = {
        let s = state_rc.borrow();
        let Some(sel) = s.visual_selection.as_ref() else { return false };
        let (start, end) = sel.range();
        // Placement MUST be computed here, while the selection still exists:
        // exit_visual_mode below clears it, and toggle_chat_layout then picks a
        // side from s.current_line alone — which cannot see a spanning range.
        let placement = placement_for_selection(&s, start, end);
        crate::input::segments::selection_context(&s, start, end)
            .map(|ctx| (ctx, start, end, placement))
    };
    let Some((pinned, start, end, placement)) = picked else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No passage in the selection", 2);
        return false;
    };
    // exit_visual_mode clears the selection tag (its normal job); re-apply it
    // over the pinned range afterwards so the passage stays marked while the
    // chat discusses it.
    crate::input::visual::exit_visual_mode(&mut state_rc.borrow_mut());
    // Opens when closed, else focuses the panel — neither path touches
    // ChatState, so the pin below survives either way.
    toggle_chat_layout(state_rc);
    {
        let mut s = state_rc.borrow_mut();
        s.chat.pinned_passage = Some(pinned);
        // Re-place from the SELECTION, overriding toggle_chat_layout's
        // cursor-derived side. Only floats: a Pinned panel (single-column) has
        // no other side to choose.
        if s.chat_placement != ChatPlacement::Pinned && s.chat_placement != placement {
            s.chat_placement = placement;
            // size_panel takes &AppState (chat.rs:790), so reborrow immutably.
            size_panel(&s);
            crate::logging::log(&format!("CHAT: placed from selection ({:?})", placement));
        }
    }
    let s = state_rc.borrow();
    crate::input::visual::apply_selection_highlight_range(&s, start, end);
    crate::input::navigation::show_chapter_toast_secs(&s, "Chat pinned to selection", 2);
    true
}

pub(crate) fn toggle_chat_layout(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat_layout_open {
        // Panel already open: Tab (from reader focus) cycles INTO the panel —
        // the prompt when its input is showing, else the transcript (a
        // retired input stays hidden until `a` re-shows it); closing is
        // Ctrl+Tab's job (CloseChatLayout).
        if s.chat_panel.input_is_open() {
            focus_prompt(&mut s);
        } else {
            focus_transcript(&mut s);
        }
        return;
    }
    if s.column_count() == 2 {
        // Two-column: float over the column the cursor is NOT in. No
        // free-space gate — a 2-col card always has column-width room.
        s.chat_placement = float_side_for_cursor(&s);
        s.chat_layout_open = true;
        reapply_card_margins(&s); // chat_pinned()==false → card untouched
        size_panel(&s);
        set_panel_header(&s);
        s.chat_panel.show();
        crate::logging::log(&format!(
            "CHAT: layout opened floating ({:?})",
            s.chat_placement
        ));
        focus_prompt(&mut s);
        return;
    }
    s.chat_placement = ChatPlacement::Pinned;
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(&s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        crate::input::navigation::show_chapter_toast_secs(&s, "No room for chat panel at this layout", 3);
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
    // Tab-cycle cue: flash the widget that just became active.
    s.chat_panel.flash_input();
}

/// The honest title/hint pair for the current chat mode (revision vs ask).
fn prompt_title_hint(s: &AppState) -> (&'static str, &'static str) {
    if s.chat.revision_of.is_some() {
        ("Revise this entry", "Ctrl+Enter send \u{b7} s update \u{b7} Tab cycle")
    } else {
        ("Ask about this passage", "Ctrl+Enter send \u{b7} s save \u{b7} S consolidate \u{b7} Tab cycle")
    }
}

/// Chat layout: the transcript pane gains input focus (j/k move the exchange
/// cursor, s saves, Tab cycles to the reader, Ctrl+Tab closes).
pub(crate) fn focus_transcript(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatTranscript;
    // Tab-cycle cue: flash the widget that just became active.
    s.chat_panel.flash_transcript();
}

/// Chat layout: the reader pane gains input focus (full reader keys live;
/// the panel stays open and visible).
pub(crate) fn focus_reader(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::Reader;
    // Tab-cycle cue: flash the widget that just became active.
    use gtk4::prelude::Cast;
    crate::ui::flash_widget(s.content_hbox.clone().upcast_ref::<gtk4::Widget>());
}

/// Submit the chat prompt's current text as a new turn: builds the segment
/// context + gloss context for the cursor's passage, assembles the multi-turn
/// history from prior exchanges, and dispatches the Claude chat request.
pub(crate) fn submit_chat_prompt(state_rc: &Rc<RefCell<AppState>>) {
    // A bare `s` is the save alias, mirroring the transcript pane's `s`:
    // saving is the natural reflex right after an answer arrives, and focus
    // is still in the input — it must never go to the API as a question
    // (or, in revision mode, as a rewrite instruction). A bare `S` (or the
    // word "consolidate") merges the whole transcript into one cohesive
    // journal Q&A instead.
    let typed = state_rc.borrow().chat_panel.peek_input_text().trim().to_string();
    if typed == "s" {
        let _ = state_rc.borrow().chat_panel.take_input_text();
        if state_rc.borrow().chat.exchanges.is_empty() {
            let s = state_rc.borrow();
            crate::input::navigation::show_chapter_toast_secs(&s, "No reply to save yet", 2);
            return;
        }
        save_selected_exchange(state_rc);
        return;
    }
    if typed == "S" || typed.eq_ignore_ascii_case("consolidate") {
        let _ = state_rc.borrow().chat_panel.take_input_text();
        consolidate_chat(state_rc);
        return;
    }
    // Revision mode: the prompt text is an instruction, not a question.
    if state_rc.borrow().chat.revision_of.is_some() {
        chat_revision::submit_revision(state_rc);
        return;
    }
    let (question, system, user_msg, turns, model, chip, meta) = {
        let s = state_rc.borrow();
        if s.chat.pending {
            crate::input::navigation::show_chapter_toast_secs(&s, "Waiting for the previous reply\u{2026}", 2);
            return;
        }
        // Resolve the passage context BEFORE consuming the input text: a
        // validation failure (no work / no passage at cursor) must leave the
        // typed question untouched for retry, not silently clear it.
        let Some(work) = s.current_work.as_ref() else { return };
        // A PINNED passage (panel opened with Tab from V-mode) is the source for
        // every question in the session — send exactly what was highlighted, no
        // neighbor segments, regardless of where the cursor has since moved.
        // Otherwise fall back to the live cursor segment ±2 neighbors.
        let seg = match s.chat.pinned_passage.clone() {
            Some(pinned) => pinned,
            None => match crate::input::segments::segment_context(&s, 2) {
                Some(seg) => seg,
                None => {
                    crate::input::navigation::show_chapter_toast_secs(&s, "No passage at the cursor", 2);
                    return;
                }
            },
        };
        let Some(gctx) = crate::gloss::build_context_for_type(work, &seg.cursor_lines, "reader-gloss") else {
            crate::input::navigation::show_chapter_toast_secs(&s, "No passage at the cursor", 2);
            return;
        };
        let question = s.chat_panel.take_input_text().trim().to_string();
        if question.is_empty() {
            return;
        }
        let source_markup =
            crate::input::actions::echoes::build_source_header(&seg.cursor_lines, &gctx.speaker);
        let (genre, unit, _units) = crate::gloss::genre_unit(&work.work_type);
        let scene = crate::app::scene_synopsis::synopsis_label(&s, seg.div1, seg.div2);
        let mut unit_label = unit.to_string();
        if let Some(c) = unit_label.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        let user_msg = crate::input::segments::chat_user_message(
            genre, &work.title, &work.author, &unit_label, &scene,
            &seg.segments, seg.cursor_index, &question,
        );
        // Prior turns: capped and deduped by build_history_turns. The
        // current message is likewise sent question-only when its passage
        // matches the last history turn's; the FULL user_msg is still
        // stored on the Exchange (revision/consolidation and any future
        // context-bearing turn read from there).
        let chip: String = seg.segments[seg.cursor_index].chars().take(120).collect();
        let (mut turns, last_chip) = build_history_turns(&s.chat.exchanges);
        let wire_current = if last_chip.as_deref() == Some(chip.as_str()) {
            same_passage_question(&question)
        } else {
            user_msg.clone()
        };
        turns.push(crate::claude::ChatTurn { role: "user", content: wire_current });
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
            // Answer visible: retire the input until asked for again (`a` on
            // the transcript reopens it) and hand focus to the transcript so
            // j/k/s work immediately.
            s.chat_panel.close_input();
            focus_transcript(&mut s);
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

/// Build the transcript rows; also returns the row index of the cursor
/// exchange's question, so renders can scroll the selection into view.
fn transcript_rows(s: &AppState) -> (Vec<crate::ui::chat_panel::TranscriptRow>, usize) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    let mut cursor_row = 0;
    let mut prev_chip: Option<&str> = None;
    for (i, e) in s.chat.exchanges.iter().enumerate() {
        if prev_chip != Some(e.chip.as_str()) {
            rows.push(R::Chip(e.chip.clone()));
        }
        prev_chip = Some(e.chip.as_str());
        let marker = if i == s.chat.cursor { "\u{25b8} " } else { "" };
        if i == s.chat.cursor {
            cursor_row = rows.len();
        }
        rows.push(R::Question(format!("{}Q: {}", marker, e.question)));
        rows.push(R::Answer(e.answer.clone()));
        if e.saved_id.is_some() {
            rows.push(R::SavedMark);
        }
    }
    (rows, cursor_row)
}

pub(crate) fn render_transcript(s: &AppState) {
    let (rows, cursor_row) = transcript_rows(s);
    s.chat_panel.render_rows_focused(&rows, cursor_row);
}

/// Put a reader-gloss into transcript slot #1 — replacing the gloss already
/// there if any, so cycling and reglossing swap the gloss IN PLACE and leave
/// follow-up exchanges below untouched.
pub(crate) fn push_gloss_exchange(
    s: &mut AppState,
    ctx: &crate::gloss::GlossContext,
    gloss_text: &str,
) {
    let ex = Exchange {
        question: String::new(), // auto-gloss: the user asked nothing
        answer: gloss_text.to_string(),
        chip: gloss_chip(s),
        user_msg: String::new(),
        div1: ctx.act,
        div2: ctx.scene,
        start_citation: ctx.start_citation.clone(),
        end_citation: ctx.end_citation.clone(),
        source_markup: ctx.source_text.clone(),
        // Tracks JOURNAL saves only. The gloss is saved to `glosses`, a
        // different store, so this stays None — `s` on this exchange
        // deliberately files a second copy in the journal.
        saved_id: None,
    };
    if s.chat.exchanges.is_empty() {
        s.chat.exchanges.push(ex);
    } else {
        s.chat.exchanges[0] = ex;
    }
    s.chat.cursor = 0;
    render_transcript(s);
}

/// The "n of N" chip for the gloss slot, so cycling shows which stored gloss
/// is on screen.
fn gloss_chip(s: &AppState) -> String {
    let n = s.chat.gloss_list.len();
    if n <= 1 {
        "Reader gloss".to_string()
    } else {
        format!("Reader gloss {} of {}", s.chat.gloss_index + 1, n)
    }
}

/// Persist a reader-gloss to lit.db and refresh the panel's gloss list.
///
/// Deliberately NOT `gloss::persist_render_install_gloss`: despite its name
/// that function drives the GLOSS OVERLAY (show_gloss_with_color/set_position,
/// and it sets gloss_list/gloss_index/gloss_context/input_mode), which would
/// throw the user out of the chat panel. Only the save is wanted here.
///
/// Returns the new gloss id on success.
pub(crate) fn save_reader_gloss(
    s: &mut AppState,
    ctx: &crate::gloss::GlossContext,
    gloss_text: &str,
    model: &str,
) -> Option<i64> {
    let new_id = match crate::db::queries::open_db_rw() {
        Ok(conn) => crate::db::queries::save_gloss(
            &conn,
            &ctx.hash,
            &ctx.work_abbrev,
            &ctx.start_citation,
            &ctx.end_citation,
            ctx.act,
            ctx.scene,
            &ctx.speaker,
            &ctx.source_text,
            gloss_text,
            "reader-gloss",
            model,
        )
        .ok(),
        Err(_) => None,
    };

    // Re-read so the cycling list includes the row just written, ordered
    // newest-first (Task 1's id DESC tiebreak makes this deterministic even
    // when two saves share a one-second timestamp).
    s.chat.gloss_list = reload_gloss_list(&ctx.work_abbrev, &ctx.start_citation);
    // On a failed save (new_id is None) leave gloss_index untouched: falling
    // back to 0 would silently repoint it at whatever gloss is newest in the
    // reloaded list, which is NOT the gloss on screen when the save failed.
    if let Some(id) = new_id {
        s.chat.gloss_index = s
            .chat
            .gloss_list
            .iter()
            .position(|g| g.gloss_id == id)
            .unwrap_or(0);
    }

    // Re-derive the glossed-line tint so the passage colors IMMEDIATELY. The
    // panel STAYS OPEN, so recompute directly rather than via a
    // return-to-reader path (which would wrongly switch the input mode) —
    // same reasoning as save_selected_exchange.
    crate::app::apply_reader_gloss_highlighting(s);

    if let Some(id) = new_id {
        crate::logging::log(&format!("CHAT-GLOSS: saved reader-gloss {}", id));
    } else {
        crate::logging::log("CHAT-GLOSS: save failed");
    }
    new_id
}

/// Fire READER_GLOSS_PROMPT for a passage and install the answer: save it to
/// lit.db and put it in transcript slot #1. Shared by `-` (cache miss) and
/// `r`/`R` (regloss).
///
/// Deliberately NOT via submit_chat_prompt: that drains a typed draft from the
/// ask card and intercepts the literal strings "s"/"S" as save/consolidate
/// aliases. The ask input never opens on this path.
pub(crate) fn request_reader_gloss(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: crate::gloss::GlossContext,
    model: String,
) {
    if state_rc.borrow().chat.pending {
        return; // in flight; a second '-' or 'r' must not double-fire
    }
    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    {
        let mut s = state_rc.borrow_mut();
        s.chat.pending = true;
        render_transcript_thinking_gloss(&s);
    }

    let model_for_db = model.clone();
    let ctx_ok = ctx.clone();
    let on_success = move |sr: &Rc<RefCell<AppState>>, reply: String| {
        let mut s = sr.borrow_mut();
        s.chat.pending = false;
        let saved = save_reader_gloss(&mut s, &ctx_ok, &reply, &model_for_db);
        push_gloss_exchange(&mut s, &ctx_ok, &reply);
        focus_transcript(&mut s);
        // Still render the gloss (the user paid for it), but if it didn't
        // persist they must know — otherwise a later '-' on this passage
        // silently re-fires a paid API call because the cache is empty.
        // `s` here is the same RefMut borrowed above; show_chapter_toast_secs
        // takes &AppState so reborrow it rather than re-entering `sr`.
        if saved.is_none() {
            crate::input::navigation::show_chapter_toast_secs(&s, "Gloss not saved", 3);
        }
    };
    let on_error = move |sr: &Rc<RefCell<AppState>>, e: &str| {
        let mut s = sr.borrow_mut();
        s.chat.pending = false;
        // No gloss row is written on failure — the DB write only happens on a
        // successful reply. The panel stays open.
        render_transcript(&s);
        crate::input::navigation::show_chapter_toast_secs(&s, "Gloss failed", 3);
        crate::logging::log(&format!("CHAT-GLOSS: API error: {}", e));
    };

    // READER_GLOSS_PROMPT is a LazyLock<String> (gloss.rs:430), and
    // run_claude_request wants an owned String — deref the lock, then clone.
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        (*crate::gloss::READER_GLOSS_PROMPT).clone(),
        user_msg,
        model,
        on_success,
        on_error,
    );
}

/// `r`/`R` in the transcript: regloss the pinned passage.
///
/// Bypasses the cache check `-` makes. That check exists to avoid re-spending
/// an API call on an already-glossed span; regloss wants precisely the
/// opposite, so it always calls Claude. The result is a NEW glosses row —
/// history is kept, nothing is overwritten.
pub(crate) fn regloss_pinned(state_rc: &Rc<RefCell<AppState>>) {
    let prepared = {
        let s = state_rc.borrow();
        match &s.chat.gloss_ctx {
            Some(ctx) => Some((ctx.clone(), s.config.claude_model.clone())),
            None => None,
        }
    };
    let Some((ctx, model)) = prepared else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No passage to regloss", 2);
        return;
    };
    crate::logging::log("CHAT-GLOSS: reglossing pinned passage");
    request_reader_gloss(state_rc, ctx, model);
}

/// The transcript with a "Glossing…" row appended, so the panel shows work in
/// flight rather than sitting blank.
fn render_transcript_thinking_gloss(s: &AppState) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let (mut rows, _) = transcript_rows(s);
    rows.push(R::Chip("Reader gloss".to_string()));
    rows.push(R::Thinking);
    s.chat_panel.render_rows(&rows);
}

/// Stored reader-glosses for a passage, newest first. Empty on any DB error.
pub(crate) fn reload_gloss_list(
    work_abbrev: &str,
    start_citation: &str,
) -> Vec<crate::db::queries::SavedGloss> {
    crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::queries::find_glosses_by_start(
                &conn,
                work_abbrev,
                start_citation,
                &["reader-gloss"],
            )
            .ok()
        })
        .unwrap_or_default()
}

fn render_transcript_with_thinking(s: &AppState, question: &str, chip: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let (mut rows, _) = transcript_rows(s);
    rows.push(R::Chip(chip.to_string()));
    rows.push(R::Question(format!("Q: {}", question)));
    rows.push(R::Thinking);
    s.chat_panel.render_rows(&rows);
}

fn render_transcript_with_error(s: &AppState, msg: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let (mut rows, _) = transcript_rows(s);
    rows.push(R::Error(msg.to_string()));
    s.chat_panel.render_rows(&rows);
}

/// Move the transcript exchange cursor by `delta` and scroll the selected
/// exchange into view. When the cursor is already clamped at a boundary
/// (single exchange, or first/last), degrade to plain viewport scrolling so
/// an answer taller than the panel stays fully readable.
pub(crate) fn transcript_cursor_move(s: &mut AppState, delta: i32) {
    let n = s.chat.exchanges.len();
    if n == 0 {
        return;
    }
    let cur = s.chat.cursor as i32 + delta;
    let clamped = cur.clamp(0, n as i32 - 1) as usize;
    if clamped == s.chat.cursor {
        s.chat_panel.scroll_transcript_step(delta as f64);
        return;
    }
    s.chat.cursor = clamped;
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
        crate::input::navigation::show_chapter_toast_secs(&s, "Entry is saved", 2);
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
            // Revision mode is ARMED but not entered: `revision_of` makes a
            // later Ctrl+Enter refine this row (and the input title read
            // "Revise this entry" via prompt_title_hint) WITHOUT opening the
            // input now. `s` is a save, full stop — it must not yank focus into
            // a text field the reader did not ask for. Tab back into the panel
            // to revise. (This used to open_input + set ChatPrompt here.)
            s.chat.revision_of = Some(id);
            // If the input happens to be open ALREADY (the `s`-alias path types
            // into it, so it is), retitle it in place — it would otherwise keep
            // saying "Ask about this passage" while revision_of is set, i.e. lie
            // about what Ctrl+Enter now does. Retitle only; focus and mode are
            // untouched. Safe: the alias consumed the text, so there is no draft
            // for open_input's vim reseed to destroy.
            if s.chat_panel.input_is_open() && s.chat_panel.peek_input_text().trim().is_empty() {
                let (title, hint) = prompt_title_hint(&s);
                s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg);
            }
            render_saved_entry(&s, &q, &a);
            // Re-derive the glossed-line tint so the just-saved passage colors
            // IMMEDIATELY, mirroring every gloss.rs save/edit/delete path.
            // Without this the entry existed but its passage stayed unmarked
            // until some other path recomputed — opening the journal overlay on
            // it and escaping out was the only way to see it (the overlay's
            // close path runs the same recompute). The chat panel STAYS OPEN
            // here, so recompute directly rather than via a return-to-reader
            // path (which would wrongly switch the input mode).
            crate::app::apply_reader_gloss_highlighting(&mut s);
            crate::input::navigation::show_chapter_toast_secs(&s, "Saved", 2);
            crate::logging::log(&format!("CHAT: saved exchange as journal page {}", id));
        }
        Err(err) => {
            crate::input::navigation::show_chapter_toast_secs(&s, "Save failed", 3);
            crate::logging::log(&format!("CHAT: save failed: {}", err));
        }
    }
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
fn consolidate_transcript(exchanges: &[Exchange]) -> String {
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

pub(crate) fn consolidate_chat(state_rc: &Rc<RefCell<AppState>>) {
    let (system, user_msg, model, fallback_q, meta) = {
        let s = state_rc.borrow();
        if s.chat.pending {
            crate::input::navigation::show_chapter_toast_secs(&s, "Waiting for the previous reply\u{2026}", 2);
            return;
        }
        if s.chat.revision_of.is_some() {
            crate::input::navigation::show_chapter_toast_secs(&s, "Entry is saved \u{2014} Ctrl+Enter revises it", 2);
            return;
        }
        if s.chat.exchanges.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, "No conversation to consolidate yet", 2);
            return;
        }
        let Some(work) = s.current_work.as_ref() else { return };
        let first = &s.chat.exchanges[0];
        let scene = crate::app::scene_synopsis::synopsis_label(&s, first.div1, first.div2);
        let transcript = consolidate_transcript(&s.chat.exchanges);
        let user_msg = format!(
            "Work: {} by {}\nThis conversation is filed under a PASSAGE in {}\n\nPassage:\n{}\n\nConversation:\n{}Consolidate this conversation into a single cohesive journal Q&A: one question capturing what the conversation was really asking, one answer synthesizing its insights (drop dead ends, false starts, and meta-chatter). Return the consolidated Q&A in exactly this format:\nQ: <question>\nA: <answer>",
            work.title, work.author, scene, first.source_markup, transcript,
        );
        let meta = (
            first.div1,
            first.div2,
            first.start_citation.clone(),
            first.end_citation.clone(),
            first.source_markup.clone(),
            first.chip.clone(),
        );
        (
            crate::gloss::journal_qa_prompt(&work.work_type),
            user_msg,
            s.config.claude_model.clone(),
            first.question.clone(),
            meta,
        )
    };
    {
        let mut s = state_rc.borrow_mut();
        s.chat.pending = true;
        let (mut rows, _) = transcript_rows(&s);
        rows.push(crate::ui::chat_panel::TranscriptRow::Thinking);
        s.chat_panel.render_rows(&rows);
        crate::input::navigation::show_persistent_chapter_toast(&s, "Consolidating\u{2026}");
    }
    let user_msg_for_exchange = user_msg.clone();
    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system,
        user_msg,
        model,
        move |st, reply| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            let (q, a) = parse_revised_qa(&reply, &fallback_q);
            let (div1, div2, start_citation, end_citation, source_markup, chip) = meta.clone();
            let (abbrev, model_for_db) = {
                let Some(work) = s.current_work.as_ref() else { return };
                (work.canonical_abbrev.clone(), s.config.claude_model.clone())
            };
            let saved = crate::db::queries::open_db_rw().and_then(|conn| {
                crate::db::journal::save_passage_page(
                    &conn, &abbrev, div1, div2,
                    &start_citation, &end_citation, &source_markup,
                    &q, &a, &model_for_db,
                )
            });
            match saved {
                Ok(id) => {
                    let merged = s.chat.exchanges.len();
                    s.chat.exchanges.push(Exchange {
                        question: q.clone(),
                        answer: a.clone(),
                        chip,
                        user_msg: user_msg_for_exchange.clone(),
                        div1,
                        div2,
                        start_citation,
                        end_citation,
                        source_markup,
                        saved_id: Some(id),
                    });
                    s.chat.cursor = s.chat.exchanges.len() - 1;
                    s.chat.revision_of = Some(id);
                    render_saved_entry(&s, &q, &a);
                    let (title, hint) = prompt_title_hint(&s);
                    s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg);
                    s.input_mode = crate::app::InputMode::ChatPrompt;
                    // Same refresh as the `s` save path: the consolidated entry
                    // is a new journal page, so re-derive the glossed-line tint
                    // now or its passage stays unmarked until some other path
                    // recomputes.
                    crate::app::apply_reader_gloss_highlighting(&mut s);
                    crate::input::navigation::show_chapter_toast_secs(&s, "Consolidated and saved", 2);
                    crate::logging::log(&format!(
                        "CHAT: consolidated {} exchanges into journal page {}",
                        merged, id
                    ));
                }
                Err(err) => {
                    render_transcript(&s);
                    crate::input::navigation::show_chapter_toast_secs(&s, "Save failed", 3);
                    crate::logging::log(&format!("CHAT: consolidation save failed: {}", err));
                }
            }
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            render_transcript_with_error(&s, msg);
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 4);
        },
    );
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

/// Size and position the panel for the current placement. Pinned: fill the
/// freed left space at the card's height. Float: cover the chosen reading
/// column exactly (live compute_bounds rect, window coords — the overlay
/// child's margin_start is relative to the window-filling outer overlay).
pub(crate) fn size_panel(s: &AppState) {
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    match s.chat_placement {
        ChatPlacement::Pinned => {
            let ww = s.window.width().max(0);
            let end = crate::app::layout::CARD_OUTER_MARGIN;
            // left outer margin (24) + gap to the card (16)
            let w = ww - card_w - end - 24 - 16;
            s.chat_panel.container.set_margin_start(24);
            s.chat_panel.container.remove_css_class("chat-panel-float");
            s.chat_panel.size_to(w, card_h);
        }
        ChatPlacement::FloatLeft | ChatPlacement::FloatRight => {
            let col = if s.chat_placement == ChatPlacement::FloatLeft {
                &s.scrolled_overlay
            } else {
                &s.right_scrolled_overlay
            };
            let (mut x, mut w) = col
                .compute_bounds(&s.window)
                .map(|b| (b.x() as i32, b.width() as i32))
                .unwrap_or((24, crate::app::MIN_TWO_COLUMN_COLUMN_WIDTH));
            // The column stops 8px short of the divider line (the divider's
            // CSS margin); extend the panel to the divider so its border
            // sits exactly on top of it instead of leaving a sliver of card.
            if let Some(d) = s.column_divider.compute_bounds(&s.window) {
                let (d_left, d_right) = (d.x() as i32, (d.x() + d.width()) as i32);
                if s.chat_placement == ChatPlacement::FloatLeft {
                    w = w.max(d_right - x);
                } else {
                    let new_x = d_left.min(x);
                    w += x - new_x;
                    x = new_x;
                }
            }
            s.chat_panel.container.set_margin_start(x.max(0));
            s.chat_panel.container.add_css_class("chat-panel-float");
            s.chat_panel.size_to(w, card_h);
        }
    }
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
    let scene = crate::app::scene_synopsis::synopsis_label(s, d1, d2);
    s.chat_panel.set_header(&scene);
}

/// How many most-recent exchanges are sent as conversation history (each is
/// two turns). Older exchanges age out of the wire request — `S` consolidate
/// is the archival path for anything the model still needs to remember.
const CHAT_HISTORY_TURNS: usize = 6;

/// Question-only wire form for a turn whose passage context is already
/// present verbatim in an earlier turn of the same request.
fn same_passage_question(q: &str) -> String {
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
fn build_history_turns(
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
                crate::input::navigation::show_chapter_toast_secs(&s, "Type a revision instruction", 2);
                return;
            }
            let Some(e) = s.chat.exchanges.iter().find(|e| e.saved_id == Some(id)) else {
                return;
            };
            let Some(work) = s.current_work.as_ref() else { return };
            let scene = crate::app::scene_synopsis::synopsis_label(&s, e.div1, e.div2);
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
            crate::input::navigation::show_persistent_chapter_toast(&s, "Rewriting Q & A\u{2026}");
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
                crate::input::navigation::show_chapter_toast_secs(&s, "Rewritten", 2);
            },
            move |st, msg| {
                let s = st.borrow_mut();
                crate::input::navigation::show_chapter_toast_secs(&s, msg, 4);
                // Restore the failed instruction for retry, mirroring
                // submit_chat_prompt's error path.
                s.chat_panel.paste_input_text(&instruction_err);
            },
        );
    }
}

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn line_in_right_column_respects_split_and_end() {
        assert!(!line_in_right_column(5, None, 40)); // no right column
        assert!(!line_in_right_column(5, Some(20), 40)); // left side
        assert!(line_in_right_column(20, Some(20), 40)); // first right line
        assert!(line_in_right_column(40, Some(20), 40)); // last line
        assert!(!line_in_right_column(41, Some(20), 40)); // off-page
    }

    // A page whose left column is lines 0..=9 and right column 10..=19:
    // split = Some(10), page_end = 19.
    const SPLIT: Option<usize> = Some(10);
    const PAGE_END: usize = 19;

    #[test]
    fn selection_wholly_in_left_column_floats_right() {
        assert_eq!(placement_for_range(2, 5, SPLIT, PAGE_END), ChatPlacement::FloatRight);
    }

    #[test]
    fn selection_wholly_in_right_column_floats_left() {
        assert_eq!(placement_for_range(12, 15, SPLIT, PAGE_END), ChatPlacement::FloatLeft);
    }

    /// The whole point: neither side keeps a spanning passage visible, so pick
    /// LEFT by rule rather than by whichever end the cursor sat on.
    #[test]
    fn selection_spanning_both_columns_floats_left() {
        assert_eq!(placement_for_range(5, 15, SPLIT, PAGE_END), ChatPlacement::FloatLeft);
    }

    #[test]
    fn single_line_selection_uses_its_own_column() {
        assert_eq!(placement_for_range(3, 3, SPLIT, PAGE_END), ChatPlacement::FloatRight);
        assert_eq!(placement_for_range(14, 14, SPLIT, PAGE_END), ChatPlacement::FloatLeft);
    }

    /// A single-column page has no right column; every selection floats right.
    #[test]
    fn no_right_column_floats_right() {
        assert_eq!(placement_for_range(2, 8, None, PAGE_END), ChatPlacement::FloatRight);
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

//! Chat layout (Tab): left chat panel + right-pinned card. This task ships
//! the layout toggle only; the panel widget and conversation land in later
//! tasks of the chat-layout plan.

use crate::app::AppState;
use gtk4::prelude::WidgetExt;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum freed left space (px) required to open the chat layout.
const CHAT_MIN_PANEL_W: i32 = 500;

/// Width (px) of the hairline seam between the card and a Pinned panel — the
/// panel starts this far past the card's right edge, and its `border-left`
/// (`.chat-panel-pinned`) paints the crisp 1px line.
const PINNED_DIVIDER_W: i32 = 1;

/// Distance from the panel container's top edge to its FIRST transcript line,
/// in px. It is the sum of the `.chat-panel` outer padding (12px) and the
/// `.chat-transcript` `padding-top` (16px) — both literals in `theme.rs`'s
/// stylesheet. `size_panel`'s Pinned branch backs this out of the panel's top
/// margin so the first line (not the container edge) aligns with the main
/// card's first reading line. Keep in sync with those two CSS values.
/// Distance from the panel container's top edge to its first transcript
/// label's INK, in px. The `.chat-panel` and `.chat-transcript` top paddings
/// are both 0 (see their notes in `theme.rs` — deliberately zeroed so the
/// panel's top chrome doesn't push the first line down into the header band),
/// so the only remaining inset is the label's own intrinsic top leading: the
/// gap Pango leaves above the glyphs of a label's first line, present on the
/// chat label but already absorbed into the reading view's measured ink
/// position. Measured empirically against the reading column's first-line ink
/// at the current font size. If the two CSS top paddings are ever restored,
/// add them back here.
const CHAT_PANEL_TOP_INSET: i32 = 19;

/// The panel container's `margin_top` (from the window top) that lands the
/// transcript's FIRST line level with the main card's first reading-line ink.
///
/// The card's first line sits `CARD_VERTICAL_OUTER_MARGIN` (content_hbox's top
/// outer margin) + `TOP_SPACER_HEIGHT` (the running-head band above the
/// columns) + `line_spacing` (`pixels_above_lines`, applied above every line
/// including the first) below the window top. The panel's first label ink sits
/// `CHAT_PANEL_TOP_INSET` below the panel container's top, so back that out to
/// align the LINES, not the container edges. Shared by both `size_panel`
/// placements (Pinned and Float) so their first-line alignment can't drift.
/// The real transcript font — `(config.font_family, config.font_size)` — the
/// same source `theme::generate_css` builds the `.chat-transcript` rule from
/// (app/mod.rs). Threaded into the panel's measure/paginate path so pagination
/// measures at the size the labels actually render, not the pango context's
/// stale font description.
fn transcript_font(s: &AppState) -> (String, i32) {
    (s.config.font_family.clone(), s.config.font_size as i32)
}

fn chat_first_line_top_margin(line_spacing: i32) -> i32 {
    (crate::app::layout::CARD_VERTICAL_OUTER_MARGIN
        + crate::app::TOP_SPACER_HEIGHT
        + line_spacing
        - CHAT_PANEL_TOP_INSET)
        .max(0)
}

/// Where the open chat panel sits. Pinned = single-column layout (card pinned
/// right, panel in the freed left space). Float* = two-column layout (panel
/// overlays one reading column; the card is untouched).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChatPlacement {
    Pinned,
    FloatLeft,
    FloatRight,
}

/// Which content the transcript pane is showing: the session's live
/// gloss/Q&A exchanges (`ChatState.exchanges`), the pinned passage's SAVED
/// journal entries (`ChatState.journal_list` — a read-only view over a
/// different lit.db table, `journal_entries` `scope='passage'`), or a single
/// just-answered follow-up (`Question` — see its own doc comment). All three
/// are toggled/reached via `t` (`flip_view`). Default is `Gloss` — the
/// panel's existing, unchanged behavior.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum PanelView {
    #[default]
    Gloss,
    Journal,
    /// Showing exactly ONE Q&A — the follow-up that was just submitted/
    /// answered (`s.chat.exchanges[s.chat.cursor]`) — instead of the whole
    /// transcript. Entered by `submit_chat_prompt` the instant a question is
    /// sent (so "thinking…" shows only that question) and kept through the
    /// answer's arrival (so the answer appears alone, with no gloss or
    /// earlier exchanges above it). `t` from here goes to `Gloss` (see
    /// `flip_view`) — this is the non-gloss side of the toggle, same as
    /// `Journal`. Transient display only: the answer's persistent record is
    /// the journal row (`persist_exchange_to_journal`/`s`), unaffected by
    /// this view.
    Question,
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
    /// j/k row cursor: index into the RENDERED transcript rows (one per
    /// `chat_panel::TranscriptRow` — a gloss answer explodes into several,
    /// speaker/verse/gloss/etc.), not into `exchanges`. This is what the
    /// accent bar (`.chat-cursor-row`) paints on and what j/k actually steps.
    /// `cursor` (the EXCHANGE cursor, used by `s` save and Ctrl+n/p gloss
    /// cycling) is derived from it via `build_transcript_rows`'
    /// `row_owner` map — see `transcript_cursor_move`. Reset to the new
    /// exchange's leading row alongside every `cursor` write (new answer,
    /// gloss push, consolidate) — see `snap_row_cursor_to_exchange`.
    pub row_cursor: usize,
    pub revision_of: Option<i64>,
    pub pending: bool,
    /// Passage PINNED by opening the panel with `Tab` from visual (`V`) mode:
    /// the reader's selection, verbatim, as a one-segment context. While set,
    /// EVERY question in the session sends exactly this passage as the source
    /// text instead of re-deriving the cursor's segment ±2 neighbors — so
    /// follow-ups keep discussing the same passage even if the cursor drifts.
    /// Cleared with the rest of ChatState when the panel closes.
    ///
    /// INVARIANT: `gloss_ctx` must always describe the SAME passage as
    /// `pinned_passage` (or be `None` if `-` hasn't glossed it yet). Any path
    /// that rewrites one must clear or set the other in the same operation —
    /// see `open_chat_pinned_to_selection`, which clears `gloss_ctx`/
    /// `gloss_list`/`gloss_index` whenever it installs a new `pinned_passage`,
    /// and the `-` path in `visual.rs`, which re-populates all three
    /// immediately after a successful pin.
    pub pinned_passage: Option<crate::input::segments::SegmentContext>,
    /// Stored reader-glosses for the pinned passage, newest first, as
    /// `find_glosses_by_start` orders them. A DIFFERENT axis from `exchanges`:
    /// these are lit.db rows (including earlier sessions'), where `exchanges`
    /// is this session's in-memory transcript. `Ctrl+n`/`Ctrl+p` moves over
    /// this list; `j`/`k` moves over `exchanges`. Never share `cursor`. Must
    /// track `pinned_passage` — see the invariant note there.
    pub gloss_list: Vec<crate::db::queries::SavedGloss>,
    /// Index into `gloss_list` of the gloss currently shown in exchange #1.
    pub gloss_index: usize,
    /// The pinned passage as a gloss context — what regloss re-sends and what
    /// a save needs for the `passages` row. Set when `-` opens the panel.
    /// Must track `pinned_passage` — see the invariant note there.
    pub gloss_ctx: Option<crate::gloss::GlossContext>,
    /// `V` visual-selection anchor, in the SAME widget-row space as
    /// `row_cursor` (see its doc comment) — NOT the reader's
    /// `AppState.visual_selection`, which is a buffer-line selection over a
    /// GtkTextBuffer the panel doesn't have. `Some(anchor)` while active;
    /// `j`/`k` then move `row_cursor` as the live end while `anchor` holds
    /// still, and `(anchor, row_cursor)` (either order) is the selected
    /// range. `None` outside visual mode. Resets with the rest of `ChatState`
    /// on panel close / work switch (`#[derive(Default)]` above).
    pub visual_anchor: Option<usize>,
    /// Which content the transcript pane currently shows — `t` flips this.
    /// Must track `pinned_passage`/`gloss_ctx` — a fresh pin (`-` or a new
    /// `Tab` selection) resets it to `Gloss` in `open_chat_pinned_to_selection`,
    /// same reset point as `gloss_ctx`/`gloss_list`, for the same reason: a
    /// `t` toggle answers "is THIS passage's journal shown", and a new pin is
    /// a different passage.
    pub view: PanelView,
    /// The pinned passage's saved journal entries (`scope='passage'`, exact
    /// citation match — see `db::journal::find_passage_pages`), in the DB
    /// query's own order (`timestamp ASC, id ASC`). Loaded lazily the first
    /// time `t` switches TO `Journal` view (see `toggle_panel_view`) and
    /// re-loaded on every subsequent switch so a `s` save made while viewing
    /// Journal is reflected immediately on the next toggle back. Empty (not
    /// None) when there are none — `render_journal_view` shows a placeholder
    /// row in that case, so "not loaded yet" and "loaded, zero rows" are
    /// never conflated (the load always runs before this is read for
    /// render). Cleared alongside `view` at the same reset point.
    pub journal_list: Vec<crate::db::journal::JournalPage>,
    /// Row cursor for `PanelView::Journal`: index into `journal_list`. `j`/`k`
    /// step it, the accent bar (`.chat-cursor-row`) paints on the cursor
    /// entry's `Q:` widget row, and `R` rewrites this entry. Reset to 0 (top)
    /// on every toggle into Journal view — matches the "land at the top of the
    /// entry" behavior. A separate axis from `row_cursor` (Gloss view) and
    /// `cursor` (exchanges); never shared. Clamped to `journal_list` on every
    /// render.
    pub journal_cursor: usize,
    /// Widget index -> journal entry index for `PanelView::Journal`, the exact
    /// analogue of the Gloss view's `build_transcript_rows` `row_owner`. Each
    /// entry now emits several widgets (a `Q:` row plus one `Answer` row per
    /// answer paragraph — see `journal_view_rows`/`split_answer_paragraphs`), so
    /// the accent bar steps `row_cursor` over the journal `landable_mask` and
    /// `journal_cursor` (the ENTRY that `R`/save act on) is derived as
    /// `journal_row_owner[row_cursor]`. Rebuilt on every `render_journal_view`;
    /// empty when the list renders only the "no entries" placeholder (nothing
    /// landable). Resets with the rest of `ChatState` on panel close.
    pub journal_row_owner: Vec<usize>,
    /// Set by `rewrite_journal_entry` while a panel-initiated `R` rewrite is in
    /// flight through the shared journal rewrite pipeline (which otherwise
    /// returns to the journal OVERLAY). The overlay-render / mode-restore sites
    /// in `journal.rs` (`rewrite_with_claude`'s success + error closures,
    /// `close_rewrite_target`, the panel instruction-card submit) guard on this
    /// to re-render the CHAT PANEL and restore `ChatTranscript` instead. Always
    /// cleared on the terminal outcome (success re-render, error, or cancel);
    /// defaults `false` and resets with the rest of `ChatState` on panel close.
    pub rewrite_return: bool,
    /// Page ranges for the CURRENT transcript render (widget-block indices into
    /// `row_widget_specs`). Recomputed on every paginated render
    /// (`render_transcript`/`render_journal_view_inner`) at the live transcript
    /// budget. Empty until the first paginated render. Task 6 (page-turn nav)
    /// reads these; the render here uses them to slice the visible page.
    pub pages: Vec<crate::ui::pagination::Page>,
    /// Which page of `pages` is currently shown. Kept in sync with `row_cursor`:
    /// each paginated render derives the page holding `row_cursor` (via
    /// `page_of_widget`) and stores it here. Clamped into `pages` on every
    /// render. Resets with the rest of `ChatState` on panel close.
    pub page_idx: usize,
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
    // Discard any pending R→a/b rewrite stash so a later ask isn't mistaken
    // for a rewrite (mirrors journal::close_prompt).
    s.journal.vim_rewrite = None;
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows(&[], &fam, sz);
    s.chat_layout_open = false;
    s.chat_placement = ChatPlacement::Pinned;
    s.chat_panel.container.remove_css_class("chat-panel-float");
    s.chat_panel.container.remove_css_class("chat-panel-pinned");
    s.page_turn_overlay.remove_css_class("card-chat-seam");
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
    // Discard any pending R→a/b rewrite stash so a later ask isn't mistaken
    // for a rewrite (mirrors journal::close_prompt).
    s.journal.vim_rewrite = None;
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows(&[], &fam, sz);
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
        crate::logging::log(&format!(
            "CHAT: regate floated panel ({:?})",
            s.chat_placement
        ));
        return;
    }
    s.chat_placement = ChatPlacement::Pinned;
    s.chat_panel.container.remove_css_class("chat-panel-float");
    reapply_card_margins(s); // pin the card left again (panel abuts on the right)
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        close_chat_layout(s);
        crate::input::navigation::show_chapter_toast_secs(&s, "No room for chat panel at this layout", 3);
        return;
    }
    size_panel(s);
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
/// Returns `true` only when the panel is OPEN and a new pin was actually
/// installed into `s.chat.pinned_passage`. Returns `false` on three failure
/// paths: no selection at all; selection maps to no passage; or
/// `toggle_chat_layout` itself failed to open the panel (single-column
/// layout with no room — it toasts "No room for chat panel at this layout"
/// and returns without setting `chat_layout_open = true`). In NONE of these
/// cases does this touch any existing pin, so callers MUST NOT infer success
/// from `chat_layout_open` alone before calling this: a panel opened by a
/// PREVIOUS call stays open (and still pinned to the OLD passage) even when
/// THIS call fails to pin a new one.
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
    let Some((pinned, _start, _end, placement)) = picked else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No passage in the selection", 2);
        return false;
    };
    // exit_visual_mode clears the selection tag (its normal job). The source
    // passage is deliberately left UNMARKED while the chat discusses it — the
    // panel appearing beside the passage is cue enough, and the tint over the
    // source lines read as clutter.
    crate::input::visual::exit_visual_mode(&mut state_rc.borrow_mut());
    // Opens when closed, else focuses the panel — neither path touches
    // ChatState, so the pin below survives either way. BUT on a single-column
    // layout with no free space, toggle_chat_layout bails WITHOUT setting
    // chat_layout_open = true (it already toasted why) — check that outcome
    // before installing a pin into a panel that isn't actually on screen.
    toggle_chat_layout(state_rc);
    {
        let s = state_rc.borrow();
        if !s.chat_layout_open {
            return false;
        }
    }
    {
        let mut s = state_rc.borrow_mut();
        // A fresh pin describes a NEW passage: clear any stale gloss context
        // from a previously pinned passage so `r`/`R`/Ctrl+n/Ctrl+p can't act
        // on it while the panel shows this one. The '-' path (visual.rs)
        // re-populates all three immediately after this call returns true;
        // Tab (keymap.rs) leaves them cleared, which is correct — Tab never
        // glosses.
        s.chat.gloss_ctx = None;
        s.chat.gloss_list.clear();
        s.chat.gloss_index = 0;
        // A new pin describes a NEW passage: any journal view/list from the
        // OLD passage must not survive to be shown (or silently reused) for
        // this one — same reasoning as the gloss_ctx/gloss_list reset above.
        s.chat.view = PanelView::Gloss;
        s.chat.journal_list.clear();
        // The TRANSCRIPT is passage-scoped too. On a FRESH open toggle_chat_layout
        // ran against a Default ChatState, but re-pinning while the panel is
        // already open (V-select A, gloss, Escape, V-select B, `-`) reaches here
        // with A's exchanges/cursor still live — and push_gloss_exchange only
        // overwrites exchanges[0], so A's follow-up Q&As would strand below B's
        // gloss. Clear them: the '-' path re-fills exchanges[0] immediately;
        // Tab correctly starts B's conversation empty.
        s.chat.exchanges.clear();
        s.chat.cursor = 0;
        s.chat.row_cursor = 0;
        s.chat.revision_of = None;
        s.chat.visual_anchor = None;
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
    // No "pinned" toast: the panel appearing beside the passage already says
    // it. Failure paths above DO toast — that is the case the reader can't see
    // for themselves. The source passage is intentionally NOT re-highlighted.
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
    focus_prompt_in_mode(s, false);
}

/// `focus_prompt`, but the ask input starts in vim INSERT — the caret is ready
/// to type with no `i`/`a` press first. For the transcript's `a`, whose gesture
/// already means "ask now"; the panel-open paths keep NORMAL via `focus_prompt`.
pub(crate) fn focus_prompt_insert(s: &mut AppState) {
    focus_prompt_in_mode(s, true);
}

fn focus_prompt_in_mode(s: &mut AppState, insert: bool) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    let (title, hint) = prompt_title_hint(s);
    if !s.chat_panel.input_is_open() {
        s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg, insert);
    } else if s.chat_panel.peek_input_text().trim().is_empty() {
        // No draft to lose: re-titling via open_input is safe (it also
        // reseeds the vim engine, but there's nothing in it to destroy).
        s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg, insert);
    }
    // Tab-cycle cue: flash the widget that just became active.
    s.chat_panel.flash_input();
}

/// The honest title/hint pair for the current chat mode (revision vs ask).
fn prompt_title_hint(s: &AppState) -> (&'static str, &'static str) {
    if s.chat.revision_of.is_some() {
        ("Revise this entry", "Ctrl+Enter revise (empty = rewrite afresh) \u{b7} Tab cycle")
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
    // Passage-derived context is resolved at SUBMIT time (the cursor may move
    // during the two async round-trips below); only the pieces that embed the
    // QUESTION (user_msg, wire turns) are (re)built later, inside
    // improve_question's callback, using the rewritten question.
    let (question, system, model, chip, meta, msg_ctx) = {
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
        // Derive genre/unit_label/scene_label EXACTLY as `ask_claude` does, so
        // the shared `build_qa_answer_message` produces the journal Passage
        // band's message byte-for-byte (titlecase_first, not make_ascii_uppercase;
        // scene_label(div1,div2), not synopsis_label — the two surfaces stay in
        // sync by construction).
        let unit_label = crate::input::actions::journal::titlecase_first(unit);
        let scene_label = crate::app::scene_synopsis::scene_label(seg.div1, seg.div2);
        // The full scene text is the SAME window the journal Passage band sends
        // (anchored on the journal's saved reader position via journal_band).
        let scene_text = crate::input::actions::journal::current_scene_text(&s);
        let chip: String = seg.segments[seg.cursor_index].chars().take(120).collect();
        // Everything build_qa_answer_message needs to (re)build the user message
        // once the question has been rewritten. Captured by value so the answer
        // call — issued inside improve_question's callback — can build it there
        // without re-borrowing the (possibly moved) cursor context.
        let msg_ctx = ChatMsgCtx {
            genre: genre.to_string(),
            title: work.title.clone(),
            author: work.author.clone(),
            unit_label,
            scene_label,
            scene_text,
        };
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
            s.config.claude_model.clone(),
            chip,
            meta,
            msg_ctx,
        )
    };

    {
        let mut s = state_rc.borrow_mut();
        s.chat.pending = true;
        // A submitted question always answers into `exchanges`, shown
        // FOCUSED (this one Q&A only, not the whole transcript) — switch to
        // Question so the "thinking…" row, and then the answer, are visible
        // alone even if the reader asked this while looking at the Gloss or
        // Journal view. `t` from here returns to Gloss (see `flip_view`).
        s.chat.view = PanelView::Question;
        // Show the reader's ORIGINAL question in the "thinking…" row while BOTH
        // Claude calls (rewrite, then answer) run — the panel is never blank
        // through the wait. The FINAL Q: row (rendered with the answer) shows
        // the IMPROVED question, matching what was actually answered.
        render_transcript_with_thinking(&s, &question, &chip);
        // Hide the ask input the INSTANT the question is submitted, not when
        // the answer arrives — the input stays gone through the whole
        // "thinking…" wait. `a` on the transcript reopens it, same as before.
        s.chat_panel.close_input();
        focus_transcript(&mut s);
    }

    // Match the journal Q&A overlay: rewrite the reader's question via Claude
    // (system prompt journal.improve-question) BEFORE answering it, so a terse
    // or malformed ask is sharpened first. No scene-terms extraction here (the
    // overlay's first call) — improve_question is called with an EMPTY terms
    // slice, so the rewrite proceeds with no term-preservation guidance. The
    // ANSWER request is issued from INSIDE this callback: pending stays true
    // across BOTH round-trips, and improve_question ALWAYS calls on_done
    // (rewritten text on success, ORIGINAL on empty/error — see journal.rs), so
    // the answer is always reached and pending always clears.
    let state_for_answer = Rc::clone(state_rc);
    crate::input::actions::journal::improve_question(
        state_rc,
        question,
        &[],
        move |st, improved| {
        let (div1, div2, start_citation, end_citation, source_markup) = meta.clone();
        // Build the user message + wire turns with the IMPROVED question. The
        // chat panel sends the journal's PASSAGE-band message — full scene text
        // + the exact passage + question — via the ONE shared builder, so the
        // two surfaces stay byte-identical by construction. The `.._citation`
        // strings are the passage's own citations (the same div/citations the
        // journal Passage band would use); the builder ignores div/start/end and
        // keys only on the variant, but we thread them through faithfully.
        let band = crate::app::JournalBand::Passage {
            div1,
            div2,
            start: start_citation.clone(),
            end: end_citation.clone(),
        };
        let (user_msg, turns) = {
            let s = st.borrow();
            let user_msg = crate::input::actions::journal::build_qa_answer_message(
                &band,
                &msg_ctx.genre,
                &msg_ctx.title,
                &msg_ctx.author,
                &msg_ctx.unit_label,
                &msg_ctx.scene_label,
                &msg_ctx.scene_text,
                &source_markup,
                &improved,
            );
            // Prior turns: capped and deduped by build_history_turns. The
            // current message is likewise sent question-only when its passage
            // matches the last history turn's; the FULL user_msg is still
            // stored on the Exchange (revision/consolidation and any future
            // context-bearing turn read from there).
            let (mut turns, last_chip) = build_history_turns(&s.chat.exchanges);
            let wire_current = if last_chip.as_deref() == Some(chip.as_str()) {
                same_passage_question(&improved)
            } else {
                user_msg.clone()
            };
            turns.push(crate::claude::ChatTurn { role: "user", content: wire_current });
            (user_msg, turns)
        };

        let question_ok = improved.clone();
        let question_err = improved;
        let chip = chip.clone();
        crate::input::actions::claude_bridge::run_claude_chat_request(
        &state_for_answer,
        system.clone(),
        turns,
        model.clone(),
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
            snap_row_cursor_to_exchange(&mut s);
            // Stay in Question view (set at submit) and render ONLY this
            // Q&A — not the whole transcript (render_transcript), which
            // would bring the gloss and any earlier exchanges back above it.
            // `t` still reaches the full gloss transcript from here.
            debug_assert_eq!(s.chat.view, PanelView::Question);
            render_current_question(&mut s);
            // Answer visible: hand focus to the transcript so j/k/s work
            // immediately. The input was already hidden on submit.
            focus_transcript(&mut s);
            // Auto-save the FIRST follow-up Q&A on this passage so the
            // reader doesn't have to press `s`. Any further follow-up in the
            // same panel session (count > 1) still requires `s` — this fires
            // exactly once per session. Deliberately does NOT arm
            // `revision_of`: the reader didn't press `s`, they may just keep
            // asking, and arming it would silently retitle a later `a` as
            // "Revise this entry" — surprising when nothing was manually
            // saved yet. `s` still works afterward and arms revision then,
            // same as it always has for a not-yet-revision-armed save.
            if is_first_question_exchange(&s.chat.exchanges) {
                let idx = s.chat.exchanges.len() - 1;
                match persist_exchange_to_journal(&mut s, idx) {
                    Some(_id) => {
                        crate::input::navigation::show_chapter_toast_secs(
                            &s, "Saved to journal", 2,
                        );
                    }
                    None => {
                        crate::input::navigation::show_chapter_toast_secs(
                            &s, "Not saved", 3,
                        );
                    }
                }
            }
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            // No exchange was ever pushed on a failed request, so there is no
            // single Q&A for Question view to focus on — render_transcript_
            // with_error shows the FULL transcript (gloss + prior exchanges)
            // plus the error, so line up `view` with what's actually on
            // screen (same "match view to the render" rule
            // save_selected_exchange follows). Otherwise `view` would say
            // Question while Gloss-shaped content is on screen, and `t`
            // would silently no-op (flip_view's Question arm) instead of
            // reaching Journal.
            s.chat.view = PanelView::Gloss;
            render_transcript_with_error(&s, msg);
            // The input was hidden on submit (close_input, above) — its vim
            // engine is gone, so paste_input_text alone would silently no-op
            // (paste_text requires a live engine). Reopen it for retry before
            // restoring the failed question.
            focus_prompt(&mut s);
            s.chat_panel.paste_input_text(&question_err);
        },
    );
    }, // end improve_question on_done (issues the answer request above)
    );
}

/// Context captured at submit time to (re)build the chat user message once the
/// question has been rewritten by `improve_question`. Everything the shared
/// `build_qa_answer_message` (Passage band) needs EXCEPT the question and the
/// passage source (the latter comes from `meta`) — resolved from the cursor's
/// passage at submit, so a cursor move during the two async round-trips can't
/// change the passage that was asked about.
struct ChatMsgCtx {
    genre: String,
    title: String,
    author: String,
    unit_label: String,
    scene_label: String,
    scene_text: String,
}

/// Wrap an exchange's answer as the right `TranscriptRow` variant: a
/// reader-gloss exchange (`push_gloss_exchange` always stores an empty
/// `question` — "the user asked nothing", see its doc comment) carries RAW
/// `<speaker>`/`<verse>`/`<gloss>` markup and must render through
/// `GlossAnswer` (typed rows, styled) rather than `Answer` (one plain label,
/// which would show the literal tags).
fn answer_row(e: &Exchange) -> crate::ui::chat_panel::TranscriptRow {
    use crate::ui::chat_panel::TranscriptRow as R;
    if e.question.is_empty() {
        R::GlossAnswer(e.answer.clone())
    } else {
        R::Answer(e.answer.clone())
    }
}

/// A `Question` row with the `Q: ` display label. Routes through
/// `journal_overlay::prefix_question` — the single source of that label — so
/// the prefix cannot drift across the render paths, and so a question already
/// beginning `Q:` is not double-prefixed (the raw `format!("Q: {}", …)` these
/// call sites used to inline had no such guard).
fn question_row(question: &str) -> crate::ui::chat_panel::TranscriptRow {
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
fn widget_row_count(row: &crate::ui::chat_panel::TranscriptRow) -> usize {
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
fn has_question_row(e: &Exchange) -> bool {
    !e.question.is_empty()
}

/// Whether the exchange list's Q&A-bearing entries (per `has_question_row`)
/// number exactly one — i.e. whether the LAST push was the first real
/// follow-up question asked in this panel session (the auto-gloss exchange,
/// `exchanges[0]` with its empty question, never counts). Used right after
/// pushing a new Q&A exchange to decide whether to auto-save it: further
/// follow-ups (count > 1) still require `s`.
fn is_first_question_exchange(exchanges: &[Exchange]) -> bool {
    exchanges.iter().filter(|e| has_question_row(e)).count() == 1
}

/// Build the transcript rows. Returns `(rows, cursor_row, row_owner)`:
/// - `cursor_row`: the WIDGET-space row index of the exchange cursor
///   (`s.chat.cursor`)'s leading row — same role as before, just expressed in
///   widget space (see `widget_row_count`) instead of `Vec<TranscriptRow>`
///   space, which only differ for a `GlossAnswer` row.
/// - `row_owner[w]`: the exchange index that owns widget row `w`. Used to
///   derive `s.chat.cursor` from `s.chat.row_cursor` (j/k) — see
///   `transcript_cursor_move`.
fn transcript_rows(
    s: &AppState,
) -> (Vec<crate::ui::chat_panel::TranscriptRow>, usize, Vec<usize>) {
    build_transcript_rows(&s.chat.exchanges, s.chat.cursor)
}

/// Pure core of `transcript_rows` (no `AppState` — takes the exchange list
/// and exchange cursor directly), so the row-count/row_owner bookkeeping is
/// unit-testable without constructing an `AppState`.
fn build_transcript_rows(
    exchanges: &[Exchange],
    cursor: usize,
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
        let ans = answer_row(e);
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
/// its `Q:` row (via `has_question_row`/`answer_row`, same rules
/// `build_transcript_rows` uses per-exchange) plus a trailing `SavedMark` if
/// it's been saved — with NO chip and NO other exchange. This is
/// `build_transcript_rows`'s per-exchange body run for a single `i == 0`
/// slice, factored out so the "one exchange -> its rows" shape is
/// unit-testable without constructing a whole transcript or an `AppState`.
/// The chip is deliberately omitted (matching `render_transcript_with_thinking`'s
/// existing "question names its own subject" reasoning) — a Question-view
/// render never shows the source-preview label the full Gloss transcript
/// does.
fn build_single_exchange_rows(e: &Exchange) -> Vec<crate::ui::chat_panel::TranscriptRow> {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    if has_question_row(e) {
        rows.push(question_row(&e.question));
    }
    rows.push(answer_row(e));
    if e.saved_id.is_some() {
        rows.push(R::SavedMark);
    }
    rows
}

/// Render the transcript at the CURRENT `s.chat.row_cursor` (clamped to the
/// rendered widget count) — never resets it. A caller that just changed
/// `s.chat.cursor` (the exchange cursor: new answer, gloss push, consolidate)
/// must explicitly snap `row_cursor` to the new exchange's leading row FIRST
/// (see `snap_row_cursor_to_exchange`) — this function alone would otherwise
/// leave j/k's row cursor stranded on stale content.
///
/// Also clamps `s.chat.visual_anchor` (if a `V` selection is active) to the
/// same widget-row count and passes the resolved `(anchor, row_cursor)` range
/// down so the panel paints `.chat-visual-row` across the selection — the
/// same clamp-on-every-render discipline `row_cursor` itself already gets, so
/// a selection anchored on content that later shrinks (e.g. a regloss) can
/// never point past the end of the rendered rows.
pub(crate) fn render_transcript(s: &mut AppState) {
    let (rows, _cursor_row, row_owner) = transcript_rows(s);
    let n = row_owner.len();
    if n == 0 {
        s.chat.row_cursor = 0;
    } else if s.chat.row_cursor >= n {
        s.chat.row_cursor = n - 1;
    }
    if let Some(anchor) = s.chat.visual_anchor {
        s.chat.visual_anchor = Some(if n == 0 { 0 } else { anchor.min(n - 1) });
    }
    let selection = s.chat.visual_anchor.map(|a| {
        let cur = s.chat.row_cursor;
        if a <= cur { (a, cur) } else { (cur, a) }
    });
    render_paginated(s, &rows, Some(s.chat.row_cursor), selection);
}

/// Paginate `rows` at the live transcript budget, store the pages on
/// `ChatState`, derive/clamp the page holding `cursor_widget` (the accent-bar
/// row cursor), and render ONLY that page slice via `ChatPanel::render_page`.
/// The single paginated-render path shared by `render_transcript` and
/// `render_journal_view_inner`: it computes `s.chat.pages`/`page_idx` so Task 6
/// page-turn nav has authoritative page state, and renders the whole-widget
/// page slice that fits by construction (no partial row at either edge).
///
/// `cursor_widget` is `None` for views with no row cursor (a placeholder-only
/// list); then the FIRST page is shown and no accent bar is painted.
fn render_paginated(
    s: &mut AppState,
    rows: &[crate::ui::chat_panel::TranscriptRow],
    cursor_widget: Option<usize>,
    selection: Option<(usize, usize)>,
) {
    use crate::ui::chat_pagination::page_of_widget;
    let specs = crate::ui::chat_panel::row_widget_specs(rows);
    let (fam, sz) = transcript_font(s);
    let pages = s.chat_panel.paginate_specs(&specs, &fam, sz);
    let page_idx = match cursor_widget {
        Some(c) => page_of_widget(&pages, c),
        None => 0,
    };
    let page_idx = if pages.is_empty() {
        0
    } else {
        page_idx.min(pages.len() - 1)
    };
    s.chat.pages = pages;
    s.chat.page_idx = page_idx;
    let Some(&page) = s.chat.pages.get(page_idx) else {
        // No pages (empty transcript): render an empty slice.
        s.chat_panel
            .render_page(&specs, crate::ui::pagination::Page { start: 0, end: 0 }, None, None);
        return;
    };
    s.chat_panel.render_page(&specs, page, cursor_widget, selection);
}

/// Render `PanelView::Question`: exactly the exchange at `s.chat.cursor` —
/// its `Q:` row + answer row (+ `SavedMark` once saved), via
/// `build_single_exchange_rows` — NOT the gloss, NOT any other exchange.
/// Plain `render_rows` (no row-cursor accent bar, no visual-selection
/// painting), same choice `render_journal_view` makes: a one-exchange view
/// has nothing for `j`/`k`'s row axis to usefully cycle over, so it degrades
/// to viewport scrolling (see `transcript_cursor_move`'s `Question` guard).
/// No-ops (renders nothing) when `cursor` is out of range — defensive; every
/// real call site sets `cursor` to a just-pushed exchange's own index first.
pub(crate) fn render_current_question(s: &mut AppState) {
    let (fam, sz) = transcript_font(s);
    let Some(e) = s.chat.exchanges.get(s.chat.cursor) else {
        s.chat_panel.render_rows(&[], &fam, sz);
        return;
    };
    let rows = build_single_exchange_rows(e);
    s.chat_panel.render_rows(&rows, &fam, sz);
}

/// Snap the row cursor to the EXCHANGE cursor's (`s.chat.cursor`) leading
/// widget row. Called by every site that just wrote `s.chat.cursor` directly
/// (new answer arrives, gloss push, consolidate) so j/k's row cursor follows
/// the new content instead of pointing at a now-stale row — the same "jump to
/// what just changed" behavior the old exchange-only cursor always had.
///
/// The exchange's leading widget is often a `ChatGlossRowKind::Speaker` row
/// (a gloss block usually opens with one) — Fix 2 forbids landing there, so
/// advance to the first LANDABLE widget at or after it
/// (`first_landable_at_or_after`), same "skip the label, land on the
/// dialogue" rule j/k itself follows.
fn snap_row_cursor_to_exchange(s: &mut AppState) {
    let (rows, cursor_row, _row_owner) = transcript_rows(s);
    let landable = landable_mask(&rows);
    s.chat.row_cursor = first_landable_at_or_after(cursor_row, &landable);
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
    // Selects the gloss, deliberately — slot #1's content just changed under
    // the reader, so leaving the cursor on a follow-up below while the gloss
    // silently swaps above would be worse. `Ctrl+n`/`Ctrl+p` inherit this and
    // snap the cursor back up to the gloss; that is intended, not a leak of
    // the gloss axis into the j/k axis (cycle_gloss writes only gloss_index).
    s.chat.cursor = 0;
    snap_row_cursor_to_exchange(s);
    // A pushed gloss is exactly the content the Gloss view renders — force
    // back to it so `-`/regloss/gloss-cycling always show what they just
    // produced, even if the reader had toggled to Journal view first (`t`)
    // and then pressed `r`/Ctrl+n/p. Without this the gloss would update
    // in-memory while the panel kept showing the (now stale) journal list.
    s.chat.view = PanelView::Gloss;
    render_transcript(s);
}

/// Step an index by `delta`, wrapping at both ends. `len == 0` stays at 0
/// (guards the modulo).
fn wrap_index(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as i32;
    (((cur as i32 + delta) % n + n) % n) as usize
}

/// `Ctrl+n`/`Ctrl+p` in the transcript: show the next/previous STORED gloss
/// for the pinned passage, wrapping.
///
/// A different axis from `j`/`k`: those move `chat.cursor` over this session's
/// in-memory `exchanges`, while this moves over lit.db rows (including earlier
/// sessions'). Swaps transcript slot #1 in place, so follow-up exchanges below
/// are untouched.
///
/// No-op in `PanelView::Journal`: the journal view is a flat, uncycled list
/// of every saved Q&A for the passage (see `toggle_panel_view`'s doc comment
/// for why cycling was deliberately NOT built for it) — Ctrl+n/p has nothing
/// to swap there. Toasts rather than silently doing nothing, matching this
/// module's other reachable-but-inapplicable binds (e.g. `copy_gloss_id`'s
/// "No gloss to copy").
///
/// NOT no-op'd in `PanelView::Question` (deliberately, no guard needed):
/// Ctrl+n/p is the gloss axis, and `push_gloss_exchange` below unconditionally
/// sets `view = Gloss` and renders the full transcript — so cycling from a
/// shown Q&A reads as "switch to the gloss, then cycle it", which is more
/// useful than a toast telling the reader to press `t` first for a bind that
/// is about to take them there anyway.
/// `c` in the chat panel's Journal view: copy the SELECTED saved entry's
/// database row id to the Wayland clipboard (`wl-copy`) + a transient toast —
/// the panel mirror of the journal overlay's `copy_current_id`. The selected
/// entry is `journal_list[journal_cursor]` (the entry the accent bar sits on,
/// what `R`/save also act on). No-op when the list is empty.
pub(crate) fn copy_journal_id(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    let Some(id) = s.chat.journal_list.get(s.chat.journal_cursor).map(|p| p.id) else {
        return;
    };
    let copied = format!("Journal Q&A ID: {}", id);
    let _ = std::process::Command::new("wl-copy").arg(&copied).spawn();
    crate::input::navigation::show_chapter_toast_secs(&s, &format!("Copied {}", copied), 2);
    crate::logging::log(&format!("CHAT: copied \"{}\"", copied));
}

pub(crate) fn cycle_gloss(s: &mut AppState, delta: i32) {
    if s.chat.view == PanelView::Journal {
        crate::input::navigation::show_chapter_toast_secs(s, "Ctrl+n/p cycles glosses \u{2014} t for gloss view", 2);
        return;
    }
    let n = s.chat.gloss_list.len();
    if n <= 1 {
        return; // nothing to cycle to
    }
    // Order is load-bearing: gloss_index must land BEFORE push_gloss_exchange,
    // whose chip reads it to render "n of N".
    s.chat.gloss_index = wrap_index(s.chat.gloss_index, delta, n);
    let text = s.chat.gloss_list[s.chat.gloss_index].gloss_text.clone();
    let Some(ctx) = s.chat.gloss_ctx.clone() else { return };
    push_gloss_exchange(s, &ctx, &text);
    crate::logging::log(&format!(
        "CHAT-GLOSS: cycled to gloss {} of {}",
        s.chat.gloss_index + 1,
        n
    ));
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
fn flip_view(v: PanelView) -> PanelView {
    match v {
        PanelView::Gloss => PanelView::Journal,
        PanelView::Journal => PanelView::Gloss,
        PanelView::Question => PanelView::Gloss,
    }
}

/// Split a saved answer into paragraph chunks (blank-line separated), each a
/// separate row so the panel cursor can traverse them. Never returns empty (an
/// empty answer yields one empty chunk so the entry still has an answer row).
fn split_answer_paragraphs(answer: &str) -> Vec<String> {
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
fn journal_view_rows(
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

/// Load the pinned passage's saved journal entries (`scope='passage'`, exact
/// citation match — see `db::journal::find_passage_pages`). Empty on any DB
/// error or when there is no `gloss_ctx` to key the lookup from (mirrors
/// `reload_gloss_list`'s own "empty on failure" contract).
fn reload_journal_list(
    work_abbrev: &str,
    start_citation: &str,
    end_citation: &str,
) -> Vec<crate::db::journal::JournalPage> {
    crate::db::queries::open_db()
        .ok()
        .and_then(|conn| {
            crate::db::journal::find_passage_pages(&conn, work_abbrev, start_citation, end_citation).ok()
        })
        .unwrap_or_default()
}

/// The first widget-row index owned by `journal_list` entry `entry`, i.e. its
/// `Q:` row, found in the parallel `row_owner` map `journal_view_rows` builds.
/// Each entry now emits a `Q:` row plus one `Answer` row per answer paragraph
/// (see `journal_view_rows`/`split_answer_paragraphs`), so entry width varies
/// and the old `entry*2` no longer holds — the accent bar anchors on this
/// row. Falls back to `0` when the entry isn't in `row_owner` (empty list /
/// stale cursor); the caller clamps `journal_cursor` first, so this is
/// defensive.
fn journal_entry_first_row(row_owner: &[usize], entry: usize) -> usize {
    row_owner.iter().position(|&e| e == entry).unwrap_or(0)
}

/// Clamp a Journal-view row cursor to a list of `len` entries: `[0, len-1]`, or
/// `0` for an empty list (which renders a single non-landable placeholder row).
fn clamp_journal_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        cursor.min(len - 1)
    }
}

/// Render the `Journal` view at the current `s.chat.journal_list`. Each entry
/// now emits a `Q:` row plus one `Answer` row per answer paragraph (see
/// `journal_view_rows`/`split_answer_paragraphs`), so the view participates in
/// the SAME `row_cursor`/`landable_mask`/`row_owner` machinery the Gloss view
/// uses: the accent bar paints on `row_cursor` (a widget index) and `j`/`k`
/// traverse the answer paragraphs. `journal_cursor` (the ENTRY selected, what
/// `R`/save act on) is derived from `row_owner[row_cursor]` after each move.
/// The parallel `journal_row_owner` is rebuilt here and stashed on `ChatState`
/// so the nav functions can map back to the entry without re-deriving rows.
///
/// `snap_to_entry`: when `true`, `row_cursor` is snapped to the FIRST widget
/// row of the selected `journal_cursor` ENTRY — the entry-granularity entry
/// points (toggle into Journal, `gg`/`G`, rewrite-return) set `journal_cursor`
/// directly and need the accent bar to follow. `j`/`k` pass `false`: they
/// already set `row_cursor` to the precise answer-paragraph row and derived
/// `journal_cursor` from it, so re-snapping would strand the bar on the `Q:`
/// row.
fn render_journal_view_inner(s: &mut AppState, snap_to_entry: bool) {
    let (rows, row_owner) = journal_view_rows(&s.chat.journal_list);
    let len = s.chat.journal_list.len();
    s.chat.journal_cursor = clamp_journal_cursor(s.chat.journal_cursor, len);
    if snap_to_entry {
        s.chat.row_cursor = journal_entry_first_row(&row_owner, s.chat.journal_cursor);
    }
    s.chat.journal_row_owner = row_owner;
    if len == 0 {
        // Placeholder-only list: no landable row, scroll to top, no accent bar.
        let (fam, sz) = transcript_font(s);
        s.chat_panel.render_rows_to_top(&rows, &fam, sz);
        return;
    }
    // Clamp the widget-space `row_cursor` into range, then land the accent bar
    // on it; `render_paginated` renders the page slice holding it. No visual
    // selection in Journal view.
    let n = rows.len();
    if s.chat.row_cursor >= n {
        s.chat.row_cursor = n.saturating_sub(1);
    }
    let cursor_row = s.chat.row_cursor;
    render_paginated(s, &rows, Some(cursor_row), None);
}

/// Entry-granularity render: snap the accent bar to the selected entry's `Q:`
/// row. The default for every caller EXCEPT `j`/`k` (which uses
/// `render_journal_view_inner(s, false)`).
fn render_journal_view(s: &mut AppState) {
    render_journal_view_inner(s, true);
}

/// `R` in the chat panel's Journal view: rewrite the SELECTED saved Q&A by
/// reusing the journal overlay's rewrite popup + pipeline (Approach A). Seeds
/// `s.journal.pages`/`page_index`/band from the cursor'd `journal_list` entry so
/// `displayed_journal_page` resolves it, sets `rewrite_return` so the pipeline's
/// overlay-render sites re-render THIS panel instead, then opens the popup.
///
/// No-op (toast) with no `gloss_ctx` (panel opened via Tab, never glossed) or an
/// empty `journal_list` — mirrors `toggle_panel_view`/`regloss_pinned`.
pub(crate) fn rewrite_journal_entry(state_rc: &Rc<RefCell<AppState>>) {
    {
        let mut s = state_rc.borrow_mut();
        if s.chat.view != PanelView::Journal {
            return;
        }
        let Some(ctx) = s.chat.gloss_ctx.clone() else {
            crate::input::navigation::show_chapter_toast_secs(&s, "No passage to rewrite", 2);
            return;
        };
        if s.chat.journal_list.is_empty() {
            crate::input::navigation::show_chapter_toast_secs(&s, "No journal entry to rewrite", 2);
            return;
        }
        let cursor = clamp_journal_cursor(s.chat.journal_cursor, s.chat.journal_list.len());
        s.chat.journal_cursor = cursor;
        // Seed the overlay page state so displayed_journal_page() resolves the
        // selected entry. Clear any stale filter (the panel has none, but the
        // pipeline reads journal.filter first).
        s.journal.filter = None;
        s.journal.pages = s.chat.journal_list.clone();
        s.journal.page_index = cursor;
        s.journal_band = crate::app::JournalBand::Passage {
            div1: ctx.act,
            div2: ctx.scene,
            start: ctx.start_citation.clone(),
            end: ctx.end_citation.clone(),
        };
        s.chat.rewrite_return = true;
    }
    // Opens the q/a/b popup (InputMode::RewriteTargetChoice); the pipeline runs
    // unchanged, and the rewrite_return guards in journal.rs route completion
    // back to this panel.
    crate::input::actions::journal::open_rewrite_target(state_rc);
}

/// Return a panel-initiated `R` rewrite to the chat panel: reload the pinned
/// passage's journal list from lit.db, re-render Journal view with the cursor
/// still on the rewritten entry (re-found by `id` so a timestamp bump can't
/// strand it), restore `ChatTranscript`, and clear `rewrite_return`. Called by
/// journal.rs's rewrite-completion / cancel sites when `rewrite_return` is set.
/// `rewritten_id` is the entry that changed (`None` on cancel — keep the cursor
/// where it is).
pub(crate) fn finish_panel_rewrite(s: &mut AppState, rewritten_id: Option<i64>) {
    if let Some(ctx) = s.chat.gloss_ctx.clone() {
        s.chat.journal_list =
            reload_journal_list(&ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation);
    }
    if let Some(id) = rewritten_id {
        if let Some(pos) = s.chat.journal_list.iter().position(|p| p.id == id) {
            s.chat.journal_cursor = pos;
        }
    }
    s.chat.journal_cursor = clamp_journal_cursor(s.chat.journal_cursor, s.chat.journal_list.len());
    s.chat.view = PanelView::Journal;
    render_journal_view(s);
    s.input_mode = crate::app::InputMode::ChatTranscript;
    s.chat.rewrite_return = false;
}

/// Open the chat panel's own input as a rewrite-INSTRUCTION card for a
/// panel-initiated `R` on the `a` (answer) or `b` (both) path. The overlay's
/// instruction card lives on the hidden journal_overlay widget, so the panel
/// must show its own. `submit_panel_rewrite` (Ctrl+Enter) reads the typed
/// instruction and runs the stashed `journal.vim_rewrite` tuple. Opens in vim
/// NORMAL (matching the overlay card) so the empty-Ctrl+Enter meaning is read
/// first; press `i` to type.
pub(crate) fn open_rewrite_instruction_input(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    s.chat_panel.open_input(
        "Rewrite instruction",
        "Ctrl+Enter rewrite \u{00b7} empty = afresh \u{00b7} Esc cancel",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
        false,
    );
    s.chat_panel.flash_input();
}

/// Ctrl+Enter in the panel rewrite-instruction card: mirror journal
/// `submit_prompt`'s rewrite branch, but read the PANEL's input text and keep
/// `rewrite_return` set so the completion re-renders the panel. No-op when no
/// `vim_rewrite` is stashed (defensive — this is only opened by the `a`/`b`
/// panel path, which always stashes first).
pub(crate) fn submit_panel_rewrite(state_rc: &Rc<RefCell<AppState>>) {
    let text = state_rc.borrow().chat_panel.take_input_text();
    let rewrite = state_rc.borrow_mut().journal.vim_rewrite.take();
    state_rc.borrow().chat_panel.close_input();
    let Some((id, question, answer, target)) = rewrite else {
        // Nothing stashed: fall back to transcript focus.
        focus_transcript(&mut state_rc.borrow_mut());
        return;
    };
    let instruction = text.trim();
    let instruction = if instruction.is_empty() {
        "No further instruction was given; answer this question afresh under the standard guidance, grounded as before."
    } else {
        instruction
    };
    crate::input::actions::journal::rewrite_with_claude(
        state_rc, id, &question, &answer, instruction, target,
    );
}

/// `t` on the transcript: toggle the panel between showing the pinned
/// passage's stored GLOSS(es) (the existing `exchanges`-driven view) and its
/// saved JOURNAL Q&As (`scope='passage'`, exact citation match — the entries
/// `s` on THIS passage saved, not the whole scene's journal).
///
/// No-op with a toast when there is no `gloss_ctx` — the panel was opened
/// with `Tab` (not `-`), so there is no glossed passage to look up a journal
/// for. This mirrors `regloss_pinned`'s "No passage to regloss" no-op for the
/// same missing-context reason.
///
/// Switching TO `Journal` (re)loads `journal_list` from lit.db every time,
/// not just on first entry: a `s` save made while looking at the journal (or
/// a follow-up `a` saved earlier in this session) must show up on the very
/// next toggle back, not a stale snapshot from whenever the view was first
/// opened. This is one DB read gated behind an explicit keypress — cheap
/// compared to the gloss-cache reload the panel already does on every `-`
/// and every gloss save, and simpler than trying to invalidate a cached copy
/// from every save site.
///
/// Deliberately does NOT cycle journal entries with `Ctrl+n`/`Ctrl+p` — a
/// flat list that `j`/`k` scrolls is simpler and the spec explicitly allows
/// it ("Decide and justify; don't over-build"). A passage rarely accumulates
/// more than a handful of saved Q&As, so paging one at a time (mirroring
/// gloss cycling) would add a second cursor axis for little benefit; showing
/// them all at once, newest last (`find_passage_pages`' own insertion order),
/// reads like the reader's own running notes on the passage.
pub(crate) fn toggle_panel_view(state_rc: &Rc<RefCell<AppState>>) {
    let ctx = state_rc.borrow().chat.gloss_ctx.clone();
    let Some(ctx) = ctx else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No passage to show a journal for", 2);
        return;
    };
    let mut s = state_rc.borrow_mut();
    s.chat.view = flip_view(s.chat.view);
    match s.chat.view {
        PanelView::Journal => {
            s.chat.journal_list =
                reload_journal_list(&ctx.work_abbrev, &ctx.start_citation, &ctx.end_citation);
            s.chat.journal_cursor = 0;
            // Task 6: a view switch resets to the FIRST page + first landable
            // widget. `render_journal_view` snaps the row cursor to entry 0's
            // leading widget and re-paginates to the page holding it (page 0);
            // reset page_idx explicitly first so the snap starts from a clean
            // page state rather than the previous view's stale page.
            s.chat.page_idx = 0;
            s.chat.row_cursor = 0;
            render_journal_view(&mut s);
            crate::logging::log(&format!(
                "CHAT-JOURNAL: view toggled to journal ({} entries)",
                s.chat.journal_list.len()
            ));
        }
        PanelView::Gloss => {
            // Task 6: reset to the first page + first landable widget on the
            // switch back to Gloss, then re-paginate/render for that cursor.
            s.chat.page_idx = 0;
            let (rows, _cursor_row, row_owner) = transcript_rows(&s);
            let landable = landable_mask(&rows);
            if let Some(first) =
                crate::ui::chat_pagination::first_landable_in_page(
                    crate::ui::pagination::Page { start: 0, end: landable.len() },
                    &landable,
                )
            {
                s.chat.row_cursor = first;
                if let Some(&owner) = row_owner.get(first) {
                    s.chat.cursor = owner;
                }
            }
            render_transcript(&mut s);
            crate::logging::log("CHAT-JOURNAL: view toggled to gloss");
        }
        // flip_view never PRODUCES Question (see its doc comment) — this arm
        // is unreachable in practice, kept only so the match stays exhaustive
        // if PanelView grows further. No render: Question is only ever
        // entered by submit_chat_prompt, not by this toggle.
        PanelView::Question => {}
    }
}

/// `c` on the transcript: copy the currently-displayed gloss's id to the
/// clipboard, mirroring the gloss OVERLAY's `c` (`gloss::copy_gloss_id`,
/// bound in `handle_gloss_key`) — same "Gloss ID: {id}" label, same `wl-copy`
/// spawn, same log line, same "Copied {}" toast.
///
/// Deliberately NOT a call to `gloss::copy_gloss_id`: that function reads
/// `s.gloss_list`/`s.gloss_index` on `AppState`, which belong to the gloss
/// OVERLAY — a same-named but entirely separate pair of fields from this
/// panel's `s.chat.gloss_list`/`s.chat.gloss_index`. Calling the overlay's
/// version from the panel would copy whatever the overlay last displayed (or
/// nothing), not the gloss on screen in the chat panel.
pub(crate) fn copy_gloss_id(state: &Rc<RefCell<AppState>>) {
    let copied = {
        let s = state.borrow();
        s.chat.gloss_list.get(s.chat.gloss_index).map(|gloss| {
            let copied = format!("Gloss ID: {}", gloss.gloss_id);
            let _ = std::process::Command::new("wl-copy").arg(&copied).spawn();
            crate::logging::log(&format!("CHAT-GLOSS: copied \"{}\" to clipboard", copied));
            copied
        })
    };
    // Toast AFTER dropping the borrow above — show_chapter_toast_secs takes
    // &AppState, so it re-borrows; mirrors copy_gloss_id's own toast-after-
    // borrow-drop discipline.
    let s = state.borrow();
    match copied {
        Some(copied) => {
            crate::input::navigation::show_chapter_toast_secs(&s, &format!("Copied {}", copied), 3);
        }
        // Panel opened via Tab (no gloss ever shown) or the gloss list failed
        // to load: unlike the overlay (where `c` is unreachable without a
        // displayed gloss), the transcript's `c` is reachable any time the
        // panel is open, so silence would read as a dead key. A short toast
        // gives the same feedback shape as every other no-op bind in this
        // module (e.g. "No passage to regloss").
        None => {
            crate::input::navigation::show_chapter_toast_secs(&s, "No gloss to copy", 2);
        }
    }
}

/// The "n of N" chip for the gloss slot, so cycling shows which stored gloss
/// is on screen.
/// The gloss slot's chip. EMPTY for a lone gloss — the label said nothing the
/// panel's own content didn't, and a bare "Reader gloss" row above the gloss
/// read as noise. It earns its row only once a passage has several stored
/// glosses, where "2 of 5" is the sole cue telling `Ctrl+n`/`Ctrl+p` which one
/// is on screen. An empty chip renders no row (see `transcript_rows`).
fn gloss_chip(s: &AppState) -> String {
    let n = s.chat.gloss_list.len();
    if n <= 1 {
        String::new()
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
        render_transcript_thinking_gloss(&s, &ctx);
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
        render_transcript(&mut s);
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

/// The transcript with the passage being glossed pushed as a `GlossAnswer`
/// row (Fix 1), followed by a "thinking…" row, so the panel shows the source
/// text AND work-in-flight instead of sitting on a bare "thinking…" for the
/// whole API round-trip.
///
/// The seam: `ctx.passage_doc()` (`src/gloss.rs`) reconstructs the SAME
/// `<speaker>`/`<verse>`/`<stage>` markup `echoes::build_source_header`
/// builds from `selected_lines` — it already exists for exactly this
/// purpose (the gloss OVERLAY's `show_glossing` loading card uses it the same
/// way, see its doc comment). `request_reader_gloss` takes `ctx:
/// GlossContext` (not `selected_lines: &[Line]`), and `ctx` alone is enough
/// to reconstruct the doc — no new parameter, no field added to `ChatState`.
/// This is preferred over threading `selected_lines` through
/// `request_reader_gloss` (which `regloss_pinned` doesn't have — it only has
/// `chat.gloss_ctx`, a `GlossContext`) and over storing the doc on
/// `ChatState` (an extra field to keep in sync with `gloss_ctx`, for a value
/// `ctx.passage_doc()` derives on demand).
///
/// Pushing `R::GlossAnswer(ctx.passage_doc())` (not a plain label) matters:
/// `chat_gloss_rows` renders that markup exactly as it will render the
/// FINISHED gloss's leading source rows (same `<speaker>`/`<verse>` tags),
/// so the panel does not jump/reflow when the answer lands — the source rows
/// are already on screen, just followed by "thinking…" instead of the
/// explication.
fn render_transcript_thinking_gloss(s: &AppState, ctx: &crate::gloss::GlossContext) {
    use crate::ui::chat_panel::TranscriptRow as R;
    // While a regloss (`r`/`R`) is in flight, show ONLY the source passage and
    // "thinking…" — NOT the prior transcript. Prepending `transcript_rows(s)`
    // left the CURRENT gloss stacked above the one being regenerated (same wall
    // -of-text problem the question path had). The passage doc is the context
    // being reglossed; the new gloss replaces it when it lands.
    let rows = vec![R::GlossAnswer(ctx.passage_doc()), R::Thinking];
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows(&rows, &fam, sz);
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

fn render_transcript_with_thinking(s: &AppState, question: &str, _chip: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    // While a question answers, show ONLY the question and "thinking…" — NOT
    // the prior transcript (the gloss + any earlier exchanges). The gloss is
    // what the question is ABOUT; leaving it above the pending answer made the
    // panel a wall of text with the live question buried at the bottom. The
    // answer, when it lands, replaces this with the SAME single-Q&A shape
    // (`render_current_question`, `PanelView::Question`) — the gloss does NOT
    // return until `t`. The source-preview chip is dropped with the rest —
    // the question names its own subject.
    let rows = vec![
        question_row(question),
        R::Thinking,
    ];
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows(&rows, &fam, sz);
}

fn render_transcript_with_error(s: &AppState, msg: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let (mut rows, _, _) = transcript_rows(s);
    rows.push(R::Error(msg.to_string()));
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows(&rows, &fam, sz);
}

/// Move the j/k ROW cursor (`s.chat.row_cursor`, widget-space — see
/// `transcript_rows`' doc comment) by `delta` and scroll it into view. When
/// the cursor is already clamped at a boundary (single row, or first/last
/// LANDABLE row), degrade to plain viewport scrolling so an answer taller
/// than the panel stays fully readable — same fallback the old exchange-only
/// cursor used, and for the same reason (see `scroll_transcript_step`'s doc
/// comment).
///
/// A `ChatGlossRowKind::Speaker` widget (e.g. "BELARIUS") is never a valid
/// landing spot — see `landable_mask`'s doc comment (Fix 2: speaker labels
/// aren't lines you'd read, copy, or mark). `chat_pagination::step_cursor_paged`
/// skips over them; `j` from the last verse of one speaker's block lands on
/// the first verse of the NEXT block, never on its label in between.
///
/// `s.chat.cursor` (the EXCHANGE cursor — save/`▶`/Ctrl+n/p target) is
/// derived from the new row position via `row_owner`, NOT moved
/// independently: the row IS inside some exchange, so "which exchange is
/// selected" is a pure function of "which row the cursor is on". Keeping two
/// independently-steppable cursors would let them drift out of sync (e.g. `s`
/// saving an exchange the accent bar isn't even on).
/// Ctrl-d / Ctrl-u: vim half-page scroll. A pure VIEWPORT move — it does not
/// touch the row cursor or care which view is active, so unlike
/// `transcript_cursor_move` it needs no landable-row logic or Gloss/Journal/
/// Question gating.
pub(crate) fn transcript_half_page(s: &mut AppState, down: bool) {
    s.chat_panel.scroll_transcript_half_page(down);
}

pub(crate) fn transcript_cursor_move(s: &mut AppState, delta: i32) {
    // Journal now uses the SAME widget-space `row_cursor` + `landable_mask`
    // machinery as the Gloss branch below, but over its own row source
    // (`journal_view_rows`, not `transcript_rows`) and its own owner map
    // (`journal_row_owner`, widget -> ENTRY). `j`/`k` step `row_cursor` across
    // the answer paragraphs and derive `journal_cursor` (the ENTRY `R`/save
    // act on) from the owner. An empty `journal_list` has no landable widget,
    // so it falls back to plain scrolling like Question.
    //
    // Question is a flat, uncycled view with no row_cursor/row_owner of its
    // own — it doesn't go through `render_transcript` (it uses
    // `render_current_question`, plain `render_rows`, no accent bar). A
    // single Q&A is rarely taller than the panel, and even when it is, plain
    // scrolling reads it fully — j/k just scrolls.
    if s.chat.view == PanelView::Journal {
        let len = s.chat.journal_list.len();
        if len == 0 {
            s.chat_panel.scroll_transcript_step(delta as f64);
            return;
        }
        // Step the WIDGET row cursor over the journal landable mask (every
        // emitted journal widget — the `Q:` row and each answer paragraph — is
        // landable), TURNING THE PAGE at the page edge via `step_cursor_paged`
        // (Task 6: lands on the next page's first / prev page's last landable
        // widget), then derive `journal_cursor` (the ENTRY `R`/save act on)
        // from `journal_row_owner`. Mirrors the Gloss branch below. `s.chat.
        // pages`/`page_idx` are authoritative here — the last journal render
        // (`render_paginated`) computed them for exactly these rows.
        let (rows, row_owner) = journal_view_rows(&s.chat.journal_list);
        s.chat.journal_row_owner = row_owner;
        let landable = landable_mask(&rows);
        let (new_cursor, new_page) = crate::ui::chat_pagination::step_cursor_paged(
            s.chat.row_cursor,
            delta,
            s.chat.page_idx,
            &s.chat.pages,
            &landable,
        );
        s.chat.row_cursor = new_cursor;
        s.chat.page_idx = new_page;
        if let Some(&entry) = s.chat.journal_row_owner.get(new_cursor) {
            s.chat.journal_cursor = entry;
        }
        render_journal_view_inner(s, false);
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_step(delta as f64);
        return;
    }
    let (rows, _cursor_row, row_owner) = transcript_rows(s);
    if row_owner.is_empty() {
        return;
    }
    let landable = landable_mask(&rows);
    // Page-aware step: within the page move to the next/prev landable widget;
    // at the page edge TURN THE WHOLE PAGE and land on the next page's first /
    // prev page's last landable widget (Task 6). Clamps (no-op) at the
    // document ends. `s.chat.pages`/`page_idx` were computed by the last
    // `render_transcript` for exactly these rows, so they are authoritative.
    let (new_cursor, new_page) = crate::ui::chat_pagination::step_cursor_paged(
        s.chat.row_cursor,
        delta,
        s.chat.page_idx,
        &s.chat.pages,
        &landable,
    );
    s.chat.row_cursor = new_cursor;
    s.chat.page_idx = new_page;
    s.chat.cursor = row_owner[new_cursor];
    render_transcript(s);
}

/// Widget-space "is this row a valid j/k landing spot" mask, one entry per
/// widget `chat_panel::rebuild_from_specs` would actually paint (same
/// granularity as `row_owner`/`widget_row_count`) — flat-mapped from
/// `chat_panel::row_widget_landable`, so it can never drift out of sync with
/// what the panel renders or with `row_widget_texts` (`y`'s copy source).
/// Only a `ChatGlossRowKind::Speaker` widget (e.g. "CYMBELINE") is `false`:
/// it isn't a line of dialogue, so it isn't a cursor stop — see Fix 2's brief
/// ("do not highlight speaker labels; only lines of dialogue").
fn landable_mask(rows: &[crate::ui::chat_panel::TranscriptRow]) -> Vec<bool> {
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
fn first_landable_at_or_after(from: usize, landable: &[bool]) -> usize {
    (from..landable.len())
        .find(|&i| landable[i])
        .unwrap_or(from)
}

/// `gg` on the transcript: move the row cursor to the FIRST page's first
/// landable widget (in `PanelView::Gloss`), set `page_idx = 0`, and re-render
/// via `render_transcript`'s `render_paginated` path — which shows the page
/// holding the cursor (here page 0), the same page-slice render
/// `transcript_cursor_move` uses. In `PanelView::Journal` this instead moves
/// `journal_cursor` to
/// entry 0 and re-renders (or, on an empty `journal_list`, falls back to a
/// plain scroll-to-top — there is no entry to land on). In
/// `PanelView::Question` (no row cursor — see `transcript_cursor_move`'s
/// guard) this is a plain scroll-to-top, mirroring how `j`/`k` degrade to
/// scrolling in that view.
pub(crate) fn transcript_cursor_first(s: &mut AppState) {
    if s.chat.view == PanelView::Journal {
        if !s.chat.journal_list.is_empty() {
            s.chat.journal_cursor = 0;
            render_journal_view(s);
        } else {
            s.chat_panel.scroll_transcript_to_edge(false);
        }
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_to_edge(false);
        return;
    }
    let (rows, _cursor_row, row_owner) = transcript_rows(s);
    let landable = landable_mask(&rows);
    // Task 6: gg jumps to the FIRST page's first landable widget. Paginate at
    // the live budget to find page 0, then land on its first landable widget.
    let specs = crate::ui::chat_panel::row_widget_specs(&rows);
    let (fam, sz) = transcript_font(s);
    let pages = s.chat_panel.paginate_specs(&specs, &fam, sz);
    let Some(&page0) = pages.first() else {
        return;
    };
    let Some(first) = crate::ui::chat_pagination::first_landable_in_page(page0, &landable) else {
        return;
    };
    s.chat.row_cursor = first;
    s.chat.page_idx = 0;
    s.chat.cursor = row_owner[first];
    render_transcript(s);
}

/// `G` on the transcript: symmetric counterpart to `transcript_cursor_first`
/// — moves the row cursor to the LAST landable row (Gloss view), moves
/// `journal_cursor` to the last entry (Journal view, or falls back to
/// scroll-to-bottom when `journal_list` is empty), or scrolls to the bottom
/// (Question view).
pub(crate) fn transcript_cursor_last(s: &mut AppState) {
    if s.chat.view == PanelView::Journal {
        let len = s.chat.journal_list.len();
        if len != 0 {
            s.chat.journal_cursor = len - 1;
            render_journal_view(s);
        } else {
            s.chat_panel.scroll_transcript_to_edge(true);
        }
        return;
    }
    if s.chat.view == PanelView::Question {
        s.chat_panel.scroll_transcript_to_edge(true);
        return;
    }
    let (rows, _cursor_row, row_owner) = transcript_rows(s);
    let landable = landable_mask(&rows);
    // Task 6: G jumps to the LAST page's last landable widget. Paginate at the
    // live budget to find the last page, then land on its last landable widget.
    let specs = crate::ui::chat_panel::row_widget_specs(&rows);
    let (fam, sz) = transcript_font(s);
    let pages = s.chat_panel.paginate_specs(&specs, &fam, sz);
    let Some(&last_page) = pages.last() else {
        return;
    };
    let Some(last) = crate::ui::chat_pagination::last_landable_in_page(last_page, &landable) else {
        return;
    };
    s.chat.row_cursor = last;
    s.chat.page_idx = pages.len() - 1;
    s.chat.cursor = row_owner[last];
    render_transcript(s);
}

/// `V` on the transcript: toggle a panel-local visual selection anchored at
/// the current `row_cursor`. A second `V` (or `Escape` — see
/// `handle_chat_transcript_key`) exits WITHOUT copying, mirroring the
/// reader's `V`/`Escape` exit path but over `ChatState.visual_anchor`, not
/// `AppState.visual_selection` (see its doc comment for why those must never
/// be conflated). No-ops (does not enter) on an empty transcript — there is
/// no row to anchor on, and entering would leave `row_cursor`/`visual_anchor`
/// both at 0 with nothing rendered to show a selection over, silently
/// swallowing the very next `y` (copies zero rows) instead of failing loudly.
///
/// Also no-ops when the transcript has rows but NONE are landable (Fix 2's
/// no-landable-rows edge case — a gloss that is somehow all speaker labels):
/// `row_cursor` is kept on a landable row by `transcript_cursor_move` /
/// `snap_row_cursor_to_exchange` whenever ANY landable row exists, so this
/// check only fires in that all-unlandable case, and exists purely so `V`
/// cannot anchor a selection on a speaker label by construction.
pub(crate) fn toggle_transcript_visual(s: &mut AppState) {
    // No row_cursor/row_owner axis in Journal or Question view (see
    // `transcript_cursor_move`'s same guard) — `V` has nothing to anchor a
    // selection over there, so it's a no-op rather than silently entering a
    // selection state that `y`/render_transcript would then act on over the
    // WRONG (Gloss-view, whole-`exchanges`) content instead of what's on
    // screen.
    if s.chat.view == PanelView::Journal || s.chat.view == PanelView::Question {
        crate::logging::log("CHAT-VISUAL: V ignored (journal/question view)");
        return;
    }
    if s.chat.visual_anchor.take().is_some() {
        render_transcript(s);
        crate::logging::log("CHAT-VISUAL: exited");
        return;
    }
    let (rows, _cursor_row, row_owner) = transcript_rows(s);
    if row_owner.is_empty() {
        crate::logging::log("CHAT-VISUAL: V ignored (empty transcript)");
        return;
    }
    if !landable_mask(&rows).iter().any(|&l| l) {
        crate::logging::log("CHAT-VISUAL: V ignored (no landable rows)");
        return;
    }
    s.chat.visual_anchor = Some(s.chat.row_cursor);
    render_transcript(s);
    crate::logging::log(&format!("CHAT-VISUAL: entered at row {}", s.chat.row_cursor));
}

/// Exit the transcript visual selection without copying (`Escape`'s first
/// press while `V` is active). No-op — and no render — when no selection is
/// active, so `Escape` falls through to `focus_reader` untouched.
pub(crate) fn exit_transcript_visual(s: &mut AppState) -> bool {
    if s.chat.visual_anchor.take().is_some() {
        render_transcript(s);
        crate::logging::log("CHAT-VISUAL: exited (Escape)");
        true
    } else {
        false
    }
}

/// Pure range resolution: anchor/cursor in either order -> `(start, end)`
/// inclusive, widget-row space. Shared by `render_transcript` (highlight) and
/// `yank_transcript_selection` (copy) so both agree on exactly which rows are
/// "selected".
fn visual_selection_range(anchor: usize, cursor: usize) -> (usize, usize) {
    if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) }
}

/// `y` on the transcript: copy to the system clipboard via `wl-copy` — ONLY
/// wl-copy, never xclip/xsel (Wayland; see `visual::action_copy`'s and
/// `copy_gloss_id`'s precedent). Two cases:
/// - Visual selection active: every widget row in `(anchor, row_cursor)`
///   (inclusive, either order), joined by newlines, THEN exits visual mode —
///   mirroring the reader's `yank_selection` contract (copy, then
///   `exit_visual_mode`), not its buffer-based mechanism (the panel has no
///   `GtkTextBuffer` to read from).
/// - No selection: just the cursor ROW's text — one verse line, one gloss
///   paragraph, matching j/k's own granularity. Deliberately NOT the whole
///   exchange or block: `row_cursor` already points at exactly one widget,
///   and `y` should copy what the accent bar is on, nothing more.
///
/// Text comes from `chat_panel::row_widget_texts`, which yields RENDERED text
/// (e.g. "CYMBELINE", not `<speaker>CYMBELINE</speaker>`) — see its doc
/// comment and `row_widget_texts_tests::gloss_answer_yields_rendered_text_not_raw_markup`.
///
/// Fix 2 / spanned-speaker decision: `j`/`k` (and thus `row_cursor`/the `V`
/// anchor) can never LAND on a Speaker widget, but a `V` selection can still
/// SPAN one — e.g. selecting from the last verse of one speaker's block down
/// to the first verse of the next necessarily passes through that next
/// speaker's label in between. This function deliberately does NOT filter
/// unlandable rows out of the slice: `all_texts[start..=end]` includes every
/// widget in range, spanned Speaker labels included. A pasted excerpt that
/// silently dropped the speaker name ("who says this line?") would be worse
/// than one that keeps it — the label is legitimate context for a quoted
/// passage, it just isn't something you'd cursor onto or copy ALONE. Compare
/// `landable_mask`, which gates j/k/V-anchor landing, not what `V`+`y` copies
/// once spanned.
pub(crate) fn yank_transcript_row_or_selection(state_rc: &Rc<RefCell<AppState>>) {
    // Journal and Question views have no row_cursor/row_owner axis (see
    // `transcript_cursor_move`'s guard) — `y` would otherwise copy whatever
    // stale Gloss-view row_cursor points at (from the whole `exchanges`
    // list), silently yanking content that isn't even the one on screen.
    if matches!(state_rc.borrow().chat.view, PanelView::Journal | PanelView::Question) {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "Nothing to copy in this view", 2);
        return;
    }
    // Collect text and drop the borrow BEFORE toasting: show_chapter_toast_secs
    // takes &AppState and re-borrows, so toasting under this borrow_mut would
    // double-borrow and panic (mirrors copy_gloss_id's own toast-after-
    // borrow-drop discipline — see its comment).
    let (text, n_rows, had_selection, flash_range) = {
        let mut s = state_rc.borrow_mut();
        let (rows, _cursor_row, _row_owner) = transcript_rows(&s);
        let all_texts: Vec<String> = rows
            .iter()
            .flat_map(crate::ui::chat_panel::row_widget_texts)
            .collect();
        let n = all_texts.len();
        let had_selection = s.chat.visual_anchor.is_some();
        let range = match s.chat.visual_anchor {
            Some(anchor) => visual_selection_range(anchor, s.chat.row_cursor),
            None => (s.chat.row_cursor, s.chat.row_cursor),
        };
        let selected: Vec<String> = if n == 0 {
            Vec::new()
        } else {
            let (start, end) = (range.0.min(n - 1), range.1.min(n - 1));
            all_texts[start..=end].to_vec()
        };
        let count = selected.len();
        let text = selected.join("\n");
        let flash_range = if n == 0 { None } else { Some((range.0.min(n - 1), range.1.min(n - 1))) };
        if had_selection {
            // Copy-then-exit, mirroring the reader's yank_selection contract.
            // This rebuild destroys the current row widgets and (via
            // render_transcript's render_page) queues their replacements on the
            // idle loop — see flash_rows' doc comment for why the flash below
            // still lands on the resulting LIVE widgets rather than the ones
            // being torn down here.
            s.chat.visual_anchor = None;
            render_transcript(&mut s);
        }
        (text, count, had_selection, flash_range)
    };

    if n_rows == 0 {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "Nothing to copy", 2);
        return;
    }

    let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
    // No success toast: the row flash below is the visual confirmation that
    // replaces it — `y` is a reflex, and the flash lands right on the copied
    // line(s), which a toast never did. The failure toast above stays — an
    // empty transcript is the case the reader cannot see for themselves.
    if let Some((start, end)) = flash_range {
        let s = state_rc.borrow();
        s.chat_panel.flash_rows(start, end);
    }
    crate::logging::log(&format!(
        "CHAT-YANK: copied {} row(s){} to clipboard",
        n_rows,
        if had_selection { " (visual)" } else { "" }
    ));
}

/// Pure persistence core shared by `save_selected_exchange` (`s`) and the
/// first-Q&A auto-save (`submit_chat_prompt`'s success callback): write
/// `exchanges[idx]` to `journal_entries` via `save_passage_page` (always
/// `scope='passage'`, keyed by the exchange's own citations — this is what
/// makes the row show up in the `t` Journal view's `find_passage_pages`
/// lookup), set `saved_id` on success, and re-derive the glossed-line tint so
/// the passage colors immediately (mirrors every gloss.rs save/edit/delete
/// path). Returns the new journal row id, or `None` if the exchange/work is
/// missing or the write failed. Callers own everything ELSE that differs
/// between the two save paths (revision arming, retitle, `render_saved_entry`
/// vs plain toast, view pivot) — this function only ever touches
/// `exchanges[idx].saved_id` and the tint.
///
/// Takes `&mut AppState` directly (no `Rc<RefCell<..>>`) so it composes under
/// a borrow the caller already holds — both call sites are inside an existing
/// `state_rc.borrow_mut()`.
fn persist_exchange_to_journal(s: &mut AppState, idx: usize) -> Option<i64> {
    let e = s.chat.exchanges.get(idx)?;
    let work = s.current_work.as_ref()?;
    let abbrev = work.canonical_abbrev.clone();
    let model = s.config.claude_model.clone();
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
            // See doc comment: without this the entry existed but its
            // passage stayed unmarked until some other path recomputed.
            crate::app::apply_reader_gloss_highlighting(s);
            crate::logging::log(&format!("CHAT: saved exchange as journal page {}", id));
            Some(id)
        }
        Err(err) => {
            crate::logging::log(&format!("CHAT: save failed: {}", err));
            None
        }
    }
}

/// `s` on the transcript: save the selected exchange as a passage journal
/// page, mark it, and pivot the panel into the revision loop on that entry.
pub(crate) fn save_selected_exchange(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat.revision_of.is_some() {
        // Already saved: `s` re-confirms (row is persisted on every
        // successful revision); just toast.
        crate::input::navigation::show_chapter_toast_secs(&s, "Entry is saved", 2);
        return;
    }
    let idx = s.chat.cursor;
    // The FIRST Q&A auto-saves on arrival WITHOUT arming revision_of, so its
    // `saved_id` is set while `revision_of` is None. Pressing `s` on it must not
    // insert a SECOND journal row — short-circuit on the exchange's own
    // saved_id, not only on revision_of.
    if s.chat.exchanges.get(idx).and_then(|e| e.saved_id).is_some() {
        crate::input::navigation::show_chapter_toast_secs(&s, "Entry is saved", 2);
        return;
    }
    let Some(e) = s.chat.exchanges.get(idx) else { return };
    let (q, a) = (e.question.clone(), e.answer.clone());
    match persist_exchange_to_journal(&mut s, idx) {
        Some(id) => {
            // render_saved_entry below always shows the just-saved exchange
            // directly (it bypasses transcript_rows/journal_view_rows/
            // build_single_exchange_rows entirely — see its own doc
            // comment), so line up `view` with what is actually on screen:
            // if `s` was pressed while looking at the Journal OR Question
            // view, the panel is about to show Gloss-shaped content, and a
            // later `t` should read as "leaving Gloss", not "leaving a
            // Journal/Question view that's no longer what's rendered".
            s.chat.view = PanelView::Gloss;
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
                s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg, false);
            }
            render_saved_entry(&s, &q, &a);
            crate::input::navigation::show_chapter_toast_secs(&s, crate::input::navigation::TOAST_SAVED, 2);
        }
        None => {
            crate::input::navigation::show_chapter_toast_secs(&s, "Save failed", 3);
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
            "Work: \"{}\" by {}\nThis conversation is filed under a PASSAGE in {}\n\nPassage:\n{}\n\nConversation:\n{}Consolidate this conversation into a single cohesive journal Q&A: one question capturing what the conversation was really asking, one answer synthesizing its insights (drop dead ends, false starts, and meta-chatter). Return the consolidated Q&A in exactly this format:\nQ: <question>\nA: <answer>",
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
        // Same reasoning as submit_chat_prompt's pending-start: consolidation
        // reads/renders `exchanges` (the Gloss view's axis), so switch back
        // to Gloss even if the reader triggered this from Journal view.
        s.chat.view = PanelView::Gloss;
        let (mut rows, _, _) = transcript_rows(&s);
        rows.push(crate::ui::chat_panel::TranscriptRow::Thinking);
        let (fam, sz) = transcript_font(&s);
        s.chat_panel.render_rows(&rows, &fam, sz);
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
                    // render_saved_entry below bypasses transcript_rows (it's
                    // a standalone 3-row revision view, no accent bar), but
                    // snap row_cursor anyway so a later j/k step (which DOES
                    // go through transcript_rows) starts from the exchange
                    // that's actually now selected, not a stale row index.
                    snap_row_cursor_to_exchange(&mut s);
                    s.chat.revision_of = Some(id);
                    render_saved_entry(&s, &q, &a);
                    let (title, hint) = prompt_title_hint(&s);
                    s.chat_panel.open_input(title, hint, &s.theme.cursor_bg, &s.theme.cursor_fg, false);
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
                    render_transcript(&mut s);
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
///
/// `question` empty means the saved exchange was a reader-gloss (see
/// `answer_row`'s doc comment) — its `answer` is raw markup, so it must
/// render through `GlossAnswer`. This path IS reachable for a gloss: `s` on
/// the transcript saves whichever exchange the cursor is on, and the gloss
/// lives at slot #1.
pub(crate) fn render_saved_entry(s: &AppState, question: &str, answer: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let answer_row = if question.is_empty() {
        R::GlossAnswer(answer.to_string())
    } else {
        R::Answer(answer.to_string())
    };
    // Show the saved entry scrolled to the very top (Q: line first), so a long
    // answer doesn't land the viewport mid-answer. No row cursor: this static
    // snapshot isn't the j/k-navigable transcript.
    let (fam, sz) = transcript_font(s);
    s.chat_panel
        .render_rows_to_top(&[R::SavedMark, question_row(question), answer_row], &fam, sz);
}

/// Size and position the panel for the current placement. Pinned: fill the
/// freed left space at the card's height. Float: cover the chosen reading
/// column exactly (live compute_bounds rect, window coords — the overlay
/// child's margin_start is relative to the window-filling outer overlay).
///
/// Both placements top-align (`valign: Start`) so the transcript's first line
/// lands at the same height as the card's first reading line. FLOAT does it to
/// clear the running-head / act-scene band (`TOP_SPACER_HEIGHT`) it would
/// otherwise paint over — it sits inside the card's own x-range, covering one
/// reading COLUMN (`scrolled_overlay`/`right_scrolled_overlay`). PINNED sits in
/// a column entirely to the LEFT of the whole card (it never overlaps the
/// header), so it isn't clearing anything — it top-aligns purely so its first
/// line reads level with the card's first line rather than floating centered.
/// The two branches differ only in the inset they back out of the top margin:
/// float aligns the container top with the column start; pinned additionally
/// subtracts `CHAT_PANEL_TOP_INSET` so the first LINE (not the container edge)
/// aligns.
pub(crate) fn size_panel(s: &AppState) {
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    match s.chat_placement {
        ChatPlacement::Pinned => {
            let ww = s.window.width().max(0);
            // Card is pinned flush LEFT with no right margin (see
            // apply_card_sizing's chat branch); the panel abuts it 1px to the
            // right — that 1px is the hairline seam, painted crisp by the
            // panel's own border-left (.chat-panel-pinned). The panel then fills
            // to the right outer margin, so card+panel read as one cream block.
            let start = crate::app::layout::CARD_OUTER_MARGIN + card_w + PINNED_DIVIDER_W;
            let w = (ww - crate::app::layout::CARD_OUTER_MARGIN - start).max(0);
            s.chat_panel.container.set_margin_start(start);
            // Pinned panel is a CARD beside the reading card, so match the reading
            // card's box exactly: top edge at the card's top outer margin, full
            // `card_h` height (bottoms already aligned). This makes the two cards
            // the same height. (The FLOAT branch instead aligns the panel's first
            // LINE with the reading column via `chat_first_line_top_margin`,
            // because it overlays a column and must clear the running-head band;
            // the pinned panel sits in its own column and has no band to clear.)
            let top_margin = crate::app::layout::CARD_VERTICAL_OUTER_MARGIN;
            let panel_h = card_h.max(0);
            s.chat_panel.container.set_valign(gtk4::Align::Start);
            s.chat_panel.container.set_margin_top(top_margin);
            s.chat_panel.container.remove_css_class("chat-panel-float");
            s.chat_panel.container.add_css_class("chat-panel-pinned");
            // Square the card's right corners so it meets the panel's hairline
            // seam flush (no rounded sliver of root at top-right/bottom-right).
            s.page_turn_overlay.add_css_class("card-chat-seam");
            s.chat_panel.size_to(w, panel_h);
        }
        ChatPlacement::FloatLeft | ChatPlacement::FloatRight => {
            let (x, w) = crate::app::layout::column_float_rect(
                s,
                s.chat_placement == ChatPlacement::FloatRight,
            );
            s.chat_panel.container.set_margin_start(x.max(0));
            s.chat_panel.container.remove_css_class("chat-panel-pinned");
            s.chat_panel.container.add_css_class("chat-panel-float");
            // Float placement leaves the card alone (it overlays a column), so
            // the card keeps all four rounded corners.
            s.page_turn_overlay.remove_css_class("card-chat-seam");
            // Clear the header band: `valign: Center` + full `card_h` used to
            // put the panel's top edge at the CARD's own top edge (same y as
            // `content_hbox`), painting over the running-head/act-scene
            // strip (`TOP_SPACER_HEIGHT` = 74px) that sits above the columns
            // in `card_vbox`. Switch to `valign: Start` with an explicit top
            // margin. The reference point is the reading column's first-line
            // ink: `CARD_VERTICAL_OUTER_MARGIN` (content_hbox's top outer
            // margin) + `TOP_SPACER_HEIGHT` (the header band's height) +
            // `line_spacing` (the text view's `pixels_above_lines`, applied
            // above every line incl. the first). Back out `CHAT_PANEL_TOP_INSET`
            // (the panel's own .chat-panel padding + .chat-transcript
            // padding-top) so the transcript's FIRST LINE — not the panel
            // container's top edge — lands level with that reading line
            // (`chat_first_line_top_margin`, shared with the Pinned branch).
            let top_margin = chat_first_line_top_margin(s.config.line_spacing as i32);
            // The two-column DIVIDER stops short of the card's bottom by its own
            // bottom margin (`two_column_divider_bottom_px`), so a panel that
            // fills to the card bottom would drop its left border BELOW the
            // divider's end. Shrink the height by that same inset so the panel's
            // bottom edge lands on the divider's, not the card's. `top_margin`
            // (measured from the card's TOP outer margin, since the container
            // sits in the window-filling outer_overlay at CARD_VERTICAL_OUTER_MARGIN)
            // consumes the space above the panel; the remaining height is what's
            // left between it and the divider end.
            let divider_inset =
                crate::input::scroll::two_column_divider_bottom_px(&s.text_view);
            let panel_h = (card_h
                - (top_margin - crate::app::layout::CARD_VERTICAL_OUTER_MARGIN)
                - divider_inset)
                .max(0);
            s.chat_panel.container.set_valign(gtk4::Align::Start);
            s.chat_panel.container.set_margin_top(top_margin);
            s.chat_panel.size_to(w, panel_h);
        }
    }
}

/// Task 6: re-paginate + re-render the panel's CURRENT view after a height
/// change (a resize tick already re-ran `size_panel`, changing the transcript
/// budget) so the page slice is recomputed for the new budget. Re-rendering
/// through the normal per-view path (`render_transcript` /
/// `render_journal_view` / `render_current_question`) recomputes
/// `s.chat.pages` and CLAMPS `page_idx`/`row_cursor` into range via
/// `render_paginated`, so the cursor's page stays valid even if the new,
/// smaller budget dropped a page. No-op when the panel isn't open.
pub(crate) fn repaginate_current_view(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    match s.chat.view {
        PanelView::Gloss => render_transcript(s),
        PanelView::Journal => render_journal_view(s),
        PanelView::Question => render_current_question(s),
    }
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
/// The user turn a question-less auto-gloss exchange stands in for, so history
/// never carries an empty user turn (which the API rejects). Grounded in the
/// passage the exchange still holds; falls back to a bare instruction if the
/// source is somehow empty, since the return must never itself be empty.
fn gloss_history_user_turn(e: &Exchange) -> String {
    let src = e.source_markup.trim();
    if src.is_empty() {
        "Provide a reader's gloss of the preceding passage.".to_string()
    } else {
        format!("Provide a reader's gloss of this passage:\n\n{src}")
    }
}

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
            // An EMPTY instruction is not an error: like the journal overlay's
            // `R` ("Ctrl+Enter with NO instruction rewrites the answer afresh
            // under the default prompt"), it re-runs the rewrite on the saved
            // Q&A with no custom steering — `rewrite_user_message` puts the
            // instruction at the end, so an empty one reads as "revise afresh".
            let instruction = s.chat_panel.take_input_text().trim().to_string();
            let Some(e) = s.chat.exchanges.iter().find(|e| e.saved_id == Some(id)) else {
                return;
            };
            let Some(work) = s.current_work.as_ref() else { return };
            let scene = crate::app::scene_synopsis::synopsis_label(&s, e.div1, e.div2);
            let context = format!(
                "Work: \"{}\" by {}\nThis Q&A is filed under a PASSAGE in {}\n\nPassage:\n{}\n\nReturn the revised Q&A in exactly this format:\nQ: <revised question>\nA: <revised answer>",
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
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 0);
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
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 0);
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
        let (rows, cursor_row, row_owner) = build_transcript_rows(&exchanges, 1);
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
        let (rows, _cursor_row, row_owner) = build_transcript_rows(&[e], 0);
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
        let (rows, _cursor_row, _row_owner) = build_transcript_rows(&exchanges, 0);
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

/// `build_single_exchange_rows` — the `PanelView::Question` focused render's
/// pure seam (one exchange -> its rows, no gloss, no other exchange, no
/// chip). House style per `panel_view_toggle_tests`/`consolidate_tests`
/// above: pure functions, no `AppState`/GTK. The actual GTK paint
/// (`render_current_question`'s `render_rows` call) is not unit-testable and
/// is exercised only by manual/headless on-screen verification.
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
        let rows = build_single_exchange_rows(&e);
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
        let rows = build_single_exchange_rows(&e);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            R::GlossAnswer(markup) => assert!(markup.contains("YORK")),
            _ => panic!("row 0 must be a GlossAnswer row for an empty-question exchange"),
        }
    }

    #[test]
    fn saved_exchange_appends_saved_mark() {
        let e = qa_exchange("Q?", "A.", Some(42));
        let rows = build_single_exchange_rows(&e);
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0], R::Question(_)));
        assert!(matches!(rows[1], R::Answer(_)));
        assert!(matches!(rows[2], R::SavedMark));
    }
}

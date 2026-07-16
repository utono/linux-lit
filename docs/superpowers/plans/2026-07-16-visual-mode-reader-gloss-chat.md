# Visual-mode Reader Gloss in the Chat Panel — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `-` in visual mode to auto-gloss the selected passage into the existing chat panel, with `r`/`R` to regloss and `Ctrl+n`/`Ctrl+p` to cycle a passage's stored glosses.

**Architecture:** Compose existing parts rather than build new ones. The panel is the existing chat panel (it already floats over a column); the Claude call goes through `claude_bridge::run_claude_request`; persistence is `db::queries::save_gloss`. Four new keys, one new DB ordering tiebreak, and new gloss-cycling state on `ChatState`. No new widget, no schema change.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite/SQLite, glib async + Tokio handle.

**Spec:** `docs/superpowers/specs/2026-07-16-visual-mode-reader-gloss-chat-design.md`

## Global Constraints

- **Build/verify with `cargo build` and `cargo test --bins`. Do NOT run `cargo run`** — the user launches the app. (CLAUDE.md)
- **`persist_render_install_gloss` (`gloss.rs:1328`) is NOT reusable here.** Despite the name it drives the *gloss overlay*: it calls `show_gloss_with_color`/`set_position`/`set_citation` and sets `gloss_list`, `gloss_index`, `gloss_context`, `record_last_gloss`. Calling it from the chat panel would throw the user out of the panel into the gloss overlay. Use `db::queries::save_gloss` directly.
- **Refresh the glossed-line tint with `crate::app::apply_reader_gloss_highlighting(&mut s)`** — the panel stays open, so recompute directly, never via a return-to-reader path (which would wrongly switch input mode). Precedent: `save_selected_exchange`, `chat.rs:615`.
- **Panel keys are arms in `handle_chat_transcript_key` (`keymap.rs:1326`), NOT binds in `keymap_config.rs`.** Reader-level `r` (`VocabPopupTap`), `R` (unbound), `Ctrl+n`/`Ctrl+p` (`VocabJournalPageNext`/`PagePrev`) must stay exactly as they are, with their tests passing untouched.
- **Plain `-` in the reader stays unbound.** The test at `keymap_config.rs:511` asserting `plain("minus") == None` must keep passing. `Ctrl+-`/`Ctrl+Shift+-` keep the vocab loop; `keymap.json` is not edited.
- **Gloss spans are citation-based** (`passages.start_citation`/`end_citation`), never line numbers. Buffer lines reach work lines via `state.work_line_for_buffer(buf_line)`.
- **Gloss type string is the literal `"reader-gloss"`.**
- Every borrow of `AppState` inside a GTK signal/async callback must avoid overlapping with an outer borrow — follow the existing `{ let s = state_rc.borrow(); ... }` scoping in `chat.rs`.

## File Structure

- **`src/db/queries.rs`** (modify) — add the `g.id DESC` tiebreak to `find_glosses_by_start`; add its ordering test.
- **`src/input/actions/chat.rs`** (modify) — new gloss-cycling state on `ChatState`; the `-` handler's panel-side work (auto-gloss submit, regloss, cycling); placement threading.
- **`src/input/visual.rs`** (modify) — the `-` entry point: build the gloss context from the selection, hand off to chat.
- **`src/input/keymap.rs`** (modify) — the `-` arm in `handle_visual_key`; `r`/`R` and `Ctrl+n`/`Ctrl+p` arms in `handle_chat_transcript_key`.

Task order is dependency order: the DB fix (Task 1) is independent and testable alone; placement (Task 2) is independent; the `-` handler (Tasks 3–4) depends on neither but is the core; regloss (Task 5) and cycling (Task 6) build on Task 4's state.

---

### Task 1: Deterministic newest-gloss ordering

`find_glosses_by_start` orders by `g.timestamp DESC`, but `glosses.timestamp` is written by SQLite's `CURRENT_TIMESTAMP` at **one-second granularity**. Two glosses saved in the same second tie, and SQLite may return either — exactly what reglossing twice quickly does. Add `g.id DESC` (monotonic, from `last_insert_rowid()`) as the final tiebreak.

**Files:**
- Modify: `src/db/queries.rs:2169` (the ORDER BY inside `find_glosses_by_start`)
- Test: `src/db/queries.rs` (new `#[cfg(test)] mod` at end of file)

**Interfaces:**
- Consumes: nothing.
- Produces: `find_glosses_by_start(conn, work_abbrev, start_citation, gloss_types) -> Result<Vec<SavedGloss>, rusqlite::Error>` — unchanged signature; `SavedGloss { gloss_id, passage_id, gloss_text, timestamp, gloss_type, start_citation, end_citation }`. Guarantees index 0 is the newest `reader-gloss` even when timestamps tie. Tasks 4 and 6 depend on this ordering.

- [ ] **Step 1: Write the failing test**

Append to `src/db/queries.rs`. Note the fixture must include `timestamp` and `claude_model` columns (the older fixture at `queries.rs:4186` omits them — this query orders on `timestamp`, so it needs them).

```rust
#[cfg(test)]
mod gloss_ordering_tests {
    use super::*;

    /// Two reader-glosses on ONE passage sharing a timestamp: `CURRENT_TIMESTAMP`
    /// has one-second granularity, so reglossing twice in a second ties. The
    /// newest (highest id) must still win.
    #[test]
    fn same_timestamp_glosses_order_newest_id_first() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER,
                claude_model TEXT, timestamp TEXT
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, 'h', 'Err', 'Err.2.2.1', 'Err.2.2.12', 2, 2, 'Antipholus', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id, claude_model, timestamp)
                VALUES (1, 1, 'reader-gloss', 'older', 'complete', NULL, 'm', '2026-07-16 10:00:00'),
                       (2, 1, 'reader-gloss', 'newer', 'complete', NULL, 'm', '2026-07-16 10:00:00');",
        ).unwrap();

        let gs = find_glosses_by_start(&conn, "Err", "Err.2.2.1", &["reader-gloss"]).unwrap();
        assert_eq!(gs.len(), 2);
        assert_eq!(gs[0].gloss_text, "newer");
        assert_eq!(gs[0].gloss_id, 2);
        assert_eq!(gs[1].gloss_text, "older");
    }

    /// The pre-existing ordering rules must survive the new tiebreak:
    /// reader-gloss outranks other types, and a newer timestamp still wins.
    #[test]
    fn reader_gloss_and_timestamp_still_outrank_id() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE passages (
                id INTEGER PRIMARY KEY, hash TEXT, work_abbrev TEXT,
                start_citation TEXT, end_citation TEXT, div1 INTEGER, div2 INTEGER,
                character TEXT, source_text TEXT
             );
             CREATE TABLE glosses (
                id INTEGER PRIMARY KEY, passage_id INTEGER, gloss_type TEXT,
                gloss_text TEXT, status TEXT, word_id INTEGER,
                claude_model TEXT, timestamp TEXT
             );
             INSERT INTO passages (id, hash, work_abbrev, start_citation, end_citation, div1, div2, character, source_text)
                VALUES (1, 'h', 'Err', 'Err.2.2.1', 'Err.2.2.12', 2, 2, 'Antipholus', 'text');
             INSERT INTO glosses (id, passage_id, gloss_type, gloss_text, status, word_id, claude_model, timestamp)
                VALUES (9, 1, 'teacher-generic', 'teacher', 'complete', NULL, 'm', '2026-07-16 12:00:00'),
                       (1, 1, 'reader-gloss', 'old-reader', 'complete', NULL, 'm', '2026-07-16 10:00:00'),
                       (2, 1, 'reader-gloss', 'new-reader', 'complete', NULL, 'm', '2026-07-16 11:00:00');",
        ).unwrap();

        let gs = find_glosses_by_start(
            &conn, "Err", "Err.2.2.1", &["teacher-generic", "reader-gloss"],
        ).unwrap();
        assert_eq!(gs.len(), 3);
        // reader-gloss first (despite the teacher gloss having the newest timestamp)
        assert_eq!(gs[0].gloss_text, "new-reader");
        assert_eq!(gs[1].gloss_text, "old-reader");
        assert_eq!(gs[2].gloss_text, "teacher");
    }
}
```

- [ ] **Step 2: Run test to verify the tie test fails**

```bash
cargo test --bins gloss_ordering_tests -- --nocapture
```

Expected: `same_timestamp_glosses_order_newest_id_first` FAILS (it may pass by luck of SQLite's row order — if it passes, verify the fix still makes it deterministic and note it). `reader_gloss_and_timestamp_still_outrank_id` PASSES already (it documents behavior the fix must preserve).

- [ ] **Step 3: Add the tiebreak**

In `src/db/queries.rs:2169`, change the ORDER BY. Before:

```rust
         ORDER BY (g.gloss_type = 'reader-gloss') DESC, g.timestamp DESC",
```

After:

```rust
         ORDER BY (g.gloss_type = 'reader-gloss') DESC, g.timestamp DESC, g.id DESC",
```

- [ ] **Step 4: Run tests to verify both pass**

```bash
cargo test --bins gloss_ordering_tests -- --nocapture
```

Expected: both PASS.

- [ ] **Step 5: Verify no existing gloss test regressed**

```bash
cargo test --bins gloss && cargo test --bins passages
```

Expected: PASS. The change strictly refines an ordering that was arbitrary within a tie, so no caller's behavior should change.

- [ ] **Step 6: Commit**

```bash
git add src/db/queries.rs
git commit -m "fix(db): break find_glosses_by_start timestamp ties by id DESC

glosses.timestamp is CURRENT_TIMESTAMP at one-second granularity, so two
glosses on one passage saved in the same second tie under timestamp DESC and
SQLite may return either. Reglossing twice quickly hits exactly that. id is
last_insert_rowid() and so monotonic per insert."
```

---

### Task 2: Both-column selections float the panel left

`float_side_for_cursor` floats the panel over the column the cursor is *not* in. A selection spanning both columns defeats that: either side covers half of it. Rule: spanning → `FloatLeft`; within one column → today's behavior.

This also fixes an ordering defect: `open_chat_pinned_to_selection` calls `exit_visual_mode` (`chat.rs:228`) **before** `toggle_chat_layout` (`:231`), and `toggle_chat_layout` picks the side via `float_side_for_cursor(s)`, which reads `s.current_line` — so placement is currently derived from the cursor *after* the selection is cleared. Invisible today (both ends agree with the cursor for a within-column selection), but it makes the spanning rule unimplementable as written.

**Files:**
- Modify: `src/input/actions/chat.rs` (add `float_side_for_range`; use it in `open_chat_pinned_to_selection`)
- Test: `src/input/actions/chat.rs` (new `#[cfg(test)] mod` at end of file)

**Interfaces:**
- Consumes: `line_in_right_column(line: usize, split: Option<usize>, end: usize) -> bool` — the existing free function `cursor_in_right_column` calls (`chat.rs:132,139`). `ChatPlacement::{FloatLeft, FloatRight, Pinned}`.
- Produces: `fn placement_for_range(start: usize, end: usize, split: Option<usize>, page_end: usize) -> ChatPlacement` — pure, no `AppState`, so it is unit-testable. Used by `open_chat_pinned_to_selection`.

- [ ] **Step 1: Write the failing test**

Append to `src/input/actions/chat.rs`:

```rust
#[cfg(test)]
mod placement_tests {
    use super::*;

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
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --bins placement_tests -- --nocapture
```

Expected: FAIL — `cannot find function 'placement_for_range' in this scope`.

- [ ] **Step 3: Add `placement_for_range`**

Add to `src/input/actions/chat.rs`, immediately after `float_side_for_cursor` (which ends at `:149`):

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --bins placement_tests -- --nocapture
```

Expected: PASS (5 tests).

- [ ] **Step 5: Apply the placement in `open_chat_pinned_to_selection`**

The selection is cleared by `exit_visual_mode` before `toggle_chat_layout` picks a side, so compute the placement **while the selection still exists** and apply it after the panel opens. In `src/input/actions/chat.rs:213-236`, change the capture block and add the placement application.

Before (`:214-219`):

```rust
    let picked = {
        let s = state_rc.borrow();
        let Some(sel) = s.visual_selection.as_ref() else { return };
        let (start, end) = sel.range();
        crate::input::segments::selection_context(&s, start, end).map(|ctx| (ctx, start, end))
    };
```

After:

```rust
    let picked = {
        let s = state_rc.borrow();
        let Some(sel) = s.visual_selection.as_ref() else { return };
        let (start, end) = sel.range();
        // Placement MUST be computed here, while the selection still exists:
        // exit_visual_mode below clears it, and toggle_chat_layout then picks a
        // side from s.current_line alone — which cannot see a spanning range.
        let placement = placement_for_selection(&s, start, end);
        crate::input::segments::selection_context(&s, start, end)
            .map(|ctx| (ctx, start, end, placement))
    };
```

Then update the destructure (`:220`):

```rust
    let Some((pinned, start, end, placement)) = picked else {
```

And after `toggle_chat_layout(state_rc);` (`:231`), override the cursor-derived side. Before:

```rust
    toggle_chat_layout(state_rc);
    state_rc.borrow_mut().chat.pinned_passage = Some(pinned);
```

After:

```rust
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
```

Note: the following `let s = state_rc.borrow();` at old `:233` still works — the block above drops its mutable borrow.

- [ ] **Step 6: Verify it builds and all tests pass**

```bash
cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5
```

Expected: build succeeds; all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "fix(chat): place the pinned panel from the selection, not the cursor

open_chat_pinned_to_selection exits visual mode before toggle_chat_layout
picks a float side from s.current_line, so placement was derived after the
selection was gone. Compute it while the selection lives; a selection
spanning both columns floats LEFT (neither side keeps it visible)."
```

---

### Task 3: The `-` bind reaches a handler

Wire the key first, with a stub that only opens the pinned panel (Task 4 adds the gloss). This keeps the bind change independently reviewable.

**Files:**
- Modify: `src/input/keymap.rs` (new arm in `handle_visual_key`, beside `"a" if is_ctrl` at `:3253` and `"Tab"` at `:3261`)
- Modify: `src/input/visual.rs` (new `action_reader_gloss_chat`)

**Interfaces:**
- Consumes: `chat::open_chat_pinned_to_selection(state_rc: &Rc<RefCell<AppState>>)` (`chat.rs:213`, Task 2's version).
- Produces: `pub(crate) fn action_reader_gloss_chat(state_rc: &Rc<RefCell<AppState>>)` in `src/input/visual.rs`. Task 4 fills in its body.

- [ ] **Step 1: Add the stub handler**

Add to `src/input/visual.rs`, immediately after `action_reader_gloss` (find its end with `rg -n "fn action_reader_gloss" src/input/visual.rs`):

```rust
/// `-` in visual mode: open the chat panel pinned to the selection and gloss
/// the passage immediately — no ask input. Sibling to `Ctrl+a` (Journal Q&A
/// ask card) and `Tab` (chat pinned, empty input) on the same select-then-act
/// flow.
pub(crate) fn action_reader_gloss_chat(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    // Pins the passage, exits visual mode, opens and places the panel. Bails
    // with its own toast when the selection has no passage, or when a
    // single-column layout has no room for the panel.
    crate::input::actions::chat::open_chat_pinned_to_selection(state_rc);
}
```

- [ ] **Step 2: Add the visual-mode key arm**

In `src/input/keymap.rs`, in `handle_visual_key`, add beside the existing `"Tab" | "ISO_Left_Tab"` arm (`:3261`):

```rust
        "minus" => {
            crate::input::visual::action_reader_gloss_chat(state);
            true
        }
```

- [ ] **Step 3: Verify reader-level minus is untouched**

```bash
cargo test --bins keymap -- --nocapture
```

Expected: PASS — in particular the existing `assert_eq!(m.get(&KeyCombo::plain("minus")), None)` (`keymap_config.rs:511`) and the `Ctrl+minus → JumpToNextVocab` assertions still hold. This task adds no entry to `keymap_config.rs`: visual mode is a modal handler, not a keymap bind.

- [ ] **Step 4: Verify it builds**

```bash
cargo build 2>&1 | tail -5
```

Expected: build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/visual.rs
git commit -m "feat(visual): '-' opens the chat panel pinned to the selection

Modal visual-mode arm beside Ctrl+a and Tab; reader-level minus stays
unbound. The auto-gloss follows."
```

---

### Task 4: `-` auto-glosses into the panel

Fill in `action_reader_gloss_chat`: build the gloss context from the selection, check the cache, and on a miss call Claude with `READER_GLOSS_PROMPT` and save the result on arrival. The ask input never opens.

**Files:**
- Modify: `src/input/visual.rs` (`action_reader_gloss_chat` body)
- Modify: `src/input/actions/chat.rs` (new `ChatState` fields; `push_gloss_exchange`; `save_reader_gloss`)

**Interfaces:**
- Consumes: `gloss::build_context_for_type(work, &[Line], "reader-gloss") -> Option<GlossContext>`; `GlossContext { hash, work_abbrev, start_citation, end_citation, act, scene, speaker, source_text, .. }` with `.source_line_pairs()`; `gloss::build_user_message(&ctx, Option<&str>, Option<&str>) -> String`; `gloss::READER_GLOSS_PROMPT`; `db::queries::find_glosses_by_start` (Task 1's ordering); `db::queries::save_gloss(conn, hash, work_abbrev, start_citation, end_citation, act, scene, character, source_text, gloss_text, gloss_type, claude_model) -> Result<i64, _>`; `db::queries::open_db()` / `open_db_rw()`; `claude_bridge::run_claude_request(state_rc, system_prompt: String, user_msg: String, model: String, on_success: impl Fn(&Rc<RefCell<AppState>>, String) + 'static, on_error: impl Fn(&Rc<RefCell<AppState>>, &str) + 'static)`; `chat::render_transcript(&AppState)`; `chat::focus_transcript(&mut AppState)`; `crate::app::apply_reader_gloss_highlighting(&mut AppState)`; `Exchange { question, answer, chip, user_msg, div1, div2, start_citation, end_citation, source_markup, saved_id }`.
- Produces on `ChatState`: `pub gloss_list: Vec<crate::db::queries::SavedGloss>`, `pub gloss_index: usize`, `pub gloss_ctx: Option<crate::gloss::GlossContext>`. Produces `pub(crate) fn push_gloss_exchange(s: &mut AppState, ctx: &crate::gloss::GlossContext, gloss_text: &str)` and `pub(crate) fn save_reader_gloss(s: &mut AppState, ctx: &crate::gloss::GlossContext, gloss_text: &str, model: &str) -> Option<i64>`. Tasks 5 and 6 consume all of these.

- [ ] **Step 1: Add the gloss-cycling state to `ChatState`**

In `src/input/actions/chat.rs:40-52`, add three fields to `ChatState` (it derives `Default`; `Vec`/`usize`/`Option` all default correctly, so no other change is needed):

```rust
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
```

- [ ] **Step 2: Add `push_gloss_exchange` and `save_reader_gloss`**

Add to `src/input/actions/chat.rs`, after `render_transcript` (`:522-525`):

```rust
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
    s.chat.gloss_index = new_id
        .and_then(|id| s.chat.gloss_list.iter().position(|g| g.gloss_id == id))
        .unwrap_or(0);

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
```

- [ ] **Step 3: Fill in `action_reader_gloss_chat`**

Replace the Task 3 stub in `src/input/visual.rs` with the full handler:

```rust
/// `-` in visual mode: open the chat panel pinned to the selection and gloss
/// the passage immediately — no ask input. Sibling to `Ctrl+a` (Journal Q&A
/// ask card) and `Tab` (chat pinned, empty input) on the same select-then-act
/// flow.
///
/// On a cache hit the stored gloss is shown and NO API call is made, so
/// pressing `-` twice on a passage is cheap. `r`/`R` in the panel is the way
/// to force a fresh gloss.
pub(crate) fn action_reader_gloss_chat(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    // Build the gloss context BEFORE opening the panel: open_chat_pinned_to_selection
    // exits visual mode, which clears the selection this reads.
    let prepared = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state
                    .work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();
        match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(ctx) => Some((ctx, state.config.claude_model.clone())),
            None => None,
        }
    };
    let Some((ctx, model)) = prepared else { return };

    // Pins the passage, exits visual mode, opens and places the panel. Bails
    // with its own toast when the selection has no passage, or when a
    // single-column layout has no room for the panel.
    crate::input::actions::chat::open_chat_pinned_to_selection(state_rc);
    {
        let s = state_rc.borrow();
        if !s.chat_layout_open {
            return; // no room for the panel; its toast already explained
        }
    }

    let cached = crate::input::actions::chat::reload_gloss_list(&ctx.work_abbrev, &ctx.start_citation);
    {
        let mut s = state_rc.borrow_mut();
        s.chat.gloss_ctx = Some(ctx.clone());
        s.chat.gloss_list = cached;
        s.chat.gloss_index = 0;
    }

    // Cache hit: show the newest stored gloss, spend no API call.
    let hit = {
        let s = state_rc.borrow();
        s.chat.gloss_list.first().map(|g| g.gloss_text.clone())
    };
    if let Some(text) = hit {
        let mut s = state_rc.borrow_mut();
        crate::input::actions::chat::push_gloss_exchange(&mut s, &ctx, &text);
        crate::input::actions::chat::focus_transcript(&mut s);
        crate::logging::log("CHAT-GLOSS: showing cached gloss");
        return;
    }

    crate::input::actions::chat::request_reader_gloss(state_rc, ctx, model);
}
```

- [ ] **Step 4: Add `request_reader_gloss` (the shared submit)**

`-` (on a cache miss) and `r`/`R` (Task 5) both call this. Add to `src/input/actions/chat.rs`, after `save_reader_gloss`:

```rust
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
        save_reader_gloss(&mut s, &ctx_ok, &reply, &model_for_db);
        push_gloss_exchange(&mut s, &ctx_ok, &reply);
        focus_transcript(&mut s);
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

/// The transcript with a "Glossing…" row appended, so the panel shows work in
/// flight rather than sitting blank.
fn render_transcript_thinking_gloss(s: &AppState) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let (mut rows, _) = transcript_rows(s);
    rows.push(R::Chip("Reader gloss".to_string()));
    rows.push(R::Thinking);
    s.chat_panel.render_rows(&rows);
}
```

- [ ] **Step 5: Verify it builds**

```bash
cargo build 2>&1 | tail -20
```

Expected: build succeeds. Verified signatures this code depends on: `GlossContext` derives `Clone` (`gloss.rs:555`); `READER_GLOSS_PROMPT` is `LazyLock<String>` (`gloss.rs:430`); `transcript_rows(&AppState) -> (Vec<TranscriptRow>, usize)` is private to `chat.rs` (`:499`) and `focus_transcript(&mut AppState)` is `pub(crate)` (`:329`) — both reachable from these call sites; `TranscriptRow::{Chip, Thinking}` exist (`chat_panel.rs:13,15`); `apply_reader_gloss_highlighting(&mut AppState)` is `pub` (`app/mod.rs:4478`).

- [ ] **Step 6: Run the full test suite**

```bash
cargo test --bins 2>&1 | tail -5
```

Expected: PASS — no existing test should change.

- [ ] **Step 7: Commit**

```bash
git add src/input/visual.rs src/input/actions/chat.rs
git commit -m "feat(chat): '-' auto-glosses the selection into the chat panel

Fires READER_GLOSS_PROMPT with no ask input, saves to passages+glosses on
arrival, and shows the gloss in transcript slot #1. A cached gloss for the
span short-circuits the API call. Saves via save_gloss directly, not
persist_render_install_gloss, which drives the gloss OVERLAY and would throw
the user out of the panel."
```

---

### Task 5: `r`/`R` reglosses

A panel key: call Claude again on the pinned passage and append a **new** `glosses` row, bypassing the cache that `-` relies on.

**Files:**
- Modify: `src/input/actions/chat.rs` (new `regloss_pinned`)
- Modify: `src/input/keymap.rs` (new arm in `handle_chat_transcript_key`, beside `"a"` at `:1355`)

**Interfaces:**
- Consumes: `request_reader_gloss(state_rc, ctx, model)` and `ChatState::gloss_ctx` (Task 4).
- Produces: `pub(crate) fn regloss_pinned(state_rc: &Rc<RefCell<AppState>>)`.

- [ ] **Step 1: Add `regloss_pinned`**

Add to `src/input/actions/chat.rs`, after `request_reader_gloss`:

```rust
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
```

- [ ] **Step 2: Add the transcript key arm**

In `src/input/keymap.rs`, in `handle_chat_transcript_key`, add beside the existing `"a"` arm (`:1355`):

```rust
        "r" | "R" => {
            crate::input::actions::chat::regloss_pinned(state);
            true
        }
```

- [ ] **Step 3: Verify reader-level r/R are untouched**

```bash
cargo test --bins keymap -- --nocapture
```

Expected: PASS — `plain("r") == VocabPopupTap` (`keymap_config.rs:508`) and `plain("R") == None` (`:514`) still hold. This arm is in the panel's modal handler; `keymap_config.rs` is not edited.

- [ ] **Step 4: Verify it builds and tests pass**

```bash
cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5
```

Expected: build succeeds; tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/chat.rs src/input/keymap.rs
git commit -m "feat(chat): 'r'/'R' reglosses the pinned passage

Always calls Claude, bypassing the cache '-' uses, and appends a new
reader-gloss row — history is kept. Panel key, so reader-level r
(VocabPopupTap) and R (unbound) are unchanged."
```

---

### Task 6: `Ctrl+n`/`Ctrl+p` cycles stored glosses

Cycle the pinned passage's stored glosses, wrapping, swapping transcript slot #1 in place while follow-ups stay put.

**Files:**
- Modify: `src/input/actions/chat.rs` (new `cycle_gloss`)
- Modify: `src/input/keymap.rs` (new arms in `handle_chat_transcript_key`, beside `"l" if is_ctrl` at `:1360`)
- Test: `src/input/actions/chat.rs` (add to the `#[cfg(test)] mod` from Task 2)

**Interfaces:**
- Consumes: `ChatState::{gloss_list, gloss_index, gloss_ctx}` and `push_gloss_exchange` (Task 4).
- Produces: `fn wrap_index(cur: usize, delta: i32, len: usize) -> usize` (pure, testable) and `pub(crate) fn cycle_gloss(s: &mut AppState, delta: i32)`.

- [ ] **Step 1: Write the failing test**

Add to the `placement_tests` module in `src/input/actions/chat.rs`, or as a sibling module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --bins gloss_cycle_tests -- --nocapture
```

Expected: FAIL — `cannot find function 'wrap_index' in this scope`.

- [ ] **Step 3: Add `wrap_index` and `cycle_gloss`**

Add to `src/input/actions/chat.rs`, after `push_gloss_exchange`:

```rust
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
pub(crate) fn cycle_gloss(s: &mut AppState, delta: i32) {
    let n = s.chat.gloss_list.len();
    if n <= 1 {
        return; // nothing to cycle to
    }
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
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --bins gloss_cycle_tests -- --nocapture
```

Expected: PASS (4 tests).

- [ ] **Step 5: Add the transcript key arms**

In `src/input/keymap.rs`, in `handle_chat_transcript_key`, add beside the existing `"l" if is_ctrl` arm (`:1360`):

```rust
        "n" if is_ctrl => {
            crate::input::actions::chat::cycle_gloss(&mut state.borrow_mut(), 1);
            true
        }
        "p" if is_ctrl => {
            crate::input::actions::chat::cycle_gloss(&mut state.borrow_mut(), -1);
            true
        }
```

- [ ] **Step 6: Verify reader-level Ctrl+n/Ctrl+p are untouched**

```bash
cargo test --bins keymap -- --nocapture
```

Expected: PASS — `ctrl("n") == VocabJournalPageNext` and `ctrl("p") == VocabJournalPagePrev` (`keymap_config.rs:308-309`) still hold.

- [ ] **Step 7: Verify it builds and the full suite passes**

```bash
cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5
```

Expected: build succeeds; all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/input/actions/chat.rs src/input/keymap.rs
git commit -m "feat(chat): Ctrl+n/Ctrl+p cycles a passage's stored glosses

Wraps at both ends, swapping transcript slot #1 in place while follow-ups
stay put. A separate axis from j/k, which move over this session's
exchanges; these move over lit.db rows. Panel keys, so reader-level Ctrl+n/p
(VocabJournalPage*) are unchanged."
```

---

### Task 7: On-screen verification

A green build is not "done" for a change with visible behavior (CLAUDE.md). The headless harness cannot exercise the Claude API path, so this task verifies what it can and hands the rest off.

**Files:** none modified.

- [ ] **Step 1: Confirm the full suite and clippy are green**

```bash
cargo test --bins 2>&1 | tail -5 && cargo clippy 2>&1 | tail -15
```

Expected: tests PASS; no new clippy warnings in the touched files.

- [ ] **Step 2: Verify the reader binds are provably untouched**

```bash
cargo test --bins keymap_config -- --nocapture 2>&1 | tail -5
```

Expected: PASS. Specifically `plain("minus") == None`, `ctrl("minus") == JumpToNextVocab`, `plain("r") == VocabPopupTap`, `plain("R") == None`, `ctrl("n") == VocabJournalPageNext`, `ctrl("p") == VocabJournalPagePrev`.

- [ ] **Step 3: Confirm `keymap.json` was not edited**

```bash
git diff --stat HEAD~6 -- ~/tty-dotfiles/linux-lit/ ; git status --short
```

Expected: no changes to `keymap.json` — every new key is a modal handler arm, not a bind.

- [ ] **Step 4: Ask the user how to verify on screen**

Per CLAUDE.md's testing rule, prompt the user to choose: an agent-run headless cage drive, or a manual hand-off. The API-calling paths need a real key and a real work, so a manual pass is likely.

Hand-off steps to offer:

1. Launch via `crll`, open a two-column play.
2. `V`, select several lines **within the left column**, press `-`. Expect: the panel floats **right**, a gloss appears with no input shown, and the passage's lines pick up the gloss tint.
3. Escape/`Ctrl+Tab` to close. `V`, select lines **spanning both columns**, press `-`. Expect: the panel floats **left**.
4. In the panel, press `r`. Expect: a second gloss is generated and replaces slot #1; the chip reads "Reader gloss 1 of 2".
5. Press `Ctrl+n` / `Ctrl+p`. Expect: cycling between the two stored glosses, wrapping, chip tracking.
6. Press `a`, ask a follow-up, press `s`. Expect: the ask input opens, the answer appends **below** the gloss, and `s` saves it to the journal.
7. Reopen the same passage with `-` later. Expect: the cached gloss appears with no API call (check the log for `CHAT-GLOSS: showing cached gloss`).

- [ ] **Step 5: Update the to-do list**

`docs/to-do/to-do.md` is the running list of reader bugs/features (CLAUDE.md). If this work corresponds to an entry, mark it `[X]` — never delete it.

- [ ] **Step 6: Finish the branch**

Per CLAUDE.md, the default is merge back to master locally and push — tests pass, clean tree, `git checkout master`, `git merge --no-ff`, re-verify the build, `git push origin master`, `git branch -d`. Confirm the on-screen verification passed first.

# Journal-Entry Top Landing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a Journal Q&A is created from the chat panel, the panel shows the entry from its top — `Q:` block first, accent-bar cursor on it, scrolled to top — with paragraph-level j/k stepping through the answer.

**Architecture:** All changes are in the chat panel's Question view. The pure row model (`build_single_exchange_rows`) splits answers into per-paragraph rows (mirroring `journal_view_rows`); `render_current_question` switches from cursor-less `render_rows` (last page) to the accent-bar `render_paginated` path (page of the cursor); `render_page` gains a scroll-to-top reset; the Question-view j/k/gg/G scroll-degrade guards become real paged cursor stepping. The journal overlay and gloss overlay already land at top (verified in source) — regression-verify only, no changes there.

**Tech Stack:** Rust, GTK4, existing chat pagination (`step_cursor_paged`, `render_paginated`).

**Spec:** `docs/superpowers/specs/2026-07-20-journal-entry-top-landing-design.md`

## Global Constraints

- No keybind changes — j/k/gg/G are already routed; only their Question-view behavior changes. No `keymap.json`, no Ctrl+/ overlay edits.
- `cargo build` only — NEVER `cargo run` (the user launches the app).
- Reader-gloss exchanges (empty `question`) carry raw `<speaker>`/`<verse>` markup: their answer must stay ONE `GlossAnswer` row — never paragraph-split.
- No source-passage text in any Question-view render (already true; keep it that way).
- Commit after each task; merge to master per house finishing-a-branch flow at the end.

---

### Task 0: Branch

**Files:** none

- [ ] **Step 1: Create the feature branch off master**

```bash
cd ~/utono/linux-lit && git checkout -b feat/journal-entry-top-landing
```

Expected: `Switched to a new branch 'feat/journal-entry-top-landing'`

---

### Task 1: Per-paragraph rows in `build_single_exchange_rows`

**Files:**
- Modify: `src/input/actions/chat_rows.rs:143-154` (`build_single_exchange_rows`)
- Test: same file, new `#[cfg(test)] mod question_view_rows_tests` (place it after `panel_view_toggle_tests`)

**Interfaces:**
- Consumes: existing `split_answer_paragraphs(&str) -> Vec<String>` (`chat_rows.rs:186`), `question_row`, `has_question_row`.
- Produces: `build_single_exchange_rows(e: &Exchange) -> Vec<TranscriptRow>` now returns `[Question, Answer(para)...(one per paragraph), SavedMark?]` for Q&A exchanges; gloss exchanges (empty question) unchanged: `[GlossAnswer]`. Tasks 2/4/5 rely on widget 0 being the `Q:` row for Q&A exchanges.

- [ ] **Step 1: Write the failing tests**

Append to `src/input/actions/chat_rows.rs` (after the `panel_view_toggle_tests` module):

```rust
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
        let rows = build_single_exchange_rows(&ex("Why?", "one\n\ntwo\n\nthree", false));
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[0], R::Question(q) if q == "Q: Why?"));
        assert!(matches!(&rows[1], R::Answer(a) if a == "one"));
        assert!(matches!(&rows[2], R::Answer(a) if a == "two"));
        assert!(matches!(&rows[3], R::Answer(a) if a == "three"));
    }

    #[test]
    fn saved_mark_trails_the_paragraphs() {
        let rows = build_single_exchange_rows(&ex("Why?", "one\n\ntwo", true));
        assert_eq!(rows.len(), 4); // Q + 2 paragraphs + SavedMark
        assert!(matches!(rows.last(), Some(R::SavedMark)));
    }

    #[test]
    fn gloss_exchange_keeps_single_gloss_answer_row() {
        let rows = build_single_exchange_rows(&ex(
            "",
            "<speaker>A</speaker>\n\n<verse>b</verse>",
            false,
        ));
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], R::GlossAnswer(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --bin linux-lit question_view_rows -- --nocapture
```

Expected: FAIL — `qa_answer_splits_into_paragraph_rows` asserts `rows.len() == 4`, current code returns 2 (`Q` + one `Answer`).

- [ ] **Step 3: Implement the split**

Replace `build_single_exchange_rows` (`chat_rows.rs:143-154`) with:

```rust
pub(crate) fn build_single_exchange_rows(e: &Exchange) -> Vec<crate::ui::chat_panel::TranscriptRow> {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    if has_question_row(e) {
        rows.push(question_row(&e.question));
    }
    if e.question.is_empty() {
        // Reader-gloss exchange: raw <speaker>/<verse> markup renders as ONE
        // GlossAnswer row (see answer_row's doc comment) — splitting it would
        // break the markup parse.
        rows.push(R::GlossAnswer(e.answer.clone()));
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
```

Also extend the function's doc comment (`chat_rows.rs:132-142`): change "its `Q:` row (via `has_question_row`/`answer_row`, ...) plus a trailing `SavedMark`" to say the answer renders as one `Answer` row per paragraph via `split_answer_paragraphs` (gloss exchanges stay one `GlossAnswer`), plus a trailing `SavedMark` if saved. `answer_row` itself is untouched (still used by `build_transcript_rows`).

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --bin linux-lit question_view_rows -- --nocapture
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/chat_rows.rs
git commit -m "feat(chat): split Question-view answers into per-paragraph rows"
```

---

### Task 2: `render_current_question` renders page-of-cursor with accent bar; completion callback lands on the `Q:` row

**Files:**
- Modify: `src/input/actions/chat.rs:1036-1053` (`render_current_question` + doc comment)
- Modify: `src/input/actions/chat.rs:874-908` (`submit_chat_prompt` answer callback)

**Interfaces:**
- Consumes: `render_paginated(s, &rows, cursor_widget: Option<usize>, selection)` (`chat.rs:1006`) — computes `s.chat.pages`/`s.chat.page_idx` and renders the page containing the cursor with the accent bar; Task 1's row shape (widget 0 = `Q:`).
- Produces: `render_current_question(s: &mut AppState)` (signature unchanged) now honors `s.chat.row_cursor` in Question-view widget space. Task 5's j/k/gg/G arms set `row_cursor`/`page_idx` and call it.

- [ ] **Step 1: Rewrite `render_current_question`**

Replace `chat.rs:1045-1053` with:

```rust
pub(crate) fn render_current_question(s: &mut AppState) {
    let Some(e) = s.chat.exchanges.get(s.chat.cursor) else {
        let (fam, sz) = transcript_font(s);
        s.chat_panel.render_rows(&[], &fam, sz);
        return;
    };
    let rows = build_single_exchange_rows(e);
    let cursor = Some(s.chat.row_cursor);
    render_paginated(s, &rows, cursor, None);
}
```

Update its doc comment (`chat.rs:1036-1044`): it no longer degrades to plain scrolling — say it renders the single exchange's rows through `render_paginated` at `s.chat.row_cursor` (Question-view widget space: `Q:` row is widget 0), painting the accent bar and making `s.chat.pages`/`page_idx` authoritative for the Question-view j/k/gg/G arms.

- [ ] **Step 2: Land the completion callback on the `Q:` row, after the auto-save**

In the `run_claude_chat_request` success callback (`chat.rs:859-908`), make three changes:

1. Replace `snap_row_cursor_to_exchange(&mut s);` (line 875) — that snaps in Gloss-transcript row space, wrong for the Question-view render — with:

```rust
            // Question view renders over its OWN row space (Q: row = widget
            // 0); land the accent bar there, at the top of the new entry.
            s.chat.row_cursor = 0;
            s.chat.page_idx = 0;
```

2. Move the auto-save block (`if is_first_question_exchange(...) { ... }`, lines 894-908) to BEFORE `render_current_question(&mut s);` so the just-set `saved_id` puts the `SavedMark` row into the first render and `s.chat.pages` match the rows j/k rebuilds later.

3. Keep `debug_assert_eq!`, `render_current_question`, `focus_transcript` order otherwise. The callback body ends up:

```rust
            s.chat.cursor = s.chat.exchanges.len() - 1;
            // Question view renders over its OWN row space (Q: row = widget
            // 0); land the accent bar there, at the top of the new entry.
            s.chat.row_cursor = 0;
            s.chat.page_idx = 0;
            debug_assert_eq!(s.chat.view, PanelView::Question);
            // Auto-save BEFORE the render so the SavedMark row is part of the
            // first render and s.chat.pages match the rows j/k rebuilds.
            // (Auto-save rationale unchanged — see the original comment.)
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
            // Stay in Question view (set at submit) and render ONLY this
            // Q&A — not the whole transcript (render_transcript), which
            // would bring the gloss and any earlier exchanges back above it.
            // `t` still reaches the full gloss transcript from here.
            render_current_question(&mut s);
            // Answer visible: hand focus to the transcript so j/k/s work
            // immediately. The input was already hidden on submit.
            focus_transcript(&mut s);
```

Preserve the existing multi-line comment explaining WHY auto-save fires once per session (lines 885-893) — keep it above the `if`, merged with the new ordering note.

- [ ] **Step 3: Build**

```bash
cargo build
```

Expected: green. (If `snap_row_cursor_to_exchange` is now reported dead: it is NOT — `push_gloss_exchange` and `consolidate_chat` still call it; do not remove it.)

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): land Question view on the Q: row with accent bar"
```

---

### Task 3: `render_page` resets the transcript scroll to top

**Files:**
- Modify: `src/ui/chat_panel.rs:251-295` (`render_page` + doc comment)

**Interfaces:**
- Produces: every `render_page` call now parks the vadjustment at 0.0. All render paths (`render_paginated`, `render_rows`, `render_rows_to_top`) inherit the fix.

- [ ] **Step 1: Add the reset**

In `render_page` (`chat_panel.rs:260`), immediately after `self.rebuild_from_specs(slice);` (line 270) add:

```rust
        // A page normally fits the budget, but a single widget taller than
        // the viewport (an unsplit long answer) DOES scroll — and GTK keeps
        // the previous adjustment value across the child rebuild, so the view
        // inherited whatever scroll position the panel was at. Every page
        // render starts at the top.
        self.transcript_scroll.vadjustment().set_value(0.0);
```

Update the doc comment's last sentence (`chat_panel.rs:258-259`): replace "The vadjustment is NOT touched — the page fits, so there is nothing to scroll." with "The vadjustment is reset to the top on every render — a page normally fits, but an oversized single widget scrolls, and the adjustment would otherwise carry over from the previous render."

- [ ] **Step 2: Build**

```bash
cargo build
```

Expected: green.

- [ ] **Step 3: Commit**

```bash
git add src/ui/chat_panel.rs
git commit -m "fix(chat): reset transcript scroll to top on every page render"
```

---

### Task 4: `render_saved_entry` splits the answer too

**Files:**
- Modify: `src/input/actions/chat.rs:2299-2312` (`render_saved_entry`)
- Modify: `src/input/actions/chat.rs:9-14` (`use super::chat_rows::{...}` — add `split_answer_paragraphs`)

**Interfaces:**
- Consumes: `split_answer_paragraphs` (Task 1's split, already in `chat_rows.rs`).
- Produces: the `s`-save snapshot renders `[SavedMark, Q:, Answer(para)...]` — page 0 now actually contains the answer's opening paragraphs instead of an oversized invisible widget.

- [ ] **Step 1: Rewrite `render_saved_entry`**

Add `split_answer_paragraphs` to the import list at `chat.rs:9-14`, then replace the function body (`chat.rs:2299-2312`) with:

```rust
pub(crate) fn render_saved_entry(s: &AppState, question: &str, answer: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = vec![R::SavedMark, question_row(question)];
    if question.is_empty() {
        rows.push(R::GlossAnswer(answer.to_string()));
    } else {
        rows.extend(split_answer_paragraphs(answer).into_iter().map(R::Answer));
    }
    // Show the saved entry scrolled to the very top (Q: line first), so a long
    // answer doesn't land the viewport mid-answer. No row cursor: this static
    // snapshot isn't the j/k-navigable transcript. Paragraph-split so page 0
    // holds the answer's opening paragraphs (an unsplit answer was one
    // oversized widget that fell off page 0 entirely).
    let (fam, sz) = transcript_font(s);
    s.chat_panel.render_rows_to_top(&rows, &fam, sz);
}
```

(The gloss branch — empty `question` — keeps its `Q:` row exactly as today; only the plain-answer branch changes.)

- [ ] **Step 2: Build**

```bash
cargo build
```

Expected: green.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "fix(chat): paragraph-split the saved-entry snapshot render"
```

---

### Task 5: Question-view j/k/gg/G become paged cursor stepping

**Files:**
- Modify: `src/input/actions/chat.rs:1747-1796` (`transcript_cursor_move` — doc comment + Question guard)
- Modify: `src/input/actions/chat.rs:1820-1862` (`transcript_cursor_first` — doc comment + Question guard)
- Modify: `src/input/actions/chat.rs:1864-1901` (`transcript_cursor_last` — doc comment + Question guard)
- Modify: `src/input/actions/chat.rs:1919-1929`, `1991-1999` (stale "no row_cursor axis" comments on the `V`/`y` guards)

**Interfaces:**
- Consumes: Task 1's rows, Task 2's `render_current_question` (renders at `s.chat.row_cursor`), existing `step_cursor_paged`, `landable_mask`.
- Produces: j/k step Q → paragraph → paragraph (page-turning at page edges); gg lands on the `Q:` row; G lands on the last landable widget.

- [ ] **Step 1: Replace the j/k guard**

Replace `chat.rs:1793-1796`:

```rust
    if s.chat.view == PanelView::Question {
        let Some(e) = s.chat.exchanges.get(s.chat.cursor) else {
            s.chat_panel.scroll_transcript_step(delta as f64);
            return;
        };
        // Same widget-space stepping as the Journal arm, over this ONE
        // exchange's rows (Q: row + one widget per answer paragraph).
        // s.chat.pages/page_idx are authoritative — the last Question render
        // (render_paginated) computed them for exactly these rows.
        let rows = build_single_exchange_rows(e);
        let landable = landable_mask(&rows);
        if !landable.iter().any(|&l| l) {
            s.chat_panel.scroll_transcript_step(delta as f64);
            return;
        }
        let (new_cursor, new_page) = crate::ui::chat_pagination::step_cursor_paged(
            s.chat.row_cursor,
            delta,
            s.chat.page_idx,
            &s.chat.pages,
            &landable,
        );
        s.chat.row_cursor = new_cursor;
        s.chat.page_idx = new_page;
        render_current_question(s);
        return;
    }
```

Update the stale paragraph in `transcript_cursor_move`'s leading comment (`chat.rs:1756-1760`, "Question is a flat, uncycled view... j/k just scrolls") to say Question now steps the widget-space row cursor over its own single-exchange rows, exactly like the Journal arm, falling back to plain scrolling only when nothing is landable.

- [ ] **Step 2: Replace the gg guard**

Replace `chat.rs:1841-1844`:

```rust
    if s.chat.view == PanelView::Question {
        let Some(e) = s.chat.exchanges.get(s.chat.cursor) else {
            s.chat_panel.scroll_transcript_to_edge(false);
            return;
        };
        let rows = build_single_exchange_rows(e);
        let landable = landable_mask(&rows);
        let Some(first) = landable.iter().position(|&l| l) else {
            s.chat_panel.scroll_transcript_to_edge(false);
            return;
        };
        s.chat.row_cursor = first;
        s.chat.page_idx = 0;
        render_current_question(s);
        return;
    }
```

(`render_current_question` → `render_paginated` re-derives the true page from the cursor, so `page_idx = 0` here is just the pre-render seed, same as the Journal arm's pattern.)

Update `transcript_cursor_first`'s doc comment sentence about Question (`chat.rs:1827-1830`) accordingly (lands on the first landable widget — the `Q:` row — instead of plain scroll-to-top).

- [ ] **Step 3: Replace the G guard**

Replace `chat.rs:1880-1883`:

```rust
    if s.chat.view == PanelView::Question {
        let Some(e) = s.chat.exchanges.get(s.chat.cursor) else {
            s.chat_panel.scroll_transcript_to_edge(true);
            return;
        };
        let rows = build_single_exchange_rows(e);
        let landable = landable_mask(&rows);
        let Some(last) = landable.iter().rposition(|&l| l) else {
            s.chat_panel.scroll_transcript_to_edge(true);
            return;
        };
        s.chat.row_cursor = last;
        s.chat.page_idx = s.chat.pages.len().saturating_sub(1);
        render_current_question(s);
        return;
    }
```

Update `transcript_cursor_last`'s doc comment (`chat.rs:1864-1868`) accordingly.

- [ ] **Step 4: Fix the now-stale guard comments on `V` and `y`**

The `V` guard comment (`chat.rs:1920-1925`) and `y` guard comment (`chat.rs:1992-1995`) both justify the Question no-op with "no row_cursor/row_owner axis". Question now HAS a row cursor; the guards stay (visual selection and yank remain Gloss-view features, out of this change's scope), so reword each comment to: Question view now has a row cursor, but `V`/`y` stay scoped to the Gloss transcript — a single Q&A's selection/yank semantics are deliberately not defined here. Behavior unchanged.

- [ ] **Step 5: Build + clippy + full unit tests**

```bash
cargo build && cargo clippy && cargo test --bin linux-lit
```

Expected: build green; clippy no NEW warnings (pre-existing warning classes only); all unit tests pass (985 + the 3 new ones).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/chat.rs
git commit -m "feat(chat): paragraph-level j/k and gg/G stepping in Question view"
```

---

### Task 6: Verification and finish

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

```bash
cargo test --bin linux-lit && cargo clippy
```

Expected: all green, no new clippy warnings.

- [ ] **Step 2: Offer the user the testing choice (REQUIRED before calling this done)**

Per the house testing rule, ask the user to choose:

**(a) Headless self-check** — the chat ask flow makes a REAL Claude API call, so the headless drive costs one API request. Flow: cage launch per CLAUDE.md (`LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo ...`), open a prose work, visual-select a passage, Ctrl+a, type a short question, Ctrl+Enter, wait for the answer, then `grim` screenshot. Confirm on the PNG: `Q:` block at the very top of the panel, accent bar on it, no source-passage text, "Saved to journal" behavior intact; then `wtype j`/`k` screenshots to confirm paragraph stepping and `G`/`gg` landing. Also regression-shot the journal overlay ask flow and the gloss overlay `Add` flow (both verified in source as already landing at top — `journal.rs:2519-2523`, `gloss_overlay.rs` `show_gloss_with_color` `cursor_full.set(0)`/`reset_scroll_top()` — but eyeball once). Follow the UI review protocol: open every PNG and report what's visible.

**(b) Manual hand-off** — user restarts `crll`, opens a passage, Ctrl+a-asks a question, and confirms: the new entry renders with `Q:` at top + accent bar, no source text, j/k steps per paragraph, gg/G land first/last, and the journal + gloss overlays still land new entries at top.

- [ ] **Step 3: Finish the branch (after testing passes)**

House default — merge back to master locally, then push:

```bash
git checkout master && git merge --no-ff feat/journal-entry-top-landing
cargo build && cargo test --bin linux-lit
git push origin master && git branch -d feat/journal-entry-top-landing
```

Expected: merge commit on master, build + tests green, branch deleted.

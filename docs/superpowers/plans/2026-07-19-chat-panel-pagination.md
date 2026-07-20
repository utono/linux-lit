# Chat Panel Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Paginate the chat panel (all three views) so only whole rows that fit render per page — no partial row at either edge — with `j`/`k` moving a row cursor and turning the whole page at the edge; and split journal answers into paragraph rows so the accent bar traverses them.

**Architecture:** Reuse `src/ui/pagination.rs` (`paginate_grouped` + `measure_text_height`), the same engine the translation/journal overlays use. A new pure per-widget height+group model feeds `paginate_grouped`; the render renders one page slice (no scroll); cursor nav turns the page at the edge. Journal answers gain a paragraph split mirroring `GlossAnswer`.

**Tech Stack:** Rust, GTK4 (gtk4-rs), `cargo test`, the existing `src/ui/pagination.rs`.

## Global Constraints

- Work in the isolated worktree `~/utono/linux-lit-wt/chat-clip` (branch `fix/chat-panel-clip`), NOT the main checkout.
- Build/test from the worktree: `cargo build`, `cargo test --bin linux-lit` (this is a binary crate — NO `--lib`). Do NOT `cargo run`.
- **Verification is on the USER's real renderer, not cage.** Cage (software render) lays out fonts/metrics differently and cannot prove pixel-exact edges — the whole reason this feature exists. Hand the user exact steps; pixel-measure their screenshot (a whole line ≈ 15-20px, a clip ≈ ≤5px sliver).
- `paginate_grouped(block_heights: &[i32], group_start: &[bool], page_height: i32) -> Vec<Page>` already: packs indivisible units (a `group_start=false` widget attaches to the preceding unit), releases an over-tall multi-widget unit into singletons, and gives a single over-tall widget its own page. `Page { start, end }` over the SAME index space as the input arrays (here: WIDGET indices). Treats `group_start[0]` as true.
- `measure_text_height(pctx, text, size_pt, family, width_px) -> i32` measures a pango layout; it does NOT include CSS padding — the height model must add each row's per-class padding.
- Decisions (locked): whole-page turn (cursor → first row of next page / last row of prev); all three views paginate; journal answers split into paragraph rows.

## File Structure

- `src/ui/chat_pagination.rs` (NEW) — pure height+group model + page-cursor arithmetic. Unit-tested, no GTK widgets.
- `src/ui/chat_panel.rs` (MODIFY) — page-slice render; remove scroll-snap + clip guard.
- `src/input/actions/chat.rs` (MODIFY) — build rows + group flags, journal answer split, page-aware cursor nav, repaginate on size/view change.
- `src/ui/mod.rs` (MODIFY) — `mod chat_pagination;`.

---

### Task 1: Pure page-cursor arithmetic (`chat_pagination.rs`)

**Files:**
- Create: `src/ui/chat_pagination.rs`
- Modify: `src/ui/mod.rs` (add `pub(crate) mod chat_pagination;`)

**Interfaces:**
- Consumes: `crate::ui::pagination::Page`.
- Produces:
  - `fn page_of_widget(pages: &[Page], widget: usize) -> usize` — index of the page containing `widget` (clamped to last).
  - `fn first_landable_in_page(page: Page, landable: &[bool]) -> Option<usize>` — first widget index in `[page.start,page.end)` with `landable[i]`.
  - `fn last_landable_in_page(page: Page, landable: &[bool]) -> Option<usize>`.
  - `fn step_cursor_paged(cursor, delta, page_idx, pages, landable) -> (usize /*new_cursor*/, usize /*new_page*/)` — step within the page; on running off the page edge, turn to the adjacent page and land on its first (delta>0) / last (delta<0) landable widget; clamp at the ends.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/chat_pagination.rs` with a `#[cfg(test)] mod tests` (implementation stubs added in step 3):

```rust
//! Pure page + row-cursor arithmetic for the paginated chat panel. No GTK.
use crate::ui::pagination::Page;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pagination::Page;

    fn pages() -> Vec<Page> {
        // 3 pages over widget indices 0..9
        vec![Page { start: 0, end: 3 }, Page { start: 3, end: 6 }, Page { start: 6, end: 9 }]
    }

    #[test]
    fn page_of_widget_locates_and_clamps() {
        let p = pages();
        assert_eq!(page_of_widget(&p, 0), 0);
        assert_eq!(page_of_widget(&p, 4), 1);
        assert_eq!(page_of_widget(&p, 8), 2);
        assert_eq!(page_of_widget(&p, 99), 2); // clamp past end
        assert_eq!(page_of_widget(&[], 0), 0); // no pages
    }

    #[test]
    fn first_last_landable_skip_unlandable() {
        // page [3,6): widget 3 unlandable (a speaker), 4 & 5 landable
        let landable = vec![true, true, true, false, true, true, true, true, true];
        assert_eq!(first_landable_in_page(Page { start: 3, end: 6 }, &landable), Some(4));
        assert_eq!(last_landable_in_page(Page { start: 3, end: 6 }, &landable), Some(5));
        // a page with no landable widget
        assert_eq!(first_landable_in_page(Page { start: 3, end: 4 }, &landable), None);
    }

    #[test]
    fn step_within_page_moves_cursor_only() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 0 on page 0, +1 → cursor 1, same page
        assert_eq!(step_cursor_paged(0, 1, 0, &p, &landable), (1, 0));
        // cursor 1, -1 → cursor 0, same page
        assert_eq!(step_cursor_paged(1, -1, 0, &p, &landable), (0, 0));
    }

    #[test]
    fn step_off_page_end_turns_to_next_first_landable() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 2 is the last widget of page 0; +1 → page 1, its first landable (3)
        assert_eq!(step_cursor_paged(2, 1, 0, &p, &landable), (3, 1));
    }

    #[test]
    fn step_off_page_top_turns_to_prev_last_landable() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 3 is the first widget of page 1; -1 → page 0, its last landable (2)
        assert_eq!(step_cursor_paged(3, -1, 1, &p, &landable), (2, 0));
    }

    #[test]
    fn step_clamps_at_document_ends() {
        let p = pages();
        let landable = vec![true; 9];
        // cursor 0 on page 0, -1 → no prev page, stay
        assert_eq!(step_cursor_paged(0, -1, 0, &p, &landable), (0, 0));
        // cursor 8 (last) on page 2, +1 → no next page, stay
        assert_eq!(step_cursor_paged(8, 1, 2, &p, &landable), (8, 2));
    }
}
```

Add to `src/ui/mod.rs` (find the `mod` list, alphabetical near `mod chat_panel;`):

```rust
pub(crate) mod chat_pagination;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit chat_pagination 2>&1 | tail -20`
Expected: FAIL — `cannot find function page_of_widget` etc.

- [ ] **Step 3: Implement**

Add above the test module in `src/ui/chat_pagination.rs`:

```rust
/// Index of the page whose `[start,end)` contains `widget`; clamps to the last
/// page (and returns 0 when there are no pages).
pub(crate) fn page_of_widget(pages: &[Page], widget: usize) -> usize {
    for (i, p) in pages.iter().enumerate() {
        if widget >= p.start && widget < p.end {
            return i;
        }
    }
    pages.len().saturating_sub(1)
}

/// First widget index in the page with `landable[i]` true, if any.
pub(crate) fn first_landable_in_page(page: Page, landable: &[bool]) -> Option<usize> {
    (page.start..page.end).find(|&i| landable.get(i).copied().unwrap_or(false))
}

/// Last widget index in the page with `landable[i]` true, if any.
pub(crate) fn last_landable_in_page(page: Page, landable: &[bool]) -> Option<usize> {
    (page.start..page.end).rev().find(|&i| landable.get(i).copied().unwrap_or(false))
}

/// Step the row cursor by `delta` (±1) over landable widgets. Within the current
/// page the cursor moves to the next/previous landable widget. When it would run
/// off the page edge, turn to the adjacent page and land on that page's first
/// (delta>0) / last (delta<0) landable widget. Clamps (no-op) at the document
/// ends. Returns `(new_cursor, new_page)`.
pub(crate) fn step_cursor_paged(
    cursor: usize,
    delta: i32,
    page_idx: usize,
    pages: &[Page],
    landable: &[bool],
) -> (usize, usize) {
    let Some(page) = pages.get(page_idx).copied() else {
        return (cursor, page_idx);
    };
    // Next landable within this page in the step direction.
    let within = if delta > 0 {
        (cursor + 1..page.end).find(|&i| landable.get(i).copied().unwrap_or(false))
    } else {
        (page.start..cursor)
            .rev()
            .find(|&i| landable.get(i).copied().unwrap_or(false))
    };
    if let Some(w) = within {
        return (w, page_idx);
    }
    // Off the page edge — turn the page.
    if delta > 0 {
        if let Some(next) = pages.get(page_idx + 1).copied() {
            if let Some(w) = first_landable_in_page(next, landable) {
                return (w, page_idx + 1);
            }
        }
    } else if page_idx > 0 {
        let prev = pages[page_idx - 1];
        if let Some(w) = last_landable_in_page(prev, landable) {
            return (w, page_idx - 1);
        }
    }
    (cursor, page_idx) // clamp at document ends
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit chat_pagination 2>&1 | tail -20`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/ui/chat_pagination.rs src/ui/mod.rs
git commit -m "feat(chat): pure page + row-cursor arithmetic for panel pagination"
```

---

### Task 2: Per-widget height + group model

**Files:**
- Modify: `src/ui/chat_pagination.rs` (add the height/group builder + a class→padding table)
- Test: same file's `mod tests`

**Interfaces:**
- Consumes: the widget class list for a rendered transcript (from `row_widget_texts` + the class each widget renders with).
- Produces:
  - `struct ChatWidget { text: String, class: String, group_start: bool }` — one per rendered widget (a `GlossAnswer`/journal answer explodes into several).
  - `fn class_pad(class: &str) -> i32` — the total vertical CSS padding (top+bottom) a `chat-a-*`/`chat-q`/`chat-a-src-lead`… class adds, mirroring `theme.rs`. Pure const table.
  - `fn widget_heights(widgets: &[ChatWidget], measure: impl Fn(&str) -> i32) -> (Vec<i32>, Vec<bool>)` — returns `(heights, group_start)`: height = `measure(text) + class_pad(class)`; `group_start` copied from each widget. `measure` is injected (the GTK pango measure) so this stays unit-testable.

**Note:** the exact padding numbers must be read from `theme.rs` at implementation time (they may have been tuned). The table below is the SHAPE; the implementer confirms each value against the current `theme.rs` `.chat-*` rules before finalizing.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/ui/chat_pagination.rs`:

```rust
#[test]
fn class_pad_reads_known_classes() {
    // Values MUST match theme.rs at implementation time; these assert the
    // shape (a src-lead row carries a big top gap; a plain answer carries little).
    assert!(class_pad("chat-a-src-lead") >= class_pad("chat-a"));
    assert!(class_pad("chat-a-gloss") > 0);
    // An unknown class contributes 0 (defensive).
    assert_eq!(class_pad("nonexistent-class"), 0);
}

#[test]
fn widget_heights_add_padding_and_carry_group_start() {
    let widgets = vec![
        ChatWidget { text: "Q".into(), class: "chat-q".into(), group_start: true },
        ChatWidget { text: "verse".into(), class: "chat-a-src-lead".into(), group_start: false },
    ];
    // measure returns a fixed 20px for any text
    let (h, gs) = widget_heights(&widgets, |_t| 20);
    assert_eq!(h[0], 20 + class_pad("chat-q"));
    assert_eq!(h[1], 20 + class_pad("chat-a-src-lead"));
    assert_eq!(gs, vec![true, false]);
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit chat_pagination 2>&1 | tail -20`
Expected: FAIL — `cannot find type ChatWidget` / `function class_pad`.

- [ ] **Step 3: Implement**

Add to `src/ui/chat_pagination.rs` (confirm each pad value against `src/theme.rs` `.chat-*` rules FIRST — grep `rg -n "chat-a|chat-q|padding-top|padding-bottom" src/theme.rs`):

```rust
/// One rendered transcript widget: its text, CSS class, and whether it starts a
/// new indivisible pagination unit (a `GlossAnswer`/journal answer's first
/// widget is a group start; its continuation widgets are not).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChatWidget {
    pub text: String,
    pub class: String,
    pub group_start: bool,
}

/// Total vertical CSS padding (top + bottom) the class adds around its text, so
/// the pagination height matches what GTK renders (`measure_text_height` sees
/// only the text). MIRRORS the `.chat-*` rules in `theme.rs` — keep in sync.
pub(crate) fn class_pad(class: &str) -> i32 {
    match class {
        "chat-q" => 0,
        "chat-a" => 0,
        "chat-a-speaker" => 14,
        "chat-a-verse" => 0,
        "chat-a-verse-flush" => 0,
        "chat-a-stage" => 8,
        "chat-a-stage-flush" => 8,
        "chat-a-gloss" => 18,
        "chat-a-src-lead" => 30,
        "chat-chip" => 0,
        "chat-error" => 0,
        "chat-saved" => 0,
        _ => 0,
    }
}

/// Per-widget heights + group-start flags for pagination. `measure(text)` is the
/// pango text-height measurement (injected so this is unit-testable without GTK).
pub(crate) fn widget_heights(
    widgets: &[ChatWidget],
    measure: impl Fn(&str) -> i32,
) -> (Vec<i32>, Vec<bool>) {
    let heights = widgets
        .iter()
        .map(|w| measure(&w.text) + class_pad(&w.class))
        .collect();
    let group_start = widgets.iter().map(|w| w.group_start).collect();
    (heights, group_start)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit chat_pagination 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/ui/chat_pagination.rs
git commit -m "feat(chat): per-widget height + group model for pagination"
```

---

### Task 3: Journal answer split into paragraph rows

**Files:**
- Modify: `src/input/actions/chat.rs` — `journal_view_rows` (~1240), `journal_entry_qrow` (~1274), and the journal-cursor nav (`transcript_cursor_move` Journal branch ~1803, `step_journal_cursor` ~1291).

**Interfaces:**
- Consumes: `journal_list: Vec<JournalPage>` (each has `.question`, `.answer`).
- Produces: `journal_view_rows` returns, per entry, `[Question(q), Answer(para1), Answer(para2), …]` (answer split on blank lines into paragraph `Answer` rows) instead of `[Question, Answer]`. The Journal view then uses the SAME `row_cursor` + `landable_mask` + `row_owner` machinery as Gloss (a `journal_row_owner` mapping each widget → entry), so `journal_cursor` (entry-granularity) is superseded by row stepping.

**Design note:** This is the accent-bar-stuck fix. After this task the Journal view has real per-widget rows; Task 5 then paginates it uniformly with Gloss. Keep `journal_cursor` as the "which ENTRY is selected" for `R`/save semantics, but drive the accent bar from `row_cursor` mapped through a widget→entry owner (mirroring Gloss's `row_owner`).

- [ ] **Step 1: Write the failing test**

The paragraph split is pure — extract it. Add a helper + test. In `chat.rs` near `journal_view_rows`:

```rust
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
```

Add to the `#[cfg(test)] mod tests` in `chat.rs`:

```rust
#[test]
fn split_answer_paragraphs_by_blank_lines() {
    assert_eq!(split_answer_paragraphs("one\n\ntwo\n\nthree"),
        vec!["one".to_string(), "two".to_string(), "three".to_string()]);
    // single paragraph → one chunk
    assert_eq!(split_answer_paragraphs("just one"), vec!["just one".to_string()]);
    // empty → one empty chunk (entry keeps an answer row)
    assert_eq!(split_answer_paragraphs("   "), vec![String::new()]);
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit split_answer_paragraphs 2>&1 | tail -15`
Expected: FAIL — `cannot find function split_answer_paragraphs`.

- [ ] **Step 3: Implement the split + wire journal_view_rows**

Add `split_answer_paragraphs` (above). Then change `journal_view_rows` (read it first: `rg -n -A12 "fn journal_view_rows" src/input/actions/chat.rs`) so each entry emits `Question(q)` followed by one `Answer(chunk)` per `split_answer_paragraphs(&entry.answer)` chunk. Build a parallel `Vec<usize>` (`journal_row_owner`) pushing the entry index once per emitted widget, stored on `ChatState` (add field `pub journal_row_owner: Vec<usize>` with `#[derive(Default)]` coverage). Update `journal_entry_qrow` callers: the accent bar now lands via `row_cursor` (a widget index) mapped to the entry through `journal_row_owner`, not `entry*2`.

Update the Journal branch of `transcript_cursor_move` to step `row_cursor` over the journal `landable_mask` (every emitted journal widget is landable — Q and each answer paragraph), setting `journal_cursor = journal_row_owner[row_cursor]` after each move (so `R`/save still act on the right entry). Mirror in `transcript_cursor_first`/`last`.

(Exact edits depend on the current bodies; the implementer reads each fn and rewires it to the row-widget model, keeping the Gloss branch untouched. The `journal_entry_qrow_is_two_per_entry` test at chat.rs:3548 must be UPDATED — an entry is now `1 + n_paragraphs` widgets, not 2; rename/adjust it to assert the new row count from a known answer.)

- [ ] **Step 4: Run tests**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo build 2>&1 | tail -5 && cargo test --bin linux-lit 2>&1 | rg "test result|FAILED" | tail -5`
Expected: builds; all pass except the known-pre-existing `theme_cycle_defaults_to_reading_themes`. Fix any test that asserted the OLD 2-rows-per-entry shape.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/input/actions/chat.rs
git commit -m "feat(chat): split journal answers into paragraph rows (accent bar traverses them)"
```

---

### Task 4: Emit group-start flags from the row builder

**Files:**
- Modify: `src/input/actions/chat.rs` — `build_transcript_rows` (~978) and the journal row builder (Task 3), to produce a `Vec<ChatWidget>` (text + class + group_start) alongside the existing widget arrays.

**Interfaces:**
- Consumes: the exchanges / journal rows.
- Produces: a function `chat_widgets(s) -> Vec<crate::ui::chat_pagination::ChatWidget>` for the CURRENT view — the flat widget list with `group_start=true` at each `TranscriptRow` boundary that begins a new indivisible unit (a `GlossAnswer`'s first widget; a journal entry's `Question`; a plain `Answer`/`Question`), and `false` for continuation widgets (a `GlossAnswer`'s verse/gloss labels after the first; a journal answer's 2nd+ paragraph). The CLASS each widget renders with must match `rebuild_rows`/`append_gloss_answer` exactly (reuse that mapping — do NOT re-derive).

**Design note:** the class + group mapping already lives implicitly in `rebuild_rows` (chat_panel.rs:397) + `append_gloss_answer` (chat_panel.rs:477). Factor the class-assignment out so BOTH the renderer and `chat_widgets` use one function — otherwise heights (Task 2) and render drift. Add `fn row_widget_specs(rows: &[TranscriptRow]) -> Vec<ChatWidget>` in `chat_panel.rs` (pub(crate)) that returns the exact (text, class, group_start) per widget; `rebuild_rows` renders from it, and `chat_widgets` measures from it. This is the single source of truth for widget expansion.

- [ ] **Step 1: Write the failing test**

Add to `chat_panel.rs` `#[cfg(test)]` (or a new `row_widget_specs_tests` module):

```rust
#[test]
fn row_widget_specs_explodes_gloss_and_marks_groups() {
    use crate::ui::chat_panel::{TranscriptRow as R, row_widget_specs};
    let rows = vec![
        R::Question("q".into()),
        R::GlossAnswer("<speaker>X</speaker>\n<verse>v1</verse>\n<gloss>g</gloss>".into()),
    ];
    let specs = row_widget_specs(&rows);
    // Question → 1 widget (group start). GlossAnswer → 3 widgets: first is a
    // group start, the rest continue the same unit.
    assert_eq!(specs.len(), 4);
    assert_eq!(specs[0].group_start, true);   // Question
    assert_eq!(specs[1].group_start, true);   // gloss unit begins
    assert_eq!(specs[2].group_start, false);  // verse continues
    assert_eq!(specs[3].group_start, false);  // gloss continues
    assert_eq!(specs[1].class, "chat-a-speaker");
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo test --bin linux-lit row_widget_specs 2>&1 | tail -15`
Expected: FAIL — `cannot find function row_widget_specs`.

- [ ] **Step 3: Implement `row_widget_specs` + refactor `rebuild_rows` to use it**

Read `rebuild_rows` (chat_panel.rs:397) + `append_gloss_answer` (477). Extract the (text, class) decision into `row_widget_specs(rows) -> Vec<ChatWidget>`: for each `TranscriptRow`, push its widget(s) with the class `rebuild_rows`/`append_gloss_answer` would use, `group_start=true` for the row's FIRST widget, `false` for `GlossAnswer` continuations (and journal-answer continuations once Task 3's rows flow through). Then rewrite `rebuild_rows` to iterate `row_widget_specs(rows)` and create one label per spec (using its class), so render and measure share ONE expansion. Keep `row_widget_texts`/`row_widget_landable` consistent (derive them from specs too, or assert equal length in a test).

- [ ] **Step 4: Run tests**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo build 2>&1 | tail -3 && cargo test --bin linux-lit 2>&1 | rg "test result|FAILED" | tail -4`
Expected: builds; all pass (except known theme_cycle). The `row_widget_texts`/`landable` lengths must still match `row_widget_specs` length.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/ui/chat_panel.rs src/input/actions/chat.rs
git commit -m "feat(chat): row_widget_specs — single source of widget expansion + group flags"
```

---

### Task 5: Paginate + page-slice render (the core swap)

**Files:**
- Modify: `src/ui/chat_panel.rs` — add a paginate+render entry point; rewrite `render_rows_focused_cursor` to render a PAGE SLICE; remove the scroll-snap.
- Modify: `src/input/actions/chat.rs` — `render_transcript` / `render_journal_view` compute pages and pass the page slice + page-local cursor; add pages + page_idx to `ChatState`.

**Interfaces:**
- Consumes: `row_widget_specs` (Task 4), `widget_heights` (Task 2), `paginate_grouped`, `page_of_widget`/`first_landable_in_page` (Task 1), the transcript pixel budget.
- Produces: a `ChatPanel::render_page(specs, page: Page, cursor_widget, selection)` that renders ONLY `specs[page.start..page.end]` into `transcript_box`, applies the accent bar at the page-local cursor, and does NOT scroll. `ChatState` gains `pub pages: Vec<Page>` + `pub page_idx: usize`.

- [ ] **Step 1: Add the transcript budget helper (measure)**

In `chat_panel.rs`, add `fn transcript_budget(&self) -> i32` = the transcript scroll's allocated height (`self.transcript_scroll.height()`, falling back to the container height minus the input card's height when unallocated). And `fn measure_widget(&self, text, class) -> i32` using `self.transcript_box.pango_context()` + `pagination::measure_text_height_leaded` at the transcript wrap width (`transcript_scroll.width() - padding`) + `chat_pagination::class_pad(class)`. (These need live GTK; no unit test — exercised by the e2e.)

- [ ] **Step 2: Rewrite `render_rows_focused_cursor` → page-slice render**

Replace the body of `render_rows_focused_cursor` (chat_panel.rs:241, the scroll-snapping version) with a page-slice render: it now takes `(specs: &[ChatWidget], page: Page, cursor_widget: usize, selection)`, rebuilds `transcript_box` from `specs[page.start..page.end]` only, applies `.chat-cursor-row` to the widget at `cursor_widget - page.start` (page-local) and `.chat-visual-row` over the page-local selection, and does NOT touch the vadjustment (the page fits). Remove the `row_tops`/`snap_down`/into-view block entirely. (Rename to `render_page` if cleaner; update callers.)

- [ ] **Step 3: Remove the clip guard + scroll machinery**

In `chat_panel.rs`: delete the `clip_guard` field, the `attach_box` call + Overlay wrap in `new()` (append `transcript_scroll` directly again), the `on_open` method, and the `on_open` call in `show()`. In `theme.rs`: delete the `.chat-panel .gloss-bottom-clip` override. In `render_rows`/`render_rows_to_top`: these now render the LAST / FIRST page slice respectively (streaming shows the last page; a fresh saved entry shows the first) — or route them through the same paginate+render path with `page_idx` set to last/first. Keep `propagate_natural_width(false)` and the width/height/margin fixes.

- [ ] **Step 4: Compute pages in `render_transcript` / `render_journal_view`**

In `chat.rs`, `render_transcript` (1064) and `render_journal_view` (1307): build `specs = row_widget_specs(rows)`; `(heights, gs) = widget_heights(specs, |t,c| panel.measure_widget(t,c))`; `pages = paginate_grouped(&heights, &gs, panel.transcript_budget())`; store on `s.chat.pages`; clamp `s.chat.page_idx`; derive the page holding `row_cursor` (`page_of_widget`) OR keep the explicit `page_idx`; call `panel.render_page(&specs, pages[page_idx], row_cursor, selection)`. Add `pages`/`page_idx` to `ChatState` (`#[derive(Default)]`).

- [ ] **Step 5: Build + full test**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo build 2>&1 | tail -5 && cargo test --bin linux-lit 2>&1 | rg "test result|FAILED" | tail -5`
Expected: builds; all pass (except known theme_cycle).

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/ui/chat_panel.rs src/input/actions/chat.rs src/theme.rs
git commit -m "feat(chat): paginate the panel — page-slice render, drop free-scroll clip machinery"
```

---

### Task 6: Page-aware cursor nav + repaginate on resize/view-switch

**Files:**
- Modify: `src/input/actions/chat.rs` — `transcript_cursor_move` (1803), `transcript_cursor_first`/`last` (1926/1955), the resize/`size_panel` path, `toggle_panel_view` (1461).

**Interfaces:**
- Consumes: `step_cursor_paged` (Task 1), `s.chat.pages`, `s.chat.page_idx`, `row_cursor`.
- Produces: `j`/`k` move the cursor via `step_cursor_paged` (turning the page at the edge), then re-render the (possibly new) page; `gg`/`G` → first page/first landable, last page/last landable; resize + view switch re-paginate and clamp.

- [ ] **Step 1: Wire `transcript_cursor_move` through `step_cursor_paged`**

In `transcript_cursor_move` (both Gloss and Journal branches now use the SAME row-widget model): `(new_cursor, new_page) = step_cursor_paged(s.chat.row_cursor, delta, s.chat.page_idx, &s.chat.pages, &landable_mask)`; set `row_cursor`/`page_idx`; update `s.chat.cursor` (Gloss) / `journal_cursor` (Journal) from the owner map; re-render the page. `transcript_cursor_first` → page 0 + `first_landable_in_page`; `transcript_cursor_last` → last page + `last_landable_in_page`.

- [ ] **Step 2: Repaginate on resize + view switch**

Find the resize tick / `size_panel` caller that re-renders the panel and the `toggle_panel_view` (1461) path; after a height change or view flip, recompute pages (Task 5's paginate) and clamp `page_idx` + `row_cursor` into range before rendering. On view switch, reset `page_idx = 0` + first landable.

- [ ] **Step 3: Build + full test**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo build 2>&1 | tail -3 && cargo test --bin linux-lit 2>&1 | rg "test result|FAILED" | tail -4`
Expected: builds; all pass (except known theme_cycle).

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add src/input/actions/chat.rs
git commit -m "feat(chat): page-aware j/k cursor + page-turn at edge; repaginate on resize/view-switch"
```

---

### Task 7: Real-renderer verification + clip-prevention.md update

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md` (rewrite the chat-panel checklist entry #17 to describe the PAGINATION fix, replacing the earlier scroll-snap description).
- No source changes (verification only).

- [ ] **Step 1: Build**

Run: `cd ~/utono/linux-lit-wt/chat-clip && cargo build 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 2: Hand the user the exact test + pixel-verify their screenshot**

Give the user:
```bash
pkill -f "cage -- ./target/debug/linux-lit"; kill <live-pid> 2>/dev/null; cd ~/utono/linux-lit-wt/chat-clip && LIT_DEV=1 ./target/debug/linux-lit 2>&1 | tee linux-lit-dev-stderr.log
```
Ask them to: open the DC caul gloss, press `j`/`k` through a long gloss AND (via `t`) a long saved journal answer, and screenshot. Then PIXEL-MEASURE the screenshot (a Python/PIL scan): the top AND bottom ink bands must be whole lines (~15-20px), never a ≤5px sliver; confirm the accent bar moves on every `j` and turns the page at the edge; confirm the journal accent bar traverses the answer paragraphs. Cage is NOT sufficient — this is the whole reason the feature exists.

- [ ] **Step 3: Rewrite clip-prevention.md #17**

Replace the current chat-panel entry (the scroll-snap description) with the pagination fix: the chat panel now PAGINATES (renders only whole rows that fit via `paginate_grouped` over per-widget heights) — no partial row at either edge, `j`/`k` turns the page at the edge. Note the reused `pagination.rs` engine, the `row_widget_specs` single-source expansion, and that a single over-tall paragraph is the one residual scroll case. Cross-reference "Pagination instead of a mask."

- [ ] **Step 4: Commit the doc**

```bash
cd ~/utono/linux-lit-wt/chat-clip
git add docs/troubleshooting/clip-prevention.md
git commit -m "docs(clip): chat panel paginates (both edges clean); supersedes the scroll-snap entry"
```

---

## Finish-up (after user confirms on the real renderer)

The branch `fix/chat-panel-clip` also carries the earlier width/height/margin/gloss-gap fixes. Sort the git state:
1. `origin/master` still has the reverted-broken first fix; local master has the revert (`6a693bf1`) + uncommitted CLAUDE.md (commit it).
2. Merge `fix/chat-panel-clip` to master from the MAIN checkout, re-verify build + `cargo test --bin linux-lit chat`, push, `git worktree remove` + `git branch -d`.
Prompt the user to choose headless self-check vs. their own final eyeball (they've been the verifier throughout — hand them the exact command).

## Self-Review

- **Spec coverage:** page-cursor arithmetic (T1), height/group model (T2), journal answer split / accent-bar fix (T3), group flags + single-source widget expansion (T4), paginate + page-slice render + remove clip machinery (T5), page-turn nav + repaginate (T6), verification + doc (T7). Over-tall group → `paginate_grouped` already releases to singletons (spec edge case); over-tall single paragraph → that one page scrolls (spec, matches reading card). All three views: Gloss + Journal via the shared row-widget model (T3–T6), Question via the same render path (single exchange, one or few pages).
- **Placeholder scan:** the padding VALUES in T2 and the exact fn-body rewrites in T3/T4/T6 are marked "confirm against current source" because they depend on live bodies the plan can't freeze — every such step names the exact function + line and the transformation; no "TODO"/"handle edge cases" hand-waves.
- **Type consistency:** `ChatWidget { text, class, group_start }` defined T2, produced by `row_widget_specs` T4, consumed by `widget_heights` T2 + `render_page` T5; `Page`/`step_cursor_paged`/`page_of_widget`/`first_landable_in_page`/`last_landable_in_page` defined T1, used T5/T6; `pages`/`page_idx` added to `ChatState` T5, used T6.

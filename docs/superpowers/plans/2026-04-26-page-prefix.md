# Page Prefix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show `<page> - <line_id>` in the bottom overlay for prose works, where page is the viewport-page that contains the currently highlighted line. Plays unchanged.

**Architecture:** Reuse the existing j-key page-forward logic to count how many j-presses from line 0 land on the same page as the current line. Refactor `last_fully_visible_line` to take an arbitrary top, build a pure `next_page_top` helper that mirrors `page_forward`, then a `viewport_page_for_line` that loops it. Wire into `page_label_text_for_buffer` for non-play works only.

**Tech Stack:** Rust, GTK4, sourceview5, existing `src/input/navigation.rs` and `src/ui/page_label.rs` modules.

---

## Spec reference

See `docs/superpowers/specs/2026-04-26-page-prefix-design.md`. In short:

- Plays: no change — citation only (`I.i.15`, `Prologue 31`, etc.).
- Everything else: prefix line label with `<page> - `, e.g. `1 - 1234`.
- Page = number of j-presses from line 0 needed to reach the page containing
  `state.current_line` (or, more precisely, the buffer line currently shown by
  the label after spacer-skip). 1-indexed.
- Recompute every label refresh. No caching in v1.

## File map

- **Modify** `src/ui/page_label.rs` — add `format_prose_label`. Add unit tests.
- **Modify** `src/input/navigation.rs` —
  - Refactor `last_fully_visible_line(state)` → `last_fully_visible_line(state, top)`.
    Update its one caller (`page_forward`) to pass `state.page_top_line`.
  - Add new pub helpers `next_page_top(state, top)` and `viewport_page_for_line(state, target_line)`.
- **Modify** `src/app.rs` — in `page_label_text_for_buffer` (around line 193, the
  prose branch), prepend the page prefix.
- **Modify** `tests/` — add a headless test using the existing prose-navigation
  fixture that asserts the prefix appears with the correct page number after
  a known sequence of j-presses.

No new files.

## Pre-flight: verify current behavior baseline

- [ ] **Step 1: Confirm current label format on a prose work**

  Read `src/app.rs` around the `page_label_text_for_buffer` function. Confirm the
  prose branch returns `Some(format!("{}", line.id))` (i.e. just a bare line id —
  no page prefix). This is the line the implementation will replace.

  Run:
  ```
  rg -n "format!\(\"\{\}\", line\.id\)" src/app.rs
  ```
  Expected: one hit inside `page_label_text_for_buffer`.

- [ ] **Step 2: Confirm baseline build is green**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  cargo test --no-run
  ```
  Expected: all succeed. If clippy or tests are already broken on this branch,
  stop and surface that to the user before continuing — the plan assumes a clean
  baseline.

---

## Task 1: Add `format_prose_label` with tests

**Files:**
- Modify: `src/ui/page_label.rs`

- [ ] **Step 1: Write the failing tests**

  Append to the existing `#[cfg(test)] mod tests` block at the bottom of
  `src/ui/page_label.rs`:

  ```rust
      #[test]
      fn prose_label_basic() {
          assert_eq!(format_prose_label(1, 1234), "1 - 1234");
          assert_eq!(format_prose_label(34, 5678), "34 - 5678");
      }

      #[test]
      fn prose_label_page_one_line_one() {
          assert_eq!(format_prose_label(1, 1), "1 - 1");
      }

      #[test]
      fn prose_label_large_numbers() {
          assert_eq!(format_prose_label(999, 123456), "999 - 123456");
      }
  ```

- [ ] **Step 2: Run tests to verify they fail**

  ```
  cargo test --lib page_label::tests::prose_label
  ```
  Expected: compile error — `format_prose_label` not found.

- [ ] **Step 3: Add the implementation**

  Add this function to `src/ui/page_label.rs`, just below the existing
  `format_play_citation` function (above the `#[cfg(test)]` block):

  ```rust
  /// Format the bottom-overlay label for non-play works as
  /// `"{page} - {line_id}"`, where `page` is the 1-indexed viewport page
  /// containing the highlighted line.
  pub fn format_prose_label(page: usize, line_id: i64) -> String {
      format!("{} - {}", page, line_id)
  }
  ```

- [ ] **Step 4: Run tests to verify they pass**

  ```
  cargo test --lib page_label::tests
  ```
  Expected: all tests in the `page_label::tests` module pass, including the new
  three plus all pre-existing `roman_*` and `play_citation_*` tests.

- [ ] **Step 5: Commit**

  ```
  git add src/ui/page_label.rs
  git commit -m "Add format_prose_label for '<page> - <line_id>' overlay text"
  ```

---

## Task 2: Refactor `last_fully_visible_line` to accept an explicit top

This is a pure refactor. No behavior change. Required so we can simulate j-presses
from line 0 in Task 3.

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Verify current signature and caller count**

  ```
  rg -n "fn last_fully_visible_line|last_fully_visible_line\(" src/input/navigation.rs
  ```
  Expected: exactly two hits — the definition (currently `fn last_fully_visible_line(state: &AppState) -> usize`) and one call inside `page_forward`.

  If there are more callers, stop and reconcile before proceeding. The plan
  assumes one caller.

- [ ] **Step 2: Change the signature and body**

  In `src/input/navigation.rs`, replace the entire `last_fully_visible_line`
  function (currently around line 107) with this version. The only changes are
  (a) the new `top: usize` parameter, and (b) every reference to
  `state.page_top_line` is replaced with `top`. The behavior is identical when
  called as `last_fully_visible_line(state, state.page_top_line)`.

  ```rust
  /// Find the last buffer line that fits within the viewport starting from
  /// `top`, matching the bottom clip calculation exactly. A line is included
  /// only if its full height fits in the remaining usable space (widget height
  /// minus descender guard). This ensures page_forward doesn't count clipped
  /// lines as "seen".
  fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
      let widget_height = state.text_view.height();
      if widget_height <= 0 {
          return top;
      }
      let line_count = state.effective_line_count();
      let descender_guard = descender_guard_px(&state.text_view, top);
      let bottom_margin = state.text_view.bottom_margin();
      let usable_height = widget_height - descender_guard - bottom_margin;
      let mut total = 0;
      let mut last = top;
      for i in top..line_count {
          let Some(iter) = state.buffer.iter_at_line(i as i32) else { break };
          let (_y, h) = state.text_view.line_yrange(&iter);
          // Match update_bottom_clip: line must fully fit in usable space
          if total + h > usable_height {
              break;
          }
          last = i;
          total += h;
      }
      // Back up past trailing speaker names and blank lines so a dangling
      // speaker at the bottom doesn't count as "visible" content.
      use crate::db::line_types;
      while last > top {
          let text = buffer_line_text(&state.buffer, last);
          if line_types::is_speaker(&text) || line_types::is_blank(&text) {
              last -= 1;
          } else {
              break;
          }
      }
      last
  }
  ```

- [ ] **Step 3: Update the one caller in `page_forward`**

  Inside `page_forward` (around line 154), find:

  ```rust
      let last_visible = last_fully_visible_line(state);
  ```

  Replace with:

  ```rust
      let last_visible = last_fully_visible_line(state, state.page_top_line);
  ```

- [ ] **Step 4: Build and run all tests**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
  Expected: clean build, no clippy warnings, all tests still pass. This refactor
  must be behavior-preserving — if any navigation/integration test fails, revert
  and re-check that every `state.page_top_line` reference inside the old function
  was renamed to `top`.

- [ ] **Step 5: Commit**

  ```
  git add src/input/navigation.rs
  git commit -m "Refactor last_fully_visible_line to take explicit top"
  ```

---

## Task 3: Add `next_page_top` helper

A pure-ish helper that computes the page-top for the page **after** the page
that begins at `top`, mirroring exactly what `page_forward` does. Returns
`line_count` (i.e. one past end) when there is no further dialogue.

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add the helper**

  In `src/input/navigation.rs`, immediately above the existing `pub fn page_forward`
  (which currently begins around line 145), add:

  ```rust
  /// Compute the page-top of the page that follows the page beginning at
  /// `top`, using the same logic as `page_forward`. Returns `line_count` when
  /// there is no further dialogue (i.e. caller is on or past the last page).
  ///
  /// Pure with respect to `state` — does not mutate. Used by
  /// `viewport_page_for_line` to count pages from line 0 forward.
  fn next_page_top(state: &AppState, top: usize) -> usize {
      let line_count = state.effective_line_count();
      if line_count == 0 || top >= line_count {
          return line_count;
      }
      let last_visible = last_fully_visible_line(state, top);
      let last = last_dialogue_in_page(
          &state.buffer,
          top,
          last_visible.saturating_sub(top) + 1,
          line_count,
      );
      let next = next_dialogue_from(&state.buffer, last + 1, line_count);
      if next >= line_count {
          return line_count;
      }
      back_up_for_speaker(&state.buffer, next)
  }
  ```

- [ ] **Step 2: Refactor `page_forward` to use the new helper**

  This keeps a single source of truth for "where does the next page start" and
  guarantees `viewport_page_for_line` matches j-presses by construction.

  In `page_forward` (around lines 154–183), replace the block:

  ```rust
      let last_visible = last_fully_visible_line(state, state.page_top_line);
      let last = last_dialogue_in_page(&state.buffer, state.page_top_line, last_visible.saturating_sub(state.page_top_line) + 1, line_count);
      let next = next_dialogue_from(&state.buffer, last + 1, line_count);

      // Debug: log page forward details
      {
          let lv_text = buffer_line_text(&state.buffer, last_visible);
          let ld_text = buffer_line_text(&state.buffer, last);
          let nx_text = if next < line_count { buffer_line_text(&state.buffer, next) } else { "(end)".into() };
          let widget_h = state.text_view.height();
          let desc_guard = descender_guard_px(&state.text_view, state.page_top_line);
          log_fmt!("PAGE_FWD: page_top={} last_visible={} last_dialogue={} next={}", state.page_top_line, last_visible, last, next);
          log_fmt!("PAGE_FWD: widget_h={} desc_guard={} usable_h={}", widget_h, desc_guard, widget_h - desc_guard);
          log_fmt!("PAGE_FWD: last_visible_text='{}'", lv_text.chars().take(60).collect::<String>());
          log_fmt!("PAGE_FWD: last_dialogue_text='{}'", ld_text.chars().take(60).collect::<String>());
          log_fmt!("PAGE_FWD: next_text='{}'", nx_text.chars().take(60).collect::<String>());
          // Log heights of lines near the boundary
          for i in last_visible.saturating_sub(2)..=(last_visible + 2).min(line_count - 1) {
              if let Some(iter) = state.buffer.iter_at_line(i as i32) {
                  let (_y, h) = state.text_view.line_yrange(&iter);
                  let t = buffer_line_text(&state.buffer, i);
                  log_fmt!("PAGE_FWD: line {} h={} '{}'", i, h, t.chars().take(50).collect::<String>());
              }
          }
      }

      if next >= line_count {
          return; // already at end
      }
      let new_top = back_up_for_speaker(&state.buffer, next);
  ```

  with:

  ```rust
      let last_visible = last_fully_visible_line(state, state.page_top_line);
      let last = last_dialogue_in_page(&state.buffer, state.page_top_line, last_visible.saturating_sub(state.page_top_line) + 1, line_count);
      let next = next_dialogue_from(&state.buffer, last + 1, line_count);
      let new_top = next_page_top(state, state.page_top_line);

      // Debug: log page forward details
      {
          let lv_text = buffer_line_text(&state.buffer, last_visible);
          let ld_text = buffer_line_text(&state.buffer, last);
          let nx_text = if next < line_count { buffer_line_text(&state.buffer, next) } else { "(end)".into() };
          let widget_h = state.text_view.height();
          let desc_guard = descender_guard_px(&state.text_view, state.page_top_line);
          log_fmt!("PAGE_FWD: page_top={} last_visible={} last_dialogue={} next={}", state.page_top_line, last_visible, last, next);
          log_fmt!("PAGE_FWD: widget_h={} desc_guard={} usable_h={}", widget_h, desc_guard, widget_h - desc_guard);
          log_fmt!("PAGE_FWD: last_visible_text='{}'", lv_text.chars().take(60).collect::<String>());
          log_fmt!("PAGE_FWD: last_dialogue_text='{}'", ld_text.chars().take(60).collect::<String>());
          log_fmt!("PAGE_FWD: next_text='{}'", nx_text.chars().take(60).collect::<String>());
          // Log heights of lines near the boundary
          for i in last_visible.saturating_sub(2)..=(last_visible + 2).min(line_count - 1) {
              if let Some(iter) = state.buffer.iter_at_line(i as i32) {
                  let (_y, h) = state.text_view.line_yrange(&iter);
                  let t = buffer_line_text(&state.buffer, i);
                  log_fmt!("PAGE_FWD: line {} h={} '{}'", i, h, t.chars().take(50).collect::<String>());
              }
          }
      }

      if next >= line_count {
          return; // already at end
      }
  ```

  Note the changes:
  - Added `let new_top = next_page_top(state, state.page_top_line);` near the top.
  - Removed `let new_top = back_up_for_speaker(&state.buffer, next);` from
    after the `if next >= line_count` guard.

  The variables `last_visible`, `last`, `next` are kept because they feed the
  debug-log block. `new_top` now matches what `next_page_top` produces, which
  matches the prior inline computation by construction (same call sequence).

- [ ] **Step 3: Build and run all tests**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
  Expected: clean. Behavior of j (`page_forward`) is unchanged because
  `next_page_top` reproduces the same `last_fully_visible_line` →
  `last_dialogue_in_page` → `next_dialogue_from` → `back_up_for_speaker`
  pipeline.

- [ ] **Step 4: Smoke-test j navigation interactively (manual)**

  This task changes a hot path. Although `cargo build` is the contract, ask the
  user to do a quick manual sanity check (open a play and a prose work, press j
  a few times, confirm pages turn as before) before moving on. The user runs
  the app — do not run `cargo run` yourself.

- [ ] **Step 5: Commit**

  ```
  git add src/input/navigation.rs
  git commit -m "Extract next_page_top helper; reuse in page_forward"
  ```

---

## Task 4: Add `viewport_page_for_line`

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add the public helper**

  In `src/input/navigation.rs`, immediately below the `next_page_top` function
  added in Task 3, add:

  ```rust
  /// Return the 1-indexed viewport page that contains `target_line`, computed
  /// by replaying j-key page-forward from line 0. Used by the bottom-overlay
  /// label to display "page - line_id" on prose works.
  ///
  /// Returns 1 for an empty work or a target at the start. The loop has a
  /// safety break if `next_page_top` fails to advance, so it cannot run away.
  pub fn viewport_page_for_line(state: &AppState, target_line: usize) -> usize {
      let line_count = state.effective_line_count();
      if line_count == 0 {
          return 1;
      }
      let mut page: usize = 1;
      let mut top: usize = 0;
      while top < line_count {
          let next_top = next_page_top(state, top);
          // target is on the current page if next page starts strictly after it
          if next_top > target_line {
              return page;
          }
          // safety: no progress means we're stuck — bail with current page
          if next_top <= top {
              return page;
          }
          top = next_top;
          page += 1;
      }
      page.saturating_sub(1).max(1)
  }
  ```

- [ ] **Step 2: Build and clippy**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  ```
  Expected: clean. No tests added in this step — this helper is integration-
  tested in Task 6.

- [ ] **Step 3: Commit**

  ```
  git add src/input/navigation.rs
  git commit -m "Add viewport_page_for_line: 1-indexed page of the current line"
  ```

---

## Task 5: Wire the prefix into the prose branch of `page_label_text_for_buffer`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Locate the call site**

  ```
  rg -n "page_label_text_for_buffer|return Some\(format!\(\"\{\}\", line\.id" src/app.rs
  ```
  Expected: the function definition and one `return Some(format!("{}", line.id))`
  inside it.

- [ ] **Step 2: Replace the prose branch**

  In `src/app.rs`, inside `page_label_text_for_buffer` (around line 193), find
  the prose branch — the line:

  ```rust
              return Some(format!("{}", line.id));
  ```

  Replace it with:

  ```rust
              let page = crate::input::navigation::viewport_page_for_line(self, idx);
              return Some(crate::ui::page_label::format_prose_label(page, line.id));
  ```

  Note `idx` is the buffer line that mapped to a real work line via the
  pre-existing forward-scan past spacers. Using `idx` (not the original
  `buffer_line` parameter) keeps the page consistent with the line whose id is
  shown.

- [ ] **Step 3: Build and clippy**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  ```
  Expected: clean build, no warnings.

- [ ] **Step 4: Run all unit tests**

  ```
  cargo test
  ```
  Expected: all unit tests pass, including the new `prose_label_*` tests from
  Task 1.

- [ ] **Step 5: Manual smoke check (user runs the app)**

  Ask the user to launch the app, open a prose work (e.g. a novel), and confirm:
  - The bottom label reads `<page> - <line_id>` (not just `<line_id>`).
  - On page 1 of the work, the prefix is `1`.
  - After tapping j, the prefix increments by 1.
  - Opening a play still shows `I.i.15`-style citations with no page prefix.

  Do not run `cargo run` yourself — the user runs it.

- [ ] **Step 6: Commit**

  ```
  git add src/app.rs
  git commit -m "Show '<page> - <line_id>' on prose line label"
  ```

---

## Task 6: Headless integration test

Extend the prose-navigation headless test fixture to assert the page prefix
appears with the right value after j-presses.

**Files:**
- Modify: a test under `tests/` (the existing prose-navigation test). Locate
  it in step 1.

- [ ] **Step 1: Locate the prose-navigation test**

  ```
  fd -e rs -p prose tests/
  rg -n "page_forward|page_label_text_for_buffer" tests/
  ```
  Pick the existing prose-navigation integration test (the one driven by the
  `test-prose-navigation` skill — likely under `tests/` and named with `prose`
  or `bleak_house`). Read it to understand its fixture, including how it
  obtains an `AppState` and how it drives navigation.

  If the test file structure differs from this plan's assumptions (e.g. the
  prose-navigation test isn't a Rust integration test but a script that drives
  the binary), stop and consult with the user before adapting the test plan.

- [ ] **Step 2: Add a test that asserts the prefix**

  Append a new `#[test]` (or async-test, matching the existing style) named
  `prose_label_has_page_prefix`. Pseudocode (adapt to the fixture's actual
  helpers):

  ```rust
  #[test]
  fn prose_label_has_page_prefix() {
      let mut state = setup_prose_state(); // uses existing fixture helper
      // page 1: prefix should be "1 - "
      let label1 = state.page_label_text_for_buffer(state.current_line)
          .expect("label on page 1");
      assert!(
          label1.starts_with("1 - "),
          "expected page 1 prefix, got {label1:?}"
      );

      // tap j once
      crate::input::navigation::page_forward(&mut state);
      let label2 = state.page_label_text_for_buffer(state.current_line)
          .expect("label on page 2");
      assert!(
          label2.starts_with("2 - "),
          "expected page 2 prefix, got {label2:?}"
      );
  }
  ```

  Use the same imports, async/await, and fixture-builder calls as the
  surrounding tests in the file. If the existing fixture exposes a different
  way to read the bottom label (e.g. a `current_label_text()` helper), use it
  instead of calling `page_label_text_for_buffer` directly.

- [ ] **Step 3: Run the new test**

  ```
  cargo test --test <prose_test_file_stem> prose_label_has_page_prefix
  ```
  Expected: PASS. If it fails because the test fixture's `AppState` lacks a
  realized `text_view` size (descender-guard math returns 0 / `widget_height
  <= 0`), the helper will return `top` (i.e. page never advances). In that
  case, set the text view size in the fixture using the same approach the
  existing `page_forward` tests use, or skip this assertion form and assert
  via a separate helper that uses a `top` you control. Do not weaken the test
  to make it pass — fix the fixture.

- [ ] **Step 4: Run the full test suite**

  ```
  cargo test
  cargo clippy --all-targets -- -D warnings
  ```
  Expected: clean.

- [ ] **Step 5: Commit**

  ```
  git add tests/
  git commit -m "Test: prose label shows correct '<page> - ' prefix"
  ```

---

## Final verification

- [ ] **Step 1: Final build, clippy, test**

  ```
  cargo build
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
  Expected: clean across the board.

- [ ] **Step 2: Manual UX check (user runs the app)**

  Ask the user to confirm:

  - Prose work: bottom label shows `<page> - <line_id>`. Page = 1 at the start
    of the work. Page increments by 1 on each j-press.
  - Play: bottom label still shows the citation only (`I.i.15`,
    `Prologue 31`, etc.) — no page prefix.
  - Comma / q (sentence/dialogue navigation) updates the page prefix when it
    crosses a page boundary.
  - gg jumps back to page 1.
  - Library picker → loading a different work resets the prefix to page 1 of
    the new work.

  If anything looks off, stop and surface the issue before declaring done.

---

## Self-review notes

- **Spec coverage:**
  - "prose shows `<page> - <line_id>`" → Task 5.
  - "plays unchanged" → Task 5 only edits the prose branch; play branch is
    untouched (verified by reading `page_label_text_for_buffer`'s structure
    where the play branch returns earlier).
  - "page = j-presses from line 0 + 1" → Task 4.
  - "recomputes every label refresh" → Task 5 calls the helper inline; no
    caching.
  - "no DB pages, no stable-across-resize" → no DB or cache code added.

- **Placeholder scan:** all code blocks contain real, complete code. Only the
  test in Task 6 is pseudocode-shaped because the fixture's exact helpers
  aren't known until Step 1 of that task — Step 1 explicitly tells the
  engineer to read the fixture first, and Step 2 says "adapt to the fixture's
  actual helpers." This is the minimum honest level of detail without
  speculating about the fixture.

- **Type consistency:**
  - `next_page_top(state: &AppState, top: usize) -> usize` — used identically
    in Task 3 and Task 4.
  - `viewport_page_for_line(state: &AppState, target_line: usize) -> usize` —
    defined in Task 4, called in Task 5 with `(self, idx)` where `self:
    &AppState` (consistent — `page_label_text_for_buffer` is a method on
    `AppState`).
  - `format_prose_label(page: usize, line_id: i64) -> String` — defined in
    Task 1, called in Task 5 with `(page, line.id)` where `line.id: i64`
    (consistent with existing `format!("{}", line.id)`).

# Prose NYTimes-Style Centered Column Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reposition prose-work text (main reading card + synopsis, gloss, journal overlays) into a centered, symmetric ~⅓-margin column (`card_width/5`) matching a nytimes.com article body.

**Architecture:** A single shared helper `prose_column_margin(card_width) = card_width / 5` is used by four surfaces. The two overlays that render both verse and prose (gloss, journal) learn the work is prose via a stateful `set_prose(bool)` setter called once per work load in `display_work_at_with_prepared`. The synopsis overlay already has a prose branch (`SynopsisProseCard`); only its margin values change. The main reading card's prose-monocle branch in `apply_tiled_mode` switches to the symmetric inset. Verse/play/two-column/sonnet/translation/BCP paths are untouched.

**Tech Stack:** Rust, GTK4 (gtk4-rs), `sourceview5::View` / `gtk4::TextView`, Cairo software rendering for tests.

## Global Constraints

- **Prose only.** All changes are guarded by `crate::db::line_types::is_prose_work(work_type)`. Verse, two-column, sonnet-sequence, translation, BCP, and tiled paths must render byte-identically to before.
- **Ratio is `card_width / 5`** (≈210px each side on the 1050px default card → ~630px centered column). Both left and right margins are symmetric.
- **`cargo build` and `cargo test --bins` must stay green** after every task.
- Do **not** run `cargo run` — the user runs the app. Runtime/visual verification is the user's (see Task 6).
- House commit-message footer (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
  ```
- Branch: `feat/prose-nyt-column` (already created off master; the spec commit `21c3595` is already here).

---

### Task 1: Shared `prose_column_margin` helper

**Files:**
- Modify: `src/ui/mod.rs` (add fn next to `card_side_margin` at line 49-51; add a test to the existing `#[cfg(test)]` region — note `mod bottom_clip_tests` starts at line 226, so add a NEW sibling test module).

**Interfaces:**
- Consumes: nothing.
- Produces: `pub(crate) fn prose_column_margin(card_width: i32) -> i32` — returns `card_width / 5`. Used by Tasks 2-5.

- [ ] **Step 1: Write the failing test**

Add a new test module at the END of `src/ui/mod.rs` (after the last existing `#[cfg(test)] mod ...` block):

```rust
#[cfg(test)]
mod prose_column_tests {
    use super::prose_column_margin;

    #[test]
    fn fifth_of_card_each_side() {
        // Default 1050px card -> 210px each side -> ~630px centered column.
        assert_eq!(prose_column_margin(1050), 210);
    }

    #[test]
    fn wide_card_scales() {
        assert_eq!(prose_column_margin(1660), 332);
    }

    #[test]
    fn zero_card_is_zero() {
        assert_eq!(prose_column_margin(0), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins prose_column_tests 2>&1 | tail -20`
Expected: FAIL — compile error `cannot find function 'prose_column_margin' in this scope` (or `module 'super' has no ... 'prose_column_margin'`).

- [ ] **Step 3: Write minimal implementation**

In `src/ui/mod.rs`, immediately after the existing `card_side_margin` fn (which ends at line 51), add:

```rust
/// Symmetric inset (both sides) for the NYTimes-style centered prose column.
/// Wider whitespace than `card_side_margin` (card_width/4): a ~⅓-margin
/// reading measure. Used by the prose reading card and the prose overlays
/// (synopsis/gloss/journal) so prose body text reads like a newspaper column,
/// centered with generous left/right margins. Verse/play surfaces keep
/// `card_side_margin`.
pub(crate) fn prose_column_margin(card_width: i32) -> i32 {
    card_width / 5
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins prose_column_tests 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/mod.rs
git commit -m "$(cat <<'EOF'
feat(prose): add prose_column_margin (card_width/5) helper

Shared symmetric inset for the NYTimes-style centered prose column,
used by the prose reading card and the prose overlays.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
EOF
)"
```

---

### Task 2: Main reading card — prose monocle symmetric column

**Files:**
- Modify: `src/app/layout.rs` (the prose `else` branch of the `left_bump` chain at ~line 130-138, and the right-margin `else` at ~line 195-198, inside `apply_tiled_mode`).

**Interfaces:**
- Consumes: `crate::ui::prose_column_margin` (Task 1); `target_card_width`, `card_side_margin` (existing, same file/module).
- Produces: prose-monocle reading card with left == right == `prose_column_margin(card_w)`. No new public symbols.

There is no pure unit test for `apply_tiled_mode` (it mutates live GTK widgets; the codebase deliberately has none for `column_split`/`last_page_top`). Verification for this task is the build plus the user's visual check in Task 6. The change is a localized edit mirroring the existing `translations_visible` branch.

- [ ] **Step 1: Read the current prose left-margin branch**

Run: `sed -n '100,140p' src/app/layout.rs`
Confirm the `let left_bump = if state.translations_visible { ... } else if ... else if is_verse { super::VERSE_LEFT_OFFSET } else { super::PROSE_LEFT_OFFSET };` chain, and that `logical_left = state.config.text_margins as i32 + left_bump;` follows at ~line 139.

- [ ] **Step 2: Change the prose `else` branch to the centered inset**

In `src/app/layout.rs`, replace the final prose arm of the `left_bump` chain. The current code is:

```rust
    } else if is_verse {
        super::VERSE_LEFT_OFFSET
    } else {
        super::PROSE_LEFT_OFFSET
    };
    let logical_left = state.config.text_margins as i32 + left_bump;
```

Change it to:

```rust
    } else if is_verse {
        super::VERSE_LEFT_OFFSET
    } else {
        // Prose monocle: a centered NYTimes-style column. Inset is a fraction of
        // the ACTUAL on-screen card width (clamped to the window), so the text
        // centers in the cream card with ~1/3 whitespace each side. Subtract the
        // base text_margins so logical_left lands exactly at prose_column_margin.
        // Mirrors the translations_visible branch's card-relative inset.
        let card_w = target_card_width(
            window_width, state.config.column_width, state.column_count(), false,
        ).min(window_width.max(1));
        (crate::ui::prose_column_margin(card_w) - state.config.text_margins as i32).max(0)
    };
    let logical_left = state.config.text_margins as i32 + left_bump;
```

(Note: `tiled` is handled by the earlier `} else if tiled { 0 }` arm, so this prose arm only runs untiled. `is_verse` is false here, so this is prose. `two_col` is handled earlier, so this is single-column.)

- [ ] **Step 3: Change the prose right-margin `else` branch to symmetric**

In the same function, the right-margin block currently ends with:

```rust
    } else {
        let logical_right = state.config.text_margins as i32
            + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(logical_right);
    }
```

This `else` is reached for the single-column non-translation, non-`one_section_per_page`, non-`two_col` case — which covers BOTH verse monocle and prose monocle. Verse monocle must keep `EXTRA_RIGHT_MARGIN`; only prose changes. Replace with:

```rust
    } else if !is_verse {
        // Prose monocle: symmetric right margin == the centered left inset, so
        // the column is centered in the card (NYTimes body look). Recompute the
        // same card-relative value used for logical_left above.
        let card_w = target_card_width(
            window_width, state.config.column_width, state.column_count(), false,
        ).min(window_width.max(1));
        state.text_view.set_right_margin(crate::ui::prose_column_margin(card_w));
    } else {
        let logical_right = state.config.text_margins as i32
            + crate::config::EXTRA_RIGHT_MARGIN;
        state.text_view.set_right_margin(logical_right);
    }
```

`is_verse` is already in scope in this function (computed near the top: `let is_verse = !crate::db::line_types::is_prose_work(&work_type);`).

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles clean (no errors). A warning-free build is expected; if `target_card_width` is flagged unused-import etc., it is already imported/in-module (same file), so there should be none.

- [ ] **Step 5: Run the bins test suite to confirm nothing regressed**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS (same count as before plus Task 1's 3 tests).

- [ ] **Step 6: Commit**

```bash
git add src/app/layout.rs
git commit -m "$(cat <<'EOF'
feat(prose): center the prose monocle reading card column

Prose single-column reading now uses a symmetric card_width/5 inset on
both sides (NYTimes body look) instead of the asymmetric
PROSE_LEFT_OFFSET (160px) left / EXTRA_RIGHT_MARGIN (88px) right.
Verse monocle, two-column, sonnet, translation, and tiled paths unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
EOF
)"
```

---

### Task 3: Synopsis overlay — prose card uses the centered column

**Files:**
- Modify: `src/app/scene_synopsis.rs` (`prose_synopsis_card` at lines 385-398: add a `card_width` param, change the two margin values).
- Modify: `src/app/scene_synopsis.rs` (two call sites: `show_synopsis_overlay` ~line 370, `cycle_synopsis` ~line 526).
- Modify: `src/input/actions/synopsis.rs` (three call sites: ~lines 180, 195, 265).

**Interfaces:**
- Consumes: `crate::ui::prose_column_margin` (Task 1).
- Produces: `pub fn prose_synopsis_card(state: &AppState, card_width: i32) -> Option<SynopsisProseCard>` — same return type, new second param. All 5 call sites updated.

`prose_synopsis_card` returns a plain struct from pure inputs, so it IS unit-testable, but it needs an `AppState` (heavy to construct). Instead, assert the geometry indirectly: the function's prose branch now computes `left_margin == right_margin == prose_column_margin(card_width)`. We verify by build + the Task 1 helper test + the user's visual check (Task 6). No new unit test for this task (consistent with the rest of the synopsis module, which has no AppState-constructing tests).

- [ ] **Step 1: Add `card_width` param and change the margins**

In `src/app/scene_synopsis.rs`, the current function is:

```rust
pub fn prose_synopsis_card(state: &AppState) -> Option<crate::ui::gloss_overlay::SynopsisProseCard> {
    let is_prose = crate::db::line_types::is_prose_work(
        state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or(""),
    );
    if !is_prose {
        return None;
    }
    Some(crate::ui::gloss_overlay::SynopsisProseCard {
        font_family: state.config.font_family.clone(),
        font_size: state.config.font_size as i32,
        left_margin: state.config.text_margins as i32 + crate::app::PROSE_LEFT_OFFSET,
        right_margin: state.config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN,
    })
}
```

Replace it with (note the new `card_width: i32` param and the two margin lines):

```rust
pub fn prose_synopsis_card(state: &AppState, card_width: i32) -> Option<crate::ui::gloss_overlay::SynopsisProseCard> {
    let is_prose = crate::db::line_types::is_prose_work(
        state.current_work.as_ref().map(|w| w.work_type.as_str()).unwrap_or(""),
    );
    if !is_prose {
        return None;
    }
    // Centered NYTimes-style column: symmetric card_width/5 inset, matching the
    // prose reading card and the prose gloss/journal overlays.
    let margin = crate::ui::prose_column_margin(card_width);
    Some(crate::ui::gloss_overlay::SynopsisProseCard {
        font_family: state.config.font_family.clone(),
        font_size: state.config.font_size as i32,
        left_margin: margin,
        right_margin: margin,
    })
}
```

Also update the doc comment just above the fn (lines ~379-384) so the stale "fixed pixel left padding (`text_margins + PROSE_LEFT_OFFSET`)" wording matches the new behavior. Replace that doc block's body with:

```rust
/// Card-matching synopsis layout for PROSE works: the main reading card's font
/// (family + size) and the centered NYTimes-style column inset
/// (`prose_column_margin(card_width)`, symmetric), so a prose synopsis reads
/// like its reading card. Returns `None` for plays/verse, which keep the
/// overlay's Charter + `card_width/4` inset look. Shared by every `show_synopsis`
/// call (open, amend, edit, undo) so a re-render after an edit keeps the layout.
```

- [ ] **Step 2: Update the two call sites in `scene_synopsis.rs`**

At ~line 367-370 in `show_synopsis_overlay`, the context is:

```rust
    let (card_width, card_height) = overlay_card_size(&s);
    let label = synopsis_label(&s, div1, div2);
    let root_color = s.theme.root_color.clone();
    let prose_card = prose_synopsis_card(&s);
```

Change the last line to:

```rust
    let prose_card = prose_synopsis_card(&s, card_width);
```

At ~line 524-526 in `cycle_synopsis`, the context is:

```rust
    let (card_width, card_height) = overlay_card_size(&s);
    let root_color = s.theme.root_color.clone();
    let prose_card = prose_synopsis_card(&s);
```

Change the last line to:

```rust
    let prose_card = prose_synopsis_card(&s, card_width);
```

- [ ] **Step 3: Update the three call sites in `input/actions/synopsis.rs`**

These three sites compute `cw` (the card width passed to `show_synopsis`) just above each `prose_synopsis_card` call. Pass that same `cw`.

At ~line 180:
```rust
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s);
```
becomes
```rust
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
```

At ~line 195:
```rust
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s);
```
becomes
```rust
            let prose_card = crate::app::scene_synopsis::prose_synopsis_card(&s, cw);
```

At ~line 265 (read the surrounding lines first to confirm the local card-width variable name — it may be `cw` or `card_width`):

Run: `sed -n '255,270p' src/input/actions/synopsis.rs`

Then change the `prose_synopsis_card(&s)` on/near line 265 to pass that local card-width variable (e.g. `prose_synopsis_card(&s, cw)`).

- [ ] **Step 4: Build (catches any missed call site)**

Run: `cargo build 2>&1 | tail -25`
Expected: compiles clean. If the compiler reports `this function takes 2 arguments but 1 argument was supplied`, a `prose_synopsis_card(&s)` call site was missed — fix it to pass the in-scope card width, then rebuild. Confirm zero remaining 1-arg calls:

Run: `rg -n 'prose_synopsis_card\(&s\)' src/`
Expected: no matches.

- [ ] **Step 5: Run the bins test suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app/scene_synopsis.rs src/input/actions/synopsis.rs
git commit -m "$(cat <<'EOF'
feat(prose): center the prose synopsis overlay column

prose_synopsis_card now takes the card width and produces a symmetric
prose_column_margin (card_width/5) inset, matching the prose reading
card. Verse/play synopses keep the card_width/4 inset.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
EOF
)"
```

---

### Task 4: Gloss overlay — `set_prose` setter + centered prose column

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (add `is_prose: Cell<bool>` field to `struct GlossOverlay` ~line 61; init in `new()` return block ~line 418; add `pub fn set_prose`; use it in `show_gloss_with_color` ~line 635 and `show_glossing` ~line 722).
- Modify: `src/app/mod.rs` (call `set_prose` in `display_work_at_with_prepared` ~line 2604).

**Interfaces:**
- Consumes: `crate::ui::prose_column_margin` (Task 1); `crate::db::line_types::is_prose_work`.
- Produces: `pub fn set_prose(&self, is_prose: bool)` on `GlossOverlay`, storing into `is_prose: Cell<bool>` (default `false`). `show_gloss_with_color` and `show_glossing` read it to pick the left/right inset.

- [ ] **Step 1: Add the `is_prose` field to the struct**

In `src/ui/gloss_overlay.rs`, in `struct GlossOverlay`, add a field near `text_margins` (line 61). Insert after line 62 (`column_width: i32,`):

```rust
    /// True when the currently-loaded work is prose. Set once per work load via
    /// `set_prose` (from display_work). Selects the centered prose column inset
    /// (card_width/5) over the verse `card_width/4` inset in the gloss render.
    is_prose: Cell<bool>,
```

- [ ] **Step 2: Initialize it in the constructor**

In `new()`'s returned `Self { ... }` (the block ending ~line 424), add after `column_width: column_width as i32,` (line 414):

```rust
            is_prose: Cell::new(false),
```

- [ ] **Step 3: Add the `set_prose` setter**

Add a method inside `impl GlossOverlay` (e.g. right after `new()` ends ~line 425, before `adjust_font_size`):

```rust
    /// Record whether the loaded work is prose, so the gloss render picks the
    /// centered prose column inset. Called once per work load from display_work.
    pub fn set_prose(&self, is_prose: bool) {
        self.is_prose.set(is_prose);
    }
```

- [ ] **Step 4: Use it in `show_gloss_with_color`**

In `show_gloss_with_color`, the current margin code (~lines 634-638) is:

```rust
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
```

Change the first line so prose uses the centered inset:

```rust
        let left = if self.is_prose.get() {
            crate::ui::prose_column_margin(card_width)
        } else {
            crate::ui::card_side_margin(card_width)
        };
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
```

The `bar_left` a few lines down (~line 655) is `crate::ui::card_side_margin(card_width)`. Change it to reuse `left` so the accent bar tracks the prose body:

```rust
        let bar_left = left;
        *self.bar_x.borrow_mut() = bar_left;
```

(Confirm the current line reads `let bar_left = crate::ui::card_side_margin(card_width);` before replacing.)

- [ ] **Step 5: Use it in `show_glossing` (the loading card)**

In `show_glossing`, the margin code (~lines 722-725) is:

```rust
        let left = crate::ui::card_side_margin(card_width);
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
```

Change identically:

```rust
        let left = if self.is_prose.get() {
            crate::ui::prose_column_margin(card_width)
        } else {
            crate::ui::card_side_margin(card_width)
        };
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.gloss_view.set_right_margin(left);
```

And the `bar_left` at ~line 745 (`let bar_left = crate::ui::card_side_margin(card_width);`) becomes:

```rust
        let bar_left = left;
```

- [ ] **Step 6: Call `set_prose` on work load**

In `src/app/mod.rs`, in `display_work_at_with_prepared`, the line at ~2604 already computes the work type:

```rust
    let work_type = state.current_work.as_ref().map(|w| w.work_type.clone()).unwrap_or_default();
    let vbox = state.vbox.clone();
    let ww = state.window.width();
    apply_tiled_mode(state, &vbox, ww);
```

Insert, right after the `work_type` line and before `apply_tiled_mode`:

```rust
    let work_is_prose = crate::db::line_types::is_prose_work(&work_type);
    state.gloss_overlay.set_prose(work_is_prose);
```

(The journal overlay setter is added in Task 5; this line is gloss-only for now.)

- [ ] **Step 7: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles clean.

- [ ] **Step 8: Run the bins test suite (covers the gloss_overlay render test at line 1793)**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS. The standalone `show_glossing` test constructs `GlossOverlay::new(1050, 80)` whose `is_prose` defaults to `false`, so it still exercises the verse path and its tag assertions hold unchanged.

- [ ] **Step 9: Commit**

```bash
git add src/ui/gloss_overlay.rs src/app/mod.rs
git commit -m "$(cat <<'EOF'
feat(prose): center the prose gloss overlay column

GlossOverlay gains an is_prose flag (set once per work load from
display_work); show_gloss_with_color and show_glossing use
prose_column_margin (card_width/5) for prose works and keep
card_side_margin (card_width/4) for verse. Accent bar tracks the body.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
EOF
)"
```

---

### Task 5: Journal overlay — `set_prose` setter + centered prose column

**Files:**
- Modify: `src/ui/journal_overlay.rs` (add `is_prose: Cell<bool>` field ~line 27; init in `new()` ~line 211; add `set_prose`; use it in `size_card` ~line 242; re-assert verse inset in `show_passage_page` ~line 356).
- Modify: `src/app/mod.rs` (call `journal_overlay.set_prose` alongside the gloss one, ~line 2604, reusing `work_is_prose` from Task 4).

**Interfaces:**
- Consumes: `crate::ui::prose_column_margin` (Task 1); `work_is_prose` local from Task 4.
- Produces: `pub fn set_prose(&self, is_prose: bool)` on `JournalOverlay`, default `false`.

- [ ] **Step 1: Add the `is_prose` field to the struct**

In `src/ui/journal_overlay.rs`, in `struct JournalOverlay`, after `column_width: i32,` (line 27) add:

```rust
    /// True when the loaded work is prose. Set once per work load via
    /// `set_prose`. Selects the centered prose column inset (card_width/5) over
    /// the verse `card_width/4` inset in `size_card`.
    is_prose: Cell<bool>,
```

- [ ] **Step 2: Initialize it in the constructor**

In `new()`'s `Self { ... }` block, after `column_width: column_width as i32,` (line 211) add:

```rust
            is_prose: Cell::new(false),
```

- [ ] **Step 3: Add the `set_prose` setter**

Inside `impl JournalOverlay`, after `attach` (~line 223) or right before `size_card`, add:

```rust
    /// Record whether the loaded work is prose, so `size_card` picks the
    /// centered prose column inset. Called once per work load from display_work.
    pub fn set_prose(&self, is_prose: bool) {
        self.is_prose.set(is_prose);
    }
```

- [ ] **Step 4: Use it in `size_card`**

In `size_card`, the current inset code (~lines 242-245) is:

```rust
        let side = crate::ui::card_side_margin(card_width);
        self.view.set_left_margin(side);
        self.view.set_right_margin(side);
        self.title.set_margin_start(side);
```

Change the first line:

```rust
        let side = if self.is_prose.get() {
            crate::ui::prose_column_margin(card_width)
        } else {
            crate::ui::card_side_margin(card_width)
        };
        self.view.set_left_margin(side);
        self.view.set_right_margin(side);
        self.title.set_margin_start(side);
```

- [ ] **Step 5: Keep verse inset for `show_passage_page`**

`show_passage_page` renders source verse and calls `size_card` first (~line 336), which now sets the prose inset when `is_prose`. Re-assert the verse inset right after the `size_card` call so passage verse keeps the `card_width/4` measure. The current head of `show_passage_page` is:

```rust
        self.size_card(card_width, card_height);
        self.title.set_text("Passage");
```

Change to:

```rust
        self.size_card(card_width, card_height);
        // Passage pages render source VERSE, not prose body — keep the verse
        // inset (card_width/4) even inside a prose work, overriding size_card's
        // prose inset. (bar_left + populate_verse_buffer below already use it.)
        let verse_side = crate::ui::card_side_margin(card_width);
        self.view.set_left_margin(verse_side);
        self.view.set_right_margin(verse_side);
        self.title.set_margin_start(verse_side);
        self.title.set_text("Passage");
```

- [ ] **Step 6: Call `journal_overlay.set_prose` on work load**

In `src/app/mod.rs`, extend the snippet added in Task 4 Step 6. It currently reads:

```rust
    let work_is_prose = crate::db::line_types::is_prose_work(&work_type);
    state.gloss_overlay.set_prose(work_is_prose);
```

Add the journal line:

```rust
    let work_is_prose = crate::db::line_types::is_prose_work(&work_type);
    state.gloss_overlay.set_prose(work_is_prose);
    state.journal_overlay.set_prose(work_is_prose);
```

- [ ] **Step 7: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles clean.

- [ ] **Step 8: Run the bins test suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui/journal_overlay.rs src/app/mod.rs
git commit -m "$(cat <<'EOF'
feat(prose): center the prose journal overlay column

JournalOverlay gains an is_prose flag (set per work load from
display_work); size_card uses prose_column_margin (card_width/5) for
prose works. show_passage_page re-asserts the verse inset since
passages render source verse, not prose body.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01TWLw43hux2Rjhqew7M9QGn
EOF
)"
```

---

### Task 6: Verify build, lint, and hand off the visual check

**Files:** none (verification only).

**Interfaces:** none.

This is a geometry change whose real acceptance criterion is "renders centered like the screenshot." The agent cannot reliably drive cage on the live dwl seat, so the rendered check is the user's. This task confirms the code is green and prepares the exact commands for the user.

- [ ] **Step 1: Full build**

Run: `cargo build 2>&1 | tail -20`
Expected: clean compile.

- [ ] **Step 2: Clippy (no new warnings)**

Run: `cargo clippy 2>&1 | tail -30`
Expected: no new warnings introduced by these changes (pre-existing warnings, if any, are out of scope).

- [ ] **Step 3: Full bins test suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS, including `prose_column_tests` (3) and the unchanged gloss/journal/synopsis suites.

- [ ] **Step 4: Confirm no stray 1-arg `prose_synopsis_card` or leftover constant use**

Run:
```bash
rg -n 'prose_synopsis_card\(&s\)' src/ ; echo "---" ; rg -n 'PROSE_LEFT_OFFSET' src/app/layout.rs src/app/scene_synopsis.rs
```
Expected: first command no matches; second command no matches in those two files (the constant may still be defined in `mod.rs` and used by verse paths — that's fine).

- [ ] **Step 5: Hand off the visual verification to the user**

Tell the user the change is built and unit-green but the centered-column look must be eyeballed on a rendered spread, and give them the commands. Prose work (should now be a centered ~630px column on a 1050px card) — main card, then each overlay:

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
# then, in the cage socket (check ls /run/user/1000/wayland-*):
#   export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
#   grim /tmp/shot-main.png         # main reading card (prose)
#   wtype "h"  ; grim /tmp/shot-syn.png    # synopsis overlay
#   (Escape, then Ctrl+g for gloss; Alt+w for journal)
pkill -f "cage -- ./target/debug/linux-lit"; pkill -f target/debug/linux-lit
```

Or simply `cargo run` on a prose work (e.g. Bleak House) and a verse work (e.g. Rom) and compare: prose surfaces centered with wide symmetric margins; verse surfaces unchanged.

Ask the user to confirm the spread (or paste screenshots) before the branch is merged. Do NOT claim visual success without their confirmation.

- [ ] **Step 6: Finish the branch (only after the user confirms the visual check)**

Per `CLAUDE.md` "Finishing a Branch": once the user confirms it renders correctly and the working tree is clean, merge back to master locally and push:

```bash
git checkout master
git merge --no-ff feat/prose-nyt-column
cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -5
git push origin master
git branch -d feat/prose-nyt-column
```

---

## Self-Review notes

- **Spec coverage:** main reading card (Task 2), synopsis overlay (Task 3), gloss overlay (Task 4), journal overlay (Task 5), shared `prose_column_margin` helper (Task 1), `card_width/5` ratio + symmetry + prose-only guard (every task), tiled degradation (Task 2 relies on the earlier `tiled` arm), `show_passage_page` verse exception (Task 5 Step 5), testing/verification handoff (Task 6). All spec sections map to a task.
- **Type consistency:** `prose_column_margin(i32) -> i32` (Task 1) used identically in Tasks 2-5. `set_prose(&self, bool)` defined on both overlays (Tasks 4-5) and called with `work_is_prose: bool` (Task 4 Step 6 / Task 5 Step 6). `prose_synopsis_card(&AppState, i32)` (Task 3) called with the in-scope card width at all 5 sites.
- **No placeholders:** every code step shows the full before/after snippet and exact verification command.

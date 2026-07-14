# Per-Work Column Layout Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the two-column e-reader layout a per-work property that defaults ON for Shakespeare plays, with `Alt+[` storing a persisted per-work override.

**Architecture:** Replace the single global `Config.column_count: u8` with a per-work `Config.column_overrides: HashMap<String, u8>` (keyed by work abbrev). `AppState::column_count()` resolves to: scroll mode → 1; else override-for-abbrev if present, else `default_column_count_for(work)` (2 for a Shakespeare play, else 1). The work-load display path renders the resolved count (shows/hides the right column); `Alt+[` flips and stores the override.

**Tech Stack:** Rust, GTK4, sourceview5, serde_json.

**Spec:** `docs/superpowers/specs/2026-06-01-two-column-ereader-layout-design.md` (Per-work column layout section).

**Builds on:** the completed two-column feature (branch `two-column-layout`). All the column_split / scroll / clip / toggle machinery exists; this plan only changes how `column_count()` is sourced and adds the per-work default + load-time render.

---

## Background

- `Config.column_count: u8` (default 1) currently exists at `src/config.rs:43`, with `fn default_column_count() -> u8 { 1 }` (~line 110) and a `Default` impl entry (~line 150). It is read ONLY by `AppState::column_count()` (`src/app.rs:317`) and written ONLY by `toggle_column_layout` (`src/input/navigation.rs:257-258`).
- `AppState::column_count()` (`src/app.rs:315-322`): EReader → `config.column_count.clamp(1,2)`, Scroll → 1.
- `toggle_column_layout` (`src/input/navigation.rs:249`): flips `config.column_count`, saves, shows/hides `right_scrolled_overlay`, invalidates page-tops, recomputes page.
- `Config` already has a per-work map pattern: `work_positions: HashMap<String, usize>` (`config.rs:55`, `#[serde(default)]`, init `HashMap::new()` in Default).
- `Work` has `author: String`, `work_type: String`, `abbrev: String` (`src/db/models.rs`).
- DB facts: Shakespeare's `author` string is exactly `"Shakespeare"`; his `work_type` values are `play` (77), `narrative_poem`, `poem`, `sonnet_sequence`. Only `play` should default to two columns.
- Work load: `display_work_at_with_prepared` (`src/app.rs:1667`) sets `state.current_work = Some(work)` at line 1850, then does layout setup (e.g. `state.text_view.set_pixels_above_lines(ls)` at 1873-1874). The right column overlay is currently only shown by `toggle_column_layout`, so on load a play would show the overlay HIDDEN despite `column_count()` returning 2 — the load path must sync visibility.
- This is a BINARY crate: test with `cargo test --bin linux-lit` (no `--lib`). Baseline before this work: 195 passed / 2 failed (the 2 are pre-existing `block_atom_tests`, unrelated).
- Per-keybind two-file rule does NOT apply here (no keybind changes; `Alt+[` already bound).

---

## File Structure

- `src/config.rs` — remove `column_count` field/default/Default-entry; add `column_overrides: HashMap<String, u8>`.
- `src/app.rs` — add `default_column_count_for(work)` helper; rewrite `column_count()` to resolve override-or-default; sync right-column visibility (and right-view line spacing) on work load.
- `src/input/navigation.rs` — rewrite `toggle_column_layout` to flip the work's effective count and store a per-work override.

---

## Task 1: Swap the config field to a per-work override map

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Remove the global `column_count` field**

In `src/config.rs`, DELETE the struct field (lines ~42-43):
```rust
    #[serde(default = "default_column_count")]
    pub column_count: u8,
```
DELETE the default fn (~line 110):
```rust
fn default_column_count() -> u8 {
    1
}
```
DELETE the `Default` impl entry (~line 150):
```rust
            column_count: default_column_count(),
```

- [ ] **Step 2: Add the per-work override map**

In the `Config` struct, after the `work_positions` field (line ~55), add (mirror `work_positions`'s `#[serde(default)]` style):
```rust
    #[serde(default)]
    pub column_overrides: HashMap<String, u8>,
```
In the `Default` impl, after `work_positions: HashMap::new(),` (~line 156), add:
```rust
            column_overrides: HashMap::new(),
```
`HashMap` is already imported (used by `work_positions`). Do NOT add `column_overrides` to the `load()` force-reset block (it must persist).

- [ ] **Step 3: Build — expect errors at the two old call sites**

Run: `cargo build`
Expected: FAIL with errors at `src/app.rs` (`column_count()` reads `config.column_count`) and `src/input/navigation.rs` (`toggle_column_layout` reads/writes `config.column_count`). These are fixed in Tasks 2 and 3. (This confirms the field had exactly the two known consumers.)

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "config: replace global column_count with per-work column_overrides map"
```

(The build is intentionally red between Task 1 and Tasks 2-3; the commit captures the config change atomically. If you prefer green commits, do Tasks 1-3 then commit once — but committing per task is fine here since Tasks 2/3 immediately follow.)

---

## Task 2: Per-work default + resolution in `column_count()`

**Files:**
- Modify: `src/app.rs` (`default_column_count_for` helper + rewrite `column_count()`)

- [ ] **Step 1: Write the failing unit test for the default helper**

Add to a `#[cfg(test)]` mod in `src/app.rs` (if one exists, add the fn there; else create the mod at the end of the file). The helper is pure over `(author, work_type)` so test it directly:

```rust
#[cfg(test)]
mod column_default_tests {
    use super::default_column_count_for_parts;

    #[test]
    fn shakespeare_play_defaults_to_two() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "play"), 2);
    }

    #[test]
    fn shakespeare_poem_defaults_to_one() {
        assert_eq!(default_column_count_for_parts("Shakespeare", "poem"), 1);
        assert_eq!(default_column_count_for_parts("Shakespeare", "sonnet_sequence"), 1);
        assert_eq!(default_column_count_for_parts("Shakespeare", "narrative_poem"), 1);
    }

    #[test]
    fn non_shakespeare_play_defaults_to_one() {
        assert_eq!(default_column_count_for_parts("Marlowe", "play"), 1);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin linux-lit column_default_tests`
Expected: FAIL — `default_column_count_for_parts` not defined.

- [ ] **Step 3: Implement the default helpers**

In `src/app.rs` (module level, near `impl AppState` or other free fns), add a pure inner fn (testable without a `Work`) plus a `Work` wrapper:

```rust
/// Pure default-column rule: a Shakespeare play gets two columns, everything
/// else one. Split out from `default_column_count_for` so it is unit-testable
/// without constructing a `Work`.
pub(crate) fn default_column_count_for_parts(author: &str, work_type: &str) -> u8 {
    if author == "Shakespeare" && work_type == "play" {
        2
    } else {
        1
    }
}

/// Default column count for a work: 2 for a Shakespeare play, else 1.
pub(crate) fn default_column_count_for(work: &crate::db::models::Work) -> u8 {
    default_column_count_for_parts(&work.author, &work.work_type)
}
```

- [ ] **Step 4: Rewrite `column_count()` to resolve override-or-default**

Replace the existing `column_count()` method (`src/app.rs:315-322`) with:

```rust
    /// Number of e-reader columns for the CURRENT work: scroll mode → 1; else a
    /// per-work override (if `Alt+[` set one) wins, otherwise the work-type
    /// default (2 for a Shakespeare play, else 1). Clamped to 1..=2.
    pub fn column_count(&self) -> u8 {
        if !matches!(self.config.navigation_mode, crate::config::NavigationMode::EReader) {
            return 1;
        }
        let Some(work) = self.current_work.as_ref() else {
            return 1;
        };
        let n = self.config.column_overrides
            .get(&work.abbrev)
            .copied()
            .unwrap_or_else(|| default_column_count_for(work));
        n.clamp(1, 2)
    }
```

- [ ] **Step 5: Run the test + build**

Run: `cargo test --bin linux-lit column_default_tests`
Expected: PASS (3 tests).
Run: `cargo build`
Expected: `src/app.rs` now compiles; `src/input/navigation.rs` still fails (Task 3). That's expected.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "app: resolve column_count per-work (Shakespeare plays default to 2)"
```

---

## Task 3: Rewrite `toggle_column_layout` to store a per-work override

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Rewrite the toggle**

Replace the body of `toggle_column_layout` (`src/input/navigation.rs:249-...`) so it flips the work's CURRENTLY-EFFECTIVE count and stores the result as a per-work override. Keep the page-recompute / overlay / invalidate logic:

```rust
pub fn toggle_column_layout(state: &mut AppState) {
    if !matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader) {
        crate::logging::log("COLUMNS: ignored (not e-reader mode)");
        return;
    }
    let Some(abbrev) = state.current_work.as_ref().map(|w| w.abbrev.clone()) else {
        return;
    };

    // Flip the work's currently-effective count (override or default) and store
    // it as a per-work override.
    let current = state.column_count();
    let new_count: u8 = if current >= 2 { 1 } else { 2 };
    state.config.column_overrides.insert(abbrev, new_count);
    crate::config::save(&state.config);

    let two = new_count == 2;
    state.right_scrolled_overlay.set_visible(two);
    if !two {
        state.right_bottom_clip.set_height_request(0);
    }

    // Page boundaries depend on column_count(); the cached page-tops index is
    // stale after a toggle, so invalidate it before recomputing.
    invalidate_page_tops(state);

    let top = back_up_for_speaker(&state.buffer, state.page_top_line);
    set_page_instant(state, top);
    if !is_line_on_screen(state, state.current_line) {
        let new_top = page_turn_top(&state.buffer, state.current_line);
        set_page_instant(state, new_top);
    }
    after_page_change(state, PageChangeReason::JumpToLine);
    crate::logging::log(&format!("COLUMNS: now {} column(s)", new_count));
}
```

Note: `current = state.column_count()` is read BEFORE inserting the override, so it reflects the override-or-default the user currently sees; flipping it and storing makes `Alt+[` a true toggle of what's on screen. Because `column_count()` requires `&self` and we then mutate `state.config`, read `current` into a local first (as shown) to avoid a borrow conflict.

- [ ] **Step 2: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: compiles clean (the whole crate now builds — Task 1's errors resolved).

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --bin linux-lit`
Expected: the new `column_default_tests` pass; baseline otherwise unchanged (2 pre-existing `block_atom_tests` failures, everything else passes).

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "nav: Alt+[ stores a per-work column override"
```

---

## Task 4: Render the resolved column count on work load

**Files:**
- Modify: `src/app.rs` (`display_work_at_with_prepared`, after `current_work` is set ~line 1850)

- [ ] **Step 1: Sync right-column visibility (and right-view spacing) to the resolved count on load**

In `display_work_at_with_prepared`, the block right after `state.current_work = Some(work);` (line 1850) sets left-view line spacing at lines 1873-1874:
```rust
    state.text_view.set_pixels_above_lines(ls);
    state.text_view.set_pixels_below_lines(ls);
```
Immediately AFTER those two lines, add: mirror the spacing onto the right view (fixes the columns rendering with different spacing — review issue #4 for the two-column case) and sync the right column's visibility to the resolved `column_count()`:

```rust
    // Keep the right column's line spacing in sync with the left (both views
    // share the buffer but have independent pixels_above/below settings).
    state.right_view.set_pixels_above_lines(ls);
    state.right_view.set_pixels_below_lines(ls);
    // Show or hide the right column to match this work's resolved column count
    // (Shakespeare plays default to two columns; a per-work Alt+[ override wins).
    let two_col = state.column_count() == 2;
    state.right_scrolled_overlay.set_visible(two_col);
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
    }
```

`column_count()` is safe to call here: `current_work` is already `Some` (set at 1850), and `navigation_mode` is whatever the user has. The subsequent page setup in this function routes through the column-aware `snap_scroll_to_line`, so both columns position correctly for the resolved count.

- [ ] **Step 2: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean.

- [ ] **Step 3: Run tests**

Run: `cargo test --bin linux-lit`
Expected: no new failures vs baseline (2 pre-existing).

- [ ] **Step 4: Manual verification (user runs the app)**

Note for the user — verify with `cargo run`:
- Opening a Shakespeare play shows TWO columns by default (no key press).
- Opening a Shakespeare poem/sonnet, or a non-Shakespeare work, shows ONE column.
- `Alt+[` on a play flips to one column; switching away and back to that play keeps one column (override persisted); restarting the app preserves the override.
- `Alt+[` on a non-play work flips it to two columns and persists per-work.
- Scroll mode still shows one column regardless.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "app: render resolved column count on work load (plays open two-column)"
```

---

## Task 5: Regression + final verification

**Files:** none (verification only)

- [ ] **Step 1: Full test suite**

Run: `cargo test --bin linux-lit`
Expected: `column_default_tests` (3) pass, all prior two-column tests pass, baseline 2 pre-existing `block_atom_tests` failures only.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --bin linux-lit`
Expected: 0 errors.

- [ ] **Step 3: Confirm no stray global `column_count` references remain**

Run: `rg -n "config\.column_count|default_column_count\b" src/`
Expected: NO matches (the global field and its default fn are fully removed). If any remain, fix them.

- [ ] **Step 4: Manual acceptance (user)** — same checklist as Task 4 Step 4.

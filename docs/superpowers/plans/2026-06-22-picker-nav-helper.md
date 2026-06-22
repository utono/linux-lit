# Shared ListBox picker-nav helper — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the identical "select row at index if it exists" tail of 13 ListBox-picker `move_selection` methods into one `select_row_at` helper, with zero behavior change.

**Architecture:** New `src/ui/picker_nav.rs` holds `select_row_at(&ListBox, i32)`. Each of the 13 ListBox-index pickers keeps its own (variant-specific) index computation and delegates only the trailing select-if-exists to the helper. Pure GTK, one task.

**Tech Stack:** Rust, GTK4 (`gtk4::ListBox`).

**Spec:** `docs/superpowers/specs/2026-06-22-picker-nav-helper-design.md`

## Global Constraints

- **No behavior change.** Each site's target-index expression is preserved verbatim; ONLY the 3-line `if let Some(row) = list_box.row_at_index(target) { list_box.select_row(Some(&row)); }` tail becomes `select_row_at(&self.list_box, target)`. No clamp added/removed; no empty-start rule changed.
- **Do NOT touch** `action_popup.rs`, `keybinds_overlay.rs`, `settings_overlay.rs`, `library_picker.rs` (different functions — wraparound-over-Vec / scroll-into-view).
- **No keybind change** → do NOT touch `keybinds_overlay.rs`, `keymap_config.rs`, `keymap.json`.
- New module registered as `pub mod picker_nav;` in `src/ui/mod.rs` (match the existing `pub mod <name>;` style).
- `cargo build` + `cargo clippy` clean; `cargo test --bins` green.
- Bash/CLI rules (CLAUDE.md): use `rg`/`fd`, not `grep`/`find`; bypass `mv`/`cp`/`rm` aliases with `\mv -f`/`\cp -f`/`command rm -f`.

---

### Task 1: Add `select_row_at`; delegate the tail in all 13 pickers

**Files:**
- Create: `src/ui/picker_nav.rs`
- Modify: `src/ui/mod.rs` (register `pub mod picker_nav;`)
- Modify (each `move_selection` tail): `src/ui/gloss_picker.rs`, `bookmark_picker.rs`, `concordance_picker.rs`, `media_picker.rs`, `journal_picker.rs`, `echo_picker.rs`, `echo_turns_picker.rs`, `echo_line_picker.rs`, `concordance_word_picker.rs`, `concordance_list_picker.rs`, `concordance_works_picker.rs`, `voice_picker.rs`, `authorship_picker.rs`

**Interfaces:**
- Produces: `pub(crate) fn select_row_at(list_box: &gtk4::ListBox, index: i32)` in `crate::ui::picker_nav`.

- [ ] **Step 1: Create `src/ui/picker_nav.rs`**

```rust
use gtk4::prelude::*;
use gtk4::ListBox;

/// Select the row at `index` in `list_box` if it exists; no-op otherwise.
/// The shared tail of every ListBox picker's `move_selection`: callers compute
/// their own target index (preserving each picker's empty-start and clamp rules)
/// and pass it here. `index < 0` or past the end selects nothing (GTK's
/// `row_at_index` returns None) — the existing behavior at every call site.
pub(crate) fn select_row_at(list_box: &ListBox, index: i32) {
    if let Some(row) = list_box.row_at_index(index) {
        list_box.select_row(Some(&row));
    }
}
```

- [ ] **Step 2: Register the module in `src/ui/mod.rs`**

Add `pub mod picker_nav;` alongside the other `pub mod <name>;` lines (match grouping/ordering, e.g. near `footer`/`ask_card`).

- [ ] **Step 3: Variant A (guard + clamp) — 5 sites**

For `gloss_picker.rs`, `bookmark_picker.rs`, `concordance_picker.rs`, `media_picker.rs`, `journal_picker.rs`: each `move_selection` currently reads (modulo whitespace)

```rust
pub fn move_selection(&self, delta: i32) {
    if let Some(current) = self.list_box.selected_row() {
        let idx = current.index();
        let new_idx = (idx + delta).max(0);
        if let Some(row) = self.list_box.row_at_index(new_idx) {
            self.list_box.select_row(Some(&row));
        }
    }
}
```

Replace ONLY the inner `if let Some(row) = ... { ... }` tail with the helper call, keeping the `let idx` / `let new_idx` bindings:

```rust
pub fn move_selection(&self, delta: i32) {
    if let Some(current) = self.list_box.selected_row() {
        let idx = current.index();
        let new_idx = (idx + delta).max(0);
        crate::ui::picker_nav::select_row_at(&self.list_box, new_idx);
    }
}
```

- [ ] **Step 4: Variant B (`unwrap_or(-1)` + clamp) — 2 sites**

For `echo_picker.rs`, `echo_turns_picker.rs`:

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    let next = (current + delta).max(0);
    crate::ui::picker_nav::select_row_at(&self.list_box, next);
}
```
(Keep the original's `let next = ...` binding; replace only the tail.)

- [ ] **Step 5: Variant C (`unwrap_or(-1)` + no clamp) — 5 sites**

For `echo_line_picker.rs`, `concordance_word_picker.rs`, `concordance_list_picker.rs`, `concordance_works_picker.rs`, `voice_picker.rs`:

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
    let next = current + delta;
    crate::ui::picker_nav::select_row_at(&self.list_box, next);
}
```
`voice_picker` writes the `.selected_row()...unwrap_or(-1)` across multiple lines — preserve its exact multi-line form; replace only the trailing `if let Some(row) = ... { ... }`.

- [ ] **Step 6: Variant D (`unwrap_or(0)` + clamp) — 1 site**

For `authorship_picker.rs`:

```rust
pub fn move_selection(&self, delta: i32) {
    let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(0);
    let next = (current + delta).max(0);
    crate::ui::picker_nav::select_row_at(&self.list_box, next);
}
```

- [ ] **Step 7: Re-read each site BEFORE editing and verify the index value is unchanged**

For each of the 13 files, read the current `move_selection`, confirm which variant (A/B/C/D) it is, and confirm the value passed to `select_row_at` equals the index the original `row_at_index(...)` received. If any site's index expression differs from its variant template above, the LIVE CODE WINS — keep that site's exact index computation and only delegate the tail. Do NOT normalize a site's clamp/empty-handling to match a sibling.

- [ ] **Step 8: Build**

Run: `cargo build`
Expected: `Finished`, no errors. The helper is used 13× so no dead_code. Resolve any now-unused-import warning ONLY if real (e.g. nothing else in a file used an import that the removed tail referenced — unlikely, since `list_box`/`select_row` come from `gtk4::prelude`).

- [ ] **Step 9: Clippy**

Run: `cargo clippy`
Expected: no new warnings.

- [ ] **Step 10: Tests**

Run: `cargo test --bins`
Expected: same pass count as before (413 at last check), 0 failed.

- [ ] **Step 11: Confirm scope**

Run: `git diff --stat`. Confirm the changed files are exactly: `src/ui/picker_nav.rs`, `src/ui/mod.rs`, and the 13 picker files listed above — and NOTHING else. Run:
`git diff src/ui/action_popup.rs src/ui/keybinds_overlay.rs src/ui/settings_overlay.rs src/ui/library_picker.rs`
and confirm it is EMPTY (excluded pickers untouched).

- [ ] **Step 12: Commit**

```bash
git add src/ui/picker_nav.rs src/ui/mod.rs \
  src/ui/gloss_picker.rs src/ui/bookmark_picker.rs src/ui/concordance_picker.rs \
  src/ui/media_picker.rs src/ui/journal_picker.rs src/ui/echo_picker.rs \
  src/ui/echo_turns_picker.rs src/ui/echo_line_picker.rs src/ui/concordance_word_picker.rs \
  src/ui/concordance_list_picker.rs src/ui/concordance_works_picker.rs \
  src/ui/voice_picker.rs src/ui/authorship_picker.rs
git commit -m "refactor(ui): extract select_row_at picker-nav helper

New src/ui/picker_nav.rs select_row_at(&ListBox, i32) replaces the identical
select-if-exists tail in 13 ListBox-index pickers' move_selection. Each keeps
its own index computation (clamp/empty-start vary by variant) so behavior is
preserved exactly. Not a full Picker trait; action_popup/keybinds/settings/
library_picker intentionally untouched.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after the task)

- `cargo build` + `cargo clippy` clean, `cargo test --bins` green.
- Reviewer confirms, per site: the variant is correctly identified, the index value passed to `select_row_at` equals the original `row_at_index` argument (no clamp/empty-start change), only the tail was delegated, and exactly the 15 intended files changed (helper + mod + 13 pickers); the 4 excluded pickers are untouched.
- **User cage pass:** navigate several pickers (concordance via Ctrl+\\, media, gloss, an echo picker) with j/k — selection moves as before, including at the top edge (clamp) and on first open (empty-start).

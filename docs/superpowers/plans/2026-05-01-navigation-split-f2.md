# Navigation Module Split (F2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the 3934-line `navigation.rs` into four focused modules: `viewport.rs` (page boundary math), `cursor.rs` (navigation verbs), `scroll.rs` (GTK scroll plumbing + page-turn animation), `highlight.rs` (dim/cursor-line tag rendering). Tests move with their code.

**Architecture:** Pure code motion — no behavior changes. Each module gets a clear responsibility boundary. `navigation.rs` becomes a thin re-export facade so external callers (`keymap.rs`, `app.rs`, `mpv/`) don't need path changes. Internal cross-module calls use explicit imports.

**Tech Stack:** Rust, GTK4, sourceview5

**Prerequisites:** F3 (clean dispatch_action) and F4 (gloss extraction) should be done first so `keymap.rs` is stable. F7 (dead code removal) should be done first so the dead `should_update_label` doesn't migrate.

---

### Task 1: Create `viewport.rs` — pure page-boundary math

**Files:**
- Create: `src/input/viewport.rs`
- Modify: `src/input/mod.rs` (add module declaration)
- Modify: `src/input/navigation.rs` (re-export, remove moved items)

This module contains the pure functions that answer "what fits on screen." No `&mut AppState`, no GTK widget mutation — only `&AppState` reads and geometry math.

- [ ] **Step 1: Create `src/input/viewport.rs`**

Move these items from `navigation.rs`:

**Types:**
- `VisibleRange` struct (lines 1256-1261)
- `NextPage` struct (lines 200-208)

**Pure functions (no GTK mutation):**
- `visible_range()` (lines 1290-1311)
- `page_for_line_in_index()` (lines 1277-1285)
- `trim_trailing_speakers_pure()` (lines 1320-1336)
- `trim_block_atoms_pure()` (lines 1434-1503)
- `block_start_for_line_pure()` (lines 1380-1426)

**GTK-bound wrappers (read-only, no mutation):**
- `trim_trailing_speakers()` (lines 1343-1367)
- `trim_block_atoms()` (lines 1536-1561)
- `block_start_for_line()` (lines 1510-1529)
- `trim_visible_range()` (lines 1632-1646)
- `clamp_at_section_break()` (lines 1563-1613)
- `last_fully_visible_line()` (lines 178-194)
- `next_page_top()` (lines 213-231)
- `prev_page_top()` (lines 248-292)
- `build_page_tops()` (lines 298-314)
- `viewport_page_for_line()` (lines 323-339)
- `invalidate_page_tops()` (lines 343-345)
- `is_line_fully_visible()` (lines 964-995)
- `is_line_on_screen()` (lines 956-958)
- `lines_per_page()` (lines 2435-2463)
- `descender_guard_px()` (lines 1973-1989)

**Helper functions used by the above:**
- `next_dialogue_from()` (lines 120-129)
- `last_dialogue_in_page()` (lines 132-143)
- `back_up_for_speaker()` (lines 150-170)
- `page_turn_top()` (lines 667-669)
- `chapter_page_top()` (lines 674-687)
- `buffer_line_text()` (lines 597-606)
- `is_dialogue_line()` (lines 609-618)
- `is_blank_buffer_line()` (lines 591-594)
- `next_dialogue_line()` (lines 621-637)
- `prev_dialogue_line()` (lines 640-661)

Add `pub(crate)` visibility to items that navigation.rs and other modules need. Keep `pub` on items that `app.rs` or `mpv/` call directly.

- [ ] **Step 2: Add module declaration**

In `src/input/mod.rs`, add:

```rust
pub mod viewport;
```

- [ ] **Step 3: Add re-exports to navigation.rs**

In `navigation.rs`, replace the moved items with re-exports so external callers don't break:

```rust
pub use crate::input::viewport::{
    VisibleRange, visible_range, page_for_line_in_index,
    trim_visible_range, last_fully_visible_line,
    viewport_page_for_line, invalidate_page_tops,
    is_line_on_screen, is_line_fully_visible,
    lines_per_page, descender_guard_px,
    next_page_top, prev_page_top,
    back_up_for_speaker, page_turn_top, chapter_page_top,
    next_dialogue_from, next_dialogue_line, prev_dialogue_line,
    buffer_line_text, is_dialogue_line,
};
```

- [ ] **Step 4: Move tests**

Move these test modules from `navigation.rs` to `viewport.rs`:
- `visible_range_helpers_tests`
- `block_atom_tests`
- `page_tops_tests`
- `prev_page_top_tests`

- [ ] **Step 5: Build and test**

Run: `cargo build && cargo test`
Expected: All tests pass. No behavior change.

- [ ] **Step 6: Commit**

```bash
git add src/input/viewport.rs src/input/mod.rs src/input/navigation.rs
git commit -m "Extract viewport.rs from navigation.rs: page-boundary math and visibility functions"
```

---

### Task 2: Create `scroll.rs` — GTK scroll plumbing and page-turn animation

**Files:**
- Create: `src/input/scroll.rs`
- Modify: `src/input/mod.rs`
- Modify: `src/input/navigation.rs`

This module owns everything that mutates the GTK scroll position or runs page-turn animations.

- [ ] **Step 1: Create `src/input/scroll.rs`**

Move these items from `navigation.rs`:

- `PageDirection` enum (lines 1112-1116)
- `PageTurnLock` struct + impl (lines 1132-1160)
- `set_page()` (lines 1692-1849)
- `set_page_instant()` (lines 1898-1902)
- `snap_scroll_to_line()` (lines 1906-1954)
- `resnap_page()` (lines 1854-1856)
- `refresh_bottom_clip()` (lines 1863-1871)
- `schedule_bottom_clip_update()` (lines 1879-1895)
- `update_bottom_clip()` (lines 1994-2074)
- `capture_page_snapshot()` (lines 1651-1689)
- `clear_old_page_dim()` (lines 1092-1108)
- `scroll_to_cursor()` / `center_cursor()` (lines 998-1052)
- `scroll_after_jump_forward()` (lines 1007-1018)
- `scroll_after_jump_backward()` (lines 1022-1034)
- `scroll_value_for_line()` (lines 2101-2110)
- `scroll_viewport()` (lines 2079-2096)
- `scroll_paragraph_to_top()` (lines 2126-2161)

- [ ] **Step 2: Add module declaration and re-exports**

In `src/input/mod.rs`:

```rust
pub mod scroll;
```

Add re-exports to `navigation.rs` for external callers.

- [ ] **Step 3: Move PageTurnLock tests**

Move `page_turn_lock_tests` module to `scroll.rs`.

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 5: Commit**

```bash
git add src/input/scroll.rs src/input/mod.rs src/input/navigation.rs
git commit -m "Extract scroll.rs from navigation.rs: GTK scroll plumbing and page-turn animation"
```

---

### Task 3: Create `highlight.rs` — dim/cursor-line tag management

**Files:**
- Create: `src/input/highlight.rs`
- Modify: `src/input/mod.rs`
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Create `src/input/highlight.rs`**

Move these functions from `navigation.rs`:

- `update_highlight()` (lines 2297-2425)
- `update_highlight_only()` (lines 2119-2122)
- `update_highlight_and_ensure_visible()` (lines 2164-2185)
- `update_highlight_and_advance_page()` (lines 2191-2208)
- `update_highlight_and_show()` (lines 2213-2257)
- `update_highlight_and_center()` (lines 2260-2266)
- `auto_show_vocab_popup()` (lines 2269-2282)

- [ ] **Step 2: Add module declaration and re-exports**

In `src/input/mod.rs`:

```rust
pub mod highlight;
```

Add re-exports to `navigation.rs`.

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src/input/highlight.rs src/input/mod.rs src/input/navigation.rs
git commit -m "Extract highlight.rs from navigation.rs: dim/cursor-line tag management"
```

---

### Task 4: Clean up navigation.rs as the cursor verbs module

**Files:**
- Modify: `src/input/navigation.rs` (verify what remains)

After Tasks 1-3, `navigation.rs` should contain only:

**Cursor movement verbs** (~500 lines):
- `jump_to_start`, `jump_to_end`
- `page_forward`, `page_backward`, `page_backward_bottom`
- `cursor_next_dialogue`, `cursor_prev_line`, `cursor_to_page_bottom`
- `jump_to_next_dialogue`, `jump_to_prev_dialogue`
- `jump_to_next_chapter`, `jump_to_prev_chapter`
- `jump_to_next_scene`, `jump_to_prev_scene`
- `jump_to_next_section`, `jump_to_prev_section`
- `jump_to_next_paragraph`, `jump_to_prev_paragraph`
- `next_bookmark`, `prev_bookmark`, `jump_to_line`
- `scroll_cursor_top`
- `seek_to_current_line`
- `position_chunk`

**After-page-change dispatch** (~50 lines):
- `PageChangeReason` enum
- `after_page_change()`

**Concordance navigation** (~100 lines):
- `concordance_jump_to_current`, `concordance_position_cursor`, etc.

**Word copy** (~100 lines):
- `word_cycle_copy`, `word_collect_copy`, `extract_buffer_line_words`, `apply_word_underline`

**Re-exports** (~20 lines)

**Tests** (~200 lines):
- `page_turn_tests` (these use text-only simulation, no GTK)
- `after_page_change_tests`

Total: ~1000 lines, down from 3934.

- [ ] **Step 1: Verify remaining content**

Run: `wc -l src/input/navigation.rs`
Expected: ~900-1100 lines.

- [ ] **Step 2: Remove stale re-exports**

Remove any re-exports that are no longer needed (items that only `navigation.rs` itself was calling internally).

- [ ] **Step 3: Build and run full test suite**

Run: `cargo build && cargo test && cargo clippy`

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Clean up navigation.rs: ~1000 lines of cursor verbs, down from 3934"
```

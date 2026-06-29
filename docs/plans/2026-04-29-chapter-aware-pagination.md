# Chapter-Aware Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Force chapter/scene boundaries to start on a new page during e-reader pagination, matching foliate-js and bk behavior.

**Architecture:** Add a `chapter_breaks` index to `LineMap` that records buffer line indices where `is_chapter == true`. Make prose `is_dialogue` return false for separators and act/scene markers so they're treated as structural (not content). Then teach `trim_visible_range` to clamp pages at chapter boundaries, and update `next_page_top` accordingly. Bump snapshot version to invalidate stale caches.

**Tech Stack:** Rust, GTK4/sourceview5, bincode (snapshot serialization)

---

## File Structure

- **Modify:** `src/db/line_types.rs` — prose `is_dialogue` returns `false` for structural lines
- **Modify:** `src/text_file_map.rs` — add `chapter_breaks: Vec<usize>` to `LineMap`, populate in `build_line_map`
- **Modify:** `src/input/navigation.rs` — add chapter-break clamping to `trim_visible_range`, update `next_page_top`
- **Modify:** `src/snapshot.rs` — bump `SNAPSHOT_VERSION` to 2

---

### Task 1: Make prose `is_dialogue` return false for structural lines

**Files:**
- Modify: `src/db/line_types.rs:58-69`

- [ ] **Step 1: Add test for prose separator classification**

```rust
// In src/db/line_types.rs, add to existing #[cfg(test)] mod tests:

#[test]
fn test_prose_separator_is_not_dialogue() {
    assert!(!is_dialogue("= Chapter One", true));
    assert!(!is_dialogue("========", true));
}

#[test]
fn test_prose_act_scene_marker_is_not_dialogue() {
    assert!(!is_dialogue("ACT 1", true));
    assert!(!is_dialogue("## Act 3, Scene 2", true));
    assert!(!is_dialogue("PROLOGUE", true));
}

#[test]
fn test_prose_normal_line_still_dialogue() {
    assert!(is_dialogue("It was the best of times.", true));
    assert!(is_dialogue("Mr. Jarndyce looked at us.", true));
}

#[test]
fn test_prose_blank_still_not_dialogue() {
    assert!(!is_dialogue("", true));
    assert!(!is_dialogue("   ", true));
}
```

- [ ] **Step 2: Run tests to verify new tests fail**

Run: `cargo test --bin linux-lit -- line_types::tests::test_prose_separator`
Expected: FAIL — `is_dialogue("= Chapter One", true)` returns `true`

- [ ] **Step 3: Update `is_dialogue` to exclude structural lines in prose mode**

Replace `src/db/line_types.rs:58-69`:

```rust
pub fn is_dialogue(text: &str, is_prose: bool) -> bool {
    if is_blank(text) {
        return false;
    }
    if is_separator(text) || is_act_scene_marker(text) {
        return false;
    }
    if is_prose {
        return true;
    }
    !is_speaker(text) && !is_stage_direction(text)
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test --bin linux-lit -- line_types`
Expected: All PASS

- [ ] **Step 5: Run full build**

Run: `cargo build`
Expected: Compiles with no new errors

- [ ] **Step 6: Commit**

```bash
git add src/db/line_types.rs
git commit -m "Classify separators and act/scene markers as non-dialogue in prose mode"
```

---

### Task 2: Add `chapter_breaks` field to `LineMap` and populate it

**Files:**
- Modify: `src/text_file_map.rs:22-31` (struct) and `src/text_file_map.rs:115+` (build_line_map)

- [ ] **Step 1: Add `chapter_breaks` field to `LineMap`**

In `src/text_file_map.rs`, add to the `LineMap` struct after `sentence_groups`:

```rust
pub struct LineMap {
    pub buffer_to_work: Vec<Option<usize>>,
    pub work_to_buffer: Vec<usize>,
    pub dialogue_buffer_lines: Vec<usize>,
    pub sentence_groups: Vec<SentenceGroup>,
    /// Buffer line indices where a new chapter starts (`Line.is_chapter == true`).
    /// Sorted ascending. Used by pagination to force page breaks at chapter boundaries.
    pub chapter_breaks: Vec<usize>,
}
```

- [ ] **Step 2: Run build to see what breaks**

Run: `cargo build`
Expected: FAIL — struct literal in `build_line_map` is missing `chapter_breaks`

- [ ] **Step 3: Populate `chapter_breaks` in `build_line_map`**

At the end of `build_line_map` in `src/text_file_map.rs`, before the `LineMap { ... }` return, add:

```rust
    let mut chapter_breaks = Vec::new();
    for (work_idx, line) in work_lines.iter().enumerate() {
        if line.is_chapter && work_idx < work_to_buffer.len() {
            chapter_breaks.push(work_to_buffer[work_idx]);
        }
    }
```

And add `chapter_breaks` to the returned struct literal:

```rust
    LineMap {
        buffer_to_work,
        work_to_buffer,
        dialogue_buffer_lines,
        sentence_groups,
        chapter_breaks,
    }
```

- [ ] **Step 4: Fix synthetic_line_map in snapshot tests**

In `src/snapshot.rs`, the `synthetic_line_map()` helper builds a `LineMap` manually. Add the field:

```rust
fn synthetic_line_map() -> LineMap {
    LineMap {
        buffer_to_work: vec![Some(0), None, Some(1), Some(2)],
        work_to_buffer: vec![0, 2, 3],
        dialogue_buffer_lines: vec![0, 2, 3],
        sentence_groups: vec![],
        chapter_breaks: vec![],
    }
}
```

- [ ] **Step 5: Run full build + tests**

Run: `cargo build && cargo test`
Expected: All PASS — `chapter_breaks` is empty for existing works without `is_chapter` flags

- [ ] **Step 6: Commit**

```bash
git add src/text_file_map.rs src/snapshot.rs
git commit -m "Add chapter_breaks index to LineMap; populate from Work.lines.is_chapter"
```

---

### Task 3: Bump snapshot version to invalidate stale caches

**Files:**
- Modify: `src/snapshot.rs:8`

- [ ] **Step 1: Bump SNAPSHOT_VERSION**

In `src/snapshot.rs:8`:

```rust
pub const SNAPSHOT_VERSION: u32 = 2;
```

- [ ] **Step 2: Run tests**

Run: `cargo test snapshot`
Expected: All PASS — the version_skew test already validates that mismatched versions are rejected

- [ ] **Step 3: Commit**

```bash
git add src/snapshot.rs
git commit -m "Bump snapshot version to 2 for chapter_breaks field"
```

---

### Task 4: Add chapter-break clamping to `trim_visible_range`

**Files:**
- Modify: `src/input/navigation.rs:1588-1598`

This is the core pagination change. When a chapter break falls within the visible range, clamp `last_fit` to the line before the break so the chapter starts on the next page.

- [ ] **Step 1: Add a `chapter_breaks` parameter to `trim_visible_range`**

Change the signature at `src/input/navigation.rs:1588`:

```rust
pub(crate) fn trim_visible_range(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
    chapter_breaks: &[usize],
) -> VisibleRange {
```

- [ ] **Step 2: Add chapter-break clamping as a pre-pass**

Insert before the existing three passes:

```rust
pub(crate) fn trim_visible_range(
    range: VisibleRange,
    page_top: usize,
    text_view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
    is_prose: bool,
    chapter_breaks: &[usize],
) -> VisibleRange {
    // Pre-pass: if a chapter break falls strictly inside the visible range
    // (not at page_top itself — that's the current chapter starting), clamp
    // last_fit to the line before the break so the chapter starts the next page.
    let r = if !chapter_breaks.is_empty() && range.count > 1 {
        let first_after_top = chapter_breaks
            .partition_point(|&b| b <= page_top);
        if let Some(&break_line) = chapter_breaks.get(first_after_top) {
            if break_line <= range.last_fit {
                // Clamp: the page ends at break_line - 1.
                let clamped_last = break_line.saturating_sub(1);
                if clamped_last >= page_top {
                    // Recompute total_height for the clamped range.
                    let buf_sv: sourceview5::Buffer = text_view.buffer().downcast().unwrap();
                    let new_range = super::visible_range(
                        text_view, &buf_sv, page_top,
                        range.last_fit + range.count, // line_count upper bound
                        i32::MAX, // usable_height — don't constrain
                    );
                    // Re-clamp to clamped_last.
                    let mut r = range;
                    r.last_fit = clamped_last;
                    r.count = clamped_last - page_top + 1;
                    // Walk to get accurate total_height up to clamped_last.
                    let mut total = 0;
                    for i in page_top..=clamped_last {
                        if let Some(iter) = buf_sv.iter_at_line(i as i32) {
                            let (_y, h) = text_view.line_yrange(&iter);
                            total += h;
                        }
                    }
                    r.total_height = total;
                    r
                } else {
                    range
                }
            } else {
                range
            }
        } else {
            range
        }
    } else {
        range
    };

    let r = trim_trailing_speakers(r, page_top, text_view, buffer);
    let r = trim_block_atoms(r, page_top, text_view, buffer, is_prose);
    trim_trailing_speakers(r, page_top, text_view, buffer)
}
```

- [ ] **Step 3: Fix all callers of `trim_visible_range`**

Search for all call sites:

```bash
grep -n "trim_visible_range(" src/input/navigation.rs
```

There are two callers:

1. `last_fully_visible_line` (~line 198) — needs `chapter_breaks` from `state`:

```rust
fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    // ...existing code to compute range...
    let is_prose = state.current_work.as_ref()
        .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
        .unwrap_or(false);
    let chapter_breaks = state.line_map.as_ref()
        .map(|lm| lm.chapter_breaks.as_slice())
        .unwrap_or(&[]);
    let trimmed = trim_visible_range(range, top, &state.text_view, &state.buffer, is_prose, chapter_breaks);
    trimmed.last_fit
}
```

2. `update_bottom_clip` (~line 2003) — also needs chapter_breaks. This function takes widget references, not `AppState`. Pass an empty slice since bottom_clip doesn't need chapter-break trimming (it only sizes the clip overlay):

Find the `trim_trailing_speakers` call in `update_bottom_clip` (not `trim_visible_range` — check if this function calls `trim_visible_range` or only `trim_trailing_speakers`).

```bash
grep -n "trim_trailing_speakers\|trim_visible_range\|trim_block_atoms" src/input/navigation.rs | grep -v "^.*fn \|^.*///\|^.*pub\|^.*test"
```

Update each caller to pass `chapter_breaks`. For callers without access to `state.line_map`, pass `&[]`.

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: Compiles — may need to fix additional call sites

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Clamp page boundaries at chapter breaks in trim_visible_range"
```

---

### Task 5: Manual verification with a prose work

**Files:** none (testing only)

- [ ] **Step 1: Clear snapshot cache**

```bash
cargo run -- --clear-cache
```

- [ ] **Step 2: Launch with Bleak House (BH)**

```bash
cargo run
```

- [ ] **Step 3: Navigate to a chapter boundary**

Press `]` to jump to the next chapter boundary. Verify the chapter heading is at or near the top of the page.

- [ ] **Step 4: Press `y` to go back one page**

Verify the previous page does NOT show the chapter heading at its bottom — it should end before the chapter break.

- [ ] **Step 5: Page forward through several chapters**

Press `x` repeatedly. Each time a chapter boundary is reached, the chapter heading should appear at the top of a new page, not mid-page.

- [ ] **Step 6: Verify play works (Troilus and Cressida or similar)**

Switch to a play work via Ctrl+p. Navigate with `x`/`y`. Act/scene markers should also trigger page breaks — the new act should start at the top of a page.

- [ ] **Step 7: Commit (if any fixes needed)**

```bash
git add -A
git commit -m "Fix chapter-aware pagination issues found during testing"
```

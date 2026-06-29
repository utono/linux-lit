# Empty Right-Column "Next: Act N, Scene M" Watermark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a two-column spread's right column is empty because the current scene ended in the left column, render a dim, vertically-centered `Next: Act N, Scene M` label in the right column showing the act/scene that opens the next canonical spread.

**Architecture:** Add a `gtk4::Label` overlay child to the existing `right_scrolled_overlay` (never buffer text — buffer text would be measured by pagination and corrupt the right-column clip). On every page render, `snap_scroll_to_line` already computes the `ColumnSplit` (`cs`); a new helper reads `cs` to decide whether the right column is empty and, if so, derives the next scene's `(div1, div2)` from `cs.next_page_top` (authoritative DB metadata, never text inference) and shows the label. Otherwise it hides the label.

**Tech Stack:** Rust, GTK4 (`gtk4`, `sourceview5`), existing helpers `scene_label`, `work_line_for_buffer`, theme `dim_fg`.

---

## Background the engineer needs

- **Project rule (authoritative metadata):** Scene/act boundaries come from the DB `(div1, div2)` columns, NEVER from classifying buffer text. `column_split` already returns `cs.next_page_top`, the first buffer line of the next spread; in the empty-right case that line is the scene marker `hi`. To get its act/scene, walk forward from `cs.next_page_top` to the first DB-*mapped* buffer line and read that line's `div1`/`div2`. The marker / `=====` chrome lines are unmapped, which is why the walk is needed. This mirrors the existing `current_scene_divs` (`src/app.rs:4477`).
- **Overlay, not chain link:** new UI panels are added with `overlay.add_overlay(&widget)`, never inserted into the size-bearing widget chain. The existing `right_bottom_clip` (`src/app.rs:1056-1061`) is the model: it is `add_overlay`'d onto `right_scrolled_overlay`. Add the watermark the same way.
- **The empty-right condition** is, from the returned `ColumnSplit`:
  `cs.page_end < cs.split && cs.split < line_count && cs.next_page_top < line_count`.
  - `cs.page_end < cs.split` → right column range is empty (the scene-break return at `src/input/viewport.rs:1204-1205` sets `page_end = hi-1 < split`).
  - `cs.split < line_count` → excludes the end-of-work case (`src/input/viewport.rs:1119-1120` returns `split == line_count`, `next_page_top == line_count`).
  - `cs.next_page_top < line_count` → a next scene actually exists.
  - The empty-LEFT mirror (`split == 0`, right column non-empty, `src/input/viewport.rs:1183-1197`) already fails `page_end < split`, so it is excluded with no extra check.
- **`ColumnSplit` is `Copy`** (used as `cs.map(|c| c.split)` then `if let Some(cs) = cs`). Fields: `split: usize`, `page_end: usize`, `next_page_top: usize` (`src/input/viewport.rs:1037-1042`).
- **`scene_label(div1, div2) -> String`** (`src/app.rs:4638`) → `"Act 1, Scene 4"`, `"Prologue"`, `"Act 2, Chorus"`.
- **`state.theme.dim_fg`** is a `String` color like `#7c6f64` (`src/theme.rs:15,134`), suitable for a Pango markup `<span foreground="…">`.

## File Structure

- **`src/app.rs`**
  - `AppState` gains one field: `pub next_scene_watermark: gtk4::Label` (declared next to `right_bottom_clip` at `:101`).
  - Build the label and `add_overlay` it onto `right_scrolled_overlay` in `build_ui` (right after the `right_bottom_clip` block, `:1056-1061`).
  - Assign it in the `AppState { … }` constructor (next to `right_bottom_clip`, `:1384`).
  - Hide it in the column-count toggle when leaving two-column mode (`:759-761`).
  - New pub helper `divs_at_buffer_line(state, buffer_line) -> (i64, i64)` (placed right after `current_scene_divs`, `:4505`).
- **`src/input/scroll.rs`**
  - New helper `update_next_scene_watermark(state, cs)`.
  - Call it from the `if let Some(cs) = cs { … }` block in `snap_scroll_to_line` (`:446`), and call the hide path on the `else`/`None` branch.

---

## Task 1: Add `divs_at_buffer_line` helper (pure logic, unit-testable)

**Files:**
- Modify: `src/app.rs` (add fn after `current_scene_divs`, currently ending at `:4505`)
- Test: `src/app.rs` (a `#[cfg(test)]` test is not feasible — see Step 2; verification is by `cargo build` + reuse in Task 4)

This helper generalizes the forward/backward walk in `current_scene_divs` to an arbitrary buffer line. It is the authoritative `(div1,div2)` lookup for the watermark.

- [ ] **Step 1: Add the helper**

Insert immediately after the closing `}` of `current_scene_divs` (after `src/app.rs:4505`):

```rust
/// Return the `(div1, div2)` (act, scene) for an arbitrary buffer line by
/// reading the DB-backed `Line` metadata — never inferred from buffer text.
/// Walks forward from `buffer_line` to the first DB-mapped line (the marker /
/// `=====` chrome lines are unmapped), then backward as a fallback. Returns
/// `(0, 0)` when nothing is mapped (treated as "Prologue" by `scene_label`).
pub fn divs_at_buffer_line(state: &AppState, buffer_line: usize) -> (i64, i64) {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return (0, 0),
    };
    let line_count = state.effective_line_count();
    for bl in buffer_line..line_count {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    for bl in (0..buffer_line).rev() {
        if let Some(work_idx) = state.work_line_for_buffer(bl) {
            if let Some(line) = work.lines.get(work_idx) {
                return (line.div1, line.div2);
            }
        }
    }
    (0, 0)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean. (No pure unit test: `divs_at_buffer_line` needs a populated `AppState` with a `line_map` + GTK widgets, which the `--bins` suite does not construct; it is exercised end-to-end in Task 4's build and live verification. A `#[allow(dead_code)]` is unnecessary because Task 4 calls it.)

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): divs_at_buffer_line — authoritative act/scene for any buffer line

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Add the `next_scene_watermark` label to `AppState` and build it

**Files:**
- Modify: `src/app.rs:101` (struct field)
- Modify: `src/app.rs:1056-1061` (build + add_overlay)
- Modify: `src/app.rs:1384` (constructor field)

- [ ] **Step 1: Declare the field**

In the `AppState` struct, immediately after the `right_bottom_clip` field (`src/app.rs:101`):

```rust
    pub right_bottom_clip: gtk4::Box,
    /// Dim "Next: Act N, Scene M" label shown centered in an empty right
    /// column (scene ended in the left column). Overlay child of
    /// `right_scrolled_overlay`; hidden in every other case.
    pub next_scene_watermark: gtk4::Label,
```

- [ ] **Step 2: Build the label and add it as an overlay**

In `build_ui`, immediately after the `right_scrolled_overlay.add_overlay(&right_bottom_clip);` line (`src/app.rs:1061`):

```rust
    right_scrolled_overlay.add_overlay(&right_bottom_clip);

    // Dim "Next: Act N, Scene M" watermark for an empty right column. Overlay
    // child (NOT buffer text — buffer text is measured by pagination and would
    // corrupt the right-column clip). Centered; hidden until snap_scroll_to_line
    // detects an empty right column with a following scene.
    let next_scene_watermark = gtk4::Label::new(None);
    next_scene_watermark.set_halign(gtk4::Align::Center);
    next_scene_watermark.set_valign(gtk4::Align::Center);
    next_scene_watermark.set_visible(false);
    next_scene_watermark.add_css_class("next-scene-watermark");
    right_scrolled_overlay.add_overlay(&next_scene_watermark);
```

- [ ] **Step 3: Assign it in the constructor**

In the `AppState { … }` literal, immediately after the `right_bottom_clip,` line (`src/app.rs:1384`):

```rust
        right_bottom_clip,
        next_scene_watermark,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean (label is constructed, stored, and unused-for-now — GTK widgets don't warn on unused fields because the struct field is `pub`).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add next_scene_watermark label overlay on right column

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Hide the watermark when leaving two-column mode

**Files:**
- Modify: `src/app.rs:759-761` (the `if !two_col` block in the column-count toggle)

Without this, the label could linger from a prior two-column spread after switching to single-column (prose) mode.

- [ ] **Step 1: Hide on single-column**

Change the `if !two_col` block (`src/app.rs:759-761`) from:

```rust
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
    }
```

to:

```rust
    if !two_col {
        state.right_bottom_clip.set_height_request(0);
        state.next_scene_watermark.set_visible(false);
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): hide next-scene watermark when leaving two-column mode

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Update + show/hide the watermark in `snap_scroll_to_line`

**Files:**
- Modify: `src/input/scroll.rs` (new helper + call sites in `snap_scroll_to_line`, `:416-485`)

- [ ] **Step 1: Add the update helper**

Add this free function in `src/input/scroll.rs`, immediately above `pub(crate) fn snap_scroll_to_line` (`:361`):

```rust
/// Show a dim "Next: Act N, Scene M" label centered in an EMPTY right column
/// (the scene ended in the left column), naming the act/scene that opens the
/// next canonical spread; hide it in every other case. `cs` is the spread's
/// `ColumnSplit` (already computed by the caller). The next scene's act/scene
/// is read from `cs.next_page_top`'s DB `(div1, div2)` — authoritative metadata,
/// never inferred from buffer text.
fn update_next_scene_watermark(state: &AppState, cs: &super::viewport::ColumnSplit) {
    let line_count = state.effective_line_count();
    let empty_right = cs.page_end < cs.split
        && cs.split < line_count
        && cs.next_page_top < line_count;
    if !empty_right {
        state.next_scene_watermark.set_visible(false);
        return;
    }
    let (div1, div2) = crate::app::divs_at_buffer_line(state, cs.next_page_top);
    let label = crate::app::scene_label(div1, div2);
    let markup = format!(
        "<span foreground=\"{}\" style=\"italic\" size=\"smaller\">Next: {}</span>",
        state.theme.dim_fg,
        glib::markup_escape_text(&label),
    );
    state.next_scene_watermark.set_markup(&markup);
    state.next_scene_watermark.set_visible(true);
}
```

- [ ] **Step 2: Call it in the empty/non-empty paths**

In `snap_scroll_to_line`, the `cs` value is consumed by `if let Some(cs) = cs {` at `:446`. Add the show call at the TOP of that block and a hide call on the single-column `else`. Change `:446`:

```rust
    if let Some(cs) = cs {
```

to:

```rust
    if let Some(cs) = cs {
        update_next_scene_watermark(state, &cs);
```

Then, AFTER the closing `}` of that `if let Some(cs) = cs { … }` block (the function-final `}` of the `if let`, currently right before `snap_scroll_to_line`'s own closing brace at `:485-486`), add the single-column hide:

```rust
    } else {
        // Single-column (prose) or layout-not-ready: never show the two-column
        // watermark.
        state.next_scene_watermark.set_visible(false);
    }
```

So the tail of `snap_scroll_to_line` reads:

```rust
        schedule_bottom_clip_update(
            state.right_view.clone(),
            state.right_bottom_clip.clone(),
            state.right_scrolled_window.clone(),
            cs.split,
            line_count,
            state.is_prose(),
            Some(right_end),
            None, // exact_end set → trim path (and section clamp) never runs
        );
    } else {
        // Single-column (prose) or layout-not-ready: never show the two-column
        // watermark.
        state.next_scene_watermark.set_visible(false);
    }
}
```

- [ ] **Step 3: Confirm `ColumnSplit` is referable and `glib` is in scope**

`update_next_scene_watermark` references `super::viewport::ColumnSplit` (the same path `snap_scroll_to_line` already uses via `super::viewport::column_split`) and `glib::markup_escape_text`. `glib` is already used elsewhere in `scroll.rs` (e.g. `glib::idle_add_local_once` at `:465`), so no new `use` is needed. `ColumnSplit` is `pub(crate)` (`src/input/viewport.rs:1037`), so `super::viewport::ColumnSplit` resolves.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean. If `ColumnSplit` is not `pub(crate)`, the error is `struct ColumnSplit is private`; fix by changing `struct ColumnSplit` to `pub(crate) struct ColumnSplit` at `src/input/viewport.rs:1037` (it is already `pub(crate)` per inspection, so this should not trigger).

- [ ] **Step 5: Run the pure-logic test suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS (no regressions). This change is render-only; no `--bins` test asserts on watermark state.

- [ ] **Step 6: Commit**

```bash
git add src/input/scroll.rs
git commit -m "feat(scroll): render 'Next: Act N, Scene M' in empty right column

When a two-column spread's right column is empty because the scene ended in
the left column, show a dim centered label naming the act/scene that opens
the next spread. Derived from cs.next_page_top + (div1,div2); hidden for
end-of-work, empty-left mirror, and single-column.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Live verification (user)

This is a render-only change; per CLAUDE.md ("When to ASK THE USER to run e2e", overlay-layout / renders-correctly-on-screen criterion) the visible result is verified by the user.

- [ ] **Step 1: Confirm clean build + green pure suite**

Run:
```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -5
```
Expected: both succeed.

- [ ] **Step 2: Ask the user to launch the H8 1.3 spread and eyeball it**

The H8 1.3 spread (the screenshot: scene ends `I am your Lordship's.` / `[They exit.]` in the left column, empty right) should now show a dim, centered `Next: Act 1, Scene 4` in the right column. Give the user the single-work launch from CLAUDE.md → Headless Verification, e.g.:

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```
then navigate to the H8 1.3 end-of-scene spread and `grim /tmp/shot.png`. The user has stated they will test it live.

- [ ] **Step 3: Acceptance**

- Empty right column at a scene end → dim centered `Next: Act N, Scene M`.
- Right column with content → no label.
- End of work (final scene fills/empties the right with no next scene) → no label.
- Prose → no label.

---

## Self-Review

**Spec coverage:**
- Trigger (empty-right scene-break) → Task 4 Step 1 (`empty_right` condition). ✓
- Label text `Next: ` + `scene_label` → Task 4 Step 1 markup. ✓
- Centered position → Task 2 Step 2 (`halign`/`valign` Center). ✓
- Dim styling via `dim_fg` markup → Task 4 Step 1 `<span foreground>`. ✓
- Next scene from `cs.next_page_top` + `(div1,div2)`, no text inference → Task 1 + Task 4. ✓
- Overlay child of `right_scrolled_overlay`, not buffer text → Task 2 Step 2. ✓
- Update in `snap_scroll_to_line` `Some(cs)` block → Task 4 Step 2. ✓
- Hide on right-has-content / end-of-work / empty-left / prose → Task 4 (`empty_right` false) + Task 3 + Task 4 `else`. ✓
- Files touched (`app.rs`, `scroll.rs`, reuse `scene_label`/`work_line_for_buffer`/`dim_fg`) → matches. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to". All code shown in full. ✓

**Type consistency:** Field `next_scene_watermark: gtk4::Label` used identically in Tasks 2, 3, 4. Helper named `divs_at_buffer_line` in Tasks 1 and 4. `update_next_scene_watermark(state, &cs)` signature matches its call. `ColumnSplit` fields (`split`, `page_end`, `next_page_top`) match `src/input/viewport.rs:1037-1042`. `scene_label(i64, i64) -> String` matches `src/app.rs:4638`. ✓

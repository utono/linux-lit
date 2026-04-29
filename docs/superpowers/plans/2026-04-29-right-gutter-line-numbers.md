# Right Gutter Line Numbers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display verse line numbers every 5th line in a right-side gutter for plays and poems, matching scholarly edition conventions.

**Architecture:** Add a `GutterRendererText` on `TextWindowType::Right` using the same pattern as the existing left gutter. A pre-built `Vec<Option<i64>>` maps buffer lines to `line_in_div` values. The renderer's `query_data` callback checks each line and renders the number (with Pango markup for subdued styling) when `line_in_div % 5 == 0`.

**Tech Stack:** Rust, GTK4, sourceview5 (`GutterRendererText`), Pango markup

---

## File Structure

- **Modify:** `src/gutter.rs` — add `setup_line_number_gutter()` and `remove_line_number_renderer()`
- **Modify:** `src/app.rs` — add two AppState fields, build data vector during work display, teardown on work switch

---

### Task 1: Add `setup_line_number_gutter()` to `gutter.rs`

**Files:**
- Modify: `src/gutter.rs` (append after line 187)

- [ ] **Step 1: Add the `setup_line_number_gutter` function**

Append after the existing `setup_chunk_gutter` function (after line 187):

```rust
pub fn setup_line_number_gutter(
    view: &View,
    line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
    dim_color: &str,
    font_size_pt: u32,
) -> sourceview5::GutterRendererText {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Right);
    let renderer = sourceview5::GutterRendererText::new();
    renderer.set_xpad(4);
    renderer.set_xalign(1.0);
    renderer.set_yalign(0.5);
    renderer.set_size_request(36, -1);
    gutter.insert(&renderer, 0);

    let color = dim_color.to_string();
    let pango_size = font_size_pt * 1024;
    renderer.connect_query_data(move |renderer, _lines_obj, line| {
        let text_renderer = renderer
            .downcast_ref::<sourceview5::GutterRendererText>()
            .unwrap();
        let idx = line as usize;
        let nums = line_numbers.borrow();
        let show = idx < nums.len()
            && nums[idx].is_some_and(|n| n % 5 == 0);
        if show {
            let n = nums[idx].unwrap();
            text_renderer.set_markup(&format!(
                "<span foreground=\"{}\" size=\"{}\">{}</span>",
                color, pango_size, n,
            ));
        } else {
            text_renderer.set_markup("");
        }
    });

    renderer
}
```

- [ ] **Step 2: Add the `remove_line_number_renderer` function**

Append immediately after `setup_line_number_gutter`:

```rust
pub fn remove_line_number_renderer(view: &View, renderer: &sourceview5::GutterRendererText) {
    let gutter = sourceview5::prelude::ViewExt::gutter(view, gtk4::TextWindowType::Right);
    gutter.remove(renderer);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles with warnings about unused functions (they're not called yet)

- [ ] **Step 4: Commit**

```bash
git add src/gutter.rs
git commit -m "Add setup_line_number_gutter for right-side verse line numbers"
```

---

### Task 2: Add AppState fields and wire up lifecycle in `app.rs`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add fields to AppState struct**

In the `AppState` struct (around line 91, after `chunk_renderer`), add:

```rust
    pub line_number_renderer: Option<sourceview5::GutterRendererText>,
    pub line_numbers: Rc<RefCell<Vec<Option<i64>>>>,
```

- [ ] **Step 2: Initialize fields in AppState constructor**

In the AppState initializer (around line 840, after `chunk_renderer: None,`), add:

```rust
        line_number_renderer: None,
        line_numbers: Rc::new(RefCell::new(Vec::new())),
```

- [ ] **Step 3: Add teardown in `display_work_at_with_prepared`**

In `display_work_at_with_prepared`, right after the existing gutter teardown block (after the `chunk_renderer` teardown around line 1678), add:

```rust
    if let Some(old_renderer) = state.line_number_renderer.take() {
        crate::gutter::remove_line_number_renderer(&state.text_view, &old_renderer);
    }
```

- [ ] **Step 4: Add setup after bookmark population**

After the chunk data loading block (around line 1724, before the font size section), add the line number gutter setup:

```rust
    // Set up right-side line number gutter for plays/verse
    {
        let is_prose = state.current_work.as_ref()
            .map(|w| crate::db::line_types::is_prose_work(&w.work_type))
            .unwrap_or(true);
        if !is_prose {
            let new_line_numbers: Vec<Option<i64>> = if let Some(ref lm) = state.line_map {
                lm.buffer_to_work
                    .iter()
                    .map(|opt_idx| {
                        opt_idx.and_then(|idx| {
                            state.current_work.as_ref()?.lines.get(idx).map(|l| l.line_in_div)
                        })
                    })
                    .collect()
            } else {
                state.current_work.as_ref()
                    .map(|w| w.lines.iter().map(|l| Some(l.line_in_div)).collect())
                    .unwrap_or_default()
            };
            *state.line_numbers.borrow_mut() = new_line_numbers;
            let font_size_pt = (state.config.font_size as f32 * 0.8) as u32;
            let renderer = crate::gutter::setup_line_number_gutter(
                &state.text_view,
                state.line_numbers.clone(),
                &state.theme.dim_fg,
                font_size_pt,
            );
            state.line_number_renderer = Some(renderer);
        }
    }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: successful build, no errors

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "Wire up right-side line number gutter for plays and verse"
```

---

### Task 3: Manual verification

- [ ] **Step 1: Run the app and open a play**

Run: `cargo run` (user runs this)
Open Comedy of Errors (Err) via Ctrl+p

Expected: Line numbers 5, 10, 15, 20... appear in the right margin, right-aligned, in a subdued smaller font. Speaker names and stage directions show no numbers. Numbers reset each scene.

- [ ] **Step 2: Switch to a prose work**

Open a novel via Ctrl+p (e.g., Bleak House)

Expected: No right gutter visible. No line numbers.

- [ ] **Step 3: Switch back to a play**

Open another Shakespeare play via Ctrl+p

Expected: Line numbers reappear in the right gutter, correct per-scene numbering.

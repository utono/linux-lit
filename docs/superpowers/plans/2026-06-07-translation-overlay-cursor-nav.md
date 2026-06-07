# Translation Overlay Cursor Nav Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `,`/`q`/`j`/`k` inside the two-column translation overlay drive the real reader cursor (with identical MPV seek/playback), highlight the cursor line in BOTH columns, and auto-follow-scroll the highlighted original line.

**Architecture:** The overlay handler calls the SAME navigation functions the main card uses (so `state.current_line` moves and MPV seeks through the existing `after_page_change` path), then asks the overlay to re-highlight and follow. The overlay records each block's two `TextView`s so it can tag the cursor's line (offset `work_idx - start_idx`) in the original and translation buffers with a `cursor-line` paragraph-background tag, and scrolls minimally to keep the highlighted original line in view.

**Tech Stack:** Rust, GTK4 (`gtk4` 0.9, `v4_12`), sourceview5, existing linux-lit overlay/keymap/navigation code.

---

## Background facts (verified against the codebase)

Read this before starting; the tasks depend on these exact anchors.

- **Overlay struct** (`src/ui/translation_overlay.rs:62`): `TranslationOverlay`
  with `pub overlay`, `scrim`, `container`, `title`, `scrolled: ScrolledWindow`,
  `content_vbox: gtk4::Box`, and
  `block_widgets: RefCell<Vec<(usize, usize, gtk4::Box)>>`.
- **`show()`** (`translation_overlay.rs:142`) signature:
  `show(&self, title, blocks: &[TranslationBlock], card_width, card_height, text_fg: &str, dim_fg: &str, body_font_size: i32)`.
  Block-build loop at line 170: for a speaker block it makes `orig`/`trans` via
  `make_column` and sets their buffers; for an interlude (`speaker == None`) it
  makes a single `view: TextView`. It pushes `(block.start_idx, block.end_idx, block_box)`
  to `block_widgets` at line ~234.
- **`scroll_to_block`** (`translation_overlay.rs:260`) reads the 3-tuple via
  `find_map(|(s,e,w)| ...)` and defers a measure with
  `glib::idle_add_local_once`, using
  `widget.compute_point(&content_vbox, gtk4::graphene::Point::new(0.0,0.0))`.
- **`scroll(delta)`** (`translation_overlay.rs:244`) is the old viewport-scroll,
  still called by the handler's `j`/`k` arms.
- **Overlay handler** (`src/input/keymap.rs:742`):
  `handle_translation_overlay_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool`.
  `i` (top) and `"Escape"` close (hide + `InputMode::Reader`); `"j"`/`"k"` call
  `translation_overlay.scroll(±1)`; `_ => true` swallows. Routed from
  `InputMode::TranslationOverlay => handle_translation_overlay_key(state, key_name)`
  (`keymap.rs:99`).
- **Open path** (`src/app.rs`, `show_translation_overlay`): calls
  `s.translation_overlay.show(...)` then, if `cursor_idx` is Some,
  `s.translation_overlay.scroll_to_block(idx)`. `cursor_idx =
  s.work_line_for_buffer(s.current_line)`.
- **Nav functions** (`src/input/navigation.rs`), all `pub fn f(state: &mut AppState)`:
  `jump_to_prev_dialogue` (849), `jump_to_next_dialogue` (865),
  `cursor_prev_line` (882), `cursor_next_dialogue` (907). Each ends in
  `after_page_change(state, reason)` → conditional `seek_to_current_line` (the
  MPV seek). These also update the main card's `text_view` highlight/scroll —
  desired, and harmless to the overlay (different widgets).
- **Nav key names** (`src/input/keymap_config.rs`): `comma`→prev-dialogue,
  `q`→next-dialogue, `j`→cursor-next-dialogue, `k`→cursor-prev-line. In the
  handler, the `,` key arrives as `key_name == "comma"`.
- **Cursor color:** `state.theme.cursor_line_bg: String` (`src/theme.rs`). The
  card builds a `cursor-line` `TextTag` with
  `.paragraph_background(&theme.cursor_line_bg)` (`src/app.rs:984`).
- **GTK geometry APIs in use here:** `buffer.iter_at_line(line: i32) ->
  Option<TextIter>`, `view.line_yrange(&iter) -> (i32 y, i32 height)`,
  `widget.compute_point(&target, &graphene::Point) -> Option<Point>` (all used in
  `src/ui/gloss_overlay.rs`).
- **`work_line_for_buffer`** (`src/app.rs:442`): `&self, usize -> Option<usize>`.

## File Structure

- **Modify** `src/ui/translation_overlay.rs` — replace the `block_widgets` tuple
  with a `BlockEntry` struct holding the two views; add a `locate_line` pure
  helper (+ test); add `highlight_work_line` and `scroll_to_highlight`; thread
  `cursor_line_bg` into `show()` and add the `cursor-line` tag per buffer; update
  `scroll_to_block` to the new struct.
- **Modify** `src/app.rs` — pass `cursor_line_bg` into `show()`; after `show`,
  call `highlight_work_line` + `scroll_to_highlight` (replacing the lone
  `scroll_to_block`).
- **Modify** `src/input/keymap.rs` — replace the handler's `j`/`k` scroll arms
  with `comma`/`q`/`j`/`k` nav arms (via a local `nav` helper) that move the real
  cursor then re-highlight + follow.

---

## Task 1: `locate_line` pure helper + `BlockEntry` struct

Introduce the data the highlight needs and a testable mapping from a work-line to
(block index, line offset).

**Files:**
- Modify: `src/ui/translation_overlay.rs`

- [ ] **Step 1: Add the `BlockEntry` struct**

In `src/ui/translation_overlay.rs`, ABOVE the `TranslationOverlay` struct
(after the `TranslationBlock` definition), add:

```rust
/// One rendered block's widgets + source range, for cursor highlighting and
/// scroll-follow. `trans` is None for a non-spoken interlude block (it has a
/// single `orig` view).
struct BlockEntry {
    start_idx: usize,
    end_idx: usize,
    block_box: gtk4::Box,
    orig: gtk4::TextView,
    trans: Option<gtk4::TextView>,
}
```

Change the `TranslationOverlay` field:

```rust
    block_widgets: RefCell<Vec<(usize, usize, gtk4::Box)>>,
```
to:
```rust
    block_widgets: RefCell<Vec<BlockEntry>>,
```

- [ ] **Step 2: Write the failing test for `locate_line`**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of the file:

```rust
    #[test]
    fn locate_line_finds_block_and_offset() {
        // Two blocks: [10..=12] and [13..=13].
        let ranges = vec![(10usize, 12usize), (13, 13)];
        assert_eq!(locate_line(&ranges, 10), Some((0, 0)));
        assert_eq!(locate_line(&ranges, 12), Some((0, 2)));
        assert_eq!(locate_line(&ranges, 13), Some((1, 0)));
    }

    #[test]
    fn locate_line_returns_none_outside_any_block() {
        let ranges = vec![(10usize, 12usize)];
        assert_eq!(locate_line(&ranges, 9), None);
        assert_eq!(locate_line(&ranges, 13), None);
        assert_eq!(locate_line(&[], 0), None);
    }
```

- [ ] **Step 3: Run the test to verify it FAILS**

Run: `cargo test --bins translation_overlay::tests::locate_line -- --nocapture`
Expected: FAIL — `locate_line` not found (won't compile / unresolved).

- [ ] **Step 4: Implement `locate_line`**

Add this free function near `make_column`/`glib_escape` (outside the `impl`,
above the test module):

```rust
/// Given each block's inclusive (start_idx, end_idx) work-line range in order,
/// return (block_index, line_offset) for the block containing `work_idx`.
fn locate_line(ranges: &[(usize, usize)], work_idx: usize) -> Option<(usize, usize)> {
    for (i, (start, end)) in ranges.iter().enumerate() {
        if work_idx >= *start && work_idx <= *end {
            return Some((i, work_idx - start));
        }
    }
    None
}
```

- [ ] **Step 5: Make `show()` and `scroll_to_block` compile against `BlockEntry`**

In `show()`'s block-build loop (`translation_overlay.rs` ~line 170): hoist the
views so both branches can store them, and push a `BlockEntry`.

For the SPEAKER branch, after building `orig`/`trans` and appending them, change
the push. For the INTERLUDE branch, rename the local `view` and store it as
`orig` with `trans: None`. Replace the final
`self.block_widgets.borrow_mut().push((block.start_idx, block.end_idx, block_box));`
and restructure so both branches set an `orig: gtk4::TextView` and
`trans: Option<gtk4::TextView>` in scope before the push. Concretely, make the
loop body:

```rust
        for block in blocks {
            let block_box = gtk4::Box::new(Orientation::Vertical, 0);
            block_box.set_margin_start(side_margin);
            block_box.set_margin_end(side_margin);
            block_box.set_margin_top(14);

            let (orig_view, trans_view): (gtk4::TextView, Option<gtk4::TextView>) =
                if let Some(speaker) = &block.speaker {
                    let header = Label::new(None);
                    header.set_halign(Align::Start);
                    header.set_markup(&format!(
                        "<span foreground='{}' font_variant='small-caps' font_weight='normal' size='{}pt'>{}</span>",
                        text_fg,
                        header_pt,
                        glib_escape(speaker),
                    ));
                    header.set_margin_bottom(4);
                    block_box.append(&header);

                    let cols = gtk4::Box::new(Orientation::Horizontal, 0);
                    let orig = make_column(col_width, text_fg, false);
                    let trans = make_column(col_width, dim_fg, true);
                    let mut orig_text = String::new();
                    let mut trans_text = String::new();
                    for (o, t) in &block.lines {
                        orig_text.push_str(o);
                        orig_text.push('\n');
                        trans_text.push_str(t);
                        trans_text.push('\n');
                    }
                    orig.buffer().set_text(orig_text.trim_end_matches('\n'));
                    trans.buffer().set_text(trans_text.trim_end_matches('\n'));

                    let divider = gtk4::Separator::new(Orientation::Vertical);
                    divider.add_css_class("column-divider");
                    divider.set_margin_start(12);
                    divider.set_margin_end(12);

                    cols.append(&orig);
                    cols.append(&divider);
                    cols.append(&trans);
                    block_box.append(&cols);
                    (orig, Some(trans))
                } else {
                    let view = TextView::new();
                    view.set_editable(false);
                    view.set_cursor_visible(false);
                    view.set_focusable(false);
                    view.set_wrap_mode(gtk4::WrapMode::WordChar);
                    view.add_css_class("gloss-text");
                    let mut text = String::new();
                    for (o, _) in &block.lines {
                        text.push_str(o);
                        text.push('\n');
                    }
                    view.buffer().set_text(text.trim_end_matches('\n'));
                    block_box.append(&view);
                    (view, None)
                };

            self.content_vbox.append(&block_box);
            self.block_widgets.borrow_mut().push(BlockEntry {
                start_idx: block.start_idx,
                end_idx: block.end_idx,
                block_box,
                orig: orig_view,
                trans: trans_view,
            });
        }
```

Update `scroll_to_block` (`translation_overlay.rs:260`) to read the struct:

```rust
    pub fn scroll_to_block(&self, work_idx: usize) {
        let target = self.block_widgets.borrow().iter().find_map(|e| {
            if work_idx >= e.start_idx && work_idx <= e.end_idx {
                Some(e.block_box.clone())
            } else {
                None
            }
        });
        let Some(widget) = target else { return };
        let scrolled = self.scrolled.clone();
        let content = self.content_vbox.clone();
        glib::idle_add_local_once(move || {
            let origin = gtk4::graphene::Point::new(0.0, 0.0);
            if let Some(point) = widget.compute_point(&content, &origin) {
                let adj = scrolled.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(adj.lower());
                adj.set_value((point.y() as f64).clamp(adj.lower(), max));
            }
        });
    }
```

- [ ] **Step 6: Build + run the test to verify it PASSES**

Run: `cargo build && cargo test --bins translation_overlay::tests -- --nocapture`
Expected: clean build; all tests pass (the 4 prior + the 2 new `locate_line`
tests). Dead-code warnings on `BlockEntry.orig`/`trans` and `locate_line` are
EXPECTED (consumed in Task 2/3); do NOT add `#[allow(dead_code)]`.

- [ ] **Step 7: Commit**

```bash
git add src/ui/translation_overlay.rs
git commit -m "refactor(translation): store per-block views in BlockEntry + locate_line helper"
```

---

## Task 2: `cursor-line` tag per buffer + `highlight_work_line`

Thread the cursor color into `show()`, register a `cursor-line` tag on every
overlay buffer, and implement style-A highlighting.

**Files:**
- Modify: `src/ui/translation_overlay.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add `cursor_line_bg` param to `show()`**

Change the `show` signature (`translation_overlay.rs:142`) to add a final param:

```rust
    pub fn show(
        &self,
        title: &str,
        blocks: &[TranslationBlock],
        card_width: i32,
        card_height: i32,
        text_fg: &str,
        dim_fg: &str,
        body_font_size: i32,
        cursor_line_bg: &str,
    ) {
```

- [ ] **Step 2: Register the `cursor-line` tag on each buffer**

Add a small free helper near `make_column`:

```rust
/// Ensure the buffer has a `cursor-line` tag painting the paragraph background
/// with the theme's cursor-line color. Idempotent (lookup before add).
fn ensure_cursor_tag(buffer: &gtk4::TextBuffer, cursor_line_bg: &str) {
    if buffer.tag_table().lookup("cursor-line").is_none() {
        let tag = gtk4::TextTag::builder()
            .name("cursor-line")
            .paragraph_background(cursor_line_bg)
            .build();
        buffer.tag_table().add(&tag);
    }
}
```

In `show()`, right after each view's `buffer().set_text(...)` call (both the
`orig` and `trans` in the speaker branch, and the single `view` in the interlude
branch), register the tag. For the speaker branch add after the two `set_text`
lines:

```rust
                    ensure_cursor_tag(&orig.buffer(), cursor_line_bg);
                    ensure_cursor_tag(&trans.buffer(), cursor_line_bg);
```

For the interlude branch add after its `set_text`:

```rust
                    ensure_cursor_tag(&view.buffer(), cursor_line_bg);
```

- [ ] **Step 3: Implement `highlight_work_line`**

Add to the `impl TranslationOverlay` block:

```rust
    /// Highlight the cursor's source line `work_idx` in BOTH columns (style A):
    /// the original line on the left and its paired translation on the right.
    /// Clears any prior highlight first. No-op if the line is outside this scene.
    pub fn highlight_work_line(&self, work_idx: usize) {
        let entries = self.block_widgets.borrow();

        // Clear every buffer's existing highlight (small block count per scene).
        for e in entries.iter() {
            clear_cursor_tag(&e.orig.buffer());
            if let Some(t) = &e.trans {
                clear_cursor_tag(&t.buffer());
            }
        }

        let ranges: Vec<(usize, usize)> =
            entries.iter().map(|e| (e.start_idx, e.end_idx)).collect();
        let Some((bi, off)) = locate_line(&ranges, work_idx) else { return };
        let entry = &entries[bi];

        apply_cursor_tag(&entry.orig.buffer(), off as i32);
        if let Some(t) = &entry.trans {
            apply_cursor_tag(&t.buffer(), off as i32);
        }
    }
```

Add the two free helpers near `ensure_cursor_tag`:

```rust
/// Remove the `cursor-line` tag from the whole buffer (if the tag exists).
fn clear_cursor_tag(buffer: &gtk4::TextBuffer) {
    if let Some(tag) = buffer.tag_table().lookup("cursor-line") {
        let (start, end) = buffer.bounds();
        buffer.remove_tag(&tag, &start, &end);
    }
}

/// Apply the `cursor-line` tag to buffer line `line` (0-based). No-op if the
/// line or tag is missing.
fn apply_cursor_tag(buffer: &gtk4::TextBuffer, line: i32) {
    let Some(tag) = buffer.tag_table().lookup("cursor-line") else { return };
    let Some(start) = buffer.iter_at_line(line) else { return };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.apply_tag(&tag, &start, &end);
}
```

- [ ] **Step 4: Pass `cursor_line_bg` from the caller and highlight on open**

In `src/app.rs` `show_translation_overlay`, where `text_fg`/`dim_fg`/`body_font_size`
are read, add:

```rust
    let cursor_line_bg = s.theme.cursor_line_bg.clone();
```

Update the `s.translation_overlay.show(...)` call to pass it as the new final arg:

```rust
    s.translation_overlay.show(
        &label,
        &blocks,
        card_width,
        card_height,
        &text_fg,
        &dim_fg,
        body_font_size,
        &cursor_line_bg,
    );
```

Then replace the existing open-time scroll:

```rust
    if let Some(idx) = cursor_idx {
        s.translation_overlay.scroll_to_block(idx);
    }
```
with highlight + (Task 3 will add follow; for now keep `scroll_to_block`):

```rust
    if let Some(idx) = cursor_idx {
        s.translation_overlay.highlight_work_line(idx);
        s.translation_overlay.scroll_to_block(idx);
    }
```

- [ ] **Step 5: Build + test**

Run: `cargo build && cargo test --bins translation_overlay::tests`
Expected: clean build; all tests pass. `scroll_to_highlight` doesn't exist yet —
that's Task 3. `highlight_work_line` is now reachable (no dead_code on it).

- [ ] **Step 6: Commit**

```bash
git add src/ui/translation_overlay.rs src/app.rs
git commit -m "feat(translation): highlight cursor line in both overlay columns"
```

---

## Task 3: Minimal auto-follow scroll

Scroll only when the highlighted ORIGINAL line is outside the viewport.

**Files:**
- Modify: `src/ui/translation_overlay.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Implement `scroll_to_highlight`**

Add to the `impl TranslationOverlay` block:

```rust
    /// Scroll minimally so the highlighted ORIGINAL line for `work_idx` is fully
    /// in view: only scroll if it is above the viewport top or below its bottom,
    /// landing it just inside the crossed edge. No-op if the line isn't found.
    pub fn scroll_to_highlight(&self, work_idx: usize) {
        let (orig_view, off) = {
            let entries = self.block_widgets.borrow();
            let ranges: Vec<(usize, usize)> =
                entries.iter().map(|e| (e.start_idx, e.end_idx)).collect();
            let Some((bi, off)) = locate_line(&ranges, work_idx) else { return };
            (entries[bi].orig.clone(), off as i32)
        };

        let scrolled = self.scrolled.clone();
        let content = self.content_vbox.clone();
        // Defer one tick so allocations/wrapping are settled before measuring.
        glib::idle_add_local_once(move || {
            let Some(iter) = orig_view.buffer().iter_at_line(off) else { return };
            let (line_y, line_h) = orig_view.line_yrange(&iter);
            // Map the line's top within the orig view into content_vbox space.
            let pt = gtk4::graphene::Point::new(0.0, line_y as f64);
            let Some(mapped) = orig_view.compute_point(&content, &pt) else { return };
            let line_top = mapped.y() as f64;
            let line_bottom = line_top + line_h as f64;

            let adj = scrolled.vadjustment();
            let value = adj.value();
            let page = adj.page_size();
            let max = (adj.upper() - page).max(adj.lower());

            let new_value = if line_top < value {
                line_top
            } else if line_bottom > value + page {
                line_bottom - page
            } else {
                return; // already fully visible — don't move
            };
            adj.set_value(new_value.clamp(adj.lower(), max));
        });
    }
```

- [ ] **Step 2: Use it on open instead of `scroll_to_block`**

In `src/app.rs` `show_translation_overlay`, change the open block to:

```rust
    if let Some(idx) = cursor_idx {
        s.translation_overlay.highlight_work_line(idx);
        s.translation_overlay.scroll_to_highlight(idx);
    }
```

(removes the `scroll_to_block` call). `scroll_to_block` may now be unused — if the
build warns `scroll_to_block is never used`, DELETE the `scroll_to_block` method
from `translation_overlay.rs` (Task 1's version) rather than leaving dead code.
Verify nothing else calls it first: `rg -n "scroll_to_block" src/`.

- [ ] **Step 3: Build + test**

Run: `cargo build 2>&1 | rg -c "error\[" ; cargo test --bins translation_overlay::tests`
Expected: 0 errors; tests pass. No `scroll_to_block`-unused warning (either still
used or deleted).

- [ ] **Step 4: Commit**

```bash
git add src/ui/translation_overlay.rs src/app.rs
git commit -m "feat(translation): minimal auto-follow scroll for overlay cursor"
```

---

## Task 4: Nav binds in the overlay handler

Replace `j`/`k` scroll with `,`/`q`/`j`/`k` cursor navigation that drives the
real cursor + MPV, then re-highlights and follows.

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Replace the handler's scroll arms with nav arms**

In `src/input/keymap.rs`, the current `handle_translation_overlay_key`
(`keymap.rs:742`) has `"j"`/`"k"` arms calling `translation_overlay.scroll(±1)`.
Replace the whole function body's `match` so the four nav keys drive the real
cursor. Final function:

```rust
fn handle_translation_overlay_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool {
    // i (the same bind that opened the overlay) toggles it closed, matching
    // Escape. Without this, a second i would be swallowed by the catch-all.
    if key_name == "i" {
        let mut s = state.borrow_mut();
        s.translation_overlay.hide();
        s.input_mode = crate::app::InputMode::Reader;
        return true;
    }
    match key_name {
        "Escape" => {
            let mut s = state.borrow_mut();
            s.translation_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        // Dialogue navigation: drive the REAL cursor (same fns as the main
        // card), which also seeks MPV, then mirror the highlight + follow in
        // the overlay.
        "comma" => { overlay_nav(state, navigation::jump_to_prev_dialogue); true }
        "q" => { overlay_nav(state, navigation::jump_to_next_dialogue); true }
        "j" => { overlay_nav(state, navigation::cursor_next_dialogue); true }
        "k" => { overlay_nav(state, navigation::cursor_prev_line); true }
        // Swallow everything else so stray keys don't leak to the reader.
        _ => true,
    }
}

/// Run a main-card navigation function (moves `current_line` + seeks MPV via
/// `after_page_change`), then re-highlight and follow in the translation overlay.
fn overlay_nav(state: &Rc<RefCell<AppState>>, nav_fn: fn(&mut AppState)) {
    nav_fn(&mut state.borrow_mut());
    let s = state.borrow();
    if let Some(w) = s.work_line_for_buffer(s.current_line) {
        s.translation_overlay.highlight_work_line(w);
        s.translation_overlay.scroll_to_highlight(w);
    }
}
```

NOTE: confirm `navigation` is in scope in `keymap.rs` (sibling arms already call
`navigation::cursor_next_dialogue` in the action dispatch — grep
`navigation::cursor_next_dialogue` to confirm the path; if the module is imported
under a different alias, match it).

- [ ] **Step 2: Build + test**

Run: `cargo build 2>&1 | rg -c "error\[" ; cargo test --bins`
Expected: 0 errors; full `--bins` suite passes. The overlay's `scroll(delta)`
method may now be unused (the handler no longer calls it). If the build warns
`method scroll is never used`, DELETE the `scroll` method from
`translation_overlay.rs` (verify with `rg -n "\.scroll\(" src/` first that
nothing else calls it).

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs src/ui/translation_overlay.rs
git commit -m "feat(translation): , q j k drive real cursor + MPV in overlay"
```

---

## Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Build, full tests, clippy**

Run:
```bash
cargo build 2>&1 | rg -c "error\["
cargo test --bins 2>&1 | rg "test result:"
cargo clippy 2>&1 | rg "translation_overlay|keymap.rs" | rg -v "dead_code|never used|never read|never constructed"
```
Expected: 0 errors; all `--bins` tests pass (prior + 2 new `locate_line`); no real
clippy lints on the changed files.

- [ ] **Step 2: Ask the user to verify on screen**

Per CLAUDE.md the acceptance criterion here is visual + audio and the agent can't
reliably launch cage on the live session. Ask the user to launch H8 and confirm:

```bash
pkill -f target/debug/linux-lit; LINUX_LIT_WORK=H8 cargo run
```

In a dialogue scene, press `i` to open the overlay, then verify:
- the cursor line is highlighted in BOTH columns (original left + translation
  right; they may be at slightly different heights — expected)
- `,` / `q` / `j` / `k` move the highlight exactly as in the main card, and MPV
  audio seeks to the new line (if MPV is connected)
- the overlay auto-scrolls to keep the highlighted ORIGINAL line in view, only
  when it would otherwise leave the viewport
- `space` still pauses/plays
- `i` / `Escape` close, and the reader is already sitting on that line

Do not claim verified until the user confirms the on-screen/audio behavior.

---

## Self-review notes

- **Spec coverage:** §1 nav model → Task 4; §2 highlight mechanism (BlockEntry +
  views) → Task 1 + Task 2; §3 `highlight_work_line` style A → Task 2; §4
  auto-follow → Task 3; §5 handler wiring → Task 4. Scope/non-goals respected (no
  row-locking, no new MPV code, no keymap.json change, reuse `cursor_line_bg`,
  `space` untouched). Verification → Task 5.
- **Type consistency:** `BlockEntry` (with `orig`/`trans: Option<TextView>`),
  `locate_line(&[(usize,usize)], usize) -> Option<(usize,usize)>`,
  `highlight_work_line(usize)`, `scroll_to_highlight(usize)`, `overlay_nav(state,
  fn(&mut AppState))`, `show(... , cursor_line_bg: &str)` — used consistently
  across tasks. The `cursor-line` tag name matches the card's tag name but lives
  in each overlay buffer's own tag table (separate from the card's buffer), so no
  collision.
- **Flagged soft spots handled inline:** dead-code on `BlockEntry`
  fields/`locate_line` is expected until Task 2/3 (noted, no `#[allow]`); the
  now-unused `scroll_to_block`/`scroll` methods are explicitly deleted in Task
  3/4 with a grep check first; `navigation` module path confirmed against the
  existing action-dispatch call sites.
```

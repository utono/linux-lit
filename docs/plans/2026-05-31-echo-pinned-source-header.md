# Pinned Source-Turn Header Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin the echoes overlay's source turn as a fixed header with a fixed rule beneath it, so only the echo list scrolls underneath.

**Architecture:** Split the echoes presentation in `src/ui/gloss_overlay.rs` into a non-scrolling `echo_header_view` (source turn) + a `gtk4::Separator` rule + the existing `gloss_view` (now echo-list-only, inside the ScrolledWindow). Remove the Cairo-drawn rule from the bar overlay. `render_echoes` (`src/input/actions/echoes.rs`) passes source and echo docs separately.

**Tech Stack:** Rust, GTK4 (TextView, ScrolledWindow, Overlay, Separator, Box), Cairo (DrawingArea overlay for the accent bar + line numbers).

**Testing note:** This is GTK widget layout — not unit-testable. Each task is compile-verified; behavior is confirmed by the user in Task 7 (manual). Do NOT run `cargo run` (project rule). The 2 pre-existing `input::viewport::block_atom_tests` failures are known/unrelated.

---

## Reference facts (verified in source)

- `GlossOverlay` struct fields (`gloss_overlay.rs:16-38`) end with `text_margins: i32, column_width: i32,`. Constructor returns the struct as a field-shorthand list at `:269-291` (ends `column_width: column_width as i32,`).
- `container` append order in `new` (`:240`): title/headers already appended, then `container.append(&gloss_scroll_overlay);` at `:240`, then the footer box.
- `gloss_scroll_overlay.set_child(...)`/`add_overlay(&bar_drawing)` at `:220,233-236`. The vadjustment `value-changed` → `bar_drawing.queue_draw()` hook is at `:226-231`.
- The Cairo rule block is `gloss_overlay.rs:181-217` (comment "Horizontal rule separating…" through the closing `}` of the `if let Some(&first_echo_line)` block). It uses `echo_lines_clone` (declared `:119`, used ONLY here) and `rule_left` (declared `:122`, used ONLY here) — both become unused after removal.
- `show_echoes` signature (`:364`): `pub fn show_echoes(&self, doc: &str, card_height: i32, root_color: Option<&str>, dim_color: Option<&str>, selected: usize)`. Calls `populate_gloss_buffer_ex(&self.gloss_view, doc, self.text_margins, bar_left, &[], Some(selected), dim_color)` at `:385`.
- `populate_gloss_buffer_ex(view, gloss, _text_margins, bar_left, source_line_numbers, selected_echo, dim_color) -> (Vec<BarRange>, Vec<LineNumber>, Vec<i32>)` (`:594`) builds the full tag table and parses `<speaker>`/`<verse>`/`<gloss>`. For a source-only doc it returns empty `bar_ranges`/`echo_lines`.
- `show` (`:304`) hides the scroll overlay (`:315`). `show_gloss_with_color` (`:325`) shows it (`:355`). `show_synopsis` (`:447`) shows it (`:471`). `show_loading_message` (`:483`) hides it (`:493`). `hide` (`:520`) hides the whole container.
- `render_echoes` (`echoes.rs:538`) builds `doc = echo_overlay_source + <gloss> lines`, then `show_echoes(&doc, h, Some(&root), Some(&dim), index)`.
- `select_first_echo` (`echoes.rs`) sets index 0, `render_echoes`, `scroll_gloss_to_top`. `scroll_gloss_to_top` sets `gloss_scrolled.vadjustment()` to `lower()`.
- `gloss_view` setup (`:94-104`): non-editable, non-focusable, word-wrap, `top_margin(24)`, `left_margin(text_margins)`, `right_margin(column_width/8)`, css class `gloss-text`.

---

## File Structure

- **Modify** `src/ui/gloss_overlay.rs`:
  - Add struct fields `echo_header_view: gtk4::TextView`, `echo_rule: gtk4::Separator`.
  - Construct + append them in `new` (header above rule above scroll overlay); default hidden.
  - Remove the Cairo rule block + now-unused `echo_lines_clone`/`rule_left`.
  - Rewrite `show_echoes` to take `source_doc` + `echo_doc`, populate header + echo views, show header/rule.
  - Hide header/rule in `show`, `show_gloss_with_color`, `show_synopsis`, `show_loading_message`.
- **Modify** `src/input/actions/echoes.rs`:
  - `render_echoes`: build echo-only doc, pass both docs to `show_echoes`.
  - `select_first_echo`: comment-only clarification (behavior already correct).

---

## Task 1: Add `echo_header_view` + `echo_rule` widgets

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Add struct fields**

In `src/ui/gloss_overlay.rs`, in `pub struct GlossOverlay { … }`, add two fields immediately before `text_margins: i32,` (around line 36):

```rust
    echo_header_view: gtk4::TextView,
    echo_rule: gtk4::Separator,
    text_margins: i32,
```

- [ ] **Step 2: Construct the widgets in `new`**

In `GlossOverlay::new`, immediately BEFORE the line `container.append(&gloss_scroll_overlay);` (currently `:240`), insert:

```rust
        // Echoes-only: a fixed source-turn header + a fixed rule, above the
        // scrolling echo list. Hidden in all non-echo overlay modes.
        let echo_header_view = gtk4::TextView::new();
        echo_header_view.set_editable(false);
        echo_header_view.set_cursor_visible(false);
        echo_header_view.set_focusable(false);
        echo_header_view.set_wrap_mode(gtk4::WrapMode::Word);
        echo_header_view.set_left_margin(text_margins as i32);
        echo_header_view.set_right_margin(right_margin);
        echo_header_view.set_top_margin(24);
        echo_header_view.add_css_class("gloss-text");
        echo_header_view.set_visible(false);
        container.append(&echo_header_view);

        let echo_rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        echo_rule.set_margin_start(text_margins as i32);
        echo_rule.set_margin_end(right_margin);
        echo_rule.set_visible(false);
        container.append(&echo_rule);
```

(`right_margin` is already in scope — declared `let right_margin = column_width as i32 / 8;` near `:99`.)

- [ ] **Step 3: Add fields to the constructor return**

In the `GlossOverlay { … }` return block (`:269-291`), add the two fields (field shorthand) immediately before `text_margins: text_margins as i32,`:

```rust
            echo_lines,
            echo_header_view,
            echo_rule,
            text_margins: text_margins as i32,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -8`
Expected: builds clean (a `field never read` note on `echo_header_view`/`echo_rule` is acceptable until later tasks use them; do NOT add `#[allow(...)]`).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Add fixed echo header view and rule widgets

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Remove the Cairo-drawn rule

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Delete the rule block**

In `src/ui/gloss_overlay.rs`, delete the entire Cairo rule block — from the comment line `// Horizontal rule separating the quoted source turn from the echo` (currently `:181`) through the closing brace of its `if let Some(&first_echo_line) = echos.first()` block (the `}` at `:217`, just before the `});` that closes `set_draw_func`). After deletion, the draw_func's last content is the line-number drawing block, immediately followed by `});`.

- [ ] **Step 2: Remove the now-unused captured clones**

Two `let` bindings captured for the closure are now unused. Remove:
- `let echo_lines_clone = echo_lines.clone();` (currently `:119`)
- `let rule_left = text_margins as i32;` (currently `:122`)

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -8`
Expected: builds clean, NO `unused variable` warnings for `echo_lines_clone` or `rule_left` (confirming both were removed and nothing else referenced them). The `echo_lines` field is still used by `scroll_echo_into_view`, so it stays.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy 2>&1 | rg 'gloss_overlay.rs' | rg -v 'show_gloss' | head`
Expected: no new warnings from the draw_func edit.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Remove Cairo-drawn echo rule (replaced by Separator widget)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Rewrite `show_echoes` for the two-view split

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Change the signature and body**

Replace the entire `pub fn show_echoes(...)` (`:364` through its closing brace at `:402`) with:

```rust
    /// Render the echoes overlay: a fixed source-turn header + rule, above the
    /// scrolling echo list. `source_doc` is the <speaker>/<verse> turn; `echo_doc`
    /// is only the <gloss> lines.
    pub fn show_echoes(
        &self,
        source_doc: &str,
        echo_doc: &str,
        card_height: i32,
        root_color: Option<&str>,
        dim_color: Option<&str>,
        selected: usize,
    ) {
        self.container.set_height_request(card_height);
        self.title.set_visible(false);
        let left = self.column_width / 8;
        self.title.set_margin_start(left);
        self.gloss_view.set_left_margin(left);
        self.echo_header_view.set_left_margin(left);
        self.hint.set_text("Esc close · a play echo · Tab play turn · n/p select · Enter open work · c copy · s curate · R refresh");
        self.orig_header.set_visible(false);
        self.original_label.set_visible(false);
        self.corr_header.set_visible(false);
        self.corrected_label.set_visible(false);

        if let Some(color) = root_color {
            if let Some((r, g, b)) = parse_hex_color(color) {
                *self.bar_color.borrow_mut() = (r, g, b);
            }
        }

        let bar_left = self.column_width / 8;
        *self.bar_x.borrow_mut() = bar_left;

        // Fixed header: render the source turn into the non-scrolling view.
        // Reuse populate_gloss_buffer_ex (it builds the speaker/verse tags and
        // returns empty bar data for a source-only doc).
        let _ = populate_gloss_buffer_ex(
            &self.echo_header_view, source_doc, self.text_margins, bar_left, &[], None, dim_color);
        self.echo_header_view.set_visible(true);
        self.echo_rule.set_visible(true);

        // Scrolling list: only the echoes. echo_lines/bar_ranges are now indexed
        // from the first echo (no source lines to offset past).
        let (ranges, nums, echo_lines) = populate_gloss_buffer_ex(
            &self.gloss_view, echo_doc, self.text_margins, bar_left, &[], Some(selected), dim_color);
        *self.bar_ranges.borrow_mut() = ranges;
        *self.line_numbers.borrow_mut() = nums;
        *self.echo_lines.borrow_mut() = echo_lines;
        // Repaint the bar overlay after GTK lays out the rebuilt buffer (drawing
        // synchronously reads stale per-line geometry).
        let bar = self.bar_drawing.clone();
        glib::idle_add_local_once(move || bar.queue_draw());

        self.gloss_scroll_overlay.set_visible(true);
        self.gloss_scrolled.vadjustment().set_value(0.0);
        self.hint.set_visible(true);
        self.scrim.set_visible(false);
        self.container.set_visible(true);
    }
```

- [ ] **Step 2: Verify it compiles (expect a call-site error)**

Run: `cargo build 2>&1 | tail -12`
Expected: a compile ERROR at the `show_echoes(...)` call site in `src/input/actions/echoes.rs` (`this function takes 6 arguments but 5 were supplied`). That is expected and fixed in Task 4. Confirm the only error is that call site — nothing inside `show_echoes`.

- [ ] **Step 3: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Split show_echoes into fixed header + scrolling echo list

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Update `render_echoes` to pass source + echo docs

**Files:**
- Modify: `src/input/actions/echoes.rs`

- [ ] **Step 1: Rewrite render_echoes**

In `src/input/actions/echoes.rs`, replace the body of `fn render_echoes(s: &mut AppState) { … }` with:

```rust
fn render_echoes(s: &mut AppState) {
    let source_doc = s.echo_overlay_source.clone();
    let mut echo_doc = String::new();
    for link in &s.echo_overlay_links {
        let title = s.echo_overlay_titles.get(&link.echo_work_abbrev)
            .cloned()
            .unwrap_or_else(|| link.echo_work_abbrev.clone());
        let star = if link.curated { "★ " } else { "" };
        echo_doc.push_str(&format!(
            "<gloss>[{}\"{}\" — {} {}.{}]</gloss>\n",
            star, link.echo_text, title, link.echo_div1, link.echo_div2
        ));
    }

    let h = s.scrolled_window.height();
    let root = s.theme.root_color.clone();
    let dim = s.theme.dim_fg.clone();
    s.gloss_overlay.show_echoes(&source_doc, &echo_doc, h, Some(&root), Some(&dim), s.echo_overlay_index);
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -6`
Expected: builds clean (the Task-1 `field never read` warnings on `echo_header_view`/`echo_rule` are now gone — both are used).

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Pass source and echo docs separately to show_echoes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Hide header/rule in non-echo overlay modes

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

In each of the four non-echo show methods, hide the echo header and rule so the pinned header only appears in the echoes view.

- [ ] **Step 1: `show`**

In `pub fn show(&self, …)` (`:304`), immediately after `self.gloss_scroll_overlay.set_visible(false);` (`:315`), add:

```rust
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
```

- [ ] **Step 2: `show_gloss_with_color`**

In `pub fn show_gloss_with_color(&self, …)`, immediately after the four `corr_header`/`corrected_label`/`orig_header`/`original_label` `set_visible(false)` lines (right before the `if let Some(color) = root_color` block), add:

```rust
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
```

- [ ] **Step 3: `show_synopsis`**

In `pub fn show_synopsis(&self, …)`, immediately after `self.position_label.set_visible(false);` (just before the `*self.bar_ranges.borrow_mut() = Vec::new();` line), add:

```rust
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
```

- [ ] **Step 4: `show_loading_message`**

In `pub fn show_loading_message(&self, …)`, immediately after `self.gloss_scroll_overlay.set_visible(false);` (`:493`), add:

```rust
        self.echo_header_view.set_visible(false);
        self.echo_rule.set_visible(false);
```

- [ ] **Step 5: Verify it compiles + clippy + tests**

Run: `cargo build 2>&1 | tail -4 && cargo clippy 2>&1 | rg 'gloss_overlay.rs' | rg -v 'show_gloss' | head && cargo test 2>&1 | tail -5`
Expected: builds clean; no new clippy warnings; tests show only the 2 known pre-existing `block_atom_tests` failures.

- [ ] **Step 6: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "Hide echo header/rule in gloss, synopsis, diff, and loading modes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Clarify `gg` (select_first_echo) for the pinned header

**Files:**
- Modify: `src/input/actions/echoes.rs`

`select_first_echo` already calls `scroll_gloss_to_top`, which now scrolls the echo-only list to its top while the header stays pinned — already correct. Only the doc comment needs to reflect the pinned-header reality.

- [ ] **Step 1: Update the comment**

In `src/input/actions/echoes.rs`, the function above `select_first_echo` has a doc comment mentioning scrolling "to the very top so the source turn's first line is visible." Replace that doc comment with:

```rust
/// Move the accent-bar selection to the first echo (`gg`) and scroll the echo
/// list to its top. The source turn is a fixed header, so it stays visible.
```

(Leave the function body unchanged — `scroll_gloss_to_top` on the echo-only view is the correct behavior.)

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -3`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Clarify gg comment for pinned-header echo overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Manual verification (user runs the app)

GTK layout cannot be exercised in `cargo test`. Hand these to the user.

- [ ] **Step 1: Build for the user**

Run: `cargo build 2>&1 | tail -3`
Expected: clean build.

- [ ] **Step 2: User reproduction script**

Ask the user to `cargo run`, open a work, make a selection / be on a turn, press `i` to open the echoes overlay, then:
1. The source turn (speaker + verse lines) sits at the top with a horizontal rule directly beneath it.
2. Press `n`/`p` repeatedly to step through echoes.
   - Expect: the source turn AND the rule stay FIXED at the top; only the echo list scrolls beneath the rule. The accent bar tracks the selected echo within the scrolling region.
3. Press `gg` → the echo list scrolls to the first echo; the header stays pinned.
4. Press `G` → the last echo is revealed (scrolled into the list region); header still pinned.
5. Press `Escape` → overlay closes.
6. Open a normal gloss overlay (the gloss/`I`-on-a-glossed-line or synopsis path) → confirm NO stray echo header or rule appears there.

- [ ] **Step 3: Note any layout issues**

If the header clips a multi-line source turn, the header view may need its natural sizing checked (it should size to content within the vertical Box; it has no height request). Report back if clipping occurs.

---

## Self-Review

**Spec coverage:**
- Fixed `echo_header_view` + `echo_rule` Separator, hidden by default → Task 1. ✓
- Header/rule appended above the scroll overlay → Task 1 Step 2 (before `container.append(&gloss_scroll_overlay)`). ✓
- Remove the Cairo rule + unused `echo_lines_clone`/`rule_left` → Task 2. ✓
- `show_echoes(source_doc, echo_doc, …)`, populate header via `populate_gloss_buffer_ex`, show header/rule, echo-only `gloss_view` → Task 3. ✓
- `render_echoes` passes both docs → Task 4. ✓
- Hide header/rule in `show`/`show_gloss_with_color`/`show_synopsis`/`show_loading_message` → Task 5. ✓
- `gg` scrolls echo list to top, header pinned → Task 6 (behavior already correct; comment updated). ✓
- Manual verification → Task 7. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. ✓

**Type consistency:** `echo_header_view: gtk4::TextView` and `echo_rule: gtk4::Separator` used identically across Tasks 1, 3, 5. `show_echoes(source_doc: &str, echo_doc: &str, card_height: i32, root_color, dim_color, selected)` signature (Task 3) matches the Task-4 call site. `populate_gloss_buffer_ex` called with the documented 7-arg shape in both Task 3 calls. ✓

**Note:** `scroll_echo_into_view` (echoes.rs) is unchanged — it already scrolls `gloss_scrolled` by `echo_lines[i]`, which are now echo-relative; no edit needed. The accent bar's draw_func still maps `bar_ranges` against `gloss_view` (now echo-only), which is correct.

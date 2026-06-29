# Two-Column E-Reader Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Arden-style two-column page layout to linux-lit's e-reader mode — text flows down the left column and continues at the top of the right column — toggled by `Alt+[`, with per-column outer-margin line numbers.

**Architecture:** One shared `sourceview5::Buffer` rendered by two side-by-side `View`s (Approach A). A pure `column_split` kernel reuses the existing visibility math to decide where the left column ends and the right begins. All existing tags (highlight, dim, cursor, gloss), MPV sync, and the `gg`/`G`/`x`/`y` navigation flow stay on the single buffer addressed by absolute line index; only the page-turn / scroll / clip layer becomes column-aware. Scroll mode is untouched.

**Tech Stack:** Rust, GTK4 (gtk4-rs), libadwaita, sourceview5, serde_json.

**Spec:** `docs/superpowers/specs/2026-06-01-two-column-ereader-layout-design.md`

---

## Background the engineer needs

linux-lit renders all text into ONE `sourceview5::View` (`state.text_view`,
created once at `src/app.rs:611`, never recreated) backed by ONE buffer
(`state.buffer`). Pagination works by scrolling that view to `state.page_top_line`
and covering the partial bottom line with a `bottom_clip` box. Everything
addresses lines by absolute buffer index.

Key reused primitives (do not reinvent):
- `visible_range(text_view, buffer, page_top, line_count, usable_height) ->
  VisibleRange { last_fit, total_height, count }` — `src/input/viewport.rs:56`.
  Walks lines from `page_top` summing heights until `usable_height` is exceeded.
- `trim_visible_range(range, page_top, text_view, buffer, is_prose)` —
  `src/input/viewport.rs:446`. Trims trailing dangling speakers / stage
  directions / split stanzas off a range.
- `last_fully_visible_line(state, top)` — `src/input/viewport.rs:803`. Returns
  the last line that fits one viewport height from `top`, after trim.
- `next_page_top(state, top)` / `prev_page_top(state, current_top)` —
  `src/input/viewport.rs:837` / `:872`. Compute adjacent page boundaries; both
  consume `last_fully_visible_line`.
- `back_up_for_speaker(buffer, line)` — `viewport.rs:598`. Walks a page-top back
  to include a preceding speaker/header.
- `effective_line_count()` — `src/app.rs:301`. Total renderable buffer lines
  (includes inserted translation lines).

The pagination unit tests (`src/input/viewport.rs`, `#[cfg(test)] mod`) use
PURE text helpers over `&[String]` with a fixed `lines_per_page` —
`trim_visible_range_pure`, `last_dialogue_in_range`, `next_dialogue_from_text`,
`back_up_for_speaker_text`, and a `page_forward(lines, page_top, lines_per_page,
is_prose)` driver. The column-split logic gets a pure analog tested the same way.

GTK-layout-dependent code (anything calling `text_view.height()` /
`line_yrange`) CANNOT be unit-tested headlessly. For those tasks, the
verification step is `cargo build` + `cargo clippy` (compile/borrow-check is the
gate) and a noted manual check; the user runs the app (per project CLAUDE.md —
never run `cargo run` yourself).

**Keybind two-file rule:** a reader binding must be added to BOTH
`src/input/keymap_config.rs` (compiled default) AND
`/home/mlj/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (stow source),
or the JSON silently overrides the compiled default at runtime.

---

## File Structure

- `src/config.rs` — add persisted `column_count: u8` field (default 1).
- `src/input/actions/mod.rs` — add `Action::ToggleColumnLayout` + `name()` +
  `category()` arms.
- `src/input/keymap_config.rs` — add `Alt+bracketleft → ToggleColumnLayout`.
- `/home/mlj/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — same binding.
- `src/input/viewport.rs` — NEW pure `column_split_pure` + its unit tests; NEW
  GTK-bound `column_split(state)`; make `last_fully_visible_line` column-aware;
  make `is_line_fully_visible` span both columns.
- `src/app.rs` — add second `View` (`right_view`) + `right_scrolled` +
  `columns_hbox` to the widget tree and to `AppState`; share `state.buffer`;
  install a second line-number gutter; add `column_count` accessor helpers.
- `src/input/scroll.rs` — make `set_page` / `snap_scroll_to_line` /
  `update_bottom_clip` scroll and clip both columns.
- `src/input/navigation.rs` — make `jump_to_end` (`G`) anchor a two-column page;
  add `toggle_column_layout` handler.
- `src/input/keymap.rs` — dispatch `ToggleColumnLayout`.

---

## Phase 1 — Plumbing: config field, Action, keybind

This phase adds the toggle's data + dispatch with NO rendering change yet
(toggling will flip a config value that nothing reads until Phase 4). Fully
compile-checked and partially unit-tested.

### Task 1: Add `column_count` config field

**Files:**
- Modify: `src/config.rs` (struct ~line 30-72, defaults ~line 87, Default impl
  ~line 136-159)

- [ ] **Step 1: Add the field to the `Config` struct**

In `src/config.rs`, inside `pub struct Config { ... }`, after the
`pub text_margins: u32,` field (line 41), add:

```rust
    #[serde(default = "default_column_count")]
    pub column_count: u8,
```

- [ ] **Step 2: Add the default function**

After `fn default_text_margins()` (near line 100-102), add:

```rust
fn default_column_count() -> u8 {
    1
}
```

- [ ] **Step 3: Add to the `Default` impl**

In `impl Default for Config`, after `text_margins: default_text_margins(),`
(line ~141), add:

```rust
            column_count: default_column_count(),
```

- [ ] **Step 4: Do NOT add to the load() force-reset block**

The spec requires `column_count` to persist across restart. Confirm you did
NOT add a `config.column_count = ...` line to the reset block in `load()`
(config.rs ~line 190-195). Leaving it out means the saved value persists.

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean (warnings about unused field are fine — it's read in
Phase 4).

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "config: add persisted column_count field (default 1)"
```

### Task 2: Add `Action::ToggleColumnLayout`

**Files:**
- Modify: `src/input/actions/mod.rs` (enum ~line 110-123, `category()` ~line
  207-222, `name()` ~line 323)

- [ ] **Step 1: Add the enum variant**

In `src/input/actions/mod.rs`, in the `pub enum Action`, in the "Settings (in
reader)" group after `ToggleSignColumn,` (line ~116), add:

```rust
    ToggleColumnLayout,
```

- [ ] **Step 2: Add to `category()` (exhaustive match — required to compile)**

In the Display group `|` chain (lines ~207-222), add a line:

```rust
            | Action::ToggleColumnLayout
```

(e.g. immediately after `| Action::ToggleSignColumn`). The chain must still end
with `=> Category::Display,`.

- [ ] **Step 3: Add to `name()` (exhaustive match — required to compile)**

In the `name()` match (line ~323, near `Action::ToggleSignColumn =>
"ToggleSignColumn",`), add:

```rust
            Action::ToggleColumnLayout => "ToggleColumnLayout",
```

- [ ] **Step 4: Write a test that the action round-trips through serde**

The JSON keymap loader parses action names via serde (`parse_action` in
keymap_config.rs). Add this test to the `#[cfg(test)] mod` in
`src/input/actions/mod.rs` (if no test mod exists, create one at end of file):

```rust
#[cfg(test)]
mod column_layout_action_tests {
    use super::*;

    #[test]
    fn toggle_column_layout_serde_roundtrip() {
        let json = "\"ToggleColumnLayout\"";
        let action: Action = serde_json::from_str(json).expect("parse");
        assert_eq!(action, Action::ToggleColumnLayout);
        assert_eq!(action.name(), "ToggleColumnLayout");
        assert_eq!(action.category(), Category::Display);
    }
}
```

(`Action` must derive `PartialEq` for `assert_eq!` — it already derives
`Deserialize, Serialize`; check it also derives `PartialEq`. If not, this test
can compare `action.name()` only.)

- [ ] **Step 5: Run the test**

Run: `cargo test --lib column_layout_action_tests`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs
git commit -m "actions: add ToggleColumnLayout (Display category)"
```

### Task 3: Bind `Alt+[` to `ToggleColumnLayout` in both keymap files

**Files:**
- Modify: `src/input/keymap_config.rs` (display_bindings ~line 283-301)
- Modify: `/home/mlj/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

- [ ] **Step 1: Add the compiled-in default binding**

In `src/input/keymap_config.rs`, in the `display_bindings()` vec near the other
alt display toggles (`(KeyCombo::alt("d"), Action::ToggleDim),` line ~292), add:

```rust
        (KeyCombo::alt("bracketleft"), Action::ToggleColumnLayout),
```

- [ ] **Step 2: Add the stow keymap.json binding**

In `/home/mlj/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, in the
`"reader"` array near the existing `bracketleft` entry (`{"key":
"bracketleft", "action": "JumpToPrevChapter"},`), add a sibling object:

```json
    {"key": "bracketleft", "alt": true, "action": "ToggleColumnLayout"},
```

- [ ] **Step 3: Write a test that the compiled binding resolves**

In the `#[cfg(test)] mod` of `src/input/keymap_config.rs`, add:

```rust
    #[test]
    fn alt_bracketleft_is_toggle_column_layout() {
        let km = Keymap::default();
        assert_eq!(
            km.lookup("bracketleft", false, false, true),
            Some(Action::ToggleColumnLayout),
        );
        // plain bracketleft is unchanged
        assert_eq!(
            km.lookup("bracketleft", false, false, false),
            Some(Action::JumpToPrevChapter),
        );
    }
```

(Check the exact constructor for the default keymap used by other tests in this
file — they call `Keymap::default()` or build from `default_reader_bindings()`.
Match the existing test style; the lookup signature is `lookup(key, ctrl,
shift, alt)`.)

- [ ] **Step 4: Run the test**

Run: `cargo test --lib alt_bracketleft_is_toggle_column_layout`
Expected: PASS.

- [ ] **Step 5: Deploy the stow keymap (so runtime picks it up later)**

Run: `cd /home/mlj/tty-dotfiles && stow -R linux-lit`
Expected: no output (symlink refreshed). Verify:
`readlink -f ~/.config/linux-lit/keymap.json` points into tty-dotfiles.

- [ ] **Step 6: Commit (linux-lit repo only — tty-dotfiles is a separate repo)**

```bash
git add src/input/keymap_config.rs
git commit -m "keymap: bind Alt+[ to ToggleColumnLayout"
```

Then commit the dotfile in its own repo:

```bash
git -C /home/mlj/tty-dotfiles add linux-lit/.config/linux-lit/keymap.json
git -C /home/mlj/tty-dotfiles commit -m "linux-lit: bind Alt+[ to ToggleColumnLayout"
```

---

## Phase 2 — Pure column-split kernel (fully unit-tested)

The heart of two-column flow, isolated as a pure function over `&[String]` so it
is testable without GTK — mirroring the existing `page_forward` test driver.

### Task 4: Add `column_split_pure` and unit tests

**Files:**
- Modify: `src/input/viewport.rs` (add pure fn near `last_fully_visible_line`;
  add tests in the existing `#[cfg(test)] mod`)

- [ ] **Step 1: Write the failing tests**

In the `#[cfg(test)] mod` of `src/input/viewport.rs`, add a new test module.
This reuses the existing pure helpers `trim_visible_range_pure(lines, page_top,
raw_last_fit, is_prose)` and `is_dialogue`/`is_speaker` already used by the
pagination tests. The model: each column fits `col_lines` lines; the left column
is `[page_top .. split-1]`, the right is `[split .. page_end]`.

```rust
#[cfg(test)]
mod column_split_tests {
    use super::*;

    fn lines(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_falls_after_left_column_capacity() {
        // 10 dialogue lines, each column holds 3 → left [0..2], right [3..5],
        // next page starts at 6.
        let l = lines(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let split = column_split_pure(&l, 0, 3, true);
        assert_eq!(split.split, 3, "right column starts at line 3");
        assert_eq!(split.page_end, 5, "page ends at line 5 (right col last)");
        assert_eq!(split.next_page_top, 6, "next page starts at line 6");
    }

    #[test]
    fn split_clamps_at_end_of_text() {
        // 4 lines, columns hold 3 → left [0..2], right [3..3], end of text.
        let l = lines(&["a", "b", "c", "d"]);
        let split = column_split_pure(&l, 0, 3, true);
        assert_eq!(split.split, 3);
        assert_eq!(split.page_end, 3);
        assert_eq!(split.next_page_top, 4); // == line_count → at end
    }

    #[test]
    fn left_column_does_not_end_on_dangling_speaker() {
        // Capacity 3 but line 2 is a speaker → left column trims to [0..1],
        // the speaker moves to the right column with its dialogue.
        let l = lines(&["First line.", "Second line.", "HAMLET", "To be.", "Or not.", "End."]);
        let split = column_split_pure(&l, 0, 3, false);
        assert_eq!(split.split, 2, "speaker pushed to right column");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib column_split_tests`
Expected: FAIL — `column_split_pure` / `ColumnSplit` not defined.

- [ ] **Step 3: Implement `ColumnSplit` and `column_split_pure`**

In `src/input/viewport.rs` (near `last_fully_visible_line`, outside the test
mod), add. NOTE: `ColumnSplit` is defined WITHOUT `#[cfg(test)]` because Task 8
will use it from non-test code — only the pure functions are test-gated:

```rust
/// Result of splitting a page into two columns. Lines `[page_top .. split-1]`
/// fill the left column; `[split .. page_end]` fill the right column;
/// `next_page_top` is the first line of the following page (== line_count when
/// this is the last page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnSplit {
    pub(crate) split: usize,
    pub(crate) page_end: usize,
    pub(crate) next_page_top: usize,
}

/// Pure two-column split over a slice of line texts. `col_lines` is how many
/// lines fit in ONE column. Reuses `trim_visible_range_pure` so neither column
/// ends on a dangling speaker / stage direction / split stanza, matching the
/// single-column page-boundary rules.
///
/// Test-only analog of the GTK-bound `column_split`; the real one measures pixel
/// heights instead of a fixed `col_lines`.
#[cfg(test)]
pub(crate) fn column_split_pure(
    lines: &[String],
    page_top: usize,
    col_lines: usize,
    is_prose: bool,
) -> ColumnSplit {
    let line_count = lines.len();
    if line_count == 0 || page_top >= line_count {
        return ColumnSplit { split: page_top, page_end: page_top, next_page_top: line_count };
    }
    // Left column: fit col_lines, then trim trailing dangling context.
    let left_raw = (page_top + col_lines - 1).min(line_count - 1);
    let left_last = trim_visible_range_pure(lines, page_top, left_raw, is_prose);
    let split = (left_last + 1).min(line_count);
    if split >= line_count {
        return ColumnSplit { split, page_end: left_last, next_page_top: line_count };
    }
    // Right column: fit col_lines from split, then trim.
    let right_raw = (split + col_lines - 1).min(line_count - 1);
    let right_last = trim_visible_range_pure(lines, split, right_raw, is_prose);
    let next_top = (right_last + 1).min(line_count);
    ColumnSplit { split, page_end: right_last, next_page_top: next_top }
}
```

NOTE: confirm the exact name and signature of the existing pure trim helper used
by the pagination tests (search for `fn trim_visible_range_pure` or
`trim_visible_range_pure(` in the test mod). If it is named differently (e.g.
returns the trimmed last index directly), adapt the call. The three tests above
assume it returns the trimmed last-fitting index. If the speaker-trim test fails
because the helper signature differs, fix the call, not the test's intent.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib column_split_tests`
Expected: PASS (all three).

- [ ] **Step 5: Commit**

```bash
git add src/input/viewport.rs
git commit -m "viewport: add pure column_split kernel + unit tests"
```

### Task 5: Source+translation atom in the split trim

**Files:**
- Modify: `src/input/viewport.rs` (`column_split_pure` + test)

- [ ] **Step 1: Write the failing test**

A translation line follows its source line. The split must not fall between
them. NOTE (from Task 4): `column_split_pure` lives inside the
`#[cfg(test)] mod headless_pagination_tests` (because it calls the test-gated
`trim_visible_range_pure`), and the Task 4 split tests were added to that same
mod, not a standalone `column_split_tests` mod. Add this test and
`column_split_pure_tr` to the SAME `headless_pagination_tests` mod. Translation
lines in the pure model are marked by a parallel `&[bool]`; the new fn accepts
it. Run tests with `cargo test --bin linux-lit` (binary crate, no `--lib`).

```rust
    #[test]
    fn split_keeps_source_and_translation_together() {
        // line 1 is the translation of line 0; line 3 translation of line 2.
        // Capacity 3 would put split after line 2 (a source line) leaving its
        // translation (line 3) orphaned at the right column top — so the split
        // must back up to keep the pair together: split = 2 → left [0..1],
        // right starts at the source line 2 with its translation 3.
        let l = lines(&["src0", "tr0", "src1", "tr1", "src2", "tr2"]);
        let is_trans = vec![false, true, false, true, false, true];
        let split = column_split_pure_tr(&l, &is_trans, 0, 3, false);
        // left column ends on a translation line (1), not splitting pair (2,3)
        assert_eq!(split.split, 2);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib column_split_tests::split_keeps_source_and_translation_together`
Expected: FAIL — `column_split_pure_tr` not defined.

- [ ] **Step 3: Implement the translation-aware variant**

Add to `src/input/viewport.rs` (test-gated, like `column_split_pure`):

```rust
/// Like `column_split_pure` but, given a parallel `is_translation` slice, never
/// lets the left/right split fall between a source line and its immediately-
/// following translation line — the pair moves together to the right column.
#[cfg(test)]
pub(crate) fn column_split_pure_tr(
    lines: &[String],
    is_translation: &[bool],
    page_top: usize,
    col_lines: usize,
    is_prose: bool,
) -> ColumnSplit {
    let mut cs = column_split_pure(lines, page_top, col_lines, is_prose);
    // If the right column would START on a translation line, that translation's
    // source is the last line of the left column — back the split up by one so
    // the source moves with its translation.
    while cs.split > page_top + 1
        && is_translation.get(cs.split).copied().unwrap_or(false)
    {
        cs.split -= 1;
    }
    cs
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib column_split_tests`
Expected: PASS (all four).

- [ ] **Step 5: Commit**

```bash
git add src/input/viewport.rs
git commit -m "viewport: keep source+translation pair together at column split"
```

---

## Phase 3 — Widget: second view and second gutter

GTK-layout work. Verification is `cargo build` + `cargo clippy`; the user
visually confirms in the app. The right view is created but not yet shown
(column_count still defaults to 1, and Phase 4 wires the show/hide + split).

### Task 6: Add `right_view`, `right_scrolled`, `columns_hbox` to the widget tree and AppState

**Files:**
- Modify: `src/app.rs` (build_ui ~line 611-708; AppState struct ~line 79-135;
  AppState construction ~line 930-990)

- [ ] **Step 1: Build the second view + scrolled window**

In `src/app.rs` build_ui, AFTER the existing `scrolled_overlay` assembly (the
block ending where `bottom_clip` is added as an overlay, ~line 674) and BEFORE
`top_spacer` (~line 677), the existing single `scrolled_overlay` becomes the
LEFT column. Add a parallel right column sharing the same `buffer`:

```rust
    // RIGHT column view — shares the same buffer as the left view. Hidden
    // until column_count == 2 (set in Phase 4's apply step).
    let right_view = View::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();
    right_view.set_show_line_numbers(false);
    right_view.set_highlight_current_line(false);
    right_view.set_pixels_above_lines(config.line_spacing as i32);
    right_view.set_pixels_below_lines(config.line_spacing as i32);
    right_view.set_left_margin(config.text_margins as i32);
    right_view.set_right_margin(config.text_margins as i32 + crate::config::EXTRA_RIGHT_MARGIN);
    right_view.set_top_margin(0);
    right_view.set_bottom_margin(40);

    let right_scrolled = ScrolledWindow::builder()
        .child(&right_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .valign(gtk4::Align::Fill)
        .overflow(gtk4::Overflow::Hidden)
        .build();
    right_scrolled.add_css_class("card-bottom");

    let right_scrolled_overlay = gtk4::Overlay::new();
    right_scrolled_overlay.set_child(Some(&right_scrolled));
    right_scrolled_overlay.set_vexpand(true);
    right_scrolled_overlay.set_hexpand(true);

    let right_bottom_clip = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    right_bottom_clip.set_valign(gtk4::Align::End);
    right_bottom_clip.set_hexpand(true);
    right_bottom_clip.set_height_request(0);
    right_bottom_clip.add_css_class("card-bottom");
    right_scrolled_overlay.add_overlay(&right_bottom_clip);

    // Columns row: left | right. Right starts hidden (1-column default).
    let columns_hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    columns_hbox.set_vexpand(true);
    columns_hbox.set_hexpand(true);
    columns_hbox.append(&scrolled_overlay);
    columns_hbox.append(&right_scrolled_overlay);
    right_scrolled_overlay.set_visible(false);
```

- [ ] **Step 2: Put `columns_hbox` into the card instead of the lone left overlay**

Change the `card_vbox` assembly (~line 684-687) from appending
`scrolled_overlay` to appending `columns_hbox`:

```rust
    let card_vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card_vbox.set_vexpand(true);
    card_vbox.append(&top_spacer);
    card_vbox.append(&columns_hbox);
```

(Leave `top_spacer` and the `page_turn_overlay` wrapping unchanged — the
snapshot still targets `card_vbox`, which now contains both columns.)

- [ ] **Step 3: Add the new fields to `AppState`**

In `pub struct AppState` (~line 79-135), after `pub scrolled_window:
ScrolledWindow,` (line 95), add:

```rust
    pub right_view: View,
    pub right_scrolled_window: ScrolledWindow,
    pub right_scrolled_overlay: gtk4::Overlay,
    pub right_bottom_clip: gtk4::Box,
    pub columns_hbox: gtk4::Box,
    pub right_line_number_renderer: Option<sourceview5::GutterRendererText>,
```

- [ ] **Step 4: Populate the new fields in the AppState constructor**

In the `AppState { ... }` struct-literal construction (~line 930-990), add:

```rust
        right_view: right_view.clone(),
        right_scrolled_window: right_scrolled.clone(),
        right_scrolled_overlay: right_scrolled_overlay.clone(),
        right_bottom_clip: right_bottom_clip.clone(),
        columns_hbox: columns_hbox.clone(),
        right_line_number_renderer: None,
```

(Match the existing clone-into-struct style used for `scrolled_window` etc.
Ensure `right_view`/`right_scrolled`/etc. are still in scope at the
construction site; if build_ui returns before constructing AppState, thread them
through the same way `scrolled` is.)

- [ ] **Step 5: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: compiles clean. The right view exists but is hidden and shows the
same buffer scrolled to the top (no split wiring yet — that's Phase 4).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "app: add hidden right column view sharing the buffer"
```

### Task 7: Install a line-number gutter on the right view

**Files:**
- Modify: `src/app.rs` (wherever `setup_line_number_gutter` is called for the
  left view — search `setup_line_number_gutter(`; ~line 1972)

- [ ] **Step 1: Mirror the left view's line-number gutter onto the right view**

Find the existing call that builds `state.line_number_renderer` via
`crate::gutter::setup_line_number_gutter(&state.text_view, state.line_numbers.clone(), ...)`.
Immediately after it, add an analogous call for the right view, storing it in
the new field:

```rust
    state.right_line_number_renderer = Some(crate::gutter::setup_line_number_gutter(
        &state.right_view,
        state.line_numbers.clone(),
        &dim_color,      // reuse the exact same args the left call uses
        &font_family,    // (copy the variable names from the left call site)
        font_size_pt,
    ));
```

Use the EXACT same argument expressions the left-view call uses (copy them
verbatim from the call site — the dim color, font family, and size variables
already exist there). The renderer reads the shared `state.line_numbers` and is
driven by GTK's per-rendered-line `query-data` callback, so it needs no
page_top knowledge.

- [ ] **Step 2: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean. (Right gutter won't be visible until the right view is shown in
Phase 4, but it's wired.)

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "app: install line-number gutter on right column view"
```

---

## Phase 4 — Column-aware paging, clipping, visibility, and the toggle

Wires `column_count` into the render path. After this phase, `Alt+[` actually
switches layouts and `x`/`y` turn two-column pages.

### Task 8: Add `column_count()` accessor + GTK-bound `column_split`

**Files:**
- Modify: `src/app.rs` (impl AppState — add accessor)
- Modify: `src/input/viewport.rs` (add GTK-bound `column_split`)

- [ ] **Step 1: Add a `column_count()` accessor to AppState**

In `impl AppState` (near `effective_line_count`, app.rs:301), add:

```rust
    /// 1 in scroll mode (two columns are e-reader-only) or when config says 1;
    /// 2 only in e-reader mode with column_count == 2.
    pub fn column_count(&self) -> u8 {
        match self.config.navigation_mode {
            crate::config::NavigationMode::EReader => self.config.column_count.clamp(1, 2),
            crate::config::NavigationMode::Scroll => 1,
        }
    }
```

- [ ] **Step 2: Add the GTK-bound `column_split` returning split / end / next**

In `src/input/viewport.rs`, add (NOT test-gated — this is the real one):

```rust
/// GTK-bound two-column split: measures pixel heights per column (column width
/// = each view's width) and returns where the right column starts, where the
/// page ends, and the next page top. Mirrors `column_split_pure` but uses
/// `visible_range` + `trim_visible_range` against `state.text_view` (left) and
/// `state.right_view` (right). Single-column callers should not use this.
pub(crate) fn column_split(state: &AppState, page_top: usize) -> ColumnSplit {
    let line_count = state.effective_line_count();
    if line_count == 0 || page_top >= line_count {
        return ColumnSplit { split: page_top, page_end: page_top, next_page_top: line_count };
    }
    let is_prose = state.is_prose();

    // Left column.
    let left_h = state.text_view.height();
    let lc_top = page_top;
    let left = if left_h > 0 {
        let guard = descender_guard_px(&state.text_view, lc_top);
        let usable = left_h - guard - super::scroll::BASE_BOTTOM_MARGIN;
        let r = visible_range(&state.text_view, &state.buffer, lc_top, line_count, usable);
        trim_visible_range(r, lc_top, &state.text_view, &state.buffer, is_prose)
    } else {
        // Layout not ready — fall back to a single-column estimate.
        visible_range(&state.text_view, &state.buffer, lc_top, line_count, 1)
    };
    let split = (left.last_fit + 1).min(line_count);
    if split >= line_count || left.count == 0 {
        return ColumnSplit { split, page_end: left.last_fit, next_page_top: line_count };
    }

    // Right column (measure against the right view).
    let right_h = state.right_view.height().max(left_h);
    let right = if right_h > 0 {
        let guard = descender_guard_px(&state.right_view, split);
        let usable = right_h - guard - super::scroll::BASE_BOTTOM_MARGIN;
        let r = visible_range(&state.right_view, &state.buffer, split, line_count, usable);
        trim_visible_range(r, split, &state.right_view, &state.buffer, is_prose)
    } else {
        visible_range(&state.right_view, &state.buffer, split, line_count, 1)
    };
    let next_top = (right.last_fit + 1).min(line_count);
    ColumnSplit { split, page_end: right.last_fit, next_page_top: next_top }
}
```

`ColumnSplit` is already non-test-gated (Task 4 defined it without
`#[cfg(test)]`), so it is usable here. Keep `column_split_pure` /
`column_split_pure_tr` test-gated — only the GTK-bound `column_split` is used in
production.

- [ ] **Step 3: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean. `column_split` is defined but not yet called.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/input/viewport.rs
git commit -m "viewport: add column_count accessor and GTK-bound column_split"
```

### Task 9: Make `last_fully_visible_line` and `is_line_fully_visible` column-aware

**Files:**
- Modify: `src/input/viewport.rs` (`last_fully_visible_line` ~803;
  `is_line_fully_visible` ~985)

- [ ] **Step 1: `last_fully_visible_line` returns the right column end in 2-col**

Replace the body of `last_fully_visible_line` (viewport.rs:803-818) so that in
two-column mode it returns the page end `E` from `column_split`:

```rust
pub(crate) fn last_fully_visible_line(state: &AppState, top: usize) -> usize {
    if state.column_count() == 2 {
        return column_split(state, top).page_end;
    }
    let widget_height = state.text_view.height();
    if widget_height <= 0 {
        return top;
    }
    let line_count = state.effective_line_count();
    let descender_guard = descender_guard_px(&state.text_view, top);
    let usable_height = widget_height - descender_guard - BASE_BOTTOM_MARGIN;
    let range = visible_range(&state.text_view, &state.buffer, top, line_count, usable_height);
    let is_prose = state.is_prose();
    let trimmed = trim_visible_range(range, top, &state.text_view, &state.buffer, is_prose);
    trimmed.last_fit
}
```

This makes `next_page_top` / `prev_page_top` (and thus `x`/`y`) turn a full
two-column page with no further edits, because they consume
`last_fully_visible_line` abstractly.

- [ ] **Step 2: `is_line_fully_visible` spans both columns**

In `is_line_fully_visible` (viewport.rs:985), add a two-column branch at the
top (after the `loading_work` and `line < page_top_line` guards, before the
cache fast-path):

```rust
    if state.column_count() == 2 {
        let cs = column_split(state, state.page_top_line);
        return line >= state.page_top_line && line <= cs.page_end;
    }
```

(Leave the single-column cache fast-path and cold-start fallback below
unchanged.)

- [ ] **Step 3: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/input/viewport.rs
git commit -m "viewport: make page-boundary and visibility checks column-aware"
```

### Task 10: Make `snap_scroll_to_line` / `set_page` / `update_bottom_clip` scroll and clip both columns

**Files:**
- Modify: `src/input/scroll.rs` (`snap_scroll_to_line` ~351;
  `update_bottom_clip` ~445; `set_page` Instant arm + clear ~135-293)

- [ ] **Step 1: Scroll the right view in `snap_scroll_to_line`**

In `snap_scroll_to_line` (scroll.rs:351), after the existing left-view scroll
logic positions the left view at `line` and computes the clamped `effective_top`
(just before the `schedule_bottom_clip_update` call ~line 399), add a
two-column branch that scrolls the right view to the split and sizes both clips:

```rust
    if state.column_count() == 2 {
        let cs = super::viewport::column_split(state, effective_top);
        // Scroll the right view so its top line is `cs.split`.
        if let Some(iter) = state.buffer.iter_at_line(cs.split as i32) {
            let (y, _h) = state.right_view.line_yrange(&iter);
            let radj = state.right_scrolled_window.vadjustment();
            let rmax = (radj.upper() - radj.page_size()).max(0.0);
            radj.set_value((y as f64).min(rmax));
        }
        // Both clips updated by schedule_bottom_clip_update calls below.
    }
```

- [ ] **Step 2: Add a right-column clip scheduler call**

Still in `snap_scroll_to_line`, after the existing
`schedule_bottom_clip_update(...)` for the left column (~line 399-406), add an
analogous call for the right column when `column_count() == 2`:

```rust
    if state.column_count() == 2 {
        let cs = super::viewport::column_split(state, effective_top);
        schedule_bottom_clip_update(
            state.right_view.clone(),
            state.right_bottom_clip.clone(),
            state.right_scrolled_window.clone(),
            cs.split,
            (cs.page_end + 1).min(state.effective_line_count()),
            state.is_prose(),
        );
    }
```

Note: `update_bottom_clip`'s `line_count` parameter bounds how far it walks; for
the right column we want it to stop at `page_end`, so pass `cs.page_end + 1` as
the effective line_count for that column's clip computation.

- [ ] **Step 3: Clear the right clip on instant/transition page sets**

In `set_page` (scroll.rs:135), the Instant arm and the start of the Crossfade/
Slide arms call `clear_old_page_dim` and set `page_top_line`. No change needed
there beyond Step 1-2 (snap handles the right scroll). But in
`set_page_instant` (scroll.rs:343) and the Instant arm, ensure the right view
follows: those already call `snap_scroll_to_line`, which now handles the right
column. Confirm by reading — no code change if snap is the single choke point.

- [ ] **Step 4: When toggling back to 1 column, hide right clip**

(Handled in Task 11's apply function, which sets the right clip height to 0 and
hides the overlay. No change here.)

- [ ] **Step 5: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/input/scroll.rs
git commit -m "scroll: scroll and clip the right column in two-column mode"
```

### Task 11: Implement the `ToggleColumnLayout` handler + dispatch

**Files:**
- Modify: `src/input/navigation.rs` (add `toggle_column_layout`)
- Modify: `src/input/keymap.rs` (dispatch arm)

- [ ] **Step 1: Write the toggle handler**

In `src/input/navigation.rs`, add:

```rust
/// Toggle between one- and two-column e-reader layout (Alt+[). No-op in scroll
/// mode. Flips config.column_count, shows/hides the right column, and recomputes
/// the current page so current_line stays visible.
pub fn toggle_column_layout(state: &mut AppState) {
    if !matches!(state.config.navigation_mode, crate::config::NavigationMode::EReader) {
        crate::logging::log("COLUMNS: ignored (not e-reader mode)");
        return;
    }
    if state.current_work.is_none() {
        return;
    }
    let new_count = if state.config.column_count >= 2 { 1 } else { 2 };
    state.config.column_count = new_count;
    crate::config::save(&state.config);

    let two = new_count == 2;
    state.right_scrolled_overlay.set_visible(two);
    if !two {
        state.right_bottom_clip.set_height_request(0);
    }

    // Recompute the page from the current page_top so the cursor stays visible.
    // back_up_for_speaker keeps the page-top on a clean boundary.
    let top = super::viewport::back_up_for_speaker(&state.buffer, state.page_top_line);
    set_page_instant(state, top);
    // If current_line fell off the recomputed page, page to it.
    if !super::viewport::is_line_on_screen(state, state.current_line) {
        let new_top = super::viewport::page_turn_top(&state.buffer, state.current_line);
        set_page_instant(state, new_top);
    }
    after_page_change(state, PageChangeReason::JumpToLine);
    crate::logging::log(&format!("COLUMNS: now {} column(s)", new_count));
}
```

(Confirm `set_page_instant`, `after_page_change`, `PageChangeReason`,
`page_turn_top`, `is_line_on_screen` are in scope in navigation.rs — they are
used by neighboring functions like `jump_to_start`. Match the existing `use`
paths.)

- [ ] **Step 2: Dispatch the action**

In `src/input/keymap.rs` `dispatch_action`, a no-op placeholder arm for
`ToggleColumnLayout` ALREADY EXISTS (added in Task 2 to keep the exhaustive
match compiling):

```rust
        ToggleColumnLayout => {
            // TODO: Task 11 - implement column layout toggle
        }
```

REPLACE that placeholder body (do NOT add a second arm) with the real call:

```rust
        ToggleColumnLayout => crate::input::navigation::toggle_column_layout(&mut state.borrow_mut()),
```

(Match the exact borrow style of the neighboring arms — many do
`&mut state.borrow_mut()`; some take `state` + `tokio_handle`. Use the simple
`&mut state.borrow_mut()` form like `ToggleDim`'s arm.)

- [ ] **Step 3: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean.

- [ ] **Step 4: Manual verification (user runs the app)**

Build only (`cargo build`). Then note for the user to verify:
- In e-reader mode, `Alt+[` shows two columns; text continues left→right.
- `Alt+[` again returns to one column.
- The setting persists across a restart.
- In scroll mode, `Alt+[` does nothing.
- Line numbers appear in each column's outer margin.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs src/input/keymap.rs
git commit -m "nav: implement Alt+[ column-layout toggle and dispatch"
```

---

## Phase 5 — `G` two-column anchor (the one non-trivial nav behavior)

`gg`, `x`/`y`, navigation jumps, and visual mode all flow from the column-aware
primitives wired in Phase 4 and need no per-key code. `G` is the exception: its
backward viewport-fill walk must accumulate TWO columns' worth of content.

### Task 12: Make `jump_to_end` (`G`) anchor a two-column final page

**Files:**
- Modify: `src/input/navigation.rs` (`jump_to_end` ~168-242)

- [ ] **Step 1: Add a two-column branch to the backward-fill walk**

In `jump_to_end` (navigation.rs:168), the block at lines ~206-237 walks
backward from `line_count-1` accumulating heights until they fill ONE
`widget_height` to find `new_top`. Wrap it so that in two-column mode it
accumulates `2 × usable_height` worth of column content. Replace the
`let new_top = if widget_height > 0 && line_count > 0 { ... } else { ... };`
block with:

```rust
    let widget_height = state.text_view.height();
    let columns = state.column_count() as i32;
    let new_top = if widget_height > 0 && line_count > 0 {
        // Fill `columns` columns of content backward from the last line.
        let usable_height = widget_height; // last page: no next-page guard
        let capacity = usable_height * columns;
        let mut total: i32 = 0;
        let mut top = line_count - 1;
        loop {
            let Some(iter) = state.buffer.iter_at_line(top as i32) else { break };
            let (_y, h) = state.text_view.line_yrange(&iter);
            if total + h > capacity && top != line_count - 1 {
                top += 1;
                break;
            }
            total += h;
            if top == 0 {
                break;
            }
            top -= 1;
        }
        top
    } else {
        let lpp = lines_per_page(state) * (columns as usize);
        line_count.saturating_sub(lpp)
    };
```

This keeps `current_line = target` (the last dialogue line) unchanged — it lands
at the bottom of the right column because the right column is the last one
filled. The left column fills with the preceding content.

- [ ] **Step 2: Build + clippy**

Run: `cargo build && cargo clippy`
Expected: clean.

- [ ] **Step 3: Manual verification (user runs the app)**

Note for the user: in two-column mode, `G` lands the last dialogue line at the
bottom of the RIGHT column with the left column filled (no blank left column,
no GTK scroll clamp). `gg` puts line 0 at the left column top.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "nav: anchor G (jump_to_end) to a full two-column final page"
```

---

## Phase 6 — Regression + final verification

### Task 13: Run the full test suite and headless pagination harnesses

**Files:** none (verification only)

- [ ] **Step 1: Run all unit tests**

Run: `cargo test --bin linux-lit` (binary crate — there is no lib target, so
`--lib` fails)
Expected: the new `column_split` tests, `column_layout_action_tests`, and the
keymap test PASS, and single-column behavior is unchanged (`column_count()`
returns 1 by default). KNOWN PRE-EXISTING FAILURES (verified present on `master`
before any two-column work — NOT regressions from this feature): two tests in
`block_atom_tests` — `block_start_stops_at_blank` and
`block_start_in_verse_stanza_bounded_above_by_stage_direction`. Confirm the
pass/fail count matches the baseline (these 2 fail, everything else passes); do
not attempt to fix them as part of this feature.

- [ ] **Step 2: Run clippy clean**

Run: `cargo clippy --all-targets`
Expected: no errors. Address any new warnings introduced by the changes.

- [ ] **Step 3: Manual two-column acceptance (user runs the app)**

Provide this checklist to the user (do NOT run `cargo run` yourself):
- `Alt+[` toggles 1↔2 columns in e-reader mode; no-op in scroll mode; persists.
- Two-column text flows left→right; right column starts where left ends.
- `x` turns to the next two-column page; `y` returns to the exact prior page.
- `gg` → line 0 at left-column top; `G` → last dialogue at right-column bottom.
- Navigation jumps (`comma`/`q` dialogue, `2`/`3` scene, `[`/`{` chapter):
  on-screen target moves cursor only; off-page target turns the page.
- Visual mode (`v` then `j`/`k`): selection flows across the column boundary;
  highlight renders in both columns; yank/gloss read the full range.
- `i` (translations): columns fit fewer source lines; a source line and its
  translation never split across the column break; toggling keeps cursor visible.
- MPV playback sync: highlight and page turns land in the correct column.

- [ ] **Step 4: Final commit (if any cleanup was needed)**

```bash
git add -A
git commit -m "two-column layout: test + clippy cleanup"
```

---

## Notes on what required NO dedicated task (inherited from Phase 4 primitives)

- **`gg` (`jump_to_start`):** unchanged — `set_page_instant(state, 0)` plus the
  column-aware `snap_scroll_to_line` renders two columns automatically.
- **`x` / `y` (`page_forward` / `page_backward`):** unchanged — they consume
  `last_fully_visible_line` / `next_page_top` / `prev_page_top`, which became
  column-aware in Task 9. The `page_back_stack` already stores page tops.
- **Navigation jumps (dialogue/scene/chapter):** unchanged — they compute a
  target line then call `scroll_after_jump_forward`/`_backward`, which consult
  the now-column-aware `is_line_on_screen` / `is_line_fully_visible`.
- **Visual mode:** unchanged — `SelectionState` is absolute-index based;
  `apply_selection_highlight` tags the shared buffer; tags render in whichever
  column shows the line. `move_selection_cursor` calls
  `update_highlight_and_ensure_visible`, which uses the column-aware visibility.
- **Translations (`i`):** the GTK `column_split` already measures pixel heights
  over `effective_line_count` (which includes inserted translation lines). The
  source+translation atom rule is covered by the pure-kernel Task 5; if the
  GTK split needs the same protection in practice (translation orphaned at right
  column top), add the same back-up loop to `column_split` using
  `state.translation_lines` — deferred unless manual testing shows it. NOTE
  (flagged during Task 5): the pure `column_split_pure_tr` only adjusts `split`,
  not `page_end`/`next_page_top`. If the GTK `column_split` ports this rule, it
  should RECOMPUTE the right column (`page_end`, `next_page_top`) from the
  backed-up `split`, since the right column now starts one line earlier and its
  capacity shifts.

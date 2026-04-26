# Library Picker Look-and-Feel Refresh — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refresh the Ctrl+P library picker with an "editorial" look — header bar with title and count crumb, hairline dividers, drop shadow, responsive sizing, footer hint row, and a tinted selection background.

**Architecture:** Pure UI changes in `src/ui/library_picker.rs` (struct + builder additions, no behavior changes) and `src/theme.rs` (CSS rule rewrite + two new blended color variables). The picker keeps its existing public API; all callers in `src/app.rs` and `src/input/` are unchanged. Sizing becomes responsive by hooking the toplevel window's width/height notifications after realize, falling back to floor dimensions before realize completes.

**Tech Stack:** Rust, GTK4 (`gtk4` crate), no new dependencies.

**Spec:** `docs/superpowers/specs/2026-04-26-library-picker-look-and-feel-design.md`

---

## File Map

- **Modify:** `src/theme.rs` — replace the `.library-picker*` CSS block, add two new blended color variables (`focus_ring`, `picker_selection_bg`, `header_border`), and use them in the new rules.
- **Modify:** `src/ui/library_picker.rs` — add `header_box`, `header_title`, `header_crumb`, `footer_box` struct fields; add private builders/updaters; remove fixed `width_request`/`height_request`; install a responsive sizing hook in `attach()`.
- **No changes:** `src/app.rs`, `src/input/keymap.rs`, `src/input/navigation.rs`, other UI files. Other pickers (`bookmark_picker.rs`, `media_picker.rs`, `concordance_picker.rs`) are out of scope and remain unchanged.

---

## Task 1: Add blended color variables and replace `.library-picker*` CSS block

**Files:**
- Modify: `src/theme.rs:347-477` (the `generate_css` function — specifically the `.library-picker*` rules at lines 360-364 and the format-args block at lines 464-475)

This is a pure CSS change. There are no unit tests for `generate_css`; verification is `cargo build` plus a visual smoke check by the user. Tests for unrelated picker logic continue to pass; we run `cargo test` to confirm.

- [ ] **Step 1: Replace the `.library-picker*` CSS rules**

Find this block in `src/theme.rs` (currently lines 360-364):

```rust
         .library-picker {{ background-color: {bg}; color: {fg}; \
           padding: 16px; border-radius: 12px; border: 1px solid {dim}; }} \
         .library-picker entry {{ margin-bottom: 8px; }} \
         .library-picker row:selected {{ background-color: {cursor_bg}; color: {cursor_fg}; }} \
         .library-picker-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
```

Replace it with:

```rust
         .library-picker {{ background-color: {bg}; color: {fg}; \
           padding: 0; border-radius: 12px; border: 1px solid {dim}; \
           box-shadow: 0 18px 48px rgba(0, 0, 0, 0.22), \
                       0 2px 6px rgba(0, 0, 0, 0.08); }} \
         .library-picker-header {{ padding: 14px 22px 10px; \
           border-bottom: 1px solid {header_border}; }} \
         .library-picker-title {{ font-size: 13px; font-weight: 600; \
           letter-spacing: 2px; color: {dim}; }} \
         .library-picker-crumb {{ font-size: 12px; color: {dim}; }} \
         .library-picker entry {{ margin: 12px 18px 8px; \
           padding: 8px 12px; border: 1px solid {dim}; \
           border-radius: 8px; background-color: {bg}; color: {fg}; }} \
         .library-picker entry:focus {{ \
           box-shadow: 0 0 0 3px {focus_ring}; }} \
         .library-picker scrolledwindow {{ padding: 4px 8px 10px; }} \
         .library-picker row {{ padding: 8px 14px; \
           border-radius: 6px; }} \
         .library-picker row label.picker-item-detail {{ \
           font-variant-numeric: tabular-nums; min-width: 32px; \
           color: {dim}; }} \
         .library-picker row:selected {{ \
           background-color: {picker_selection_bg}; color: {cursor_fg}; }} \
         .library-picker row:selected label.picker-item-detail {{ \
           color: {cursor_fg}; opacity: 0.8; }} \
         .library-picker-footer {{ padding: 8px 22px 12px; \
           border-top: 1px solid {header_border}; \
           font-size: 11px; letter-spacing: 1.5px; color: {dim}; }} \
         .library-picker-scrim {{ background-color: rgba(0, 0, 0, 0.3); }} \
```

Note: `text-transform: uppercase` is intentionally **not** in the CSS. Header titles and footer text are uppercased in Rust before being passed to `Label::set_text`, because GTK 4 CSS does not support `text-transform`.

- [ ] **Step 2: Add the three new blended color variables to the format-args block**

Find this block (currently lines 464-476):

```rust
        root = theme.root_color,
        bg = theme.text_bg,
        fg = theme.text_fg,
        dim = theme.dim_fg,
        cursor_bg = theme.cursor_bg,
        cursor_fg = theme.cursor_fg,
        vocab = theme.vocab_fg,
        vocab_popup_fg = blend_colors(&theme.text_bg, &theme.root_color, 0.60),
        vocab_popup_dim = blend_colors(&theme.text_bg, &theme.root_color, 0.45),
        vocab_popup_border = blend_colors(&theme.text_bg, &theme.root_color, 0.25),
        font = font_family,
        size = font_size,
```

Add three new lines just before `font = font_family,`:

```rust
        focus_ring = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.4),
        picker_selection_bg = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.5),
        header_border = blend_colors(&theme.dim_fg, &theme.text_bg, 0.5),
```

The full replaced block now reads:

```rust
        root = theme.root_color,
        bg = theme.text_bg,
        fg = theme.text_fg,
        dim = theme.dim_fg,
        cursor_bg = theme.cursor_bg,
        cursor_fg = theme.cursor_fg,
        vocab = theme.vocab_fg,
        vocab_popup_fg = blend_colors(&theme.text_bg, &theme.root_color, 0.60),
        vocab_popup_dim = blend_colors(&theme.text_bg, &theme.root_color, 0.45),
        vocab_popup_border = blend_colors(&theme.text_bg, &theme.root_color, 0.25),
        focus_ring = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.4),
        picker_selection_bg = blend_colors(&theme.cursor_bg, &theme.text_bg, 0.5),
        header_border = blend_colors(&theme.dim_fg, &theme.text_bg, 0.5),
        font = font_family,
        size = font_size,
```

- [ ] **Step 3: Build and verify it compiles**

Run: `cargo build`
Expected: build succeeds. There may be unrelated warnings already present in the working tree; the new code must not add any new warnings.

If the build fails with `unused argument` for one of the three new variables, that means the CSS block in step 1 wasn't pasted correctly. Re-check that `{focus_ring}`, `{picker_selection_bg}`, and `{header_border}` all appear at least once in the CSS string.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all existing tests pass. There are no tests for `generate_css` itself; we are confirming nothing else broke.

- [ ] **Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "ui(theme): expand library-picker CSS with header, footer, tinted selection"
```

---

## Task 2: Add header and footer GtkBoxes to LibraryPicker struct

**Files:**
- Modify: `src/ui/library_picker.rs:70-129` (struct + `new()` constructor)

This task adds the new widgets and CSS classes but does **not** change behavior or text yet — header labels are empty strings and the footer is empty. Subsequent tasks fill them in. We split this way so each commit produces a buildable, runnable binary.

- [ ] **Step 1: Update the `LibraryPicker` struct fields**

Find lines 70-78:

```rust
pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    scrim: GtkBox,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
}
```

Replace with:

```rust
pub struct LibraryPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    header_box: GtkBox,
    header_title: Label,
    header_crumb: Label,
    search_entry: Entry,
    list_box: ListBox,
    footer_box: GtkBox,
    scrim: GtkBox,
    groups: Vec<AuthorGroup>,
    level: PickerLevel,
}
```

- [ ] **Step 2: Construct the header and footer in `new()` and assemble them**

Find the existing `new()` body (lines 83-129). Replace the existing assembly section so the final body reads:

```rust
    pub fn new() -> Self {
        let overlay = Overlay::new();

        // Scrim — sits between base content and the picker box.
        let scrim = GtkBox::builder()
            .hexpand(true)
            .vexpand(true)
            .build();
        scrim.add_css_class("library-picker-scrim");
        scrim.set_visible(false);

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(360)
            .height_request(280)
            .build();
        picker_box.add_css_class("library-picker");

        // Header: title (left) + crumb (right)
        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .build();
        header_box.add_css_class("library-picker-header");

        let header_title = Label::builder()
            .label("")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();
        header_title.add_css_class("library-picker-title");

        let header_crumb = Label::builder()
            .label("")
            .halign(gtk4::Align::End)
            .build();
        header_crumb.add_css_class("library-picker-crumb");

        header_box.append(&header_title);
        header_box.append(&header_crumb);

        let search_entry = Entry::builder()
            .placeholder_text("Filter authors...")
            .build();

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        // Footer: built empty for now; populated by update_footer().
        let footer_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .hexpand(true)
            .build();
        footer_box.add_css_class("library-picker-footer");

        picker_box.append(&header_box);
        picker_box.append(&search_entry);
        picker_box.append(&scrolled);
        picker_box.append(&footer_box);

        LibraryPicker {
            overlay,
            picker_box,
            header_box,
            header_title,
            header_crumb,
            search_entry,
            list_box,
            footer_box,
            scrim,
            groups: Vec::new(),
            level: PickerLevel::Authors,
        }
    }
```

Key changes vs the previous `new()`:

- `picker_box` `spacing` is now `0` (each region pads itself).
- `picker_box` `width_request`/`height_request` lowered to floor values `360`/`280`. The full responsive sizing hook is added in Task 5.
- The `header_box` and its two `Label` children are constructed and appended **before** the search entry.
- The `footer_box` is constructed empty and appended **after** the scrolled list.
- An unused-import warning will likely appear for `header_box` since we hold a reference to it but don't read it after construction yet — that's fine; Task 4 reads it via `self.footer_box`. (`header_box` is also held in the struct so the compiler is happy.)

- [ ] **Step 3: Build and verify it compiles**

Run: `cargo build`
Expected: build succeeds.

If the build fails complaining about `Label` not being in scope, confirm the existing `use gtk4::{...}` line (line 2-4) already includes `Label`. It does at the time of writing, so no `use` change should be necessary.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all tests pass. The struct grew; existing tests don't construct the struct so they're unaffected.

- [ ] **Step 5: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "ui(library-picker): add header and footer scaffolding to struct"
```

---

## Task 3: Implement `update_header()` and call it from level transitions

**Files:**
- Modify: `src/ui/library_picker.rs` — add a private `update_header()` method on `impl LibraryPicker`, and call it from `show_finish()`, `refresh_after_level_change()`, and `set_works()`.

After this task, the header text is correct.

- [ ] **Step 1: Add a unit test for header text computation**

We are about to write logic that computes a uppercase title and "<n> authors" / "<n> works" crumb. Extract that pure logic into a helper so we can test it. Add this test inside the existing `mod tests` block in `library_picker.rs` (currently around line 396):

```rust
    // ── Task 3 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_header_text_for_authors_level() {
        let groups = vec![
            AuthorGroup { author: "Shakespeare".into(), works: vec![] },
            AuthorGroup { author: "Austen".into(), works: vec![] },
        ];
        let (title, crumb) = header_text(&PickerLevel::Authors, &groups);
        assert_eq!(title, "LIBRARY — AUTHORS");
        assert_eq!(crumb, "2 AUTHORS");
    }

    #[test]
    fn test_header_text_for_works_level() {
        let groups = vec![
            AuthorGroup {
                author: "Shakespeare".into(),
                works: vec![
                    make_work("Ham", "Hamlet", "Shakespeare"),
                    make_work("Mac", "Macbeth", "Shakespeare"),
                ],
            },
        ];
        let level = PickerLevel::Works("Shakespeare".into());
        let (title, crumb) = header_text(&level, &groups);
        assert_eq!(title, "LIBRARY — SHAKESPEARE");
        assert_eq!(crumb, "2 WORKS");
    }

    #[test]
    fn test_header_text_for_works_level_unknown_author() {
        // Defensive: if the author name in PickerLevel doesn't match any group
        // (shouldn't happen via real flows), the crumb falls back to "0 WORKS".
        let groups: Vec<AuthorGroup> = vec![];
        let level = PickerLevel::Works("Nobody".into());
        let (title, crumb) = header_text(&level, &groups);
        assert_eq!(title, "LIBRARY — NOBODY");
        assert_eq!(crumb, "0 WORKS");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib library_picker::tests::test_header_text -- --nocapture`
Expected: FAIL with `cannot find function 'header_text' in this scope` (or similar — the function doesn't exist yet).

- [ ] **Step 3: Implement the `header_text` helper at file scope**

Add this function to `src/ui/library_picker.rs` near the bottom of the file, immediately above the `// ─── Tests ───────────────────────────────────────────────────────────────────` divider (around line 392):

```rust
/// Compute the (title, crumb) header text pair for the given picker level.
/// Title format: "LIBRARY — AUTHORS" or "LIBRARY — <AUTHOR NAME>".
/// Crumb format: "<n> AUTHORS" or "<n> WORKS".
/// Both strings are uppercase because GTK 4 CSS does not support text-transform.
pub(crate) fn header_text(level: &PickerLevel, groups: &[AuthorGroup]) -> (String, String) {
    match level {
        PickerLevel::Authors => (
            "LIBRARY — AUTHORS".to_string(),
            format!("{} AUTHORS", groups.len()),
        ),
        PickerLevel::Works(author) => {
            let count = groups
                .iter()
                .find(|g| &g.author == author)
                .map(|g| g.works.len())
                .unwrap_or(0);
            (
                format!("LIBRARY — {}", author.to_uppercase()),
                format!("{} WORKS", count),
            )
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib library_picker::tests::test_header_text -- --nocapture`
Expected: all three tests PASS.

Run all picker tests to confirm nothing else broke: `cargo test --lib library_picker`. Expected: PASS.

- [ ] **Step 5: Add `update_header()` method on `LibraryPicker`**

Add this method inside the `impl LibraryPicker` block (anywhere — convention says near the other private updater helpers, e.g. just before `populate_list`):

```rust
    fn update_header(&self) {
        let (title, crumb) = header_text(&self.level, &self.groups);
        self.header_title.set_text(&title);
        self.header_crumb.set_text(&crumb);
    }
```

- [ ] **Step 6: Call `update_header()` from existing entry points**

Update `set_works`, `show_finish`, and `refresh_after_level_change` to invoke `update_header`.

In `set_works` (currently lines 131-135):

```rust
    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.groups = group_works(&works);
        self.level = PickerLevel::Authors;
        self.update_header();
        self.populate_list("");
    }
```

In `show_finish` (currently lines 144-151):

```rust
    pub fn show_finish(&self) {
        self.picker_box.set_visible(true);
        self.scrim.set_visible(true);
        self.search_entry.set_placeholder_text(Some("Filter authors..."));
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.update_header();
        self.populate_list("");
    }
```

In `refresh_after_level_change` (currently lines 194-203):

```rust
    pub fn refresh_after_level_change(&self) {
        let placeholder = match &self.level {
            PickerLevel::Authors => "Filter authors...",
            PickerLevel::Works(_) => "Filter works...",
        };
        self.search_entry.set_placeholder_text(Some(placeholder));
        self.search_entry.set_text("");
        self.update_header();
        self.populate_list("");
        self.search_entry.grab_focus();
    }
```

- [ ] **Step 7: Build and run all tests**

Run: `cargo build && cargo test`
Expected: build succeeds, all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "ui(library-picker): populate header title and crumb on level change"
```

---

## Task 4: Implement `update_footer()` and call it from level transitions

**Files:**
- Modify: `src/ui/library_picker.rs` — add a `footer_hints()` helper, an `update_footer()` method, and call it from the same three entry points used in Task 3.

The footer always rebuilds its children when the level changes. Hints are passed as plain `Label` widgets so the small-caps CSS can style them.

- [ ] **Step 1: Add unit tests for footer hint computation**

Add to the same `mod tests` block:

```rust
    // ── Task 4 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_footer_hints_for_authors_level() {
        let hints = footer_hints(&PickerLevel::Authors);
        assert_eq!(hints, vec!["↑↓ MOVE", "↵ OPEN", "ESC CLOSE"]);
    }

    #[test]
    fn test_footer_hints_for_works_level() {
        let level = PickerLevel::Works("Shakespeare".into());
        let hints = footer_hints(&level);
        assert_eq!(hints, vec!["↑↓ MOVE", "↵ OPEN", "BACKSPACE BACK", "ESC CLOSE"]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib library_picker::tests::test_footer_hints -- --nocapture`
Expected: FAIL with `cannot find function 'footer_hints' in this scope`.

- [ ] **Step 3: Implement the `footer_hints` helper**

Add immediately below `header_text` in `src/ui/library_picker.rs`:

```rust
/// Footer hint strings for the given picker level, in display order.
/// Each entry is one segment; the renderer joins them with " · ".
pub(crate) fn footer_hints(level: &PickerLevel) -> Vec<&'static str> {
    match level {
        PickerLevel::Authors => vec!["↑↓ MOVE", "↵ OPEN", "ESC CLOSE"],
        PickerLevel::Works(_) => vec![
            "↑↓ MOVE",
            "↵ OPEN",
            "BACKSPACE BACK",
            "ESC CLOSE",
        ],
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib library_picker::tests::test_footer_hints -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Implement `update_footer()` on `LibraryPicker`**

Add inside `impl LibraryPicker`, near `update_header`:

```rust
    fn update_footer(&self) {
        // Remove existing children
        while let Some(child) = self.footer_box.first_child() {
            self.footer_box.remove(&child);
        }

        let hints = footer_hints(&self.level);
        for (i, hint) in hints.iter().enumerate() {
            if i > 0 {
                let sep = Label::builder().label(" · ").build();
                self.footer_box.append(&sep);
            }
            let label = Label::builder().label(*hint).build();
            self.footer_box.append(&label);
        }
    }
```

- [ ] **Step 6: Call `update_footer()` from the same entry points as `update_header()`**

Update `set_works`, `show_finish`, and `refresh_after_level_change`. Each gets one new line. Final shapes:

```rust
    pub fn set_works(&mut self, works: Vec<WorkSummary>) {
        self.groups = group_works(&works);
        self.level = PickerLevel::Authors;
        self.update_header();
        self.update_footer();
        self.populate_list("");
    }
```

```rust
    pub fn show_finish(&self) {
        self.picker_box.set_visible(true);
        self.scrim.set_visible(true);
        self.search_entry.set_placeholder_text(Some("Filter authors..."));
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.update_header();
        self.update_footer();
        self.populate_list("");
    }
```

```rust
    pub fn refresh_after_level_change(&self) {
        let placeholder = match &self.level {
            PickerLevel::Authors => "Filter authors...",
            PickerLevel::Works(_) => "Filter works...",
        };
        self.search_entry.set_placeholder_text(Some(placeholder));
        self.search_entry.set_text("");
        self.update_header();
        self.update_footer();
        self.populate_list("");
        self.search_entry.grab_focus();
    }
```

- [ ] **Step 7: Build and run all tests**

Run: `cargo build && cargo test`
Expected: build succeeds, all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "ui(library-picker): populate footer hints on level change"
```

---

## Task 5: Make picker box size responsive to window dimensions

**Files:**
- Modify: `src/ui/library_picker.rs` — extend `attach()` to install a resize handler that updates `picker_box` size request based on the toplevel window's allocated dimensions.

This task is the only place GTK signals get wired in. Sizing math lives in a pure helper that has unit tests; the wiring code is small and tested manually via `cargo build`.

- [ ] **Step 1: Add a unit test for the responsive sizing math**

Add to `mod tests`:

```rust
    // ── Task 5 tests ──────────────────────────────────────────────────────

    #[test]
    fn test_responsive_size_clamps_to_max() {
        // Big window — clamp at the 640x560 ceiling.
        let (w, h) = responsive_size(2400, 1600);
        assert_eq!(w, 640);
        assert_eq!(h, 560);
    }

    #[test]
    fn test_responsive_size_uses_floor() {
        // Tiny window — never go below 360x280.
        let (w, h) = responsive_size(200, 200);
        assert_eq!(w, 360);
        assert_eq!(h, 280);
    }

    #[test]
    fn test_responsive_size_scales_in_middle() {
        // 800x600 -> 60% width = 480, 70% height = 420.
        let (w, h) = responsive_size(800, 600);
        assert_eq!(w, 480);
        assert_eq!(h, 420);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib library_picker::tests::test_responsive_size -- --nocapture`
Expected: FAIL — `responsive_size` does not exist.

- [ ] **Step 3: Implement `responsive_size`**

Add immediately below `footer_hints`:

```rust
/// Compute the picker box size request from the toplevel window's allocated
/// dimensions. Returns (width, height) in pixels.
///
/// Width  = clamp(0.6 * window_width,  min = 360, max = 640)
/// Height = clamp(0.7 * window_height, min = 280, max = 560)
pub(crate) fn responsive_size(window_w: i32, window_h: i32) -> (i32, i32) {
    let w = (window_w as f32 * 0.6).round() as i32;
    let h = (window_h as f32 * 0.7).round() as i32;
    let w = w.clamp(360, 640);
    let h = h.clamp(280, 560);
    (w, h)
}
```

- [ ] **Step 4: Run to verify the tests pass**

Run: `cargo test --lib library_picker::tests::test_responsive_size -- --nocapture`
Expected: all three PASS.

- [ ] **Step 5: Wire up the resize handler in `attach()`**

Find the existing `attach` method (currently lines 162-167):

```rust
    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }
```

Replace with:

```rust
    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);

        // Responsive sizing: when the picker_box is realized, hook the
        // toplevel window's width/height notifications so we can update
        // its size request as the window resizes.
        let picker_box = self.picker_box.clone();
        self.picker_box.connect_realize(move |pb| {
            if let Some(window) = pb.root().and_then(|r| r.downcast::<gtk4::Window>().ok()) {
                let apply = {
                    let pb = picker_box.clone();
                    let window = window.clone();
                    move || {
                        let (w, h) = responsive_size(
                            window.default_width().max(window.width()),
                            window.default_height().max(window.height()),
                        );
                        pb.set_size_request(w, h);
                    }
                };
                // Apply once on realize so the floor isn't visible at startup.
                apply();
                // Update on every resize.
                let apply_for_w = apply.clone();
                window.connect_default_width_notify(move |_| apply_for_w());
                let apply_for_h = apply.clone();
                window.connect_default_height_notify(move |_| apply_for_h());
            }
        });
    }
```

The `move`/`clone` dance is necessary because both `picker_box` and `window` need to be moved into the inner closure, and the closure itself is moved into both `connect_default_width_notify` and `connect_default_height_notify`, so we clone it for each. `responsive_size` returns the clamped values; `set_size_request` accepts negative numbers as "no minimum" but we always pass the computed clamped value.

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: build succeeds. Possible compile errors and fixes:

- *"`Window` not found in `gtk4`"*: ensure the existing imports cover it; if not, add `use gtk4::Window;` near the top of the file. (Note: GTK4 also has `gtk4::ApplicationWindow` which is a subclass of `Window`, so downcasting to `Window` works.)
- *"closure captures `apply`"*: the snippet clones `apply` into named bindings before each `connect_*`; do not collapse them.
- *"`connect_default_width_notify` not found"*: this is a GLib property notify signal generated for the `default-width` property. If for some reason it's missing, fall back to `connect_property_default_width_notify`.

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/ui/library_picker.rs
git commit -m "ui(library-picker): responsive sizing within 360x280..640x560"
```

---

## Task 6: Manual visual verification

**Files:** none changed in this task — verification only.

The user runs the app themselves (CLAUDE.md: do not run `cargo run`). The implementation agent must NOT run `cargo run`. Instead, prepare the manual verification checklist for the user.

- [ ] **Step 1: Print the verification checklist for the user**

Output the following to the user, verbatim:

> Build succeeded. Please run `cargo run` and verify:
>
> 1. Press Ctrl+P. The header reads `LIBRARY — AUTHORS` on the left and `<n> AUTHORS` on the right.
> 2. The footer reads `↑↓ MOVE  ·  ↵ OPEN  ·  ESC CLOSE` (small caps, dim).
> 3. There's a hairline divider under the header and above the footer.
> 4. The picker has a soft drop shadow against the scrim.
> 5. Selection highlight is a muted pinkish-cream — NOT the full coral cursor color.
> 6. Type to filter — the search entry shows a subtle 3px focus ring around it.
> 7. Press ↵ on Shakespeare. The header switches to `LIBRARY — SHAKESPEARE` with `49 WORKS`. The footer adds `BACKSPACE BACK`.
> 8. Press Backspace — back to authors.
> 9. Resize the window from small to wide. Picker grows up to ~640×560 then stops, stays centered.
> 10. Press Esc — picker closes cleanly, no flicker.
>
> Anything off? Report it and I'll iterate.

---

## Self-Review

Spec coverage check:

- ✅ Header layout (Task 2 + Task 3) — small caps title left, count crumb right, divider beneath.
- ✅ Authors / Works level header text (Task 3, `header_text`).
- ✅ Footer hint row (Task 2 + Task 4).
- ✅ Authors / Works level footer text (Task 4, `footer_hints`).
- ✅ Responsive sizing 360×280…640×560 (Task 5, `responsive_size` + `connect_realize` hook).
- ✅ Picker box CSS: drop shadow, padding 0, hairline border (Task 1).
- ✅ Header CSS classes: `.library-picker-header`, `.library-picker-title`, `.library-picker-crumb` (Task 1 CSS, used in Task 2).
- ✅ Search CSS with focus ring using `focus_ring` blended variable (Task 1).
- ✅ List row CSS, tabular-nums, tinted selection background via `picker_selection_bg` (Task 1).
- ✅ Footer CSS class `.library-picker-footer` with top border (Task 1).
- ✅ `header_border` blended variable (Task 1).
- ✅ Header reflects current level/group, not filtered count — `update_header` only reads `self.level` and `self.groups`, never the filter (Task 3).
- ✅ Public API unchanged — only private methods added; `set_works`, `show_finish`, `refresh_after_level_change` are pre-existing and keep their signatures (Tasks 3 & 4).
- ✅ Manual verification covers all 8 spec verification items (Task 6 expands to 10 for clarity).

Out-of-scope items from the spec are honored: no other pickers touched.

Placeholder scan: every step contains exact code or exact commands. No "TBD" or "implement appropriately."

Type/name consistency: `header_text` (Task 3) and `footer_hints` / `responsive_size` (Tasks 4–5) names match between the test code and the implementation code. The struct fields `header_box`, `header_title`, `header_crumb`, `footer_box` introduced in Task 2 match the methods in Tasks 3–4. CSS classes referenced in `theme.rs` (Task 1) match the classes added by `add_css_class` calls in Task 2. The blended-variable names used in the format string (`focus_ring`, `picker_selection_bg`, `header_border`) match the format-args block.

One spec text-transform note: I removed `text-transform: uppercase` from CSS because GTK 4 does not implement it; instead the Rust code passes already-uppercased strings. This is documented in the CSS comment in Task 1 step 1 and again in the doc-comment of `header_text`.

# Two-column Translation Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a full-screen scrolling overlay (opened with `Alt+i`) that shows the current scene's dialogue as speaker-grouped two-column blocks — original verse on the left, modern translation on the right.

**Architecture:** A new `TranslationOverlay` GTK widget (modeled on `GlossOverlay`) sits in the existing overlay chain above the reading card. Its content is built from `current_work.lines` (the DB-backed `Vec<Line>`, which already carries `speaker` and `id`) and the existing `translations: HashMap<i64,String>`. A single shared `ScrolledWindow` holds an alternating sequence of full-width speaker-header rows and paired (original | translation) two-column blocks, so lockstep scrolling is automatic. A new `InputMode::TranslationOverlay` routes `j`/`k`/`Escape` to the overlay. The reading card never changes layout. The interlinear `i` feature is untouched.

**Tech Stack:** Rust, GTK4 (`gtk4` crate), sourceview5, existing linux-lit overlay/keymap infrastructure.

---

## Background facts (verified against the codebase)

These are the exact anchors the tasks below depend on. Read this before starting.

- **Overlay precedent:** `src/ui/gloss_overlay.rs` — `GlossOverlay` struct (`pub overlay: Overlay`, plus `scrim`, `container`, a `ScrolledWindow`, views). Its `attach(child)` (line 485) does `overlay.set_child(child)` then `add_overlay(&scrim)` + `add_overlay(&container)` with `set_measure_overlay(.., false)` and `set_clip_overlay(.., true)`. `show_synopsis` (line 774) sizes the container to `(card_width, card_height)`, fills content, shows scrim+container. `hide()` (line 1237) hides container+scrim. `scroll_gloss(delta)` (line 1192) steps `gloss_scrolled.vadjustment()` by `row_step()*3*delta`. `is_visible()` (line 1256).
- **Overlay chain in `src/app.rs`:** constructed ~lines 1229–1296. `gloss_overlay` is built at 1263 (`GlossOverlay::new(config.column_width, config.text_margins)`), attached at 1266 (`gloss_overlay.attach(&gamepad_overlay.overlay)`), then `gloss_picker.attach(&gloss_overlay.overlay)` at 1271. The `AppState` field is added in the struct construction list (`gloss_overlay,` at ~1559) and declared on the struct at line 230.
- **`InputMode` enum:** `src/app.rs:37`. Variants include `Reader`, `SynopsisOverlay`, `EchoesOverlay`, etc.
- **Keymap dispatch:** `src/input/keymap.rs:83-110` — when `input_mode != Reader`, a match routes each mode to a handler. `SynopsisOverlay => handle_synopsis_overlay_key(...)` (line 98). `handle_synopsis_overlay_key` is at line 741; it handles `Escape` (hide + set `Reader`), `j`/`k` (scroll), etc.
- **Action enum:** `src/input/actions/mod.rs` — variants at line 95+. `ShowEchoes` at line 98. `ShowSynopsisOverlay` at line 112. The `category()` match lists `Action::ShowEchoes` at 201 and `Action::ShowSynopsisOverlay` at 227 (both `Category::Display`). The `name()` match has `Action::ShowEchoes => "ShowEchoes"` at 315 and `Action::ShowSynopsisOverlay => "ShowSynopsisOverlay"` at 325.
- **Action dispatch:** `src/input/keymap.rs` — `ShowEchoes => ...show_echoes_for_cursor_line(...)` at 1383; `ShowSynopsisOverlay => crate::app::show_synopsis_overlay(state)` at 1531; `ToggleTranslations => {...}` at 1393.
- **Keybind defaults:** `src/input/keymap_config.rs:270` `(KeyCombo::plain("i"), Action::ToggleTranslations)`, line 297 `(KeyCombo::alt("i"), Action::ShowEchoes)`. `KeyCombo::alt(key)` helper at line 36.
- **Stowed JSON:** `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — line 39 `{"key": "i", "action": "ToggleTranslations"}`, line 42 `{"key": "i", "alt": true, "action": "ShowEchoes"}`.
- **`Line` struct:** `src/db/models.rs` — fields `id: i64`, `text: String`, `speaker: Option<String>`, `div1: i64`, `div2: i64`, `is_dialogue: bool`. `Work.lines: Vec<Line>`.
- **Translation data:** `AppState.translations: HashMap<i64,String>` keyed by `line.id` (the `line_mapping.id`). Loaded in `display_work` at `app.rs:2642`.
- **Cursor → scene:** `current_scene_divs(state) -> (i64,i64)` (`app.rs:4685`) returns the cursor line's `(div1,div2)`. `state.work_line_for_buffer(buffer_line) -> Option<usize>` (`app.rs:442`) maps a buffer line to a `work.lines` index.
- **Theme colors:** `state.theme.text_fg`, `state.theme.dim_fg`, `state.theme.text_bg` (`src/theme.rs:10-18`). No dedicated speaker color — reuse `text_fg` for headers.
- **Show-overlay precedent fn:** `show_synopsis_overlay` (`app.rs:4828`) reads `content_hbox.width()/height()`, calls the overlay's show method, then sets `input_mode = SynopsisOverlay`.

A pure helper that groups lines by speaker is the only unit-testable piece; everything else is GTK wiring verified visually by the user.

---

## File Structure

- **Create** `src/ui/translation_overlay.rs` — the `TranslationOverlay` widget + a pure `group_scene_into_blocks` helper and its `TranslationBlock` type.
- **Modify** `src/ui/mod.rs` — register the new module.
- **Modify** `src/app.rs` — add the `translation_overlay` field, construct + attach it, add `show_translation_overlay()`.
- **Modify** `src/app.rs` (`InputMode`) — add `TranslationOverlay` variant.
- **Modify** `src/input/actions/mod.rs` — add `ShowTranslationOverlay` action (enum + `category()` + `name()`).
- **Modify** `src/input/keymap.rs` — dispatch the action; route the new InputMode to a new `handle_translation_overlay_key`.
- **Modify** `src/input/keymap_config.rs` — `Alt+i` → `ShowTranslationOverlay`; `Alt+e` → `ShowEchoes`.
- **Modify** `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — same keybind swap.

---

## Task 1: Pure speaker-grouping helper + type

This is the only pure-logic unit. It takes a scene's lines and produces an ordered list of blocks (speaker speeches and non-spoken interludes) with their source index ranges, so the overlay can render blocks and find the cursor's block for the scroll anchor.

**Files:**
- Create: `src/ui/translation_overlay.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Register the module**

In `src/ui/mod.rs`, add the module declaration alphabetically near the other `pub mod` lines (e.g. after `pub mod settings_overlay;` or wherever `pub mod` entries live):

```rust
pub mod translation_overlay;
```

- [ ] **Step 2: Write the failing test**

Create `src/ui/translation_overlay.rs` with the type, a stub helper, and tests:

```rust
use crate::db::models::Line;

/// One render unit in the translation overlay: either a speaker's speech
/// (with original + translation paired per line) or a non-spoken interlude
/// (stage direction / scene header, shown full-width with no translation).
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationBlock {
    /// Speaker label for a speech block; `None` for a non-spoken interlude.
    pub speaker: Option<String>,
    /// (original_text, translation_or_empty) per source line, in order.
    pub lines: Vec<(String, String)>,
    /// Inclusive range of `work.lines` indices this block covers.
    pub start_idx: usize,
    pub end_idx: usize,
}

/// Group a slice of scene lines into ordered blocks. Consecutive lines that
/// share the same `speaker` form one speech block; runs of `speaker == None`
/// lines (stage directions, scene headers) form non-spoken interlude blocks.
/// `idx_of(i)` maps the i-th element of `lines` back to its `work.lines` index;
/// `translation_of(line_id)` returns the modern translation if one exists.
pub fn group_scene_into_blocks(
    lines: &[Line],
    idx_of: impl Fn(usize) -> usize,
    translation_of: impl Fn(i64) -> Option<String>,
) -> Vec<TranslationBlock> {
    let _ = (lines, &idx_of, &translation_of);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::Line;

    fn mk(id: i64, text: &str, speaker: Option<&str>) -> Line {
        Line {
            id,
            citation: String::new(),
            text: text.to_string(),
            normalized: String::new(),
            speaker: speaker.map(|s| s.to_string()),
            is_dialogue: speaker.is_some(),
            timestamp: None,
            div1: 1,
            div2: 1,
            line_in_div: 0,
            is_chapter: false,
            is_spoken: None,
        }
    }

    #[test]
    fn groups_consecutive_speaker_lines_into_one_block() {
        let lines = vec![
            mk(10, "She shall be to the happiness of England", Some("CRANMER")),
            mk(11, "An aged princess; many days shall see her,", Some("CRANMER")),
            mk(12, "O lord", Some("KING")),
        ];
        let trans = |id: i64| match id {
            10 => Some("She shall be to England's happiness".to_string()),
            11 => Some("An aged princess; many days will see her,".to_string()),
            12 => Some("O lord".to_string()),
            _ => None,
        };
        let blocks = group_scene_into_blocks(&lines, |i| i, trans);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker.as_deref(), Some("CRANMER"));
        assert_eq!(blocks[0].lines.len(), 2);
        assert_eq!(blocks[0].lines[0].0, "She shall be to the happiness of England");
        assert_eq!(blocks[0].lines[0].1, "She shall be to England's happiness");
        assert_eq!(blocks[0].start_idx, 0);
        assert_eq!(blocks[0].end_idx, 1);
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
        assert_eq!(blocks[1].start_idx, 2);
        assert_eq!(blocks[1].end_idx, 2);
    }

    #[test]
    fn non_spoken_lines_form_their_own_block_with_blank_translation() {
        let lines = vec![
            mk(20, "Enter KING and CRANMER", None),
            mk(21, "Thou speakest wonders.", Some("KING")),
        ];
        let blocks = group_scene_into_blocks(&lines, |i| i, |_| None);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker, None);
        assert_eq!(blocks[0].lines[0].0, "Enter KING and CRANMER");
        assert_eq!(blocks[0].lines[0].1, "");
        assert_eq!(blocks[1].speaker.as_deref(), Some("KING"));
    }

    #[test]
    fn idx_of_maps_back_to_work_indices() {
        let lines = vec![
            mk(30, "first", Some("A")),
            mk(31, "second", Some("A")),
        ];
        // Scene started at work index 100.
        let blocks = group_scene_into_blocks(&lines, |i| 100 + i, |_| None);
        assert_eq!(blocks[0].start_idx, 100);
        assert_eq!(blocks[0].end_idx, 101);
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --bins translation_overlay::tests -- --nocapture`
Expected: the three tests FAIL (assertions on an empty `Vec`).

- [ ] **Step 4: Implement `group_scene_into_blocks`**

Replace the stub body with:

```rust
pub fn group_scene_into_blocks(
    lines: &[Line],
    idx_of: impl Fn(usize) -> usize,
    translation_of: impl Fn(i64) -> Option<String>,
) -> Vec<TranslationBlock> {
    let mut blocks: Vec<TranslationBlock> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let work_idx = idx_of(i);
        let translation = if line.speaker.is_some() {
            translation_of(line.id).unwrap_or_default()
        } else {
            String::new()
        };

        // Extend the current block if this line continues the same speaker
        // (including a run of None == None for interludes); else start a new one.
        let same_as_prev = blocks
            .last()
            .map(|b| b.speaker == line.speaker)
            .unwrap_or(false);

        if same_as_prev {
            let b = blocks.last_mut().unwrap();
            b.lines.push((line.text.clone(), translation));
            b.end_idx = work_idx;
        } else {
            blocks.push(TranslationBlock {
                speaker: line.speaker.clone(),
                lines: vec![(line.text.clone(), translation)],
                start_idx: work_idx,
                end_idx: work_idx,
            });
        }
    }

    blocks
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --bins translation_overlay::tests -- --nocapture`
Expected: all three PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/translation_overlay.rs src/ui/mod.rs
git commit -m "feat(translation): speaker-grouping helper for two-column overlay"
```

---

## Task 2: The `TranslationOverlay` widget

Build the GTK widget: scrim + centered container holding one `ScrolledWindow` whose child is a vertical box of full-width speaker headers and paired two-column blocks. Provide `new`, `attach`, `show`, `hide`, `is_visible`, `scroll`, and a `scroll_to_block` used for the cursor anchor.

**Files:**
- Modify: `src/ui/translation_overlay.rs`

- [ ] **Step 1: Add imports and the struct**

At the top of `src/ui/translation_overlay.rs`, add:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Label, Orientation, Overlay, ScrolledWindow, TextView};
use std::cell::RefCell;
```

Then add the struct (below the existing `TranslationBlock`):

```rust
pub struct TranslationOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,
    /// Scroll viewport shared by both columns (one scrollbar == lockstep).
    scrolled: ScrolledWindow,
    /// Vertical stack of header rows + paired column blocks, inside `scrolled`.
    content_vbox: gtk4::Box,
    /// Per rendered speech/interlude block: the (start_idx, end_idx) source
    /// range and the block's top widget, so we can scroll to the cursor block.
    block_widgets: RefCell<Vec<(usize, usize, gtk4::Box)>>,
}
```

- [ ] **Step 2: Implement `new` and `attach`**

```rust
impl TranslationOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(Orientation::Vertical, 0);
        scrim.add_css_class("gloss-scrim");
        scrim.set_visible(false);

        let container = gtk4::Box::new(Orientation::Vertical, 0);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.add_css_class("gloss-overlay");
        container.set_visible(false);

        let title = Label::new(Some("Translation"));
        title.add_css_class("gloss-title");
        title.set_halign(Align::Start);
        title.set_margin_start(24);
        title.set_margin_top(24);
        title.set_margin_bottom(8);
        container.append(&title);

        let content_vbox = gtk4::Box::new(Orientation::Vertical, 0);
        content_vbox.set_hexpand(true);

        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled.set_propagate_natural_height(false);
        scrolled.set_vexpand(true);
        scrolled.set_hexpand(true);
        scrolled.set_margin_bottom(20);
        scrolled.set_child(Some(&content_vbox));
        container.append(&scrolled);

        Self {
            overlay,
            scrim,
            container,
            title,
            scrolled,
            content_vbox,
            block_widgets: RefCell::new(Vec::new()),
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.container);
        self.overlay.set_measure_overlay(&self.scrim, false);
        self.overlay.set_measure_overlay(&self.container, false);
        self.overlay.set_clip_overlay(&self.scrim, true);
        self.overlay.set_clip_overlay(&self.container, true);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
    }
}
```

- [ ] **Step 3: Implement `show` (build the blocks)**

Add to the `impl` block. `show` clears the previous content, sizes the card, builds one header row + one two-column block per `TranslationBlock`, and records block ranges. `text_fg`/`dim_fg` are passed in from the caller (the overlay has no theme of its own).

```rust
    /// Populate and reveal the overlay. `blocks` come from
    /// `group_scene_into_blocks`. `text_fg`/`dim_fg` are theme colors.
    pub fn show(
        &self,
        title: &str,
        blocks: &[TranslationBlock],
        card_width: i32,
        card_height: i32,
        text_fg: &str,
        dim_fg: &str,
    ) {
        self.container.set_width_request(card_width);
        self.container.set_height_request(card_height);
        self.title.set_text(title);

        // Clear any previous render.
        while let Some(child) = self.content_vbox.first_child() {
            self.content_vbox.remove(&child);
        }
        self.block_widgets.borrow_mut().clear();

        let side_margin = card_width / 12;
        let col_width = ((card_width - 2 * side_margin) / 2 - 12).max(120);

        for block in blocks {
            let block_box = gtk4::Box::new(Orientation::Vertical, 0);
            block_box.set_margin_start(side_margin);
            block_box.set_margin_end(side_margin);
            block_box.set_margin_top(14);

            if let Some(speaker) = &block.speaker {
                // Full-width speaker header.
                let header = Label::new(Some(speaker));
                header.set_halign(Align::Start);
                header.set_markup(&format!(
                    "<span foreground='{}' size='smaller' font_variant='small-caps' letter_spacing='1024'>{}</span>",
                    text_fg,
                    glib_escape(speaker),
                ));
                header.set_margin_bottom(4);
                block_box.append(&header);

                // Two-column paired text.
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
            } else {
                // Non-spoken interlude: full-width italic, no translation column.
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
            }

            self.content_vbox.append(&block_box);
            self.block_widgets
                .borrow_mut()
                .push((block.start_idx, block.end_idx, block_box));
        }

        self.scrim.set_visible(true);
        self.container.set_visible(true);
        self.scroll_to_top();
    }
```

Add these free helpers at the bottom of the file (outside `impl`):

```rust
fn make_column(width: i32, color: &str, italic: bool) -> TextView {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_focusable(false);
    view.set_wrap_mode(gtk4::WrapMode::WordChar);
    view.set_size_request(width, -1);
    view.add_css_class("gloss-text");
    if italic {
        view.add_css_class("translation-col");
    }
    // Color via an inline CSS provider would be heavier; rely on the
    // .gloss-text / .translation-col classes for base style and let the
    // theme's text color show. `color` reserved for a future inline tag.
    let _ = (color, italic);
    view
}

fn glib_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
```

- [ ] **Step 4: Implement scroll + cursor anchor**

```rust
    pub fn scroll(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let step = adj.page_size() * 0.15;
        let max = (adj.upper() - adj.page_size()).max(adj.lower());
        let target = (adj.value() + step * 3.0 * delta as f64)
            .clamp(adj.lower(), max);
        adj.set_value(target);
    }

    pub fn scroll_to_top(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
    }

    /// Scroll so the block whose source range contains `work_idx` sits at the
    /// top of the viewport. No-op if no block matches.
    pub fn scroll_to_block(&self, work_idx: usize) {
        let target = self.block_widgets.borrow().iter().find_map(|(s, e, w)| {
            if work_idx >= *s && work_idx <= *e {
                Some(w.clone())
            } else {
                None
            }
        });
        let Some(widget) = target else { return };
        // Defer one tick so allocations are settled before measuring.
        // `compute_point` maps the block's top-left (0,0) into the
        // content_vbox's coordinate space; that y IS the scroll offset that
        // brings the block to the viewport top (content_vbox is the scrolled
        // child, so its origin is the scroll's value==lower position).
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

- [ ] **Step 5: Build to verify it compiles**

Run: `cargo build`
Expected: compiles clean (warnings about the unused `color` param are acceptable; the `_ = (color, italic);` silences them).

- [ ] **Step 6: Run the unit tests again (no regressions)**

Run: `cargo test --bins translation_overlay::tests`
Expected: all PASS (the widget code is not exercised by these pure tests).

- [ ] **Step 7: Commit**

```bash
git add src/ui/translation_overlay.rs
git commit -m "feat(translation): TranslationOverlay widget (scrim, paired columns, scroll)"
```

---

## Task 3: Wire the overlay into AppState + construct it

Add the `InputMode` variant, the `AppState` field, and construct/attach the overlay in the chain.

**Files:**
- Modify: `src/app.rs:37` (InputMode), `src/app.rs:230` (struct field), `src/app.rs:~1266` (construct/attach), `src/app.rs:~1559` (struct literal)

- [ ] **Step 1: Add the InputMode variant**

In `src/app.rs`, in the `InputMode` enum (line 37), add after `SynopsisOverlay,`:

```rust
    TranslationOverlay,
```

- [ ] **Step 2: Add the AppState field declaration**

In the `AppState` struct, after the `pub gloss_overlay: ...` line (line 230), add:

```rust
    pub translation_overlay: crate::ui::translation_overlay::TranslationOverlay,
```

- [ ] **Step 3: Construct + attach in the overlay chain**

In `src/app.rs`, find the block (around line 1263–1271):

```rust
    let gloss_overlay = crate::ui::gloss_overlay::GlossOverlay::new(config.column_width, config.text_margins);
    gloss_overlay.attach(&gamepad_overlay.overlay);
    gloss_overlay.overlay.set_vexpand(true);
```
…followed by `let gloss_picker = ...; gloss_picker.attach(&gloss_overlay.overlay);`.

Insert the new overlay BETWEEN `gloss_overlay` and `gloss_picker` so it wraps `gloss_overlay.overlay` and `gloss_picker` then wraps it. Replace the `gloss_picker.attach(&gloss_overlay.overlay);` line so it attaches to the new overlay instead:

```rust
    let translation_overlay = crate::ui::translation_overlay::TranslationOverlay::new();
    translation_overlay.attach(&gloss_overlay.overlay);
    translation_overlay.overlay.set_vexpand(true);
```

Then change the following `gloss_picker.attach(...)` call (originally `gloss_picker.attach(&gloss_overlay.overlay);` at line 1271) to:

```rust
    gloss_picker.attach(&translation_overlay.overlay);
```

- [ ] **Step 4: Add the field to the AppState struct literal**

In the struct construction (the long list around line 1559 that contains `gloss_overlay,`), add:

```rust
        translation_overlay,
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: compiles. If the keymap match in `keymap.rs:83-110` now errors with "non-exhaustive patterns" for `InputMode::TranslationOverlay`, that is expected and fixed in Task 5 — but to keep this task green, proceed to Step 6 only after Task 5 if the build fails here. (Rust match on `InputMode` in `keymap.rs` has an explicit arm list, so adding a variant forces an arm. To keep commits green, do Task 5 Step 1 now if the build complains.)

Note: the `dispatch_action` `match mode` at `keymap.rs:84` lists arms explicitly and ends with `InputMode::Reader => unreachable!()`, so a new variant requires a new arm. Add a temporary arm to keep this task compiling:

In `src/input/keymap.rs`, in the `match mode` block (after the `SynopsisOverlay` arm at line 98), add:

```rust
            crate::app::InputMode::TranslationOverlay => handle_translation_overlay_key(state, key_name),
```

…and add a minimal stub handler near `handle_synopsis_overlay_key` (line 741) to satisfy the compiler (fleshed out in Task 5):

```rust
fn handle_translation_overlay_key(_state: &Rc<RefCell<AppState>>, _key_name: &str) -> bool {
    true
}
```

- [ ] **Step 6: Build again**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/input/keymap.rs
git commit -m "feat(translation): wire TranslationOverlay into AppState + overlay chain"
```

---

## Task 4: `show_translation_overlay` open function

Build the blocks from the current scene and reveal the overlay, anchored at the cursor's speaker block.

**Files:**
- Modify: `src/app.rs` (add `show_translation_overlay`, near `show_synopsis_overlay` at line 4828)

- [ ] **Step 1: Add the open function**

In `src/app.rs`, add after `show_synopsis_overlay` (ends ~line 4870):

```rust
/// Open the two-column speaker-grouped translation overlay for the current
/// scene, scrolled to the speaker block containing the cursor line.
pub fn show_translation_overlay(state: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let s = state.borrow();

    // Toggle off if already open.
    if s.translation_overlay.is_visible() {
        drop(s);
        let mut s = state.borrow_mut();
        s.translation_overlay.hide();
        s.input_mode = InputMode::Reader;
        return;
    }

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return,
    };

    let (div1, div2) = current_scene_divs(&s);

    // Collect this scene's lines (preserving order) with their work indices.
    let scene_lines: Vec<crate::db::models::Line> = work
        .lines
        .iter()
        .filter(|l| l.div1 == div1 && l.div2 == div2)
        .cloned()
        .collect();
    if scene_lines.is_empty() {
        return;
    }
    // Index of the first scene line within work.lines, for idx_of mapping.
    let base = work
        .lines
        .iter()
        .position(|l| l.div1 == div1 && l.div2 == div2)
        .unwrap_or(0);

    let translations = s.translations.clone();
    let blocks = crate::ui::translation_overlay::group_scene_into_blocks(
        &scene_lines,
        |i| base + i,
        |id| translations.get(&id).cloned(),
    );

    let card_width = s.content_hbox.width();
    let card_height = s.content_hbox.height();
    let text_fg = s.theme.text_fg.clone();
    let dim_fg = s.theme.dim_fg.clone();
    let label = synopsis_label(&s, div1, div2);

    // Cursor's work index, to pick the block to anchor on.
    let cursor_idx = s.work_line_for_buffer(s.current_line);

    s.translation_overlay.show(
        &label,
        &blocks,
        card_width,
        card_height,
        &text_fg,
        &dim_fg,
    );
    if let Some(idx) = cursor_idx {
        s.translation_overlay.scroll_to_block(idx);
    }
    drop(s);

    let mut s = state.borrow_mut();
    s.input_mode = InputMode::TranslationOverlay;
}
```

Note: `synopsis_label(&s, div1, div2)` is the existing scene-label helper used by `show_synopsis_overlay` (`app.rs:4864`). If it is not `pub`/visible here, reuse `current_scene_divs` + a plain `format!("Act {}, Scene {}", div1, div2)`; confirm by grep before choosing.

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles. If `synopsis_label` is private or has a different signature, replace the `let label = ...` line with:

```rust
    let label = format!("Act {div1}, Scene {div2}");
```

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(translation): show_translation_overlay builds scene blocks at cursor"
```

---

## Task 5: Action + dispatch + key handler

Add the `ShowTranslationOverlay` action, dispatch it to `show_translation_overlay`, and flesh out the overlay key handler (`j`/`k` scroll, `Escape` close).

**Files:**
- Modify: `src/input/actions/mod.rs` (enum + `category()` + `name()`)
- Modify: `src/input/keymap.rs` (dispatch arm + handler body)

- [ ] **Step 1: Add the action variant**

In `src/input/actions/mod.rs`, after `ShowSynopsisOverlay,` (line 112):

```rust
    ShowTranslationOverlay,
```

- [ ] **Step 2: Add it to `category()`**

In the `category()` match, alongside `Action::ShowSynopsisOverlay` (line 227, a `Category::Display` group), add to that arm's `|` chain:

```rust
            | Action::ShowTranslationOverlay
```

- [ ] **Step 3: Add it to `name()`**

In the `name()` match, after `Action::ShowSynopsisOverlay => "ShowSynopsisOverlay",` (line 325):

```rust
            Action::ShowTranslationOverlay => "ShowTranslationOverlay",
```

- [ ] **Step 4: Dispatch the action**

In `src/input/keymap.rs`, after the `ShowSynopsisOverlay => crate::app::show_synopsis_overlay(state),` arm (line 1531):

```rust
        ShowTranslationOverlay => crate::app::show_translation_overlay(state),
```

- [ ] **Step 5: Flesh out the key handler**

Replace the stub `handle_translation_overlay_key` (added in Task 3 Step 5) with:

```rust
fn handle_translation_overlay_key(state: &Rc<RefCell<AppState>>, key_name: &str) -> bool {
    match key_name {
        "Escape" => {
            let mut s = state.borrow_mut();
            s.translation_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        // Alt+i pressed again while open also closes (the action toggles).
        "j" => {
            state.borrow().translation_overlay.scroll(1);
            true
        }
        "k" => {
            state.borrow().translation_overlay.scroll(-1);
            true
        }
        // Swallow everything else so stray keys don't leak to the reader.
        _ => true,
    }
}
```

- [ ] **Step 6: Build + test**

Run: `cargo build && cargo test --bins`
Expected: compiles; all `--bins` tests PASS (including Task 1's three tests).

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap.rs
git commit -m "feat(translation): ShowTranslationOverlay action + key handler (j/k/Esc)"
```

---

## Task 6: Keybind swap (Alt+i → overlay, Alt+e → echoes)

Rebind in both the compiled defaults and the stowed JSON (JSON takes precedence — both must change).

**Files:**
- Modify: `src/input/keymap_config.rs:297`
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json:42`

- [ ] **Step 1: Edit compiled defaults**

In `src/input/keymap_config.rs`, change line 297 from:

```rust
        (KeyCombo::alt("i"), Action::ShowEchoes),
```
to two lines:

```rust
        (KeyCombo::alt("i"), Action::ShowTranslationOverlay),
        (KeyCombo::alt("e"), Action::ShowEchoes),
```

- [ ] **Step 2: Edit the stowed JSON**

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, change line 42 from:

```json
    {"key": "i", "alt": true, "action": "ShowEchoes"},
```
to:

```json
    {"key": "i", "alt": true, "action": "ShowTranslationOverlay"},
    {"key": "e", "alt": true, "action": "ShowEchoes"},
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles.

- [ ] **Step 4: Deploy the stow package**

Run:

```bash
cd ~/tty-dotfiles && stow -R linux-lit
```
Expected: no conflict output (the symlink already exists; `-R` restows).

- [ ] **Step 5: Commit (both repos)**

```bash
cd ~/utono/linux-lit && git add src/input/keymap_config.rs && \
  git commit -m "feat(translation): Alt+i opens translation overlay; Alt+e -> echoes"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && \
  git commit -m "feat(linux-lit): Alt+i translation overlay; Alt+e echoes"
```

---

## Task 7: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full build + pure-logic suite**

Run: `cargo build && cargo test --bins`
Expected: clean build; all `--bins` tests PASS.

- [ ] **Step 2: Clippy**

Run: `cargo clippy 2>&1 | rg -i "warning|error" | head`
Expected: no NEW warnings from `translation_overlay.rs` or the edited files (pre-existing warnings elsewhere are out of scope).

- [ ] **Step 3: Ask the user to verify on screen**

Per CLAUDE.md, this change's acceptance criterion is visual and the agent cannot reliably launch cage on the live session. Ask the user to run, with a work that has translations (e.g. H8):

```bash
LINUX_LIT_WORK=H8 cargo run
```

and confirm by eye:
- press `Alt+i` → overlay opens; two columns (original left, translation right), equal-ish halves, speaker headers full-width above each block
- it opens scrolled to the cursor's speaker block
- `j`/`k` scroll both columns together (lockstep); `Escape` returns to the reading card unchanged
- plain `i` still toggles the interlinear under-line translations independently
- `Alt+e` now opens echoes; `Alt+i` no longer opens echoes

Do not claim the feature verified until the user confirms the screenshot/behavior.

---

## Self-review notes

- **Spec coverage:** §1 widget → Task 2/3; §2 content model + block ranges → Task 1 + Task 4; §3 open/scroll/close → Task 4 + Task 5; §4 keybind swap → Task 6. Scope/non-goals respected (no DB/LineMap/pagination changes; interlinear untouched). Verification → Task 7.
- **Type consistency:** `TranslationBlock`, `group_scene_into_blocks`, `TranslationOverlay::{new,attach,show,hide,is_visible,scroll,scroll_to_top,scroll_to_block}`, `show_translation_overlay`, `handle_translation_overlay_key`, `Action::ShowTranslationOverlay`, `InputMode::TranslationOverlay` — used consistently across tasks.
- **Known soft spots flagged inline:** `synopsis_label` visibility (Task 4 fallback given); the non-exhaustive `match mode` requiring an arm before the handler exists (handled in Task 3 Step 5 with a stub, fleshed out in Task 5); inline per-column text color is left to CSS classes (`color` param reserved) to avoid per-view CSS providers — acceptable since `dim_fg` styling already exists via `.gloss-text` and can be refined visually in Task 7 if the translation column needs to read dimmer.

# Page Transition System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add selectable page transition styles (Crossfade, Slide, Instant) with a settings popup option to choose between them.

**Architecture:** Snapshot-based transitions using GTK4's `WidgetPaintable` to capture the old page, then animating between the snapshot and the new content. The transition style is stored in config and selectable from the existing settings overlay.

**Tech Stack:** Rust, GTK4 (v4_12 feature), gtk4::WidgetPaintable, gtk4::Picture, add_tick_callback

---

### Task 1: Add TransitionStyle enum and config field

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add TransitionStyle enum**

Add this enum after the existing `NavigationMode` enum (after line 13 in `src/config.rs`):

```rust
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransitionStyle {
    #[default]
    Crossfade,
    Slide,
    Instant,
}
```

- [ ] **Step 2: Add transition_style field to Config struct**

Add this field to the `Config` struct, after the `navigation_mode` field (after line 34):

```rust
    #[serde(default)]
    pub transition_style: TransitionStyle,
```

- [ ] **Step 3: Add transition_style to Default impl**

In the `Default` impl for `Config` (around line 111), add after `navigation_mode`:

```rust
            transition_style: TransitionStyle::default(),
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors. Warning about unused `TransitionStyle` is expected at this stage.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "Add TransitionStyle enum (Crossfade, Slide, Instant) to config"
```

---

### Task 2: Add PageDirection enum and update set_page signature

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add PageDirection enum**

Add this enum just above the `CROSSFADE_MS` constant (before line 588 in `src/input/navigation.rs`):

```rust
/// Direction of a page turn, used by the Slide transition.
#[derive(Clone, Copy)]
enum PageDirection {
    Forward,
    Backward,
}
```

- [ ] **Step 2: Add direction parameter to set_page**

Change the `set_page` function signature from:

```rust
fn set_page(state: &mut AppState, new_top: usize) {
```

to:

```rust
fn set_page(state: &mut AppState, new_top: usize, direction: PageDirection) {
```

The body stays the same for now — direction is unused until Task 4.

- [ ] **Step 3: Update all set_page callers with direction**

There are 14 call sites in `navigation.rs`. Update each one:

**Line ~66** (in `move_cursor`, cursor moved above page top → backward):
```rust
                    set_page(state, new_top, PageDirection::Backward);
```

**Line ~71** (in `move_cursor`, cursor moved below page bottom → forward):
```rust
                        set_page(state, next, PageDirection::Forward);
```

**Line ~73** (in `move_cursor`, at last line → forward):
```rust
                        set_page(state, state.current_line, PageDirection::Forward);
```

**Line ~140** (in `page_forward` → forward):
```rust
    set_page(state, new_top, PageDirection::Forward);
```

**Line ~158** (in `page_backward` → backward):
```rust
    set_page(state, new_top, PageDirection::Backward);
```

**Line ~413** (in search jump, direction depends on target vs current):
```rust
                    let dir = if line_idx >= state.page_top_line {
                        PageDirection::Forward
                    } else {
                        PageDirection::Backward
                    };
                    set_page(state, line_idx, dir);
```

**Line ~495** (in `scroll_after_jump_forward` → forward):
```rust
                    set_page(state, new_top, PageDirection::Forward);
```

**Line ~497** (in `scroll_after_jump_forward`, fallback → forward):
```rust
                    set_page(state, prev_line, PageDirection::Forward);
```

**Line ~513** (in `scroll_after_jump_backward` → backward):
```rust
                set_page(state, new_top, PageDirection::Backward);
```

**Line ~741** (paragraph scroll in e-reader mode, direction depends on target):
```rust
                let dir = if para_start >= state.page_top_line {
                    PageDirection::Forward
                } else {
                    PageDirection::Backward
                };
                set_page(state, para_start, dir);
```

**Line ~758** (in `update_highlight_and_ensure_visible`, direction depends on current line):
```rust
                let dir = if state.current_line >= state.page_top_line {
                    PageDirection::Forward
                } else {
                    PageDirection::Backward
                };
                set_page(state, state.current_line, dir);
```

**Line ~984** (in `jump_to_next_vocab` → forward):
```rust
                set_page(state, target_line, PageDirection::Forward);
```

**Line ~1004** (in `jump_to_vocab_at`, direction depends on target):
```rust
                let dir = if target_line >= state.page_top_line {
                    PageDirection::Forward
                } else {
                    PageDirection::Backward
                };
                set_page(state, target_line, dir);
```

- [ ] **Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles. Warning about unused `direction` parameter is expected.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Add PageDirection enum, thread direction through all set_page callers"
```

---

### Task 3: Implement snapshot-based crossfade transition

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Rewrite set_page to use WidgetPaintable snapshot for crossfade**

Replace the entire `set_page` function body with:

```rust
fn set_page(state: &mut AppState, new_top: usize, direction: PageDirection) {
    log_fmt!(
        "PAGE_TURN: new_top={} old_top={} current_line={} transition={:?}",
        new_top, state.page_top_line, state.current_line, state.config.transition_style
    );

    match state.config.transition_style {
        crate::config::TransitionStyle::Instant => {
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
        }
        crate::config::TransitionStyle::Crossfade => {
            // Capture snapshot of current page
            let paintable = gtk4::WidgetPaintable::new(Some(&state.card_vbox));
            let snapshot_pic = gtk4::Picture::for_paintable(&paintable);
            snapshot_pic.set_can_shrink(false);
            snapshot_pic.set_keep_aspect_ratio(false);
            state.page_turn_overlay.add_overlay(&snapshot_pic);

            // Scroll to new position (hidden behind snapshot)
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);

            // Animate crossfade: snapshot fades out, live content fades in
            let card_vbox = state.card_vbox.clone();
            let overlay = state.page_turn_overlay.clone();
            let start_time = std::cell::Cell::new(None::<f64>);
            state.card_vbox.add_tick_callback(move |_widget, clock| {
                let now = clock.frame_time() as f64 / 1_000.0;
                let t0 = match start_time.get() {
                    Some(t) => t,
                    None => {
                        start_time.set(Some(now));
                        now
                    }
                };
                let elapsed = now - t0;
                let progress = (elapsed / CROSSFADE_MS).min(1.0);
                card_vbox.set_opacity(progress);
                snapshot_pic.set_opacity(1.0 - progress);

                if progress >= 1.0 {
                    card_vbox.set_opacity(1.0);
                    overlay.remove_overlay(&snapshot_pic);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }
        crate::config::TransitionStyle::Slide => {
            // Slide transition — implemented in Task 4
            // For now, fall back to crossfade behavior
            let paintable = gtk4::WidgetPaintable::new(Some(&state.card_vbox));
            let snapshot_pic = gtk4::Picture::for_paintable(&paintable);
            snapshot_pic.set_can_shrink(false);
            snapshot_pic.set_keep_aspect_ratio(false);
            state.page_turn_overlay.add_overlay(&snapshot_pic);

            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);

            let card_vbox = state.card_vbox.clone();
            let overlay = state.page_turn_overlay.clone();
            let start_time = std::cell::Cell::new(None::<f64>);
            state.card_vbox.add_tick_callback(move |_widget, clock| {
                let now = clock.frame_time() as f64 / 1_000.0;
                let t0 = match start_time.get() {
                    Some(t) => t,
                    None => {
                        start_time.set(Some(now));
                        now
                    }
                };
                let elapsed = now - t0;
                let progress = (elapsed / CROSSFADE_MS).min(1.0);
                card_vbox.set_opacity(progress);
                snapshot_pic.set_opacity(1.0 - progress);

                if progress >= 1.0 {
                    card_vbox.set_opacity(1.0);
                    overlay.remove_overlay(&snapshot_pic);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }
    }
}
```

Note: The `Slide` arm is a temporary copy of `Crossfade` so the code compiles. Task 4 replaces it.

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Implement snapshot-based crossfade using WidgetPaintable + Picture overlay"
```

---

### Task 4: Implement slide transition

**Files:**
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Replace the Slide arm in set_page**

Replace the `crate::config::TransitionStyle::Slide` match arm with:

```rust
        crate::config::TransitionStyle::Slide => {
            // Capture snapshot of current page
            let paintable = gtk4::WidgetPaintable::new(Some(&state.card_vbox));
            let snapshot_pic = gtk4::Picture::for_paintable(&paintable);
            snapshot_pic.set_can_shrink(false);
            snapshot_pic.set_keep_aspect_ratio(false);
            state.page_turn_overlay.add_overlay(&snapshot_pic);

            // Get the widget width for slide distance
            let width = state.card_vbox.width() as f64;

            // Scroll to new position (hidden behind snapshot)
            state.card_vbox.set_opacity(0.0);
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);

            // Animate slide: snapshot slides out, live content slides in
            let card_vbox = state.card_vbox.clone();
            let overlay = state.page_turn_overlay.clone();
            let start_time = std::cell::Cell::new(None::<f64>);
            let is_forward = matches!(direction, PageDirection::Forward);
            state.card_vbox.add_tick_callback(move |_widget, clock| {
                let now = clock.frame_time() as f64 / 1_000.0;
                let t0 = match start_time.get() {
                    Some(t) => t,
                    None => {
                        start_time.set(Some(now));
                        // Show live content immediately (it slides in from off-screen)
                        card_vbox.set_opacity(1.0);
                        now
                    }
                };
                let elapsed = now - t0;
                let progress = (elapsed / CROSSFADE_MS).min(1.0);
                // Ease-out cubic for smoother deceleration
                let eased = 1.0 - (1.0 - progress).powi(3);
                let offset = (width * eased) as i32;

                if is_forward {
                    // Snapshot slides left, content slides in from right
                    snapshot_pic.set_margin_start(0);
                    snapshot_pic.set_margin_end(offset);
                    card_vbox.set_margin_start(width as i32 - offset);
                    card_vbox.set_margin_end(0);
                } else {
                    // Snapshot slides right, content slides in from left
                    snapshot_pic.set_margin_start(offset);
                    snapshot_pic.set_margin_end(0);
                    card_vbox.set_margin_start(0);
                    card_vbox.set_margin_end(width as i32 - offset);
                }

                if progress >= 1.0 {
                    // Clean up: remove snapshot, reset margins
                    overlay.remove_overlay(&snapshot_pic);
                    card_vbox.set_margin_start(0);
                    card_vbox.set_margin_end(0);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Implement slide transition with directional animation and ease-out cubic"
```

---

### Task 5: Add Transition setting to settings overlay

**Files:**
- Modify: `src/ui/settings_overlay.rs`

- [ ] **Step 1: Update NUM_SETTINGS**

Change line 6:

```rust
const NUM_SETTINGS: usize = 6;
```

- [ ] **Step 2: Add transition_style to SettingsSnapshot**

Add the field to `SettingsSnapshot` (after `navigation_mode`):

```rust
    transition_style: crate::config::TransitionStyle,
```

- [ ] **Step 3: Add transition_style field to SettingsOverlay**

Add this field to the `SettingsOverlay` struct (after `navigation_mode`):

```rust
    transition_style: crate::config::TransitionStyle,
```

- [ ] **Step 4: Add "Transition" to the names array**

Change line 50:

```rust
        let names = ["Theme", "Line Spacing", "Column Width", "Text Margins", "Navigation", "Transition"];
```

- [ ] **Step 5: Update the constructor**

In `SettingsOverlay::new()`, update the `SettingsOverlay` struct initialization to include:

```rust
            transition_style: crate::config::TransitionStyle::default(),
```

Also update the `snapshot` initialization to include:

```rust
                transition_style: crate::config::TransitionStyle::default(),
```

- [ ] **Step 6: Update show() to accept transition_style**

Change the `show` method signature:

```rust
    pub fn show(&mut self, line_spacing: u32, column_width: u32, text_margins: u32, navigation_mode: crate::config::NavigationMode, transition_style: crate::config::TransitionStyle) {
```

Update the snapshot creation inside `show`:

```rust
        self.snapshot = SettingsSnapshot {
            line_spacing,
            column_width,
            text_margins,
            theme_index: self.theme_index,
            navigation_mode,
            transition_style,
        };
        self.transition_style = transition_style;
```

Update the call to `update_displayed_values`:

```rust
        self.update_displayed_values(line_spacing, column_width, text_margins, navigation_mode, transition_style);
```

- [ ] **Step 7: Update snapshot() return type**

Change the `snapshot` method to return the transition style:

```rust
    pub fn snapshot(&self) -> (u32, u32, u32, usize, crate::config::NavigationMode, crate::config::TransitionStyle) {
        (
            self.snapshot.line_spacing,
            self.snapshot.column_width,
            self.snapshot.text_margins,
            self.snapshot.theme_index,
            self.snapshot.navigation_mode,
            self.snapshot.transition_style,
        )
    }
```

- [ ] **Step 8: Update adjust_value to handle index 5 (Transition)**

Add `transition_style` parameter to `adjust_value`:

```rust
    pub fn adjust_value(
        &mut self,
        delta: i32,
        line_spacing: u32,
        column_width: u32,
        text_margins: u32,
        navigation_mode: crate::config::NavigationMode,
        transition_style: crate::config::TransitionStyle,
    ) -> SettingsChange {
```

Add a new match arm for index 5, before the `_ =>` arm:

```rust
            5 => {
                use crate::config::TransitionStyle;
                let variants = [TransitionStyle::Crossfade, TransitionStyle::Slide, TransitionStyle::Instant];
                let current_idx = variants.iter().position(|v| *v == transition_style).unwrap_or(0);
                let new_idx = (current_idx as i32 + delta).rem_euclid(variants.len() as i32) as usize;
                let new_style = variants[new_idx];
                self.transition_style = new_style;
                let label = match new_style {
                    TransitionStyle::Crossfade => "Crossfade",
                    TransitionStyle::Slide => "Slide",
                    TransitionStyle::Instant => "Instant",
                };
                self.value_labels[5].set_label(&format!("\u{25C0} {} \u{25B6}", label));
                SettingsChange::Transition(new_style)
            }
```

- [ ] **Step 9: Update update_displayed_values**

Add `transition_style` parameter:

```rust
    pub fn update_displayed_values(&self, line_spacing: u32, column_width: u32, text_margins: u32, navigation_mode: crate::config::NavigationMode, transition_style: crate::config::TransitionStyle) {
```

Add the transition label at the end of the method:

```rust
        let transition_label = match transition_style {
            crate::config::TransitionStyle::Crossfade => "Crossfade",
            crate::config::TransitionStyle::Slide => "Slide",
            crate::config::TransitionStyle::Instant => "Instant",
        };
        self.value_labels[5].set_label(&format!("\u{25C0} {} \u{25B6}", transition_label));
```

- [ ] **Step 10: Add Transition variant to SettingsChange**

Add to the `SettingsChange` enum:

```rust
    Transition(crate::config::TransitionStyle),
```

- [ ] **Step 11: Build to verify**

Run: `cargo build`
Expected: Build errors in `keymap.rs` because callers haven't been updated yet. This is expected — Task 6 fixes them.

- [ ] **Step 12: Commit**

```bash
git add src/ui/settings_overlay.rs
git commit -m "Add Transition setting row to settings overlay (Crossfade/Slide/Instant)"
```

---

### Task 6: Update keymap.rs to wire up the Transition setting

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Update settings overlay show() call**

Around line 370-375, where the settings overlay is shown, add `ts`:

```rust
        let ls = s.config.line_spacing;
        let cw = s.config.column_width;
        let tm = s.config.text_margins;
        let nm = s.config.navigation_mode;
        let ts = s.config.transition_style;
        drop(s);
        state.borrow_mut().settings_overlay.show(ls, cw, tm, nm, ts);
```

- [ ] **Step 2: Update Escape handler (revert snapshot)**

Around line 384, update the snapshot destructure to include transition_style:

```rust
                let (snap_ls, snap_cw, snap_tm, snap_ti, snap_nm, snap_ts) = state.borrow().settings_overlay.snapshot();
```

Add inside the revert block (after `s.config.navigation_mode = snap_nm;`):

```rust
                    s.config.transition_style = snap_ts;
```

- [ ] **Step 3: Update h/l adjust_value calls**

Around lines 431-444, add `ts` to the tuple destructure and the `adjust_value` call. For the "h" handler:

```rust
            "h" | "Left" => {
                let (ls, cw, tm, nm, ts) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(-1, ls, cw, tm, nm, ts);
                apply_settings_change(state, change);
                return true;
            }
```

For the "l" handler:

```rust
            "l" | "Right" => {
                let (ls, cw, tm, nm, ts) = {
                    let s = state.borrow();
                    (s.config.line_spacing, s.config.column_width, s.config.text_margins, s.config.navigation_mode, s.config.transition_style)
                };
                let change = state.borrow_mut().settings_overlay.adjust_value(1, ls, cw, tm, nm, ts);
                apply_settings_change(state, change);
                return true;
            }
```

- [ ] **Step 4: Update "r" reset handler**

Around line 448-472, add transition style reset. After `let nm = crate::config::NavigationMode::default();`:

```rust
                let ts = crate::config::TransitionStyle::default();
```

After `s.config.navigation_mode = nm;`:

```rust
                s.config.transition_style = ts;
```

Update the `update_displayed_values` call:

```rust
                s.settings_overlay.update_displayed_values(ls, cw, tm, nm, ts);
```

- [ ] **Step 5: Handle SettingsChange::Transition in apply_settings_change**

In the `apply_settings_change` function (around line 1295-1331), add a new arm before `SettingsChange::None`:

```rust
        SettingsChange::Transition(style) => {
            s.config.transition_style = style;
        }
```

- [ ] **Step 6: Build to verify**

Run: `cargo build`
Expected: Compiles with no errors.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy`
Expected: No warnings.

- [ ] **Step 8: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Wire up Transition setting in keymap: show, adjust, reset, revert, apply"
```

---

### Task 7: Test and verify

- [ ] **Step 1: Run full build**

Run: `cargo build`
Expected: Clean compilation.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: All existing tests pass.

- [ ] **Step 4: Verify config deserialization**

Existing config files without `transition_style` should load with `Crossfade` as default due to `#[serde(default)]`. No manual test needed — this is guaranteed by serde's default attribute, same pattern used by `navigation_mode`.

# Migrate Animations to adw::TimedAnimation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all three manual `add_tick_callback` animation loops with `adw::TimedAnimation` for proper easing, cancellation, and shorter durations.

**Architecture:** Add libadwaita 0.7 dependency. Store animation handles in `AppState` so mid-flight animations can be cancelled via `.skip()`. Each animation uses `CallbackAnimationTarget` to drive property changes per frame, with `EaseOutCubic` easing.

**Tech Stack:** Rust, gtk4-rs 0.9, libadwaita 0.7, `adw::TimedAnimation`, `adw::CallbackAnimationTarget`, `adw::Easing`

**Spec:** `docs/superpowers/specs/2026-04-03-libadwaita-animations-design.md`

---

### Task 1: Add libadwaita dependency and initialize

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs:37` (before Application::builder)

- [ ] **Step 1: Add libadwaita to Cargo.toml**

In `Cargo.toml`, add to `[dependencies]`:

```toml
libadwaita = { version = "0.7", features = ["v1_4"] }
```

- [ ] **Step 2: Initialize adw in main.rs**

In `src/main.rs`, add `adw::init()` before the `gtk4::Application::builder()` call. Also add `use libadwaita as adw;` at the top of the file (after the existing `use` statements on line 16-17).

Replace:

```rust
let application = gtk4::Application::builder()
    .application_id(app_id)
    .build();
```

With:

```rust
adw::init().expect("Failed to initialize libadwaita");

let application = gtk4::Application::builder()
    .application_id(app_id)
    .build();
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully with libadwaita linked.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "Add libadwaita dependency and initialize at startup"
```

---

### Task 2: Add animation handles to AppState

**Files:**
- Modify: `src/app.rs:57-59` (AppState struct fields)
- Modify: `src/app.rs:511` (AppState construction)

- [ ] **Step 1: Replace animation_gen with animation handles in AppState struct**

In `src/app.rs`, add `use libadwaita as adw;` near the top imports.

Replace the `animation_gen` field in the `AppState` struct:

```rust
    /// Generation counter for crossfade animations. Incremented on each page turn
    /// so stale animation callbacks don't stomp on opacity.
    pub animation_gen: std::rc::Rc<std::cell::Cell<u64>>,
```

With:

```rust
    /// Active page-turn animation (crossfade or slide). Stored so it can be
    /// cancelled via .skip() if a new page turn fires mid-flight.
    pub page_turn_anim: Option<adw::TimedAnimation>,
    /// Active cursor highlight fade-out animation.
    pub cursor_fade_anim: Option<adw::TimedAnimation>,
```

- [ ] **Step 2: Update AppState construction**

In `src/app.rs`, replace:

```rust
        animation_gen: std::rc::Rc::new(std::cell::Cell::new(0)),
```

With:

```rust
        page_turn_anim: None,
        cursor_fade_anim: None,
```

- [ ] **Step 3: Fix any remaining references to animation_gen**

Search for `animation_gen` across the codebase. If any other files reference it, they will be addressed in later tasks when those animation sites are rewritten. For now, just ensure `app.rs` compiles.

Run: `cargo build`
Expected: Compiles. (If other files reference `animation_gen`, they'll produce errors — that's expected and will be fixed in Tasks 3-5.)

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "Replace animation_gen counter with adw::TimedAnimation handles in AppState"
```

---

### Task 3: Convert page-turn crossfade to adw::TimedAnimation

**Files:**
- Modify: `src/input/navigation.rs:584-681` (CROSSFADE_MS constant and Crossfade match arm)

- [ ] **Step 1: Add libadwaita import**

At the top of `src/input/navigation.rs`, add:

```rust
use libadwaita as adw;
```

- [ ] **Step 2: Remove the CROSSFADE_MS constant**

Delete line 585:

```rust
const CROSSFADE_MS: f64 = 650.0;
```

- [ ] **Step 3: Rewrite the Crossfade match arm**

Replace the entire `TransitionStyle::Crossfade` arm (lines ~641-682) with:

```rust
        crate::config::TransitionStyle::Crossfade => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            // Update page underneath (live content stays fully opaque)
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.card_vbox.set_opacity(1.0);

            // Fade out the snapshot overlay: 1.0 → 0.0, 250ms, ease-out-cubic
            let overlay = state.page_turn_overlay.clone();
            let snap = snapshot_pic.clone();
            let target = adw::CallbackAnimationTarget::new(move |value| {
                snap.set_opacity(value);
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                1.0,  // from
                0.0,  // to
                250,  // duration ms
                &target,
            );
            anim.set_easing(adw::Easing::EaseOutCubic);

            let snap_cleanup = snapshot_pic.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: May fail if Slide arm still references `CROSSFADE_MS`. That's fine — Task 4 fixes it. If so, temporarily add `const CROSSFADE_MS: f64 = 650.0;` back to unblock compilation, then remove it in Task 4.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Convert page-turn crossfade from tick callback to adw::TimedAnimation"
```

---

### Task 4: Convert page-turn slide to adw::TimedAnimation

**Files:**
- Modify: `src/input/navigation.rs:683-746` (Slide match arm)

- [ ] **Step 1: Rewrite the Slide match arm**

Replace the entire `TransitionStyle::Slide` arm (lines ~683-746) with:

```rust
        crate::config::TransitionStyle::Slide => {
            // Capture static snapshot of current page
            let Some(snapshot_pic) = capture_page_snapshot(state) else {
                clear_old_page_dim(state);
                state.page_top_line = new_top;
                snap_scroll_to_line(state, new_top);
                return;
            };

            // Cancel any in-flight page animation
            if let Some(prev) = state.page_turn_anim.take() {
                prev.skip();
            }

            let width = state.card_vbox.width() as f64;

            // Update page underneath, show live content immediately
            clear_old_page_dim(state);
            state.page_top_line = new_top;
            snap_scroll_to_line(state, new_top);
            state.card_vbox.set_opacity(1.0);
            state.card_vbox.set_margin_start(0);
            state.card_vbox.set_margin_end(0);

            // Animate snapshot sliding out: 0.0 → 1.0 progress, 250ms, ease-out-cubic
            let overlay = state.page_turn_overlay.clone();
            let card = state.card_vbox.clone();
            let snap = snapshot_pic.clone();
            let is_forward = matches!(direction, PageDirection::Forward);
            let target = adw::CallbackAnimationTarget::new(move |progress| {
                let offset = (width * progress) as i32;
                if is_forward {
                    snap.set_margin_start(0);
                    snap.set_margin_end(offset);
                    card.set_margin_start((width as i32) - offset);
                    card.set_margin_end(0);
                } else {
                    snap.set_margin_start(offset);
                    snap.set_margin_end(0);
                    card.set_margin_start(0);
                    card.set_margin_end((width as i32) - offset);
                }
            });
            let anim = adw::TimedAnimation::new(
                &snapshot_pic,
                0.0,  // from
                1.0,  // to
                250,  // duration ms
                &target,
            );
            anim.set_easing(adw::Easing::EaseOutCubic);

            let snap_cleanup = snapshot_pic.clone();
            let card_cleanup = state.card_vbox.clone();
            anim.connect_done(move |_| {
                overlay.remove_overlay(&snap_cleanup);
                card_cleanup.set_margin_start(0);
                card_cleanup.set_margin_end(0);
            });

            anim.play();
            state.page_turn_anim = Some(anim);
        }
```

- [ ] **Step 2: Remove CROSSFADE_MS if still present**

If the `CROSSFADE_MS` constant still exists (was temporarily kept for compilation in Task 3), delete it now. No code should reference it.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully. Both Crossfade and Slide arms now use `adw::TimedAnimation`.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Convert page-turn slide from tick callback to adw::TimedAnimation"
```

---

### Task 5: Convert cursor highlight fade to adw::TimedAnimation

**Files:**
- Modify: `src/input/navigation.rs:922-994` (HIGHLIGHT_FADE_MS constant and fade-out block in update_highlight)

- [ ] **Step 1: Remove HIGHLIGHT_FADE_MS constant**

Delete:

```rust
/// Duration for cursor line highlight crossfade in milliseconds.
const HIGHLIGHT_FADE_MS: f64 = 500.0;
```

- [ ] **Step 2: Rewrite the cursor fade-out block in update_highlight**

In the `update_highlight` function, find the block that handles `!state.dim_enabled` and applies the fade-out animation (the `if let Some(old_line) = state.prev_highlight_line.get()` block). Replace the inner animation logic.

Replace the block starting at the `// Apply fade-out to the old cursor line (if it changed)` comment through the closing `}` of the `if old_line != state.current_line` block (approximately lines 955-995) with:

```rust
        // Apply fade-out to the old cursor line (if it changed)
        if let Some(old_line) = state.prev_highlight_line.get() {
            if old_line != state.current_line {
                // Cancel any in-flight cursor fade
                if let Some(prev) = state.cursor_fade_anim.take() {
                    prev.skip();
                }

                // Remove any existing fade, then apply to old line
                buffer.remove_tag(fade_tag, &buf_start, &buf_end);
                if let Some(old_start) = buffer.iter_at_line(old_line as i32) {
                    let mut old_end = old_start;
                    if !old_end.ends_line() {
                        old_end.forward_to_line_end();
                    }
                    buffer.apply_tag(fade_tag, &old_start, &old_end);
                }

                // Animate fade-out: alpha from 1.0 → 0.0, 150ms, ease-out-cubic
                let fade_tag_clone = fade_tag.clone();
                let buf_clone = buffer.clone();
                let target = adw::CallbackAnimationTarget::new(move |value| {
                    let alpha = value as f32 * 0.10; // max alpha 0.10
                    use gtk4::prelude::TextTagExt;
                    fade_tag_clone.set_paragraph_background_rgba(Some(
                        &gtk4::gdk::RGBA::new(0.0, 0.3, 0.86, alpha),
                    ));
                    if value <= 0.0 {
                        let (s, e) = buf_clone.bounds();
                        buf_clone.remove_tag(&fade_tag_clone, &s, &e);
                    }
                });
                // Need a widget to attach the animation to — use text_view
                let anim = adw::TimedAnimation::new(
                    &state.text_view,
                    1.0,  // from
                    0.0,  // to
                    150,  // duration ms
                    &target,
                );
                anim.set_easing(adw::Easing::EaseOutCubic);
                anim.play();
                state.cursor_fade_anim = Some(anim);
            }
        }
```

Note: `state.cursor_fade_anim` requires `&mut` access. The `update_highlight` function currently takes `&AppState`. This needs to change to `&mut AppState`. Check the call sites of `update_highlight` — they should already have `&mut` access to state since they go through the `RefCell<AppState>` borrow. Update the function signature:

```rust
fn update_highlight(state: &mut AppState) {
```

If any call sites pass `&state` where `&mut state` is needed, update them.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "Convert cursor highlight fade from tick callback to adw::TimedAnimation"
```

---

### Task 6: Clean up removed code

**Files:**
- Modify: `src/app.rs` (remove animation_gen if not already done)
- Modify: `src/input/navigation.rs` (verify no stale references)

- [ ] **Step 1: Search for stale references**

Search the codebase for any remaining references to:
- `animation_gen`
- `CROSSFADE_MS`
- `HIGHLIGHT_FADE_MS`
- `add_tick_callback` (in animation contexts — the library picker or other UI may use it legitimately)

Remove any dead code found.

- [ ] **Step 2: Verify full build and clippy**

Run: `cargo build && cargo clippy`
Expected: No errors, no warnings related to the changes.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Clean up stale animation references"
```

Only commit if there were actual changes to clean up. Skip if the previous tasks already removed everything.

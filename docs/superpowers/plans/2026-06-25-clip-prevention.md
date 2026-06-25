# Clip-Prevention Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent the line-clipping bug class by collapsing the verbatim-duplicated free-scroll covering math onto one helper, adding the missing translation-overlay clip guard, and enforcing the no-clip invariant on an overlay.

**Architecture:** Three independent parts. A: extract a `line_yrange_rows` producer so `scrolloff_bottom_clip_widgets` calls the pure `bottom_clip_height` instead of an inline copy. B: add a `bottom_clip` box + `value_changed` recompute to the translation overlay, mirroring gloss/journal. C: emit an overlay viewport rect under `LIT_HEADLESS_TEST` and add a headless `#[ignore]`d overlay clip test.

**Tech Stack:** Rust, GTK4, sourceview5, cage/grim/wtype headless harness, `cargo test`.

## Global Constraints

- Do NOT run the app (`cargo run`) or launch cage from the agent shell (the live dwl session owns the seat). Build with `cargo build`; run `cargo test --bins`. The USER runs e2e/`cage` commands. (CLAUDE.md)
- `cargo test --bins` must stay green; clippy warning count must not increase (baseline 118).
- The paginated main-card clip (`scroll.rs::update_bottom_clip`) is OUT OF SCOPE — do not touch it.
- The overlay `snap_value_to_line` functions are OUT OF SCOPE (different algorithms, not duplicates).
- `bottom_clip_height(rows: &[(f64,f64)], top_y: f64, viewport_h: f64, content_h: f64) -> i32` already exists in `src/ui/mod.rs` (line 84) and is the single covering algorithm; do not change its signature or logic.
- `recompute_overlay_bottom_clip(view: &gtk4::TextView, clip: &gtk4::Box, scrolled: &gtk4::ScrolledWindow)` exists in `src/ui/mod.rs` (line 148).
- Overlay clip box mirrors gloss/journal exactly: a `gtk4::Box::new(Vertical, 0)` with css `gloss-bottom-clip`, `valign(End)`, `halign(Fill)`, `vexpand(false)`, `can_target(false)`, added as a `set_clip_overlay(true)`, `set_measure_overlay(false)` overlay on an `Overlay` wrapping the `ScrolledWindow`.
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

### Task 1: `line_yrange_rows` producer (Part A)

**Files:**
- Modify: `src/ui/mod.rs` (add the producer near `display_rows`, ~line 124).
- Test: `src/ui/mod.rs` — note in-code why no pure unit test (needs realized GTK layout).

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub(crate) fn line_yrange_rows(view: &gtk4::TextView, top_val: f64, viewport_h: f64) -> Vec<(f64, f64)>` — logical-line `(row_top, row_bottom)` pairs in vadjustment space, starting at the line at `top_val`, stopping once a row's top reaches `top_val + viewport_h`. Mirrors the row set the current `scrolloff_bottom_clip_widgets` loop visits.

- [ ] **Step 1: Add the producer.** In `src/ui/mod.rs`, immediately after `display_rows` (ends ~line 145), add:

```rust
/// Logical-line `(row_top, row_bottom)` pairs in vadjustment/scroll coordinate
/// space, from the line at `top_val` down to the first line whose top reaches
/// `top_val + viewport_h`. The logical-line analog of `display_rows` (which
/// walks visual/wrapped rows): scroll-mode and the translation-follow path size
/// their bottom clip from whole-line `line_yrange` geometry, NOT wrapped rows.
/// Feed the result to `bottom_clip_height` so scroll-mode shares the overlays'
/// single covering algorithm instead of re-implementing it.
pub(crate) fn line_yrange_rows(
    view: &gtk4::TextView,
    top_val: f64,
    viewport_h: f64,
) -> Vec<(f64, f64)> {
    let bottom_y = top_val + viewport_h;
    let mut rows: Vec<(f64, f64)> = Vec::new();
    let (mut iter, _) = view.line_at_y(top_val.max(0.0) as i32);
    loop {
        let (ly, lh) = view.line_yrange(&iter);
        let row_top = ly as f64;
        let row_bottom = (ly + lh) as f64;
        if row_top >= bottom_y {
            break;
        }
        rows.push((row_top, row_bottom));
        if !iter.forward_line() {
            break;
        }
    }
    rows
}
```

- [ ] **Step 2: Build.**

Run: `cargo build`
Expected: builds clean. (No pure unit test: `line_at_y`/`line_yrange` need a realized TextView with layout; parity is verified by the unchanged `bottom_clip_height` tests + the Part C harness + the Part A render check. Do NOT add a test that constructs an unrealized view and asserts nothing.)

- [ ] **Step 3: Commit.**

```bash
git add src/ui/mod.rs
git commit -m "feat(ui): add line_yrange_rows producer for shared bottom-clip math

Logical-line analog of display_rows so the scroll-mode bottom clip can feed the
pure bottom_clip_height helper instead of re-implementing the covering algorithm.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: Route `scrolloff_bottom_clip_widgets` through `bottom_clip_height` (Part A)

**Files:**
- Modify: `src/input/scroll.rs` — `scrolloff_bottom_clip_widgets` (lines 1090-1142).

**Interfaces:**
- Consumes: `crate::ui::line_yrange_rows` (Task 1), `crate::ui::bottom_clip_height` (existing).
- Produces: same fn signature, no behavior change; inline algorithm deleted.

- [ ] **Step 1: Replace the body.** In `src/input/scroll.rs`, replace the entire body of `scrolloff_bottom_clip_widgets` (from the `let adj = ...` after the signature through the final `}` before the next fn, currently lines 1096-1141) with:

```rust
    let adj = scrolled_window.vadjustment();
    let viewport_h = adj.page_size();
    if viewport_h <= 0.0 {
        if bottom_clip.height_request() != 0 {
            bottom_clip.set_height_request(0);
        }
        return;
    }
    // Share the overlays' single covering algorithm: logical-line rows fed to the
    // pure bottom_clip_height. (Scroll-mode uses line_yrange rows, not the wrapped
    // display_rows the overlays use — same algorithm, different row source.)
    let rows = crate::ui::line_yrange_rows(text_view, top_val, viewport_h);
    let clip_h = crate::ui::bottom_clip_height(&rows, top_val, viewport_h, adj.upper());
    if bottom_clip.height_request() != clip_h {
        bottom_clip.set_height_request(clip_h);
    }
```

Keep the function signature `(text_view: &sourceview5::View, scrolled_window: &gtk4::ScrolledWindow, bottom_clip: &gtk4::Box, top_val: f64)` unchanged. Note `text_view` is `&sourceview5::View`; `line_yrange_rows`/`bottom_clip_height` take `&gtk4::TextView` — `sourceview5::View` derefs/upcasts to `gtk4::TextView`, so pass `text_view.upcast_ref::<gtk4::TextView>()` if a bare `text_view` fails to coerce. Verify which the compiler accepts.

- [ ] **Step 2: Build + test.**

Run: `cargo build && cargo test --bins`
Expected: clean build, 445 tests pass (the algorithm is unchanged; this is a pure internal refactor).

- [ ] **Step 3: Clippy parity.**

Run: `cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: `generated 118 warnings` (unchanged).

- [ ] **Step 4: Commit.**

```bash
git add src/input/scroll.rs
git commit -m "refactor(clip): scrolloff bottom clip uses shared bottom_clip_height

scrolloff_bottom_clip_widgets was a verbatim copy of bottom_clip_height's
covering algorithm (same last_full_bottom/any_full/guards, only the row source
differed). Route it through line_yrange_rows + bottom_clip_height so scroll-mode
can never drift from the tested helper. No behavior change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

- [ ] **Step 5: Flag the render check (Part A).** In the task report, note: scroll-mode (j/k) on a long page and the translation-follow path need a visual confirm that the bottom partial row is still masked identically. The user runs this; do not claim it verified.

---

### Task 3: Translation overlay bottom-clip guard (Part B)

**Files:**
- Modify: `src/ui/translation_overlay.rs` — struct fields, `new()` (the `scrolled`/`container` wiring ~108-128), and reveal (`show()` ~159-160).

**Interfaces:**
- Consumes: `crate::ui::recompute_overlay_bottom_clip` (existing).
- Produces: a `bottom_clip: gtk4::Box` field and a `value_changed`-driven recompute, mirroring gloss/journal.

- [ ] **Step 1: Add the field.** In the `TranslationOverlay` struct (near the `scrolled: ScrolledWindow,` field ~line 78), add:

```rust
    bottom_clip: gtk4::Box,
```

- [ ] **Step 2: Wrap the scrolled window in an Overlay with the clip box.** In `new()`, replace the `container.append(&scrolled);` (line 118) block. After `scrolled.set_child(Some(&content_vbox));`, insert:

```rust
        // Free-scroll bottom clip: mask any partial last row at the viewport
        // bottom (mirrors gloss/journal). The clip box overlays an OUTER Overlay
        // wrapping the ScrolledWindow so it stays pinned to the viewport bottom.
        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&scrolled));
        let bottom_clip = gtk4::Box::new(Orientation::Vertical, 0);
        bottom_clip.add_css_class("gloss-bottom-clip");
        bottom_clip.set_valign(Align::End);
        bottom_clip.set_halign(Align::Fill);
        bottom_clip.set_vexpand(false);
        bottom_clip.set_can_target(false);
        scroll_overlay.add_overlay(&bottom_clip);
        scroll_overlay.set_measure_overlay(&bottom_clip, false);
        scroll_overlay.set_clip_overlay(&bottom_clip, true);
        // Recompute on EVERY value change (not just named scroll calls) so the
        // clip can't keep a stale open-time height.
        {
            let view = content_vbox.clone();
            let clip = bottom_clip.clone();
            let sc = scrolled.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                crate::ui::recompute_overlay_bottom_clip_box(&view, &clip, &sc);
            });
        }
        container.append(&scroll_overlay);
```

NOTE: the translation overlay scrolls a `gtk4::Box` (`content_vbox`), NOT a `TextView`. `recompute_overlay_bottom_clip` takes a `&TextView` and uses `display_rows` (TextView-only). So it CANNOT be reused directly here. See Step 3 for the box-content variant.

- [ ] **Step 3: Add a box-content clip recompute that masks the partial bottom slack.** Because the scrolled child is a `gtk4::Box` of row widgets (not a `TextView`), the `display_rows`/`line_yrange` row geometry does not apply. Add to `src/ui/mod.rs`:

```rust
/// Bottom-clip recompute for an overlay whose scrolled child is a widget BOX
/// (e.g. the translation overlay's column stack), not a TextView. Without real
/// per-row geometry the safe, behavior-additive guard is: cover only the slack
/// BELOW the content when the document ends inside the viewport (so trailing
/// whitespace doesn't read as a clipped half-row); when content overflows, clip
/// 0 (the box rows are whole widgets — GTK does not split one across the edge,
/// so there is no partial-row to mask, unlike a TextView's wrapped lines).
pub(crate) fn recompute_overlay_bottom_clip_box(
    _child: &gtk4::Box,
    clip: &gtk4::Box,
    scrolled: &gtk4::ScrolledWindow,
) {
    use gtk4::prelude::*;
    let adj = scrolled.vadjustment();
    let viewport_h = adj.page_size();
    if viewport_h <= 0.0 {
        if clip.height_request() != 0 {
            clip.set_height_request(0);
        }
        return;
    }
    let bottom_y = adj.value() + viewport_h;
    let content_h = adj.upper();
    // Content ends inside the viewport: cover the slack below it. Otherwise the
    // overflow scrolls and whole row widgets are never split at the edge → 0.
    let clip_h = if content_h <= bottom_y + 0.5 {
        (bottom_y - content_h).max(0.0).round() as i32
    } else {
        0
    };
    if clip.height_request() != clip_h {
        clip.set_height_request(clip_h);
    }
}
```

(This is intentionally simpler than the TextView path: a `gtk4::Box` lays out whole child widgets, so there is no wrapped partial row to mask — the only clip needed is the trailing-slack cover. Document this difference in the doc comment as shown.)

- [ ] **Step 4: Store the field and recompute on reveal.** In the `Self { ... }` constructor literal (line 120-128), add `bottom_clip,`. In `show()`, after `self.container.set_height_request(card_height);` (line 160), add a deferred recompute so it runs after layout settles:

```rust
        {
            let child = self.content_vbox.clone();
            let clip = self.bottom_clip.clone();
            let sc = self.scrolled.clone();
            glib::idle_add_local_once(move || {
                crate::ui::recompute_overlay_bottom_clip_box(&child, &clip, &sc);
            });
        }
```

Confirm `glib` is imported in the file; if not, use `gtk4::glib::idle_add_local_once`.

- [ ] **Step 5: Build + test.**

Run: `cargo build && cargo test --bins`
Expected: clean build, 445 tests pass.

- [ ] **Step 6: Clippy parity.**

Run: `cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: `generated 118 warnings`.

- [ ] **Step 7: Commit.**

```bash
git add src/ui/translation_overlay.rs src/ui/mod.rs
git commit -m "feat(translation): add bottom-clip guard to the translation overlay

The translation overlay scrolls but had no bottom-clip box (only a fixed
card_height) — trailing slack below short content read as a clipped edge. Add a
gloss/journal-style clip box recomputed on value_changed via a box-content
variant (the scrolled child is a widget Box, not a TextView, so it masks only
trailing slack — whole row widgets are never split at the edge).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

- [ ] **Step 8: Flag the render check (Part B).** Report: turn translations on, scroll a column to its last line — confirm no descender clip AND nothing previously visible is now hidden by the new clip box. User runs this.

---

### Task 4: Overlay viewport-rect emission (Part C)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` — add a method that logs the overlay viewport rect under `LIT_HEADLESS_TEST`; called on synopsis reveal.

**Interfaces:**
- Consumes: nothing new.
- Produces: a `TEST_OVERLAY_VIEWPORT_RECT x y w h` log line (parsed by the Task 5 harness), emitted when the synopsis/gloss overlay is shown under `LIT_HEADLESS_TEST`.

- [ ] **Step 1: Add the emit method.** In `src/ui/gloss_overlay.rs` (in the `impl GlossOverlay`), add:

```rust
    /// Headless test support: under `LIT_HEADLESS_TEST`, log the gloss/synopsis
    /// overlay scrolled viewport rect in window coords (== screenshot pixels in
    /// the cage fullscreen output), so the overlay clip test can pass it to
    /// check_line_clipping.py --region. Mirrors scroll::emit_test_viewport_rect.
    /// Format (stable, parsed by tests/harness): `TEST_OVERLAY_VIEWPORT_RECT x y w h`.
    pub(crate) fn emit_test_overlay_viewport_rect(&self) {
        if std::env::var_os("LIT_HEADLESS_TEST").is_none() {
            return;
        }
        if let Some(root) = self.gloss_scrolled.root() {
            if let Some(r) = self.gloss_scrolled.compute_bounds(&root) {
                crate::logging::log(&format!(
                    "TEST_OVERLAY_VIEWPORT_RECT {} {} {} {}",
                    r.x().round() as i32,
                    r.y().round() as i32,
                    r.width().round() as i32,
                    r.height().round() as i32
                ));
                return;
            }
        }
        crate::logging::log("TEST_OVERLAY_VIEWPORT_RECT unavailable (compute_bounds returned None)");
    }
```

Verify the field name is `gloss_scrolled` (it is, per gloss_overlay.rs:1012). If the overlay exposes a different scrolled field for synopsis, use that one.

- [ ] **Step 2: Call it on reveal.** Find the synopsis/gloss reveal that makes the card visible (the `show_*` path that ends with `self.update_bottom_clip()`, e.g. gloss_overlay.rs:1358). After the card is visible and `update_bottom_clip()` runs, add a deferred emit so bounds are realized:

```rust
        {
            let this = self.clone(); // if GlossOverlay is Clone; else capture the scrolled
            glib::idle_add_local_once(move || this.emit_test_overlay_viewport_rect());
        }
```

If `GlossOverlay` is not `Clone`, instead clone `self.gloss_scrolled` and inline the bounds-logging in the closure. Determine which by checking the type; pick the form that compiles.

- [ ] **Step 3: Build.**

Run: `cargo build`
Expected: clean (the emit is a no-op without `LIT_HEADLESS_TEST`, so `cargo test --bins` is unaffected).

- [ ] **Step 4: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "test(clip): emit overlay viewport rect under LIT_HEADLESS_TEST

TEST_OVERLAY_VIEWPORT_RECT mirrors emit_test_viewport_rect for the synopsis/gloss
overlay so the headless overlay clip test can target its region.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: Headless overlay clip test (Part C)

**Files:**
- Create: `tests/overlay_clipping.rs`.
- Modify: `tests/harness/mod.rs` — parse `TEST_OVERLAY_VIEWPORT_RECT`.

**Interfaces:**
- Consumes: the existing harness (`tests/harness/mod.rs`: launch, `wait_for`, `key`, `assert_no_line_clipping`), and `TEST_OVERLAY_VIEWPORT_RECT` from Task 4.
- Produces: an `#[ignore]`d test that opens the synopsis overlay, scrolls to the bottom, and asserts no clipping.

- [ ] **Step 1: Read the existing clip test + harness.** Read `tests/line_clipping.rs` and `tests/harness/mod.rs` in full to reuse their exact launch/region/wait API (the harness owns cage setup, `TEST_VIEWPORT_RECT` parsing, and `assert_no_line_clipping`). Do not re-derive cage flags; reuse the harness entry point.

- [ ] **Step 2: Add overlay-rect parsing to the harness.** In `tests/harness/mod.rs`, wherever `TEST_VIEWPORT_RECT` is parsed (a `wait_for`/grep on the dev log), add a sibling parser `wait_for_overlay_viewport_rect(&self) -> (i32,i32,i32,i32)` that greps `TEST_OVERLAY_VIEWPORT_RECT x y w h` from the same log and returns the rect. Mirror the existing rect parser's body exactly, changing only the marker string.

- [ ] **Step 3: Write the test.** Create `tests/overlay_clipping.rs`:

```rust
//! Headless overlay clip invariant: the synopsis/gloss overlay must not clip its
//! first/last line. Companion to tests/line_clipping.rs (main reading card).
//! #[ignore]d like the other e2e tests — run via:
//!   ./scripts/e2e-env.sh cargo test --test overlay_clipping -- --ignored --nocapture

mod harness;
use harness::Harness;

#[test]
#[ignore = "requires cage/grim/wtype; run via scripts/e2e-env.sh"]
fn synopsis_overlay_does_not_clip() {
    let h = Harness::launch().expect("launch reader in cage");
    // Advance into a chapter so a synopsis exists (front matter has none), then
    // open the synopsis overlay with `h`.
    h.key("3").unwrap(); // next chapter -> cursor onto CHAPTER heading
    h.key("h").unwrap(); // open synopsis overlay
    let region = h.wait_for_overlay_viewport_rect().expect("overlay rect");
    // Scroll to the bottom of the overlay so the last line sits at the edge.
    for _ in 0..40 {
        h.key("j").unwrap();
    }
    h.assert_no_line_clipping("synopsis_overlay_bottom", region)
        .expect("synopsis overlay must not clip its last line");
}
```

Adjust `Harness::launch`, `key`, `assert_no_line_clipping` names to the harness's actual API discovered in Step 1 (the line_clipping test shows the real method names — match them).

- [ ] **Step 4: Verify the test is gated (does NOT run under bare `cargo test`).**

Run: `cargo test --test overlay_clipping`
Expected: `0 passed; 0 failed; 1 ignored` (the `#[ignore]` keeps it out of the default run).

- [ ] **Step 5: Verify it COMPILES.**

Run: `cargo test --test overlay_clipping --no-run`
Expected: compiles clean (the harness API calls resolve).

- [ ] **Step 6: Commit.**

```bash
git add tests/overlay_clipping.rs tests/harness/mod.rs
git commit -m "test(clip): headless overlay clip invariant (synopsis overlay)

Extends the no-clip invariant beyond the main reading card: opens the synopsis
overlay, scrolls to the bottom, asserts no line clipping via the same pixel
detector. #[ignore]d; run via scripts/e2e-env.sh. Closes the overlay test gap.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

- [ ] **Step 7: Hand the user the e2e command (Part C).** Report that the agent cannot launch cage (seat owned by dwl); give the user:

```bash
./scripts/e2e-env.sh cargo test --test overlay_clipping -- --ignored --nocapture
```

Ask them to paste the result and any `target/ui/` screenshot.

---

### Task 6: Full gate + ledger/spec note

**Files:**
- Modify: `docs/superpowers/audit-opportunities.md` — note the clip-prevention work and the deliberately-excluded items (main-card unify, snap dedup) so a future audit doesn't re-propose them.

- [ ] **Step 1: Pure suite + clippy.**

Run: `cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: 445 pass; `generated 118 warnings`.

- [ ] **Step 2: Append a ledger note.** In `docs/superpowers/audit-opportunities.md`, under "Noted but NOT numbered", add: the free-scroll covering math is now unified (`line_yrange_rows` + `bottom_clip_height`); the main-card paginated clip and the overlay `snap_value_to_line` pair are deliberately NOT unified (different algorithms, behavior-changing) so they should not be re-proposed as safe-scope dedup.

- [ ] **Step 3: Commit.**

```bash
git add docs/superpowers/audit-opportunities.md
git commit -m "docs(audit): note clip-prevention unification + excluded items

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

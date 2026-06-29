# BottomClipGuard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hand-wired bottom-clip lifecycle in the gloss, journal, and translation overlays with one self-wiring `BottomClipGuard` that owns all recompute paths, so a surface cannot drop a path (the journal Q&A clip bug).

**Architecture:** A new `BottomClipGuard` struct (`src/ui/bottom_clip_guard.rs`) builds the clip `Box`, adds it to the scroll `Overlay`, and wires the persistent `value_changed` catch-all in one `attach()` call. It exposes `recompute()` (named scroll sites) and `on_open()` (the open-time `connect_changed`+idle coverage lifted from gloss's `reset_scroll_top`). Two constructors: `attach` (TextView math) and `attach_box` (Box-child math). Gloss is migrated first as the behavior-preserving reference; journal and translation then inherit the proven wiring (journal GAINS the missing paths → bug fixed).

**Tech Stack:** Rust, GTK4 (`gtk4`, `glib`), the existing shared clip math in `src/ui/mod.rs`.

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test`. The user runs the app and verifies on-screen. (CLAUDE.md)
- `cargo test --bins` stays green; clippy warning count must not increase (baseline 118).
- The clip MATH stays in `src/ui/mod.rs` — the guard CALLS `recompute_overlay_bottom_clip` (TextView) and `recompute_overlay_bottom_clip_box` (Box); it does not move the math.
- Gloss is the REFERENCE: its migration must be behavior-preserving (it clips correctly today). Verify on the real display, do not change its visible behavior.
- The clip Box properties are fixed (must match gloss today): `valign=End`, `halign=Fill`, `vexpand=false`, `can_target=false`, `add_css_class("gloss-bottom-clip")`, `height_request=0`. Overlay wiring: `add_overlay`, `set_measure_overlay(false)`, `set_clip_overlay(true)`.
- The journal **selection-bar** `value_changed` handler (`bar_for_scroll.queue_draw()`, journal_overlay.rs:147-150) is a SEPARATE concern and MUST stay — only the missing CLIP recompute is added (via the guard's own handler; two handlers on one signal is fine).
- The MAIN card paginated clip (`scroll.rs::update_bottom_clip`) and the top-edge line-snapping are OUT OF SCOPE — do not touch.
- Commit messages end with the Co-Authored-By: Claude Opus 4.8 / Claude-Session trailer.

## Reference: gloss's four mechanisms (the behavior to preserve)

From `src/ui/gloss_overlay.rs` (the working surface):
- **clip box** (~270-279): the props above + `add_overlay`/`measure=false`/`clip=true`.
- **path (c)** (~287-294): `gloss_scrolled.vadjustment().connect_value_changed(|_| recompute_overlay_bottom_clip(&view,&clip,&scrolled))`.
- **path (a)** = `reset_scroll_top` (~the fn): snaps to top, connects a `connect_changed` handler that re-pins to top while `pinning` is true AND recomputes the clip, disconnects after a 250ms `timeout_add_local_once`, plus an `idle_add_local_once` backstop recompute. Called from `show_gloss_with_color`:656, `show_glossing`:736, `show_echoes`:810, `show_synopsis`:942.
- **path (b)** = `update_bottom_clip()` (~1033) called from `scroll_cursor_into_view`:1380, `scroll_gloss`:1616, `scroll_gloss_to_top`:1624, `scroll_gloss_to_bottom`:1640.

---

### Task 1: `BottomClipGuard` struct + `attach` (TextView) + `recompute`

**Files:**
- Create: `src/ui/bottom_clip_guard.rs`
- Modify: `src/ui/mod.rs` — add `pub(crate) mod bottom_clip_guard;`

**Interfaces:**
- Consumes: `crate::ui::recompute_overlay_bottom_clip(view, clip, scrolled)` (existing, mod.rs).
- Produces: `BottomClipGuard` with `attach(scroll_overlay, view, scrolled) -> Self`, `clip(&self) -> &gtk4::Box`, `recompute(&self)`. Used by Tasks 3-5.

- [ ] **Step 1: Create the file with `attach` + `recompute`.** Write `src/ui/bottom_clip_guard.rs`:

```rust
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{glib, Align, Overlay};

/// Which shared clip-math fn a guard drives: TextView surfaces mask the partial
/// wrapped row; Box-child surfaces (the translation column stack) only cover
/// trailing slack.
#[derive(Clone)]
enum ClipKind {
    TextView(gtk4::TextView),
    Box,
}

/// Owns a free-scroll surface's bottom clip box AND every recompute path, so a
/// surface attaches it once and cannot drop a path (the historical bug: a
/// surface hand-wired some paths but not the `value_changed` catch-all, so the
/// clip went stale on resize/scroll). See docs/troubleshooting/clip-prevention.md.
pub(crate) struct BottomClipGuard {
    kind: ClipKind,
    clip: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
}

impl BottomClipGuard {
    /// Build the clip Box (fixed props), add it to `scroll_overlay`
    /// (measure=false, clip=true), and wire the persistent `value_changed`
    /// catch-all (path c). For a TextView-content scrolled window.
    pub(crate) fn attach(
        scroll_overlay: &Overlay,
        view: &gtk4::TextView,
        scrolled: &gtk4::ScrolledWindow,
    ) -> Self {
        let clip = build_clip_box();
        scroll_overlay.add_overlay(&clip);
        scroll_overlay.set_measure_overlay(&clip, false);
        scroll_overlay.set_clip_overlay(&clip, true);

        let guard = Self {
            kind: ClipKind::TextView(view.clone()),
            clip: clip.clone(),
            scrolled: scrolled.clone(),
        };
        // path (c): recompute on EVERY value change (scroll OR layout-driven
        // page_size change, e.g. an ask card resizing the viewport).
        {
            let kind = guard.kind.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                recompute(&kind, &clip, &scrolled);
            });
        }
        guard
    }

    /// The clip Box, e.g. so the caller can stack a selection-bar overlay after it.
    pub(crate) fn clip(&self) -> &gtk4::Box {
        &self.clip
    }

    /// (b) Recompute now — call from the named scroll methods.
    pub(crate) fn recompute(&self) {
        recompute(&self.kind, &self.clip, &self.scrolled);
    }
}

fn build_clip_box() -> gtk4::Box {
    let clip = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    clip.set_valign(Align::End);
    clip.set_halign(Align::Fill);
    clip.set_vexpand(false);
    clip.set_can_target(false);
    clip.add_css_class("gloss-bottom-clip");
    clip.set_height_request(0);
    clip
}

fn recompute(kind: &ClipKind, clip: &gtk4::Box, scrolled: &gtk4::ScrolledWindow) {
    match kind {
        ClipKind::TextView(view) => {
            crate::ui::recompute_overlay_bottom_clip(view, clip, scrolled)
        }
        ClipKind::Box => crate::ui::recompute_overlay_bottom_clip_box(clip, scrolled),
    }
}
```

- [ ] **Step 2: Register the module.** In `src/ui/mod.rs`, add alongside the other `pub mod` lines (e.g. after `pub mod ask_card;`):

```rust
pub(crate) mod bottom_clip_guard;
```

- [ ] **Step 3: Build.**

Run: `cargo build`
Expected: clean. (`Box` variant + `attach_box` come in Task 2; `recompute_overlay_bottom_clip_box` already exists in mod.rs. The `ClipKind::Box` arm compiles now even though no constructor yields it yet — it may warn "variant never constructed"; that resolves in Task 2. If clippy flags it as dead now, add `#[allow(dead_code)]` on the `Box` variant with a `// Task 2 wires attach_box` note and REMOVE it in Task 2.)

- [ ] **Step 4: Commit.**

```bash
git add src/ui/bottom_clip_guard.rs src/ui/mod.rs
git commit -m "feat(ui): BottomClipGuard — attach (TextView) + value_changed catch-all + recompute

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: `attach_box` (Box-child) + `on_open` (open-time coverage)

**Files:**
- Modify: `src/ui/bottom_clip_guard.rs`

**Interfaces:**
- Consumes: `crate::ui::recompute_overlay_bottom_clip_box(clip, scrolled)` (existing).
- Produces: `BottomClipGuard::attach_box(scroll_overlay, scrolled) -> Self`, `on_open(&self)`. Used by Tasks 3-5.

- [ ] **Step 1: Add `attach_box`.** In `impl BottomClipGuard`, after `attach`:

```rust
    /// Like `attach`, but for a scrolled window whose child is a widget BOX (no
    /// wrapped partial row — covers trailing slack only). Drives
    /// `recompute_overlay_bottom_clip_box`.
    pub(crate) fn attach_box(
        scroll_overlay: &Overlay,
        scrolled: &gtk4::ScrolledWindow,
    ) -> Self {
        let clip = build_clip_box();
        scroll_overlay.add_overlay(&clip);
        scroll_overlay.set_measure_overlay(&clip, false);
        scroll_overlay.set_clip_overlay(&clip, true);

        let guard = Self {
            kind: ClipKind::Box,
            clip: clip.clone(),
            scrolled: scrolled.clone(),
        };
        {
            let kind = guard.kind.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            scrolled.vadjustment().connect_value_changed(move |_| {
                recompute(&kind, &clip, &scrolled);
            });
        }
        guard
    }
```

- [ ] **Step 2: Add `on_open`** (path a — the open-time multi-pass coverage lifted from gloss's `reset_scroll_top`). In `impl BottomClipGuard`, after `recompute`:

```rust
    /// (a) Open-time coverage: snap to top, then keep recomputing the clip across
    /// the open's layout passes via a one-shot `connect_changed` (range-change)
    /// handler that self-disconnects after 250ms, plus an idle backstop. Mirrors
    /// the gloss overlay's `reset_scroll_top`. Call from every show/open path.
    pub(crate) fn on_open(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());

        let kind = self.kind.clone();
        let clip = self.clip.clone();
        let scrolled = self.scrolled.clone();

        // Pin the scroll to top across the open's layout passes, then release so
        // we stop fighting later user scrolls.
        let pinning = Rc::new(Cell::new(true));
        let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
        let id = adj.connect_changed({
            let pinning = pinning.clone();
            let kind = kind.clone();
            let clip = clip.clone();
            let scrolled = scrolled.clone();
            move |a| {
                if pinning.get() && a.value() != a.lower() {
                    a.set_value(a.lower());
                }
                recompute(&kind, &clip, &scrolled);
            }
        });
        *handler.borrow_mut() = Some(id);

        let adj_for_stop = adj.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            pinning.set(false);
            if let Some(hid) = handler.borrow_mut().take() {
                adj_for_stop.disconnect(hid);
            }
        });

        // Backstop: size the clip on first open even if `changed` never fires.
        let kind2 = kind;
        let clip2 = clip;
        let scrolled2 = scrolled;
        glib::idle_add_local_once(move || {
            recompute(&kind2, &clip2, &scrolled2);
        });
    }
```

- [ ] **Step 3: Remove any `#[allow(dead_code)]`** added in Task 1 Step 3 (the `Box` variant is now constructed by `attach_box`).

- [ ] **Step 4: Build + clippy.**

Run: `cargo build && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: clean build; `generated 118 warnings` (the guard is still unused by overlays until Tasks 3-5, so it may report its own pub-fns as never-used — if clippy exceeds 118 only due to `BottomClipGuard`'s not-yet-called methods, that's expected and resolves in Task 3; note it in the commit and confirm it returns to 118 after Task 3).

- [ ] **Step 5: Commit.**

```bash
git add src/ui/bottom_clip_guard.rs
git commit -m "feat(ui): BottomClipGuard attach_box + on_open (open-time multi-pass coverage)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Migrate the GLOSS overlay to the guard (behavior-preserving reference)

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

**Interfaces:**
- Consumes: `BottomClipGuard::{attach, clip, recompute, on_open}` (Tasks 1-2).

**CRITICAL:** Gloss clips correctly TODAY. This migration must be a pure refactor — same four mechanisms, now owned by the guard. Do NOT change visible behavior.

- [ ] **Step 1: Replace the struct field.** Change the `bottom_clip: gtk4::Box` field to `clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard`. (Find the field in the struct def and the `Self { … bottom_clip, … }` initializer.)

- [ ] **Step 2: Replace clip box creation + path (c)** (~gloss_overlay.rs:270-294). Delete the inline `bottom_clip` box creation, the `add_overlay`/`measure`/`clip` calls, AND the `{ … connect_value_changed … recompute_overlay_bottom_clip … }` block. Replace with:

```rust
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &gloss_scroll_overlay,
            &gloss_view,
            &gloss_scrolled,
        );
```

If any later construction code references `bottom_clip` (e.g. stacking the selection bar after it), use `clip_guard.clip()` instead. Store `clip_guard` in the `Self { … }` initializer (replacing `bottom_clip`).

- [ ] **Step 3: Replace `reset_scroll_top` body with `on_open`.** The `fn reset_scroll_top(&self)` body (the whole pin/connect_changed/timeout/idle block) becomes:

```rust
    fn reset_scroll_top(&self) {
        self.clip_guard.on_open();
    }
```

(Keep the `reset_scroll_top` method name + its 4 call sites unchanged — only its body delegates now.)

- [ ] **Step 4: Replace `update_bottom_clip` body with `recompute`.** `fn update_bottom_clip(&self)` (~1033) becomes:

```rust
    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }
```

(Its 4 call sites — 1380/1616/1624/1640 — stay unchanged.)

- [ ] **Step 5: Build + tests + clippy.**

Run: `cargo build && cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: clean; tests green; `generated 118 warnings` (now that the guard's methods are called, the not-used warnings clear).

- [ ] **Step 6: Commit.**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "refactor(gloss): own the bottom-clip lifecycle via BottomClipGuard

Behavior-preserving: the four hand-wired clip mechanisms (value_changed catch-all,
reset_scroll_top range+idle, update_bottom_clip) now delegate to the guard. Gloss
is the reference surface; no visible change.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 4: Migrate the JOURNAL overlay (gains the missing paths → FIXES THE BUG)

**Files:**
- Modify: `src/ui/journal_overlay.rs`

**Interfaces:**
- Consumes: `BottomClipGuard::{attach, clip, recompute, on_open}`.

**This is the bug fix.** Journal currently has NO path (c) clip recompute and NO path (a) open-time range handler. The guard supplies both. The selection-bar `value_changed` handler (147-150) STAYS — it is a separate concern.

- [ ] **Step 1: Replace the struct field.** Change `bottom_clip: gtk4::Box` (journal_overlay.rs:17) to `clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard`. Update the `Self { … }` initializer (replace `bottom_clip,` ~181 with `clip_guard,`).

- [ ] **Step 2: Replace clip box creation.** The inline `bottom_clip` box creation + `add_overlay`/`measure`/`clip` (~journal_overlay.rs:93-101) becomes:

```rust
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach(
            &scroll_overlay,
            &view,
            &scrolled,
        );
```

- [ ] **Step 3: Keep the selection-bar handler; ensure the bar stacks over the clip.** The selection-bar block (the `DrawingArea` + its own `connect_value_changed(|_| bar_for_scroll.queue_draw())` at 147-150, and `scroll_overlay.add_overlay(&bar_drawing)` etc.) STAYS. If it referenced `&bottom_clip` for ordering, it doesn't need to — `bar_drawing` is added to `scroll_overlay` after the guard's clip, so it already stacks above. Leave the bar handler intact.

- [ ] **Step 4: Replace `update_bottom_clip` with the guard recompute.** Journal's `fn update_bottom_clip(&self)` (the one calling `recompute_overlay_bottom_clip(&self.view,&self.bottom_clip,&self.scrolled)` ~457) becomes:

```rust
    fn update_bottom_clip(&self) {
        self.clip_guard.recompute();
    }
```

- [ ] **Step 5: Route open-time through `on_open`.** Journal's `show_page` (:254-255) and `show_passage_page` (:326-328) currently call `self.update_bottom_clip(); self.schedule_bottom_clip_recompute();`. Replace BOTH lines at each site with a single:

```rust
        self.clip_guard.on_open();
```

Then DELETE the now-unused `fn schedule_bottom_clip_recompute(&self)` method.

- [ ] **Step 6: Fix the ask-card open/close.** `open_ask_card` (:530) and `close_ask_card` (:539) call `self.schedule_bottom_clip_recompute();` — replace each with:

```rust
        self.clip_guard.recompute();
```

(With path (c) now wired, the ask resize fires `value_changed` and recomputes automatically; the explicit `recompute()` here is belt-and-suspenders for the synchronous case and harmless. Keep the `footer_container.set_visible(...)` lines unchanged.)

- [ ] **Step 7: Build + tests + clippy.**

Run: `cargo build && cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: clean; tests green; `generated 118 warnings`. Confirm `schedule_bottom_clip_recompute` is gone (`rg -n schedule_bottom_clip_recompute src/ui/journal_overlay.rs` → no matches).

- [ ] **Step 8: Commit.**

```bash
git add src/ui/journal_overlay.rs
git commit -m "fix(journal): own the bottom-clip lifecycle via BottomClipGuard

Journal previously had no value_changed clip recompute (its handler redrew only
the selection bar) and no open-time range handler, so the clip went stale when the
ask card resized the viewport — a partial last row showed behind the ask card. The
guard supplies path (c) + on_open; the selection-bar handler stays separate.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: Migrate the TRANSLATION overlay (Box-child) to the guard

**Files:**
- Modify: `src/ui/translation_overlay.rs`

**Interfaces:**
- Consumes: `BottomClipGuard::{attach_box, clip, recompute}`.

Translation already has path (c) (`connect_value_changed → recompute_overlay_bottom_clip_box`, 141-142) — this migration consolidates it onto the guard (no behavior change). Translation has no on_open today; ADDING it is a safe improvement (covers its open-time layout), but keep it minimal: wire `attach_box` + route the existing scroll recompute sites through the guard.

- [ ] **Step 1: Replace the struct field.** `bottom_clip: gtk4::Box` (:86) → `clip_guard: crate::ui::bottom_clip_guard::BottomClipGuard`. Update the `Self { … bottom_clip, … }` (~155) to `clip_guard,`.

- [ ] **Step 2: Replace clip box creation + path (c).** The inline box creation + `add_overlay`/`measure`/`clip` (:127-135) AND the `{ … connect_value_changed … recompute_overlay_bottom_clip_box … }` block (:139-142) become:

```rust
        let clip_guard = crate::ui::bottom_clip_guard::BottomClipGuard::attach_box(
            &scroll_overlay,
            &scrolled,
        );
```

- [ ] **Step 3: Route the other recompute site through the guard.** The second `recompute_overlay_bottom_clip_box(&clip, &sc)` site (~284-287, inside a scroll method) — replace its body with `self.clip_guard.recompute();`. (If it's inside a closure that clones, simplify to a direct `self.clip_guard.recompute();` call where `self` is available; otherwise keep a cloned-guard call — but the guard isn't Clone, so prefer calling `self.clip_guard.recompute()` directly at the call site, not inside a moved closure.)

- [ ] **Step 4: Build + tests + clippy.**

Run: `cargo build && cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: clean; tests green; `generated 118 warnings`.

- [ ] **Step 5: Commit.**

```bash
git add src/ui/translation_overlay.rs
git commit -m "refactor(translation): own the bottom-clip lifecycle via BottomClipGuard (attach_box)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 6: Extend the pixel clip-invariant e2e test to the journal overlay

**Files:**
- Modify/Create under `tests/` — model on `tests/overlay_clipping.rs` (the synopsis-overlay clip test) and `tests/harness/mod.rs`.

**Interfaces:**
- Consumes: the cage harness (`tests/harness/mod.rs`), `scripts/check_line_clipping.py`, the app's `TEST_*_VIEWPORT_RECT` logging.

**Note:** This is the defense-in-depth half. The journal overlay must log a viewport rect the harness can read (like `TEST_OVERLAY_VIEWPORT_RECT`). If the journal overlay does NOT already emit such a rect under `LIT_HEADLESS_TEST`, that emission must be added first (mirror `src/input/scroll.rs::emit_test_viewport_rect` / the gloss overlay's rect emit). Decide in Step 1.

- [ ] **Step 1: Check for a journal viewport-rect emit.** Run: `rg -n "TEST_.*VIEWPORT_RECT|LIT_HEADLESS_TEST" src/ui/journal_overlay.rs src/input/scroll.rs`. If the journal overlay emits no rect, add one on its reveal/show_page (under `LIT_HEADLESS_TEST`), logging `TEST_JOURNAL_VIEWPORT_RECT x y w h` in window==screenshot coords, mirroring the existing overlay rect emit. Build.

- [ ] **Step 2: Write the journal clip test.** Create `tests/journal_clipping.rs` modeled on `tests/overlay_clipping.rs`: launch under cage, drive keys to open a prose work's journal Q&A (the key sequence that opens the journal overlay — confirm via `src/input/keymap.rs`), scroll to the bottom (`j` repeatedly), then **press the ask-open key (`A`)** and screenshot. Read the journal viewport rect from the log, pass `--region` to `scripts/check_line_clipping.py`, assert no clipped first/last row WITH the ask card open (the exact regression). Gate `#[ignore]` like the others.

- [ ] **Step 3: Run it.**

Run: `./scripts/e2e-env.sh cargo test --test journal_clipping -- --ignored --nocapture`
Expected: PASS (no clip with ask open). If the harness cannot launch cage in this environment (seat busy / SIGTERM), STOP and report that runtime e2e is blocked — do NOT claim verified; hand the command to the user. (Per CLAUDE.md "When to ASK THE USER to run e2e-env.sh".)

- [ ] **Step 4: Commit.**

```bash
git add tests/journal_clipping.rs src/ui/journal_overlay.rs
git commit -m "test(e2e): journal Q&A clip invariant (ask card open) — catches a dropped clip path

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 7: Doc note + gate + user verification (handoff)

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md`

- [ ] **Step 1: Add a "BottomClipGuard owns the three paths" note.** In `clip-prevention.md`, after "When the clip MUST be recomputed (the three paths)", add a short paragraph: free-scroll surfaces now attach a `BottomClipGuard` (`src/ui/bottom_clip_guard.rs`) which wires path (c) in `attach()`/`attach_box()` and provides `on_open()` (path a) + `recompute()` (path b) — so a surface cannot drop a path. Point failure-checklist item #1 at "confirm the surface uses BottomClipGuard". Commit.

- [ ] **Step 2: Full suite + clippy.**

Run: `cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: green; `generated 118 warnings`.

- [ ] **Step 3: Hand the user the verification** (agent does not run the GUI):
  1. `cargo run`, open **Cromwell**, open the journal Q&A (long answer), press **A** — the answer text stops cleanly ABOVE the ask card (no partial row behind it). Press **Escape** — no partial row above the footer.
  2. Gloss synopsis (`h`) on any work — scroll `j`/`k`, both edges show whole lines (UNCHANGED — gloss is the reference).
  3. Translation (`i`) — page through, no clipped partial row.
  4. If available, run the e2e: `./scripts/e2e-env.sh cargo test --test journal_clipping -- --ignored --nocapture`.

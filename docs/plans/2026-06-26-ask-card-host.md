# Ask-card Host + Viewport-Shrink Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the ask card opens, make the scrolled viewport SHRINK (so reading text ends above the card, not behind it), and unify the duplicated ask-card hosting lifecycle into one shared `AskCardHost` — fixing journal's reported bug and gloss's identical latent bug at once.

**Architecture:** The overlay container is added to its `Overlay` with `set_measure_overlay(container, false)`, so it receives the overlay's full height and its `set_size_request(_, card_height)` acts only as a *minimum* — when the ask card opens the container grows past `card_height` and the `vexpand` scroll keeps its height (occlusion). The fix bounds the container to `card_height` so the box must shrink the scroll for the ask card. A new `AskCardHost` owns the clip recompute + optional footer toggle + the open/close lifecycle; both overlays delegate to it.

**Tech Stack:** Rust, GTK4 (`gtk4::Overlay`, `Box`, `ScrolledWindow`), the existing `BottomClipGuard` (`src/ui/bottom_clip_guard.rs`) and `AskCard` (`src/ui/ask_card.rs`).

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build`/`cargo test`. The on-screen result is USER-verified. (CLAUDE.md)
- `cargo test --bins` stays green; clippy must not increase (baseline 118).
- **The acceptance signal is measurable and must be confirmed by the user at runtime:** with the ask card OPEN, the scrolled `vadjustment().page_size()` must be SMALLER than with it CLOSED. Today they are EQUAL — that equality is the bug. A `LIT_HEADLESS_TEST`/diag-logged `page_size` before/after ask-open is the proof.
- Gloss is a working reference for everything EXCEPT this bug (it has the same latent bug). Its visible non-ask behavior (synopsis scroll/clip) must not regress.
- The ask card is the SHARED `AskCard` component; the fix + lifecycle live in shared code so BOTH overlays get them.
- The journal selection-bar handler and the `BottomClipGuard` are unchanged — the host COMPOSES the guard.
- Commit messages end with the Co-Authored-By: Claude Opus 4.8 / Claude-Session trailer.

## Key current code

- `src/ui/picker_attach.rs::attach_overlay_panel` — adds `container` via `overlay.add_overlay(container)` + `set_measure_overlay(container, false)`. THIS is why the container isn't height-bounded.
- `journal_overlay.rs`: `size_card` (`:201`) does `container.set_size_request(card_width, card_height)`; `open_ask_card` (`:523`) / `close_ask_card` (`:561`) hand-wire `ask.open`/`ask.close` + footer toggle + `clip_guard.recompute()`.
- `gloss_overlay.rs`: `set_width_request`+`set_height_request` per show; `open_ask_card_with` (`:1433`) / `close_ask_card` (`:1441`) / `schedule_ask_clip_recompute` (`:1450`).
- `ask_card.rs`: `AskCard` with `container()`, `open(title,hint,card_width)`, `close()`, `is_open()`, `toggle_focus()`, `take_text()`, `input()`.

---

### Task 1: SPIKE — find the GTK change that makes the viewport actually shrink

**This task is a throwaway diagnostic to DE-RISK the layout fix before building the host. Its output is knowledge + a confirmed one-line change, committed as the minimal journal-only fix; Tasks 2-4 generalize it.**

**Files:**
- Modify: `src/ui/journal_overlay.rs` (the container/size_card + a temp diag), and possibly `src/ui/picker_attach.rs`.

- [ ] **Step 1: Add a before/after `page_size` diagnostic.** In journal `open_ask_card`, after `self.ask.open(...)`, log `page_size` synchronously and on an idle; do the same in `close_ask_card`. (This is the same diagnostic that proved the bug.) Build.

```rust
// in open_ask_card and close_ask_card, after ask.open(...)/ask.close():
{
    let sc = self.scrolled.clone();
    crate::logging::log(&format!("ASKFIX page_size sync={:.0}", sc.vadjustment().page_size()));
    glib::idle_add_local_once(move || {
        crate::logging::log(&format!("ASKFIX page_size idle={:.0}", sc.vadjustment().page_size()));
    });
}
```

- [ ] **Step 2: Try the height-bounding change and HAND IT TO THE USER to measure.** The hypothesis: the container is not height-bounded because `attach_overlay_panel` adds it with `set_measure_overlay(false)` and it gets the overlay's full height; `size_request` is only a min. Candidate fixes to try (the implementer picks the FIRST that the user confirms shrinks `page_size`):
  - **(2a)** Wrap the card content in a height-bounded box: keep `container` at `valign=Center` but give the SCROLL area (or a wrapper holding title+scroll+footer+ask) a bounded height so the vexpand scroll can't exceed `card_height - (title+footer+ask)`. Concretely: set the container's height as a MAX, not just min — e.g. constrain via a fixed-height wrapper the scroll lives in.
  - **(2b)** Change `size_card` to ALSO cap the scroll: when the ask card is open, the scroll's allocated height must be `card_height - title - footer - ask_natural_height`. If GTK won't do it via container bounds, set the scroll's `max_content_height`/explicit height to the remaining space on ask-open and clear it on close.
  - **(2c)** Stop the container from over-allocating: in `attach_overlay_panel`, the container is `valign=Center` over the full overlay; if instead the container is sized to exactly `card_height` (a real bound, e.g. via a parent that clips/bounds it), the box must shrink the scroll.

  Because the agent CANNOT run the GUI, after implementing the most promising candidate (start with 2b — directly capping the scroll height is the most deterministic), the implementer MUST: build, `cargo test --bins` (460), then STOP and report **SPIKE-NEEDS-USER**: give the user the exact run + the grep:

```bash
cd ~/utono/linux-lit && cargo run
# open Cromwell -> journal Q&A -> press A -> Escape -> quit
grep ASKFIX ~/utono/linux-lit/linux-lit-dev.log | tail -8
```

  The user pastes the `page_size` numbers. SUCCESS = ask-open `page_size` is SMALLER than ask-closed (e.g. closed 1025, open ~820). If still equal, the candidate failed — try the next (2a/2c) and re-ask. Do NOT proceed to Task 2 until the user confirms a candidate shrinks the viewport.

- [ ] **Step 3: Once the user confirms a working change**, remove the `ASKFIX` diagnostic, keep the confirmed layout change, build + `cargo test --bins` + clippy (118). Commit `fix(journal): shrink the scrolled viewport when the ask card opens (no occlusion)` with the trailer, and record in the commit WHICH mechanism (2a/2b/2c) worked. This is the minimal, confirmed fix for journal; Tasks 2-4 lift it into the shared host and apply to gloss.

---

### Task 2: `AskCardHost` — own the lifecycle (composes BottomClipGuard + optional footer)

**Files:**
- Modify: `src/ui/ask_card.rs` — add `AskCardHost`.

**Interfaces:**
- Consumes: `AskCard` (existing), `crate::ui::bottom_clip_guard::BottomClipGuard`, the confirmed layout-shrink mechanism from Task 1.
- Produces: `AskCardHost` with `new(...)`, `open(title, hint, card_width)`, `close()`, `toggle_focus()`, `take_text()`, `is_open()`, `input()`, `card()` (the `&AskCard` / its container for attaching). Used by Tasks 3-4.

- [ ] **Step 1: Define `AskCardHost`.** It holds: the `AskCard`, the `ScrolledWindow` whose viewport must shrink, an optional `footer: Option<gtk4::Box>` to hide on open, and whatever Task 1 needs to apply/undo the viewport shrink (e.g. the scroll's saved height or the wrapper). Sketch (refine to Task 1's mechanism):

```rust
pub(crate) struct AskCardHost {
    ask: AskCard,
    scrolled: gtk4::ScrolledWindow,
    footer: Option<gtk4::Box>,
    // + whatever Task 1 confirmed it needs to shrink/restore the viewport
    recompute: std::rc::Rc<dyn Fn()>, // closure that calls the overlay's BottomClipGuard recompute
}

impl AskCardHost {
    pub(crate) fn new(
        ask: AskCard,
        scrolled: gtk4::ScrolledWindow,
        footer: Option<gtk4::Box>,
        recompute: std::rc::Rc<dyn Fn()>,
    ) -> Self { ... }

    pub(crate) fn open(&self, title: &str, hint: &str, card_width: i32) {
        self.ask.open(title, hint, card_width);
        // APPLY the Task-1 viewport-shrink so the scroll yields room for the card.
        // recompute the clip for the new (smaller) viewport.
        (self.recompute)();
        if let Some(f) = &self.footer { f.set_visible(false); }
    }

    pub(crate) fn close(&self) {
        self.ask.close();
        // UNDO the viewport-shrink (restore full height).
        (self.recompute)();
        if let Some(f) = &self.footer { f.set_visible(true); }
    }

    pub(crate) fn toggle_focus(&self) { self.ask.toggle_focus(); }
    pub(crate) fn take_text(&self) -> String { self.ask.take_text() }
    pub(crate) fn is_open(&self) -> bool { self.ask.is_open() }
    pub(crate) fn input(&self) -> &gtk4::TextView { self.ask.input() }
    pub(crate) fn container(&self) -> &gtk4::Box { self.ask.container() }
}
```

The `recompute` closure indirection lets the host call each overlay's existing `BottomClipGuard` without the host owning the guard (the guard isn't Clone; a boxed closure capturing cloned widgets is fine — same pattern as the gloss ask-recompute today).

- [ ] **Step 2: Build.** `cargo build` (the host is unused until Tasks 3-4 — expect "never used" warnings, which clear in Task 3). Do NOT run the app.

- [ ] **Step 3: Commit** `feat(ui): AskCardHost — uniform ask-card open/close lifecycle` with the trailer.

---

### Task 3: Migrate the JOURNAL overlay onto `AskCardHost`

**Files:**
- Modify: `src/ui/journal_overlay.rs`

- [ ] **Step 1: Construct an `AskCardHost`** in `new()`: pass the `ask`, the `scrolled`, `Some(footer_container.clone())`, and a `recompute` closure that calls `self.clip_guard.recompute()` (build it from cloned widgets, like the existing ask-recompute). Store it as a field (replace direct `ask` field usage where the host now owns it, or keep `ask` and add `ask_host`).

- [ ] **Step 2: Delegate the lifecycle methods.** `open_ask_card` → `self.ask_host.open(title, hint, card_width)` (drop the manual footer-hide + clip recompute — the host does them + the Task-1 shrink). `close_ask_card` → `self.ask_host.close()`. `toggle_ask_focus`/`take_ask_text`/`ask_is_open` → delegate to the host. Keep the public method names (keymap calls them).

- [ ] **Step 3: Move the Task-1 viewport-shrink into the host path.** The journal-only fix from Task 1 must now happen inside `ask_host.open/close` (so gloss gets it too in Task 4), not inline in the journal overlay. Ensure the journal no longer applies it directly.

- [ ] **Step 4: Build + tests + clippy + verify.** `cargo build && cargo test --bins` (460) `&& cargo clippy --bins` (118). Confirm no stray direct `ask.open`/`ask.close`/footer-toggle in journal (`rg -n "self.ask.open|self.ask.close|footer_container.set_visible" src/ui/journal_overlay.rs` → only inside the host construction / gone). Commit `refactor(journal): route ask-card lifecycle through AskCardHost` with the trailer.

---

### Task 4: Migrate the GLOSS overlay onto `AskCardHost` (fixes its latent bug)

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

- [ ] **Step 1: Construct an `AskCardHost`** in gloss `new()`: pass `ask`, `gloss_scrolled`, `None` (gloss has no footer), and a `recompute` closure calling `self.clip_guard.recompute()`. Because gloss routes through the host, it now ALSO gets the Task-1 viewport-shrink — fixing its identical latent occlusion.

- [ ] **Step 2: Delegate** `open_ask_card`/`open_ask_card_with` → `self.ask_host.open(...)`; `close_ask_card` → `self.ask_host.close()`; `toggle_ask_focus`/`take_ask_text`/`ask_is_open` → host. DELETE `schedule_ask_clip_recompute` (the host's open/close now own the recompute). Keep public names.

- [ ] **Step 3: Build + tests + clippy.** `cargo build && cargo test --bins` (460) `&& cargo clippy --bins` (118). `rg -n "schedule_ask_clip_recompute" src/ui/gloss_overlay.rs` → gone. Commit `fix(gloss): route ask-card through AskCardHost (shrinks viewport, no occlusion)` with the trailer.

---

### Task 5: Doc update + gate + user verification (handoff)

**Files:**
- Modify: `docs/troubleshooting/clip-prevention.md`

- [ ] **Step 1: Update the "occlusion is not clipping" note** to record the fix: the ask card now shrinks the scroll viewport via `AskCardHost` (the host bounds/caps the scroll so the vexpand area yields), so the rows are no longer occluded; both overlays share it. Commit.

- [ ] **Step 2: Full suite + clippy.** `cargo test --bins` (460) + clippy (118).

- [ ] **Step 3: Hand the user the verification** (agent cannot run the GUI):
  1. `cargo run`, open **Cromwell**, open journal Q&A (long answer), press **A** — answer text ends ABOVE the ask card (no row behind it); Escape — text fills back down.
  2. Open a long synopsis (`h`) on any work, open its ask card — synopsis text ends above the ask card (gloss latent bug fixed).
  3. Optional e2e (seat permitting): `./scripts/e2e-env.sh cargo test --test journal_clipping -- --ignored --nocapture` — should now PASS.

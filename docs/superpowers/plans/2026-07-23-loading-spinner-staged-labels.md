# Animated Loading Spinner + Staged Labels — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Animate the Q&A/gloss/synopsis loading indicator with a Braille spinner, and stage the journal ask as "Refining question…" → "Answering…" (holding the reader's raw question until the answer phase).

**Architecture:** A small reusable `LoadingAnimator` owns a `glib` timeout tick that, each frame, calls a caller-supplied `Fn(String)` sink to write `"{spinner} {label}"` (with the question body if any). Each overlay owns one animator and supplies a sink that targets its own loading widget — the journal overlay writes its `view` TextView buffer; the gloss overlay writes its `title` Label. Result-render paths call `animator.stop()` so no late tick repaints the answer.

**Tech Stack:** Rust, GTK4 (gtk4-rs), `glib::timeout_add_local`, the existing `JournalOverlay`/`GlossOverlay` loading paths.

## Global Constraints

- Braille spinner frames: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` (10 frames), ~120 ms/frame.
- Journal ask stage labels: **"Refining question…"** (extract-terms + improve phase, raw question held) then **"Answering…"** (answer phase, improved question). Use the ellipsis char `…` (`\u{2026}`), matching the existing `"Asking\u{2026}"`.
- The journal loading label lives in the **`view` TextView buffer** (`self.view.buffer().set_text`); the gloss/synopsis loading label lives in the **`title` Label** (`self.title.set_text`). The animator must target each via a sink closure — do NOT assume one widget type.
- PRIMARY RISK: a late tick must never repaint over the rendered result. Every result-render path (`show_page`, `show_message`, gloss render) calls `animator.stop()`; `start()` removes any prior source first; `stop()` is idempotent and removes the `glib::SourceId`.
- The tick closure holds NO `Rc<RefCell<AppState>>` borrow — it captures widget clones only.
- No keybind/legend/keymap surfaces are touched.
- Build with `cargo build`; do NOT run the app; do NOT `cargo run`. Headless-verify via cage.
- Shell aliases hang non-interactively: bypass with `command rm -f` / `\cp -f`.
- Commit trailers:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01M7TTE768j8p7NxgjzDyEqQ`

---

### Task 1: The `LoadingAnimator` (+ pure frame helper + unit test)

**Files:**
- Create: `src/ui/loading_animator.rs`
- Modify: `src/ui/mod.rs` (register the module)

**Interfaces:**
- Produces:
  - `pub(crate) fn spinner_frame(i: usize) -> char` — `SPINNER[i % 10]`.
  - `pub(crate) struct LoadingAnimator` with:
    - `pub fn new() -> Self`
    - `pub fn start(&self, sink: Rc<dyn Fn(String)>, body: String, label: String)` — installs a ~120 ms tick; each tick advances the frame and calls `sink(format!(...))`; paints frame 0 immediately before installing the timeout so the first paint isn't blank.
    - `pub fn stop(&self)` — removes the active source (idempotent).

- [ ] **Step 1: Write the failing unit test for the frame helper**

Create `src/ui/loading_animator.rs` with:

```rust
//! A small Braille-spinner animator for overlay loading states. Each tick
//! rewrites a caller-supplied text sink with the current spinner frame + a
//! label (and an optional held body above it). Used by the journal and gloss
//! overlays; stopped by the result-render paths so a late tick never repaints
//! over the answer.

use std::cell::RefCell;
use std::rc::Rc;

/// The 10 Braille spinner frames, cycled ~every 120 ms.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The spinner glyph for frame `i` (wraps every 10).
pub(crate) fn spinner_frame(i: usize) -> char {
    SPINNER[i % SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_wraps_every_ten_frames() {
        assert_eq!(spinner_frame(0), '⠋');
        assert_eq!(spinner_frame(9), '⠏');
        assert_eq!(spinner_frame(10), '⠋'); // wraps
        assert_eq!(spinner_frame(23), spinner_frame(3));
    }
}
```

- [ ] **Step 2: Register the module and run the test to confirm it passes**

In `src/ui/mod.rs`, add near the other `pub mod` lines (e.g. after `pub mod ask_card;`):

```rust
pub mod loading_animator;
```

Run: `cargo test --bins loading_animator -- --nocapture`
Expected: `spinner_wraps_every_ten_frames ... ok`.

- [ ] **Step 3: Add the animator struct**

Append to `src/ui/loading_animator.rs` (after the `spinner_frame` fn, before `#[cfg(test)]`):

```rust
/// Owns the active spinner tick. `start` installs a 120 ms `glib` timeout that
/// advances the frame and repaints via `sink`; `stop` removes it. Idempotent.
pub(crate) struct LoadingAnimator {
    source: RefCell<Option<gtk4::glib::SourceId>>,
    frame: std::cell::Cell<usize>,
}

impl LoadingAnimator {
    pub fn new() -> Self {
        Self { source: RefCell::new(None), frame: std::cell::Cell::new(0) }
    }

    /// Start animating: `sink(text)` receives the full text to display each
    /// frame — `"{body}\n\n{spinner} {label}"`, or `"{spinner} {label}"` when
    /// `body` is empty. Paints frame 0 immediately, then ticks every 120 ms.
    pub fn start(&self, sink: Rc<dyn Fn(String)>, body: String, label: String) {
        self.stop();
        self.frame.set(0);
        let render = {
            let sink = Rc::clone(&sink);
            let body = body.clone();
            let label = label.clone();
            move |i: usize| {
                let g = spinner_frame(i);
                let text = if body.is_empty() {
                    format!("{g} {label}")
                } else {
                    format!("{body}\n\n{g} {label}")
                };
                sink(text);
            }
        };
        // Immediate first paint (frame 0) so there is no blank gap.
        render(0);
        let frame_cell = self.frame.clone_for_tick();
        let id = gtk4::glib::timeout_add_local(
            std::time::Duration::from_millis(120),
            move || {
                let next = frame_cell.get().wrapping_add(1);
                frame_cell.set(next);
                render(next);
                gtk4::glib::ControlFlow::Continue
            },
        );
        *self.source.borrow_mut() = Some(id);
    }

    /// Stop animating and drop the timeout source. Safe to call when not
    /// running (idempotent) — the result-render paths call this before painting
    /// the answer so a queued tick can never repaint over it.
    pub fn stop(&self) {
        if let Some(id) = self.source.borrow_mut().take() {
            id.remove();
        }
    }
}
```

NOTE on `frame_cell.clone_for_tick()`: `std::cell::Cell<usize>` is not `Clone`-shareable into the `'static` timeout closure. Replace the two lines
`let frame_cell = self.frame.clone_for_tick();` … and the closure's use of it with a shared counter that IS `'static`: use `let frame_cell = Rc::new(std::cell::Cell::new(0usize));` declared BEFORE the timeout, move a clone into the closure, and DROP the `self.frame` field entirely (it was only for this). Concretely:

- Remove the `frame: std::cell::Cell<usize>` field and its `new()` init.
- In `start`, before installing the timeout: `let frame_cell = Rc::new(std::cell::Cell::new(0usize));`
- Move `Rc::clone(&frame_cell)` into the timeout closure; the closure does `let next = fc.get() + 1; fc.set(next); render(next);`.
- `render(0)` still fires the immediate first paint.

(The `frame` field is unnecessary once the counter lives in the `Rc` captured by the closure; the animator only needs `source` to stop it.)

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: `Finished`. (If a borrow/lifetime error appears on the closure, confirm `sink`, `body`, `label`, and the `Rc<Cell>` counter are all moved/cloned into the `'static` timeout closure — nothing borrowed from `&self`.)

- [ ] **Step 5: Run the frame test again + commit**

Run: `cargo test --bins loading_animator`
Expected: PASS.

```bash
git add src/ui/loading_animator.rs src/ui/mod.rs
git commit -m "feat(ui): LoadingAnimator — Braille spinner tick for loading states"
```

---

### Task 2: Journal overlay — animated, staged `show_loading`

**Files:**
- Modify: `src/ui/journal_overlay.rs` (animator field; `show_loading` gains `label`; stop on result render)

**Interfaces:**
- Consumes: `LoadingAnimator` (Task 1).
- Produces: `JournalOverlay::show_loading(&self, question: &str, label: &str)` (was `(&self, question: &str)`).

- [ ] **Step 1: Add the animator field**

In `struct JournalOverlay` (`src/ui/journal_overlay.rs`), add a field:

```rust
    loading_animator: crate::ui::loading_animator::LoadingAnimator,
```

In its constructor (`JournalOverlay::new`, in the final `Self { ... }` / struct-literal return), initialize:

```rust
        loading_animator: crate::ui::loading_animator::LoadingAnimator::new(),
```

(Find the struct literal that returns the overlay — grep `JournalOverlay {` or the `Self {` near the end of `new`.)

- [ ] **Step 2: Rewrite `show_loading` to take a label + animate**

Replace the current `show_loading` (around line 950):

```rust
    pub fn show_loading(&self, question: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        let body = if question.trim().is_empty() {
            "Asking\u{2026}".to_string()
        } else {
            format!("{}\n\nAsking\u{2026}", prefix_question(question))
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask_host.card().close();
        self.clear_blocks();
        self.footer_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }
```

with (adds `label`, starts the animator writing the `view` buffer, holds the question as the body):

```rust
    pub fn show_loading(&self, question: &str, label: &str) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.apply_font();
        self.ask_host.card().close();
        self.clear_blocks();
        self.footer_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        // Animate the indicator: the sink writes the view buffer each frame.
        // Body = the held question (empty → indicator only). The animator paints
        // frame 0 immediately, so the first paint is correct before any tick.
        let body = if question.trim().is_empty() {
            String::new()
        } else {
            prefix_question(question)
        };
        let view = self.view.clone();
        let sink: std::rc::Rc<dyn Fn(String)> =
            std::rc::Rc::new(move |text: String| view.buffer().set_text(&text));
        self.loading_animator.start(sink, body, label.to_string());
    }
```

- [ ] **Step 3: Stop the animator on every result-render path**

The result renders replace the loading buffer with the real page/message. Add `self.loading_animator.stop();` as the FIRST line of each of these `JournalOverlay` methods so no queued tick repaints the answer:
- `show_page` (the main answer render)
- `show_message` (error/short-message render)
- `show_passage_source` (pending-passage render, if it can follow a loading state)

Locate them: `rg -n "pub fn show_page|pub fn show_message|pub fn show_passage_source" src/ui/journal_overlay.rs`. For each, insert `self.loading_animator.stop();` immediately after the `{`.

Also stop it in `hide()` (the universal close funnel) so leaving the overlay mid-load cannot leave a tick running: add `self.loading_animator.stop();` in `JournalOverlay::hide()`.

- [ ] **Step 4: Update the two journal.rs call sites (staged labels + raw-vs-improved body)**

In `src/input/actions/journal.rs`:

(a) `submit_passage_question` (~line 2272) — the refine phase; hold the RAW question:

Replace:
```rust
        s.journal_overlay.show_loading(text);
```
with:
```rust
        s.journal_overlay.show_loading(text, "Refining question\u{2026}");
```

(b) `ask_claude` (~line 2610) — the answer phase; the `question` here is the improved question:

Replace:
```rust
        s.journal_overlay.show_loading(question);
```
with:
```rust
        s.journal_overlay.show_loading(question, "Answering\u{2026}");
```

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: `Finished` (no "missing argument" errors — both call sites updated; no other caller of `JournalOverlay::show_loading` exists — confirm with `rg -n "journal_overlay.show_loading|\.show_loading\(" src/input/actions/journal.rs`).

- [ ] **Step 6: Run the bin suite**

Run: `cargo test --bins 2>&1 | rg "test result"`
Expected: `ok. N passed; 0 failed`.

- [ ] **Step 7: Commit**

```bash
git add src/ui/journal_overlay.rs src/input/actions/journal.rs
git commit -m "feat(journal): animated spinner + Refining/Answering staged loading"
```

---

### Task 3: Gloss/synopsis overlay — animate the loading title

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (animator field; animate `show_loading_message`; stop on render)

**Interfaces:**
- Consumes: `LoadingAnimator` (Task 1).
- Produces: no signature change — `show_loading()` / `show_loading_message(&str)` keep their shapes; their bodies now animate.

- [ ] **Step 1: Add the animator field**

In `struct GlossOverlay`, add:

```rust
    loading_animator: crate::ui::loading_animator::LoadingAnimator,
```

Initialize in `GlossOverlay::new`'s returning struct literal:

```rust
        loading_animator: crate::ui::loading_animator::LoadingAnimator::new(),
```

- [ ] **Step 2: Animate `show_loading_message`**

The gloss loading label is the `title` Label (`self.title.set_text(message)`), NOT a TextView buffer. In `show_loading_message` (~line 3153), the method currently ends by setting `self.title.set_text(message)` and styling it. AFTER the existing title setup (keep the sizing/style/visibility code as-is), start the animator with an empty body and the message as the label, writing the `title` label each frame:

Add at the END of `show_loading_message`, after the existing `self.title.set_*` calls:

```rust
        // Animate the label: the sink writes the title Label each frame. Empty
        // body → the title shows just "<spinner> <message>". The animator paints
        // frame 0 immediately.
        let title = self.title.clone();
        let sink: std::rc::Rc<dyn Fn(String)> =
            std::rc::Rc::new(move |text: String| title.set_text(&text));
        // `message` may carry a trailing "..."/"…" already (e.g. "Glossing...").
        // Strip a trailing ellipsis/dots so the animated label reads
        // "<spinner> Glossing" without doubled dots; keep the word(s).
        let label = message
            .trim_end_matches(|c| c == '.' || c == '\u{2026}')
            .trim_end()
            .to_string();
        self.loading_animator.start(sink, String::new(), label);
```

(Result: the title animates as `⠋ Glossing`, `⠙ Glossing`, … — the spinner replaces the static dots. `show_loading()` calls `show_loading_message("Glossing...")`, so it inherits this; the `Synthesizing…` synopsis caller likewise.)

- [ ] **Step 3: Stop the animator on the gloss result-render + hide paths**

Add `self.loading_animator.stop();` as the first line of the gloss overlay's result-render method(s) — the ones that paint the real gloss/synopsis over the loading title. Locate: `rg -n "pub fn show_page|pub fn render|pub fn show_gloss|pub fn show_synopsis|pub fn hide" src/ui/gloss_overlay.rs`. Add the stop to the render path(s) that replace the loading title with content, AND to `GlossOverlay::hide()`.

If uncertain which render method is the "result" path, the safe superset: add `self.loading_animator.stop();` to `hide()` and to whichever method sets `self.title.set_visible(false)` or repopulates the gloss blocks after a load (that is the render that supersedes the loading title). Confirm by reading the method that `open_gloss_at_cursor`'s success path calls.

- [ ] **Step 4: Build + test**

Run: `cargo build`
Expected: `Finished`.
Run: `cargo test --bins 2>&1 | rg "test result"`
Expected: `ok. N passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): animate the Glossing/Synthesizing loading label"
```

---

### Task 4: Headless verification

**Files:** none (cage/grim/wtype flow).

- [ ] **Step 1: Build + launch under cage**

```bash
cd <worktree-root> && cargo build
pkill -f "cage -- ./target/debug/linux-lit" 2>/dev/null; sleep 1
LIT_LOG_PATH=/tmp/spin.log LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
sleep 5
export WAYLAND_DISPLAY=$(command ls -t /run/user/1000/wayland-* | grep -vE '\.lock|wayland-0$' | head -1 | xargs basename) XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

- [ ] **Step 2: Journal ask — capture both staged phases + the result**

Confirm binds first (`Ctrl+j` journal, `Ctrl+a` ask). Drive:

```bash
wtype -M ctrl -k j -m ctrl; sleep 2
wtype -M ctrl -k a -m ctrl; sleep 2
wtype "what is the tone of this passage"; sleep 1
wtype -M ctrl -k Return -m ctrl
# capture the refine phase fast (raw Q held + spinner + "Refining question…")
sleep 1; grim -o HEADLESS-1 /tmp/spin-refine.png
# capture the answer phase (improved Q + spinner + "Answering…")
sleep 4; grim -o HEADLESS-1 /tmp/spin-answer.png
# capture the final result (answer rendered, NO spinner/label residue)
sleep 12; grim -o HEADLESS-1 /tmp/spin-done.png
```

Read all three. Confirm:
- `spin-refine.png`: shows your RAW question + a Braille spinner glyph (one of `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) + "Refining question…".
- `spin-answer.png`: shows the IMPROVED question + a spinner glyph + "Answering…".
- `spin-done.png`: the answer is rendered full-width with NO leftover spinner glyph or "Answering…" line (the animator stopped cleanly).

- [ ] **Step 3: Gloss loading — confirm the label animates**

Drive a gloss that generates (a prose passage with no cached gloss, so it shows "Glossing…"), and capture a frame:

```bash
wtype -k Escape; sleep 1   # leave journal
# navigate to a prose passage / trigger a gloss generation (confirm the gloss key
# and a no-cached-gloss target in keymap_config.rs), then:
grim -o HEADLESS-1 /tmp/spin-gloss.png
```

Read `spin-gloss.png`: the loading title reads `<spinner glyph> Glossing` (spinner glyph present, no doubled dots). If a generating gloss target is hard to reach headless, note it and rely on the journal captures + the code path (the gloss uses the same animator).

- [ ] **Step 4: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 5: No commit (verification only)**

---

## Self-Review

**Spec coverage:**
- Braille spinner on all three surfaces → Task 1 (animator) + Task 2 (journal sink) + Task 3 (gloss sink). ✓
- Staged journal labels (Refining/Answering) + raw-then-improved body → Task 2 Step 4. ✓
- Stop-on-result (primary risk) → Task 2 Step 3 + Task 3 Step 3 (render paths + hide). ✓
- Journal buffer vs gloss title-label difference → Task 2 uses `view.buffer()` sink, Task 3 uses `title` sink. ✓
- No AppState borrow in the tick → animator captures widget clones only (Task 1). ✓
- Headless verification of both phases + clean stop → Task 4. ✓

**Placeholder scan:** No TBD/TODO. Task 1 Step 3 flags the `Cell`-into-`'static`-closure pitfall and prescribes the `Rc<Cell>` counter fix explicitly. Task 3 Step 3 gives a concrete "safe superset" when the exact render method is ambiguous, resolved by reading the success path.

**Type consistency:** `LoadingAnimator::start(Rc<dyn Fn(String)>, String, String)` / `stop()` / `spinner_frame(usize)->char` used identically in Tasks 2 and 3. `show_loading(&self, question:&str, label:&str)` matches the two updated call sites. Sinks are `Rc<dyn Fn(String)>` in both overlays.

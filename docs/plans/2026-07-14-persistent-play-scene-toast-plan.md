# Persistent Play/Chapter Scene Toast Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `+` (`ShowCurrentChapter`) toast persist as a live "you are here" indicator for plays and prose-with-chapters — following the cursor, toggling off on re-press, and yielding only to the search toast.

**Architecture:** Add a `chapter_toast_persistent` flag and a `chapter_toast_persists()` predicate to `AppState`. `show_current_chapter` becomes a toggle for persisting works, reusing a factored-out `compute_current_chapter_text`. A `refresh_persistent_chapter_toast` helper rides the existing per-navigation `update_title_bar_scene` call sites so the toast text tracks the cursor. The two search-toast sites hide the persistent toast; the next navigation re-shows it.

**Tech Stack:** Rust, GTK4 (gtk4-rs / sourceview5), existing `AppState` + `glib` toast machinery.

## Global Constraints

- Persistence is enabled ONLY for: a play (`work_type == "play"`) OR prose with ≥1 chapter marker. Front-matter-only prose, non-play verse (`poem`, `sonnet_sequence`), and anthology keep the unchanged transient 3-second toast.
- Boundaries/scene labels come from authoritative `(div1, div2)` metadata — never inferred from buffer text (repo rule).
- Do NOT modify the shared `ui/toast.rs::show_transient` helper — it is used by many toasts; scope changes to the search call sites only.
- Verify GUI/behavior on the real render per `docs/troubleshooting/clip-prevention.md`; do NOT run `cargo run` (the user launches the app). Build with `cargo build`; unit-test with `cargo test`.
- `+` is keysym `plus`; the action is `ShowCurrentChapter`, also bound to `C`. Both get the toggle.

---

### Task 1: `is_play()` predicate + unit test

**Files:**
- Modify: `src/app/mod.rs` (add method near `is_anthology`, `src/app/mod.rs:751`)
- Test: `src/app/mod.rs` (new `#[cfg(test)]` module, mirroring the existing style at `src/app/mod.rs:4789`)

**Interfaces:**
- Produces: `AppState::is_play(&self) -> bool` — true iff a work is loaded and `work_type == "play"`.
- Produces (test helper): a free function `work_type_is_play(work_type: &str) -> bool` so the predicate's core is unit-testable without constructing an `AppState`.

- [ ] **Step 1: Write the failing test**

Add at the end of `src/app/mod.rs`:

```rust
#[cfg(test)]
mod is_play_tests {
    use super::work_type_is_play;

    #[test]
    fn play_is_play() {
        assert!(work_type_is_play("play"));
    }

    #[test]
    fn non_play_types_are_not_play() {
        for t in ["poem", "sonnet_sequence", "novel", "prose", "prose_book", "essay_collection", "anthology"] {
            assert!(!work_type_is_play(t), "{t} must not be a play");
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib work_type_is_play 2>&1 | tail -20`
Expected: FAIL to COMPILE — `cannot find function work_type_is_play`.

- [ ] **Step 3: Write minimal implementation**

Add the free function just above `impl AppState`'s `is_anthology` region (anywhere at module scope; place it directly above the `impl AppState` block that contains `is_prose`). A convenient spot is immediately before the `is_prose` method's `impl` — but since methods live inside `impl AppState`, add the FREE function at module top-level (e.g. right after the `use` block or just before the `impl AppState`). Concretely, add near `src/app/mod.rs:725` (just before `pub fn is_prose`):

```rust
// (module-level, NOT inside impl AppState)
/// Core of `AppState::is_play`, split out so it is unit-testable without an
/// `AppState`. A "play" is `work_type == "play"` exactly — NOT the whole
/// `!is_prose()` set (poem / sonnet_sequence / anthology are excluded).
pub(crate) fn work_type_is_play(work_type: &str) -> bool {
    work_type == "play"
}
```

Then add the method inside `impl AppState`, immediately after `is_anthology` (after `src/app/mod.rs:755`):

```rust
    /// True only for a play (`work_type == "play"`). Distinct from `!is_prose()`,
    /// which also matches poem / sonnet_sequence / anthology. Used to decide
    /// whether the `+` chapter toast persists.
    pub fn is_play(&self) -> bool {
        self.current_work.as_ref()
            .map(|w| work_type_is_play(&w.work_type))
            .unwrap_or(false)
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib work_type_is_play 2>&1 | tail -20`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(app): add is_play predicate for the persistent chapter toast"
```

---

### Task 2: `chapter_toast_persists()` predicate

**Files:**
- Modify: `src/app/mod.rs` (add method after `is_play`)

**Interfaces:**
- Consumes: `AppState::is_play` (Task 1), `AppState::is_prose` (`src/app/mod.rs:726`), `state.line_map: Option<LineMap>` (`src/app/mod.rs:415`, field `chapter_breaks: Vec<usize>`), `state.current_work.lines[*].is_chapter` (`src/db/models.rs:48`).
- Produces: `AppState::chapter_toast_persists(&self) -> bool` — true for a play, or prose with ≥1 chapter marker.

This predicate depends on GTK/DB state (`line_map`, `current_work`), so it is covered by e2e (Task 7), not a unit test. Keep it a thin composition of already-tested pieces.

- [ ] **Step 1: Write the implementation**

Add inside `impl AppState`, immediately after `is_play`:

```rust
    /// True when the `+` chapter toast should PERSIST (live "you are here"
    /// indicator) instead of auto-dismissing: a play, or prose that actually
    /// has chapter markers. Front-matter-only prose (no markers), non-play
    /// verse, and anthology are false — they keep the transient toast. The
    /// "has chapters" test mirrors `show_current_chapter`: prefer the line
    /// map's `chapter_breaks`, else scan `is_chapter` on the work lines.
    pub fn chapter_toast_persists(&self) -> bool {
        if self.is_play() {
            return true;
        }
        if !self.is_prose() {
            return false;
        }
        let has_chapters = if let Some(ref lm) = self.line_map {
            !lm.chapter_breaks.is_empty()
        } else {
            self.current_work
                .as_ref()
                .map(|w| w.lines.iter().any(|l| l.is_chapter))
                .unwrap_or(false)
        };
        has_chapters
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean (a `dead_code` warning on `chapter_toast_persists` is acceptable until Task 4 wires it — do NOT add `#[allow]`; the next task consumes it).

- [ ] **Step 3: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(app): add chapter_toast_persists predicate (play or prose-with-chapters)"
```

---

### Task 3: `chapter_toast_persistent` state flag

**Files:**
- Modify: `src/app/mod.rs` (add field near `chapter_toast_gen` `src/app/mod.rs:669`; init in the struct literal near `src/app/mod.rs:1954`; reset in `display_work` `src/app/mod.rs:2897`)

**Interfaces:**
- Produces: `AppState.chapter_toast_persistent: std::rc::Rc<std::cell::Cell<bool>>` — the single source of truth for whether the toast is currently a persistent indicator. Cloneable `Rc<Cell<_>>` so timer/closure sites can read it (matches `chapter_toast_gen`'s `Rc<Cell<u64>>` shape at `src/app/mod.rs:669`).

- [ ] **Step 1: Add the struct field**

In the `AppState` struct, immediately after the `chapter_toast_gen` field (`src/app/mod.rs:669`):

```rust
    /// True while the `+` chapter/scene toast is a PERSISTENT live indicator
    /// (plays + prose-with-chapters). When true the toast has no auto-hide
    /// timer, its text is refreshed on every cursor move
    /// (`navigation::refresh_persistent_chapter_toast`), and it is re-shown
    /// after a search toast borrows the bottom strip. Reset on work switch.
    pub chapter_toast_persistent: Rc<Cell<bool>>,
```

- [ ] **Step 2: Initialize it in the struct literal**

In the `AppState { ... }` construction, immediately after `chapter_toast_gen: Rc::new(Cell::new(0)),` (`src/app/mod.rs:1954`):

```rust
        chapter_toast_persistent: Rc::new(Cell::new(false)),
```

- [ ] **Step 3: Reset it on work switch**

Read `display_work` and `display_work_at_with_prepared` (`src/app/mod.rs:2897`, `:2913`) to find where per-work state is reset (where `current_work`/`current_line` are assigned). Add, at the start of the shared display path (in `display_work_at_with_prepared`, right after the new work is installed):

```rust
    // A persistent chapter toast belongs to one work; never leak it across a
    // work switch. Clear the flag and hide the pill.
    state.chapter_toast_persistent.set(false);
    state.chapter_toast.set_visible(false);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs
git commit -m "feat(app): add chapter_toast_persistent flag, reset on work switch"
```

---

### Task 4: Factor out `compute_current_chapter_text`; make `+` a toggle

**Files:**
- Modify: `src/input/navigation.rs` (`show_current_chapter` `src/input/navigation.rs:2347`; `show_chapter_toast` `src/input/navigation.rs:2468`)

**Interfaces:**
- Consumes: `AppState::chapter_toast_persists` (Task 2), `state.chapter_toast_persistent` (Task 3), `scene_synopsis::current_scene_divs` (`src/app/scene_synopsis.rs:102`), `scene_label_for` (`:455`), `prose_chapter_numbering` (`src/input/navigation.rs:2453`).
- Produces: `pub(crate) fn compute_current_chapter_text(state: &AppState) -> String` — the exact toast text for the current cursor (scene label for plays/verse, "Chapter N of M — title" / "Front matter" for prose). `pub(crate) fn show_chapter_toast_persistent(state: &AppState, text: &str)` — sets text + visibility, bumps `chapter_toast_gen`, installs NO hide timer.

- [ ] **Step 1: Extract `compute_current_chapter_text`**

`show_current_chapter` (`src/input/navigation.rs:2347-2446`) currently builds text inline across three branches and calls `show_chapter_toast` in each. Refactor so ALL text-building lives in one function. Replace the body of `show_current_chapter` from line 2358 (the comment above `if !state.is_prose()`) through the final `show_chapter_toast(state, &text);` (`:2445`) so that the text-building moves into a new function and `show_current_chapter` only decides transient-vs-toggle.

Add this new function immediately above `show_current_chapter` (`src/input/navigation.rs:2347`):

```rust
/// Build the chapter/scene toast text for the current cursor position — the
/// SAME string a fresh `+` shows. Plays/verse get the authoritative act/scene
/// label; prose with chapter markers gets "Chapter N of M — title"; prose
/// front matter gets "Front matter — title"; prose without markers falls back
/// to the scene label. Boundaries come from `(div1, div2)` metadata, never
/// from buffer text (see CLAUDE.md → authoritative-boundary principle).
pub(crate) fn compute_current_chapter_text(state: &AppState) -> String {
    let (abbrev, work) = match &state.current_work {
        Some(w) => (w.abbrev.clone(), w),
        None => return String::new(),
    };

    // Plays/verse: authoritative act/scene label, never "Chapter N of M".
    if !state.is_prose() {
        let (div1, div2) = crate::app::scene_synopsis::current_scene_divs(state);
        let label = crate::app::scene_synopsis::scene_label_for(state, div1, div2);
        return format!("{} — {}", abbrev, label);
    }

    let chapter_lines: Vec<usize> = if let Some(ref lm) = state.line_map {
        lm.chapter_breaks.clone()
    } else {
        work.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_chapter)
            .map(|(i, _)| i)
            .collect()
    };

    // Prose without chapter markers: scene-label fallback.
    if chapter_lines.is_empty() {
        let (div1, div2) = crate::app::scene_synopsis::current_scene_divs(state);
        let label = crate::app::scene_synopsis::scene_label_for(state, div1, div2);
        return format!("{} — {}", abbrev, label);
    }

    let work_line_text = |bl: usize| -> &str {
        if let Some(ref lm) = state.line_map {
            lm.buffer_to_work.get(bl)
                .and_then(|o| o.as_ref())
                .map(|wi| work.lines[*wi].text.as_str())
                .unwrap_or("")
        } else {
            work.lines.get(bl).map(|l| l.text.as_str()).unwrap_or("")
        }
    };

    let current_bl = state.current_line;
    let (div1, _) = crate::app::scene_synopsis::current_scene_divs(state);
    let max_div1 = work.lines.iter().map(|l| l.div1).max().unwrap_or(0);

    let title = match chapter_lines.iter().rposition(|&bl| bl <= current_bl) {
        Some(idx) => work_line_text(chapter_lines[idx]).trim().to_string(),
        None => {
            (0..=current_bl)
                .rev()
                .map(|bl| work_line_text(bl).trim())
                .find(|t| {
                    let lower = t.to_lowercase();
                    lower.contains("chapter") || lower.contains("part ")
                })
                .filter(|t| !t.is_empty())
                .map(|t| t.to_string())
                .unwrap_or_else(|| work.title.clone())
        }
    };

    match prose_chapter_numbering(div1, max_div1) {
        Some((num, total)) => format!("{} — Chapter {} of {} — {}", abbrev, num, total, title),
        None => format!("{} — Front matter — {}", abbrev, work.title),
    }
}
```

- [ ] **Step 2: Rewrite `show_current_chapter` as a toggle**

Replace the whole body of `show_current_chapter` (`src/input/navigation.rs:2347-2446`) with:

```rust
pub fn show_current_chapter(state: &mut AppState) {
    if state.current_work.is_none() {
        log_fmt!("SHOW_CHAPTER (+): no current work — nothing to show");
        return;
    }
    log_fmt!("SHOW_CHAPTER (+): current_line={} is_prose={} persists={}",
        state.current_line, state.is_prose(), state.chapter_toast_persists());

    // Persisting works (plays + prose-with-chapters): `+` toggles a live
    // "you are here" indicator that follows the cursor. Non-persisting works
    // keep the transient 3-second toast.
    if state.chapter_toast_persists() {
        if state.chapter_toast_persistent.get() {
            // Toggle OFF: bump the generation (defensive) and hide.
            state.chapter_toast_persistent.set(false);
            state.chapter_toast_gen.set(state.chapter_toast_gen.get().wrapping_add(1));
            state.chapter_toast.set_visible(false);
            log_fmt!("CHAPTER_TOAST: persistent OFF");
            return;
        }
        state.chapter_toast_persistent.set(true);
        let text = compute_current_chapter_text(state);
        show_chapter_toast_persistent(state, &text);
        log_fmt!("CHAPTER_TOAST: persistent ON text={:?}", text);
        return;
    }

    let text = compute_current_chapter_text(state);
    show_chapter_toast(state, &text);
}
```

- [ ] **Step 3: Add the timer-less `show_chapter_toast_persistent`**

Add immediately after `show_chapter_toast` (after `src/input/navigation.rs:2487`):

```rust
/// Like `show_chapter_toast` but with NO auto-hide timer — the toast stays up
/// until the persistent flag is toggled off or a work switch clears it. Bumps
/// `chapter_toast_gen` so any in-flight transient hide-timer becomes a no-op.
pub(crate) fn show_chapter_toast_persistent(state: &AppState, text: &str) {
    let gen = state.chapter_toast_gen.get().wrapping_add(1);
    state.chapter_toast_gen.set(gen);
    log_fmt!("CHAPTER_TOAST: show persistent gen={} text={:?}", gen, text);
    state.chapter_toast.set_text(text);
    state.chapter_toast.set_visible(true);
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -30`
Expected: builds clean. If the compiler flags the now-unused `prose_chapter_numbering` privacy or an unused import, fix minimally (it is still used by `compute_current_chapter_text`).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: PASS (Task 1's `is_play` tests still green; no regressions).

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(nav): + toggles a persistent chapter toast for plays and prose-with-chapters"
```

---

### Task 5: Live "you are here" refresh on cursor move

**Files:**
- Modify: `src/input/navigation.rs` (add `refresh_persistent_chapter_toast`)
- Modify: `src/input/highlight.rs` (call it next to `update_title_bar_scene` at `src/input/highlight.rs:446` and `:486`)

**Interfaces:**
- Consumes: `state.chapter_toast_persistent` (Task 3), `compute_current_chapter_text` (Task 4).
- Produces: `pub(crate) fn refresh_persistent_chapter_toast(state: &AppState)` — when the flag is on, recompute the text and keep the pill visible; otherwise no-op. Deliberately independent of title-bar visibility (unlike `update_title_bar_scene`, which early-returns when the title bar is hidden, `src/app/scene_synopsis.rs:567`).

- [ ] **Step 1: Add the refresh helper**

Add immediately after `show_chapter_toast_persistent` in `src/input/navigation.rs`:

```rust
/// Keep the persistent chapter toast in sync with the cursor: recompute its
/// text and hold it visible. No-op when the toast is not in persistent mode.
/// Rides the per-navigation `update_title_bar_scene` sites but is a SEPARATE
/// call — it must refresh even when the title bar is hidden.
pub(crate) fn refresh_persistent_chapter_toast(state: &AppState) {
    if !state.chapter_toast_persistent.get() {
        return;
    }
    let text = compute_current_chapter_text(state);
    state.chapter_toast.set_text(&text);
    state.chapter_toast.set_visible(true);
}
```

- [ ] **Step 2: Wire the early-return highlight path**

In `src/input/highlight.rs`, at the site around `:446`:

```rust
        repaint_reader_gloss_visible(state);
        crate::app::scene_synopsis::update_title_bar_scene(state);
        crate::input::navigation::refresh_persistent_chapter_toast(state);
        return;
```

- [ ] **Step 3: Wire the main highlight path**

In `src/input/highlight.rs`, at the site around `:486` (end of `update_highlight`):

```rust
    repaint_reader_gloss_visible(state);
    crate::app::scene_synopsis::update_title_bar_scene(state);
    crate::input::navigation::refresh_persistent_chapter_toast(state);
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean.

- [ ] **Step 5: Run the test suite**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs src/input/highlight.rs
git commit -m "feat(nav): persistent chapter toast tracks the cursor across scenes/chapters"
```

---

### Task 6: Yield to the search toast

**Files:**
- Modify: `src/input/search.rs` (`no_match_toast` `src/input/search.rs:97-101`, `edge_toast` `src/input/search.rs:408-415`)

**Interfaces:**
- Consumes: `state.chapter_toast_persistent` (Task 3), `state.chapter_toast` (`src/app/mod.rs:663`).
- Produces: no new symbols — two call-site edits that hide the persistent toast when a search toast appears. The next cursor move re-shows it (Task 5 backstop); the search toast's own 3s timer then clears it.

Do NOT touch `ui/toast.rs::show_transient` — other toasts share it. Scope the yield to these two search sites only.

- [ ] **Step 1: Yield in `no_match_toast`**

In `src/input/search.rs`, edit `no_match_toast` (`:97-101`) so it hides the persistent toast before showing the search toast:

```rust
pub(crate) fn no_match_toast(state: &AppState) {
    let q = state.search_bar.query();
    let text = format!("No match for \u{201c}{}\u{201d}", display_pattern(&q));
    // The search toast borrows the bottom strip; a persistent chapter toast
    // yields to it (the next cursor move re-shows the chapter toast).
    if state.chapter_toast_persistent.get() {
        state.chapter_toast.set_visible(false);
    }
    crate::ui::toast::show_transient(&state.search_toast, &text, 3);
}
```

- [ ] **Step 2: Yield in `edge_toast`**

In `src/input/search.rs`, edit `edge_toast` (`:408-415`) the same way, immediately before its `show_transient`:

```rust
    if state.chapter_toast_persistent.get() {
        state.chapter_toast.set_visible(false);
    }
    crate::ui::toast::show_transient(&state.search_toast, &text, 3);
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean.

- [ ] **Step 4: Run the test suite**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/search.rs
git commit -m "feat(search): persistent chapter toast yields to the search toast"
```

---

### Task 7: End-to-end verification on the real render

**Files:**
- No source changes — headless e2e drive + screenshots.

This task confirms the on-screen behavior the unit tests cannot: persistence, live update, toggle-off, and search yield. Use the headless cage harness (`test-headless-navigation` skill / `scripts/e2e-env.sh`). Confirm current key names in `src/input/keymap_config.rs` before scripting (`+` = keysym `plus`).

- [ ] **Step 1: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: builds clean.

- [ ] **Step 2: Play — persist, live-update, toggle off**

Drive a play (e.g. `Cym`) headlessly:
- Press `+` → screenshot: dim bottom-center toast `"{abbrev} — Act N, Scene M"` visible.
- Wait > 3s, screenshot again: toast STILL visible (did not auto-dismiss).
- Navigate forward across a scene boundary (page turns / `j`), screenshot: toast text updated to the new Act/Scene.
- Press `+` again → screenshot: toast hidden.

Open each PNG and confirm by eye per the UI review protocol.

- [ ] **Step 3: Prose-with-chapters — persist + chapter update**

Drive a prose work with chapters (e.g. `BH` Bleak House):
- Press `+` → screenshot: `"{abbrev} — Chapter N of M — {title}"` visible.
- Wait > 3s, screenshot: still visible.
- Navigate into the next chapter, screenshot: text now "Chapter N+1 of M".
- Press `+` → screenshot: hidden.

- [ ] **Step 4: Regression — front-matter-only prose stays transient**

On a prose work positioned in front matter with no chapter markers (or a prose work known to lack `is_chapter` lines), press `+`, wait > 3s, screenshot: toast has auto-dismissed (transient path unchanged).

- [ ] **Step 5: Search yield**

On the play from Step 2 with the persistent toast up, trigger a search boundary toast (submit a query with no match, or search past the last occurrence). Screenshot: chapter toast hidden while the search toast shows; after the search toast clears (or after the next `j`), screenshot: chapter toast reappears.

- [ ] **Step 6: Record results**

Note pass/fail for each sub-case inline (quote the on-screen toast text seen in each PNG). If any fails, fix in the relevant task's file and re-run this task. No commit (verification only) unless a fix was needed.

---

## Self-Review notes

- **Spec coverage:** §1 predicate → Tasks 1–2; §2 toggle + timer-less show → Task 4; §3 live refresh → Task 5; §4 search yield → Task 6; §5 work-switch reset → Task 3; testing → Task 7. All covered.
- **Escape interaction (from spec review):** `escape_reader_mode` (`src/input/actions/escape.rs:20-26`) hides `chapter_toast` and bumps the gen but does NOT clear `chapter_toast_persistent`; the next cursor move re-shows it via Task 5's refresh. This is the intended "yield, then reappear" behavior — no code change needed, but confirm during Task 7 Step 5 if you also press Escape.
- **Type consistency:** `chapter_toast_persistent: Rc<Cell<bool>>` used identically in Tasks 3–6; `compute_current_chapter_text` / `show_chapter_toast_persistent` / `refresh_persistent_chapter_toast` signatures match across producer (Task 4/5) and consumers.

# Speaker-turn Navigation (J / K) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `J` / `K` reader keybinds that jump the cursor to the first dialogue line of the next / previous speaker turn (the next time the speaker changes), seeking MPV audio to the landed line.

**Architecture:** A pure scan function `speaker_turn_target` computes the target buffer line from a per-line `(speaker, is_dialogue)` view, so it is unit-testable without GTK. Two `AppState` handlers (`jump_to_next_speaker` / `jump_to_prev_speaker`) build that view from `work.lines` + `work_line_for_buffer`, set `current_line`, scroll, and call `after_page_change(.., PageChangeReason::Dialogue)` so audio seeks and the page follows — identical tail to the existing `jump_to_next_dialogue` / `jump_to_prev_dialogue`.

**Tech Stack:** Rust, GTK4 / sourceview5, the existing `Action` keymap-as-data system. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-06-09-speaker-turn-navigation-design.md`

---

## Background the implementer needs

- `AppState.current_line: usize` is a **buffer line** index (the highlighted line).
- `state.work_line_for_buffer(bl) -> Option<usize>` (src/app.rs:478) maps a buffer line to a work-line index, handling both the `LineMap` path and the mid-load fallback. A buffer line with no mapping (blank / separator / structural chrome) returns `None`.
- `work.lines[wi].speaker: Option<String>` (loaded from `line_mapping.speaker`, src/db/queries.rs:76) is the **authoritative** per-line speaker. Every line of a speech — including wrapped continuation lines — repeats the speaker (verified against `Rom 1.1`). Read it; never classify buffer text to detect a speaker change (CLAUDE.md → authoritative-boundary principle).
- `is_dialogue_line(&state.buffer, bl) -> bool` (src/input/viewport.rs:670) is the dialogue test used by every nav verb; it is imported into navigation.rs already.
- `after_page_change(state, PageChangeReason::Dialogue)` (src/input/navigation.rs:134) repaints the highlight, seeks MPV to the new `current_line`, and shows the vocab popup. `Dialogue` is the reason `q`/`comma` use.
- `scroll_after_jump_forward(state, prev_line)` and `scroll_after_jump_backward(state)` (re-exported from scroll.rs) turn the page / scroll so the new cursor is visible. These are exactly what `jump_to_next_dialogue` / `jump_to_prev_dialogue` call (src/input/navigation.rs:865-879, 849-862).

## File map

- **Modify** `src/input/navigation.rs` — add the pure `speaker_turn_target` fn, the `SpeakerLine` view type, the two public handlers, and a `#[cfg(test)]` module for the pure fn.
- **Modify** `src/input/actions/mod.rs` — add `Action::JumpToNextSpeaker` / `Action::JumpToPrevSpeaker` to the enum, the `category()` Navigation arm, and the `name()` arm.
- **Modify** `src/input/keymap.rs` — add two dispatch arms.
- **Modify** `src/input/keymap_config.rs` — add two compiled-in default bindings (`J`, `K`) and a test.
- **Modify** `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — add the same two bindings (stow source).
- **Modify** `src/ui/keybinds_overlay.rs` — add `J` / `K` caps + `describe()` arms (done via the `update-cairo-keybinds-overlay` skill in Task 7).

---

## Task 1: Pure speaker-turn scan function + tests

The core logic. A "speaker turn" is a maximal run of consecutive work-lines sharing the same `speaker`. The scan operates on a `Vec<SpeakerLine>` indexed by **buffer line**, where each entry carries that buffer line's speaker (`Option<String>`, `None` for unmapped/chrome lines) and whether it is a dialogue line.

**Files:**
- Modify: `src/input/navigation.rs` (add near the other cursor-verb helpers, e.g. just below `first_dialogue_line` around line 1297)
- Test: `src/input/navigation.rs` (new `#[cfg(test)] mod speaker_turn_tests` at end of file)

- [ ] **Step 1: Write the failing test**

Add at the very end of `src/input/navigation.rs`:

```rust
#[cfg(test)]
mod speaker_turn_tests {
    use super::{speaker_turn_target, SpeakerLine, Direction};

    /// Build a view from compact tuples: (speaker, is_dialogue).
    /// `""` speaker means None (unmapped/chrome line).
    fn view(rows: &[(&str, bool)]) -> Vec<SpeakerLine> {
        rows.iter()
            .map(|(sp, dlg)| SpeakerLine {
                speaker: if sp.is_empty() { None } else { Some(sp.to_string()) },
                is_dialogue: *dlg,
            })
            .collect()
    }

    // Sequence with wrapped continuation lines, a stage direction, and a
    // re-appearing speaker:  A A [stage] B B A C C
    fn sample() -> Vec<SpeakerLine> {
        view(&[
            ("A", true),   // 0
            ("A", true),   // 1  (wrapped continuation of A)
            ("", false),   // 2  [stage direction] — unmapped
            ("B", true),   // 3
            ("B", true),   // 4  (wrapped continuation of B)
            ("A", true),   // 5  (A speaks again)
            ("C", true),   // 6
            ("C", true),   // 7  (wrapped continuation of C)
        ])
    }

    #[test]
    fn next_lands_on_first_line_of_next_turn() {
        let v = sample();
        // From inside the first A block (line 0) -> first B line (3).
        assert_eq!(speaker_turn_target(&v, 0, Direction::Next), Some(3));
        // From the B continuation (4) -> the re-appearing A (5).
        assert_eq!(speaker_turn_target(&v, 4, Direction::Next), Some(5));
        // From A (5) -> first C line (6).
        assert_eq!(speaker_turn_target(&v, 5, Direction::Next), Some(6));
    }

    #[test]
    fn next_returns_none_at_last_turn() {
        let v = sample();
        // Inside the final C block -> nothing after.
        assert_eq!(speaker_turn_target(&v, 6, Direction::Next), None);
        assert_eq!(speaker_turn_target(&v, 7, Direction::Next), None);
    }

    #[test]
    fn prev_lands_on_first_line_of_previous_turn() {
        let v = sample();
        // From the C block (7) -> first line of previous block (the A at 5).
        assert_eq!(speaker_turn_target(&v, 7, Direction::Prev), Some(5));
        // From A (5) -> first B line (3), NOT the second B (4).
        assert_eq!(speaker_turn_target(&v, 5, Direction::Prev), Some(3));
        // From B continuation (4) -> first A line (0), NOT the second A (1).
        assert_eq!(speaker_turn_target(&v, 4, Direction::Prev), Some(0));
        assert_eq!(speaker_turn_target(&v, 3, Direction::Prev), Some(0));
    }

    #[test]
    fn prev_returns_none_at_first_turn() {
        let v = sample();
        assert_eq!(speaker_turn_target(&v, 0, Direction::Prev), None);
        assert_eq!(speaker_turn_target(&v, 1, Direction::Prev), None);
    }

    #[test]
    fn none_speaker_origin_treats_any_some_as_different() {
        // Front matter: origin line has no speaker. Next should find the first
        // Some-speaker dialogue line.
        let v = view(&[("", false), ("", true), ("A", true), ("A", true), ("B", true)]);
        assert_eq!(speaker_turn_target(&v, 0, Direction::Next), Some(2));
    }

    #[test]
    fn prose_with_no_speakers_is_noop_both_directions() {
        let v = view(&[("", true), ("", true), ("", true)]);
        assert_eq!(speaker_turn_target(&v, 1, Direction::Next), None);
        assert_eq!(speaker_turn_target(&v, 1, Direction::Prev), None);
    }

    #[test]
    fn non_dialogue_lines_are_never_a_target() {
        // A [stage] B — the stage line (1) is never returned; Next from A(0)
        // skips it to B(2).
        let v = view(&[("A", true), ("", false), ("B", true)]);
        assert_eq!(speaker_turn_target(&v, 0, Direction::Next), Some(2));
        assert_eq!(speaker_turn_target(&v, 2, Direction::Prev), Some(0));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib speaker_turn_tests 2>&1 | tail -20`
Expected: FAIL — compile error, `cannot find function speaker_turn_target` / `cannot find type SpeakerLine` / `Direction`.

- [ ] **Step 3: Write the minimal implementation**

In `src/input/navigation.rs`, add this block right after the `first_dialogue_line` function (around line 1297, before `jump_to_next_scene`):

```rust
// ---------------------------------------------------------------------------
// Speaker-turn navigation (J / K)
// ---------------------------------------------------------------------------

/// A per-buffer-line view used by the pure speaker-turn scan. `speaker` is the
/// authoritative `line_mapping.speaker` for that buffer line (None for unmapped
/// chrome lines — blanks, separators, structural headers). `is_dialogue` is the
/// same dialogue test the rest of navigation uses.
#[derive(Clone, Debug)]
pub(crate) struct SpeakerLine {
    pub speaker: Option<String>,
    pub is_dialogue: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Next,
    Prev,
}

/// Pure scan: from buffer line `from`, return the first dialogue line of the
/// next / previous speaker turn — the next run of consecutive lines whose
/// speaker differs from `from`'s speaker. Returns the FIRST dialogue line of
/// that run in both directions. `None` when there is no such turn ahead /
/// behind (work boundary, or a work with no speakers).
///
/// Authoritative: operates only on the per-line speaker carried in `lines`,
/// never on buffer text. See CLAUDE.md → authoritative-boundary principle.
pub(crate) fn speaker_turn_target(
    lines: &[SpeakerLine],
    from: usize,
    dir: Direction,
) -> Option<usize> {
    let cur = lines.get(from).and_then(|l| l.speaker.clone());
    match dir {
        Direction::Next => {
            // First dialogue line after `from` whose speaker differs from `cur`.
            // That line is already the first line of the next turn.
            ((from + 1)..lines.len()).find(|&i| {
                let l = &lines[i];
                l.is_dialogue && l.speaker != cur
            })
        }
        Direction::Prev => {
            // Walk back to the first dialogue line whose speaker differs from
            // `cur` (the LAST line of the previous turn); call its speaker
            // `prev`. Then keep walking back while dialogue lines stay `prev`,
            // returning the first line of that block.
            let mut i = from;
            let prev_block_speaker;
            loop {
                if i == 0 {
                    return None;
                }
                i -= 1;
                let l = &lines[i];
                if l.is_dialogue && l.speaker != cur {
                    prev_block_speaker = l.speaker.clone();
                    break;
                }
            }
            // `i` is the last dialogue line of the previous turn. Back up over
            // its own block (same speaker), tracking the earliest such line.
            let mut first = i;
            while i > 0 {
                i -= 1;
                let l = &lines[i];
                if l.is_dialogue {
                    if l.speaker == prev_block_speaker {
                        first = i;
                    } else {
                        break;
                    }
                }
                // Non-dialogue lines inside the block (none expected, but a
                // stage direction could interleave) are skipped without ending
                // the block.
            }
            Some(first)
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib speaker_turn_tests 2>&1 | tail -20`
Expected: PASS — `test result: ok. 7 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(nav): pure speaker-turn scan (speaker_turn_target)"
```

---

## Task 2: AppState handlers that build the view and jump

Wrap the pure function in two `pub` handlers that build the `Vec<SpeakerLine>` from the current work and drive the cursor + scroll + audio seek.

**Files:**
- Modify: `src/input/navigation.rs` (add right after `speaker_turn_target`)

- [ ] **Step 1: Write the implementation**

In `src/input/navigation.rs`, immediately after the `speaker_turn_target` function added in Task 1, add:

```rust
/// Build the per-buffer-line speaker view for the current work. Index is the
/// buffer line; `speaker` comes from `work.lines[work_line_for_buffer(bl)]`.
fn build_speaker_view(state: &AppState) -> Vec<SpeakerLine> {
    let line_count = state.effective_line_count();
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return Vec::new(),
    };
    (0..line_count)
        .map(|bl| {
            let speaker = state
                .work_line_for_buffer(bl)
                .and_then(|wi| work.lines.get(wi))
                .and_then(|l| l.speaker.clone());
            SpeakerLine {
                speaker,
                is_dialogue: is_dialogue_line(&state.buffer, bl),
            }
        })
        .collect()
}

/// Jump to the first dialogue line of the NEXT speaker turn (`J`). Seeks audio.
pub fn jump_to_next_speaker(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let view = build_speaker_view(state);
    if let Some(target) = speaker_turn_target(&view, state.current_line, Direction::Next) {
        let prev_line = state.current_line;
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        log_fmt!("SPEAKER_NEXT: {} -> {}", prev_line, target);
        scroll_after_jump_forward(state, prev_line);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}

/// Jump to the first dialogue line of the PREVIOUS speaker turn (`K`). Seeks audio.
pub fn jump_to_prev_speaker(state: &mut AppState) {
    if state.current_work.is_none() {
        return;
    }
    let view = build_speaker_view(state);
    if let Some(target) = speaker_turn_target(&view, state.current_line, Direction::Prev) {
        log_fmt!("SPEAKER_PREV: {} -> {}", state.current_line, target);
        state.current_line = target;
        state.pending_advance = None;
        state.pending_advance_ignore_bl = None;
        state.prev_highlight_line.set(None);
        scroll_after_jump_backward(state);
        after_page_change(state, PageChangeReason::Dialogue);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean (warnings about the two new `pub fn` being unused are acceptable until Task 4 wires them).

- [ ] **Step 3: Run the full lib test suite to confirm no regressions**

Run: `cargo test --lib 2>&1 | tail -15`
Expected: PASS — all existing tests plus the 7 from Task 1 still green.

- [ ] **Step 4: Commit**

```bash
git add src/input/navigation.rs
git commit -m "feat(nav): jump_to_next_speaker / jump_to_prev_speaker handlers"
```

---

## Task 3: Add the two Actions to the enum

**Files:**
- Modify: `src/input/actions/mod.rs:50-51` (enum), `:173-174` (category), `:282` (name)

- [ ] **Step 1: Write the failing test**

Add this test to the `#[cfg(test)] mod tests` block at the bottom of `src/input/actions/mod.rs` (after `category_assignments_are_correct`, around line 399):

```rust
    #[test]
    fn speaker_turn_actions_are_navigation() {
        assert_eq!(Action::JumpToNextSpeaker.category(), Category::Navigation);
        assert_eq!(Action::JumpToPrevSpeaker.category(), Category::Navigation);
        assert_eq!(Action::JumpToNextSpeaker.name(), "JumpToNextSpeaker");
        assert_eq!(Action::JumpToPrevSpeaker.name(), "JumpToPrevSpeaker");
        // Serde round-trip (keymap.json action parsing relies on this).
        let a: Action = serde_json::from_str("\"JumpToNextSpeaker\"").expect("parse");
        assert_eq!(a, Action::JumpToNextSpeaker);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib speaker_turn_actions_are_navigation 2>&1 | tail -15`
Expected: FAIL — compile error, `no variant named JumpToNextSpeaker`.

- [ ] **Step 3: Add the enum variants**

In `src/input/actions/mod.rs`, in the `// Cursor / dialogue navigation` group, after `JumpToPrevDialogue,` (line 47), add:

```rust
    JumpToNextSpeaker,
    JumpToPrevSpeaker,
```

- [ ] **Step 4: Add the category arm**

In the `category()` Navigation match arm, after `| Action::JumpToPrevDialogue` (line 170), add:

```rust
            | Action::JumpToNextSpeaker
            | Action::JumpToPrevSpeaker
```

- [ ] **Step 5: Add the name arm**

In the `name()` match, after the `Action::JumpToPrevDialogue => "JumpToPrevDialogue",` line (line 278), add:

```rust
            Action::JumpToNextSpeaker => "JumpToNextSpeaker",
            Action::JumpToPrevSpeaker => "JumpToPrevSpeaker",
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib speaker_turn_actions_are_navigation 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs
git commit -m "feat(actions): JumpToNextSpeaker / JumpToPrevSpeaker variants"
```

---

## Task 4: Dispatch the actions to the handlers

**Files:**
- Modify: `src/input/keymap.rs:1532` (after the JumpToPrevDialogue arm)

- [ ] **Step 1: Add the dispatch arms**

In `src/input/keymap.rs`, in `dispatch_action`'s `// Cursor / dialogue` group, immediately after:

```rust
        JumpToPrevDialogue => navigation::jump_to_prev_dialogue(&mut state.borrow_mut()),
```

add:

```rust
        JumpToNextSpeaker => navigation::jump_to_next_speaker(&mut state.borrow_mut()),
        JumpToPrevSpeaker => navigation::jump_to_prev_speaker(&mut state.borrow_mut()),
```

- [ ] **Step 2: Verify it compiles (exhaustive match now satisfied)**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean — no "non-exhaustive patterns" error, no "unused function" warning for the handlers.

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(keymap): dispatch JumpToNextSpeaker / JumpToPrevSpeaker"
```

---

## Task 5: Compiled-in default bindings J / K

**Files:**
- Modify: `src/input/keymap_config.rs` — `nav_bindings()` (around line 217) + a test (around line 367)

- [ ] **Step 1: Write the failing test**

In `src/input/keymap_config.rs`, in the `#[cfg(test)] mod tests` block, add after `default_reader_bindings_contains_known_bindings` (around line 368):

```rust
    #[test]
    fn speaker_turn_keys_bound_to_capital_j_and_k() {
        let m = default_reader_bindings();
        assert_eq!(m.get(&KeyCombo::plain("J")), Some(&Action::JumpToNextSpeaker));
        assert_eq!(m.get(&KeyCombo::plain("K")), Some(&Action::JumpToPrevSpeaker));
        // Lowercase j / k keep their existing cursor bindings (regression guard).
        assert_eq!(m.get(&KeyCombo::plain("j")), Some(&Action::CursorNextDialogue));
        assert_eq!(m.get(&KeyCombo::plain("k")), Some(&Action::CursorPrevLine));
    }

    #[test]
    fn shift_j_resolves_to_next_speaker_via_lookup() {
        let km = Keymap::default();
        // GTK delivers Shift+j as key "J" with shift=true; is_uppercase_letter
        // strips the redundant shift, so plain("J") matches.
        assert_eq!(km.lookup("J", false, true, false), Some(Action::JumpToNextSpeaker));
        assert_eq!(km.lookup("K", false, true, false), Some(Action::JumpToPrevSpeaker));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib speaker_turn_keys 2>&1 | tail -15`
Expected: FAIL — `assertion failed: ... Some(JumpToNextSpeaker)` (got `None`).

- [ ] **Step 3: Add the bindings**

In `src/input/keymap_config.rs`, in `nav_bindings()`, in the `// Cursor / dialogue` group, after the `q` binding line:

```rust
        (KeyCombo::plain("q"), Action::JumpToNextDialogue),
```

add:

```rust
        (KeyCombo::plain("J"), Action::JumpToNextSpeaker),
        (KeyCombo::plain("K"), Action::JumpToPrevSpeaker),
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib speaker_turn_keys shift_j_resolves 2>&1 | tail -15`
Expected: PASS for both new tests.

- [ ] **Step 5: Confirm no binding-count assertion broke**

Run: `cargo test --lib keymap_config 2>&1 | tail -15`
Expected: PASS — including `default_reader_bindings_returns_nonempty_map` (count is `> 50`, still satisfied).

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap_config.rs
git commit -m "feat(keymap): bind J/K to next/prev speaker turn (defaults)"
```

---

## Task 6: Add J / K to the stowed keymap.json

The stow source overrides compiled defaults, so the bindings must also live here or `J`/`K` would fall back to defaults (which now agree — but per CLAUDE.md "always update both files" so the JSON is authoritative and self-documenting).

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

- [ ] **Step 1: Inspect the current dialogue bindings in the JSON**

Run: `rg -n '"q"|"comma"|JumpToNextDialogue|JumpToPrevDialogue' ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
Expected: shows the existing `q` / `comma` entries and their surrounding JSON shape (so the new entries match formatting exactly).

- [ ] **Step 2: Add the two bindings**

Edit `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`. In the `"reader"` array, next to the existing `q` dialogue binding, add two objects (match the file's existing indentation and trailing-comma style):

```json
    { "key": "J", "action": "JumpToNextSpeaker" },
    { "key": "K", "action": "JumpToPrevSpeaker" },
```

- [ ] **Step 3: Validate the JSON parses**

Run: `jq . ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json > /dev/null && echo OK`
Expected: `OK` (no parse error). If `jq` reports an error, fix the comma/brace placement.

- [ ] **Step 4: Confirm the live symlink resolves to the stow source**

Run: `readlink -f ~/.config/linux-lit/keymap.json`
Expected: a path under `~/tty-dotfiles/linux-lit/...`. If it is NOT a symlink into tty-dotfiles, run `cd ~/tty-dotfiles && stow linux-lit` to deploy, then re-check. (Do not edit the live file directly.)

- [ ] **Step 5: Commit the dotfiles repo**

```bash
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && \
  git commit -m "linux-lit: bind J/K to next/prev speaker turn"
cd ~/utono/linux-lit
```

---

## Task 7: Update the Ctrl+/ keybinds overlay (J / K)

The overlay is a hand-maintained mirror with no compile-time enforcement (CLAUDE.md → "Always update the Ctrl+/ overlay too"). Use the dedicated skill, which carries the mandatory three-pass cross-reference.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (HOME_ROW caps for `J`/`K` + `describe()` arms)

- [ ] **Step 1: Invoke the overlay skill**

Invoke the `update-cairo-keybinds-overlay` skill. Provide it this change set:
- `J` → `JumpToNextSpeaker` — "Next speaker turn — first line of the next speech by a different character; seeks audio. Handler: `jump_to_next_speaker` — src/input/navigation.rs"
- `K` → `JumpToPrevSpeaker` — "Prev speaker turn — first line of the previous speech by a different character; seeks audio. Handler: `jump_to_prev_speaker` — src/input/navigation.rs"

`J` and `K` are the shifted forms of the existing `j`/`k` cursor keys; place their caps/detail rows accordingly in the home-row table.

- [ ] **Step 2: Verify it builds**

Run: `cargo build 2>&1 | tail -20`
Expected: builds clean.

- [ ] **Step 3: Confirm both labels have describe() arms (no blank detail rows)**

Run: `rg -n "JumpToNextSpeaker|JumpToPrevSpeaker|\"J\"|\"K\"" src/ui/keybinds_overlay.rs`
Expected: each of `J` and `K` appears in the row table AND has a `describe()` arm with the text above. No real binding may have an empty detail slot.

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(overlay): J/K speaker-turn keys in Ctrl+/ overlay"
```

---

## Task 8: Full verification

- [ ] **Step 1: Run the whole pure-logic suite**

Run: `cargo test --bins 2>&1 | tail -20`
Expected: PASS — all tests, including the 7 speaker-turn-scan tests, the action category/name test, and the two keymap tests.

- [ ] **Step 2: Clippy clean on touched files**

Run: `cargo clippy 2>&1 | rg -i "navigation.rs|keymap|actions/mod" | tail -20`
Expected: no new warnings in the files this plan touched. (Pre-existing warnings elsewhere are out of scope.)

- [ ] **Step 3: Request the headless visual check from the user**

The handlers page-turn when the target is off-screen (via `after_page_change` → `scroll_after_jump_*`). The landing computation is unit-covered, but the on-screen result (highlight visible, page follows, audio seeks) is a render/runtime criterion an agent cannot self-verify on the live dwl session. Ask the user to:

1. Launch a multi-character play (e.g. `Rom`) headless or in their session.
2. Press `J` repeatedly through scene 1.1 (which alternates SAMPSON / GREGORY).
3. Confirm each press lands the highlight on the **first line of the next speaker's** turn, the page follows when the turn is off-screen, and (with sync/MPV running) audio seeks to that line.
4. Press `K` to confirm the reverse lands on the **first** line of the previous speaker's turn.

Provide the exact command from CLAUDE.md → *Headless Verification* / *Automated UI tests* if they want the cage harness:

```bash
./scripts/e2e-env.sh cargo test -- --ignored --nocapture
```

Do not claim runtime verification is done until the user confirms the on-screen behavior.

- [ ] **Step 4: Final review commit (if any cleanup was needed)**

If steps 1–2 surfaced fixes, commit them:

```bash
git add -A && git commit -m "chore(nav): speaker-turn nav cleanup"
```

Otherwise this plan is complete.

---

## Self-review notes (for the implementer)

- **Spec coverage:** J/K keys (Tasks 5–7), seek-audio via `Dialogue` reason (Task 2), authoritative speaker source via `work_line_for_buffer` (Task 2 `build_speaker_view`), first-line-of-block semantics both directions (Task 1 tests), None-speaker + prose no-op (Task 1 tests), all five wiring touch-points (Tasks 3–7), unit tests (Task 1), headless visual ask (Task 8). All spec sections map to a task.
- **Type consistency:** `SpeakerLine { speaker: Option<String>, is_dialogue: bool }`, `Direction::{Next,Prev}`, and `speaker_turn_target(&[SpeakerLine], usize, Direction) -> Option<usize>` are used identically in Tasks 1 and 2. Handler names `jump_to_next_speaker` / `jump_to_prev_speaker` and action names `JumpToNextSpeaker` / `JumpToPrevSpeaker` are consistent across Tasks 2–7.
- **No placeholders:** every code step shows complete code; every command has an expected result.

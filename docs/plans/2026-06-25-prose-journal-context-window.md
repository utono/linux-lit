# Prose Journal-Q&A Context Windowing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** For prose works, send only the reader's cursor paragraph ±10 paragraphs to the Claude API as journal-Q&A context, instead of the whole division (the entire book for single-division prose like Cromwell).

**Architecture:** Add `scene_text_windowed` in scene_synopsis.rs that, for prose, renders only the anchor paragraph ±radius (clamped to the division) and, for plays, delegates to the unchanged `scene_text_for`. The paragraph-index math is a pure `window_range` helper (unit-tested). The journal `ask_claude` path resolves the anchor from the reader's saved position and routes the Scene/Passage bands through the windowed builder.

**Tech Stack:** Rust, GTK4 (AppState), SQLite-backed Work model. Pure-logic tests via `cargo test --bins`.

## Global Constraints

- Do NOT run the app (`cargo run`); only `cargo build` / `cargo test`. The user runs the app + verifies the on-screen / request-size result. (CLAUDE.md)
- `cargo test --bins` stays green; clippy warning count must not increase (baseline 118).
- `radius = 10` (module const `PROSE_CONTEXT_RADIUS: usize = 10`) → up to `2*radius+1 = 21` paragraphs.
- Plays / non-prose are UNCHANGED: `scene_text_windowed` returns `scene_text_for(...)` verbatim for `!is_prose_work(work.work_type)`.
- `scene_text_for` itself is NOT modified (it stays the full-scene primitive; the windowed fn delegates to it for plays).
- Prose detection: `crate::db::line_types::is_prose_work(&work.work_type) -> bool`.
- A prose work's paragraphs are `work.lines` rows where `(div1,div2)` match the division, in order (each row = one paragraph; speaker interleave matches `scene_text_for`).
- Anchor = `s.journal.return_pos.0` (saved buffer `current_line`) mapped via `state.work_line_for_buffer(buf) -> Option<usize>`; fall back to the division's first paragraph if None/unresolvable.
- Commit messages end with the Co-Authored-By / Claude-Session trailer per CLAUDE.md.

---

### Task 1: Pure `window_range` helper + tests

**Files:**
- Modify: `src/app/scene_synopsis.rs` — add `window_range` + a `#[cfg(test)]` module.

**Interfaces:**
- Produces: `fn window_range(anchor_pos: usize, radius: usize, n: usize) -> (usize, usize)` — inclusive paragraph index range to include, clamped to `[0, n)`. Returns `(lo, hi)` with `lo <= hi` when `n > 0`. Used by Task 2.

- [ ] **Step 1: Write the failing tests.** Add to `src/app/scene_synopsis.rs` (in a `#[cfg(test)] mod window_tests`):

```rust
#[cfg(test)]
mod window_tests {
    use super::window_range;

    #[test]
    fn middle_anchor_full_window() {
        // anchor 50, radius 10, n 100 -> [40, 60] inclusive = 21 paragraphs
        assert_eq!(window_range(50, 10, 100), (40, 60));
    }
    #[test]
    fn clamps_low_near_start() {
        assert_eq!(window_range(2, 10, 100), (0, 12));
    }
    #[test]
    fn clamps_high_near_end() {
        assert_eq!(window_range(98, 10, 100), (88, 99));
    }
    #[test]
    fn whole_division_when_smaller_than_window() {
        // n=5, any anchor -> the whole [0,4]
        assert_eq!(window_range(2, 10, 5), (0, 4));
    }
    #[test]
    fn empty_division_is_safe() {
        // n=0 -> (0,0); caller must treat n==0 as "no paragraphs" and not index.
        assert_eq!(window_range(0, 10, 0), (0, 0));
    }
}
```

- [ ] **Step 2: Run to verify it fails.**

Run: `cargo test --bins window_range`
Expected: FAIL — `window_range` not defined.

- [ ] **Step 3: Implement.** Add near the top of `src/app/scene_synopsis.rs` (after the existing `use`/consts):

```rust
/// Inclusive paragraph index range `anchor_pos ± radius`, clamped to `[0, n)`.
/// Returns `(lo, hi)` with `lo <= hi`. When `n == 0` returns `(0, 0)` — callers
/// must check `n == 0` separately and not index.
fn window_range(anchor_pos: usize, radius: usize, n: usize) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let lo = anchor_pos.saturating_sub(radius);
    let hi = (anchor_pos + radius).min(n - 1);
    (lo, hi)
}
```

- [ ] **Step 4: Run to verify it passes.**

Run: `cargo test --bins window_range`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit.**

```bash
git add src/app/scene_synopsis.rs
git commit -m "feat(journal): add pure window_range helper for prose context windowing

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: `scene_text_windowed` (prose windows, plays unchanged)

**Files:**
- Modify: `src/app/scene_synopsis.rs` — add `scene_text_windowed` (near `scene_text_for`, ~line 153).

**Interfaces:**
- Consumes: `window_range` (Task 1), `scene_text_for` (existing), `crate::db::line_types::is_prose_work`.
- Produces: `pub fn scene_text_windowed(state: &AppState, div1: i64, div2: i64, anchor_work_line: usize, radius: usize) -> String`. Used by Task 3.

- [ ] **Step 1: Read `scene_text_for` to mirror its render logic.** It is at `src/app/scene_synopsis.rs:153`. It iterates `work.lines.filter(|l| l.div1==div1 && l.div2==div2)`, prints `speaker\n` when the speaker CHANGES, then `line.text\n`. The windowed prose path must reproduce this exactly over the selected sub-range (so the only difference vs the full scene is WHICH paragraphs, not how they render).

- [ ] **Step 2: Implement `scene_text_windowed`.** Add after `scene_text_for`:

```rust
/// Like `scene_text_for`, but for PROSE works returns only the paragraphs around
/// `anchor_work_line` (±`radius`, clamped to the division). Non-prose works
/// (plays) return the full `scene_text_for` — a real scene is small and the
/// whole scene is the intended context. Up to `2*radius + 1` paragraphs.
pub fn scene_text_windowed(
    state: &AppState,
    div1: i64,
    div2: i64,
    anchor_work_line: usize,
    radius: usize,
) -> String {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return String::new(),
    };
    // Plays / non-prose: unchanged full-scene behavior.
    if !crate::db::line_types::is_prose_work(&work.work_type) {
        return scene_text_for(state, div1, div2);
    }

    // Prose: the work-line indices of this division, in order.
    let idxs: Vec<usize> = work
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.div1 == div1 && l.div2 == div2)
        .map(|(i, _)| i)
        .collect();
    if idxs.is_empty() {
        return String::new();
    }
    // Anchor's position within the division; fall back to the first paragraph.
    let anchor_pos = idxs.iter().position(|&i| i == anchor_work_line).unwrap_or(0);
    let (lo, hi) = window_range(anchor_pos, radius, idxs.len());

    // Render the selected paragraphs with the SAME speaker-interleave as
    // scene_text_for (speaker label printed only when it changes).
    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for &wi in &idxs[lo..=hi] {
        let line = &work.lines[wi];
        match line.speaker.as_deref() {
            Some(sp) if last_speaker != Some(sp) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(sp);
                out.push('\n');
                last_speaker = Some(sp);
            }
            _ => {}
        }
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}
```

- [ ] **Step 3: Build.**

Run: `cargo build`
Expected: clean. (No standalone unit test here — it needs a realized `AppState`/`Work`; the index math is covered by Task 1, and a real-data check is in Task 4. Do NOT fabricate a test that constructs an empty AppState and asserts `""`.)

- [ ] **Step 4: Commit.**

```bash
git add src/app/scene_synopsis.rs
git commit -m "feat(journal): scene_text_windowed — prose windows to anchor +/-radius

Prose works return the anchor paragraph +/- radius (clamped to the division);
non-prose delegate to the unchanged scene_text_for. Mirrors scene_text_for's
speaker-interleave render over the selected sub-range.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: Wire windowing into the journal ask path

**Files:**
- Modify: `src/input/actions/journal.rs` — `ask_claude` (the initial borrow block, ~301-318) + add `PROSE_CONTEXT_RADIUS`.

**Interfaces:**
- Consumes: `scene_text_windowed` (Task 2).
- Produces: no signature change; the Scene/Passage bands now send windowed prose context.

- [ ] **Step 1: Add the const.** Near the top of `src/input/actions/journal.rs` (with the other consts):

```rust
/// Prose journal-Q&A context window radius (paragraphs each side of the
/// reader's anchor). Prose divisions can be the whole book, so cap the context.
const PROSE_CONTEXT_RADIUS: usize = 10;
```

- [ ] **Step 2: Resolve the anchor and route the bands.** In `ask_claude`, replace the `let scene_text = match band { ... };` block (`journal.rs:312-318`) with:

```rust
        // Anchor on the reader's saved position (where the journal overlay was
        // opened from), mapped to a work line. Falls back to 0 (the division's
        // first paragraph) when unresolvable — scene_text_windowed clamps.
        let anchor_work_line = s
            .journal
            .return_pos
            .and_then(|(buf, _top)| s.work_line_for_buffer(buf))
            .unwrap_or(0);
        let scene_text = match band {
            JournalBand::Work => String::new(),
            JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::scene_text_windowed(
                &s, d1, d2, anchor_work_line, PROSE_CONTEXT_RADIUS,
            ),
            JournalBand::Passage { div1, div2, .. } => {
                crate::app::scene_synopsis::scene_text_windowed(
                    &s, div1, div2, anchor_work_line, PROSE_CONTEXT_RADIUS,
                )
            }
        };
```

(The `Work` band is unchanged — empty string. The Passage band still appends
`passage_source_text` downstream, unchanged.)

- [ ] **Step 3: Build + full suite.**

Run: `cargo build && cargo test --bins`
Expected: clean build; 454+5 = all pass (Task 1 added 5 tests).

- [ ] **Step 4: Clippy parity.**

Run: `cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: `generated 118 warnings`.

- [ ] **Step 5: Commit.**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): window prose Q&A context to the reader's +/-10 paragraphs

ask_claude resolves the anchor from journal.return_pos and routes the Scene/
Passage bands through scene_text_windowed, so a prose work no longer ships its
whole division (the entire book for single-division prose). Plays unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 4: Real-data regression test (lit.db-gated)

**Files:**
- Modify: `src/app/scene_synopsis.rs` — add a gated test asserting prose shrinks and plays are unchanged.

**Interfaces:**
- Consumes: `scene_text_windowed`, `scene_text_for`, `crate::db::queries::{open_db, load_work}`.

**Note:** `scene_text_windowed` needs an `AppState` (it reads `state.current_work`). If constructing a full `AppState` in a test is impractical, instead test the windowing on a `Work` directly by adding a thin internal variant OR assert via the building blocks. Prefer: if `scene_text_windowed` can be refactored to take `&Work` + `work_line_for_buffer`-free anchor (a work-line index) without GTK, do so and call that from the AppState wrapper. Decide in Step 1; keep the public AppState signature for Task 3.

- [ ] **Step 1: Check testability.** Read `scene_text_windowed` — it uses only `state.current_work` and the anchor work-line. Extract the prose body into a pure `fn prose_window_text(work: &Work, div1, div2, anchor_work_line, radius) -> String` that takes `&Work` directly; have `scene_text_windowed` call it (for prose) after the `is_prose_work` check. This makes it testable without `AppState`. (If you do this extraction, it is a pure refactor of Task 2's code — keep behavior identical.)

- [ ] **Step 2: Write the gated test.**

```rust
#[test]
fn prose_window_shrinks_cromwell_play_unchanged() {
    let conn = match crate::db::queries::open_db() {
        Ok(c) => c, Err(_) => { eprintln!("skip: no lit.db"); return; }
    };
    // Prose: Cromwell is one division (1,0) of thousands of paragraphs.
    if let Ok(work) = crate::db::queries::load_work(&conn, "Cromwell") {
        if crate::db::line_types::is_prose_work(&work.work_type) {
            // anchor somewhere in the middle
            let mid = work.lines.len() / 2;
            let windowed = prose_window_text(&work, 1, 0, mid, 10);
            // full division text length, computed the scene_text_for way
            let full_len: usize = work.lines.iter()
                .filter(|l| l.div1 == 1 && l.div2 == 0)
                .map(|l| l.text.len() + 1).sum();
            assert!(!windowed.is_empty());
            assert!(windowed.len() < full_len / 10,
                "windowed prose ({}) must be far smaller than full division ({})",
                windowed.len(), full_len);
        }
    }
    // Play: scene_text_windowed must equal scene_text_for for a scene. (This
    // needs AppState; if not feasible in a unit test, assert the is_prose_work
    // gate instead: a non-prose work_type returns the full path. Document which.)
}
```

If the play-equality half needs AppState and that's impractical, drop it and
instead unit-assert that `prose_window_text` is only reached for prose (the
`is_prose_work` gate lives in `scene_text_windowed`, already covered by reading
the code) — state this in the commit.

- [ ] **Step 3: Run.**

Run: `cargo test --bins prose_window_shrinks_cromwell -- --nocapture`
Expected: PASS (or "skip: no lit.db" on a runner without the DB).

- [ ] **Step 4: Commit.**

```bash
git add src/app/scene_synopsis.rs
git commit -m "test(journal): real-data check prose context window shrinks vs full division

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: Gate + user verification (handoff)

**Files:** none.

- [ ] **Step 1: Full suite + clippy.**

Run: `cargo test --bins && cargo clippy --bins 2>&1 | grep -oE "generated [0-9]+ warnings"`
Expected: all pass; `generated 118 warnings`.

- [ ] **Step 2: Hand the user the verification.** Per CLAUDE.md the agent does not run the app. Ask the user to:
  1. `cargo run`, open **Cromwell**, scroll to a paragraph mid-book, press **A** (journal Scene band), type a question, submit.
  2. Confirm it returns promptly (no multi-second whole-book upload) and the answer is about the surrounding passage, not generic.
  3. Optional signal: the dev log shows the Claude request / user_msg is now small (KB, not MB).
  4. Sanity on a **play** (e.g. 2H6): journal Q&A on a scene still includes the full scene (unchanged).

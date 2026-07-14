# Vocab-Sentence Loop Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ctrl+r enters a fully modal drill mode that jumps between sentences containing vocab words, loops each sentence gaplessly via MPV's native ab-loop, and karaoke-highlights it (static sentence tint + moving phrase sweep) until `n`/`p` advances or Escape exits.

**Architecture:** A new `src/input/vocab_loop.rs` holds pure, unit-tested helpers (sentence grouping, time-range resolution, index stepping) plus the impure enter/activate/advance/exit functions. A new `InputMode::VocabLoop` makes the mode fully modal in `keymap.rs`. MPV looping reuses the existing `SetAbLoop`/`ClearAbLoop` commands (already built for the echoes overlay — nothing new in `src/mpv/`). The phrase sweep is the existing `phrase_highlight.rs` machinery, forced to PHRASE mode while the loop is active; a new lower-alpha `vocab_sentence_tag` marks the sentence extent underneath it.

**Tech Stack:** Rust, GTK4 (TextTag/TextBuffer), MPV IPC (existing command channel), SQLite (`phrase_timestamps` via existing `phrase_spans_for_line`).

**Spec:** `docs/superpowers/specs/2026-07-09-vocab-sentence-loop-design.md` (approved).

## Global Constraints

- **Never `cargo run`** — verify with `cargo build` / `cargo test`; the user launches the app themselves (`crll`).
- Only one dev instance may run at a time; do not launch the app headlessly for this feature (live MPV audio is the acceptance criterion — user verifies).
- `~/.config/linux-lit/keymap.json` is NOT touched — Ctrl+r keeps its existing `JumpToNextVocab` action; the new behavior branches inside the handler.
- The Ctrl+/ keybinds overlay (`src/ui/keybinds_overlay.rs`) is a hand-maintained mirror — Task 6 must update it via the `update-cairo-keybinds-overlay` skill.
- Pre-existing known test failure: `db::queries::tests::test_load_work_hamlet` (asserts live lit.db state) — not caused by this work; every "run the suite" step expects it and it alone to fail.
- Commit after every task; commit messages end with the standard Co-Authored-By/Claude-Session trailer.
- `vocab_matches.line_index`, `char_start`, `char_end` are **buffer**-line index and unicode-char offsets within that line (same space as `sentence_bounds` and `PhraseSpan` char offsets).

---

### Task 1: Pure core — sentence grouping, time ranges, index math

**Files:**
- Create: `src/input/vocab_loop.rs`
- Modify: `src/input/mod.rs` (register module)

**Interfaces:**
- Consumes: `crate::app::VocabMatch` (`src/app/mod.rs:44` — pub fields `word: String, line_index: usize, char_start: usize, char_end: usize`), `crate::db::queries::PhraseSpan` (`start_time/end_time: f64, start_char/end_char: usize`), `crate::input::phrase_highlight::sentence_bounds(text, sc, ec) -> (usize, usize)`.
- Produces (used by Tasks 4–5):
  - `pub struct VocabSentence { pub buffer_line: usize, pub sent_start_char: usize, pub sent_end_char: usize, pub start_time: f64, pub end_time: f64, pub words: Vec<String> }` (derive `Clone, Debug`)
  - `pub struct VocabLoopState { pub sentences: Vec<VocabSentence>, pub idx: usize }`
  - `pub fn group_matches_into_sentences(matches: &[crate::app::VocabMatch], line_text_of: &dyn Fn(usize) -> String) -> Vec<(usize, (usize, usize), Vec<String>)>`
  - `pub fn sentence_time_range(spans: &[crate::db::queries::PhraseSpan], sc: usize, ec: usize) -> Option<(f64, f64)>`
  - `pub fn start_index(sentences: &[VocabSentence], current_line: usize, forward: bool) -> usize`
  - `pub fn step_index(idx: usize, len: usize, forward: bool) -> usize`

- [ ] **Step 1: Create the module with failing tests**

Create `src/input/vocab_loop.rs`:

```rust
//! Vocab-sentence loop mode: Ctrl+r drill mode that jumps between sentences
//! containing vocab words, loops each one gaplessly via MPV ab-loop, and
//! karaoke-highlights it (sentence tint + phrase sweep) until n/p/Escape.
//!
//! Pure helpers here are unit-tested; the impure enter/activate/advance/exit
//! functions (added in a later task) drive AppState, MPV, and the tags.
//! Design: docs/superpowers/specs/2026-07-09-vocab-sentence-loop-design.md

use crate::app::VocabMatch;
use crate::db::queries::PhraseSpan;
use crate::input::phrase_highlight::sentence_bounds;

/// One sentence containing >=1 vocab word, with its resolved audio window.
/// Char offsets are unicode chars within `buffer_line`'s text (the same space
/// as VocabMatch and PhraseSpan offsets).
#[derive(Clone, Debug)]
pub struct VocabSentence {
    pub buffer_line: usize,
    pub sent_start_char: usize,
    pub sent_end_char: usize,
    pub start_time: f64,
    pub end_time: f64,
    pub words: Vec<String>,
}

/// Mode state held in AppState while the loop is active.
pub struct VocabLoopState {
    pub sentences: Vec<VocabSentence>,
    pub idx: usize,
}

/// Group vocab matches into sentence candidates: `(buffer_line, (sent_start,
/// sent_end), words)`, in buffer order (vocab_matches is built in buffer
/// order). Matches whose sentence bounds coincide merge into one entry; a
/// word repeated within one sentence is listed once. Lines whose text is
/// empty (out of range) are skipped.
pub fn group_matches_into_sentences(
    matches: &[VocabMatch],
    line_text_of: &dyn Fn(usize) -> String,
) -> Vec<(usize, (usize, usize), Vec<String>)> {
    let mut out: Vec<(usize, (usize, usize), Vec<String>)> = Vec::new();
    for m in matches {
        let text = line_text_of(m.line_index);
        if text.is_empty() {
            continue;
        }
        let (sc, ec) = sentence_bounds(&text, m.char_start, m.char_end);
        match out
            .iter_mut()
            .find(|(bl, (s, _), _)| *bl == m.line_index && *s == sc)
        {
            Some((_, _, words)) => {
                if !words.contains(&m.word) {
                    words.push(m.word.clone());
                }
            }
            None => out.push((m.line_index, (sc, ec), vec![m.word.clone()])),
        }
    }
    out
}

/// Audio window of the sentence `[sc, ec)`: start of the FIRST span
/// intersecting it through end of the LAST. None when no span intersects
/// (sentence has no phrase data — caller drops it).
pub fn sentence_time_range(spans: &[PhraseSpan], sc: usize, ec: usize) -> Option<(f64, f64)> {
    let mut it = spans.iter().filter(|sp| sp.start_char < ec && sp.end_char > sc);
    let first = it.next()?;
    let last = it.last().unwrap_or(first);
    Some((first.start_time, last.end_time))
}

/// Entry index: forward = first sentence at/after the cursor line (wraps to
/// 0); backward = last sentence strictly before it (wraps to the end).
pub fn start_index(sentences: &[VocabSentence], current_line: usize, forward: bool) -> usize {
    if forward {
        sentences
            .iter()
            .position(|s| s.buffer_line >= current_line)
            .unwrap_or(0)
    } else {
        sentences
            .iter()
            .rposition(|s| s.buffer_line < current_line)
            .unwrap_or(sentences.len().saturating_sub(1))
    }
}

/// Wrapping n/p step.
pub fn step_index(idx: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (idx + 1) % len
    } else {
        (idx + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(line: usize, cs: usize, ce: usize, w: &str) -> VocabMatch {
        VocabMatch {
            word: w.to_string(),
            line_index: line,
            char_start: cs,
            char_end: ce,
        }
    }

    fn sp(st: f64, et: f64, sc: usize, ec: usize) -> PhraseSpan {
        PhraseSpan { start_time: st, end_time: et, start_char: sc, end_char: ec }
    }

    fn vs(bl: usize) -> VocabSentence {
        VocabSentence {
            buffer_line: bl,
            sent_start_char: 0,
            sent_end_char: 1,
            start_time: 0.0,
            end_time: 1.0,
            words: vec![],
        }
    }

    #[test]
    fn grouping_merges_same_sentence_and_splits_sentences() {
        // "One two. Three four." — sentence 1 = chars [0,8), sentence 2 = [9,20).
        let text = "One two. Three four.";
        let matches = vec![m(0, 4, 7, "two"), m(0, 9, 14, "three"), m(0, 15, 19, "four")];
        let out = group_matches_into_sentences(&matches, &|_| text.to_string());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], (0, (0, 8), vec!["two".to_string()]));
        assert_eq!(
            out[1],
            (0, (9, 20), vec!["three".to_string(), "four".to_string()])
        );
    }

    #[test]
    fn grouping_dedupes_repeated_word_and_skips_empty_lines() {
        let text = "Fog here, fog there.";
        let matches = vec![m(3, 0, 3, "fog"), m(3, 10, 13, "fog"), m(99, 0, 3, "fog")];
        let out = group_matches_into_sentences(&matches, &|bl| {
            if bl == 3 { text.to_string() } else { String::new() }
        });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], (3, (0, 20), vec!["fog".to_string()]));
    }

    #[test]
    fn time_range_spans_first_to_last_intersecting() {
        let spans = vec![
            sp(10.0, 11.0, 0, 8),
            sp(11.0, 12.5, 9, 14),
            sp(12.5, 14.0, 15, 20),
            sp(14.0, 15.0, 21, 30),
        ];
        assert_eq!(sentence_time_range(&spans, 9, 20), Some((11.0, 14.0)));
        assert_eq!(sentence_time_range(&spans, 0, 8), Some((10.0, 11.0)));
        assert_eq!(sentence_time_range(&spans, 40, 50), None);
        assert_eq!(sentence_time_range(&[], 0, 5), None);
    }

    #[test]
    fn start_index_forward_backward_and_wrap() {
        let ss = vec![vs(5), vs(10), vs(20)];
        assert_eq!(start_index(&ss, 0, true), 0);
        assert_eq!(start_index(&ss, 10, true), 1); // at/after cursor
        assert_eq!(start_index(&ss, 21, true), 0); // wrap to first
        assert_eq!(start_index(&ss, 21, false), 2); // last before cursor
        assert_eq!(start_index(&ss, 10, false), 0); // strictly before
        assert_eq!(start_index(&ss, 5, false), 2); // none before -> wrap to last
    }

    #[test]
    fn step_index_wraps_both_directions() {
        assert_eq!(step_index(1, 3, true), 2);
        assert_eq!(step_index(2, 3, true), 0);
        assert_eq!(step_index(0, 3, false), 2);
        assert_eq!(step_index(0, 0, true), 0);
    }
}
```

- [ ] **Step 2: Register the module**

In `src/input/mod.rs`, add alongside the existing module declarations (e.g. next to `pub mod phrase_highlight;`):

```rust
pub mod vocab_loop;
```

- [ ] **Step 3: Run the tests — expect a compile error first**

Run: `cargo test vocab_loop 2>&1 | tail -20`

Expected: compile error — `sentence_bounds` is reachable (already `pub` in `phrase_highlight.rs`), but if any import path is wrong the compiler names it. Fix imports until the five tests run and PASS. (If they pass first try, that's the TDD "watch it fail" satisfied by the deliberate empty-line/wrap edge cases — verify each assertion is actually exercised by breaking one locally and restoring it.)

- [ ] **Step 4: Commit**

```bash
git add src/input/vocab_loop.rs src/input/mod.rs
git commit -m "feat: vocab-loop pure core (sentence grouping, time ranges, index math)"
```

---

### Task 2: Sentence tint tag + dimmed theme color

**Files:**
- Modify: `src/theme.rs` (add `dim_rgba_alpha` free fn + `Theme::vocab_sentence_bg` method + test)
- Modify: `src/app/mod.rs` (create `vocab_sentence_tag` BEFORE `phrase_tag` at ~line 1016; add AppState field ~line 257; init in the AppState literal ~line 1627)
- Modify: `src/input/actions/settings.rs:296` (theme-switch updates the tag color)
- Modify: `src/input/phrase_highlight.rs` (generalize `apply_phrase_tag` into a shared char-range tagger; make `buffer_line_text` `pub(crate)`)

**Interfaces:**
- Consumes: `Theme.phrase_highlight_bg: String` (`src/theme.rs:23`), existing tag-creation block (`src/app/mod.rs:1014-1020`), theme-apply site (`settings.rs:296`).
- Produces:
  - `pub fn dim_rgba_alpha(color: &str, factor: f64) -> String` and `impl Theme { pub fn vocab_sentence_bg(&self) -> String }` in `theme.rs`
  - `AppState.vocab_sentence_tag: gtk4::TextTag`
  - `pub(crate) fn apply_char_range_tag(s: &AppState, tag: &gtk4::TextTag, bl: usize, start_char: usize, end_char: usize)` and `pub(crate) fn buffer_line_text(s: &AppState, bl: usize) -> String` in `phrase_highlight.rs`

- [ ] **Step 1: Write the failing theme test**

In `src/theme.rs`, add at the end (create a `#[cfg(test)] mod tests` if none exists):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_rgba_alpha_scales_only_the_alpha() {
        assert_eq!(
            dim_rgba_alpha("rgba(255, 255, 255, 0.22)", 0.45),
            "rgba(255, 255, 255, 0.099)"
        );
        // Non-rgba strings pass through untouched.
        assert_eq!(dim_rgba_alpha("#ffcc00", 0.45), "#ffcc00");
    }
}
```

- [ ] **Step 2: Run it — expect FAIL (function not defined)**

Run: `cargo test dim_rgba_alpha 2>&1 | tail -5`
Expected: compile error `cannot find function dim_rgba_alpha`.

- [ ] **Step 3: Implement the color helper**

In `src/theme.rs`, below the `Theme` struct's existing impl (or add an `impl Theme` block):

```rust
/// Scale the alpha of an `rgba(r, g, b, a)` string by `factor`. Any other
/// color form (hex, named) passes through unchanged — the caller then reuses
/// the undimmed color, which is a safe worst case (tint too strong, never
/// wrong-colored or invalid CSS).
pub fn dim_rgba_alpha(color: &str, factor: f64) -> String {
    let inner = color
        .trim()
        .strip_prefix("rgba(")
        .and_then(|r| r.strip_suffix(')'));
    if let Some(inner) = inner {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 4 {
            if let Ok(a) = parts[3].parse::<f64>() {
                return format!(
                    "rgba({}, {}, {}, {:.3})",
                    parts[0],
                    parts[1],
                    parts[2],
                    a * factor
                );
            }
        }
    }
    color.to_string()
}

impl Theme {
    /// Sentence-extent tint for the vocab-sentence loop: the karaoke sweep
    /// color at ~45% of its alpha, so the moving sweep stays readable inside
    /// the static sentence marker.
    pub fn vocab_sentence_bg(&self) -> String {
        dim_rgba_alpha(&self.phrase_highlight_bg, 0.45)
    }
}
```

(If `theme.rs` already has an `impl Theme` block, put the method inside it instead of opening a second one.)

- [ ] **Step 4: Run the test — expect PASS**

Run: `cargo test dim_rgba_alpha 2>&1 | tail -5`
Expected: `test theme::tests::dim_rgba_alpha_scales_only_the_alpha ... ok`

- [ ] **Step 5: Create the tag, field, and theme-apply hook**

In `src/app/mod.rs` at ~line 1014, INSERT the new tag creation **immediately BEFORE** the `phrase_tag` builder (GTK tag priority follows add-order; the sweep's `phrase_tag` must be added AFTER so its background wins on the overlap and the sweep shows above the sentence tint):

```rust
    // Sentence-extent tint for the vocab-sentence loop mode: marks the whole
    // looping sentence while the phrase sweep (phrase_tag, added after this,
    // so it wins the overlap) moves inside it.
    let vocab_sentence_tag = gtk4::TextTag::builder()
        .name("vocab-sentence")
        .background(&theme.vocab_sentence_bg())
        .build();
    buffer.tag_table().add(&vocab_sentence_tag);
```

Add the AppState field next to `phrase_tag` (~line 257):

```rust
    pub vocab_sentence_tag: gtk4::TextTag,
```

Add to the AppState literal next to `phrase_tag,` (~line 1627):

```rust
        vocab_sentence_tag,
```

In `src/input/actions/settings.rs`, directly after line 296 (`state.phrase_tag.set_property(...)`):

```rust
    state
        .vocab_sentence_tag
        .set_property("background", &theme.vocab_sentence_bg());
```

- [ ] **Step 6: Generalize the char-range tagger in phrase_highlight.rs**

Replace the body of `apply_phrase_tag` (`src/input/phrase_highlight.rs:347-371`) with a thin wrapper over a shared helper, and widen `buffer_line_text`'s visibility (line 181, `fn` → `pub(crate) fn`):

```rust
/// Move the phrase sweep tag to `[start_char, end_char)` of buffer line `bl`.
fn apply_phrase_tag(s: &AppState, bl: usize, start_char: usize, end_char: usize) {
    let tag = s.phrase_tag.clone();
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&tag, &bs, &be);
    apply_char_range_tag(s, &tag, bl, start_char, end_char);
}

/// Apply `tag` to `[start_char, end_char)` of buffer line `bl`, clamped to
/// the line's char count (GTK iter offsets are unicode chars, matching the
/// Python backfill's str indices; clamping guards data drift). Does NOT
/// remove prior applications — callers own their tag's lifecycle.
pub(crate) fn apply_char_range_tag(
    s: &AppState,
    tag: &gtk4::TextTag,
    bl: usize,
    start_char: usize,
    end_char: usize,
) {
    let buffer = &s.buffer;
    let Some(line_start) = buffer.iter_at_line(bl as i32) else {
        return;
    };
    let line_chars = {
        let mut e = line_start;
        if !e.ends_line() {
            e.forward_to_line_end();
        }
        e.line_offset().max(0) as usize
    };
    let sc = start_char.min(line_chars);
    let ec = end_char.min(line_chars).max(sc);
    if ec == sc {
        return;
    }
    let mut a = line_start;
    a.set_line_offset(sc as i32);
    let mut b = line_start;
    b.set_line_offset(ec as i32);
    buffer.apply_tag(tag, &a, &b);
}
```

- [ ] **Step 7: Build and run the suite**

Run: `cargo build 2>&1 | tail -3` — expect clean.
Run: `cargo test --bins 2>&1 | tail -5` — expect all pass except the known `test_load_work_hamlet`.

- [ ] **Step 8: Commit**

```bash
git add src/theme.rs src/app/mod.rs src/input/actions/settings.rs src/input/phrase_highlight.rs
git commit -m "feat: vocab-sentence tint tag with dimmed sweep color; shared char-range tagger"
```

---

### Task 3: Extract the shared canonical cursor landing in navigation.rs

**Files:**
- Modify: `src/input/navigation.rs:2775-2853` (`jump_to_next_vocab` / `jump_to_prev_vocab`)

**Interfaces:**
- Consumes: the identical landing block currently duplicated in both vocab jumps (`center_cursor`, `is_line_fully_visible`, `canonical_page_top_for`, `set_page_instant`, `after_page_change` — all already in scope inside `navigation.rs`).
- Produces: `pub fn land_cursor_on_line(state: &mut AppState, target_line: usize)` — used by Task 4's `activate_current`.

- [ ] **Step 1: Extract the helper**

In `src/input/navigation.rs`, above `jump_to_next_vocab` (~line 2774), add:

```rust
/// Move the cursor to `target_line` and land on its CANONICAL spread — the
/// same page paging through the work shows — not force-top-aligned. Shared by
/// the vocab jumps and the vocab-sentence loop; mirrors bookmark jump_to_line
/// and search n/N.
pub fn land_cursor_on_line(state: &mut AppState, target_line: usize) {
    state.current_line = target_line;
    state.page_back_stack.clear();
    state
        .page_back_stack
        .push((state.page_top_line, state.page_top_offset));
    match state.config.navigation_mode {
        crate::config::NavigationMode::Scroll => center_cursor(state),
        crate::config::NavigationMode::EReader => {
            if !is_line_fully_visible(state, target_line) {
                set_page_instant(state, canonical_page_top_for(state, target_line));
            }
        }
    }
    after_page_change(state, PageChangeReason::Vocab);
}
```

Then replace the duplicated tail of BOTH `jump_to_next_vocab` and `jump_to_prev_vocab` (everything from `state.current_line = target_line;` through `after_page_change(state, PageChangeReason::Vocab);`) with:

```rust
    land_cursor_on_line(state, target_line);
```

(Keep each function's index-selection logic and `state.vocab_match_idx = Some(...)` line untouched.)

- [ ] **Step 2: Build and run the suite (behavior-preserving refactor)**

Run: `cargo build 2>&1 | tail -3` — clean.
Run: `cargo test --bins 2>&1 | tail -5` — all pass except known `test_load_work_hamlet`.

- [ ] **Step 3: Commit**

```bash
git add src/input/navigation.rs
git commit -m "refactor: extract land_cursor_on_line from the vocab jumps"
```

---

### Task 4: Mode machinery — InputMode::VocabLoop, state, enter/activate/advance/exit, modal handler

**Files:**
- Modify: `src/app/mod.rs` (InputMode variant ~line 88-135; `vocab_loop` AppState field ~line 513; literal init ~line 1770)
- Modify: `src/input/vocab_loop.rs` (impure functions)
- Modify: `src/input/keymap.rs` (modal handler + dispatch arm in the `mode != Reader` match at ~line 123)

**Interfaces:**
- Consumes: Task 1's pure fns and structs; Task 2's `vocab_sentence_tag` + `apply_char_range_tag` + `buffer_line_text`; Task 3's `navigation::land_cursor_on_line`; existing `MpvCommand::{SetAbLoop, ClearAbLoop, ResumeAndSeek, TogglePause}`; `navigation::{SYNC_SUPPRESS_SEEK, show_chapter_toast}` (`show_chapter_toast` is `pub(crate)` at `navigation.rs:2446`); `phrase_highlight::paint_pending_phrase`; `AppState.{media_id, mpv_connected, sync_enabled, translations_visible, vocab_matches, current_line, work_line_for_buffer, current_work, phrase_paint_hold, suppress_sync_until, pending_prose_cross, pending_advance, input_mode, cmd_tx, buffer}`; `queries::{open_db, phrase_spans_for_line}`.
- Produces (used by Task 5):
  - `pub fn enter_vocab_loop(state: &Rc<RefCell<AppState>>, forward: bool) -> bool`
  - `pub fn advance(state: &Rc<RefCell<AppState>>, forward: bool)`
  - `pub fn exit_vocab_loop(s: &mut AppState)`
  - `InputMode::VocabLoop`, `AppState.vocab_loop: Option<VocabLoopState>`

- [ ] **Step 1: Add the InputMode variant and AppState field**

In `src/app/mod.rs`, add to `enum InputMode` (after `SegmentVim`, ~line 116):

```rust
    /// Fully modal vocab-sentence drill loop (Ctrl+r when the playing media
    /// has phrase data): the sentence under review repeats via MPV ab-loop;
    /// n/p step between vocab sentences, a/Space toggles pause, Escape (or
    /// Ctrl+r) exits. All other keys are swallowed.
    VocabLoop,
```

Add the AppState field next to `vocab_match_idx` (~line 513):

```rust
    pub vocab_loop: Option<crate::input::vocab_loop::VocabLoopState>,
```

and to the AppState literal next to `vocab_match_idx: None,` (~line 1770):

```rust
        vocab_loop: None,
```

- [ ] **Step 2: Build to surface every exhaustive InputMode match**

Run: `cargo build 2>&1 | rg "non-exhaustive|E0004" -A 3`

For each flagged `match` add a `VocabLoop` arm: in the keymap dispatch (`src/input/keymap.rs` ~line 123-151) route it (Step 4's handler); anywhere else (e.g. focus/overlay helpers) treat `VocabLoop` exactly like `Reader` unless the surrounding arms obviously group modal overlays — copy the adjacent pattern and say so in the commit message. If the build is already clean (wildcard arms), continue.

- [ ] **Step 3: Add the impure functions to vocab_loop.rs**

Append to `src/input/vocab_loop.rs`:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{AppState, InputMode};
use crate::input::phrase_highlight::{apply_char_range_tag, buffer_line_text};
use crate::mpv::MpvCommand;

/// Build the work's vocab-sentence list for the active media: group matches
/// into sentences, resolve each sentence's audio window from its line's
/// phrase spans, drop sentences without phrase data. Spans are fetched once
/// per distinct line (one prose paragraph often holds many vocab sentences).
fn build_vocab_sentences(s: &AppState) -> Vec<VocabSentence> {
    let Some(media) = s.media_id else {
        return Vec::new();
    };
    let Ok(conn) = crate::db::queries::open_db() else {
        return Vec::new();
    };
    let grouped = group_matches_into_sentences(&s.vocab_matches, &|bl| buffer_line_text(s, bl));
    let mut spans_cache: std::collections::HashMap<i64, Vec<PhraseSpan>> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for (bl, (sc, ec), words) in grouped {
        let Some(wi) = s.work_line_for_buffer(bl) else {
            continue;
        };
        let Some(line_id) = s
            .current_work
            .as_ref()
            .and_then(|w| w.lines.get(wi))
            .map(|l| l.id)
        else {
            continue;
        };
        let spans = spans_cache
            .entry(line_id)
            .or_insert_with(|| crate::db::queries::phrase_spans_for_line(&conn, line_id, media));
        let Some((start_time, end_time)) = sentence_time_range(spans, sc, ec) else {
            continue;
        };
        out.push(VocabSentence {
            buffer_line: bl,
            sent_start_char: sc,
            sent_end_char: ec,
            start_time,
            end_time,
            words,
        });
    }
    out
}

/// Enter the loop mode at the first vocab sentence at/after (forward) or
/// before (backward) the cursor. Returns false when the mode cannot start —
/// the caller falls back to the plain vocab jump. Requires connected MPV,
/// an active media id, sync on, and translations hidden (inflated buffer
/// misaligns char offsets, same gate as the phrase sweep).
pub fn enter_vocab_loop(state: &Rc<RefCell<AppState>>, forward: bool) -> bool {
    let mut s = state.borrow_mut();
    if !s.mpv_connected || !s.sync_enabled || s.translations_visible || s.media_id.is_none() {
        return false;
    }
    let sentences = build_vocab_sentences(&s);
    if sentences.is_empty() {
        if !s.vocab_matches.is_empty() {
            crate::input::navigation::show_chapter_toast(&s, "no vocab sentences with audio");
        }
        return false;
    }
    let idx = start_index(&sentences, s.current_line, forward);
    s.vocab_loop = Some(VocabLoopState { sentences, idx });
    s.input_mode = InputMode::VocabLoop;
    crate::logging::log("VOCAB_LOOP: enter");
    activate_current(&mut s);
    true
}

/// n/p inside the mode: step the index (wrapping) and re-activate.
pub fn advance(state: &Rc<RefCell<AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    remove_sentence_tag(&s);
    {
        let Some(vl) = s.vocab_loop.as_mut() else {
            return;
        };
        vl.idx = step_index(vl.idx, vl.sentences.len(), forward);
    }
    activate_current(&mut s);
}

/// Land on, tint, and start looping the current sentence. One funnel for
/// entry and n/p so the ab-loop, tint, toast, and sync suppression can never
/// drift apart.
fn activate_current(s: &mut AppState) {
    let (sentence, idx, len) = {
        let Some(vl) = s.vocab_loop.as_ref() else {
            return;
        };
        (vl.sentences[vl.idx].clone(), vl.idx, vl.sentences.len())
    };
    crate::input::navigation::land_cursor_on_line(s, sentence.buffer_line);
    // A loop never coexists with a scheduled sync page turn or line advance.
    s.pending_prose_cross = None;
    s.pending_advance = None;
    // Gapless native loop; ResumeAndSeek unpauses and jumps to the start.
    let _ = s.cmd_tx.try_send(MpvCommand::SetAbLoop {
        a: sentence.start_time,
        b: sentence.end_time,
    });
    let _ = s.cmd_tx.try_send(MpvCommand::ResumeAndSeek(sentence.start_time));
    s.suppress_sync_until = Some(
        std::time::Instant::now() + crate::input::navigation::SYNC_SUPPRESS_SEEK,
    );
    // Paint the first phrase immediately (same pattern as do_mpv_seek) so the
    // sweep shows before live TimePos ticks arrive.
    if crate::input::phrase_highlight::paint_pending_phrase(s, sentence.start_time) {
        s.phrase_paint_hold = s.suppress_sync_until;
    }
    apply_char_range_tag(
        s,
        &s.vocab_sentence_tag.clone(),
        sentence.buffer_line,
        sentence.sent_start_char,
        sentence.sent_end_char,
    );
    crate::input::navigation::show_chapter_toast(
        s,
        &format!("vocab {}/{} — {}", idx + 1, len, sentence.words.join(", ")),
    );
    crate::logging::log(&format!(
        "VOCAB_LOOP: {}/{} line={} chars=[{},{}) t=[{:.2},{:.2}] words={:?}",
        idx + 1,
        len,
        sentence.buffer_line,
        sentence.sent_start_char,
        sentence.sent_end_char,
        sentence.start_time,
        sentence.end_time,
        sentence.words
    ));
}

/// Remove the sentence-extent tint everywhere.
fn remove_sentence_tag(s: &AppState) {
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&s.vocab_sentence_tag, &bs, &be);
}

/// The ONE exit funnel: Escape/Ctrl+r in-mode, and defensively on work
/// switch. Clears the MPV ab-loop (a leaked loop would trap normal
/// playback), drops the state and tint, and returns to Reader. Playback
/// continues from wherever it is; normal sync resumes on the next TimePos.
/// No handling is needed for MPV quit/disconnect — the ab-loop lives in the
/// MPV process and dies with it.
pub fn exit_vocab_loop(s: &mut AppState) {
    if s.vocab_loop.take().is_none() {
        return;
    }
    let _ = s.cmd_tx.try_send(MpvCommand::ClearAbLoop);
    remove_sentence_tag(s);
    if s.input_mode == InputMode::VocabLoop {
        s.input_mode = InputMode::Reader;
    }
    crate::logging::log("VOCAB_LOOP: exit");
}
```

- [ ] **Step 4: Add the modal key handler and dispatch arm**

In `src/input/keymap.rs`, add near the other modal handlers (e.g. below `handle_keybinds_key`):

```rust
/// Fully modal vocab-sentence loop keys: n/p step, a/Space toggles pause,
/// Escape or Ctrl+r exits. EVERYTHING else is swallowed (returns true) so
/// no reader bind can fire mid-drill.
fn handle_vocab_loop_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    match key_name {
        "n" if !is_ctrl => crate::input::vocab_loop::advance(state, true),
        "p" if !is_ctrl => crate::input::vocab_loop::advance(state, false),
        "a" | "space" if !is_ctrl => {
            let _ = state
                .borrow()
                .cmd_tx
                .try_send(crate::mpv::MpvCommand::TogglePause);
        }
        "Escape" => crate::input::vocab_loop::exit_vocab_loop(&mut state.borrow_mut()),
        "r" | "R" if is_ctrl => {
            crate::input::vocab_loop::exit_vocab_loop(&mut state.borrow_mut())
        }
        _ => {}
    }
    true
}
```

In the `mode != Reader` dispatch match (~line 123-151), add the arm (next to `InputMode::Settings`):

```rust
            crate::app::InputMode::VocabLoop => handle_vocab_loop_key(state, key_name, is_ctrl),
```

- [ ] **Step 5: Build and run the suite**

Run: `cargo build 2>&1 | tail -3` — clean (the handler is not yet reachable; `enter_vocab_loop` unused warnings are acceptable until Task 5, silence none).
Run: `cargo test --bins 2>&1 | tail -5` — all pass except known `test_load_work_hamlet`.

- [ ] **Step 6: Commit**

```bash
git add src/app/mod.rs src/input/vocab_loop.rs src/input/keymap.rs
git commit -m "feat: VocabLoop modal mode - state, enter/advance/exit, key handler"
```

---

### Task 5: Wire entry, force the sweep, guard sync page turns, clear on work switch

**Files:**
- Modify: `src/input/actions/concordance.rs:143-156` (Ctrl+r / Ctrl+Shift+R entry branch)
- Modify: `src/input/phrase_highlight.rs:194-200` (`active_mode` forces PHRASE while looping)
- Modify: `src/main.rs:500` and `src/main.rs:546` (skip scheduled page turn / line advance while looping)
- Modify: `src/app/mod.rs:2777` area in `display_work` (defensive exit on work switch)

**Interfaces:**
- Consumes: Task 4's `enter_vocab_loop` / `exit_vocab_loop`, `AppState.vocab_loop`.
- Produces: the user-visible feature — no new API.

- [ ] **Step 1: Branch the vocab-jump handlers**

In `src/input/actions/concordance.rs`, replace both handlers:

```rust
/// Jump to the next vocab match. When the playing media has phrase data and
/// at least one vocab sentence resolves, Ctrl+r enters the vocab-sentence
/// loop mode instead; otherwise the plain jump (unchanged behavior).
pub(crate) fn jump_to_next_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    if crate::input::vocab_loop::enter_vocab_loop(state, true) {
        return;
    }
    navigation::jump_to_next_vocab(&mut state.borrow_mut());
}

/// Jump to the previous vocab match, or enter the vocab-sentence loop mode
/// backward (see jump_to_next_vocab).
pub(crate) fn jump_to_prev_vocab(
    state: &Rc<RefCell<AppState>>,
    _tokio_handle: &tokio::runtime::Handle,
) {
    if crate::input::vocab_loop::enter_vocab_loop(state, false) {
        return;
    }
    navigation::jump_to_prev_vocab(&mut state.borrow_mut());
}
```

- [ ] **Step 2: Force the phrase sweep while looping**

In `src/input/phrase_highlight.rs`, `active_mode` (~line 194):

```rust
/// The karaoke mode for the current work's class (prose vs verse flag).
/// The vocab-sentence loop always shows the phrase sweep, whatever the
/// class's configured mode — restored implicitly when the mode exits.
fn active_mode(s: &AppState) -> PhraseHighlightMode {
    if s.vocab_loop.is_some() {
        return PhraseHighlightMode::Phrase;
    }
    if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    }
}
```

- [ ] **Step 3: Guard the two scheduled-sync blocks in main.rs**

At `src/main.rs:500` (the `pending_prose_cross` firing block) change:

```rust
                        if s.sync_enabled && !s.loading_work.get() {
```

to:

```rust
                        if s.sync_enabled && !s.loading_work.get() && s.vocab_loop.is_none() {
```

Make the IDENTICAL edit at `src/main.rs:546` (the `pending_advance` block). Both blocks start with that exact condition; `activate_current` also clears both pendings, so this guard is belt-and-braces against a cross scheduled by a TimePos tick that raced the mode entry.

- [ ] **Step 4: Defensive exit on work switch**

In `src/app/mod.rs` `display_work`, immediately after line 2777 (`state.pending_prose_cross = None;`):

```rust
    // A vocab-sentence loop never survives a work switch (its buffer lines,
    // media id, and ab-loop all belong to the old work).
    crate::input::vocab_loop::exit_vocab_loop(state);
```

- [ ] **Step 5: Build, test, clippy**

Run: `cargo build 2>&1 | tail -3` — clean, and the Task-4 dead-code warnings are gone.
Run: `cargo test --bins 2>&1 | tail -5` — all pass except known `test_load_work_hamlet`.
Run: `cargo clippy 2>&1 | rg "vocab_loop" -B1 -A3` — no new warnings in the new module (fix any it names).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/concordance.rs src/input/phrase_highlight.rs src/main.rs src/app/mod.rs
git commit -m "feat: Ctrl+r enters vocab-sentence loop on phrase-data works"
```

---

### Task 6: Keybinds overlay + CLAUDE.md

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (Ctrl+r / Ctrl+Shift+R detail-panel `describe()` arms, ~line 387)
- Modify: `CLAUDE.md` (Concordance System section — the Ctrl+r line)

- [ ] **Step 1: Run the overlay skill**

Invoke the `update-cairo-keybinds-overlay` skill for this change. The substance: the `r` key's ctrl / ctrl+shift describe() text currently says only "next/prev vocab word jump" and must now read (adapt to the file's phrasing conventions, keep the `-> handler — src/path` reference format):

```
Ctrl+r — next vocab jump; on phrase-data works enters the vocab-sentence
loop (sentence repeats via MPV ab-loop; n/p step, a/Space pause, Esc exits)
-> concordance::jump_to_next_vocab — src/input/actions/concordance.rs
```

and the mirrored text for Ctrl+Shift+R (backward entry). Run the skill's three cross-reference passes; no new keycap slots are needed (n/p/a/Escape exist only inside the modal mode, which the reader overlay does not enumerate — same treatment as the other modal editors).

- [ ] **Step 2: Update CLAUDE.md**

In the Concordance System section, extend the Ctrl+r bullet:

```markdown
- **Ctrl+r / Ctrl+Shift+R** — next/prev vocab word jump (always, ignores concordance state). On works whose playing media has `phrase_timestamps`, these instead enter the **vocab-sentence loop mode**: the sentence containing the vocab word repeats gaplessly (MPV ab-loop) with sentence tint + phrase sweep; `n`/`p` step between vocab sentences, `a`/Space pauses, Escape/Ctrl+r exits (fully modal). See `src/input/vocab_loop.rs`.
```

- [ ] **Step 3: Build and commit**

Run: `cargo build 2>&1 | tail -3` — clean.

```bash
git add src/ui/keybinds_overlay.rs CLAUDE.md
git commit -m "docs: keybinds overlay + CLAUDE.md for vocab-sentence loop"
```

---

### Task 7: Final verification and live handoff

**Files:** none (verification only)

- [ ] **Step 1: Full check**

```bash
cargo build 2>&1 | tail -3
cargo test --bins 2>&1 | tail -8
cargo clippy 2>&1 | tail -5
```

Expected: clean build; suite green except the pre-existing `test_load_work_hamlet`; no new clippy warnings.

- [ ] **Step 2: Hand live verification to the user**

Loop gaplessness and the tint are audible/visual acceptance criteria on the real GL renderer with real MPV — do NOT verify headlessly (a headless instance connects to the live MPV socket and would fight the user's session). Ask the user to run, in a work with phrase data (BH-Barrett):

```bash
crll
```

then check: Ctrl+r lands on a vocab sentence, audio loops the sentence seamlessly, the sentence shows a light tint with the brighter sweep moving inside it, the toast shows `vocab N/M — word`, `n`/`p` step (and wrap), `a` pauses, other keys do nothing, Escape returns to normal reading with sync working, and a plain Ctrl+r still does the old jump on a verse work without phrase data (e.g. any `Cym` edition).

- [ ] **Step 3: Finish the branch per the house rule**

If work was done on a feature branch, follow the finishing-a-development-branch flow (merge to master locally with `--no-ff`, re-verify, push, delete branch). If executed directly on `master`, just push after the user's live verdict.

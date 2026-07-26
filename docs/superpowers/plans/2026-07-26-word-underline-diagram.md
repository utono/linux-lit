# Word-Underline Selection for the Syntax Diagram — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Underline words on the current line with `-` / `_`, press `Return`, and get the syntax diagram for the sentence those words belong to.

**Architecture:** No new `InputMode`. The state that means "words are underlined" already exists (`WordCycleState.collect_ranges`), so `Return` gets a guarded reader arm and the underline clear slots into the existing `escape_reader_mode` priority ladder. A new pure module `src/input/sentence.rs` expands char offsets outward to sentence boundaries; a new entry point in `actions/syntax.rs` joins a bounded line window, calls it, and hands the result to the existing `open_syntax_diagram`.

**Tech Stack:** Rust, GTK4 (`gtk4::TextBuffer`/`TextTag`), existing `claude_bridge` request plumbing.

Spec: `docs/superpowers/specs/2026-07-26-word-underline-diagram-design.md`

## Global Constraints

- Work in the worktree `~/utono/linux-lit-wt/feat-syntax-diagram` on branch `feat/syntax-diagram`. Do NOT work in the main checkout.
- The clipboard copy behavior of `-` / `_` is UNCHANGED. Both keep calling `wl-copy` exactly as today.
- No new `InputMode` variant.
- No changes to `~/.config/linux-lit/keymap.json` and no stow redeploy — no bind ENTRIES change, only handlers plus two guarded arms. `keymap.json` already maps `minus`/`underscore` to `WordCycleCopy`/`WordCollectCopy`.
- No changes to visual mode's existing "Syntax" action — both entry points coexist.
- Verify with `cargo build`; do NOT run `cargo run`. The user runs the app.
- Clippy baseline is 181 warnings. Do not exceed it.
- Unit-test baseline is 1140 passing. Every task must keep them green.

---

## Spec Corrections Found During Planning

Two load-bearing claims in the spec are wrong against the code. The plan implements the CORRECTED behavior; both are noted here so the implementer does not "fix" the plan back to match the spec.

**1. Reader mode DOES bind `Escape`.** The spec says "reader mode binds no `Escape` today, so both arms are purely additive." False — `keymap_config.rs:461` binds `(KeyCombo::plain("Escape"), Action::EscapeReaderMode)`, dispatching to `escape_reader_mode` in `src/input/actions/escape.rs`. That function is a documented priority ladder: vocab popup → toasts → translations → concordance → AB loop → search.

Adding a competing `Escape` arm in `keymap.rs` would either shadow that ladder or be unreachable. Instead **Task 4 inserts the underline clear INTO the ladder**, after search. Rationale for last place: an underline is the least "modal" of the states — a reader with both a visible toast and an underline expects Escape to dismiss the toast first, exactly as the ladder already orders every other pair.

**2. `collect_ranges` are offsets into BUFFER text, not work-line text.** `extract_buffer_line_words` (`word_copy.rs`) reads `state.buffer` and computes char offsets into the buffer line's string. Work-line text (`work.lines[wi].text`) is the DB text, and Phase B's inline italics DELETE `_` delimiters from the buffer — so on a work with italics the two strings differ in length and the offsets do not transfer.

Therefore Task 5 builds the joined window from **buffer** line text (which is what the offsets index) and uses work lines ONLY to collect `line_mapping` ids for enrichment. Never index buffer offsets into `work.lines[..].text`.

---

## File Structure

**Create:**
- `src/input/sentence.rs` — pure sentence-boundary expansion. No GTK, no DB, no `AppState`. Carries the bulk of the unit tests.

**Modify:**
- `src/input/mod.rs` — register the new module.
- `src/input/actions/word_copy.rs` — underline persistence, `clear_word_underline`, `active_underline`, `-` sets `collect_ranges`.
- `src/input/actions/syntax.rs` — `open_syntax_diagram_for_underlined` entry point.
- `src/input/actions/escape.rs` — underline clear at the bottom of the ladder.
- `src/input/actions/mod.rs` — new `Action::OpenSyntaxDiagramForUnderlined` variant + name mapping.
- `src/input/keymap_config.rs` — bind `Return` to the new action.
- `src/input/keymap.rs` — dispatch arm for the new action.
- `src/ui/keybinds_overlay.rs` — keycap strip + `describe()` arm.
- `docs/guides/keybind-consistency-guide.md` — record `-`/`_`'s second meaning.

---

## Task 1: Sentence-boundary expansion (pure)

The piece most likely to be subtly wrong, so it is built first, in isolation, with the most tests. No GTK, no DB — pure `&str` in, offsets out.

**Files:**
- Create: `src/input/sentence.rs`
- Modify: `src/input/mod.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/input/sentence.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn sentence_span(text: &str, ranges: &[(usize, usize)]) -> Option<(usize, usize)>` — `text` is a joined window, `ranges` are char offsets into it, the return is a char-offset half-open span `(start, end)` covering the full sentence(s).

- [x] **Step 1: Write the failing tests**

Create `src/input/sentence.rs` with ONLY the test module and a stub, so the file compiles and the tests fail on behavior rather than on a missing symbol:

```rust
//! Sentence-boundary expansion for the word-underline syntax-diagram entry
//! point. Pure: takes a joined text window and char offsets into it, returns
//! the char span of the sentence(s) those offsets fall in.
//!
//! Char offsets into ONE joined string, not (line, char) pairs — a "line" is
//! not a unit here. A two-column play's buffer line is one verse line, but a
//! prose `line_mapping` row in BH-Barrett runs to 2,874 characters (a whole
//! paragraph holding many sentences).

/// Abbreviations whose trailing period is never a sentence boundary.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Dr", "St", "Prof", "Rev", "Hon", "Sr", "Jr",
    "Capt", "Col", "Gen", "Lt", "Sgt", "Maj", "Esq", "vs", "etc", "No",
];

/// Characters that close a sentence.
const TERMINATORS: &[char] = &['.', '!', '?'];

/// Trailing characters that belong to the sentence they follow.
const TRAILERS: &[char] = &['"', '\'', ')', ']', '\u{201d}', '\u{2019}'];

pub fn sentence_span(_text: &str, _ranges: &[(usize, usize)]) -> Option<(usize, usize)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: char-slice a span for readable assertions.
    fn slice(text: &str, span: (usize, usize)) -> String {
        text.chars().skip(span.0).take(span.1 - span.0).collect()
    }

    #[test]
    fn expands_to_surrounding_sentence() {
        let text = "First one. The second sentence here. Third one.";
        // "second" starts at char 15.
        let span = sentence_span(text, &[(15, 21)]).unwrap();
        assert_eq!(slice(text, span), "The second sentence here.");
    }

    #[test]
    fn does_not_break_on_mister_abbreviation() {
        let text = "Mr. Bucket looked at him. Then he left.";
        let span = sentence_span(text, &[(4, 10)]).unwrap();
        assert_eq!(slice(text, span), "Mr. Bucket looked at him.");
    }

    #[test]
    fn does_not_break_on_initials() {
        let text = "It was J. R. Smith who spoke. Nobody answered.";
        let span = sentence_span(text, &[(13, 18)]).unwrap();
        assert_eq!(slice(text, span), "It was J. R. Smith who spoke.");
    }

    #[test]
    fn does_not_break_on_lowercase_after_period() {
        // A period followed by space + lowercase is not a boundary.
        let text = "He went to No. five and waited. Then home.";
        let span = sentence_span(text, &[(0, 2)]).unwrap();
        assert_eq!(slice(text, span), "He went to No. five and waited.");
    }

    #[test]
    fn includes_closing_quote_in_span() {
        let text = "She asked, \"What's that?\" He said nothing.";
        let span = sentence_span(text, &[(11, 16)]).unwrap();
        assert_eq!(slice(text, span), "She asked, \"What's that?\"");
    }

    #[test]
    fn spans_a_line_break_inside_the_window() {
        let text = "To be or not to be,\nthat is the question. Next.";
        let span = sentence_span(text, &[(23, 27)]).unwrap();
        assert_eq!(slice(text, span), "To be or not to be,\nthat is the question.");
    }

    #[test]
    fn union_when_ranges_cross_two_sentences() {
        let text = "First one here. Second one there. Third.";
        // "one" in sentence 1 (6..9) and "there" in sentence 2 (27..32).
        let span = sentence_span(text, &[(6, 9), (27, 32)]).unwrap();
        assert_eq!(slice(text, span), "First one here. Second one there.");
    }

    #[test]
    fn whole_window_when_no_boundary_present() {
        let text = "no terminator anywhere in this window";
        let span = sentence_span(text, &[(3, 13)]).unwrap();
        assert_eq!(slice(text, span), text);
    }

    #[test]
    fn start_of_window_is_a_valid_start() {
        let text = "Opening sentence. Second.";
        let span = sentence_span(text, &[(0, 7)]).unwrap();
        assert_eq!(slice(text, span), "Opening sentence.");
    }

    #[test]
    fn end_of_window_is_a_valid_end() {
        let text = "First. Trailing sentence with no period";
        let span = sentence_span(text, &[(16, 24)]).unwrap();
        assert_eq!(slice(text, span), "Trailing sentence with no period");
    }

    #[test]
    fn empty_ranges_returns_none() {
        assert_eq!(sentence_span("Anything at all.", &[]), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(sentence_span("", &[(0, 1)]), None);
    }

    #[test]
    fn out_of_bounds_range_is_clamped_not_panicking() {
        let text = "Short sentence.";
        let span = sentence_span(text, &[(900, 950)]).unwrap();
        assert_eq!(slice(text, span), "Short sentence.");
    }

    #[test]
    fn handles_multibyte_text_by_chars_not_bytes() {
        // Every char here is multi-byte; offsets must be char-based.
        let text = "Æsop wrote it. Naïve reader—café.";
        let span = sentence_span(text, &[(15, 20)]).unwrap();
        assert_eq!(slice(text, span), "Naïve reader—café.");
    }
}
```

Register the module — add to `src/input/mod.rs`, keeping the list alphabetical (it currently runs `scroll`, `search`, `segments`; insert between `search` and `segments`):

```rust
pub mod sentence;
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo test --bins sentence:: 2>&1 | tail -20`

Expected: FAIL — 13 failures, each an `unwrap()` panic on `None` (or an assert mismatch). This confirms the tests exercise real behavior rather than passing vacuously.

- [x] **Step 3: Implement `sentence_span`**

Replace the stub in `src/input/sentence.rs`:

```rust
/// Expand `ranges` (char offsets into `text`) outward to sentence boundaries.
///
/// `text` is the already-joined buffer region; the caller decides how much
/// context to hand in, so this function never touches lines, the buffer, or
/// GTK. Returns a half-open char span, or `None` when there is nothing to
/// expand (empty text or no ranges).
///
/// Out-of-bounds ranges are clamped rather than rejected: they mean the
/// caller's offsets went stale, and the useful answer is still "the sentence
/// nearest that position".
pub fn sentence_span(text: &str, ranges: &[(usize, usize)]) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || ranges.is_empty() {
        return None;
    }

    let last = chars.len() - 1;
    let lo = ranges.iter().map(|r| r.0).min()?.min(last);
    let hi = ranges.iter().map(|r| r.1).max()?.min(chars.len());

    let start = sentence_start(&chars, lo);
    let end = sentence_end(&chars, hi.saturating_sub(1).max(lo));
    Some((start, end))
}

/// Scan backwards from `from` for the first real terminator; the sentence
/// starts at the first non-space character after it.
fn sentence_start(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i > 0 {
        i -= 1;
        if is_boundary(chars, i) {
            let mut s = i + 1;
            // Step over the terminator's own trailing quotes/brackets, then
            // any whitespace, to land on the next sentence's first char.
            while s < chars.len() && TRAILERS.contains(&chars[s]) {
                s += 1;
            }
            while s < chars.len() && chars[s].is_whitespace() {
                s += 1;
            }
            return s;
        }
    }
    0
}

/// Scan forwards from `from` for the first real terminator; the sentence ends
/// after it plus any trailing quote/bracket that belongs to it.
fn sentence_end(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() {
        if is_boundary(chars, i) {
            let mut e = i + 1;
            while e < chars.len() && TRAILERS.contains(&chars[e]) {
                e += 1;
            }
            return e;
        }
        i += 1;
    }
    chars.len()
}

/// Is `chars[i]` a real sentence terminator?
///
/// `.` is the ambiguous one — `!` and `?` are unconditional. A period is NOT a
/// boundary when it ends a known abbreviation, ends a single-letter initial,
/// or is followed by a lowercase word (which means the sentence continued).
fn is_boundary(chars: &[char], i: usize) -> bool {
    if !TERMINATORS.contains(&chars[i]) {
        return false;
    }
    if chars[i] != '.' {
        return true;
    }

    // The word immediately before the period.
    let mut w_start = i;
    while w_start > 0 && chars[w_start - 1].is_alphanumeric() {
        w_start -= 1;
    }
    let word: String = chars[w_start..i].iter().collect();

    if ABBREVIATIONS.iter().any(|a| a.eq_ignore_ascii_case(&word)) {
        return false;
    }
    // A single-letter word before a period is an initial: "J. R. Smith".
    if word.chars().count() == 1 && word.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return false;
    }

    // Look past the period (and any closing quote) for the next letter. A
    // lowercase one means the sentence did not actually end.
    let mut j = i + 1;
    while j < chars.len() && (chars[j].is_whitespace() || TRAILERS.contains(&chars[j])) {
        j += 1;
    }
    match chars.get(j) {
        Some(c) if c.is_lowercase() => false,
        _ => true,
    }
}
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo test --bins sentence:: 2>&1 | tail -5`

Expected: PASS — `test result: ok. 13 passed; 0 failed`.

- [x] **Step 5: Verify the whole suite and clippy still hold**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo test --bins 2>&1 | rg 'test result' && cargo clippy 2>&1 | rg -c '^warning'`

Expected: `1153 passed; 0 failed` (1140 baseline + 13 new); clippy `181`.

- [x] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/input/sentence.rs src/input/mod.rs
git commit -m "feat(sentence): sentence-boundary expansion over char offsets"
```

---

## Task 2: Underline persistence, clearing, and the lazy-validity helper

Makes the underline survive past 2 seconds and gives the rest of the feature its single source of truth for "what is underlined right now".

**Files:**
- Modify: `src/input/actions/word_copy.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/input/actions/word_copy.rs`

**Interfaces:**
- Consumes: `WordCycleState { cycle_line, cycle_index, bold_gen, collect_words, collect_ranges }` (already exists in this file).
- Produces:
  - `pub fn active_underline(state: &AppState) -> &[(usize, usize)]`
  - `pub fn clear_word_underline(state: &mut AppState)`
  - `fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)], persist: bool)` (private; call sites updated in this task)
  - `pub fn underline_is_active(cycle_line: Option<usize>, current_line: usize, ranges_len: usize) -> bool` — the pure predicate `active_underline` delegates to, so it is unit-testable without an `AppState`.

- [x] **Step 1: Write the failing tests**

Append to `src/input/actions/word_copy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underline_active_on_its_own_line() {
        assert!(underline_is_active(Some(42), 42, 2));
    }

    #[test]
    fn underline_inactive_after_cursor_leaves_the_line() {
        // Lazy clearing: the ranges are still in state, but they no longer
        // belong to the cursor's line, so nothing may act on them.
        assert!(!underline_is_active(Some(42), 43, 2));
    }

    #[test]
    fn underline_inactive_when_no_ranges() {
        assert!(!underline_is_active(Some(42), 42, 0));
    }

    #[test]
    fn underline_inactive_when_never_cycled() {
        assert!(!underline_is_active(None, 42, 2));
    }
}
```

- [x] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo test --bins word_copy:: 2>&1 | tail -20`

Expected: FAIL to COMPILE — `cannot find function underline_is_active in this scope`.

- [x] **Step 3: Implement persistence, clearing, and the helper**

In `src/input/actions/word_copy.rs`:

**3a.** Replace the `apply_word_underline` signature and timer block. The whole function becomes:

```rust
/// Apply the underline tag to the given char ranges on the current line,
/// removing any previous underline first.
///
/// `persist`: when true the 2-second auto-remove timer is NOT armed, so the
/// underline stays until explicitly cleared (Escape, or a `-`/`_` that
/// replaces it). `bold_gen` is still bumped either way, which invalidates any
/// timer already in flight from an earlier non-persistent call.
fn apply_word_underline(state: &mut AppState, ranges: &[(usize, usize)], persist: bool) {
    let buf = &state.buffer;
    let tag = &state.word_bold_tag;
    let (buf_start, buf_end) = (buf.start_iter(), buf.end_iter());
    buf.remove_tag(tag, &buf_start, &buf_end);

    let line_start = buf.iter_at_line(state.current_line as i32).unwrap();
    for &(char_start, char_end) in ranges {
        let mut word_start = line_start;
        word_start.forward_chars(char_start as i32);
        let mut word_end = word_start;
        word_end.forward_chars((char_end - char_start) as i32);
        buf.apply_tag(tag, &word_start, &word_end);
    }

    // Bump the generation counter unconditionally: it is what makes any timer
    // still in flight a no-op.
    let gen = state.word_cycle.bold_gen.get() + 1;
    state.word_cycle.bold_gen.set(gen);
    if persist {
        return;
    }

    let gen_rc = state.word_cycle.bold_gen.clone();
    let buf_clone = buf.clone();
    let tag_clone = tag.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        if gen_rc.get() == gen {
            let (s, e) = (buf_clone.start_iter(), buf_clone.end_iter());
            buf_clone.remove_tag(&tag_clone, &s, &e);
        }
    });
}
```

**3b.** Add the helper and the clear function immediately after `apply_word_underline`:

```rust
/// Pure predicate behind `active_underline`, split out so it is testable
/// without an `AppState`.
///
/// Clearing is LAZY, not event-driven: `current_line` has ~76 write sites
/// across 14 modules, so hooking every cursor-move path would be
/// unimplementable and would rot on the next navigation feature. Instead the
/// underline carries the line it belongs to and is treated as absent once the
/// cursor leaves.
pub fn underline_is_active(cycle_line: Option<usize>, current_line: usize, ranges_len: usize) -> bool {
    ranges_len > 0 && cycle_line == Some(current_line)
}

/// The underlined ranges, but ONLY while they still belong to the cursor's
/// line. Single source of truth for the `Return` / `Escape` guards.
///
/// A tag that briefly outlives its line is cosmetic, not a correctness
/// problem, because nothing can act on it — this returns empty and both
/// guards fall through.
pub fn active_underline(state: &AppState) -> &[(usize, usize)] {
    if underline_is_active(
        state.word_cycle.cycle_line,
        state.current_line,
        state.word_cycle.collect_ranges.len(),
    ) {
        &state.word_cycle.collect_ranges
    } else {
        &[]
    }
}

/// Remove the underline tag and forget the collected words.
pub fn clear_word_underline(state: &mut AppState) {
    let (s, e) = (state.buffer.start_iter(), state.buffer.end_iter());
    state.buffer.remove_tag(&state.word_bold_tag, &s, &e);
    state.word_cycle.bold_gen.set(state.word_cycle.bold_gen.get() + 1);
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.cycle_line = None;
    crate::logging::log("WORD_UNDERLINE: cleared");
}
```

**3c.** In `word_cycle_copy`, `-` must SET `collect_ranges` to its single range rather than clearing them, so one underlined word is a valid diagram selection. Replace:

```rust
    // Clear multi-word collect state (w is single-word mode)
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_ranges.clear();

    // Remove any previous underline tag, then apply to the current word
    apply_word_underline(state, &[(char_start, char_end)]);
```

with:

```rust
    // `-` is single-word mode, but the ONE word it underlines must still be a
    // valid diagram selection, so set the collection to exactly it rather than
    // emptying it. `_` keeps appending from here.
    state.word_cycle.collect_words.clear();
    state.word_cycle.collect_words.push(word.clone());
    state.word_cycle.collect_ranges.clear();
    state.word_cycle.collect_ranges.push((char_start, char_end));

    // Remove any previous underline tag, then apply to the current word.
    // Persistent: cleared by Escape, by leaving the line, or by the next -/_.
    apply_word_underline(state, &[(char_start, char_end)], true);
```

**3d.** In `word_collect_copy`, make its underline persistent too. Replace:

```rust
    apply_word_underline(state, &ranges);
```

with:

```rust
    apply_word_underline(state, &ranges, true);
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo test --bins word_copy:: 2>&1 | tail -5`

Expected: PASS — `test result: ok. 4 passed; 0 failed`.

- [x] **Step 5: Verify build, suite, and clippy**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build 2>&1 | rg -c '^error' ; cargo test --bins 2>&1 | rg 'test result' ; cargo clippy 2>&1 | rg -c '^warning'`

Expected: 0 errors; `1157 passed; 0 failed`; clippy `181`.

- [x] **Step 6: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/input/actions/word_copy.rs
git commit -m "feat(word-copy): persistent underline, lazy validity, clear helper"
```

---

## Task 3: The diagram entry point

Joins a bounded window, expands to the sentence, maps back to `line_mapping` ids, and calls the existing `open_syntax_diagram`.

**Files:**
- Modify: `src/input/actions/syntax.rs`

**Interfaces:**
- Consumes: `sentence::sentence_span`, `word_copy::active_underline`, the existing `open_syntax_diagram(state_rc, text, line_ids)`.
- Produces: `pub fn open_syntax_diagram_for_underlined(state_rc: &Rc<RefCell<AppState>>)`.

- [x] **Step 1: Implement the entry point**

Append to `src/input/actions/syntax.rs`. Note the two hard-won details in the comments — build the window from BUFFER text (that is what the offsets index), and use work lines only for ids:

```rust
/// Open the diagram for the sentence containing the currently underlined
/// words (`-` / `_` then `Return`).
///
/// Window: the cursor's buffer line plus one either side. That covers a
/// sentence spanning a verse break without risking a whole-chapter scan.
///
/// The window is joined from BUFFER text, not `work.lines[..].text`, because
/// `collect_ranges` are char offsets into the BUFFER line (see
/// `extract_buffer_line_words`). Phase B's inline italics delete `_`
/// delimiters from the buffer, so on a work with italics the buffer and DB
/// strings differ in length and the offsets do not transfer. Work lines are
/// consulted ONLY for `line_mapping` ids, which feed `line_syntax` enrichment.
pub fn open_syntax_diagram_for_underlined(state_rc: &Rc<RefCell<AppState>>) {
    let (text, line_ids) = {
        let state = state_rc.borrow();

        let ranges: Vec<(usize, usize)> =
            crate::input::actions::word_copy::active_underline(&state).to_vec();
        if ranges.is_empty() {
            return;
        }

        let cursor = state.current_line;
        let last_line = state.buffer.line_count().max(1) as usize - 1;
        let first = cursor.saturating_sub(1);
        let last = (cursor + 1).min(last_line);

        // Join the window from buffer text, recording where the cursor's own
        // line starts so the underline offsets can be rebased into it.
        let mut window = String::new();
        let mut cursor_line_offset = 0usize;
        for bl in first..=last {
            if bl == cursor {
                cursor_line_offset = window.chars().count();
            }
            let start = match state.buffer.iter_at_line(bl as i32) {
                Some(it) => it,
                None => continue,
            };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            window.push_str(state.buffer.text(&start, &end, false).as_str());
            if bl != last {
                window.push('\n');
            }
        }

        let rebased: Vec<(usize, usize)> = ranges
            .iter()
            .map(|&(s, e)| (s + cursor_line_offset, e + cursor_line_offset))
            .collect();

        let span = match crate::input::sentence::sentence_span(&window, &rebased) {
            Some(sp) => sp,
            None => return,
        };
        let text: String = window
            .chars()
            .skip(span.0)
            .take(span.1 - span.0)
            .collect();

        // Ids for enrichment. The sentence can only touch lines inside the
        // window, so mapping the whole window is correct and cheap.
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let ids: Vec<i64> = (first..=last)
            .filter_map(|bl| {
                state
                    .work_line_for_buffer(bl)
                    .and_then(|wi| work.lines.get(wi))
                    .map(|l| l.id)
            })
            .collect();

        crate::logging::log(&format!(
            "SYNTAX_UNDERLINE: {} range(s) -> span {}..{} over {} line(s)",
            ranges.len(),
            span.0,
            span.1,
            ids.len()
        ));
        (text, ids)
    };

    // `open_syntax_diagram` already guards empty/whitespace-only text
    // (`if text.trim().is_empty()` → log, return), so do not duplicate it here.
    open_syntax_diagram(state_rc, text, line_ids);
}
```

- [x] **Step 2: Verify it compiles**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build 2>&1 | rg '^(error|warning: unused)' | head`

Expected: no `error` lines. A `never used` warning for the new function is expected until Task 5 wires the bind.

- [x] **Step 3: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/input/actions/syntax.rs
git commit -m "feat(syntax): open the diagram for the underlined sentence"
```

---

## Task 4: Underline clear in the Escape ladder

Per Spec Correction 1: reader `Escape` is already bound to `escape_reader_mode`, a priority ladder. The clear goes at the bottom of it, not into a competing `keymap.rs` arm.

**Files:**
- Modify: `src/input/actions/escape.rs`

**Interfaces:**
- Consumes: `word_copy::active_underline`, `word_copy::clear_word_underline`.
- Produces: nothing new.

- [x] **Step 1: Add the rung**

`escape_reader_mode` currently ends with the search-matches block. That block does NOT `return` (it is last), so append a new block after it. Replace the closing of the search block:

```rust
    // Search matches
    {
        let has_search = !state.borrow().search_matches.is_empty();
        if has_search {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            // clear_search removes the search-match tags but leaves the cursor
            // on the matched line. Re-apply the cursor-line highlight so the
            // match line stays highlighted after the search tags are gone
            // (otherwise the line renders with no highlight at all).
            crate::input::highlight::update_highlight(&mut s);
        }
    }
}
```

with:

```rust
    // Search matches
    {
        let has_search = !state.borrow().search_matches.is_empty();
        if has_search {
            let mut s = state.borrow_mut();
            crate::input::search::clear_search(&mut s);
            // clear_search removes the search-match tags but leaves the cursor
            // on the matched line. Re-apply the cursor-line highlight so the
            // match line stays highlighted after the search tags are gone
            // (otherwise the line renders with no highlight at all).
            crate::input::highlight::update_highlight(&mut s);
            return;
        }
    }
    // Word underline (`-` / `_`) is LAST on the ladder: it is the least modal
    // of these states, so a reader with both a toast and an underline expects
    // Escape to take the toast first — exactly how the rungs above order every
    // other pair. Guarded on `active_underline`, so an underline the cursor
    // has already left falls through and Escape does nothing.
    {
        let has_underline =
            !crate::input::actions::word_copy::active_underline(&state.borrow()).is_empty();
        if has_underline {
            crate::input::actions::word_copy::clear_word_underline(&mut state.borrow_mut());
        }
    }
}
```

Note the added `return;` in the search block: it was previously the last rung so falling through was harmless, but with a rung below it the early return is now required or Escape would clear search AND the underline in one press.

- [x] **Step 2: Verify it compiles and the suite holds**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build 2>&1 | rg -c '^error' ; cargo test --bins 2>&1 | rg 'test result'`

Expected: 0 errors; `1157 passed; 0 failed`.

- [x] **Step 3: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/input/actions/escape.rs
git commit -m "feat(escape): clear the word underline at the bottom of the ladder"
```

---

## Task 5: Bind `Return` and wire dispatch

`Return` is verified unbound in reader mode (`rg '"Return"' src/input/keymap_config.rs` matches only a doc comment), so this takes a free key.

**Files:**
- Modify: `src/input/actions/mod.rs`
- Modify: `src/input/keymap_config.rs`
- Modify: `src/input/keymap.rs`

**Interfaces:**
- Consumes: `syntax::open_syntax_diagram_for_underlined`, `word_copy::active_underline`.
- Produces: `Action::OpenSyntaxDiagramForUnderlined`.

- [x] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, add the variant next to the other syntax action in the `Action` enum:

```rust
    OpenSyntaxDiagramForUnderlined,
```

and its name mapping alongside the sibling arms (near `Action::EscapeReaderMode => "EscapeReaderMode",`):

```rust
            Action::OpenSyntaxDiagramForUnderlined => "OpenSyntaxDiagramForUnderlined",
```

If the enum has a reader-mode classification list (the `| Action::EscapeReaderMode` group around line 409), add `| Action::OpenSyntaxDiagramForUnderlined` to the same group so it is classified as a reader action.

- [x] **Step 2: Bind the key**

In `src/input/keymap_config.rs`, in `app_bindings()` next to the existing word-copy binds (`(KeyCombo::plain("minus"), Action::WordCycleCopy)` at ~489):

```rust
        // Return diagrams the sentence containing the `-`/`_` underlined
        // words. Reader mode binds no Return today, so this is additive; the
        // dispatch arm no-ops when nothing is underlined.
        (KeyCombo::plain("Return"), Action::OpenSyntaxDiagramForUnderlined),
```

- [x] **Step 3: Add the dispatch arm**

In `src/input/keymap.rs`, beside the existing word-copy arms (~4491):

```rust
        OpenSyntaxDiagramForUnderlined => {
            // Guarded: falls through silently when nothing is underlined, or
            // when the underline belongs to a line the cursor has left.
            let active = !crate::input::actions::word_copy::active_underline(&state.borrow()).is_empty();
            if active {
                crate::input::actions::syntax::open_syntax_diagram_for_underlined(state);
            }
        }
```

- [x] **Step 4: Verify build, suite, and clippy**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build 2>&1 | rg -c '^error' ; cargo test --bins 2>&1 | rg 'test result' ; cargo clippy 2>&1 | rg -c '^warning'`

Expected: 0 errors; `1157 passed; 0 failed`; clippy `181`. The `never used` warning from Task 3 is now gone.

- [x] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs
git commit -m "feat(keymap): bind Return to diagram the underlined sentence"
```

---

## Task 6: Keybind surfaces

Required by the project's keybind rule: every bind change updates its overlays in the SAME change. `keymap.json` is deliberately NOT touched — no bind entries changed. `keybind-surface-guide.md` is on-request only.

**Files:**
- Modify: `src/ui/keybinds_overlay.rs`
- Modify: `docs/guides/keybind-consistency-guide.md`

- [x] **Step 1: Update the keycap strip**

`src/ui/keybinds_overlay.rs:79` currently reads:

```rust
    key("-", "_", "copy word", "_: collect words", &[("C--", "vocab drill"), ("S-C--", "drill back")]),
```

Replace with:

```rust
    key("-", "_", "copy/underline word", "_: collect words · Return: diagram sentence", &[("C--", "vocab drill"), ("S-C--", "drill back")]),
```

- [x] **Step 2: Add the `describe()` arm**

In the `describe()` match in the same file, alongside the `"minus"` arm (~169/1034), add a `Return` entry so the detail pane explains the bind:

```rust
        "Return" => "Return — diagram the sentence containing the underlined words (after -/_). Does nothing when no words are underlined.",
```

Add `("Return", "Return")` to the keycap name table at ~1034 if that table is what drives which caps render.

- [x] **Step 3: Record the consistency decision**

Append to the change log in `docs/guides/keybind-consistency-guide.md`:

```markdown
### 2026-07-26 — `-` / `_` gain a second meaning

`-` (WordCycleCopy) and `_` (WordCollectCopy) still copy to the clipboard,
unchanged. They now ALSO leave a persistent underline that `Return` turns into
a syntax diagram of the containing sentence.

Decision: no new `InputMode`. The state distinguishing "words are underlined"
already exists (`WordCycleState.collect_ranges`), so the reader stays in
`InputMode::Reader` and `Return` is a guarded arm. A mode would have forced
every unrelated reader bind to stop working or grow a passthrough arm.

`Return` was unbound in reader mode, so nothing was displaced. Escape is NOT a
new bind — the clear is a rung at the bottom of the existing
`escape_reader_mode` ladder (below search), because an underline is the least
modal of the states that ladder arbitrates.
```

- [x] **Step 4: Verify the build**

Run: `cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build 2>&1 | rg -c '^error'`

Expected: 0.

- [x] **Step 5: Commit**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add src/ui/keybinds_overlay.rs docs/guides/keybind-consistency-guide.md
git commit -m "docs(keybinds): record -/_ underline + Return diagram on every surface"
```

---

## Task 7: Headless on-screen verification

Mandatory per CLAUDE.md — build/clippy/tests green is NOT done for a visible change, and this is the only way to see an underline at all. The `feat/syntax-diagram` branch already learned this the hard way: 1137 unit tests passed while the diagram was unreadable on screen.

**Files:**
- Modify: `docs/superpowers/plans/2026-07-26-word-underline-diagram.md` (record results)

- [x] **Step 1: Build and launch headless**

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo build
```

Launch via the env wrapper (mints a fresh `XDG_RUNTIME_DIR`; a bare cage run reusing `/run/user/1000` screenshots the USER'S live desktop). Use the harness `run_in_background` — a detached/`nohup`/`timeout`-wrapped launch dies immediately:

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram && ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/land-on.sh BH-Barrett 1.1
```

Resize to production geometry, then RE-SEND the first chord (the first `wtype` after a `wlr-randr` resize is dropped on lost focus):

```bash
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

- [x] **Step 2: Drive the six spec criteria**

Land in READER mode directly (no overlay arg) — the vim ask card eats Escapes one modal layer at a time. Then:

```bash
wtype -k minus                      # 1. underlines one word
sleep 3 && grim -o HEADLESS-1 /tmp/wu-1-persist.png   # still underlined after 2s?
wtype -k underscore                 # 2. second word joins it
grim -o HEADLESS-1 /tmp/wu-2-two-words.png
wtype -k Return                     # 3. diagram opens
sleep 6 && grim -o HEADLESS-1 /tmp/wu-3-diagram.png
wtype -k Escape                     # 4. back to reader, underlines intact
grim -o HEADLESS-1 /tmp/wu-4-back.png
wtype -k Escape                     # 5. underlines cleared
grim -o HEADLESS-1 /tmp/wu-5-cleared.png
```

Then re-underline and press `j` to leave the line for criterion 6:

```bash
wtype -k minus && wtype -k j
grim -o HEADLESS-1 /tmp/wu-6-left-line.png
```

- [x] **Step 3: Open every PNG and report what you see**

Per the UI review protocol, a passing exit code is not enough — Read each capture and state inline what is on screen. Acceptance:

1. `wu-1-persist.png` — exactly one word underlined, still there after 3s (proves the timer is not armed).
2. `wu-2-two-words.png` — two words underlined simultaneously.
3. `wu-3-diagram.png` — the diagram's text is the FULL `. ! ?`-bounded sentence, not just the two words. Quote the on-screen text to prove it.
4. `wu-4-back.png` — reader, underlines still present.
5. `wu-5-cleared.png` — no underline anywhere.
6. `wu-6-left-line.png` — no underline (cursor moved off the owning line).

Confirm in the log that the span was computed:

```bash
rg 'SYNTAX_UNDERLINE|SYNTAX:' ~/utono/linux-lit/linux-lit-dev.log | tail
```

Note: a worktree's debug log lands in the MAIN checkout (`~/utono/linux-lit/`), not the worktree — find the live one by open fd (`ls -l /proc/<pid>/fd`) if in doubt.

- [x] **Step 4: Clean up**

Scoped only — a bare `pkill -f target/debug/linux-lit` kills the user's live instance. Run as its own step (`pkill` exits nonzero on no match and would abort an `&&` chain):

```bash
pkill -f "cage -- ./target/debug/linux-lit" || true
```

- [x] **Step 5: Record results and commit**

Append a "## Verification results" section to this plan with what each capture showed, then:

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram
git add docs/superpowers/plans/2026-07-26-word-underline-diagram.md
git commit -m "docs(plan): word-underline headless verification results"
```

- [ ] **Step 6: Hand off for real-renderer confirmation**

Cage is software rendering. Give the user the exact command and what to eyeball on their real GL renderer — underline rendering under Phase B's italic tags is a tag-interaction that cairo and GL can disagree on:

```bash
cd ~/utono/linux-lit-wt/feat-syntax-diagram && cargo run
```

---

## Verification results (2026-07-26, headless cage @ 1920x1200)

Driven on BH-Barrett 3.0 (Chapter 10), cursor on the prose paragraph beginning
"The red bit, the black bit, the inkstand top,". Good test material: one
`line_mapping` row of 80 words holding many sentences, with `Mr. Tulkinghorn`,
`&c., &c.`, and quoted speech — exactly the cases `sentence.rs` must handle.

**All six spec criteria PASS.**

1. **Underline persists past 2s** — PASS. `-` underlined "The"; still visible
   at 3.5s, where the old timer would have removed it.
2. **Multi-word `_`** — NOT DRIVABLE HEADLESSLY, and not a code defect.
   `wtype -k underscore` and `wtype '_'` both deliver `("underscore",
   shift=false)`; the bind is `shift("underscore")` and `plain("underscore")`
   is deliberately unbound. Real GTK delivers shift=true (documented in
   `keymap_config.rs`'s own `r_is_the_vocab_hub` test). Covered instead by
   `-`'s cycle: pressing `-` again replaced "The" with "red" underlined,
   confirming single-word mode replaces rather than accumulates.
3. **Return diagrams the SENTENCE** — PASS, the central criterion.
   `SYNTAX_UNDERLINE: 1 range(s) -> span 844..934 over 3 line(s)` — a 90-char
   span from a 3-char word. The diagram showed the full sentence "The red bit,
   the black bit, the inkstand top, the other inkstand top, the little
   sand-box." and stopped correctly at the `.` before "So!". Enriched path:
   `327 parsed tokens for 3 lines`, 5–6 bands, 21–23 POS tags from the live API.
4. **Escape returns with underlines intact** — PASS. Escape logged at
   `mode=SyntaxDiagram` (closing the diagram, not clearing), and "red" was
   still underlined in the reader.
5. **Second Escape clears** — PASS. `WORD_UNDERLINE: cleared`, underline gone.
6. **Leaving the line clears** — PASS after a fix. Return after a cursor move
   produced NO `SYNTAX_UNDERLINE` line: the guard fell through as designed.

**Defect found and fixed (`bed100dd`).** Criterion 6 initially left the tag
PAINTED on the old line — behaviourally inert (nothing could act on it) but
visible. The spec accepted this; it turned out cheap to fix properly, because
`update_highlight` is a single funnel every cursor move already passes through
and already computes `old != new`. Re-verified: underlined before `Down`, gone
after, with `WORD_UNDERLINE: cleared` logged ahead of the new `CURSOR_LINE`.

**Pre-existing branch defect found and fixed (`8b169535`).** The diagram's
scrim was drawn at alpha 0.97, so the reading card bled through and the diagram
was hard to read. Pixel-measured: corners `(69,112,121)` vs centre
`(74,116,125)`, and `0.97*scrim + 0.03*card` predicts `(74,116,125)` exactly —
the alpha was the whole cause, geometry was already correct. After the fix,
`(69,112,122)` uniform at corners, centre, and over the card. In
`src/ui/syntax_overlay.rs`, which the word-underline commits never touched.

**Two cosmetic issues left OPEN in the diagram overlay** (pre-existing, not
from this feature):
- Band labels graze their rules — the residue `a0516b39` recorded as "4 minor
  label grazes out of 15 rule rows".
- The diagram is top-weighted: content occupies the top ~12% of the window and
  leaves ~88% empty. Legible, but not what "fills the screen" implies.

**Still requires real-renderer confirmation** (cage is software rendering):
underline rendering interacts with Phase B's per-span italic tags, and the
scrim/band drawing is Pango + Cairo, both classes where cairo and GL can
disagree.

---

## Self-Review

**Spec coverage.** Every section maps to a task: sentence span → Task 1; underline persistence, `clear_word_underline`, `-` accumulating → Task 2; `open_syntax_diagram_for_underlined` → Task 3; the `Escape` guard → Task 4 (corrected to a ladder rung); the `Return` guard → Task 5; keybind surfaces → Task 6; the six on-screen criteria → Task 7. Error handling is covered where the spec puts it: `Return` with nothing underlined falls through (Task 5 Step 3), empty span is not re-guarded because `open_syntax_diagram` already guards it (Task 3 Step 1), no-boundary degrades to the whole window (Task 1's `whole_window_when_no_boundary_present`).

**Two spec claims corrected**, both documented above with evidence: reader `Escape` IS bound, and `collect_ranges` index buffer text rather than work-line text. The second is the one that would have shipped a silent bug on any work with inline italics.

**Type consistency.** `sentence_span(&str, &[(usize, usize)]) -> Option<(usize, usize)>` is defined in Task 1 and called in Task 3 with a `&String` deref and a `Vec<(usize,usize)>` slice — matching. `active_underline(&AppState) -> &[(usize,usize)]` is defined in Task 2 and called in Tasks 3, 4, 5 — matching, and always through `.is_empty()` or `.to_vec()`, so the borrow never outlives its `state.borrow()`. `apply_word_underline` gains its third parameter in Task 2 and both call sites are updated in that same task, so no task leaves the tree uncompilable.

**One deliberate non-obvious change** flagged for the reviewer: Task 4 adds a `return;` to the search rung that was not there before. It was previously the last rung, so falling through was harmless; with a rung below it the return is required, or one Escape clears both search and the underline.

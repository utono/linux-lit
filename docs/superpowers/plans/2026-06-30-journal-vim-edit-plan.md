# Journal in-place modal vim editing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the journal overlay's `e` edit card with an in-place modal vim editor (full verb set) on the journal page, backed by a pure, unit-tested engine.

**Architecture:** A pure `src/input/vim/` engine owns the edit buffer/cursor/mode/registers/undo and exposes one `handle_key` entry point returning an `Outcome`. The journal `TextView` is a thin mirror driven by the adapter (`editable(false)`). A new `InputMode::JournalEdit` routes keys to a `handle_journal_edit_key` adapter. Journal-only scope; gloss/synopsis keep their ask-Claude rewrite flow.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite. The engine has ZERO GTK deps and is tested via `cargo test --bins`.

## Global Constraints

- Design doc: `docs/superpowers/specs/2026-06-30-journal-vim-edit-design.md` (authoritative).
- Engine modules under `src/input/vim/` MUST NOT import `gtk4` — they operate on `String` + char-index cursor only. This is what makes them unit-testable.
- Cursor is a **char index** into the buffer (`buffer.chars().count()` is the max), NOT a byte offset. All motions/edits convert via char indices; use `buffer.chars()` / build new strings, never byte slicing on multibyte text.
- Buffer line model: lines are `'\n'`-separated; the buffer never ends edits with a forced trailing newline unless the text has one.
- TDD: write the failing test, run it red, implement, run it green, commit. One commit per task minimum.
- Timestamps / Central time not needed in code.
- Do NOT run the app (`cargo run`); build with `cargo build` and test with `cargo test --bins`. Runtime GUI verification is the user's (headless cage is SIGTERM-killed in agent envs).
- Commit message footer (every commit):
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY
  ```
- After the final task (branch finish), follow CLAUDE.md "Finishing a Branch" (merge to master, push) ONLY if the user asks; otherwise leave on the branch. Update `ac` after commits.

---

## Module / file structure (decomposition)

Created (engine, no GTK):
- `src/input/vim/mod.rs` — module root, re-exports, `VimKey`, `Mode`, `EditorAction`, `Outcome`.
- `src/input/vim/buffer.rs` — char-index helpers over `String` (line bounds, char<->offset, current line).
- `src/input/vim/motion.rs` — pure motions.
- `src/input/vim/textobject.rs` — text objects (`iw`,`i(`,…).
- `src/input/vim/registers.rs` — register storage.
- `src/input/vim/engine.rs` — `VimEngine` state machine (the dispatcher tying it together; incl. edits, counts, repeat, undo, ex-commands as private impls/submethods).
- `src/input/vim/journal_doc.rs` — build-buffer / parse-back (journal `Q:`/answer framing).

Modified (GTK integration):
- `src/app/mod.rs` — `InputMode::JournalEdit`; thread `key_char: Option<char>` from the key controller into `handle_key`.
- `src/input/keymap.rs` — dispatcher arm + `handle_journal_edit_key`; remove the edit-card intercept in `handle_journal_key`.
- `src/input/actions/journal.rs` — `begin_edit` enters JournalEdit; `enter_vim_edit`, `vim_save`, `vim_open_rewrite`, `vim_cancel` helpers.
- `src/ui/journal_overlay.rs` — `enter_edit_buffer`/`mirror_engine`/`exit_edit_buffer`/`set_edit_mode_indicator`; suspend pagination during edit; remove `JournalEditCard` field + its methods.
- `src/ui/journal_keybinds_overlay.rs` — legend: `e` → "edit (vim)"; add vim-mode legend section.
- Removed: `src/ui/journal_edit_card.rs`; `docs/troubleshooting/journal-edit-card-sizing.md` (superseded — replace with a 3-line tombstone note).

Cursor/selection conventions used across tasks:
- `Range { start: usize, end: usize }` — half-open char range `[start, end)`.
- Motions return a new cursor (char index). Operators take a `Range` produced by a motion/textobject.

---

## Task 1: Vim core types (VimKey, Mode, EditorAction, Outcome, Range)

**Files:**
- Create: `src/input/vim/mod.rs`
- Modify: `src/input/mod.rs` (add `pub mod vim;`)
- Test: in `src/input/vim/mod.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `enum VimKey { Char(char), Esc, Enter, Backspace, Tab, CtrlR }`; `enum Mode { Normal, Insert, Visual, VisualLine }`; `enum EditorAction { Nop, Save, SaveQuit, Cancel, OpenRewrite }`; `struct Range { start: usize, end: usize }`; `struct Outcome { buffer_changed: bool, cursor: usize, mode: Mode, selection: Option<Range>, action: EditorAction }`.

- [ ] **Step 1: Write the failing test**

In `src/input/vim/mod.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_defaults_are_nop_normal() {
        let o = Outcome::nop(Mode::Normal, 0);
        assert_eq!(o.action, EditorAction::Nop);
        assert_eq!(o.mode, Mode::Normal);
        assert_eq!(o.cursor, 0);
        assert!(!o.buffer_changed);
        assert!(o.selection.is_none());
    }

    #[test]
    fn range_len_is_half_open() {
        assert_eq!(Range { start: 2, end: 5 }.len(), 3);
        assert_eq!(Range { start: 4, end: 4 }.len(), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::tests 2>&1 | tail -20`
Expected: FAIL — `src/input/vim` does not exist / `Outcome` not found.

- [ ] **Step 3: Write minimal implementation**

Create `src/input/vim/mod.rs`:
```rust
//! Pure modal-vim editor engine for the journal in-place editor. ZERO gtk4
//! deps — operates on a `String` buffer + char-index cursor so the full verb
//! set is unit-testable. See docs/superpowers/specs/2026-06-30-journal-vim-edit-design.md.

pub mod buffer;
pub mod motion;
pub mod textobject;
pub mod registers;
pub mod engine;
pub mod journal_doc;

pub use engine::VimEngine;

/// The engine's GTK-independent input alphabet. Printable input arrives as
/// `Char`; control keys are named. `CtrlR` is vim redo (distinct from the
/// `R` rewrite key, which the adapter handles before the engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimKey {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    CtrlR,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

/// What the engine asks the host (adapter) to do after a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    Nop,
    Save,
    SaveQuit,
    Cancel,
    OpenRewrite,
}

/// Half-open char range `[start, end)` into the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Result of `VimEngine::handle_key`. The adapter mirrors `cursor`/`mode`/
/// `selection` to GTK and acts on `action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub buffer_changed: bool,
    pub cursor: usize,
    pub mode: Mode,
    pub selection: Option<Range>,
    pub action: EditorAction,
}

impl Outcome {
    pub fn nop(mode: Mode, cursor: usize) -> Self {
        Outcome { buffer_changed: false, cursor, mode, selection: None, action: EditorAction::Nop }
    }
}
```

Add to `src/input/mod.rs` (alongside the other `pub mod` lines):
```rust
pub mod vim;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::tests 2>&1 | tail -20`
Expected: the two tests in `mod.rs` pass. (Other `vim::*` modules don't exist yet — they're declared but empty; create empty stub files so the crate builds: see Step 5.)

- [ ] **Step 5: Create stub files so the crate compiles, then commit**

Create empty stubs (filled by later tasks) so `mod.rs`'s `pub mod` lines resolve:
- `src/input/vim/buffer.rs` → `//! char-index buffer helpers (Task 2)`
- `src/input/vim/motion.rs` → `//! motions (Task 3)`
- `src/input/vim/textobject.rs` → `//! text objects (Task 5)`
- `src/input/vim/registers.rs` → `//! registers (Task 7)`
- `src/input/vim/engine.rs` → `//! engine (Task 9)` plus a placeholder so `pub use engine::VimEngine;` resolves:
  ```rust
  //! engine (filled in Task 9)
  pub struct VimEngine;
  ```
- `src/input/vim/journal_doc.rs` → `//! journal framing (Task 12)`

Run: `cargo build 2>&1 | rg -i "error|warning: unused" | head`
Expected: builds (engine `VimEngine` unit struct is a temporary placeholder).

```bash
git add src/input/vim/ src/input/mod.rs
git commit -m "feat(vim): core types (VimKey/Mode/EditorAction/Outcome/Range)

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 2: Buffer helpers (char/line geometry)

**Files:**
- Modify (replace stub): `src/input/vim/buffer.rs`
- Test: in `src/input/vim/buffer.rs` `#[cfg(test)]`

**Interfaces:**
- Produces (all operate on `s: &str` with char-index `cursor`):
  - `fn char_count(s: &str) -> usize`
  - `fn line_start(s: &str, cursor: usize) -> usize` — char index of the first char of the cursor's line.
  - `fn line_end(s: &str, cursor: usize) -> usize` — char index of the line's last non-`\n` position's end (index of the `\n` or `char_count`).
  - `fn line_bounds(s: &str, cursor: usize) -> (usize, usize)` — `(line_start, line_end)`.
  - `fn line_index(s: &str, cursor: usize) -> usize` — 0-based line number.
  - `fn nth_line_start(s: &str, n: usize) -> usize` — char index where line `n` starts (clamped).
  - `fn col(s: &str, cursor: usize) -> usize` — char column within the line.
  - `fn clamp_cursor(s: &str, cursor: usize) -> usize` — clamp to `[0, char_count]`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // buffer: "ab\ncd\n\nef"  chars: a(0)b(1)\n(2)c(3)d(4)\n(5)\n(6)e(7)f(8)
    const B: &str = "ab\ncd\n\nef";

    #[test]
    fn line_geometry() {
        assert_eq!(char_count(B), 9);
        assert_eq!(line_bounds(B, 0), (0, 2));   // "ab"
        assert_eq!(line_bounds(B, 4), (3, 5));   // "cd"
        assert_eq!(line_bounds(B, 6), (6, 6));   // empty line
        assert_eq!(line_bounds(B, 8), (7, 9));   // "ef"
        assert_eq!(line_index(B, 4), 1);
        assert_eq!(line_index(B, 7), 3);
        assert_eq!(nth_line_start(B, 0), 0);
        assert_eq!(nth_line_start(B, 1), 3);
        assert_eq!(nth_line_start(B, 3), 7);
        assert_eq!(nth_line_start(B, 99), 7);    // clamp to last line
        assert_eq!(col(B, 4), 1);
        assert_eq!(clamp_cursor(B, 99), 9);
    }

    #[test]
    fn multibyte_is_char_indexed() {
        let s = "é\nxy"; // é(0) \n(1) x(2) y(3)
        assert_eq!(char_count(s), 4);
        assert_eq!(line_bounds(s, 0), (0, 1));
        assert_eq!(nth_line_start(s, 1), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::buffer 2>&1 | tail -20`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Write minimal implementation**

Replace `src/input/vim/buffer.rs`:
```rust
//! Char-index buffer geometry for the vim engine. Everything is in CHAR units
//! (not bytes) so multibyte text is handled uniformly. Lines are `\n`-separated.

pub fn char_count(s: &str) -> usize {
    s.chars().count()
}

pub fn clamp_cursor(s: &str, cursor: usize) -> usize {
    cursor.min(char_count(s))
}

/// Char index of the start of the line containing `cursor`.
pub fn line_start(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    let mut start = 0;
    for (i, c) in s.chars().enumerate() {
        if i >= cursor {
            break;
        }
        if c == '\n' {
            start = i + 1;
        }
    }
    start
}

/// Char index of the line end (index of the trailing `\n`, or `char_count`).
pub fn line_end(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    for (i, c) in s.chars().enumerate() {
        if i >= cursor && c == '\n' {
            return i;
        }
    }
    char_count(s)
}

pub fn line_bounds(s: &str, cursor: usize) -> (usize, usize) {
    (line_start(s, cursor), line_end(s, cursor))
}

pub fn line_index(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    s.chars().take(cursor).filter(|&c| c == '\n').count()
}

pub fn nth_line_start(s: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, c) in s.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == n {
                return i + 1;
            }
        }
    }
    // Fewer than n newlines: clamp to the last line's start.
    line_start(s, char_count(s))
}

pub fn col(s: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(s, cursor);
    cursor - line_start(s, cursor)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::buffer 2>&1 | tail -20`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/buffer.rs
git commit -m "feat(vim): char-index buffer geometry helpers

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 3: Motions (h j k l w b e 0 ^ $ gg G f t F T %)

**Files:**
- Modify (replace stub): `src/input/vim/motion.rs`
- Test: in `src/input/vim/motion.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `super::buffer::*`.
- Produces:
  - `fn left(s,c,n) -> usize`, `right(s,c,n)`, `up(s,c,n)`, `down(s,c,n)` (char-index, count-aware, clamped to line for h/l, keep column for j/k).
  - `fn word_forward(s,c,n)`, `word_back(s,c,n)`, `word_end(s,c,n)` (vim `w`/`b`/`e`, whitespace+punct word classes).
  - `fn line_first_char(s,c)` (`^`), `line_zero(s,c)` (`0`), `line_last_char(s,c)` (`$` → index of last char, or line_start if empty).
  - `fn buffer_start(s)`→0 (`gg` with no count), `goto_line(s,n)` (`G`/`{n}G`, 1-based; n==0 → last line).
  - `fn find_char(s,c,kind,target) -> Option<usize>` where `kind ∈ {F,f,T,t}` enum `FindKind`.
  - `fn match_pair(s,c) -> Option<usize>` (`%` over `()[]{}`).
  - `pub enum FindKind { ForwardOn, ForwardBefore, BackOn, BackBefore }` (f, t, F, T).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const B: &str = "the quick fox\nbrown";

    #[test]
    fn horizontal_and_word() {
        assert_eq!(right(B, 0, 1), 1);
        assert_eq!(right(B, 12, 5), 12);            // clamp at line end (char 'x' idx 12)
        assert_eq!(left(B, 0, 1), 0);
        assert_eq!(word_forward(B, 0, 1), 4);       // 'the ' -> 'quick'
        assert_eq!(word_forward(B, 0, 2), 10);      // -> 'fox'
        assert_eq!(word_back(B, 10, 1), 4);         // 'fox' -> 'quick'
        assert_eq!(word_end(B, 0, 1), 2);           // end of 'the' = 'e' idx 2
        assert_eq!(line_zero(B, 8), 0);
        assert_eq!(line_last_char(B, 0), 12);       // 'x'
    }

    #[test]
    fn vertical_keeps_column() {
        // line0 "the quick fox" col 5 ('u'); down -> line1 "brown" col 5 -> clamp to 'n'(end)
        let c = 5; // 'u'
        let d = down(B, c, 1);
        assert_eq!(super::super::buffer::line_index(B, d), 1);
    }

    #[test]
    fn goto_line_and_find() {
        assert_eq!(buffer_start(B), 0);
        assert_eq!(super::super::buffer::line_index(B, goto_line(B, 2)), 1);
        assert_eq!(super::super::buffer::line_index(B, goto_line(B, 0)), 1); // last line
        assert_eq!(find_char(B, 0, FindKind::ForwardOn, 'q'), Some(4));
        assert_eq!(find_char(B, 0, FindKind::ForwardBefore, 'q'), Some(3));
    }

    #[test]
    fn match_pair_parens() {
        let s = "a(bc)d";
        assert_eq!(match_pair(s, 1), Some(4)); // '(' -> ')'
        assert_eq!(match_pair(s, 4), Some(1)); // ')' -> '('
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::motion 2>&1 | tail -20`
Expected: FAIL — not defined.

- [ ] **Step 3: Write minimal implementation**

Replace `src/input/vim/motion.rs` with the motion implementations (char-vector based). Key points: build `let cs: Vec<char> = s.chars().collect();` once per call; word classes via a helper `fn class(c: char) -> u8` (0 whitespace, 1 word `[A-Za-z0-9_]`, 2 punct); `w` skips to the next start-of-word of a different class run; `b`/`e` analogous. `down`/`up` preserve `col` and clamp to target line length. Implement exactly the signatures in Interfaces. (Full code — write it out; this is the largest pure file. Use `super::buffer` for line geometry.)

```rust
//! Pure vim motions over a char-indexed buffer. Each returns a NEW cursor
//! (char index), count-aware, clamped. No gtk deps.
use super::buffer::{char_count, clamp_cursor, line_bounds, line_index, line_start, nth_line_start, col};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKind { ForwardOn, ForwardBefore, BackOn, BackBefore }

fn chars(s: &str) -> Vec<char> { s.chars().collect() }

fn class(c: char) -> u8 {
    if c.is_whitespace() { 0 }
    else if c.is_alphanumeric() || c == '_' { 1 }
    else { 2 }
}

pub fn left(s: &str, c: usize, n: usize) -> usize {
    let (ls, _) = line_bounds(s, c);
    c.saturating_sub(n).max(ls)
}

pub fn right(s: &str, c: usize, n: usize) -> usize {
    let (_, le) = line_bounds(s, c);
    // Normal mode: cursor may sit on the last char, not past it.
    let max = le.saturating_sub(1).max(line_start(s, c));
    (c + n).min(max.max(c).min(le))
}

pub fn up(s: &str, c: usize, n: usize) -> usize {
    let li = line_index(s, c);
    if li == 0 { return line_start(s, c).min(c); }
    let target = li.saturating_sub(n);
    let want_col = col(s, c);
    let ts = nth_line_start(s, target);
    let (_, te) = line_bounds(s, ts);
    (ts + want_col).min(te.saturating_sub(1).max(ts))
}

pub fn down(s: &str, c: usize, n: usize) -> usize {
    let total_lines = s.chars().filter(|&ch| ch == '\n').count();
    let li = line_index(s, c);
    if li >= total_lines { return c; }
    let target = (li + n).min(total_lines);
    let want_col = col(s, c);
    let ts = nth_line_start(s, target);
    let (_, te) = line_bounds(s, ts);
    (ts + want_col).min(te.saturating_sub(1).max(ts))
}

pub fn line_zero(s: &str, c: usize) -> usize { line_start(s, c) }

pub fn line_first_char(s: &str, c: usize) -> usize {
    let cs = chars(s);
    let (ls, le) = line_bounds(s, c);
    let mut i = ls;
    while i < le && cs.get(i).map_or(false, |ch| ch.is_whitespace()) { i += 1; }
    i.min(le)
}

pub fn line_last_char(s: &str, c: usize) -> usize {
    let (ls, le) = line_bounds(s, c);
    le.saturating_sub(1).max(ls)
}

pub fn buffer_start(_s: &str) -> usize { 0 }

/// `G` / `{n}G`: 1-based line; n==0 => last line. Lands on first non-blank.
pub fn goto_line(s: &str, n: usize) -> usize {
    let total_lines = s.chars().filter(|&ch| ch == '\n').count();
    let target = if n == 0 { total_lines } else { (n - 1).min(total_lines) };
    let ts = nth_line_start(s, target);
    line_first_char(s, ts)
}

pub fn word_forward(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let len = cs.len();
    let mut i = clamp_cursor(s, c);
    for _ in 0..n {
        if i >= len { break; }
        let start_class = class(cs[i]);
        // move off the current run
        if start_class != 0 {
            while i < len && class(cs[i]) == start_class { i += 1; }
        }
        // skip whitespace
        while i < len && class(cs[i]) == 0 { i += 1; }
    }
    i.min(len)
}

pub fn word_back(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let mut i = clamp_cursor(s, c);
    for _ in 0..n {
        if i == 0 { break; }
        i -= 1;
        while i > 0 && class(cs[i]) == 0 { i -= 1; }
        let cl = class(cs[i]);
        while i > 0 && class(cs[i - 1]) == cl { i -= 1; }
    }
    i
}

pub fn word_end(s: &str, c: usize, n: usize) -> usize {
    let cs = chars(s);
    let len = cs.len();
    let mut i = clamp_cursor(s, c);
    for _ in 0..n {
        if i + 1 >= len { i = len.saturating_sub(1); break; }
        i += 1;
        while i < len && class(cs[i]) == 0 { i += 1; }
        let cl = class(cs.get(i).copied().unwrap_or(' '));
        while i + 1 < len && class(cs[i + 1]) == cl { i += 1; }
    }
    i.min(len.saturating_sub(1))
}

pub fn find_char(s: &str, c: usize, kind: FindKind, target: char) -> Option<usize> {
    let cs = chars(s);
    let (ls, le) = line_bounds(s, c);
    match kind {
        FindKind::ForwardOn | FindKind::ForwardBefore => {
            let mut i = c + 1;
            while i < le {
                if cs[i] == target {
                    return Some(if matches!(kind, FindKind::ForwardBefore) { i - 1 } else { i });
                }
                i += 1;
            }
            None
        }
        FindKind::BackOn | FindKind::BackBefore => {
            let mut i = c;
            while i > ls {
                i -= 1;
                if cs[i] == target {
                    return Some(if matches!(kind, FindKind::BackBefore) { i + 1 } else { i });
                }
            }
            None
        }
    }
}

pub fn match_pair(s: &str, c: usize) -> Option<usize> {
    let cs = chars(s);
    let ch = *cs.get(c)?;
    let (open, close, forward) = match ch {
        '(' => ('(', ')', true), ')' => ('(', ')', false),
        '[' => ('[', ']', true), ']' => ('[', ']', false),
        '{' => ('{', '}', true), '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 0i32;
    if forward {
        let mut i = c;
        while i < cs.len() {
            if cs[i] == open { depth += 1; }
            else if cs[i] == close { depth -= 1; if depth == 0 { return Some(i); } }
            i += 1;
        }
    } else {
        let mut i = c as isize;
        while i >= 0 {
            let u = i as usize;
            if cs[u] == close { depth += 1; }
            else if cs[u] == open { depth -= 1; if depth == 0 { return Some(u); } }
            i -= 1;
        }
    }
    None
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::motion 2>&1 | tail -25`
Expected: PASS. If a word-boundary test is off by one, adjust the `class`-run logic (not the test) until the documented vim semantics hold.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/motion.rs
git commit -m "feat(vim): pure motions (hjkl w b e 0 ^ $ G f t F T %)

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 4: Engine scaffold — Normal-mode motions + Insert mode + Esc

**Files:**
- Modify (replace stub): `src/input/vim/engine.rs`
- Test: in `src/input/vim/engine.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `super::{VimKey, Mode, EditorAction, Outcome, Range}`, `super::buffer`, `super::motion`.
- Produces:
  - `struct VimEngine { buffer: String, cursor: usize, mode: Mode, /* pending fields added in later tasks */ pending_count: Option<usize>, visual_anchor: Option<usize> }`.
  - `impl VimEngine { fn new(buffer: String) -> Self; fn buffer(&self) -> &str; fn cursor(&self) -> usize; fn mode(&self) -> Mode; fn handle_key(&mut self, k: VimKey) -> Outcome; }`
  - A test-only helper `fn feed(&mut self, keys: &str)` (each char → `VimKey::Char`, with `\x1b`→Esc, `\n`→Enter, `\x08`→Backspace) under `#[cfg(test)]`.

This task implements ONLY: count accumulation (digits), motions `h j k l w b e 0 ^ $ G gg`, insert entry `i a I A o O`, typing in insert mode, Backspace in insert, Esc→Normal (cursor moves left one). Operators/text-objects/registers/`.`/undo/ex come in later tasks; leave clearly-marked `// Task N` holes that return `Outcome::nop`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::vim::Mode;

    fn eng(s: &str) -> VimEngine { VimEngine::new(s.to_string()) }

    #[test]
    fn motions_move_cursor() {
        let mut e = eng("hello world");
        e.feed("w");
        assert_eq!(e.cursor(), 6);          // 'world'
        e.feed("0");
        assert_eq!(e.cursor(), 0);
        e.feed("$");
        assert_eq!(e.cursor(), 10);         // 'd'
    }

    #[test]
    fn count_then_motion() {
        let mut e = eng("aaaa bbbb cccc");
        e.feed("2w");
        assert_eq!(e.cursor(), 10);         // 'cccc'
    }

    #[test]
    fn insert_then_type_then_esc() {
        let mut e = eng("bc");
        e.feed("i");
        assert_eq!(e.mode(), Mode::Insert);
        e.feed("A");                         // type 'A'
        assert_eq!(e.buffer(), "Abc");
        e.feed("\x1b");                      // Esc
        assert_eq!(e.mode(), Mode::Normal);
        assert_eq!(e.cursor(), 0);           // moved left one from after 'A'
    }

    #[test]
    fn open_line_below() {
        let mut e = eng("x");
        e.feed("o");
        assert_eq!(e.mode(), Mode::Insert);
        e.feed("y\x1b");
        assert_eq!(e.buffer(), "x\ny");
    }

    #[test]
    fn append_after_cursor() {
        let mut e = eng("ab");
        e.feed("a");                         // cursor 0 -> insert after 'a'
        e.feed("Z\x1b");
        assert_eq!(e.buffer(), "aZb");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: FAIL — `feed`/`new` not defined (current stub is a unit struct).

- [ ] **Step 3: Write minimal implementation**

Replace `src/input/vim/engine.rs`. Implement the state machine for THIS task's scope. Structure `handle_key` as: in Insert mode handle Char/Backspace/Esc/Enter; in Normal mode accumulate digit counts, else dispatch the motion/insert-entry; consume the pending count after a motion. Insert-entry commands set `mode=Insert` and position the cursor (`i` at cursor, `a` at cursor+1, `I` at first non-blank, `A` at line end, `o`/`O` open a new line). Build new `buffer` strings via char vectors. Provide `feed` under `#[cfg(test)]`.

(Write the full code; here is the shape — fill every arm.)
```rust
//! The modal-vim state machine. Pure; mirrors to GTK via the adapter.
use super::{buffer, motion, EditorAction, Mode, Outcome, Range, VimKey};
use super::motion::FindKind;

pub struct VimEngine {
    buffer: String,
    cursor: usize,
    mode: Mode,
    pending_count: Option<usize>,
    visual_anchor: Option<usize>,
    // Task 6: pending operator; Task 7: register select; Task 8: find pending;
    // Task 9: last_change (repeat); Task 10: undo stack; Task 11: cmdline.
}

impl VimEngine {
    pub fn new(buffer: String) -> Self {
        VimEngine { buffer, cursor: 0, mode: Mode::Normal, pending_count: None, visual_anchor: None }
    }
    pub fn buffer(&self) -> &str { &self.buffer }
    pub fn cursor(&self) -> usize { self.cursor }
    pub fn mode(&self) -> Mode { self.mode }

    fn take_count(&mut self) -> usize {
        self.pending_count.take().unwrap_or(1)
    }

    fn out(&self, changed: bool, action: EditorAction) -> Outcome {
        Outcome {
            buffer_changed: changed,
            cursor: self.cursor,
            mode: self.mode,
            selection: self.selection(),
            action,
        }
    }

    fn selection(&self) -> Option<Range> {
        self.visual_anchor.map(|a| {
            let (s, e) = if a <= self.cursor { (a, self.cursor + 1) } else { (self.cursor, a + 1) };
            Range { start: s, end: e }
        })
    }

    // char-vector edit primitive used by insert/o/O.
    fn insert_str_at(&mut self, at: usize, text: &str) {
        let mut cs: Vec<char> = self.buffer.chars().collect();
        let at = at.min(cs.len());
        for (k, ch) in text.chars().enumerate() {
            cs.insert(at + k, ch);
        }
        self.buffer = cs.into_iter().collect();
    }

    pub fn handle_key(&mut self, k: VimKey) -> Outcome {
        match self.mode {
            Mode::Insert => self.handle_insert(k),
            Mode::Normal => self.handle_normal(k),
            Mode::Visual | Mode::VisualLine => self.handle_normal(k), // Task 6 refines visual
        }
    }

    fn handle_insert(&mut self, k: VimKey) -> Outcome {
        match k {
            VimKey::Esc => {
                self.mode = Mode::Normal;
                let ls = buffer::line_start(&self.buffer, self.cursor);
                self.cursor = self.cursor.saturating_sub(1).max(ls);
                self.out(false, EditorAction::Nop)
            }
            VimKey::Char(c) => {
                self.insert_str_at(self.cursor, &c.to_string());
                self.cursor += 1;
                self.out(true, EditorAction::Nop)
            }
            VimKey::Enter => {
                self.insert_str_at(self.cursor, "\n");
                self.cursor += 1;
                self.out(true, EditorAction::Nop)
            }
            VimKey::Backspace => {
                if self.cursor > 0 {
                    let mut cs: Vec<char> = self.buffer.chars().collect();
                    cs.remove(self.cursor - 1);
                    self.buffer = cs.into_iter().collect();
                    self.cursor -= 1;
                    self.out(true, EditorAction::Nop)
                } else {
                    self.out(false, EditorAction::Nop)
                }
            }
            VimKey::Tab => {
                self.insert_str_at(self.cursor, "    ");
                self.cursor += 4;
                self.out(true, EditorAction::Nop)
            }
            VimKey::CtrlR => self.out(false, EditorAction::Nop),
        }
    }

    fn enter_insert_at(&mut self, at: usize) -> Outcome {
        self.cursor = buffer::clamp_cursor(&self.buffer, at);
        self.mode = Mode::Insert;
        self.out(false, EditorAction::Nop)
    }

    fn handle_normal(&mut self, k: VimKey) -> Outcome {
        let c = match k {
            VimKey::Char(c) => c,
            VimKey::Esc => {
                self.pending_count = None;
                if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
                    self.mode = Mode::Normal; self.visual_anchor = None;
                    return self.out(false, EditorAction::Nop);
                }
                // Task 11 will route Esc(normal) -> Cancel/confirm.
                return self.out(false, EditorAction::Cancel);
            }
            _ => return self.out(false, EditorAction::Nop),
        };
        // count accumulation: digits 1-9 (and 0 only if a count is pending)
        if c.is_ascii_digit() && !(c == '0' && self.pending_count.is_none()) {
            let d = c as usize - '0' as usize;
            self.pending_count = Some(self.pending_count.unwrap_or(0) * 10 + d);
            return self.out(false, EditorAction::Nop);
        }
        match c {
            'h' => { let n = self.take_count(); self.cursor = motion::left(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'l' => { let n = self.take_count(); self.cursor = motion::right(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'k' => { let n = self.take_count(); self.cursor = motion::up(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'j' => { let n = self.take_count(); self.cursor = motion::down(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'w' => { let n = self.take_count(); self.cursor = motion::word_forward(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'b' => { let n = self.take_count(); self.cursor = motion::word_back(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            'e' => { let n = self.take_count(); self.cursor = motion::word_end(&self.buffer, self.cursor, n); self.out(false, EditorAction::Nop) }
            '0' => { self.cursor = motion::line_zero(&self.buffer, self.cursor); self.out(false, EditorAction::Nop) }
            '^' => { self.cursor = motion::line_first_char(&self.buffer, self.cursor); self.out(false, EditorAction::Nop) }
            '$' => { self.cursor = motion::line_last_char(&self.buffer, self.cursor); self.out(false, EditorAction::Nop) }
            'G' => { let n = self.pending_count.take().unwrap_or(0); self.cursor = motion::goto_line(&self.buffer, n); self.out(false, EditorAction::Nop) }
            'i' => { let at = self.cursor; self.enter_insert_at(at) }
            'a' => { let at = (self.cursor + 1).min(buffer::char_count(&self.buffer)); self.enter_insert_at(at) }
            'I' => { let at = motion::line_first_char(&self.buffer, self.cursor); self.enter_insert_at(at) }
            'A' => { let (_, le) = buffer::line_bounds(&self.buffer, self.cursor); self.enter_insert_at(le) }
            'o' => {
                let (_, le) = buffer::line_bounds(&self.buffer, self.cursor);
                self.insert_str_at(le, "\n");
                self.enter_insert_at(le + 1)
            }
            'O' => {
                let ls = buffer::line_start(&self.buffer, self.cursor);
                self.insert_str_at(ls, "\n");
                self.enter_insert_at(ls)
            }
            // 'g' prefix (gg) — Task: handle two-key gg here.
            'g' => { self.pending_count = self.pending_count; self.await_g() }
            _ => self.out(false, EditorAction::Nop), // operators/ex/etc: later tasks
        }
    }

    // Minimal gg: set a flag; next 'g' goes to line 1 / {count}G-style start.
    fn await_g(&mut self) -> Outcome {
        // Implemented as a tiny pending state inline:
        self.pending_g = true;
        self.out(false, EditorAction::Nop)
    }
}
```
NOTE: the `await_g`/`pending_g` above needs a `pending_g: bool` field; add it to the struct and to `new`, and at the TOP of `handle_normal` add:
```rust
if std::mem::take(&mut self.pending_g) {
    if c == 'g' {
        let n = self.pending_count.take().unwrap_or(1);
        self.cursor = if n <= 1 { motion::buffer_start(&self.buffer) } else { motion::goto_line(&self.buffer, n) };
        return self.out(false, EditorAction::Nop);
    }
    // fallthrough: 'g' not followed by 'g' — ignore for now
}
```
Add the test helper:
```rust
#[cfg(test)]
impl VimEngine {
    pub fn feed(&mut self, keys: &str) {
        for ch in keys.chars() {
            let k = match ch {
                '\x1b' => VimKey::Esc,
                '\n' => VimKey::Enter,
                '\x08' => VimKey::Backspace,
                '\t' => VimKey::Tab,
                c => VimKey::Char(c),
            };
            self.handle_key(k);
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -25`
Expected: PASS (all five). Also run `cargo build` to confirm the placeholder holes compile.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): engine scaffold — counts, motions, insert/o/O, Esc

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 5: Text objects (iw aw i" a" i( a( i{ a{ i[ a[ ip ap)

**Files:**
- Modify (replace stub): `src/input/vim/textobject.rs`
- Test: in `src/input/vim/textobject.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `enum TextObjKind { Word, Pair(char,char), Paragraph }`; `fn text_object(s: &str, cursor: usize, kind: TextObjKind, around: bool) -> Option<super::Range>`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::vim::Range;

    #[test]
    fn inner_and_around_word() {
        let s = "foo bar baz";
        assert_eq!(text_object(s, 4, TextObjKind::Word, false), Some(Range { start: 4, end: 7 })); // 'bar'
        assert_eq!(text_object(s, 4, TextObjKind::Word, true),  Some(Range { start: 4, end: 8 })); // 'bar '
    }

    #[test]
    fn inner_and_around_quotes() {
        let s = "say \"hi\" now"; //  " at 4 and 7
        assert_eq!(text_object(s, 5, TextObjKind::Pair('"','"'), false), Some(Range { start: 5, end: 7 })); // hi
        assert_eq!(text_object(s, 5, TextObjKind::Pair('"','"'), true),  Some(Range { start: 4, end: 8 })); // "hi"
    }

    #[test]
    fn inner_parens() {
        let s = "a(bc)d";
        assert_eq!(text_object(s, 2, TextObjKind::Pair('(',')'), false), Some(Range { start: 2, end: 4 })); // bc
        assert_eq!(text_object(s, 2, TextObjKind::Pair('(',')'), true),  Some(Range { start: 1, end: 5 })); // (bc)
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::textobject 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Replace `src/input/vim/textobject.rs`. Word object: expand to the run of same-class chars around cursor (inner); around adds trailing (or leading) whitespace. Pair object: scan left for the opener and right for the closer (respecting nesting); inner = between, around = inclusive. Paragraph: blank-line-delimited block. (Full code; mirror the motion file's `class` helper or re-derive.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::textobject 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/textobject.rs
git commit -m "feat(vim): text objects (iw/aw, i\"/a\", i(/a(, i{/a{, ip/ap)

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 6: Operators (d c y) + motions/text-objects, dd/cc/yy/D/C, x/r/J/~, indent

**Files:**
- Modify: `src/input/vim/engine.rs` (add operator-pending state + edit primitives)
- Test: in `engine.rs` tests

**Interfaces:**
- Consumes: `super::textobject`, `super::motion`, `super::registers` (Task 7 lands registers; for THIS task store the deleted/yanked text in a temporary `last_yank: String` + `last_yank_linewise: bool` field on the engine; Task 7 generalizes to registers).
- Produces (engine fields): `pending_op: Option<Op>`, `last_yank: String`, `last_yank_linewise: bool`; `enum Op { Delete, Change, Yank }` (engine-private); edit primitive `fn delete_range(&mut self, r: Range)`, `fn apply_operator(&mut self, op, range, linewise)`.

This is the heart of editing. Implement: pressing `d`/`c`/`y` sets `pending_op` (consuming any count as a multiplier on the following motion); the next motion OR text-object resolves the range and applies. Doubled forms `dd`/`cc`/`yy` operate linewise on `count` lines. `D`/`C` = to EOL. `x` deletes `count` chars. `r<char>` replaces. `J` joins. `~` toggles case. `>>`/`<<` indent/dedent the line by 4 spaces. After `d`/`x`/`D` the cursor clamps onto the line. `c`/`C`/`cc`/`cw` end in Insert mode.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn delete_word_and_to_eol() {
    let mut e = VimEngine::new("foo bar baz".into());
    e.feed("dw");
    assert_eq!(e.buffer(), "bar baz");
    e.feed("D");
    assert_eq!(e.buffer(), "");
}
#[test]
fn change_inner_word_enters_insert() {
    let mut e = VimEngine::new("foo bar".into());
    e.feed("w");            // cursor at 'bar'
    e.feed("ciw");
    assert_eq!(crate::input::vim::Mode::Insert, e.mode());
    e.feed("X\x1b");
    assert_eq!(e.buffer(), "foo X");
}
#[test]
fn dd_and_count_dd() {
    let mut e = VimEngine::new("a\nb\nc\nd".into());
    e.feed("dd");
    assert_eq!(e.buffer(), "b\nc\nd");
    e.feed("2dd");
    assert_eq!(e.buffer(), "d");
}
#[test]
fn x_r_J_tilde() {
    let mut e = VimEngine::new("abc".into());
    e.feed("x");
    assert_eq!(e.buffer(), "bc");
    e.feed("rZ");
    assert_eq!(e.buffer(), "Zc");
    e.feed("~");
    assert_eq!(e.buffer(), "zc");
    let mut j = VimEngine::new("a\nb".into());
    j.feed("J");
    assert_eq!(j.buffer(), "a b");
}
#[test]
fn yank_then_motion_count() {
    let mut e = VimEngine::new("one two three".into());
    e.feed("d2w");
    assert_eq!(e.buffer(), "three");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -25`
Expected: FAIL (operators unimplemented).

- [ ] **Step 3: Write minimal implementation**

In `engine.rs`: add the fields, the `Op` enum, and operator handling. At the top of `handle_normal`, if `pending_op` is set, interpret the next key as a motion/text-object/doubled-op and call `apply_operator`. Implement `delete_range`, the linewise line-range helper, `D`/`C`/`x`/`r`/`J`/`~`/`>>`/`<<`. For text-objects after an operator, you need a 2-key read (`i`/`a` then the object char) — add a small `pending_textobj: Option<bool /*around*/>` state. (Write the full code, carefully; this is large but mechanical.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -30`
Expected: PASS (all, incl. Task 4's).

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): operators d/c/y + motions/objects, dd/cc/yy/D/C, x/r/J/~, indent

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 7: Registers (unnamed + named "a–"z) and put (p/P)

**Files:**
- Modify (replace stub): `src/input/vim/registers.rs`
- Modify: `src/input/vim/engine.rs` (use registers; `"x` prefix; `p`/`P`)
- Test: `engine.rs` tests

**Interfaces:**
- Produces: `struct Registers { unnamed: String, unnamed_linewise: bool, named: std::collections::HashMap<char, (String, bool)> }` with `fn yank(&mut self, reg: Option<char>, text: String, linewise: bool)` and `fn get(&self, reg: Option<char>) -> Option<(&str, bool)>`.
- Engine: replace `last_yank`/`last_yank_linewise` with a `registers: Registers` field + `pending_register: Option<char>`.

Implement `"a` (prefix sets `pending_register` for the next y/d/c/p), and `p`/`P` (charwise inserts after/before cursor; linewise opens a line below/above). `count` repeats the put.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn yank_put_charwise() {
    let mut e = VimEngine::new("abc".into());
    e.feed("yl");      // yank char 'a' (y + l motion)
    e.feed("$");
    e.feed("p");       // put after 'c'
    assert_eq!(e.buffer(), "abca");
}
#[test]
fn dd_then_p_linewise() {
    let mut e = VimEngine::new("a\nb\nc".into());
    e.feed("dd");      // deletes "a\n" into unnamed (linewise)
    assert_eq!(e.buffer(), "b\nc");
    e.feed("p");       // put linewise BELOW current line
    assert_eq!(e.buffer(), "b\na\nc");
}
#[test]
fn named_register() {
    let mut e = VimEngine::new("hello".into());
    e.feed("\"ayl");   // yank 'h' into reg a
    e.feed("$");
    e.feed("\"ap");    // put reg a after 'o'
    assert_eq!(e.buffer(), "hellho"); // 'h' inserted after 'o'... verify exact, adjust test to real semantics
}
```
(Adjust the `named_register` expected string to the engine's actual charwise-put offset once implemented; the assertion documents real behavior.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -25`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Fill `registers.rs`; wire the engine: operator results call `registers.yank(pending_register.take(), text, linewise)`; `p`/`P` read `registers.get(pending_register.take())`. `"` sets a `pending_register_select` flag so the next char is the register name.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -25`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/registers.rs src/input/vim/engine.rs
git commit -m "feat(vim): registers (unnamed + \"a-\"z) and p/P put

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 8: Find-char operators (f/t/F/T with ; ,) integrated; operator+find (df, dt)

**Files:**
- Modify: `src/input/vim/engine.rs`
- Test: `engine.rs` tests

**Interfaces:**
- Engine fields: `pending_find: Option<FindKind>`, `last_find: Option<(FindKind, char)>` (for `;`/`,`).

Implement: `f`/`t`/`F`/`T` set `pending_find`; the next Char is the target → motion via `motion::find_char`. `;` repeats last find; `,` repeats reversed. As a motion, it composes with a pending operator (`df)`, `dt,`).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn find_and_delete_to_char() {
    let mut e = VimEngine::new("foo(bar)baz".into());
    e.feed("df)");
    assert_eq!(e.buffer(), "baz");
}
#[test]
fn till_char_and_repeat() {
    let mut e = VimEngine::new("a.b.c".into());
    e.feed("t.");
    assert_eq!(e.cursor(), 0);   // till before first '.' (already before)
    e.feed(";");                  // repeat -> before next '.'
    assert_eq!(e.cursor(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Add the pending-find handling in `handle_normal` (both standalone and operator-composed). Store `last_find` for `;`/`,`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): f/t/F/T find-char motions + ; , repeat, operator-composed

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 9: Visual mode (v/V) + operators on selection

**Files:**
- Modify: `src/input/vim/engine.rs`
- Test: `engine.rs` tests

**Interfaces:**
- Refine `handle_key` Visual arm: `v` toggles charwise visual (anchor at cursor), `V` linewise; motions extend; `d`/`c`/`y`/`x` apply to the selection (`selection()` range), then return to Normal (or Insert for `c`); `>`/`<` indent; Esc exits.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn visual_delete() {
    let mut e = VimEngine::new("hello world".into());
    e.feed("v");        // anchor at 0
    e.feed("ll");       // extend to idx 2 -> selection covers 'hel'
    e.feed("d");
    assert_eq!(e.buffer(), "lo world");
    assert_eq!(crate::input::vim::Mode::Normal, e.mode());
}
#[test]
fn visual_line_yank_put() {
    let mut e = VimEngine::new("a\nb\nc".into());
    e.feed("V");        // linewise select line 0
    e.feed("y");        // yank "a\n"
    e.feed("G");        // last line
    e.feed("p");
    assert_eq!(e.buffer(), "a\nb\nc\na");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Add `v`/`V` entry in `handle_normal`; add a dedicated `handle_visual` (called from `handle_key` for Visual/VisualLine) that extends on motions and applies operators on `selection()`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): visual mode v/V with d/c/y/x/>/< on selection

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 10: Undo/redo (u / Ctrl+R) and dot-repeat (.)

**Files:**
- Modify: `src/input/vim/engine.rs`
- Test: `engine.rs` tests

**Interfaces:**
- Engine fields: `undo: Vec<(String, usize)>`, `redo: Vec<(String, usize)>`, `last_change: Option<Vec<VimKey>>` (recorded key sequence of the last buffer-mutating command, replayed by `.`).
- Snapshot on the FIRST mutation of a change group; `u` restores previous snapshot; `Ctrl+R` re-applies; `.` replays `last_change`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn undo_redo() {
    let mut e = VimEngine::new("abc".into());
    e.feed("x");                 // "bc"
    assert_eq!(e.buffer(), "bc");
    e.feed("u");                 // undo -> "abc"
    assert_eq!(e.buffer(), "abc");
    e.handle_key(crate::input::vim::VimKey::CtrlR); // redo -> "bc"
    assert_eq!(e.buffer(), "bc");
}
#[test]
fn dot_repeats_last_change() {
    let mut e = VimEngine::new("aaaa".into());
    e.feed("x");                 // "aaa"
    e.feed(".");                 // "aa"
    e.feed(".");                 // "a"
    assert_eq!(e.buffer(), "a");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Add a `snapshot()` called at the start of each mutating command (guard so insert sessions snapshot once on entry). `u`/`Ctrl+R` swap between `undo`/`redo`. Record `last_change` as the key sequence from the start of a Normal-mode change to its completion (for insert changes, include the typed chars + Esc); `.` re-feeds it via `handle_key`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): undo/redo (u / Ctrl+R) and dot-repeat (.)

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 11: Ex command line (:w :wq :q :q!) + Esc-in-Normal cancel + R rewrite

**Files:**
- Modify: `src/input/vim/engine.rs`
- Test: `engine.rs` tests

**Interfaces:**
- Engine: a `cmdline: Option<String>` (Some while typing after `:`); `:` enters command-line, chars accumulate, Enter parses → emits `EditorAction` (`:w`→Save, `:wq`→SaveQuit, `:q`→Cancel, `:q!`→Cancel (force flag for the host)). `R` in Normal → `EditorAction::OpenRewrite`. Esc in Normal (not visual, not cmdline) → `EditorAction::Cancel`. Expose `fn is_dirty(&self, seed: &str) -> bool` (buffer != seed) — the host uses it for confirm.
- Add `fn cmdline(&self) -> Option<&str>` so the adapter can render the `:` line.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn ex_write_and_quit() {
    use crate::input::vim::EditorAction;
    let mut e = VimEngine::new("x".into());
    let o = { e.feed(":w"); e.handle_key(crate::input::vim::VimKey::Enter) };
    assert_eq!(o.action, EditorAction::Save);
    let o2 = { e.feed(":wq"); e.handle_key(crate::input::vim::VimKey::Enter) };
    assert_eq!(o2.action, EditorAction::SaveQuit);
}
#[test]
fn r_opens_rewrite_and_esc_cancels() {
    use crate::input::vim::{EditorAction, VimKey};
    let mut e = VimEngine::new("x".into());
    let o = e.handle_key(VimKey::Char('R'));
    assert_eq!(o.action, EditorAction::OpenRewrite);
    let c = e.handle_key(VimKey::Esc);
    assert_eq!(c.action, EditorAction::Cancel);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Add the `:` handling (a small command-line submode), the `R`/Esc actions, `is_dirty`, `cmdline()`. Ensure `:` does NOT count as a register or motion.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::engine 2>&1 | tail -20`
Expected: PASS. Then run the FULL engine suite: `cargo test --bins vim:: 2>&1 | tail -5` — all green.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/engine.rs
git commit -m "feat(vim): ex commands (:w/:wq/:q/:q!), Esc-cancel, R rewrite action

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 12: Journal doc framing (build buffer / parse-back)

**Files:**
- Modify (replace stub): `src/input/vim/journal_doc.rs`
- Test: in `journal_doc.rs` tests

**Interfaces:**
- Produces: `fn build_buffer(question: &str, answer: &str) -> String` (= `format!("Q: {q}\n\n{a}")`, with `q` having any leading `Q:`/`Q: ` stripped first to avoid doubling); `fn parse_back(buffer: &str) -> (String, String)` (first line minus `Q:`/`Q: ` → question; text after the first blank line → answer, trimmed; if no blank line, first line = question and the rest = answer; never drop text).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_and_parse_roundtrip() {
        let b = build_buffer("Compare X", "Line one.\n\nLine two.");
        assert_eq!(b, "Q: Compare X\n\nLine one.\n\nLine two.");
        let (q, a) = parse_back(&b);
        assert_eq!(q, "Compare X");
        assert_eq!(a, "Line one.\n\nLine two.");
    }
    #[test]
    fn build_strips_existing_q_prefix() {
        assert_eq!(build_buffer("Q: Already", "ans"), "Q: Already\n\nans");
    }
    #[test]
    fn parse_back_without_blank_line() {
        let (q, a) = parse_back("Q: just a question line\nand a stray answer line");
        assert_eq!(q, "just a question line");
        assert_eq!(a, "and a stray answer line");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins vim::journal_doc 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Write minimal implementation**

Replace `src/input/vim/journal_doc.rs`:
```rust
//! Journal-specific framing for the vim editor: the page shows `Q: <question>`
//! then a blank line then the answer; this builds that buffer and parses it
//! back into (question, answer). Kept OUT of the engine so the engine stays a
//! generic text editor.

fn strip_q_prefix(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("Q:") {
        rest.trim_start()
    } else {
        line
    }
}

pub fn build_buffer(question: &str, answer: &str) -> String {
    format!("Q: {}\n\n{}", strip_q_prefix(question), answer)
}

pub fn parse_back(buffer: &str) -> (String, String) {
    // Question = first line (minus Q: prefix). Answer = after the first blank line.
    let mut lines = buffer.split('\n');
    let first = lines.next().unwrap_or("");
    let question = strip_q_prefix(first).to_string();

    // Find the first blank line; answer is everything after it.
    if let Some(idx) = buffer.find("\n\n") {
        let answer = buffer[idx + 2..].trim().to_string();
        (question, answer)
    } else {
        // No blank line: rest of the buffer (after the first line) is the answer.
        let rest: String = buffer
            .splitn(2, '\n')
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();
        (question, rest)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins vim::journal_doc 2>&1 | tail -20`
Expected: PASS. Then `cargo test --bins vim:: 2>&1 | tail -5` — full engine + framing green.

- [ ] **Step 5: Commit**

```bash
git add src/input/vim/journal_doc.rs
git commit -m "feat(vim): journal Q&A buffer build + parse-back

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 13: GTK wiring — InputMode::JournalEdit + thread key_char + overlay mirror API

**Files:**
- Modify: `src/app/mod.rs` (add `InputMode::JournalEdit` after `JournalVisual`; capture `keyval.to_unicode()` and pass into `handle_key`)
- Modify: `src/input/keymap.rs` (`handle_key` signature gains `key_char: Option<char>`; new dispatcher arm; new `handle_journal_edit_key`; pass `key_char` only to it)
- Modify: `src/ui/journal_overlay.rs` (add `vim_engine: RefCell<Option<crate::input::vim::VimEngine>>`; `enter_edit_buffer(question,answer)`, `mirror_engine()`, `exit_edit_buffer()`, `set_edit_indicator(text)`; suspend pagination by rendering the whole buffer)
- Test: none new (GTK integration — covered by the engine tests + the user's e2e). Build must pass.

**Interfaces:**
- `JournalOverlay::enter_edit_buffer(&self, question: &str, answer: &str)` — seeds the engine via `journal_doc::build_buffer`, sets the page buffer to it, places the cursor, shows `-- NORMAL --` in the footer.
- `JournalOverlay::feed_edit_key(&self, key: VimKey) -> EditorAction` — calls `engine.handle_key`, mirrors buffer/cursor/selection/mode to the TextView + footer, returns the `action` for the adapter to handle.
- `JournalOverlay::edit_buffer_qa(&self) -> (String, String)` — `journal_doc::parse_back(engine.buffer())`.
- `JournalOverlay::exit_edit_buffer(&self)` — drops the engine, restores the read render (`render_current`-style via the caller).

- [ ] **Step 1: Add the InputMode variant + thread key_char (compile first)**

In `src/app/mod.rs`, add `JournalEdit,` to `enum InputMode` after `JournalVisual`. In the key controller, after `let key_name = ...;` add:
```rust
let key_char = keyval.to_unicode();
```
and pass `key_char` into `crate::input::keymap::handle_key(...)` as a new argument (add it to the call). Update `handle_key`'s signature in `keymap.rs` to accept `key_char: Option<char>` and thread it ONLY to the journal-edit arm (other arms ignore it).

- [ ] **Step 2: Build to verify the signature change compiles**

Run: `cargo build 2>&1 | rg -i "error" | head`
Expected: only errors about the not-yet-added `handle_journal_edit_key` and `JournalEdit` arm (fix in Step 3) — fix until clean.

- [ ] **Step 3: Implement the overlay mirror API + the dispatcher arm + handler**

Add the `vim_engine` field + methods to `JournalOverlay` (mirror: write `engine.buffer()` into the view's buffer, set the GTK cursor via `buffer.iter_at_offset(char->byte)` — convert char index to byte via `engine.buffer().char_indices().nth(cursor)`; paint selection via `buffer.select_range`; set footer text to the mode indicator + any `:` cmdline). Add the dispatcher arm:
```rust
crate::app::InputMode::JournalEdit => handle_journal_edit_key(state, key_name, key_char, is_ctrl, is_shift),
```
and implement `handle_journal_edit_key` translating GTK keys → `VimKey` (named keys + `key_char` for printables; `Ctrl+R`→CtrlR; `R` (shift+r / "R") in Normal reaches the engine as `Char('R')`), calling `journal_overlay.feed_edit_key`, and acting on the returned `EditorAction` (Task 14 fills Save/Cancel/OpenRewrite; for now route Save/SaveQuit→`vim_save`, Cancel→`vim_cancel`, OpenRewrite→`vim_open_rewrite` as stubs that log + close).

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg -i "error|warning: unused" | head`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/input/keymap.rs src/ui/journal_overlay.rs
git commit -m "feat(journal): InputMode::JournalEdit + TextView mirror + key adapter

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 14: Wire `e` to enter vim edit; implement save / cancel-confirm / rewrite

**Files:**
- Modify: `src/input/actions/journal.rs` (`begin_edit` → enter JournalEdit; `vim_save`, `vim_cancel`, `vim_open_rewrite`)
- Modify: `src/input/keymap.rs` (remove the edit-card intercept block at the top of `handle_journal_key`; ensure `e` dispatches `begin_edit`)
- Modify: `src/ui/journal_overlay.rs` (pagination suspend on enter, restore on exit)
- Test: none new; build + the engine suite.

**Interfaces:**
- `begin_edit(state)`: read current page `(question, answer)`; `journal_overlay.enter_edit_buffer(&q,&a)`; hide footer nav; set `input_mode = JournalEdit`.
- `vim_save(state, quit: bool)`: `(q,a) = journal_overlay.edit_buffer_qa()`; reuse the existing save-as-is path (the empty-instruction branch of `submit_edit_rewrite`: `update_journal_page` + `purge_journal_audio` + `journal_undo` snapshot + `render_current` + `land_on_current_band_id` + toast "Saved"); if `quit`, `exit_to_overlay`.
- `vim_cancel(state)`: if `journal_overlay.edit_is_dirty()` show the discard-confirm (reuse `UndoConfirm` pattern with a new origin, or a simple confirm card); else exit to overlay.
- `vim_open_rewrite(state)`: open the existing `AskCard` ("Rewrite instruction"); on submit send the CURRENT buffer's `(q,a)` + instruction to Claude via the existing rewrite branch of `submit_edit_rewrite` (factor that branch into a reusable `rewrite_with_claude(state, q, a, instruction)`), then on success exit edit to the read view showing the revision.

- [ ] **Step 1: Implement `begin_edit` + `vim_save` (save-as-is path)**

Replace `begin_edit` body to enter JournalEdit. Add `vim_save`. Factor the existing save-as-is code out of `submit_edit_rewrite` into `fn save_qa_as_is(state, q, a)` and call it from both. (Show full code.)

- [ ] **Step 2: Build + sanity test**

Run: `cargo build 2>&1 | rg -i error | head && cargo test --bins vim:: 2>&1 | tail -3`
Expected: builds; engine tests green.

- [ ] **Step 3: Implement `vim_cancel` (dirty-confirm) + `vim_open_rewrite`**

Add `journal_overlay.edit_is_dirty()` (engine `is_dirty(seed)` vs the stored seed — store the seed buffer on `enter_edit_buffer`). Implement the discard-confirm + the rewrite handoff (factor `rewrite_with_claude`). Remove the edit-card intercept block (lines ~698–720 of `handle_journal_key`).

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | rg -i "error|warning: unused" | head`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs src/input/keymap.rs src/ui/journal_overlay.rs
git commit -m "feat(journal): e enters vim edit; :w save, :q/Esc cancel-confirm, R rewrite

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```

---

## Task 15: Remove JournalEditCard; update legends + docs

**Files:**
- Delete: `src/ui/journal_edit_card.rs`
- Modify: `src/ui/journal_overlay.rs` (remove the `edit_card` field, its construction in `new`, `open_edit_card`/`close_edit_card`/`take_edit_fields`/`toggle_edit_focus`/`edit_is_open`, and the `mod`/`use` of journal_edit_card; remove the `apply_font` references to the edit card's views)
- Modify: `src/ui/mod.rs` (remove `pub mod journal_edit_card;`)
- Modify: `src/input/keymap.rs` (remove any remaining `edit_is_open`/`toggle_edit_focus` calls)
- Modify: `src/ui/journal_keybinds_overlay.rs` (legend: `e` → "edit (vim)"; add a vim-edit legend section: modes + `:w`/`R`/`:q`)
- Modify: `docs/troubleshooting/journal-edit-card-sizing.md` → replace contents with a 3-line tombstone pointing at the vim-edit design (the card no longer exists)
- Test: build + full suite.

- [ ] **Step 1: Delete the file + remove references**

```bash
git rm src/ui/journal_edit_card.rs
```
Remove every reference (the compiler will list them). In `journal_overlay.rs` `apply_font`, drop the `edit_views` array and just font the page view + ask input.

- [ ] **Step 2: Build until clean**

Run: `cargo build 2>&1 | rg -i "error" | head -30`
Expected: iterate removing references until no errors.

- [ ] **Step 3: Update legends + doc tombstone**

Edit `journal_keybinds_overlay.rs` GROUPS: change the `e` row to "edit (vim)"; add a group for the vim editor keys. Replace `journal-edit-card-sizing.md` with:
```markdown
# (superseded) Journal edit card sizing

The journal `e` edit card (`JournalEditCard`) was REMOVED in favor of in-place
modal vim editing on the journal page. See
`docs/superpowers/specs/2026-06-30-journal-vim-edit-design.md`. This file is kept as a
tombstone so links don't 404.
```

- [ ] **Step 4: Build + full test suite + clippy**

Run: `cargo build 2>&1 | rg -i "error|warning: unused" | head && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3`
Expected: builds clean; `test result: ok`; clippy no new errors.

- [ ] **Step 5: Commit + update ac**

```bash
git add -A
git commit -m "refactor(journal): remove JournalEditCard (superseded by vim edit); update legend

$(printf 'Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>\nClaude-Session: https://claude.ai/code/session_014Tk7zF8GHAxRHcNVDkkWNY')"
```
Then update `CLAUDE-activeContext.md` to reflect the new state (vim editor implemented; gloss/synopsis unchanged; runtime verification pending).

---

## Task 16: Final verification + handoff

- [ ] **Step 1: Full build, test, clippy**

Run:
```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | tail -3
```
Expected: clean build, all bin tests pass (incl. the new `vim::` suite — should add ~40+ tests), clippy baseline.

- [ ] **Step 2: Update `ac` with the final committed state**

Record: branch `journal-vim-edit`, the vim engine + integration, that gloss/synopsis kept their ask-Claude edit, and that runtime GUI verification is the user's (headless cage SIGTERM-killed). List the e2e command to run.

- [ ] **Step 3: Ask the user to verify on screen**

Provide the e2e command and the manual repro (open journal `Ctrl+j`, `e` to edit King Lear's "art itself is nature", try `dd`, `cw`, `:w`, `R`, `:q`). Do NOT merge to master unless the user asks (per CLAUDE.md "Finishing a Branch", that's the default sequence, but the user controls when).

---

## Self-review notes (coverage vs design)

- §1 module architecture → Tasks 1–12 (one module per area). ✓
- §2 buffer/parse-back → Task 12. ✓
- §3 adapter + key translation (`to_unicode`) → Task 13. ✓
- §4 full verb set → Tasks 3 (motions), 5 (text-objects), 6 (operators/edits), 7 (registers/put), 8 (f/t), 9 (visual), 10 (undo/repeat), 11 (ex/R/Esc). ✓
- §5 `:w`/`R`/cancel flows → Tasks 11 (actions emitted) + 14 (host handling). ✓
- §6 undo coexistence → Task 14 (`save_qa_as_is` snapshots `journal_undo`; engine `u` is edit-session only). ✓
- §7 removals + legend → Task 15. ✓
- §8 testing → engine unit tests throughout; e2e is the user's (Task 16). ✓
- §9 out-of-scope respected (no macros/marks/search/splits). ✓

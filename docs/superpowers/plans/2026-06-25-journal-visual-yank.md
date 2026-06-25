# Journal Q&A visual-select-and-yank — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add vim-style visual block selection + clipboard yank to the journal
Q&A overlay (`Shift+V` enters, `j`/`k`/`gg`/`G` extend, `y` yanks, `Esc`/`V`
cancel), mirroring the gloss/synopsis overlay's `Shift+V` mode.

**Architecture:** A pure `journal_blocks` helper splits a journal page buffer
into blank-line-separated paragraph blocks with buffer-line spans. `JournalOverlay`
gains parallel visual-selection state + methods (matching `GlossOverlay`'s names)
and a left selection-bar Cairo overlay. A new `InputMode::JournalVisual` is
dispatched to a parallel `handle_journal_visual_key`. Normal `j`/`k` stays scroll.

**Tech Stack:** Rust, GTK4 (`gtk4::TextView`, `DrawingArea`, `Overlay`), `wl-copy`.

**Spec:** `docs/superpowers/specs/2026-06-25-journal-visual-yank-design.md`

## Global Constraints

- **Normal journal `j`/`k`/`gg`/`G` MUST stay scroll** — the block cursor exists
  ONLY inside `JournalVisual` mode. Do not change `handle_journal_key`'s existing
  `j`/`k`/`g`/`G` arms.
- **Parallel, not abstracted.** Do NOT introduce a trait over `GlossOverlay` +
  `JournalOverlay`. `handle_journal_visual_key` is a near-verbatim copy of
  `handle_block_visual_key` calling `JournalOverlay` methods. (YAGNI.)
- **No `keymap.json` / `keymap_config.rs` / `keybinds_overlay.rs` change** —
  overlay-internal keys (like the gloss `V`) are hardcoded in their per-overlay
  handler, never in the reader-binding table or the Ctrl+/ overlay.
- Selection-bar color is the FIXED gloss default `(0.53, 0.62, 0.71)` — a plain
  field, NOT theme-wired. No line numbers in the journal bar.
- The bar must clear on every `show_page` / `show_passage_page` so a stale
  selection never paints across page changes.
- `cargo build` + `cargo clippy` clean (no NEW warnings); `cargo test --bins`
  green (current baseline: 430 passed).
- Bash/CLI rules (CLAUDE.md): `rg`/`fd` not `grep`/`find`; `\cp -f`/`\mv -f`/
  `command rm -f` for non-interactive overwrite/delete. US Central timestamps.
- Commit trailer on EVERY commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
  ```

---

### Task 1: Pure `journal_blocks` helper + unit tests

**Files:**
- Create: `src/ui/journal_block.rs`
- Modify: `src/ui/mod.rs` (register `pub mod journal_block;`)

**Interfaces:**
- Produces:
  - `pub struct JournalBlock { pub start_line: i32, pub end_line: i32, pub text: String }`
  - `pub fn journal_blocks(lines: &[&str]) -> Vec<JournalBlock>`

- [ ] **Step 1: Write the failing tests**

Create `src/ui/journal_block.rs` with ONLY the tests first (the fns referenced
won't exist yet, so it won't compile — that is the intended red state):

```rust
//! Pure paragraph-block splitter for the journal Q&A overlay. A journal page
//! buffer is plain text (`question\n\nanswer`, or verse + `———` + Q&A); blocks
//! are maximal runs of non-blank lines, separated by one-or-more blank lines.

#[derive(Debug, Clone, PartialEq)]
pub struct JournalBlock {
    /// First buffer line (0-based) of the paragraph.
    pub start_line: i32,
    /// Last buffer line (0-based) of the paragraph.
    pub end_line: i32,
    /// The paragraph's lines, joined by '\n'.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<JournalBlock> {
        let lines: Vec<&str> = s.split('\n').collect();
        journal_blocks(&lines)
    }

    #[test]
    fn plain_qa_two_blocks() {
        // "Q\n\nA" -> line 0 = Q, line 1 = blank, line 2 = A.
        let b = split("Q\n\nA");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 0, text: "Q".into() });
        assert_eq!(b[1], JournalBlock { start_line: 2, end_line: 2, text: "A".into() });
    }

    #[test]
    fn multiline_paragraph_stays_one_block() {
        // A paragraph with a hard newline but no blank line is ONE block.
        let b = split("line one\nline two\n\nanswer");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 1, text: "line one\nline two".into() });
        assert_eq!(b[1], JournalBlock { start_line: 3, end_line: 3, text: "answer".into() });
    }

    #[test]
    fn passage_page_blocks() {
        // verse(2 lines) blank sep blank Q blank A
        // lines: 0 v1, 1 v2, 2 blank, 3 ———, 4 blank, 5 Q, 6 blank, 7 A
        let b = split("v1\nv2\n\n———\n\nQ\n\nA");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], JournalBlock { start_line: 0, end_line: 1, text: "v1\nv2".into() });
        assert_eq!(b[1], JournalBlock { start_line: 3, end_line: 3, text: "———".into() });
        assert_eq!(b[2], JournalBlock { start_line: 5, end_line: 5, text: "Q".into() });
        assert_eq!(b[3], JournalBlock { start_line: 7, end_line: 7, text: "A".into() });
    }

    #[test]
    fn consecutive_and_edge_blanks_collapse() {
        // Leading blank, double blank between, trailing blank -> 2 blocks, no empties.
        let b = split("\nQ\n\n\nA\n");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], JournalBlock { start_line: 1, end_line: 1, text: "Q".into() });
        assert_eq!(b[1], JournalBlock { start_line: 4, end_line: 4, text: "A".into() });
    }

    #[test]
    fn empty_and_all_blank_yield_no_blocks() {
        assert_eq!(journal_blocks(&[]), Vec::new());
        assert_eq!(split("\n\n\n"), Vec::new());
        // split("") yields one empty line -> blank -> no blocks
        assert_eq!(split(""), Vec::new());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (won't compile — `journal_blocks` undefined)**

Run: `cargo test --bins journal_block 2>&1 | tail -20`
Expected: compile error `cannot find function journal_blocks` (red state).

- [ ] **Step 3: Implement `journal_blocks`**

Add ABOVE the `#[cfg(test)]` module:

```rust
/// Split `lines` (a buffer's text split on '\n') into paragraph blocks. A block
/// is a maximal run of lines that are not entirely whitespace; runs of blank
/// lines separate blocks and produce no block of their own. `start_line` /
/// `end_line` are 0-based buffer line indices. Empty / all-blank input yields an
/// empty vec.
pub fn journal_blocks(lines: &[&str]) -> Vec<JournalBlock> {
    let mut blocks = Vec::new();
    let mut run_start: Option<i32> = None;
    for (i, line) in lines.iter().enumerate() {
        let blank = line.trim().is_empty();
        if blank {
            if let Some(start) = run_start.take() {
                let end = i as i32 - 1;
                blocks.push(make_block(lines, start, end));
            }
        } else if run_start.is_none() {
            run_start = Some(i as i32);
        }
    }
    if let Some(start) = run_start {
        let end = lines.len() as i32 - 1;
        blocks.push(make_block(lines, start, end));
    }
    blocks
}

fn make_block(lines: &[&str], start: i32, end: i32) -> JournalBlock {
    let text = lines[start as usize..=end as usize].join("\n");
    JournalBlock { start_line: start, end_line: end, text }
}
```

- [ ] **Step 4: Register the module**

In `src/ui/mod.rs`, add `pub mod journal_block;` alongside the other
`pub mod <name>;` lines (near `journal_overlay` / `gloss_block`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins journal_block 2>&1 | tail -20`
Expected: the 5 `journal_block` tests pass; overall `test result: ok`.

- [ ] **Step 6: Build + clippy**

Run: `cargo build 2>&1 | rg -i "^error|warning: unused|never used.*journal_block" | tail`
Expected: no errors; `journal_blocks` is used by tests so no dead-code on it.
(Other pre-existing dead-code warnings in unrelated modules are fine — do NOT
"fix" them.)
Run: `cargo clippy 2>&1 | rg "journal_block" | tail`
Expected: no clippy findings referencing journal_block.

- [ ] **Step 7: Commit**

```bash
git add src/ui/journal_block.rs src/ui/mod.rs
git commit -m "feat(journal): pure journal_blocks paragraph splitter + tests

journal_blocks(&[&str]) -> Vec<JournalBlock> splits a journal page buffer on
blank lines into paragraph blocks with 0-based buffer-line spans. Pure +
unit-tested (plain Q&A, multiline paragraph, passage page, collapsed/edge
blanks, empty). Backs the journal overlay's coming Shift+V visual selection.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: `JournalOverlay` visual-selection machinery + selection bar

**Files:**
- Modify: `src/ui/journal_overlay.rs`

**Interfaces:**
- Consumes: `crate::ui::journal_block::{JournalBlock, journal_blocks}` (Task 1),
  `crate::ui::gloss_block::visual_block_range` (existing,
  `pub fn visual_block_range(anchor: usize, cursor: usize) -> (usize, usize)`).
- Produces (on `JournalOverlay`):
  - `pub fn enter_visual(&self) -> bool`
  - `pub fn visual_step(&self, delta: i32)`
  - `pub fn visual_to_end(&self, last: bool)`
  - `pub fn visual_selection_text(&self) -> String`
  - `pub fn visual_selection_len(&self) -> usize`
  - `pub fn exit_visual(&self)`
  - `pub fn exit_visual_to_anchor(&self)`
  - `pub fn set_journal_hint(&self)`
  - `pub fn set_journal_visual_hint(&self)`

This task assumes the journal overlay has the structure as on the branch base
(master). It currently stores `footer_left: Label` but NOT the hint label, and
has NO bar overlay. Read the file first; if the live structure differs from the
snippets below (e.g. an already-present `position_label` or a clip helper),
KEEP the live code and adapt — only ADD the visual machinery.

- [ ] **Step 1: Read the current `JournalOverlay` struct + `new()` + show paths**

Run: `rg -n "struct JournalOverlay|fn new|fn show_page|fn show_passage_page|fn size_card|footer_left|scroll_overlay|connect_value_changed" src/ui/journal_overlay.rs`
Read those regions so the additions below splice into the real code.

- [ ] **Step 2: Add `use` + struct fields**

Add to the top `use` block:
```rust
use crate::ui::journal_block::{journal_blocks, JournalBlock};
use crate::ui::gloss_block::visual_block_range;
use std::rc::Rc;
```
Add these fields to `struct JournalOverlay`:
```rust
    hint: Label,
    bar_drawing: gtk4::DrawingArea,
    bar_ranges: Rc<RefCell<Vec<(i32, i32)>>>,
    blocks: RefCell<Vec<JournalBlock>>,
    visual_anchor: Cell<Option<usize>>,
    cursor_block: Cell<usize>,
```

- [ ] **Step 3: Build the selection bar in `new()` and wire it into the overlay**

The journal overlay wraps its `ScrolledWindow(TextView)` in a `scroll_overlay`
(`Overlay`) that already holds the `bottom_clip`. Add a `bar_drawing`
`DrawingArea` overlay to that SAME `scroll_overlay`, with a draw func that
strokes a 2px vertical line over each selected buffer-line span, and repaint on
scroll. Insert AFTER the `scroll_overlay` + `bottom_clip` are created but using
whatever the live variable names are (read Step 1). Pattern (adapt names):

```rust
let bar_ranges: Rc<RefCell<Vec<(i32, i32)>>> = Rc::new(RefCell::new(Vec::new()));
let bar_drawing = gtk4::DrawingArea::new();
bar_drawing.set_can_target(false);
{
    let ranges_clone = bar_ranges.clone();
    let view_clone = view.clone();
    bar_drawing.set_draw_func(move |_area, cr, _w, _h| {
        let ranges = ranges_clone.borrow();
        if ranges.is_empty() {
            return;
        }
        // Fixed gloss accent default (NOT theme-wired).
        cr.set_source_rgb(0.53, 0.62, 0.71);
        cr.set_line_width(2.0);
        let buffer = view_clone.buffer();
        let x = 4.0; // left inset; the card side margin already pads the text
        for (start_line, end_line) in ranges.iter() {
            if let (Some(si), Some(ei)) =
                (buffer.iter_at_line(*start_line), buffer.iter_at_line(*end_line))
            {
                let start_loc = view_clone.iter_location(&si);
                let (y_end, h_end) = view_clone.line_yrange(&ei);
                let (_, by_start) = view_clone.buffer_to_window_coords(
                    gtk4::TextWindowType::Widget, 0, start_loc.y());
                let (_, by_end) = view_clone.buffer_to_window_coords(
                    gtk4::TextWindowType::Widget, 0, y_end + h_end);
                cr.move_to(x, by_start as f64);
                cr.line_to(x, by_end as f64);
                let _ = cr.stroke();
            }
        }
    });
}
// Repaint the bar when the view scrolls (buffer->window y is scroll-dependent).
{
    let bar_for_scroll = bar_drawing.clone();
    scrolled.vadjustment().connect_value_changed(move |_| {
        bar_for_scroll.queue_draw();
    });
}
scroll_overlay.add_overlay(&bar_drawing);
scroll_overlay.set_measure_overlay(&bar_drawing, false);
scroll_overlay.set_clip_overlay(&bar_drawing, true);
```

If the live `new()` does NOT already have a `scroll_overlay` wrapping the
scrolled window (some versions wrap differently), add the `bar_drawing` to
whichever `Overlay` already hosts `bottom_clip` — the bar and clip must share the
overlay that sits over the scrolled view. Do not restructure the existing tree.

- [ ] **Step 4: Capture the hint label + set the normal hint with the new suffix**

Where `new()` builds the footer (`build_footer_row(...)`), the returned
`FooterRow` exposes `.left` AND `.hint`. Currently only `.left` is kept. Also
keep `.hint`:
```rust
let footer = crate::ui::footer::build_footer_row(
    text_margins as i32,
    "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select",
);
let footer_left = footer.left;
let hint = footer.hint;
```
(The ` · ⇧V select` suffix is added directly to the constructor string above, so
the normal hint already advertises visual mode at startup.)

- [ ] **Step 5: Initialize the new fields in the `Self { .. }` constructor**

```rust
    hint,
    bar_drawing,
    bar_ranges,
    blocks: RefCell::new(Vec::new()),
    visual_anchor: Cell::new(None),
    cursor_block: Cell::new(0),
```

- [ ] **Step 6: Rebuild blocks + clear the bar at the end of both show paths**

At the END of `show_page` (after the buffer is set + clip updated) AND at the
end of `show_passage_page`, add:
```rust
self.rebuild_blocks();
self.clear_bar();
```
These two helpers (Step 7) read the freshly-set buffer and reset any stale
selection so reopening a page never paints an old bar.

- [ ] **Step 7: Add the visual-selection methods (impl block)**

```rust
    /// Rebuild `self.blocks` from the current buffer text (paragraph runs).
    fn rebuild_blocks(&self) {
        let buffer = self.view.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        *self.blocks.borrow_mut() = journal_blocks(&lines);
        self.cursor_block.set(0);
        self.visual_anchor.set(None);
    }

    /// Clear the selection bar (no ranges) and repaint.
    fn clear_bar(&self) {
        self.bar_ranges.borrow_mut().clear();
        self.bar_drawing.queue_draw();
    }

    /// Redraw the bar over the current selection span (anchor..=cursor). No-op
    /// (clears) when no anchor is set or there are no blocks.
    fn refresh_bar(&self) {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => {
                drop(blocks);
                self.clear_bar();
                return;
            }
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        let span = (blocks[s].start_line, blocks[e].end_line);
        drop(blocks);
        *self.bar_ranges.borrow_mut() = vec![span];
        self.bar_drawing.queue_draw();
    }

    /// Index of the first block whose end_line is at or below the current
    /// viewport top — the anchor seed for Shift+V. Falls back to 0.
    fn topmost_visible_block(&self) -> usize {
        let top_y = self.scrolled.vadjustment().value();
        let buffer = self.view.buffer();
        let blocks = self.blocks.borrow();
        for (i, b) in blocks.iter().enumerate() {
            if let Some(iter) = buffer.iter_at_line(b.end_line) {
                let (y, h) = self.view.line_yrange(&iter);
                if (y + h) as f64 >= top_y {
                    return i;
                }
            }
        }
        0
    }

    /// Enter visual mode: anchor at the topmost visible block. Returns false
    /// (no-op) when there are no blocks.
    pub fn enter_visual(&self) -> bool {
        if self.blocks.borrow().is_empty() {
            return false;
        }
        let seed = self.topmost_visible_block();
        self.visual_anchor.set(Some(seed));
        self.cursor_block.set(seed);
        self.refresh_bar();
        true
    }

    /// Move the cursor end of the selection by `delta` blocks (clamped), redraw
    /// the bar, and scroll the cursor block into view.
    pub fn visual_step(&self, delta: i32) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor_block.get().min(len - 1) as i64;
        let next = (cur + delta as i64).clamp(0, len as i64 - 1) as usize;
        self.cursor_block.set(next);
        self.refresh_bar();
        self.scroll_cursor_into_view();
    }

    /// Move the cursor end to the first (`false`) or last (`true`) block.
    pub fn visual_to_end(&self, last: bool) {
        let len = self.blocks.borrow().len();
        if len == 0 {
            return;
        }
        self.cursor_block.set(if last { len - 1 } else { 0 });
        self.refresh_bar();
        self.scroll_cursor_into_view();
    }

    /// The selected paragraphs' text (anchor..=cursor), blank-line joined.
    pub fn visual_selection_text(&self) -> String {
        let blocks = self.blocks.borrow();
        let anchor = match self.visual_anchor.get() {
            Some(a) if !blocks.is_empty() => a.min(blocks.len() - 1),
            _ => return String::new(),
        };
        let cursor = self.cursor_block.get().min(blocks.len() - 1);
        let (s, e) = visual_block_range(anchor, cursor);
        blocks[s..=e]
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Number of blocks currently selected.
    pub fn visual_selection_len(&self) -> usize {
        match self.visual_anchor.get() {
            Some(a) => {
                let (s, e) = visual_block_range(a, self.cursor_block.get());
                e - s + 1
            }
            None => 0,
        }
    }

    /// Exit visual mode: clear the anchor and the bar. (The journal has no
    /// persistent normal-mode cursor, so yank and cancel both just clear.)
    pub fn exit_visual(&self) {
        self.visual_anchor.set(None);
        self.clear_bar();
    }

    /// Exit visual mode returning the cursor to the anchor block. Equivalent to
    /// `exit_visual` here (no persistent cursor), provided for handler symmetry.
    pub fn exit_visual_to_anchor(&self) {
        if let Some(anchor) = self.visual_anchor.get() {
            self.cursor_block.set(anchor);
        }
        self.visual_anchor.set(None);
        self.clear_bar();
    }

    /// Scroll the viewport so the current cursor block is visible. Uses the
    /// view's vadjustment and the cursor block's line range.
    fn scroll_cursor_into_view(&self) {
        let idx = self.cursor_block.get();
        let blocks = self.blocks.borrow();
        let Some(b) = blocks.get(idx) else { return };
        let buffer = self.view.buffer();
        let adj = self.scrolled.vadjustment();
        let page = adj.page_size();
        if let Some(si) = buffer.iter_at_line(b.start_line) {
            let (y_top, _) = self.view.line_yrange(&si);
            let y_top = y_top as f64;
            if y_top < adj.value() {
                adj.set_value(y_top);
            }
        }
        if let Some(ei) = buffer.iter_at_line(b.end_line) {
            let (y, h) = self.view.line_yrange(&ei);
            let y_bottom = (y + h) as f64;
            if y_bottom > adj.value() + page {
                adj.set_value((y_bottom - page).max(adj.lower()));
            }
        }
    }

    /// Normal-navigation footer hint (advertises Shift+V). Re-set on visual exit.
    pub fn set_journal_hint(&self) {
        self.hint.set_text(
            "Alt+w work \u{00b7} Ctrl+\\ pick \u{00b7} Alt+g gloss \u{00b7} Ctrl+g view gloss \u{00b7} \u{21e7}V select",
        );
    }

    /// Footer hint shown while journal visual mode is active.
    pub fn set_journal_visual_hint(&self) {
        self.hint
            .set_text("\u{21e7}V/Esc exit \u{00b7} j/k extend \u{00b7} gg/G ends \u{00b7} y yank");
    }
```

- [ ] **Step 8: Build**

Run: `cargo build 2>&1 | rg -i "^error|error\[" | tail`
Expected: no errors. If `Rc` / `Cell` / `RefCell` are already imported, do not
re-import (remove the duplicate `use`). If a borrow-panic risk exists (e.g.
borrowing `self.blocks` across a `self.clear_bar()` that also borrows it), the
methods above already `drop(blocks)` before re-borrowing — keep that pattern.

- [ ] **Step 9: Clippy**

Run: `cargo clippy 2>&1 | rg "journal_overlay" | tail`
Expected: no NEW clippy findings in journal_overlay.rs. The new pub methods are
called from Task 3, so within this task some may warn `never used` — that is
expected mid-feature and resolved by Task 3; do NOT add `#[allow(dead_code)]`.

- [ ] **Step 10: Tests**

Run: `cargo test --bins 2>&1 | rg "test result" | tail`
Expected: `test result: ok`, count >= 435 (430 baseline + 5 from Task 1). No
regressions.

- [ ] **Step 11: Commit**

```bash
git add src/ui/journal_overlay.rs
git commit -m "feat(journal): visual-selection state + selection bar on JournalOverlay

Add the block-visual machinery mirroring GlossOverlay: blocks (from
journal_blocks), visual_anchor/cursor_block, a left Cairo selection bar over the
scroll overlay (fixed accent, repainted on scroll), and the enter_visual /
visual_step / visual_to_end / visual_selection_text / visual_selection_len /
exit_visual(_to_anchor) / set_journal_hint(/visual) methods. Blocks rebuild and
the bar clears on every show_page/show_passage_page. Normal j/k unaffected.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: `InputMode::JournalVisual` + routing + `Shift+V` entry

**Files:**
- Modify: `src/app/mod.rs` (the `enum InputMode` definition, line ~87)
- Modify: `src/input/keymap.rs`

**Interfaces:**
- Consumes: the `JournalOverlay` methods from Task 2; `InputMode::JournalOverlay`
  (existing return mode).
- Produces: `InputMode::JournalVisual`; `fn handle_journal_visual_key(...)`.

- [ ] **Step 1: Find the InputMode enum and the dispatch site**

Run: `rg -n "enum InputMode|GlossVisual|JournalOverlay =>|SynopsisVisual =>" src --glob '*.rs'`
Note the file/line of `enum InputMode` and the `match ... input_mode` dispatch in
`keymap.rs` (~line 116-120).

- [ ] **Step 2: Add the `JournalVisual` variant**

In the `enum InputMode { ... }` definition, add `JournalVisual,` next to
`GlossVisual` / `SynopsisVisual`. Match the existing derive/style (these are
plain unit variants).

- [ ] **Step 3: Dispatch `JournalVisual` to the new handler**

In `keymap.rs`, next to the existing
`crate::app::InputMode::GlossVisual => handle_block_visual_key(...)` arm, add:
```rust
            crate::app::InputMode::JournalVisual => handle_journal_visual_key(state, key_state, key_name),
```

- [ ] **Step 4: Add `handle_journal_visual_key`**

Place it directly after `handle_block_visual_key` in `keymap.rs`. It is the
journal-typed parallel of that function:
```rust
/// Visual block selection in the journal Q&A overlay (entered with Shift+V).
/// gg/G jump the cursor end, j/k extend, y yanks the selected blocks to the
/// clipboard and exits, Esc/V cancel. All other keys are consumed. Parallel to
/// `handle_block_visual_key` but calls `JournalOverlay` (a different type, so it
/// cannot share `BlockVisualCfg`, which is fixed to `GlossOverlay`).
fn handle_journal_visual_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
) -> bool {
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().journal_overlay.visual_to_end(false);
        }
        return true;
    }
    match key_name {
        "j" => {
            state.borrow().journal_overlay.visual_step(1);
            true
        }
        "k" => {
            state.borrow().journal_overlay.visual_step(-1);
            true
        }
        "G" => {
            state.borrow().journal_overlay.visual_to_end(true);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "y" => {
            let (text, n) = {
                let s = state.borrow();
                (s.journal_overlay.visual_selection_text(), s.journal_overlay.visual_selection_len())
            };
            if !text.is_empty() {
                let _ = std::process::Command::new("wl-copy").arg(&text).spawn();
                crate::logging::log(&format!("JOURNAL: copied {} blocks", n));
            }
            {
                let mut s = state.borrow_mut();
                s.journal_overlay.exit_visual();
                s.input_mode = crate::app::InputMode::JournalOverlay;
                s.journal_overlay.set_journal_hint();
                crate::ui::toast::show_transient(&s.chapter_toast, "Copied", 2);
            }
            true
        }
        "Escape" | "V" => {
            let mut s = state.borrow_mut();
            s.journal_overlay.exit_visual_to_anchor();
            s.input_mode = crate::app::InputMode::JournalOverlay;
            s.journal_overlay.set_journal_hint();
            true
        }
        _ => true,
    }
}
```
VERIFY against `handle_block_visual_key`: the `chapter_toast` field name, the
`show_transient` signature, and `crate::logging::log` must match what that
function uses. If the live gloss handler uses a different toast helper/arg order,
COPY ITS EXACT FORM (the journal handler must call the same APIs).

- [ ] **Step 5: Add the `V` arm in `handle_journal_key`**

In `handle_journal_key`, among the plain-key arms (alongside `A`/`E`/`D`), add:
```rust
        "V" => {
            let entered = state.borrow().journal_overlay.enter_visual();
            if entered {
                let mut s = state.borrow_mut();
                s.input_mode = crate::app::InputMode::JournalVisual;
                s.journal_overlay.set_journal_visual_hint();
            }
            true
        }
```
Confirm the ask-card intercept and the `gg` chord both run BEFORE this match (so
`V` while the ask card is focused still types into the input) — they already do
in the current handler; do not reorder them.

- [ ] **Step 6: Build**

Run: `cargo build 2>&1 | rg -i "^error|error\[" | tail`
Expected: no errors. The Task-2 "never used" warnings on the visual methods are
now resolved (Task 3 calls them all).

- [ ] **Step 7: Clippy**

Run: `cargo clippy 2>&1 | rg -i "^error|warning" | rg -i "journal" | tail`
Expected: no NEW journal warnings.

- [ ] **Step 8: Tests**

Run: `cargo test --bins 2>&1 | rg "test result" | tail`
Expected: `test result: ok`, no regressions (>= 435).

- [ ] **Step 9: Confirm no out-of-scope files changed**

Run: `git status --short`
Expected ONLY: `src/app/mod.rs` + `src/input/keymap.rs`
changed in this task. Run:
`git diff --stat -- .config 2>/dev/null; rg -l "JournalVisual" src/input/keymap_config.rs ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json src/ui/keybinds_overlay.rs 2>/dev/null`
Expected: NO matches (no keymap.json / keymap_config / keybinds_overlay change).

- [ ] **Step 10: Commit**

```bash
git add src/app/mod.rs src/input/keymap.rs
git commit -m "feat(journal): Shift+V visual mode routing + y yank

Add InputMode::JournalVisual, dispatch it to a new handle_journal_visual_key
(parallel to handle_block_visual_key but on JournalOverlay), and add the
Shift+V entry arm in handle_journal_key. j/k extend, gg/G ends, y copies the
selected paragraphs via wl-copy (+ 'Copied' toast) and exits, Esc/V cancel.
Normal j/k stays scroll. No keymap.json/keybinds-overlay change (overlay-
internal key, like the gloss V).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

## Verification (after all tasks)

- `cargo build` + `cargo clippy` clean (no NEW warnings); `cargo test --bins`
  green (>= 435).
- Reviewer confirms: normal `j`/`k`/`gg`/`G` arms in `handle_journal_key`
  unchanged; no trait introduced; no `keymap.json`/`keymap_config.rs`/
  `keybinds_overlay.rs` change; the journal visual handler calls the SAME toast/
  log/wl-copy APIs as the gloss handler; the bar clears on every show.
- **User cage pass** (agent cannot drive cage on the live dwl session): open a
  journal Q&A (`Ctrl+j`), `Shift+V` (bar appears on first visible block), `j`/`k`
  extend, `y` (toast "Copied", exits, `wl-paste` shows the selected paragraphs),
  reopen + `Shift+V` + `Esc` (bar clears, no copy), and confirm normal `j`/`k`
  still scrolls outside visual mode.

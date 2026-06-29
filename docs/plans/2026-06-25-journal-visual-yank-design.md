# Journal Q&A visual-select-and-yank — design

## Goal

Give the journal Q&A overlay (`Ctrl+j`) the same vim-style visual block
selection + clipboard yank the gloss and synopsis overlays already have:

- **`Shift+V`** — enter visual mode, anchoring at the **topmost block currently
  in view** (journal has no persistent block cursor — normal `j`/`k` stays
  scroll, per the user's decision).
- **`j` / `k`** — extend the selection down / up one paragraph block.
- **`gg` / `G`** — extend to the first / last block.
- **`y`** — copy the selected blocks (blank-line-joined) to the clipboard via
  `wl-copy`, toast "Copied", and exit to the journal overlay.
- **`Escape` / `V`** — cancel, returning to the journal overlay (no copy).

This mirrors `GlossOverlay`'s `Shift+V` visual mode (`GLOSS_VISUAL_CFG`,
`SYNOPSIS_VISUAL_CFG`) — same keys, same selection bar, same yank-to-`wl-copy`.

## Why a parallel implementation, not the generic `handle_block_visual_key`

The existing generic `handle_block_visual_key` + `BlockVisualCfg`
(`src/input/keymap.rs`) is hardcoded to `&crate::ui::gloss_overlay::GlossOverlay`
in every fn-pointer slot (`yank_text`, `yank_exit`, `escape_exit`, `set_hint`).
`JournalOverlay` is a different type, so it cannot reuse that config without
introducing a trait over both overlays — a larger abstraction than this feature
warrants. We add a small **parallel** `handle_journal_visual_key` that calls the
same-named methods on `JournalOverlay`. The visual-mode METHODS on
`JournalOverlay` mirror the `GlossOverlay` ones 1:1 (same names, same behavior),
so the two handlers read identically; only the receiver type differs.

YAGNI: no trait, no shared config struct. If a third overlay ever needs this,
that is the moment to extract a trait — not now.

## Block model for a journal page

A journal page's buffer is **plain text**, not tag-structured like glosses
(`<p>`/`<speaker>`). The body is built in `JournalOverlay::show_page` as
`format!("{}\n\n{}", question, answer)` and in `show_passage_page` as
`verse + "\n\n———\n\n" + question + "\n\n" + answer`.

Blocks are therefore **blank-line-separated paragraphs of the rendered buffer**.
We add a pure helper:

```rust
/// Split a journal page buffer's text into blank-line-separated paragraph
/// blocks, each paired with its [start_line, end_line] buffer-line span.
/// `lines` is the buffer split on '\n'. A block is a maximal run of
/// non-blank lines; runs of blank lines are separators (not blocks). Returns
/// the blocks in document order. Empty input -> empty vec.
pub fn journal_blocks(lines: &[&str]) -> Vec<JournalBlock>;

pub struct JournalBlock {
    pub start_line: i32,  // first buffer line of the paragraph
    pub end_line: i32,    // last buffer line of the paragraph
    pub text: String,     // the paragraph's lines, '\n'-joined
}
```

This lives in a small new module `src/ui/journal_block.rs` (pure, unit-tested)
so the splitting logic is testable without GTK. For a plain Q&A page the blocks
are `[question-para, answer-para]` (each may be multi-line if the question or
answer itself contains hard newlines — those stay within one block since there
is no blank line between them). For a passage page the blocks are the verse
paragraph(s), the `———` separator line (its own one-line block — harmless to
select), then the question and answer paragraphs.

The separator `———` becomes a selectable block. That is acceptable (selecting it
yanks a line of dashes); not worth special-casing.

## Component changes

### New: `src/ui/journal_block.rs`

`JournalBlock` struct + `journal_blocks(&[&str]) -> Vec<JournalBlock>` +
`visual_block_range` is reused from `gloss_block` (already
direction-independent). Pure, unit-tested.

### `src/ui/journal_overlay.rs`

Add the visual-selection machinery, mirroring `GlossOverlay`:

- **Fields:**
  - `blocks: RefCell<Vec<JournalBlock>>` — rebuilt on each `show_page` /
    `show_passage_page` from the freshly-set buffer text.
  - `visual_anchor: Cell<Option<usize>>` — the anchor block index (None outside
    visual mode).
  - `cursor_block: Cell<usize>` — the moving end of the selection (only
    meaningful inside visual mode).
  - `bar_drawing: gtk4::DrawingArea` + `bar_ranges: Rc<RefCell<Vec<(i32, i32)>>>`
    — the left selection bar overlay, drawn over `scroll_overlay` exactly like
    the gloss bar (vertical Cairo line over the selected buffer-line span,
    repainted on `scrolled.vadjustment().connect_value_changed`). Use a fixed
    accent color — the same default the gloss bar uses, `(0.53, 0.62, 0.71)` —
    stored as a plain field, NOT theme-wired. No per-block coloring, no line
    numbers.

- **Methods (names match `GlossOverlay` so the handler reads the same):**
  - `rebuild_blocks(&self)` — read the current buffer text, call
    `journal_blocks`, store in `self.blocks`. Called at the end of `show_page`
    and `show_passage_page` (after `buffer.set_text` / verse insert).
  - `topmost_visible_block(&self) -> usize` — the index of the first block whose
    `end_line` is at or below the current viewport top (the anchor seed for
    `Shift+V`). Falls back to 0.
  - `enter_visual(&self) -> bool` — if `blocks` non-empty, set
    `visual_anchor = Some(topmost_visible_block())`, `cursor_block` = same,
    refresh the bar, return true; else false.
  - `visual_step(&self, delta: i32)` — clamp-move `cursor_block`, refresh bar,
    scroll cursor block into view.
  - `visual_to_end(&self, last: bool)` — move `cursor_block` to first/last,
    refresh bar, scroll into view.
  - `visual_selection_text(&self) -> String` — `journal_blocks` text of
    `anchor..=cursor` joined by `\n\n`; empty if no anchor.
  - `visual_selection_len(&self) -> usize` — block count in the selection.
  - `exit_visual_to_anchor(&self)` — clear anchor, leave cursor at anchor, clear
    the bar.
  - `exit_visual(&self)` — clear anchor + clear the bar (used by yank exit; the
    journal has no persistent cursor to collapse, so yank and Escape both just
    clear — `exit_visual_to_anchor` and `exit_visual` are equivalent here, but
    both are provided to keep the handler symmetric with the gloss).
  - `set_journal_hint(&self)` / `set_journal_visual_hint(&self)` — footer hint
    strings. Normal hint keeps the existing footer text and appends
    `· ⇧V select`; visual hint is `⇧V/Esc exit · j/k extend · gg/G ends · y yank`.

- The bar must clear (empty `bar_ranges` + `queue_draw`) on every `show_page` /
  `show_passage_page` so a stale selection from a previous page never paints.

### `src/app.rs` (InputMode)

Add `InputMode::JournalVisual` to the `InputMode` enum (next to `GlossVisual` /
`SynopsisVisual`).

### `src/input/keymap.rs`

- Dispatch `InputMode::JournalVisual => handle_journal_visual_key(state,
  key_state, key_name)`.
- Add `handle_journal_visual_key` — a near-verbatim copy of
  `handle_block_visual_key`'s body, but calling the `JournalOverlay` methods on
  `s.journal_overlay` and returning to `InputMode::JournalOverlay` with
  `set_journal_hint`. Keys: `gg` extend-to-first, `j`/`k` extend, `gg`/`G` ends,
  `y` yank+exit (wl-copy + toast "Copied" + log `JOURNAL: copied N blocks`),
  `Escape`/`V` cancel. All other keys consumed (`true`).
- In `handle_journal_key` (normal journal mode), add the `V` arm:

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

  Place it among the existing single-key arms (alongside `A`/`E`/`D`). It must
  NOT collide with the ask-card intercept or the gg chord (both already run
  before the match).

### Footer hint

The journal overlay's footer hint currently shows
`Alt+w work · Ctrl+\ pick · Alt+g gloss · Ctrl+g view gloss`
(built in `JournalOverlay::new` via `build_footer_row`). Append ` · ⇧V select`
to advertise the new mode. The visual-mode hint replaces the whole string while
active and restores the normal string on exit. The journal overlay currently
stores only `footer_left` (the band/position label) — it does NOT keep the hint
label. So **add a `hint: Label` field** (from `FooterRow.hint`, already
exposed) and a `set_journal_hint` / `set_journal_visual_hint` pair that set
`self.hint`. The normal hint string is the existing footer text plus
` · ⇧V select`; set it at construction and re-set it on visual-mode exit.

## Keybind overlay + keymap.json

`Shift+V` in the journal overlay is handled directly in `handle_journal_key`
(not via the `keymap.json` reader-binding table — overlay keys are hardcoded in
their per-overlay handlers, exactly like the gloss `V`). So **no `keymap.json`
or `keymap_config.rs` change**. The Ctrl+/ keybinds overlay
(`keybinds_overlay.rs`) documents reader-mode binds; overlay-internal binds
(gloss `V`, journal `A`/`E`/`D`) are not in that table today, so journal `V`
follows the same convention and the overlay is not updated. (If the user later
wants overlay-internal keys documented, that is a separate, cross-overlay task.)

## Behavior preservation

- Normal journal `j`/`k`/`gg`/`G` are UNCHANGED — they still scroll / jump the
  viewport. The block cursor exists only inside `JournalVisual` mode.
- The clip fix and footer-position changes already on this branch are unaffected.
- Entering visual mode with zero blocks (empty "No pages yet" card) is a no-op
  (`enter_visual` returns false), so `Shift+V` does nothing there — matching the
  gloss behavior on an empty card.

## Testing

- **Pure unit tests** (`cargo test --bins`) for `journal_blocks`:
  - plain `"Q\n\nA"` -> 2 blocks with correct line spans;
  - passage `"v1\nv2\n\n———\n\nQ\n\nA"` -> verse block (lines 0-1), separator
    block (line 3), Q (line 5), A (line 7);
  - leading/trailing blank lines and multiple consecutive blanks collapse to
    separators (no empty blocks);
  - empty / all-blank input -> empty vec;
  - `visual_block_range` reuse already covered by gloss tests (no new test).
- **GTK-realized parts (bar drawing, scroll-into-view, enter_visual seeding)**
  are not exercisable in `cargo test --bins`; verified by the user's cage pass
  (the agent cannot drive cage on the live dwl session).

## Out of scope

- A persistent normal-mode block cursor in the journal overlay (user chose: keep
  `j`/`k` as scroll).
- Extracting a shared `VisualSelectable` trait across gloss + journal (YAGNI
  until a third consumer).
- Documenting overlay-internal keys in the Ctrl+/ overlay.
- Line numbers in the journal bar (the gloss bar draws every-5th line numbers;
  the journal Q&A is short prose — no line numbers).

## User cage pass (acceptance)

Open a journal Q&A (`Ctrl+j`), press `Shift+V` (selection bar appears on the
first visible block), `j`/`k` to extend (bar grows/shrinks), `y` (toast
"Copied", mode exits, clipboard holds the selected paragraphs — verify with
`wl-paste`), reopen and `Shift+V` then `Esc` (bar clears, no copy). Confirm
normal `j`/`k` still scrolls when not in visual mode.

# Cursor highlight + nav binds in the two-column translation overlay

## Problem

The two-column translation overlay (opened with `i`,
`src/ui/translation_overlay.rs`) is currently read-only: `j`/`k` scroll the
viewport, and there is no cursor-line highlight. We want it to behave like the
main reading card — the dialogue-navigation binds move a real cursor, the
current line is highlighted, and MPV audio seeks/plays exactly as it does in the
card.

## Decisions (from brainstorming)

- **Drive the real cursor.** `,` `q` `j` `k` move `state.current_line` by calling
  the SAME navigation functions the main card uses, so MPV seek/playback is
  identical and the reader is already on that line when the overlay closes.
- **Nav binds:** `,` = prev dialogue, `q` = next dialogue, `j` = next dialogue
  (cursor), `k` = prev line — matching the main card.
- **Highlight style A:** highlight the cursor's ORIGINAL line (left) AND its
  paired TRANSLATION line (right), both columns.
- **Accept the vertical offset.** Columns stay free-flow (each wraps
  independently); the two highlighted lines may sit on different screen rows. No
  row-locking rework.
- **Auto-follow, minimal scroll.** Moving the cursor scrolls the overlay only
  when the highlighted ORIGINAL line would leave the viewport, landing it near
  the crossed edge (terminal-like) — not re-centering every move.

## Background facts (verified)

- **Nav binds** (`src/input/keymap_config.rs`): `comma`→`JumpToPrevDialogue`,
  `q`→`JumpToNextDialogue`, `j`→`CursorNextDialogue`, `k`→`CursorPrevLine`.
- **Nav functions** (`src/input/navigation.rs`): `jump_to_prev_dialogue` (849),
  `jump_to_next_dialogue` (865), `cursor_next_dialogue` (907), `cursor_prev_line`
  (882). Each ends in `after_page_change(state, reason)`, which calls
  `seek_to_current_line(state)` when `reason.should_seek()` — this is the MPV
  seek path, identical to the card.
- **`space` play/pause** is a global toggle handled BEFORE mode dispatch
  (`keymap.rs:62`), so it already works inside the overlay. No change needed.
- **Overlay handler:** `handle_translation_overlay_key(state, key_name)`
  (`keymap.rs:742`), routed from `InputMode::TranslationOverlay` (`keymap.rs:99`).
  Today: `i`/`Escape` close; `j`/`k` call `translation_overlay.scroll(±1)`.
- **Overlay block model** (`translation_overlay.rs`): on `show()`, each speaker
  block builds an `orig: TextView` + `trans: TextView` (each a multi-line buffer
  with one line per source line); interlude blocks (`speaker == None`) build a
  single `TextView`. `block_widgets: RefCell<Vec<(usize, usize, gtk4::Box)>>`
  records `(start_idx, end_idx, block_box)` — the absolute work-line range per
  block. Block lines are contiguous, so source-line W maps to buffer line
  `W - start_idx`.
- **Cursor highlight on the card** (`src/app.rs:984`): a `cursor-line` `TextTag`
  with `paragraph_background(theme.cursor_line_bg)`, applied per line in
  `update_highlight` (`src/input/highlight.rs:263`).
- **Cursor→work line:** `state.work_line_for_buffer(state.current_line)` →
  `Option<usize>` (`app.rs:442`).
- **Overlay→content geometry:** `scroll_to_block` already maps a block widget's
  origin into `content_vbox` via `widget.compute_point(&content_vbox, Point(0,0))`
  (the project-proven API, also used at `app.rs:5033`).

## Design

### 1. Per-block view handles in `block_widgets`

Change the recorded tuple so the overlay can tag the right buffers later. Replace
`block_widgets: Vec<(usize, usize, gtk4::Box)>` with a struct:

```rust
struct BlockEntry {
    start_idx: usize,
    end_idx: usize,
    block_box: gtk4::Box,
    /// Original (left) view, and translation (right) view. For an interlude
    /// block (speaker == None) `trans` is None and `orig` is the single view.
    orig: gtk4::TextView,
    trans: Option<gtk4::TextView>,
}
```

`show()` populates `orig`/`trans` when it builds each block (it already creates
those views — just store them). `scroll_to_block` updates to read
`entry.block_box`.

### 2. A `cursor-line` tag per overlay buffer

Each overlay `TextView` has its own `TextBuffer`, so the tag must live in each
buffer's tag table. On `show()`, for every `orig`/`trans`/interlude buffer, add a
`cursor-line` tag with `paragraph_background(cursor_line_bg)` (the color is
already passed/derivable; thread `cursor_line_bg: &str` into `show()` like
`text_fg`/`dim_fg`, sourced from `state.theme.cursor_line_bg`). Store nothing
extra — look the tag up by name (`buffer.tag_table().lookup("cursor-line")`) when
applying/clearing.

### 3. `highlight_work_line(work_idx)` — style A

New method on `TranslationOverlay`:

```rust
pub fn highlight_work_line(&self, work_idx: usize) { ... }
```

1. **Clear** the previous highlight: remove the `cursor-line` tag across every
   block's `orig` and `trans` buffers (clear all — a scene has a small number of
   blocks, so a full sweep is cheap and avoids tracking the last-highlighted
   entry).
2. **Find** the `BlockEntry` whose `[start_idx, end_idx]` contains `work_idx`.
   No match → return (cursor is outside this scene; leave nothing highlighted).
3. **Apply** the tag to line `off = work_idx - start_idx` in the `orig` buffer
   (iter from `iter_at_line(off)` to that line's end) AND, if `trans` is Some, to
   line `off` of the `trans` buffer. Interlude block → only `orig`.

A small pure helper supports the unit test:

```rust
/// Returns (block_index, line_offset) for the block containing `work_idx`.
fn locate_line(ranges: &[(usize, usize)], work_idx: usize) -> Option<(usize, usize)>
```

`highlight_work_line` calls `locate_line` over the entries' ranges.

### 4. Auto-follow scroll — minimal, original-column

New method `scroll_to_highlight(&self, work_idx)` (or fold into
`highlight_work_line`): after tagging, compute the highlighted ORIGINAL line's
absolute y in the scroll viewport and scroll only if it's outside:

1. Locate the entry + `off` (as above). Get the line's y-range in the `orig`
   view: `orig.iter_at_line(off)` → `orig.line_yrange(&iter)` → `(y, height)`
   (y is relative to the orig view's buffer).
2. Map the orig view's origin into `content_vbox` via
   `orig.compute_point(&content_vbox, Point(0, y))` → absolute `line_top`;
   `line_bottom = line_top + height`.
3. Read the `scrolled` vadjustment `value`/`page_size`. If
   `line_top < value`, set value = `line_top` (scrolled up just enough). If
   `line_bottom > value + page_size`, set value = `line_bottom - page_size`
   (scrolled down just enough). Otherwise leave it. Clamp to
   `[lower, upper - page_size]`.

Defer the measure one idle tick (like `scroll_to_block`) so allocations are
settled. On initial `show()`, the existing `scroll_to_block(cursor_idx)` is
replaced by `highlight_work_line(cursor_idx)` + `scroll_to_highlight(cursor_idx)`
so the open lands highlighted and in view.

### 5. Handler wiring

`handle_translation_overlay_key` gains four arms BEFORE the catch-all. Each calls
the real nav function under `borrow_mut()`, then re-highlights + follows under a
fresh borrow:

```rust
"comma" => { nav(state, jump_to_prev_dialogue); true }
"q"     => { nav(state, jump_to_next_dialogue); true }
"j"     => { nav(state, cursor_next_dialogue);  true }
"k"     => { nav(state, cursor_prev_line);      true }
```

where `nav` is a local helper:

```rust
fn nav(state: &Rc<RefCell<AppState>>, f: fn(&mut AppState)) {
    f(&mut state.borrow_mut());                 // moves cursor + seeks MPV
    let s = state.borrow();
    if let Some(w) = s.work_line_for_buffer(s.current_line) {
        s.translation_overlay.highlight_work_line(w);
        s.translation_overlay.scroll_to_highlight(w);
    }
}
```

The old `j`/`k` `scroll(±1)` arms are removed. `i`/`Escape` close (unchanged);
everything else is still swallowed (`_ => true`).

The nav functions also update the main card's highlight/scroll on `text_view` —
that's desired (reader stays in sync) and harmless to the overlay (different
widgets).

## Scope / non-goals

- No row-locking; columns stay free-flow (offset accepted).
- No new MPV code — reuse the nav → `after_page_change` → seek path verbatim.
- No change to the main card, the interlinear view (`Alt+i`/`ToggleTranslations`),
  or `keymap.json`/`keymap_config.rs` — these binds are intercepted in the overlay
  handler, not added as global binds.
- Highlight reuses `cursor_line_bg`; no new theme color.
- `space` already works (global); not touched.

## Verification

- `cargo build`, `cargo test --bins` — add a unit test for `locate_line`
  (block-range → (block, offset), including not-found and boundary cases).
- Visual ("renders on screen") criterion → headless/manual launch of H8: open the
  overlay with `i`; press `,`/`q`/`j`/`k` and confirm the cursor highlight moves
  in BOTH columns, MPV seeks (audio jumps to the line), the overlay auto-scrolls
  to keep the highlighted original line visible (only when it would leave view),
  and `space` pauses/plays. Closing returns the reader already on that line.

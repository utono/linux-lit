# Vocab Popup Shows Stale Content After Page Turns

## Symptom

With auto vocab popup enabled (toggled by `h`), the popup kept showing the
definition of a word that was not even on the visible page. For example, the
cursor highlighted a line containing "insipid" but the popup showed the
definition for "eminent".

## Root Causes

Two distinct bugs combined to produce this behavior.

### 1. Page-level navigation never refreshed the popup

`auto_show_vocab_popup(state)` in `src/input/navigation.rs` refreshes the
popup when the cursor moves. Most cursor-moving functions called it, but
several page-level and jump-based ones did not:

- `page_forward` (the `x` key)
- `page_backward` (the `y` key)
- `page_backward_bottom` (Shift+,)
- `cursor_to_page_bottom` (`Q`)
- `jump_to_next_vocab`
- `jump_to_prev_vocab`

Each updates `state.current_line` but returned without telling the popup to
refresh, so the popup kept whatever data it last loaded.

### 2. "Current paragraph" broke on prose without blank-line separators

`open_vocab_popup` and `refresh_vocab_popup` collected vocab words from the
"current paragraph", computed by `current_paragraph_range()` — which walks
backward/forward until it finds a blank buffer line.

In prose works loaded via a `text_file`, `build_line_map` joins lines with
`\n` and strips blanks, so the buffer has **no blank lines**. The paragraph
walk therefore ran to the buffer boundaries, producing a range covering the
entire work. The popup was then populated with every vocab word in the book
(393 of them), and always displayed the first one — "eminent".

Log excerpt showing the failure:

```
VOCAB POPUP: current_line=807 paragraph=0..2214
VOCAB POPUP: 393 words: ["eminent", "frank", ...]
```

## Fix

1. **Scope popup content to the current line only.** `open_vocab_popup` and
   `refresh_vocab_popup` now filter `vocab_matches` by
   `m.line_index == state.current_line` instead of using paragraph ranges.
   This works uniformly for plays and prose and matches the visual mental
   model (cursor is on this line → popup shows this line's vocab words).

2. **Added `auto_show_vocab_popup` calls** to the six navigation functions
   listed above so page turns and vocab jumps trigger a refresh.

3. **Added a dedicated tracking field `vocab_popup_line: Option<usize>`.**
   Previously the auto-refresh check reused `current_paragraph_start`, which
   is also written by the MPV sync loop for scroll-on-paragraph-change. The
   two concerns are now decoupled.

4. **Hide the popup when the new line has no vocab words.**
   `refresh_vocab_popup` used to early-return in that case, leaving stale
   content on screen. It now clears and hides.

## Files Changed

- `src/input/navigation.rs` — `auto_show_vocab_popup` calls added, rewritten
  to gate on `vocab_popup_line` instead of `current_paragraph_start`
- `src/app.rs` — added `vocab_popup_line` state, rewrote `open_vocab_popup`
  and `refresh_vocab_popup` to use current line only, hide on empty

## How to Verify

1. Open a prose work with multiple vocab words across pages (e.g. the Shakespeare intro).
2. Press `h` to enable auto popup.
3. Use `x` / `y` to page forward and back. The popup should update on every
   page turn and only show words that are on the currently highlighted line.
4. Use `j` / `k` to move line by line. The popup should update each move; on
   lines with no vocab words it should close.
5. On a line with two vocab words, `\` should cycle between them and the
   counter should show `1 / 2`.

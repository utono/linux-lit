# Vocab Popup: Stale, Bleeding, or Blocking Content

Frequency-ordered ledger for vocab-popup surface bugs. Read the matching
section before attempting a fix.

## Popup bleeds through an overlay or picker (2026-07-29)

**Tell.** A gloss/journal overlay or any picker is open over the reader and
the vocab definition card is still painted at the right edge, on top of it.

**Root cause.** There was no suspend/restore concept at all: the popup's
visibility was tied only to explicit open/close calls, and nothing observed
the reader→overlay transition. There is also **no shared "enter overlay"
helper** — roughly 50 sites assign
`input_mode = InputMode::{Gloss,Journal,Synopsis}Overlay` directly — so there
was no single place a close could have been added, and adding one per site
would rot as new open paths land.

**Fix.** `crate::input::keymap::handle_key` is the sole funnel for key input,
so the reconciliation lives there: sample `input_mode` before dispatch,
compare after, and on a transition into an overlay call
`vocab_popup::suspend_for_overlay` (hides the widget, sets `suspended`,
preserves `data`/`index`/`view`); on the way back to the reader call
`restore_after_overlay` (re-places, then re-shows). `handle_key` is now a
thin wrapper around `handle_key_inner`, which holds the original body.

**Two traps this hit, in order:**

1. **Suspend must NOT be a `close_vocab_popup`.** A close is permanent and
   also hands a chat-anchored popup's carved slot back to the panel. The
   suspend has to leave the popup's data intact or the restore repaints an
   empty card. Conversely `close_vocab_popup` now clears `suspended`, so a
   popup deliberately closed while an overlay is up (the vocab-Q&A path does
   exactly this) is not resurrected on return.
2. **A whitelist of overlay variants is the wrong shape.** The first attempt
   armed only on `Reader` → {Gloss,Journal,Synopsis}Overlay. That misses
   Reader → `JournalPicker` → `JournalOverlay` (Ctrl+j) entirely — verified
   still bleeding on screen mid-fix — and it misses every picker, which also
   covers the reader card. `InputMode` has ~50 variants and grows, so a
   whitelist silently omits each new surface.

**The predicate is INVERTED, not enumerated.** `is_suspending_overlay` is
defined as `!is_reader_mode`, and `is_reader_mode` is the short list of modes
that ARE the reader card: `Reader | Visual | VocabLoop`. A new `InputMode`
therefore suspends by default — the safe direction, since a wrong suspend
costs a popup hidden for one keystroke while a wrong omission paints a card
over content.

`VocabLoop` is in the reader set deliberately: the vocab-sentence drill is
fully modal but draws NO overlay of its own (it ab-loops on the reader card),
so suspending there would hide the definition of the very word being drilled.

**Verification.** Headless (`scripts/land-on.sh BH-Barrett 4.0`), `rr` to open
the popup, then exercise a direct overlay (Ctrl+g), a picker (Ctrl+j, `z`),
and the picker→overlay chain (Ctrl+j → Return). Log breadcrumbs are
`VOCAB POPUP: suspended for overlay` / `VOCAB POPUP: restored after overlay`;
the chain should suspend ONCE and restore ONCE, not per hop. Screenshot-compare
the right edge — the restore should be byte-identical to the pre-overlay
capture.

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

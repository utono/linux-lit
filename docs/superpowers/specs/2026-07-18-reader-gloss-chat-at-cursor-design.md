# Reader-gloss chat from the main card (`-` at cursor)

**Date:** 2026-07-18
**Status:** Approved design

## Problem

Opening a reader-gloss in the chat panel currently requires entering visual
mode (`V`), selecting the passage, then pressing `-` (`action_reader_gloss_chat`
in `src/input/visual.rs`). When the cursor already sits on a line that a
reader-gloss covers, the `V`-select step is redundant — the passage span is
already known from the stored gloss.

## Goal

In **reader mode** (no `V`), pressing `-` on a line covered by a reader-gloss
opens the chat panel pinned to that gloss's full passage and shows the stored
gloss — the same end state as visual-mode `-`, without the selection step.

## Behavior

Plain `-` in reader mode (`InputMode::Reader`):

1. Resolve the cursor line to its `(div1, div2, line_in_div)` citation
   (via `work_line_for_buffer` + `Work.lines`), under `Work.canonical_abbrev`.
2. Load reader-gloss passages for the work
   (`find_glossed_passages(conn, abbrev, &["reader-gloss"])`) and find the one
   **covering** the cursor line (`passage_covers`, as `open_gloss_at_cursor`
   does).
3. **Covering reader-gloss found:** build the gloss context over that passage's
   full `[start_citation, end_citation]` line span and drive the existing
   pin-and-load-gloss flow — pin the chat to the passage, show the stored gloss
   (cache hit, no API call), land focus in the transcript. `r`/`R` in the panel
   still forces a fresh gloss.
4. **No covering reader-gloss:** brief `No gloss on this line` toast; stay in
   the reader. `-` is otherwise a no-op.

**Scope decisions (confirmed with user):**

- **reader-gloss only** — not the 3-type set (`teacher-generic`,
  `inner-monologue`, `reader-gloss`) that `Ctrl+g`/`open_gloss_at_cursor`
  matches. The chat panel's gloss flow is the reader-gloss flow.
- **Full passage span** — pin to the gloss's authored `[start,end]` span, not
  the single cursor line, so the context matches the stored gloss (cache hit,
  no re-gloss).
- **No-gloss = toast + no-op**, matching `open_gloss_at_cursor`.

## Design

Reuse, don't duplicate. The existing `action_reader_gloss_chat` already does
the hard part once it has a `GlossContext`: pin via
`open_chat_pinned_to_selection`, retire the input, load the cached gloss list,
push the cached exchange (or request a fresh gloss). It differs from the new
path only in **how it obtains the context**:

- Visual path: from `visual_selection`'s line range.
- Cursor path: from the reader-gloss passage covering `current_line`.

Refactor so the shared tail takes a prepared `GlossContext`:

- Extract the post-context body of `action_reader_gloss_chat` into a helper,
  e.g. `open_reader_gloss_chat_with_ctx(state_rc, ctx, model)`. This is
  everything from `open_chat_pinned_to_selection` onward. **Caveat:** that
  function pins to the current `visual_selection`; the cursor path has none.
  Confirm during implementation whether `open_chat_pinned_to_selection` can pin
  from a `GlossContext`/citation span directly, or whether the pin step needs a
  context-driven variant. If a selection is structurally required, the cursor
  path may set a transient selection over the passage span before pinning, then
  the existing exit-visual-mode clears it — to be resolved in the plan, not
  assumed here.
- `action_reader_gloss_chat` (visual) builds its ctx from the selection, then
  calls the helper — behavior unchanged.
- New `reader_gloss_chat_at_cursor(state_rc)`:
  - Resolves the covering reader-gloss passage from the cursor (the
    `open_gloss_at_cursor` resolution pattern, filtered to `reader-gloss`).
  - Builds the passage's `Line` vec from its `[start,end]` span and
    `crate::gloss::build_context_for_type(work, &lines, "reader-gloss")`.
  - Toasts `No gloss on this line` and returns on any miss.
  - Calls the shared helper.

### Files

- `src/input/actions/mod.rs` — add `Action::ReaderGlossChatAtCursor`.
- `src/input/keymap_config.rs` — bind `KeyCombo::plain("minus")` →
  `ReaderGlossChatAtCursor`. Plain `minus` is currently **unbound**
  (asserted `None` at keymap_config.rs:512) — no conflict. Update the freed-key
  comment + the `plain("minus") == None` assertion (now `Some(...)`).
- `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`) —
  add the same bind so the JSON override doesn't shadow the compiled default;
  deploy with `cd ~/tty-dotfiles && stow linux-lit`.
- `src/input/keymap.rs` — reader-mode dispatch: `ReaderGlossChatAtCursor` →
  new handler. (Plain `minus` in reader context must route here; confirm no
  existing reader-mode `"minus"` arm intercepts before dispatch.)
- New handler in `src/input/visual.rs` (beside `action_reader_gloss_chat`) or
  `src/input/actions/chat.rs` — `reader_gloss_chat_at_cursor` + the extracted
  shared helper.
- `src/ui/keybinds_overlay.rs` — `-` is a main-card bind, so add it to the
  Ctrl+/ overlay: keycap strip entry + `describe()` detail arm
  (`update-cairo-keybinds-overlay` skill, three-pass cross-reference).

## Testing

Headless cage drive (`test-headless-navigation` harness):

1. Open a work with a known reader-glossed passage (e.g. a `Cym`/`Err` passage
   from the gloss fixtures).
2. Move the cursor onto a glossed line, press `-`; assert the chat panel opens
   showing the stored gloss (transcript focus, no re-gloss).
3. Move onto an unglossed line, press `-`; assert the toast fires and no panel
   opens.

`cargo build` + `cargo test --bins` for the keymap assertions
(`plain("minus")` now `Some`, dispatch mapping). Final on-screen eyeball on the
real GL renderer handed to the user.

## Out of scope

- Non-reader-gloss types (teacher-generic / inner-monologue) via `-`.
- Opening an empty chat pinned to an unglossed line (that's `Tab`'s job).
- Any change to visual-mode `-` behavior beyond the internal refactor.

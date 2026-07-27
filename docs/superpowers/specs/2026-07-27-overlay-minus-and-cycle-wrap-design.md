# Overlay `-` family + `\` cycle wrap — design

Date: 2026-07-27 (US Central)

Two independent changes to overlay keybinds, specced together because both
touch the gloss and journal Q&A overlay modal handlers.

## 1. The `-` family inside the gloss and journal Q&A overlays

### Problem

The word-copy/underline family (`-`, `Shift+-`, `Alt+-`, `Ctrl+-`) works only
in reader mode. Inside the gloss and journal Q&A overlays the same caps do
nothing, so a word in a gloss body or a Q&A answer cannot be copied or
underlined without leaving the overlay.

### Current behavior

`handle_gloss_key` and `handle_journal_key` bind neither `minus` nor
`underscore` nor plain `Return`. All four binds are additive with no
collisions on either surface.

The reader implementation (`src/input/actions/word_copy.rs`) operates on
`state.buffer` and `state.current_line` — the reader's own `GtkTextView` and
line cursor. Neither is meaningful inside an overlay.

### Decisions

- **Scope is the whole current cursor block**, not a visual line. Both
  overlays already have a block cursor (`cursor_block` + `blocks`) and a
  `current_block_text()` accessor, and overlay blocks wrap across several
  display lines, so a line scope would be arbitrary. This is the same scope
  the `rr` vocab popup already uses on these surfaces
  (`gloss_overlay_scope_words` / `journal_overlay_scope_words`).
- **All four binds port**: `-` word forward, `Shift+-` word back, `Alt+-`
  collect, `Ctrl+-` next sentence. Wrapping behavior matches the reader
  (forward past the last word → first; back from the first → last).
- **`Return` is NOT bound** on either overlay. In the reader, `Return`
  (`OpenSyntaxDiagramForUnderlined`) opens a syntax gloss for the underlined
  span. That must not happen from inside an overlay — an explicit user
  instruction, and it also avoids recursively opening a gloss from a gloss.
- Clipboard copy and the persistent underline behave as they do in the
  reader.

### Design notes

`word_copy.rs`'s pure helpers (`next_word_index`, `next_sentence_first_word`,
`first_word_from`) are already `AppState`-free and unit-tested; they are
reused unchanged. What is new is the per-surface adapter: read the cursor
block's text and buffer span, tokenize it, and apply the underline tag within
that span. The gloss overlay's `blocks` carry `start_line`/`end_line` buffer
line ranges, which give the span directly; the journal overlay's blocks are
checked for the equivalent during implementation.

Because `gloss_view` and its buffer are private, the tag application lands as
a method on each overlay type, following the established
`apply_vocab_tags` / `apply_rewrite_diff` pattern, rather than exposing the
buffer.

### Out of scope

Chat transcript and synopsis overlays (chat already binds plain `-` to close
the panel).

## 2. `\` overlay cycle: wrap, and skip empty stops

### Problem (as reported)

`\` "is not cycling to the syntax overlay." Reproduced and root-caused from
the debug log and lit.db, NOT from the cycle code alone.

### Root cause

Two distinct issues; the reported symptom is caused by the second.

**(a) The lap ends rather than wrapping.** By design the lap is
`reader → gloss → journal Q&A → syntax → reader (ends)`.

**(b) The journal stop is a dead end when the scene has no Q&A.** This is
the actual defect. `overlay_cycle.rs`'s module doc claims the journal stop
"toasts and continues to the syntax stop regardless", but the code cannot do
that: `cycle_from_gloss` ends by calling `journal::open_journal_scene`, which
returns `()`. On an empty scene that function toasts "No journal entry for
this segment" and returns early, having already closed the gloss overlay and
restored the reader position — so the user lands back in the reader and the
lap is dead. The syntax stop is then unreachable by cycling.

Evidence (Ant-Arkangel 5.2, `linux-lit-dev.log`):

- `journal_entries` has zero rows for `Ant` 5.2.
- The log shows `\` in `mode=GlossOverlay` followed immediately by `\` in
  `mode=Reader`, repeatedly, with `CHAPTER_TOAST: … "No journal entry for
  this segment"` on exactly those presses — the journal and syntax stops
  never open.
- The same passage DOES have a syntax gloss (passage 17589,
  `Ant.5.2.424`–`425`), and it opens correctly via the `Return` path
  (`SYNTAX-GLOSS: showing cached gloss`), confirming the gloss itself is
  fine and only the cycle route is broken.

Note the reader-gloss and the syntax-gloss sit on DIFFERENT passages with
different spans (17588: 424–437; 17589: 424–425). That is expected — a
syntax gloss is created from an explicit narrower selection — and is not
itself the bug, but it means the syntax stop's cursor-covering lookup can
legitimately miss when the anchor line falls outside the narrower span.

### Decisions

- **Wrap, do not end.** `\` at the syntax stop reopens the gloss stop
  instead of returning to the reader, so `\` rotates
  `gloss → journal Q&A → syntax → gloss → …` indefinitely.
- **Escape is the only exit.** The reader is removed from the rotation; each
  overlay's existing Escape/close keys are untouched and remain the way out.
- **Skip empty stops.** A stop with nothing to show is passed over rather
  than ending the lap. This is a prerequisite for the wrap to be usable at
  all — without it the rotation still dies at the first empty stop.
- **If no other stop has content, stay put and toast.** Rather than spinning
  forever or silently dropping out, the current overlay stays open and a
  toast says there is nothing else to cycle to. Without this the key would
  appear dead — the user pressed `\` and nothing visibly happened.

### Design notes

`open_journal_scene` and `try_open_syntax_gloss_at_cursor` must report
whether they opened anything (return `bool`, or an equivalent
`did_open` signal) so the cycle can fall through. Their standalone callers
ignore the value and keep today's toast behavior.

Guard against infinite recursion when every stop is empty: the advance walks
at most one full rotation and stops. When the walk completes without finding
a stop that can open, the current overlay is left untouched and a toast
reports that there is nothing else to show — reusing
`show_chapter_toast_secs`, the same transient the empty-stop toasts already
use.

The advance must therefore be able to test a stop WITHOUT tearing down the
current overlay first: today `cycle_from_gloss` hides the gloss overlay and
restores the reader position before it ever calls the journal open. If the
next stop turns out to be empty, that teardown has to be undone to "stay
put". Cleaner is to check availability first (a cheap DB lookup for a
covering journal entry / syntax gloss at the anchor), and only tear down once
a reachable stop is known.

### Known quirk, deliberately unchanged for now

`handle_gloss_key`'s `\` arm branches on the *displayed* `gloss_type`, not on
how the overlay was entered. With wrapping, opening a syntax gloss directly
from the picker and pressing `\` now advances to the gloss stop instead of
returning to the reader. That follows from the wrap decision and is
consistent; called out here so it is not mistaken for a regression.

## Verification

Build, clippy, and `cargo test --bins` are mandatory. On-screen acceptance
per the project's headless-verification rules:

- `-`/`Shift+-`/`Alt+-`/`Ctrl+-` underline and copy within the cursor block
  in BOTH overlays; `Return` does nothing in either.
- `\` from a gloss on a scene with NO journal entry reaches the syntax stop
  (the Ant 5.2 case from the bug report).
- `\` at the syntax stop returns to the gloss stop, not the reader.
- `\` on a passage whose ONLY stop is the current overlay leaves it open and
  toasts that there is nothing else to cycle to.
- Escape still exits every overlay.

Both keybind changes require the lockstep keybind-surface updates:
`keymap_config.rs` is not involved (these are overlay modal binds, not
reader binds), but the per-overlay legends
(`src/ui/{gloss,journal}_keybinds_overlay.rs` GROUPS + MRU consts) must be
updated in the same change, per CLAUDE.md.

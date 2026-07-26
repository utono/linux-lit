# Word-underline selection for the syntax diagram

## Purpose

Give the reader a second, lighter way into the syntax diagram: underline one or
more words on the current line with `-` / `_`, press `Return`, and get the
diagram for the sentence those words belong to.

Reader visual mode (`V` → Action menu → Syntax) already opens the diagram for a
LINE RANGE. That is the right tool for a speech or a paragraph. It is heavy for
the common case — "what is this clause doing?" — which starts from a word, not
a line span.

## What already exists

`-` and `_` are `WordCycleCopy` / `WordCollectCopy`
(`src/input/actions/word_copy.rs`), bound in `keymap_config.rs` and mirrored in
the stowed `~/.config/linux-lit/keymap.json`:

- `-` cycles words on the current line, copying each to the clipboard via
  `wl-copy` and underlining it.
- `_` accumulates a multi-word phrase the same way, underlining all of them.
- Both underline via `apply_word_underline`, which arms a 2-second
  `glib::timeout_add_local_once` that removes the tag. `WordCycleState.bold_gen`
  is a generation counter that invalidates a stale timer.

`open_syntax_diagram(state, text, line_ids)`
(`src/input/actions/syntax.rs`) already takes a text-only path when `line_ids`
is empty, and the diagram's own Escape handler already returns to the reader
(`handle_syntax_diagram_key`).

**The clipboard copy is unchanged by this design.** `-`/`_` keep copying
exactly as they do today.

## Design decisions

### No new InputMode

The obvious shape is `InputMode::WordUnderline`. It is the wrong one here.

A mode would mean every unrelated reader bind either stops working while words
are underlined or needs an explicit passthrough arm. But the state that
distinguishes "words are underlined" ALREADY EXISTS:
`WordCycleState.collect_ranges` is non-empty exactly then.

So the reader stays in `InputMode::Reader`, and `Return` / `Escape` get arms
guarded on `!collect_ranges.is_empty()`. Reader navigation, search, and page
turns keep working while words are underlined.

`Return` is currently UNBOUND in reader mode (verified against
`keymap_config.rs`), so this takes a free key rather than displacing anything.

### The binds are handler changes, not new bind entries

`-`/`_` keep their existing `WordCycleCopy` / `WordCollectCopy` actions. The new
behavior lives in the handlers and in two new guarded reader arms. Consequence:
the stowed `keymap.json` (which already maps `minus`/`underscore` to those
actions) needs NO redeploy and cannot silently shadow this change.

### Underline lifetime

Persistent, not the current 2-second expiry:

- Cleared by `Escape` in the reader.
- Cleared by moving the cursor to a DIFFERENT line — mirrors the existing
  `cycle_line` reset, and stops an underline drifting offscreen where it can
  still be diagrammed by an unwitting `Return`.
- Cleared by loading another work — otherwise `collect_ranges` are stale char
  offsets pointing into different text.

Persistence is implemented by NOT arming the timer, so `bold_gen` keeps its
existing job of invalidating any timer still in flight.

**Clearing is lazy, not event-driven.** `current_line` has ~76 write sites
across 14 modules; hooking every cursor-move path would be unimplementable and
would rot on the next navigation feature. Instead the underline state carries
the line it belongs to — `WordCycleState.cycle_line` ALREADY records this — and
is treated as empty whenever `cycle_line != current_line` or the work has
changed. One helper,

```rust
fn active_underline(state: &AppState) -> &[(usize, usize)]
```

returns the ranges only when they still belong to the cursor's line and work,
and is the single source of truth for the `Return`/`Escape` guards. The visible
tag is removed opportunistically on the next `-`/`_`/`Escape`; a tag that
briefly outlives its line is cosmetic, not a correctness problem, because
nothing can act on it.

### `-` accumulates too

Today `-` CLEARS `collect_ranges` (it is single-word mode). For diagramming, one
underlined word must be a valid selection, so `-` SETS `collect_ranges` to
exactly its one range instead. `_` keeps appending. Both then give a uniform
answer to "what is underlined".

### The diagram round-trip preserves the selection

`Escape` in the diagram returns to the reader with the words STILL underlined,
so the span can be widened with another `_` and re-diagrammed. A second `Escape`
(now in the reader, with a non-empty selection) clears them.

## Components

### 1. `src/input/sentence.rs` — the sentence span (new)

Pure, no GTK, unit-testable without a display. The piece most likely to be
subtly wrong, so it carries the most tests.

```rust
/// Expand `ranges` (char offsets into `text`) outward to sentence boundaries.
/// `text` is the already-joined buffer region; the caller decides how much
/// context to hand in, so this function never touches lines, the buffer, or
/// GTK.
pub fn sentence_span(text: &str, ranges: &[(usize, usize)]) -> Option<(usize, usize)>;
```

**Char offsets into one joined string, not (line, char) pairs.** Two reasons
found while reviewing:

- A "line" is not a unit here. Buffer lines in a two-column play are short verse
  lines, but a prose `line_mapping` row in BH-Barrett runs to 2,874 characters —
  a whole paragraph holding many sentences. A `(start_line, end_line)` struct
  implies a granularity the data does not have.
- The consumer, `open_syntax_diagram`, wants a `String` and a `Vec<i64>` of line
  ids. Char offsets into a joined region give both directly.

The caller joins a bounded window (the cursor's line plus one line either side,
which covers a sentence spanning a verse break without risking a whole-chapter
scan), maps the resulting span back to the line ids it covers, and passes those
along for `line_syntax` enrichment.

Boundaries are `.`, `!`, `?`. Cases that must be handled, because the corpus is
full of them:

- **Abbreviations** — `Mr.`, `Mrs.`, `Dr.`, `St.`, initials (`J. R.`). A period
  followed by a space and a lowercase letter, or preceded by a known
  abbreviation, is NOT a boundary.
- **Quoted speech** — `"What's that?"` — the closing quote/paren belongs to the
  sentence, so the boundary is after any trailing `"`, `'`, `)`.
- **Multi-line sentences** — prose wraps and verse breaks mid-sentence; the span
  crosses line breaks.
- **Underline spanning two sentences** — take the UNION (first sentence's start
  to last sentence's end) rather than guessing.
- **Work edges** — no preceding boundary means start-of-work; no following
  boundary means end-of-work.

### 2. `word_copy.rs` — persistence and clearing

- `apply_word_underline` gains a `persist: bool`; when true it does not arm the
  removal timer.
- New `clear_word_underline(state)` removes the tag and empties
  `collect_words` / `collect_ranges`.
- `word_cycle_copy` sets `collect_ranges` to its single range rather than
  clearing it.

### 3. `actions/syntax.rs` — the new entry point

```rust
pub fn open_syntax_diagram_for_underlined(state_rc: &Rc<RefCell<AppState>>);
```

Resolves `collect_ranges` → `sentence_span` → the span's text and the
`line_mapping` ids it covers, then calls the EXISTING
`open_syntax_diagram(state, text, line_ids)`. Works with `line_syntax`
enrichment on the five parsed works and the text-only path everywhere else,
inheriting both paths for free.

### 4. `keymap.rs` — two guarded reader arms

- `"Return"` when `!active_underline(state).is_empty()` →
  `open_syntax_diagram_for_underlined`.
- `"Escape"` when `!active_underline(state).is_empty()` →
  `clear_word_underline`.

Both guards read `active_underline(state)`, so they fall through both when
nothing is underlined AND when the underline belongs to a line the cursor has
since left. Neither key changes behavior for a reader with nothing underlined,
and reader mode binds no `Escape` today, so both arms are purely additive.

No cursor-move or work-load hooks are added — clearing is lazy (see "Underline
lifetime").

## Error handling

- **`Return` with nothing underlined** — falls through. Not an error; the bind
  is simply not active, so no toast. This includes the lazy-clear case: ranges
  belonging to a line the cursor has left are treated as absent.
- **Sentence span resolves empty or whitespace-only** — `open_syntax_diagram`
  ALREADY guards this (`if text.trim().is_empty()` → log, return), so the new
  entry point adds no check of its own and simply does not open. Do not
  duplicate the guard.
- **No sentence boundary found in the window** — the whole joined window is the
  span. Degrades to "diagram this paragraph", which is a reasonable answer, not
  an error.
- **Everything downstream** — inherits the diagram's existing three failure
  modes (API error, malformed JSON, invalid spans).

## Keybind surfaces

Per the project's keybind rule, the same change updates:

- `src/ui/keybinds_overlay.rs` — keycap strip AND the `describe()` arm, for
  `-`, `_`, `Return`, and reader `Escape`.
- `docs/guides/keybind-consistency-guide.md` — `-`/`_` gain a second meaning
  (select-for-diagram alongside copy); record the decision in its change log.
- NOT `keymap.json` — no bind entries change (see above).
- NOT `keybind-surface-guide.md` — on-request only.

## Testing

`sentence.rs` is pure — it takes a `&str` and returns offsets, so every case
below is a `cargo test --bins` unit test with no display, buffer, or DB:
boundary expansion, abbreviations (`Mr.`, initials), quoted speech (closing
quote included), sentences spanning a line break inside the joined window,
two-sentence unions, a window with NO boundary (whole window is the span), and
window edges.

`active_underline` is also pure enough to test directly: same line + same work
returns the ranges; a different line or work returns empty.

Headless on-screen check (mandatory, and the only way to see an underline):
drive `-`, `_`, `Return`, `Escape` on BH-Barrett. Criteria:

1. `-` underlines one word and the underline PERSISTS past 2 seconds.
2. `_` adds a second word; both stay underlined.
3. `Return` opens the diagram for the whole sentence, not just the words —
   verify the diagram's text is the full `. ! ?`-bounded sentence.
4. `Escape` in the diagram returns to the reader with underlines intact.
5. `Escape` again clears them.
6. Moving the cursor off the line clears them.

## Non-goals

- **No new InputMode.**
- **No change to the clipboard copy.**
- **No underline editing** (no "remove the last word") — `Escape` and start over.
- **No change to visual mode's Syntax action** — both entry points coexist.
- **No cross-work or cross-paragraph spans** — a sentence stops at the work's
  edges.

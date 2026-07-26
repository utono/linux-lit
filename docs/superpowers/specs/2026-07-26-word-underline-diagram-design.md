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
/// Expand `ranges` (char offsets on `line`) outward to sentence boundaries,
/// crossing line breaks. Returns the char span of the whole sentence.
pub fn sentence_span(
    lines: &[String],
    line: usize,
    ranges: &[(usize, usize)],
) -> Option<SentenceSpan>;

pub struct SentenceSpan {
    pub start_line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
}
```

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

- `"Return"` when `!collect_ranges.is_empty()` → `open_syntax_diagram_for_underlined`.
- `"Escape"` when `!collect_ranges.is_empty()` → `clear_word_underline`.

Both fall through when the selection is empty, so neither key changes behavior
for a reader with nothing underlined. Reader mode binds no `Escape` today, so
this arm is purely additive.

Cursor-move and work-load paths call `clear_word_underline`.

## Error handling

- **`Return` with nothing underlined** — falls through. Not an error; the bind
  is simply not active, so no toast.
- **Sentence span resolves empty or whitespace-only** — toast, do not open.
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

`sentence.rs` is pure: `cargo test --bins` covers boundary expansion,
abbreviations, quoted speech, multi-line spans, two-sentence unions, and
start/end-of-work edges.

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

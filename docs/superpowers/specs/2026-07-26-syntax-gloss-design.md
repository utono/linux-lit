# syntax-gloss — grammatical analysis as a gloss type

## Purpose

Replace the ephemeral Cairo syntax diagram with `syntax-gloss`, a sixth
`gloss_type` stored and rendered like every other gloss: prose in
`glosses.gloss_text`, keyed to a `passage_id`, drawn by the existing gloss
overlay.

The reader selects a passage, asks for a syntax gloss, and gets the passage's
grammatical structure, a note on what that structure is doing rhetorically, and
definitions of the terms used — saved, re-openable, searchable, and listed in
the gloss picker alongside everything else.

## Why the drawing goes

The Cairo diagram (merged 2026-07-26, `0e3573f9`) gave two things: bands sat
under the words they covered, and depth/span/POS were visible at once.

Against that, from one day of building it:

- **Four rounds of layout defects**, each fix exposing the next: labels
  overprinting their own rules, band rules striking through the POS row,
  sibling labels colliding, then a width fix that suppressed 5 of 6 labels.
  Recorded in `clip-prevention.md` entry 15.
- **Cage passed layouts the real GL renderer rejected**, so every acceptance
  needed a hand-off.
- **~1,000 lines** of `syntax_overlay.rs`, nearly all geometry arithmetic.
- **It cannot join the gloss system.** Cairo is not text, so the diagram has no
  storage, no picker entry, no search, no TTS, no editing, no model provenance
  — and every viewing costs a fresh API round-trip.
- **Alignment already degraded on wrap.** A band spanning two visual lines
  splits into segments, so the spatial story fragments on exactly the long
  sentences this reader is full of.

The bands data is the valuable part. Rendering it as geometry bought less than
it cost.

**The accepted loss: spatial alignment.** Spans are quoted rather than drawn
under their words. This is the one real capability given up, and it is
deliberate.

## What is stored

One row in `glosses`:

- `gloss_type = 'syntax-gloss'`
- `passage_id` — the same passage key every other type uses
- `gloss_text` — the existing block markup
- `claude_model` — model provenance, as for other types

`gloss_type` is a free-text column, so **there is no schema migration**. The
five existing values (`reader-gloss`, `teacher-generic`, `inner-monologue`,
`vocab-word`, `vocab-elided`) gain a sixth.

## The gloss body

Three sections, in this order, in the markup vocabulary the renderer already
parses (`<segment>`, `<gloss>`):

1. **The passage**, in a `<segment>` pair — the same treatment every gloss
   gives its source text.
2. **Structure** — one line per band: the term, an em dash, and the words that
   band covers, indented by nesting depth. A band's own text is quoted rather
   than referenced by position, so nothing can misalign and a line wrap is
   irrelevant. Long spans elide in the middle (`first words…last words`).
3. **What the structure is doing** — the rhetorical note, in a `<gloss>` pair.
   Two or three sentences. This already exists in the diagram's schema and is
   generated on every call today; it is merely hidden behind a toggle.
4. **Terms** — one entry per DISTINCT band label, in first-appearance order,
   each in a `<gloss>` pair. Three "conjoined predicate" bands yield one entry.
   Each defines the term generally ("a clause opening with who, which, or
   that…"), not the specific span.

Sections 3 and 4 are the reason for the change: they answer *what is an
appositive* in place, which the drawing never did.

**Per-word POS tags are dropped.** They existed to fill the diagram's tag row
and were the source of the abbreviation problem the legend was added to solve
(and of the request to suppress `PUNCT`). In prose they would be noise — a
reader who wants to know that "composure" is a noun is served by the Terms
section, not by 23 abbreviations. The prompt stops requesting them, which also
shortens the response.

## Prompt

The existing compiled fallback in `src/input/actions/syntax.rs` is rewritten to
request markup rather than JSON. lit.db has **no `syntax.diagram` row** today
(verified: `SELECT ... FROM api_prompts WHERE prompt_key LIKE 'syntax%'`
returns nothing), so the compiled string is the single source of truth and no
DB migration or prompt-version bump is needed.

The prompt keeps the `line_syntax` enrichment exactly as it works now: where a
work has parsed rows they are sent as a token table so Claude anchors on the
real parse; where it does not, the text goes alone. That is a solved problem
and does not change.

## Entry points — full parity

Per the decision to give this the same reach as other gloss types:

- **Gloss picker.** `GlossPickerFilter` (`src/input/actions/pickers.rs:17`)
  gains a `SyntaxGloss` variant in its `gloss_type()` and `next()` arms, so
  Alt+t cycles to it and Alt+g lists saved syntax glosses for the work.
- **Re-open without an API call.** A saved gloss for the passage opens from the
  picker like any other; only a first request hits the API.
- **The `\` overlay cycle.** `overlay_cycle.rs` gains syntax-gloss in the
  rotation.
- **The reader bind.** The existing entry points are kept: visual mode →
  "Syntax", and `-`/`_` underline then `Return`. Both now produce a saved gloss
  instead of an ephemeral drawing.

## What is deleted

- `src/ui/syntax_overlay.rs` (~1,000 lines) — the Cairo surface.
- `src/ui/syntax_keybinds_overlay.rs` — its Ctrl+/ legend.
- `InputMode::SyntaxDiagram` and `handle_syntax_diagram_key`.
- `src/syntax_diagram.rs`'s band/POS geometry helpers (`assign_rows`,
  `max_row`) — the row-packing work from the superseded layout plan.
- The `n` toggle-note bind, since the note is always shown.

`src/db/syntax.rs` (`load_line_syntax`) is KEPT — the enrichment still feeds
the prompt.

## Error handling

Inherits the gloss system's existing paths rather than defining new ones: an
API error toasts and closes; a save failure is logged. The malformed-JSON and
invalid-span cases disappear with the JSON schema — the response is prose, and
prose that reads oddly is a quality problem, not a crash.

## Testing

**Unit (`cargo test --bins`, no display):** the structure-section builder is
pure — bands plus text in, indented markup out. Cases: nested bands indent by
depth; a repeated label yields one Terms entry; a long span elides in the
middle; zero bands yields a gloss with note and terms but no structure list.

**On screen, on the real GL renderer** (cage disagreed with GL on every layout
defect this branch hit): a syntax gloss renders all three sections in the gloss
overlay; it appears in the Alt+g picker under Alt+t; re-opening a saved one
makes no API call; the `\` cycle reaches it.

## Non-goals

- **No Cairo, no geometry, no Reed-Kellogg.** The drawing is removed, not
  reworked.
- **No persistent term glossary table.** Terms are defined per gloss; a shared
  lit.db glossary was considered and declined.
- **No change to `line_syntax` or litdb.** Read-only consumer, unchanged.
- **No change to the other five gloss types.**
- **The POS legend and the queued `PUNCT` removal are moot** — both belonged to
  the Cairo POS row, which no longer exists.
- **Overlay visual-mode `s` is deliberately NOT carried over.** The Cairo
  diagram's `Shift+V, s` bind (gloss/synopsis/journal overlays) built its
  diagram straight from the selected on-screen blocks. A gloss must key to a
  `passage_id`, and overlay prose has no `line_mapping` rows to derive one
  from — there is no `passage_id` to attach a syntax-gloss to from an overlay
  selection. The bind is removed outright, not rehomed.

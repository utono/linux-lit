# Journal Q&A source-quote paragraph breaks

## Problem

The source passage quoted at the top of a journal Q&A entry renders all its
lines run together as one block, with only soft line-wrapping. When a prose
passage spans several paragraphs (e.g. *A Tale of a Tub* Preface, citation
`0.0.5–7` — three distinct paragraphs), the paragraph breaks are lost: the
reader sees a wall of text instead of the three separated paragraphs.

## Root cause

`source_paragraphs` in `src/input/actions/journal.rs:421` collapses **every**
quote line into a single `\n`-joined paragraph (`out.push(verse.join("\n"))`,
line 449). The overlay (`src/ui/journal_overlay.rs`) renders each
`JournalSource.paras` entry as its own paragraph joined by `"\n\n"`, so with
only one quote paragraph, no blank-line break ever appears between the source's
distinct paragraphs — only the citation gets its own separate paragraph.

The `passages.source_text` column stores plain text with `\n` separating lines,
but the source_text that reaches `source_paragraphs` at render time is rebuilt
with per-line `<segment>` tags by `build_source_header`
(`src/input/actions/echoes.rs`) — verified on-screen: the render receives
`<segment>This infallibly convinced me…</segment>\n<segment>In two days…`.
So the fix must handle the `<segment>`/`<stage>` path, not only the untagged
plain-text path. Each source line is:

- a distinct **paragraph** for prose works (TT: `line_in_div` 5, 6, 7 are three
  full paragraphs), OR
- a **verse line** within one continuous speech for verse/play works (Cymbeline:
  `line_in_div` 1, 2, 3 are the three verse lines of a single sentence).

`block_type` does not distinguish these (both stored as `'prose'` in this DB),
so the newline count alone cannot tell them apart.

## Authoritative signal

`works.work_type` is the discriminator: TT is `prose`, Cymbeline is `play`.
The reader already exposes this via `crate::db::line_types::is_prose_work(work_type)`
(`src/db/line_types.rs:22`), and the journal render path already computes
`is_prose` from the current work at `journal.rs:578-581` and hands it to
`set_prose_reading` (`journal.rs:582`). This is the "authoritative metadata,
not text inference" rule the project follows — no line-length heuristics.

## Fix

Give `source_paragraphs` an `is_prose: bool` parameter and flush the
accumulated quote lines (from BOTH the `<segment>`/`<stage>` path and the
untagged-plain path) by it:

- **Prose work** (`is_prose` true): each quote line becomes its **own** `paras`
  entry, so the overlay's `"\n\n"` join yields a blank-line gap between
  paragraphs.
- **Verse/play work** (`is_prose` false): unchanged — quote lines collapse into
  ONE `\n`-joined paragraph rendered at pure line-height, matching the main
  reading card.

A `<speaker>` label flushes the prior speech first (so lines never merge across
a label) and is always its own paragraph. The flush lives in one closure so both
the `<segment>`/`<stage>` branch and the untagged branch share it.

The caller at `journal.rs:640` passes the `is_prose` already in scope from
line 578-581.

## Styling is already correct — no second edit

`apply_source_style` (`journal_overlay.rs:1550`) already gates the hang-indent
verse tag behind `!self.prose_reading.get()` (line 1617), because a prose quote
is one wrapped buffer line and the hang indent would ragged-left it. So for
prose works the verse styling is skipped entirely; the only styled source role
is the citation, whose buffer line `source_line_roles` computes correctly from
per-paragraph start positions (`journal_overlay.rs:221-227`) no matter how many
quote paragraphs there are. `set_prose_reading(is_prose)` (`journal.rs:582`)
uses the same flag, so overlay styling and paragraph splitting stay in lockstep.

## Testing

TDD — extend the existing `source_paragraphs` unit tests in
`src/input/actions/journal.rs`:

- **prose, multi-paragraph + citation**: 3 `\n`-separated plain lines with
  `is_prose=true` → 4 `paras` (3 quote paragraphs + citation), `has_speaker`
  false, `has_citation` true.
- **prose, single line**: 1 plain line + citation, `is_prose=true` → 2 `paras`.
- **verse, unchanged**: `<speaker>` + 3 `<segment>` + citation with
  `is_prose=false` → speaker, ONE joined verse block, citation (existing
  `source_paragraphs_speaker_verse_citation` assertion, plus the `is_prose`
  arg).

Update the three existing `source_paragraphs(...)` call sites in the test
module to pass the `is_prose` arg.

Then a headless cage screenshot of the TT `0.0.5–7` Q&A to confirm the
blank-line gaps render on screen (the acceptance criterion is visual — verify on
the rendered surface, not from the paragraph count alone).

## Scope

One function-signature change + its single caller + test updates. No schema
change, no `source_text` capture-path change, no keybind change, no
`source_line_roles`/`apply_source_style` change.

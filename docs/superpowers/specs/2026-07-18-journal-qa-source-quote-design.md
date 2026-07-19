# Journal Q&A: quote source text above the question

**Date:** 2026-07-18
**Status:** Approved (design), pending implementation plan
**Scope:** Reader journal Q&A overlay — passage-scope entries

## Problem

When a journal entry is associated with source text — a highlighted passage the
question was asked about — the reader currently stores that source
(`journal_entries.source_text` + `start_citation`/`end_citation`) for provenance
but **never reproduces it** in the Q&A rendering. Landing on such an entry
(e.g. via Ctrl+n/p) shows only the question and answer; the reader has no
on-screen reminder of *which lines* the Q&A concerns.

Today's render path makes this explicit — `src/input/actions/journal.rs:503–507`:

> Every Q&A — including passage pages — renders as a plain Q&A. The passage
> source block is intentionally NOT shown … a Q&A's rendering never reproduces
> the source. (Previously passage pages used a verse renderer that printed the
> source above the answer.)

We are reversing that decision: quote the source, labelled and cited, above the
question.

## Goal

For a journal entry that carries source text, render — above the question — the
quoted passage with:

1. a **speaker label** (small-caps),
2. the **verse/prose lines** of the passage (hang-indented like the reading card),
3. a **compact citation** line: `— <Title>, <act>.<scene>.<line-range>`
   (e.g. `— Cymbeline, 1.1.1–3`), right-aligned,
4. a centered **`———` separator** between the source block and the Q&A.

The question and answer render exactly as they do today, below the separator.

## Verified data & code facts

Established during brainstorming against `~/utono/litdb/data/lit.db` and the
linux-lit source — the design rests on these, not assumptions:

- **`JournalPage` already carries everything.** `src/db/journal.rs` `JournalPage`
  has `source_text: Option<String>`, `start_citation`, `end_citation`, `kind`,
  `question`, `answer`. The render function already holds the page. **No schema
  change, no query change.**
- **The renderer already exists.** `crate::ui::gloss_render::populate_gloss_buffer`
  parses `<speaker>/<verse>/<stage>` markup into a small-caps speaker + hang-
  indented verse — the exact look the approved mockup shows. The transient
  passage-ask card (`journal_overlay::show_passage_source`,
  `journal.rs:487–501`) already calls it. We reuse this, not a new parser.
- **Current data.** All 38 journal entries; **29 have `source_text`**, and every
  one of those is `scope='passage', kind='qa'` **with a citation present**
  (`source_no_citation = 0`). So "any entry with `source_text`" and
  "only passage-scope" select the identical rows today. The citation-omission
  branch (below) is future-proofing that never fires on current data.

## Decisions (locked in brainstorming)

- **Which entries:** render the source block whenever `page.source_text` is
  **non-empty**. Not gated on `scope`. If the citations are missing/partial,
  render the quote **without** the citation line (omit, don't guess).
- **Citation format:** compact — `— Cymbeline, 1.1.1–3`. Title from the work;
  `1.1.1` from `start_citation` (`ABBR.div1.div2.line`); the range tail from
  `end_citation`'s line. Collapse to a single locator when start==end
  (`— Cymbeline, 1.1.1`).
- **Separator:** centered `———` (the same separator `journal_block.rs` already
  recognizes on passage pages).
- **Paging:** the source block + citation appears on the entry's **first page
  only**. Continued-answer pages (Ctrl+n within a long Q&A) do **not** repeat
  the source. First page = `page_index`'s first rendered page for the entry;
  concretely, the source is prepended only when rendering the first buffer page
  of the current entry.

## Architecture

One rendering change, isolated to the passage-Q&A branch of the journal
overlay's page render.

### Components

**1. Citation formatting (pure helper).**
A small pure function — `format_source_citation(title, start_citation,
end_citation) -> Option<String>` — producing `— Cymbeline, 1.1.1–3`, the single-
locator collapse, and `None` when start_citation is absent. Lives beside the
existing citation helpers in `src/input/actions/journal.rs` (near
`band_label_for_page` at ~line 337–360, which already parses citations).
Unit-testable in isolation.

**2. Source-block assembly.**
Build the source document string the overlay renders: the page's `source_text`
markup, plus a trailing citation line, plus the `———` separator, positioned
above the Q&A. Two candidate seams (chosen in the plan):

- **(a) Overlay-side (preferred):** extend `journal_overlay::show_page` with an
  optional `source_doc: Option<&str>` argument. When `Some`, the overlay renders
  it through `populate_gloss_buffer` (speaker/verse), appends the citation +
  `———`, then renders the Q&A blocks below — reusing the exact machinery
  `show_passage_source` already uses. This keeps buffer/paragraph/block bookkeeping
  in the overlay, where `show_passage_source` and `show_page` both live, and
  keeps the navigable Q&A blocks intact (unlike the transient card, which clears
  blocks).
- **(b) Caller-side string prepend:** `journal.rs` prepends a pre-rendered source
  string to the question before calling `show_page`. Simpler call site but loses
  the speaker/verse *rendering* (small-caps, hang-indent) that only
  `populate_gloss_buffer` applies to live markup — so (a) is preferred.

**3. Render dispatch.**
Replace the `journal.rs:503–517` "plain Q&A only" branch: when
`page.source_text` is non-empty **and** this is the entry's first page, pass the
assembled source doc to `show_page`; otherwise pass `None` (unchanged behavior).

### Data flow

```
nav_page (journal.rs)
  └─ page = pages[page_index]              // JournalPage, already loaded
     ├─ if pending_passage matches band → show_passage_source   (unchanged)
     └─ else:
         source_doc = (page.source_text non-empty && first page of entry)
             ? Some(assemble(source_text, format_source_citation(title, start, end)))
             : None
         show_page(..., q, a, kind, source_doc, cw, h)          // extended
```

### What does NOT change

- `journal_entries` schema, all journal DB queries, and the `JournalPage` struct.
- The transient passage-ask path (`show_passage_source`) — it already shows the
  source; untouched.
- Scene/author/word note rendering (no `source_text` → `source_doc = None`).
- Block navigation, TTS caching recolor, overlay search, and entry-diff
  highlight that run after `show_page` — the Q&A blocks are still present; the
  source block is non-navigable chrome (matching how `show_passage_source`
  treats it).
- Ctrl+/ journal legend (no new keybind; this is passive rendering).

## Edge cases

- **No citation** (`start_citation` empty): render speaker + verse, omit the
  citation line. Never fabricate a locator.
- **start == end line:** single locator (`— Cymbeline, 1.1.1`), no en-dash range.
- **Speakerless source** (prose works, `<speaker>UNKNOWN</speaker>`):
  `populate_gloss_buffer` already handles this (flush, no hang) — no special case.
- **Long Q&A across pages:** source only on the first rendered page; the
  "first page of entry" test must be robust to the overlay's internal block
  paging (plan defines exactly how "first page" is detected).
- **Empty `source_text`** but non-null: treated as absent (`.is_empty()` after
  trim) → `None`.

## Testing

- **Unit:** `format_source_citation` — range, single-locator collapse, missing
  citation → `None`, en-dash correctness. Pure, no GTK.
- **Unit:** source-doc assembly — separator placement, citation appended,
  speakerless input.
- **Headless e2e (verify-overlay-ui / manual):** open Cymbeline (Arkangel),
  land on entry #12 in the journal overlay, confirm on screen:
  - `FIRST GENTLEMAN` small-caps label,
  - three verse lines, hang-indented,
  - `— Cymbeline, 1.1.1–3` right-aligned,
  - `———` separator, then the question and answer,
  - Ctrl+n to a continued page shows **no** repeated source block.
- **Regression:** a scene/author note (no source_text) renders exactly as before.

## Reference

- Approved visual mockup (compact citation, source above question, `———`
  separator): matches this design.
- Current "source NOT shown" decision being reversed:
  `src/input/actions/journal.rs:503–507`.
- Reused renderer: `src/ui/gloss_render.rs::populate_gloss_buffer`;
  existing source-render call site: `journal_overlay.rs::show_passage_source`.

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
  the source. First page = `page_idx == 0` for the current entry.
- **Styled:** the source renders with the mockup's styling — small-caps speaker
  label + hang-indented verse + dim right-aligned citation.
- **Stoppable:** the source lines are **navigable** — `j`/`k` stop on them and
  read-aloud voices them, i.e. they are real Q&A blocks, not chrome.

## Seam decision (finalized after code analysis)

A `code-searcher` analysis established that `render_page`'s buffer is a plain
`set_text(body)` where `body = paras[page_start..page_end].join("\n\n")`, and
every downstream concern (blocks, `<hi>` highlight, overlay search, rewrite
diff, cursor projection, `page_char_span`) is anchored to that plain body at
buffer offset 0. Writing tagged content via `populate_verse_buffer` (which does
its own `set_text("")` and owns the buffer) cannot coexist with that model, and
threading a page-0 char/line offset through 5 functions is high-risk.

**Chosen approach — source lines are REAL leading paragraphs, styled after
`set_text`:**

1. At the render call site (`journal.rs` `nav_page`, ~508–517), when
   `page.source_text` is non-empty, build source paragraphs (speaker line,
   verse line(s), citation line, `———`) and pass them to `show_page` as a new
   `source_para: Option<Vec<String>>` argument.
2. `show_page` prepends those paragraphs to `all_paragraphs` **before** the
   question/answer paragraphs (only for the render — they are real entries in
   the paragraph list). Because they are ordinary paragraphs, blocks, cursor,
   char-span, search, and diff all keep working with **zero offset math** — the
   analysis's main risk is avoided.
3. Styling is a **post-`set_text` tag pass** (`apply_source_style`), gated to
   `page_idx == 0`, mirroring the existing `apply_hi_color` pattern
   (`journal_overlay.rs:1181`): it looks up/creates small-caps-speaker,
   hang-verse, and dim-citation tags and applies them by line range over the
   source paragraphs shown at the top of the buffer. Continued pages
   (`page_idx > 0`) never include the source paragraphs, so nothing to style.
4. Stoppability is automatic: the source paragraphs are real blocks, so `j`/`k`
   and read-aloud include them, as decided.

**Consequence noted:** prepending source paragraphs shifts the block index of
the question/answer paragraphs for that entry, which shifts `journal_audio`
cache keys `(entry_id, block_index)` for already-cached answers. This is a
one-time stale-miss (re-render/re-synthesis on next play), **not** corruption.
Acceptable.

## Architecture

One rendering change, isolated to the passage-Q&A branch of the journal
overlay's page render, plus two pure helpers and one after-`set_text` tag pass.

### Components

**1. Citation formatting (pure helper).**
`format_source_citation(title, start_citation, end_citation) -> Option<String>`
— produces `— Cymbeline, 1.1.1–3`, the single-locator collapse
(`— Cymbeline, 1.1.1`), and `None` when `start_citation` is absent/unparseable.
Lives beside the existing citation helpers in `src/input/actions/journal.rs`.
Uses `crate::app::parse_citation` for `(div1,div2,line)`. Pure, unit-testable.

**2. Source-paragraph assembly (pure helper).**
`source_paragraphs(source_text, citation) -> Vec<String>` — parses the
`<speaker>/<verse>` markup into paragraph strings the overlay will style:
one speaker paragraph (plain text, styled later), the verse line(s) as
paragraph(s), the citation line, and a `———` separator paragraph. Reuses the
existing markup parse (`gloss_render::parse_gloss_tags` or the local
`first_plain_source_line` sibling) to strip tags to plain text. Pure,
unit-testable.

**3. `show_page` extension + source styling (overlay).**
`journal_overlay::show_page` gains a `source_para: Option<Vec<String>>` argument.
When `Some`, it prepends those paragraphs to `all_paragraphs` ahead of the Q&A
paragraphs (so they are real navigable blocks), records how many source
paragraphs precede the Q&A, and — after `render_page`'s `set_text` on page 0 —
runs `apply_source_style`: a tag pass mirroring `apply_hi_color`
(`journal_overlay.rs:1181`) that applies small-caps to the speaker paragraph,
hang-indent to the verse paragraph(s), and a dim right-aligned tag to the
citation, by line range within the shown buffer. Gated to `page_idx == 0`
(continued pages don't carry the source paragraphs).

**4. Render dispatch (`journal.rs`).**
Replace the `journal.rs:503–517` "plain Q&A only" branch: when
`page.source_text` is non-empty, build `source_para` via the two helpers (title
from `s.current_work`) and pass `Some(source_para)`; otherwise pass `None`
(unchanged behavior for notes and source-less entries).

### Data flow

```
nav_page (journal.rs)
  └─ page = pages[page_index]              // JournalPage, already loaded
     ├─ if pending_passage matches band → show_passage_source   (unchanged)
     └─ else:
         source_para = page.source_text.non_empty()
             ? Some(source_paragraphs(source_text,
                      format_source_citation(current_work.title, start, end)))
             : None
         show_page(..., q, a, kind, source_para, cw, h)         // extended
             → all_paragraphs = [ ...source_para, Q, A... ]      (page 0)
             → render_page set_text(body); apply_source_style() when page_idx==0
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

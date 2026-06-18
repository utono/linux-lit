# Kindle-style BCP rendering on the `text_file` path

**Status:** Design (recommend + open questions; pending implementation plan)
**Related:**
`2026-06-16-bcp-decorative-typography-design.md` (the rubric/heading/small-caps
styling this extends to a new path),
`2026-06-16-bcp-sentence-per-line-design.md` (the sentence-per-line constraint
trait #3 supersedes for the wrapped mode),
`2026-04-02-ereader-pagination-design.md`,
`2026-04-28-block-atoms-and-prev-page-top-design.md`,
`2026-06-01-two-column-ereader-layout-design.md` (the pager model trait #3
extends);
sibling `ws-book-of-common-prayer-references` (produces the BCP TEI → `.txt`).

## Problem

BCP1549 Matins now renders through an **authored `.txt`** (the sibling repo's
`tei_to_text.py` output, pointed at by `works.text_file`). That reached the
"1662 layout" (sentence-per-line prayers, inline `Aunswere.`/`Priest.`), but it
does **not** match the Oxford BCP Kindle edition the user is targeting. Side by
side, the Kindle differs in four ways:

1. **Rubrics** (stage directions) are centered + italic. linux-lit shows them
   upright and left-indented.
2. **Speaker cues** (`Aunswere.`, `Priest.`) sit on their own centered italic
   line above the response. linux-lit renders them inline (`Aunswere.  text`).
3. **Prayers** (Lord's Prayer, Gloria) are wrapped, justified paragraphs.
   linux-lit shows one sentence per physical line.
4. **Body text** is fully justified. linux-lit is ragged-right.

**Root cause for #1/#2.** The decorative BCP typography from
`2026-06-16-bcp-decorative-typography-design.md` is real and works — but it only
runs on the **DB-loaded** path. `apply_dialogue_formatting` (`src/app.rs:3582`)
calls `apply_bcp_formatting` (`src/app.rs:3792`) **only when
`work.text_file.is_none()`** (guard `src/app.rs:3593-3598`). Setting
`works.text_file` (done for BCP1549 Matins) routes the work through the generic
prose path, which finds no `is_speaker` matches in BCP prose, sets
`dialogue_formatting_active = false`, and returns — applying **no** line-type
styling. The work then shows whatever literal whitespace the `.txt` carries
(`clean_file_lines`, `src/app.rs:3246`, preserves leading spaces verbatim).

This spec defines how to bring the Kindle look to BCP works **that have a
`text_file`**, reusing the styling already specified for the DB path and adding a
true wrapped-paragraph mode.

## Goal

For `BCP*` works loaded via `text_file`:
- run the existing decorative typography (centered/italic rubrics, centered
  headings, small-caps divine names) — trait #1;
- render speaker cues on their own centered italic line — trait #2;
- render prayers as wrapped paragraphs that still paginate and stay audio-synced
  — trait #3.

Full justification (trait #4) is **out of scope** (see below). Non-BCP works are
untouched throughout.

## Findings (load-bearing, with anchors)

- **Two render paths.** `apply_dialogue_formatting` (`src/app.rs:3582`) →
  `apply_bcp_formatting` (`:3792`) gated on `text_file.is_none()`
  (`:3593-3598`). text_file BCP works fall through to the generic speaker scan
  (`:3615`) and get no styling.
- **Styling primitives already exist** (built in `apply_bcp_formatting`):
  `bcp-heading` `Justification::Center` (`src/app.rs:3823`), `bcp-rubric-centered`
  `Italic + Center` (`:3831`), `bcp-rubric-hanging` (`:3842`), divine-name
  small-caps spans. Classification in `src/db/line_types.rs`: `is_rubric:83`,
  `rubric_is_centered:96`, `is_speaker:30`, `is_bcp_heading:65`,
  `split_bcp_sentences:195`.
- **`clean_file_lines`** (`src/app.rs:3246`) keeps each `.txt` line verbatim
  except blank-before-speaker drop, multi-line `[...]` fold, and `## ` strip — it
  does **not** trim leading whitespace, so the `.txt`'s cosmetic centering/indent
  reaches the buffer literally.
- **Matcher and audio-sync are line-granularity-agnostic.**
  `MatchMode::ParagraphAccumulate` (`src/text_file_map.rs:363-462`) re-accumulates
  consecutive physical lines into one `line_mapping` row; a whole paragraph on one
  physical line matches trivially (the degenerate 1:1 case). Timestamps and gutter
  signs key on `line_mapping.id` via `work_line_for_buffer`
  (`src/app.rs:509-552`) and `src/input/timestamps.rs:86-94` — never the physical
  buffer line. So changing physical-line granularity does not break sync.
- **Pagination is the only blocker to wrapping.** `visible_range`
  (`src/input/viewport.rs:56-77`) sums whole-physical-line heights (`line_yrange`,
  which already includes GTK soft-wrap height) and can only break a page
  **between** physical lines. A paragraph emitted as one physical line that is
  taller than a column stalls (`count == 0`, the chain cannot advance) — the exact
  reason `2026-06-16-bcp-sentence-per-line-design.md` split sentences. GTK already
  soft-wraps for display (`WrapMode::Word`, `src/app.rs:1136`). Wrap-aware y→row
  primitives exist but are unused by the pager: `row_bottom` / `line_at_y`
  (`src/input/scroll.rs:1100-1132`). `page_top` is a `usize` line index pervasively
  across `viewport.rs`, `scroll.rs`, and the column split — that index assumption
  is the load-bearing constraint trait #3 must relax.
- **Full-justify is a documented GTK dead-end** (`src/app.rs:3802-3808`; also
  noted in both prior BCP specs): GtkTextView renders `Justification::Fill` as
  ragged-right for word-wrapped text. True justification needs a custom
  Pango/Cairo body widget.

## Recommended design, per trait

### Trait #1 — decorative typography on the text_file path
Make a BCP work run BCP styling regardless of `text_file`:
- Extend the guard at `src/app.rs:3593-3598` so `is_bcp_work(&w.abbrev)` runs
  `apply_bcp_formatting` (or a text_file-aware sibling) even when
  `text_file.is_some()`.
- For BCP, **stop baking cosmetic centering/indent into the `.txt`** and stop
  preserving it: have `tei_to_text.py` emit heads/rubrics flush (no leading-space
  centering, no 4-space rubric indent) and/or strip that leading whitespace for
  BCP in `clean_file_lines`. Otherwise the `Justification::Center` tag centers a
  line that is *already* space-padded → double offset.
- Reuse the existing `bcp-heading` / `bcp-rubric-centered` / `bcp-rubric-hanging`
  / divine-name tags unchanged. No new styling machinery — this is purely making
  the existing pass run on this path with a clean (un-padded) buffer.

### Trait #2 — speaker cue on its own centered italic line
Two coordinated changes:
- **Data (`tei_to_text.py`, sibling repo):** emit a `<sp>` as the speaker label on
  its own line, then the response below — instead of the current inline
  `Speaker.  text`.
- **Reader:** classify the label line via `is_speaker` and style it with a new
  `bcp-speaker-centered` tag mirroring `bcp-rubric-centered` (`Italic + Center`).
- **Matcher contract (must verify):** the canonical `line_mapping` row stores the
  speaker inline (`Aunswere. And my mouthe…`). Splitting it into a label line +
  a response line means `ParagraphAccumulate` must re-accumulate **both** back
  into that one row. Confirm the normalized concatenation of the two lines equals
  the row text (it does, modulo whitespace), and add a regression test
  (`src/text_file_map.rs` test module) for a speaker label + response → one row.

### Trait #3 — wrapped prayer paragraphs (the pager change)
Supersede sentence-per-line **for the wrapped mode**:
- Emit **one physical line per BCP paragraph** (bypass `split_bcp_sentences` for
  text_file BCP, and the sentence split in `tei_to_text.py`). The matcher
  collapses to 1:1 (findings above) — strictly simpler.
- Add a BCP-gated pager that breaks a page at a **soft-wrap display-row boundary
  inside a physical line**, using the y→row primitives already in
  `src/input/scroll.rs:1100-1132`, instead of only between whole lines in
  `visible_range` (`src/input/viewport.rs:56-77`). This moves the page-boundary
  model from "line index" to "line index + display-row offset" for BCP, and must
  thread through the block-atom trim (`viewport.rs:313-389`) and the two-column
  split (`viewport.rs:1290-1307`).
- Audio sync / gutter unchanged (row-keyed). 
- **HIGH RISK / pervasive** (`page_top: usize` assumption is everywhere). Recommend
  implementing as a **separate phase with its own review**, after #1/#2 land.

### Trait #4 — full justification
**Out of scope.** Record the GTK limitation (`src/app.rs:3802-3808`); achieving it
would require replacing the TextView body renderer with a custom Pango/Cairo
widget, affecting every work, not just BCP. Not pursued here.

## Phasing
- **Phase 1 (low risk):** traits #1 + #2 — self-contained styling + speaker-line
  split, verifiable against the Kindle screenshot.
- **Phase 2 (high risk, separate review):** trait #3 — the pager wrapping change.
- **Not planned:** trait #4.

## Open questions (for the implementation plan)
- **Architecture: keep `.txt` or drive BCP from the DB?** Both can host the
  styling. Keeping the authored `.txt` (recommended) preserves layout authorship
  and the sibling-repo TEI pipeline; extend its guard (trait #1). Alternatively
  drive BCP purely from the DB path (where `2026-06-16-bcp-sentence-per-line`
  already runs `apply_bcp_formatting`) — styling comes free but the authored
  `.txt` layout is abandoned. Spec recommends keep-`.txt` + extend-guard.
- **Snapshot version.** `2026-06-16-bcp-sentence-per-line-design.md` notes
  text_file-backed BCP works **do** use the snapshot cache (unlike DB-path BCP).
  Traits #2 and #3 change BCP buffer line counts, so a `SNAPSHOT_VERSION` bump
  (`src/snapshot.rs:8-25`) is likely required to invalidate stale caches.
- **`RUBRIC_CENTER_MAX_WORDS`** — reuse the threshold/heuristic from the
  decorative-typography spec; re-tune against 1549 rites if needed.

## Testing
- Unit (`cargo test --bins`): a `bcp-speaker-centered` classification/styling
  test; a `ParagraphAccumulate` regression test for speaker-label + response →
  one row (trait #2); for trait #3, a pager test that a wrapped paragraph taller
  than a column paginates (breaks at a soft-wrap row) instead of stalling.
- Visual (user-run `cargo run`, per the repo's headless caveat): load BCP1549
  Matins via its `text_file` and compare against the Oxford Kindle screenshots —
  centered italic rubrics, centered speaker cues, wrapped prayers.

## Out of scope (this spec)
Full justification (#4). DB schema changes. Any change to non-BCP rendering.

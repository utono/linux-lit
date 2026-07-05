# Prose Visual-Row Pagination — Design

**Date:** 2026-07-05
**Status:** Approved design, pre-plan
**Scope:** linux-lit engine + lit.db schema (+ hot schema mirror), litdb
phrase-timestamp import

## Problem

Prose works (e.g. BH) paginate at whole-buffer-line granularity, and one
prose buffer line = one paragraph. Two user-visible failures (screenshots
2026-07-05, BH paragraphs 54–55):

- **Tail skip (text loss).** The over-tall sub-line paging shipped in
  `0044804` only lives in the `x`/PageForward path
  (`src/input/navigation.rs:886`). A `j` cursor step past an over-tall
  paragraph turns the page via the cursor-follow path
  (`NAV_PAGE_FWD: current=55 old_top=54 new_top=55`, `offset=0.0`
  throughout), so the wrapped tail of paragraph 54 ("—in which Mr.
  Sladdery … the sincere and entire truth.") appears on **no** page.
  Sync-driven turns bypass the over-tall branch the same way.
- **Underfill.** `viewport::visible_range` (`src/input/viewport.rs:56`)
  only places whole paragraphs, so a page ends early whenever the next
  paragraph will not fully fit — pages with ~40% blank card.

Root cause for both: page boundaries cannot land inside a paragraph
except in the one narrow over-tall-at-page-top case.

## Decisions (brainstorm outcomes)

- **Full e-reader fill.** Every prose page fills with wrapped visual
  rows; paragraphs split across pages routinely. One boundary rule for
  `x`, `j`/cursor-follow, and sync.
- **Pinned prose page table**, like the play tables: new prose-shaped
  tables. **lit.db gets the schema change first; the hot repo's schema
  is updated afterward.** Play tables stay play-shaped.
- **Sync turns use phrase timestamps** (`phrase_timestamps`, already in
  lit.db schema but empty), with char-fraction interpolation as the
  fallback when a media file has no phrase rows.
- **Import phrase timestamps for BH first**, wire the import into the
  litdb production workflow for all future works, backfill existing
  prose works in later batches. Plays are low-urgency (page breaks never
  split a verse line).

## 1. Engine — visual-row page fill (linux-lit)

A prose page boundary becomes `(buffer_line, row_offset_px)`. Page-fill
walks wrapped visual rows from the current boundary, accumulating row
heights until `usable_height` is exceeded, and snaps the break to a real
visual-row top via `scroll::snap_value_to_display_row`
(`src/input/scroll.rs:1409`). The existing over-tall branch
(`overtall_forward_step`, `src/input/navigation.rs:731`) becomes the
general case rather than a special case.

- All three turn paths — `page_forward`/`page_backward`, the
  cursor-follow turn in `j`/`k` stepping, and the sync page turn —
  resolve boundaries through the **same** function (or, once pinned,
  through the table; see §2).
- Single-column prose only (`column_count() == 1`). Two-column plays
  keep `column_split` untouched.
- No widow/orphan rules initially (YAGNI). Breaks are guaranteed on
  clean visual-row tops by the snap helper; a one-row carryover is
  acceptable.

### Contracts respected

- `page_top_offset` (`src/app/mod.rs:231`) remains the sub-line state;
  viewport top = `line_yrange(page_top_line).y + page_top_offset`.
- Back-stack stays `Vec<(usize, i32)>`; every push records
  `(page_top_line, page_top_offset)`.
- Offsets must strictly advance (`new_off > cur_off`) or the turn falls
  back to a whole-line step (stall guard, as today).
- Clip: pages whose top or bottom sits mid-paragraph use the per-row
  `display_rows`/`bottom_clip_height` math with **no** descender
  allowance (clip-prevention.md #6/#10). Whole-line boundaries keep
  `paged_bottom_clip` + `descender_allowance`. The two paths are not
  merged.
- Cursor/highlight stays per-paragraph. When the highlighted paragraph
  straddles a boundary, the visible portion is highlighted; no new
  highlight machinery. Cursor-at-page-top == paragraph line, so the
  existing sync guard semantics hold.

## 2. Pinned prose page table

New tables in **lit.db** (schema change lands here first; mirror in the
hot repo's schema afterward — hot's `page_spread`/`page_spread_meta`
remain play-only):

```sql
CREATE TABLE prose_pages (
    work_abbrev      TEXT NOT NULL,      -- edition's own abbrev
    page_index       INTEGER NOT NULL,
    start_line_id    INTEGER NOT NULL,   -- line_mapping.id
    start_row_offset INTEGER NOT NULL,   -- px from line top, row-snapped
    end_line_id      INTEGER NOT NULL,
    end_row_offset   INTEGER NOT NULL,   -- px; exclusive bottom edge
    PRIMARY KEY (work_abbrev, page_index)
);

CREATE TABLE prose_pages_meta (
    work_abbrev        TEXT PRIMARY KEY,
    layout_fingerprint TEXT NOT NULL,
    db_fingerprint     TEXT NOT NULL,
    page_count         INTEGER NOT NULL,
    generated_at       TEXT NOT NULL,
    validated          INTEGER NOT NULL
);
```

Lifecycle copies the play-table pattern (`src/input/page_table.rs`):

- **Generate lazily** in-app, once per session, at settled geometry, by
  running the §1 engine forward from line 0. Citation-keyed
  (`line_mapping.id`), never buffer indexes.
- **Invariant-gate before persisting** (prose-shaped suite, pure +
  unit-tested):
  - *Coverage / no text loss:* every visual row of every paragraph lies
    in exactly one page interval — zero gaps, zero overlaps. This is
    the machine-checked guarantee that the screenshot bug class cannot
    recur under the table.
  - *Ordering:* boundaries strictly monotone in `(line, offset)`.
  - *Fit:* each page's summed row heights ≤ `usable_height`.
  - *Row alignment:* every stored offset is a real visual-row top at
    generation geometry.
  - Fail → log `PAGES: VALIDATE_FAIL`, keep the live engine (same
    fallback posture as plays).
- **Staleness:** `layout_fingerprint` (font, Pango metrics probe,
  window size, spacing, margins, columns) + `db_fingerprint` (same
  digest as the snapshot cache), both must match on load; drop on
  resize (`revalidate_on_resize` analog).
- **Consumption:** one gate à la `active_page_table` — `None` unless
  prose, EReader mode, table valid, `LIT_NO_PAGE_TABLE` unset. Turns
  become index ±1; sync and `G` land on the table grid (as plays do
  after `c85e87b`).
- **Lookup convention:** a buffer line maps to the page containing its
  **first** visual row; row-offset-aware variant for cursor/sync
  positions inside straddling paragraphs.

Offsets are pixel values valid only under the stored
`layout_fingerprint`, which is exactly what the meta gating enforces —
same trade the play tables already make.

## 3. Sync — phrase-timestamp page crossing

When the playing paragraph spans a page boundary, the boundary is a
known char offset into that paragraph (from the row break → byte/char
index in the buffer line). Turn timing:

- **Primary:** look up the `phrase_timestamps` row for
  `(line_mapping_id, media_id)` covering that `start_char`; turn the
  page at its `start_time` minus the usual sync preroll. Accuracy is
  within one phrase (≤5 words).
- **Fallback:** if the media file has no phrase rows, interpolate by
  char fraction across the line's audio window (its `start_time` to the
  next line's `start_time`).
- Landing follows the table grid (§2 lookup), mirroring the play-table
  sync landing rule. The existing `cursor > last_vis` guard semantics
  are unchanged; the turn is driven by time-crossing, not cursor
  overflow.

## 4. litdb — populate phrase_timestamps

`scripts/build_phrase_timestamps.py` already implements WhisperX-word →
`canonical_text` alignment with punctuation/gap-aware phrases (≤5
words) writing to `phrase_timestamps`. The table exists and is empty.

- Run for BH's two media files (media ids 243, 244; WhisperX caches at
  `~/Music/dickens-charles/whisperx-cache/BleakHouse*_ep6…json`). Fix
  any bit-rot; validate alignment on a sample (chapter openings, the
  paragraph 54 "Sladdery" break) before writing.
- Add the step to the litdb production workflow so all future imports
  with a WhisperX cache get phrase rows.
- Backfill existing works opportunistically, prose before plays. Size
  note: lit.db is ~795 MB; a full backfill across the ~734 MB of cached
  transcripts is on the order of +100 MB — roll out per-author, not
  big-bang.

## Sequencing

1. Engine (§1), verified by prose nav-fuzz plus a new no-text-loss e2e
   assertion (page N's last visible row and page N+1's first row are
   adjacent visual rows).
2. Table + invariants (§2) in lit.db; consumption gate; fuzz asserts
   read table boundaries in table mode (per the play-table lesson).
3. BH phrase import (§4, first bullet).
4. Sync crossing (§3).
5. hot repo schema mirror.

Each step is independently shippable; the engine alone fixes the text
loss and underfill.

## Verification

- `cargo test` unit coverage for the pure boundary/invariant helpers.
- Headless cage e2e: BH at 1920×1200 (`wlr-randr` resize), drive
  `j`/`x` across paragraphs 52–56, assert no skipped rows and no
  underfill beyond one row; screenshot review per UI protocol.
- test-playback-sync skill run for the §3 crossing behavior.
- `validate-play-pages`-style audit skill for `prose_pages` (follow-up).

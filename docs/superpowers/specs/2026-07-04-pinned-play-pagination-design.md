# Pinned Play Pagination — Design

**Decision (2026-07-04):** Approach A — an authoritative page table for
Shakespeare plays stored in lit.db by citation, generated and validated by the
app itself at the pinned layout, with the live engine as generator and
fallback. Includes a `validate-play-pages` skill for auditing stored tables.

## Problem

Every play-pagination bug in the repo's history — the `y GAP` class, H8's `G`
stranded at ~1701, the MND dialogue-tail (fixed 2026-07-04), the off-page
cursor after the final-spread redirect — has the same shape: **page boundaries
are recomputed at runtime by walking measured geometry through heuristic
discriminators** (empty-right-column tests, dialogue-below tests, fill guards,
redirects). Pango's measurement is reliable; the derived decisions drift.

The reading environment for plays is fixed in practice: Charter 17pt,
1920x1200 (dwl), two columns, e-reader mode. At a pinned layout, pages are a
precomputable property of the text. Making them **data** retires the
discriminator bug class for the common case: navigation becomes index
arithmetic, and correctness (full coverage, tail reachability, `G`
idempotency, no clipped columns) becomes a set of invariants validated once at
generation time instead of emergent runtime behavior.

## Scope

- **In:** two-column Shakespeare plays (work types that resolve to the 2-col
  verse layout), e-reader mode, no interlinear translations.
- **Out (live engine keeps handling these, unchanged):** prose works,
  anthologies, sonnet sequences; scroll mode (`j`/`k` free scroll);
  interlinear translations (`Ctrl+Alt+i`); any 1-col override; any layout
  fingerprint mismatch (font cycled, other machine/resolution, re-imported
  text). Toggling a fallback mode off returns to the table.

## Data model (lit.db)

Two tables, created by an app-side migration (same pattern as the
`claude_model` migration):

```sql
CREATE TABLE IF NOT EXISTS play_pages (
    work_abbrev   TEXT NOT NULL,
    page_no       INTEGER NOT NULL,          -- 1-based, contiguous
    left_start_id INTEGER NOT NULL,          -- line_mapping.id, first line of left col
    split_id      INTEGER,                   -- first line of right col; NULL = empty right
    end_id        INTEGER NOT NULL,          -- last line ON the page (inclusive)
    PRIMARY KEY (work_abbrev, page_no)
);
CREATE TABLE IF NOT EXISTS play_pages_meta (
    work_abbrev        TEXT PRIMARY KEY,
    layout_fingerprint TEXT NOT NULL,
    db_fingerprint     TEXT NOT NULL,        -- same digest the snapshot cache uses
    page_count         INTEGER NOT NULL,
    generated_at       TEXT NOT NULL,        -- US Central ISO timestamp
    validated          INTEGER NOT NULL      -- 1 only after the invariant suite passed
);
```

- **Citation keys, not buffer indexes.** `line_mapping.id` survives buffer
  renumbering; the `db_fingerprint` guard catches the case where a re-import
  changes the ids themselves (table is then stale and ignored).
- **Edge pages are representable, not special-cased:** empty left column
  (first-spread short-opening moved right) = `left_start_id == split_id`;
  empty right column (scene-end watermark spread) = `split_id NULL`.
- ~~The base abbrev convention follows `canonical_abbrev`: `Cym`, `Cym-Amb`,
  `Cym-BBC` share text and therefore share one table keyed by the canonical
  abbrev (same rule as glosses/journal).~~ **AMENDED post-ship (2026-07-04):
  tables are keyed by the EDITION's own abbrev.** Canonical sharing cannot
  work in lit.db: each edition carries its own `line_mapping` ids, so a table
  stored under the base could never be loaded by a sibling edition (the
  `db_fingerprint` gate fails closed) while editions would overwrite each
  other's rows under the shared key. Each edition now holds its own table;
  the hot.db greenfield schema (`page_spread`, keyed by base abbrev +
  citation strings) is where true sharing lives.

## Layout fingerprint

A short hash of everything the geometry depends on:

- font family + size from config (pinned: Charter 17)
- a Pango metrics probe of that font (ascent, descent, approx char width in
  pango units) — catches font-stack upgrades that change metrics at the same
  nominal size
- monitor logical resolution + the resolved card/column widths
- `line_spacing`, `text_margins`, column count, and a schema version constant

Computed at load; equality with `play_pages_meta.layout_fingerprint` (and
`db_fingerprint`) is the gate. Any mismatch → live engine + a
`PAGES: fallback (<reason>)` log line.

## Generation (in-app, lazy)

On loading a play in table scope with missing/stale meta:

1. After the first settled layout (the existing resize-tick reveal point), run
   the **existing live engine** once as the generator: walk the forward chain
   from line 0 (`next_page_top`/`column_split`), recording each spread's
   `(left_start, split, end)`, ending with the `last_page_top` anchor spread.
2. Run the **invariant suite** (below) against the recorded spreads.
3. Pass → write `play_pages` + `play_pages_meta` in one transaction
   (`validated=1`), log `PAGES: generated N pages for <abbrev>`. Fail → write
   nothing, log `PAGES: VALIDATE_FAIL <invariant> <details>`, keep the live
   engine for the session. No retry loop; regeneration re-attempts next load.

Because generation runs on the real display with the real font stack, there is
no generator/runtime environment drift by construction. No headless generation.

## Invariant suite (shared by generation and the skill)

1. **Coverage:** every dialogue-bearing line of the work appears in exactly one
   page interval; intervals are contiguous and monotone (page N's `end` + 1
   region = page N+1's `left_start` modulo non-dialogue tail lines).
2. **Tail:** the last page's interval contains the work's last dialogue line
   (trailing stage directions/exits may sit past it — documented exception).
3. **Fit:** each column's summed line heights ≤ `usable_height`
   (`widget_height − descender_guard − BASE_BOTTOM_MARGIN`), i.e. the
   descender-allowance clip (`paged_bottom_clip`) provably cannot cut ink.
4. **Sanity:** `left_start ≤ split ≤ end` (where split non-NULL); empty-right
   pages only where the next page opens a `(div1,div2)` section (the watermark
   rule, checked against `section_starts`, never against buffer text).
5. **Determinism:** re-running the walk from page N's `left_start` reproduces
   page N+1 (chain idempotency — the property `G` historically violated).

## Runtime consumption

- Work load resolves the table (ids → buffer lines via the existing line map)
  into a `Vec<Spread>` on `AppState`.
- `x`/`y` = page index ± 1; `G` = last index; `gg` = 0; startup/bookmark/sync
  "which page holds line L" = binary search. `column_split`, `last_page_top`,
  `redirect_to_final_spread`, scene-snap, fill guards are **not consulted** in
  table mode.
- Rendering and clipping reuse the existing `exact_end` machinery verbatim:
  the table supplies `split` / `end + 1` as the two views' `exact_end`s, so
  `update_bottom_clip`'s validated descender-allowance path runs unchanged.
- Cursor landing keeps current semantics (first on-page dialogue on forward
  turns; last on-page dialogue when landing on the final page), now computed
  against table intervals.
- One log line per turn: `PAGES: page N/M top=<line>`.

## validate-play-pages skill

`.claude/skills/validate-play-pages/SKILL.md` + a backing script
(`validate-play-pages.sh`, SQL-first, read-only):

- **Args:** `<ABBR>` or `--all`.
- Checks per work, against lit.db only (no app launch): coverage/monotonicity/
  sanity via `line_mapping` joins (invariants 1, 2, 4), meta present +
  `validated=1`, `db_fingerprint` matches a freshly computed digest, and
  reports `layout_fingerprint` with generation date.
- Output: per-work PASS/STALE/FAIL table + exact failing rows. STALE (fingerprint
  mismatch after a re-import or font change) is the expected signal to delete
  the rows and let the app regenerate on next load; the skill prints that
  command but never writes.
- Frontmatter description: "Use when auditing lit.db play_pages tables after a
  litdb re-import, font/layout change, or suspected pagination drift…"

## Testing

- Unit: table load/resolve, fingerprint composition, interval binary search.
- The invariant suite itself is pure (heights + ids in, verdict out) — unit
  tested.
- e2e: existing nav-fuzz must pass with the table ACTIVE (MND + Ham) and with
  it absent (fallback path, e.g. `LIT_NO_PAGE_TABLE=1` env for tests);
  `line_clipping` e2e unchanged. Fuzz gains one assertion: `PAGES: page N/M`
  monotone ±1 on x/y.

## Risks

- **Stale tables after litdb re-import** — gated by `db_fingerprint`; the
  skill audits; app regenerates lazily.
- **Silent divergence between table and live fallback** — both produce the
  same spreads by construction (the generator IS the live engine); the
  determinism invariant catches walk nondeterminism at generation time.
- **Multi-monitor / resolution change** — fingerprint mismatch → fallback;
  regeneration only happens at the pinned resolution, so a session on another
  display simply uses the live engine.
- **lit.db write contention** (litdb tooling open concurrently) — single
  short transaction; on `SQLITE_BUSY`, skip and retry next load.

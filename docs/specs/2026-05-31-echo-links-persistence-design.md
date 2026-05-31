# Persist Turn→Echo Relationships

**Date:** 2026-05-31

## Problem

Pressing `I` on a line embeds the cursor turn and runs a semantic search every
time — a Voyage API call plus a full scan of `passage_embeddings`. Results are
not stored, so:

- Repeated presses on the same line repeat the cost.
- There is no way to keep a hand-picked ("curated") echo for a line.

## Solution

Store turn→echo relationships in lit.db. Two purposes:

1. **Cache:** the search result for a turn is persisted; pressing `I` on a
   cached turn shows it instantly with no API call.
2. **Curate:** within a turn's cached echoes, mark some as curated (★). Curated
   echoes sort first and are never discarded on refresh — even if they fall
   outside the top 15 of a later search.

## Data Model

Two new tables, created at startup via `ensure_echo_tables` (mirroring
`ensure_bookmarks_table`).

```sql
-- One row per turn that has been searched (the cache key).
CREATE TABLE IF NOT EXISTS echo_turns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev TEXT NOT NULL,
    div1 INTEGER,
    div2 INTEGER,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    speaker TEXT,
    turn_text TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(work_abbrev, div1, div2, start_line, end_line)
);

-- The echoes found for a turn (cached results + curated marks).
CREATE TABLE IF NOT EXISTS echo_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_id INTEGER NOT NULL REFERENCES echo_turns(id) ON DELETE CASCADE,
    echo_work_abbrev TEXT NOT NULL,
    echo_div1 INTEGER,
    echo_div2 INTEGER,
    echo_text TEXT NOT NULL,        -- displayed sentence (verse-formatted, with \n)
    similarity REAL,
    curated INTEGER NOT NULL DEFAULT 0,
    rank INTEGER NOT NULL,          -- display order from the search (0-based)
    UNIQUE(turn_id, echo_work_abbrev, echo_div1, echo_div2, echo_text)
);
CREATE INDEX IF NOT EXISTS idx_echo_links_turn ON echo_links(turn_id);
```

The turn is keyed by `(work_abbrev, div1, div2, start_line, end_line)` — the same
shape as `passage_embeddings`. Pressing `I` anywhere in the same contiguous
speaker block resolves to the same turn key.

## Flow on `I`

1. Resolve the cursor turn → `(work, div1, div2, start_line, end_line)` plus
   speaker and turn text.
2. **Cache lookup:** `find_echo_turn(key)`. If found:
   - `load_echo_links(turn_id)` → render immediately, no API call.
   - Sort: curated first (by rank), then non-curated by rank.
3. **Cache miss:** run embed + `find_similar_passages` as today (over-fetch 60,
   dedup by first sentence, sort by work, cap 15). Then **persist**:
   - Insert `echo_turns` row, get `turn_id`.
   - Insert one `echo_links` row per echo (`rank` = display index, `similarity`,
     `curated` = 0, `echo_text` = the verse-formatted first sentence).
   - Render.

## Curate Toggle (`s`)

In the echoes overlay, `s` toggles the `curated` flag on the selected echo's
`echo_links` row (`UPDATE echo_links SET curated = 1 - curated WHERE id = ?`),
writes immediately, and re-renders. A **★** prefix marks curated echoes; they
sort first.

## Refresh (`R`)

`R` re-runs the embed + search for the current turn and overwrites the cache,
**always keeping curated links**:

1. Run the fresh search → new ranked list (top 15).
2. Delete all non-curated `echo_links` for this `turn_id`.
3. Insert the fresh results that are not already present as curated.
4. Curated links are untouched — they remain even if outside the new top 15.
5. Re-render (curated first, then fresh by rank).

## Keys in the Echoes Overlay (updated)

- **Ctrl+n / Ctrl+p** — move selection
- **Enter** — copy selected echo (with citation) to clipboard
- **s** — toggle curated on the selected echo
- **R** — refresh (re-search, overwrite cache, keep curated)
- **j / k / g / G** — scroll
- **Esc** — close

## Rendering

The ★ marker prefixes the bracketed quote of curated echoes:
`<gloss>[★ "echo text" — Work act.scene]</gloss>`. The existing echo render
(`split_echo` + quote/citation tags) stays; the ★ is just part of the quote
prefix. Curated echoes appear first in the list.

## New Code

- `src/db/queries.rs`:
  - `ensure_echo_tables(conn)` — create both tables
  - `EchoTurnKey` struct + `find_echo_turn`, `save_echo_turn`
  - `StoredEchoLink` struct + `load_echo_links`, `insert_echo_links`,
    `toggle_echo_curated`, `delete_noncurated_echo_links`
- `src/app.rs`: call `ensure_echo_tables` at startup (next to bookmarks)
- `src/input/actions/echoes.rs`:
  - cache lookup before search; persist after search
  - `move_echo_selection` unchanged
  - `toggle_curated` (s) and `refresh_echoes` (R) handlers
  - track `turn_id` in AppState so curate/refresh know which turn
- `src/input/keymap.rs`: add `s` and `R` to `handle_echoes_overlay_key`
- `src/ui/gloss_overlay.rs`: nothing structural — the ★ is in the gloss text

## State

New `AppState` fields:
- `echo_overlay_turn_id: Option<i64>` — the cached turn row id (for curate/refresh)
- replace `echo_overlay_candidates: Vec<EchoCandidate>` with
  `echo_overlay_links: Vec<StoredEchoLink>`, where `StoredEchoLink` carries
  `link_id: i64`, `echo_work_abbrev`, `echo_div1`, `echo_div2`, `echo_text`,
  `curated: bool`. The render and copy paths read from `StoredEchoLink`. This
  unifies the cache-hit and cache-miss paths (a miss builds `StoredEchoLink`s
  after persisting and reading back the inserted rows).

## Reuse

- `voyage::embed_query`, `find_similar_passages`, `load_work_titles`
- the existing dedup/sort/first-sentence logic in `echoes.rs`
- the gloss overlay echo rendering

## Out of Scope

- No cross-device sync of curated echoes (lit.db is local)
- No bulk curate / export
- No editing of echo text

## Risks

- **Stale cache after corpus rebuild:** if `passage_embeddings` is rebuilt
  (new model), cached `echo_links` may reference text that changed. Mitigated by
  the `R` refresh key; curated links persist regardless. Acceptable.
- **Turn boundary drift:** if the cleaned text changes line numbers, the turn
  key shifts and the old cache row is orphaned (a fresh search runs). Harmless.
- **Curated echo no longer in corpus:** a curated link's `echo_text` is stored
  verbatim, so it displays even if the underlying passage is gone. Acceptable —
  it is a saved reference.

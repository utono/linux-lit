# Journal Auto-Tagging (linux-lit reader) — Design

> Design/spec. Next step is `superpowers:writing-plans` → an implementation
> plan under `docs/superpowers/plans/`. Canonical location per repo convention is
> `docs/superpowers/plans/` (this file), NOT `docs/superpowers/specs/`.

## Goal

When the user creates, rewrites, or manually edits a journal Q&A entry in the
linux-lit reader, automatically extract the entry's terms of art and write them
to `journal_tags` — so the `f` term-browse suggestion list and the tags-first
match path stay current without a separate `/litdb:tag-journal` batch run.

Today the reader is read-only on `journal_tags`; tags come only from litdb's
batch tagger. A new entry is findable via the FTS5 fallback immediately (the
FTS triggers auto-sync) but never shows up as a suggested tag and never takes
tags-first precedence until the batch tagger is re-run. This design closes that
gap for reader-created/edited entries.

## Non-goals

- No change to the batch tagger (`~/utono/litdb/scripts/tag_journal.py`) or its
  prompt. The reader REUSES the same `api_prompts` prompt row.
- No change to the FTS5 index or its triggers (already auto-synced).
- No new suggestion caching / invalidation (`find_distinct_terms` re-queries on
  every `f` open, so a new tag appears on the next open with no restart).
- No retro-tagging of existing untagged entries (that stays the batch tagger's
  job).

## Architecture & data flow

A **fire-and-forget background tagger**. The entry save/rewrite/edit UX is
UNCHANGED and synchronous. After the entry is committed, the reader spawns an
async task that:

1. Loads the active extraction prompt via
   `crate::db::prompts::active_prompt("journal.extract-terms")` — the SAME
   `api_prompts` row litdb's batch tagger uses (single source of truth for the
   extraction instructions, so reader-tagged and batch-tagged output match).
2. Calls `crate::claude::send_message(system=prompt, user=<question + answer>,
   model=<tag_extract_model>)` — one synchronous call (seconds), on a small
   model (default Haiku) since term extraction is cheap classification.
3. Parses `{"terms":[...]}` → lowercase, trim, dedupe (order-preserving), cap at
   8 — mirroring `tag_journal.py::parse_terms_result`.
4. Upserts into `journal_tags` with `source='reader-auto'` under a
   **replace-auto-keep-manual** transaction (see below).

A failure (API error, malformed JSON, DB error) leaves the entry with whatever
tags it already had — identical to today's behavior — and is recoverable via
`/litdb:tag-journal`. The background task never blocks or delays the reader.

## Re-tag policy (replace-auto-keep-manual)

On every (re)tag the upsert runs, in one transaction:

```sql
DELETE FROM journal_tags
  WHERE entry_id = ?1 AND source IN ('backfill', 'reader-auto');
-- then, per extracted term:
INSERT OR IGNORE INTO journal_tags (entry_id, term, source)
  VALUES (?1, ?2, 'reader-auto');
```

- `source = 'reader-auto'` marks reader-generated tags.
- The DELETE also clears `source = 'backfill'` rows so a reader edit supersedes
  the batch tagger's output for that entry (they use the same prompt, so this is
  consistent, not lossy).
- Any tag with a different source (e.g. `source = 'manual'` from a hand SQL
  insert) is NEVER deleted — manual curation survives a re-tag.

### Source taxonomy

| source        | written by                    | deleted on reader re-tag? |
|---------------|-------------------------------|---------------------------|
| `backfill`    | litdb batch tagger            | yes                       |
| `reader-auto` | this feature                  | yes                       |
| `manual`      | hand SQL / any other          | no (preserved)            |

No litdb migration and no rewrite of existing rows is required.

## Trigger events

| Event                    | Handler                      | Action                              |
|--------------------------|------------------------------|-------------------------------------|
| New entry (ask card)     | `begin_ask` save completion  | `spawn_retag` on the new entry_id   |
| Answer rewrite (`R`)     | `begin_rewrite` completion   | `spawn_retag` (re-extract, replace) |
| Manual edit (`e`, vim)   | `begin_edit` save            | `spawn_retag` (re-extract, replace) |
| Delete (`D`)             | (none)                       | `ON DELETE CASCADE` removes tags    |

**Delete prerequisite:** the cascade fires only if `PRAGMA foreign_keys = ON`
on the reader's connection. The plan MUST verify `open_db()` sets it and add it
if missing (the correct fix regardless — a latent gap flagged in the
term-browse review).

## Components & interfaces

Four focused units.

### 1. `src/journal_tags.rs` (new — pure, unit-tested)

```rust
/// Parse the extractor's {"terms":[...]} response into a clean term list:
/// lowercase, trim, dedupe (order-preserving), cap at 8. Tolerant — returns
/// an empty Vec on missing "terms" key or a non-list value. Mirrors
/// litdb tag_journal.py::parse_terms_result.
pub fn parse_terms(raw: &str) -> Vec<String>;
```

### 2. `src/db/journal.rs` (extend)

```rust
/// Replace this entry's auto-generated tags (source in backfill/reader-auto)
/// with `terms` (source='reader-auto'), in one transaction. Preserves tags
/// with any other source. An empty `terms` clears the auto tags.
pub fn replace_auto_tags(
    conn: &Connection, entry_id: i64, terms: &[String],
) -> Result<(), rusqlite::Error>;
```

The save fns (`save_journal_page` / `save_passage_page` / …) must return the
committed `entry_id` (`conn.last_insert_rowid()` or the reused-row id). The plan
confirms whether they already do or need a small signature change.

### 3. `src/input/actions/journal.rs` (extend — GTK glue, verified e2e)

```rust
/// Fire-and-forget: if config.auto_tag_journal, spawn an async task that loads
/// the extract-terms prompt, calls Claude (tag_extract_model), parses via
/// journal_tags::parse_terms, and writes via db::journal::replace_auto_tags.
/// No-op when the config toggle is off. Text is captured by value at spawn.
fn spawn_retag(state: &Rc<RefCell<AppState>>, entry_id: i64,
               question: String, answer: String);
```

Uses the reader's existing async Claude bridge / spawn pattern
(`src/input/actions/claude_bridge.rs`).

### 4. Call sites

`begin_ask`, `begin_rewrite`, `begin_edit` each call `spawn_retag(...)` after
the entry is committed, passing the just-saved `entry_id` and the final
question/answer text.

## Config

- New config field `auto_tag_journal: bool`, default `true`. `spawn_retag`
  no-ops when false — the escape hatch to disable the API calls (batch-only via
  `/litdb:tag-journal`).
- New config field `tag_extract_model: String`, default
  `claude-haiku-4-5-20251001`. Changeable without a rebuild, per the reader's
  dev/release config split (edit `config-dev.json` while no dev instance runs).

## Concurrency & edge cases

- **Rapid re-edits of one entry:** two taggers may race on the same `entry_id`.
  Each does DELETE-auto + INSERT in one transaction, so last-writer-wins —
  correct, since each captured its own text at spawn. No lock needed.
- **Empty extraction (`{"terms":[]}`):** the transaction still runs the DELETE,
  so an entry whose terms were all removed ends with no auto tags. Intended.
- **API failure vs. "no terms":** distinguish a call ERROR from a successful
  `{"terms":[]}`. On a call error, SKIP the write entirely (leave existing tags
  untouched). Only a successful response triggers the replace — a transient
  failure never wipes good tags.
- **lit.db write contention:** the tagger opens its own short-lived write
  connection for the tiny upsert; SQLite serializes it. The multi-instance
  "avoid concurrent lit.db writers" note applies but the write is small.

## Testing

- **Unit (`cargo test`):**
  - `parse_terms`: valid list; `{"terms":[]}`; missing "terms" key; non-list
    value; >8 cap; dedupe + lowercase + trim.
  - `replace_auto_tags`: replaces `backfill` and `reader-auto` rows; PRESERVES a
    `manual` row; empty `terms` clears the auto rows only.
- **Integration seam:** the save fns return the real `entry_id`.
- **Headless e2e:** create a Q&A whose answer names a term of art → wait for the
  background call → press `f`, confirm the new term appears in the suggestion
  list and the entry is found tags-first. Then delete an entry and confirm its
  tags are gone (validates `PRAGMA foreign_keys` / the cascade).
- **Cost note:** this makes REAL Claude API calls at save time (small, Haiku).
  The e2e incurs token cost; the user runs the live eyeball.

## Files

- Create: `src/journal_tags.rs` (pure `parse_terms`) + register in the module
  tree (`src/main.rs`/`lib` mod list).
- Modify: `src/db/journal.rs` (`replace_auto_tags`; save fns return `entry_id`).
- Modify: `src/input/actions/journal.rs` (`spawn_retag` + calls in
  `begin_ask`/`begin_rewrite`/`begin_edit`).
- Modify: `src/config.rs` (`auto_tag_journal`, `tag_extract_model` defaults).
- Verify/modify: `src/db/queries.rs` `open_db()` sets `PRAGMA foreign_keys=ON`.

## Open questions for the plan phase

- Exact current signatures of the save fns (do they already return `entry_id`?).
- The reader's canonical async-spawn idiom for a background Claude call that
  writes lit.db (mirror `claude_bridge.rs`; confirm no `AppState` borrow is held
  across the await).

# Data-driven per-work narrator voice

## Problem

`resolve_default_voice` picks the ElevenLabs voice for a block of narration.
For **prose** (gloss / explication) it currently short-circuits to a single
narrator with one hardcoded exception:

```rust
if !is_verse {
    let voice = if work_abbrev.starts_with("BCP") {
        DEFAULT_MALE_VOICE_ID          // Book of Common Prayer
    } else {
        DEFAULT_FEMALE_VOICE_ID        // Eleanor — everything else
    };
    return (voice.to_string(), OP_MODEL_ID.to_string());
}
```

Two problems:

1. **The author/work → voice mapping is hardcoded.** Adding a new author group
   (the library has 24 authors — Shakespeare, BCP, Dickens, Ibsen, KJV, …) means
   editing Rust, exactly the `starts_with("BCP")` smell this replaces.
2. **The global default is wrong going forward.** Eleanor should be the narrator
   for **Shakespeare**, not for everything. Every other author should default to
   the male narrator (Benedick) unless overridden.

## Goal

Make the prose narrator data-driven: resolvable per-work, falling back per-author,
falling back to a global default. Adding or changing an author's narrator — or a
single work's — becomes a lit.db edit, not a code change. **Verse resolution is
untouched.**

## Scope

- **In scope:** the prose/gloss branch of `resolve_default_voice`; a new
  `works.default_voice_id` column; a new `author_default_voice` table; seeding
  Shakespeare → Eleanor; tests; the docs guide note.
- **Out of scope:** verse resolution (keeps the existing (gender, age) catalog
  containment/nearest-band logic — UNKNOWN→male, named characters by
  gender/age); the voice picker UI; per-gloss associated-voice overrides (those
  already override at the `play_block_tts` call site and are unaffected).

## Schema changes (lit.db)

lit.db is the live, gitignored store. **Snapshot before migrating**
(`\cp -f ~/utono/litdb/data/lit.db /tmp/lit.db.bak-narrator-voice`) — this is the
only undo; there is no git history for the data.

1. **`works.default_voice_id TEXT` (nullable, new column).** Per-work override.
   NULL = "no per-work override; use the author default." `ALTER TABLE works ADD
   COLUMN default_voice_id TEXT` — guarded against re-run (SQLite `ADD COLUMN`
   has no `IF NOT EXISTS`; check `PRAGMA table_info(works)` for the column first,
   or catch the duplicate-column error).

2. **New table `author_default_voice`:**

   ```sql
   CREATE TABLE IF NOT EXISTS author_default_voice (
       author   TEXT PRIMARY KEY,
       voice_id TEXT NOT NULL
   );
   ```

3. **Seed (idempotent):**

   ```sql
   INSERT OR IGNORE INTO author_default_voice (author, voice_id)
   VALUES ('Shakespeare', '<DEFAULT_FEMALE_VOICE_ID = D4LX5VBnEN6zrrsnTMO8>');
   ```

   The Rust seed references `DEFAULT_FEMALE_VOICE_ID` (not the literal string) so
   it tracks future voice swaps. The `author` value must match `works.author`
   exactly ("Shakespeare").

Schema creation + seeding follow the existing `ensure_voice_catalog_table`
pattern (idempotent `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE`) and run
from the same migration entry point.

## Resolution

### Prose path — new precedence (first non-null wins)

A new helper owns the decision:

```rust
/// The narrator voice_id for PROSE/gloss of `work_abbrev`:
/// per-work override → per-author default → global male default.
/// Always resolves (a query error logs and falls through).
fn resolve_prose_voice(conn: &Connection, work_abbrev: &str) -> String
```

1. `works.default_voice_id` for `work_abbrev`, if non-NULL → use it.
2. else `author_default_voice.voice_id` joined on `works.author` for that work,
   if present → use it.
3. else **global default: `DEFAULT_MALE_VOICE_ID` (Benedick)**.

Each lookup is an `.optional()` query; a DB error logs (matching the existing
containment/nearest defensive pattern) and falls through to the next step, so a
failure degrades to the global default rather than panicking. The narration
model is always `OP_MODEL_ID`.

The call site collapses to:

```rust
if !is_verse {
    return (resolve_prose_voice(conn, work_abbrev), OP_MODEL_ID.to_string());
}
```

The `starts_with("BCP")` hardcode is **deleted**. BCP now resolves to Benedick
via step 3 (no author row) — identical behavior, but as data.

Net effect with only the seed row present:

- Shakespeare prose → Eleanor (step 2, author row)
- BCP / Dickens / Ibsen / KJV / all other authors prose → Benedick (step 3)
- any work with `default_voice_id` set → that voice (step 1), overriding both

### Verse path — unchanged

The `(gender, age)` catalog resolution (containment band → nearest same-gender
band → `voice_for` last resort) is untouched. The new column and table are
consulted **only** in the prose branch. UNKNOWN verse → male; named characters
by gender/age.

## Components

- `resolve_prose_voice(conn, work_abbrev) -> String` — new private helper in
  `src/db/queries.rs`. One purpose, depends only on `conn` + abbrev.
- `resolve_default_voice` — its prose branch delegates to the helper; verse
  branch unchanged.
- Schema/seed — extend the existing voice-catalog migration function (or a
  sibling alongside it) to add the column, create the table, and seed the
  Shakespeare row.

## Testing

In-memory DB, extending the existing `seed_catalog_and_chars` helper to also
create `author_default_voice` + the `default_voice_id` column and seed the
Shakespeare row (and a couple of `works` rows with known authors).

- **Shakespeare prose → Eleanor** (step 2, author row).
- **BCP prose → Benedick** (step 3, no author row — preserves current behavior).
- **Non-BCP non-Shakespeare prose → Benedick** (e.g. a Dickens work) — the
  flipped global default.
- **Per-work override wins** — set `works.default_voice_id` on a work (including
  a Shakespeare work) and assert it beats the author default (step 1).
- **Verse unchanged** — UNKNOWN verse → male; named character (Juliet) verse →
  female. Proves the new path doesn't leak into verse.

The existing `resolve_prose_narrator_eleanor_except_bcp` test is **replaced** by
these — its "BCP exception" framing is obsolete.

## Docs

Update the prose-narrator note in
`docs/guides/elevenlabs-v3-custom-voices.md` to describe the data-driven
precedence (per-work → per-author → global Benedick; Shakespeare→Eleanor seeded)
instead of the BCP exception.

## Migration / rollout

1. Snapshot lit.db.
2. Run the schema migration + seed (idempotent; safe to re-run).
3. Manually re-verify affected rows (cannot `git diff` the DB):
   - `PRAGMA table_info(works)` shows `default_voice_id`.
   - `SELECT * FROM author_default_voice;` shows the Shakespeare→Eleanor row.
4. `cargo build && cargo test --bin linux-lit resolve` green.
5. Spot-check in-app: a Shakespeare gloss reads Eleanor; a BCP gloss reads
   Benedick.

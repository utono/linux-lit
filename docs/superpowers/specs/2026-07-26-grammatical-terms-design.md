# grammatical_terms — lit.db supplies syntax-gloss definitions

## Purpose

Move grammatical term definitions out of every API response and into lit.db.
Claude returns what is passage-specific; the definitions come from a table.

## The problem

Every syntax-gloss asks Claude to define the same terms from scratch. Three
costs, in rising order of seriousness:

- **Tokens and latency.** "main clause" is redefined on every gloss.
- **Drift.** Two glosses can disagree about what a relative clause is, because
  nothing makes them agree.
- **Gaps.** A term used only in the rhetorical note gets no definition at all.
  Reported 2026-07-26: a note read "an appositive, and inside that appositive a
  relative clause" while the Terms section defined only the labels that
  happened to appear in the Structure list. The prompt was widened to cover
  the note (`f5aaf5ec`), but that treats the symptom — the model is still
  re-deriving stable facts about English on every call.

Definitions of grammatical terms do not vary by passage. They are reference
data, and lit.db is where this project keeps reference data.

## The table

New in lit.db, mirroring the existing `rhetorical_terms` shape:

```sql
CREATE TABLE grammatical_terms (
    id         INTEGER PRIMARY KEY,
    term       TEXT UNIQUE NOT NULL,
    definition TEXT NOT NULL,
    source     TEXT,                 -- 'claude' | 'curated'
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE INDEX idx_grammatical_terms_term ON grammatical_terms(term);
```

**Separate from `rhetorical_terms`, deliberately.** That table holds 19
rhetorical FIGURES — anaphora, chiasmus, zeugma, litotes. These are
grammatical STRUCTURES — main clause, subject, predicate, relative clause. The
two sets overlap at exactly one entry, "appositive". Filing "predicate" among
rhetorical figures would be a category error that misleads whoever reads the
table next, and the cost of a second table is one migration.

`source` distinguishes machine-written definitions from curated ones, so a
later audit can find what Claude wrote.

**Seed list**, harvested from the syntax glosses already in lit.db rather than
invented: adverbial clause, adverbial phrase, appositive, main clause,
participial modifier, predicate, relative clause, subject. Plus the remaining
terms the prompt names: conjoined predicate.

## What the API returns

The prompt drops the `Terms:` section entirely. Claude returns three things,
all passage-specific and none derivable from a table:

1. The passage, in a `<segment>` pair.
2. `Structure:` — one line per span.
3. `What the structure is doing:` — the rhetorical note.

It gains one obligation. The user message carries the list of terms lit.db
already knows; Claude appends a `New terms:` section defining ONLY the
grammatical terms it used that were not in that list. Usually empty.

This is what makes the table self-growing: the first gloss to use
"periodic sentence" supplies its definition, and every gloss after that reads
the stored one — which is the consistency the change is for.

**Only the term NAMES are sent, not their definitions.** The list is what
Claude needs to know which terms are new; sending definitions back would
reintroduce the tokens this change removes. At the seed size (9 terms) the
list is a few dozen tokens, and it grows only as the corpus meets genuinely
new grammatical vocabulary — a bounded set, unlike the per-gloss definitions
it replaces.

## Assembly at save

The API reply currently goes to `persist_render_install_gloss` verbatim. A new
step sits between:

1. Scan the note and Structure for terms present in `grammatical_terms`.
2. Insert any `New terms:` entries (`source='claude'`).
3. Append a `Terms:` section built FROM THE TABLE — alphabetical, one
   `<gloss>term: definition.</gloss>` pair each, exactly the shape the renderer
   already handles.

**Baked into `gloss_text` at save, not joined at display.** The stored row
stays self-contained, so export, search, and TTS keep working without each
learning about a join. The accepted cost, stated plainly: editing a definition
in lit.db later does NOT update glosses already saved.

## Boundaries

- `src/db/grammatical_terms.rs` (new) — `load_all`, `insert_missing`. DB only.
- The term scan and the Terms builder are pure functions over `&str` and a
  term list: no DB, no GTK, unit-testable without a display. They are the part
  most likely to be subtly wrong, so they carry the tests.
- `src/gloss.rs` — the prompt loses section 4, gains the `New terms:`
  instruction.
- `src/input/visual.rs` — the assembly step, at the seam before
  `persist_render_install_gloss`.
- litdb: one migration, following the `add_vocab_rhetoric.sql` convention.

## Error handling

- **`grammatical_terms` unreadable.** Fall back to the current behavior: no
  Terms section appended, gloss still saves. A definitions table being down
  must not cost the reader their analysis.
- **A term in the note is not in the table and Claude sent no definition for
  it.** No entry; log it. This is the honest failure — better a missing
  definition than a fabricated one at save time.
- **A `New terms:` entry duplicates an existing row.** `INSERT OR IGNORE`; the
  stored definition wins. Consistency beats recency.

## Testing

**Unit (`cargo test --bins`, no display):**

- The scan finds multi-word terms — "main clause", not a bare "clause" inside
  it.
- A term used three times yields one glossary entry.
- Entries sort alphabetically.
- An empty term list yields no `Terms:` section at all, not an empty heading.
- Parsing a `New terms:` section, including the common empty case.

**On screen, the real GL renderer** (cage disagreed with GL on every layout
defect this feature hit as a drawing):

- A fresh gloss shows a definition for every grammatical term in its note.
- Generating a second gloss that reuses a term inserts nothing new — verified
  by row count before and after.

## Non-goals

- **No change to `rhetorical_terms`** or to the other five gloss types.
- **No retroactive rewrite** of existing syntax glosses. They keep the
  definitions baked in when they were saved.
- **No cross-linking to `vocab_words`.** Making grammatical terms clickable in
  the reader is a separate, larger change.
- **No curation UI.** Editing a definition is `sqlite3` for now.

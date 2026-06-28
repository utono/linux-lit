# Work-type-aware journal Q&A prompt — design

_2026-06-28 (US Central)_

## Problem

The journal Q&A prompt hard-codes play/scene vocabulary. The **active** prompt is
the lit.db `api_prompts` row `journal.qa` **v3** (the `src/gloss.rs` FALLBACK is
dormant — the DB row wins whenever the DB is reachable). Its first two paragraphs
read:

> "…a reader who is working through **a play**, one **scene** at a time… situate
> the **scene** within the **whole play**… echoes earlier **scenes**…"

For a prose work such as *Bleak House* (`work_type = 'prose'`, Dickens) the model
spends its opening paragraph correcting the premise — "Bleak House is a novel, not
a play, though your instinct to read it scene by scene…". The answer should
instead assume the reader already knows the work's type and speak in that genre's
idiom.

The `user_msg` assembled in `ask_claude` is **also** play-specific:
`"Reader's question about the play as a whole"`, `"Scene: <label>"`.

## Goal

Make the journal Q&A prompt **work-type aware** so a novel is discussed as a
novel (chapters), an epic as an epic (books), etc., with no genre correction in
the answer. One parameterized prompt, genre vocabulary injected from the work's
`work_type` at request time. (Decision: one parameterized prompt over per-type
variants or a fully genre-neutral prompt.)

## Non-goals

- No change to gloss/synopsis/inner-monologue prompts.
- No hot-reload of DB prompts (unchanged: applies on next launch).
- No new `work_type` values or DB-schema change to `works`.

## Background facts (verified)

- `Work` and `WorkSummary` already carry `work_type: String` (from
  `works.work_type`); it is available at both journal Q&A call sites via
  `s.current_work`.
- Distinct `work_type` values in lit.db (count): `play`(89), `bible_book`(71),
  `prose`(20), `prose_book`(5), `epic`(4), `narrative_poem`(2), `poem`(2),
  `epic_translation`(2), `verse_essay`(1), `sonnet_sequence`(1),
  `essay_collection`(1), `anthology`(1).
- `src/db/line_types.rs::PROSE_TYPES = ["novel","essay_collection","prose_book",
  "prose"]` (note: lit.db uses `prose`, not `novel`, for the Dickens works).
- Prompts are managed by the `~/utono/claude-api-prompts` repo: editable masters
  in `prompts/<key>.md`, synced to lit.db `api_prompts` by `scripts/sync-to-db.py`
  (each sync bumps the version, demotes the prior active row). **There is no
  `prompts/journal.qa.md` master today** — the DB v1–v3 were synced without a
  tracked master. The repo's established placeholder convention is `{token}`
  substituted at assembly (e.g. `{ipa_rules}`).
- `template_or("journal.qa", FALLBACK)` reads the active DB row, else the
  compiled FALLBACK. `JOURNAL_QA_PROMPT` is currently a `LazyLock<String>`
  resolved once per process.

## Design

### 1. Genre/unit lookup (linux-lit)

A pure function mapping a `work_type` to its genre noun and unit nouns:

```rust
/// (genre, unit, units_plural) for a work_type. Unknown -> generic.
pub fn genre_unit(work_type: &str) -> (&'static str, &'static str, &'static str)
```

| work_type | genre | unit | units |
|---|---|---|---|
| play | play | scene | scenes |
| prose, prose_book | novel | chapter | chapters |
| bible_book | book | chapter | chapters |
| epic, epic_translation | epic poem | book | books |
| narrative_poem | narrative poem | section | sections |
| poem | poem | section | sections |
| sonnet_sequence | sequence | sonnet | sonnets |
| verse_essay | essay | section | sections |
| essay_collection | collection | essay | essays |
| anthology | anthology | selection | selections |
| _(unknown)_ | work | section | sections |

Location: `src/gloss.rs` (next to the prompt it serves) as
`pub fn genre_unit(...)`. Exhaustively unit-tested: one assertion per known type
plus the unknown-fallback case.

### 2. `JOURNAL_QA_PROMPT` becomes per-request

Replace the static `LazyLock<String>` with:

```rust
pub fn journal_qa_prompt(work_type: &str) -> String
```

It reads `template_or("journal.qa", FALLBACK)`, then substitutes
`{genre}` / `{unit}` / `{units}` from `genre_unit(work_type)`. The `src/gloss.rs`
FALLBACK is rewritten to use those placeholders so the dormant path is also
genre-correct. Substitution is plain `str::replace` (matches the existing
`{ipa_rules}` style; tokens that don't appear are simply no-ops).

Both call sites pass the work type:
- `src/input/actions/journal.rs::ask_claude` (was line 509)
- `src/input/actions/journal.rs::submit_edit_rewrite` (was line 629)

Each already borrows `s.current_work`; capture `work_type` there.

### 3. Parameterize the user message

In `ask_claude`'s `user_msg` builder and in the rewrite path's user message:
- Work band: `"…question about the {genre} as a whole:"`.
- Scene band: field `"Scene:"` → `"{Unit_titlecased}:"` (e.g. `Chapter:`) and
  `"Scene text:"` → `"{Unit_titlecased} text:"`.
- Passage band: the same two relabels (`Scene:` and `Scene text:`); the
  `"Passage:"` field name is unchanged (a passage is a passage in any genre).
- Prepend a `"Work type: {genre}\n"` line so the model has the ground truth even
  if a future DB template drops a token.

`scene_label(d1,d2)` keeps producing the numeric label; only the field name
changes. `{Unit_titlecased}` is the unit noun with its first letter uppercased
(`chapter` → `Chapter`); a tiny inline helper, not a new dependency.

### 4. DB side (claude-api-prompts repo)

1. Create `prompts/journal.qa.md` seeded from the current DB v3 text, edited to
   use `{genre}` / `{unit}` / `{units}` in place of play/scene/scenes/whole play.
2. Commit the master (the commit subject becomes the DB version note).
3. `scripts/sync-to-db.py journal.qa` → writes **v4 active**, demotes v3.
   linux-lit must be **closed** during the sync; the change applies on the next
   launch.

The linux-lit FALLBACK and the DB v4 master are kept textually identical (the
repo's normal invariant for seed text).

## Data flow

```
ask_claude / submit_edit_rewrite
  -> work_type = s.current_work.work_type
  -> system = journal_qa_prompt(work_type)
       = template_or("journal.qa", FALLBACK)
         .replace("{genre}", g).replace("{unit}", u).replace("{units}", us)
  -> user_msg built with {genre}/{Unit} + "Work type: {genre}"
  -> run_claude_request(system, user_msg, model, ...)
```

## Error handling / edge cases

- Empty / unknown `work_type` → generic `(work, section, sections)`; never panics.
- DB unreachable → FALLBACK (already genre-parameterized) is used.
- A future DB template that omits a token → that token simply isn't substituted;
  the `"Work type:"` user-message line still informs the model.
- `prose` maps to "novel" even though `PROSE_TYPES` lists `prose` — `genre_unit`
  is independent of `is_prose_work` and is the single source for genre nouns.

## Testing

- **Unit**: `genre_unit` — one assertion per work_type + unknown fallback.
- **Unit**: `journal_qa_prompt("prose")` contains "novel" and "chapter" and does
  NOT contain "play"/"scene" (guards both the FALLBACK text and the substitution).
- **Build + `cargo test --bins`** green.
- **Visual acceptance** (user, after DB v4 sync + restart): a whole-work Q&A on
  *Bleak House* opens without a "not a play" correction and refers to chapters.

## Files

- `src/gloss.rs` — `genre_unit`, `journal_qa_prompt`, rewritten FALLBACK, tests.
- `src/input/actions/journal.rs` — both call sites + parameterized user messages.
- `~/utono/claude-api-prompts/prompts/journal.qa.md` — new master (then synced to
  DB v4).

## Rollback

- linux-lit: revert the commit; `JOURNAL_QA_PROMPT` returns to the static form.
- DB: `restore-version.py journal.qa 3` re-activates v3.

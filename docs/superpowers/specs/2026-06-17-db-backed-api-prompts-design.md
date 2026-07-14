# DB-backed Claude-API prompt management

**Date:** 2026-06-17
**Branch:** `db-backed-api-prompts` (off `master`)
**Repos touched:** linux-lit, litdb, new `~/utono/claude-api-prompts`, shared `lit.db`

## Goal

Move the Claude-API prompts that linux-lit and litdb send (gloss explications,
synopsis generation) out of hardcoded source and into `lit.db`, so every prompt
version is reviewable and any old version is restorable. Then update the gloss
and synopsis prompts to (a) discuss rhetorical devices like anaphora in the
exemplar phrasing of gloss id 21741, and (b) complement the Eleanor narration
voice.

The work ships end-to-end in one pass: build the pipeline, seed the current
prompts as version 1 (no behavior change), then update the phrasing as
version 2 (active) with version 1 left restorable.

## Background

The "prompts used by linux-lit" live in three places today:

- **Gloss prompts** — `LazyLock<String>`/`const` in `src/gloss.rs`:
  `TEACHER_GENERIC_PROMPT`, `USER_QUESTION_PROMPT`, `EDIT_GLOSS_PROMPT`, the
  three `INNER_MONOLOGUE_*` prompts, `FIX_IPA_PROMPT`, plus the shared
  `op_ipa_conventions!` macro and the `IPA_VERSE_RULES` / `IPA_VERSE_RULES_SPARSE`
  fragments selected by the `APPEND_IPA` switch.
- **Synopsis amend prompt** — `SYNOPSIS_AMEND_PROMPT` const in
  `src/input/actions/synopsis.rs`.
- **Batch synopsis prompt** — `SYSTEM_PROMPT` in
  `~/utono/litdb/scripts/improve_synopses.py`.

Gloss id 21741 (`teacher-generic`, the Eleanor temptation speech from 2H6 1.2)
is the exemplar for the desired explication style: it names rhetorical devices
(anaphora, caesura, enjambment, antithesis), defines each inline on first use,
ties them to the operative words and the actor's breath/drive, and references
voice coaches (Rodenburg, Berry). The Eleanor voice-design prompt
(`~/utono/eleven-lit/docs/prompts/eleanor-voice-design.md`) describes a slow,
deliberate, coaxing, seductive-command delivery in the chest register; the
explication prose should cue the reader toward that register.

## Decisions

- **Runtime model:** DB-load with compiled fallback. Each consumer reads the
  active prompt from `lit.db`; on a missing row or DB error it uses its
  compiled-in / in-file fallback. The DB is source of truth; the fallback is a
  safety net so a fresh checkout or empty DB still works.
- **Repo contents:** plaintext masters + sync scripts. Git history is the
  human-readable review/revert log; the DB versions mirror it 1:1.
- **Composition:** keep `{}` placeholders + shared fragments. The OP-IPA
  conventions block and the IPA verse-rules are their own prompt keys; gloss
  templates keep their placeholder and are composed at runtime as today. A
  `render-prompt.py` reconstructs the final assembled string for review.
- **Scope this branch:** all gloss prompts in `gloss.rs`, the linux-lit synopsis
  amend prompt, and the litdb batch synopsis prompt.
- **Sequencing:** one pass — repo + table + seed v1 + wiring, then content v2.

## Data model — `api_prompts` in lit.db

```sql
CREATE TABLE api_prompts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    prompt_key  TEXT NOT NULL,          -- e.g. 'gloss.teacher-generic'
    version     INTEGER NOT NULL,       -- monotonic per prompt_key, from 1
    text        TEXT NOT NULL,          -- master content (may contain {placeholders})
    is_active   INTEGER NOT NULL DEFAULT 0,
    note        TEXT,                   -- one-line "why", from git commit subject
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(prompt_key, version)
);
CREATE INDEX idx_api_prompts_active ON api_prompts(prompt_key, is_active);
```

- Exactly one row per `prompt_key` has `is_active=1`, enforced by the sync/restore
  scripts inside a transaction (no DB trigger).
- **Restore** = set `is_active=1` on an older version AND copy its text back to
  the master file so git and DB stay in sync.

### Prompt keys

- Gloss templates: `gloss.teacher-generic`, `gloss.user-question`, `gloss.edit`,
  `gloss.inner-monologue`, `gloss.inner-monologue-add`,
  `gloss.inner-monologue-edit`, `gloss.fix-ipa`
- Shared fragments: `ipa.op-conventions`, `ipa.verse-rules`,
  `ipa.verse-rules-sparse`
- Synopsis: `synopsis.amend` (linux-lit), `synopsis.batch` (litdb)

`synopsis.amend` and `synopsis.batch` are plain strings (no placeholder). The
gloss templates retain their `{}` slot filled at runtime by the relevant
`ipa.verse-rules*` fragment; that fragment in turn embeds `ipa.op-conventions`.

## Repo: `~/utono/claude-api-prompts`

```
claude-api-prompts/
  README.md
  CLAUDE.md
  prompts/
    gloss.teacher-generic.md
    gloss.user-question.md
    gloss.edit.md
    gloss.inner-monologue.md
    gloss.inner-monologue-add.md
    gloss.inner-monologue-edit.md
    gloss.fix-ipa.md
    ipa.op-conventions.md
    ipa.verse-rules.md
    ipa.verse-rules-sparse.md
    synopsis.amend.md
    synopsis.batch.md
  scripts/
    sync-to-db.py        # upsert one/all masters -> new active version
    restore-version.py   # old DB version -> active + rewrite master file
    list-versions.py     # version history (active marked) per key or all
    render-prompt.py     # assemble template + fragments -> final string
  .claude/skills/
    update-gloss-prompt/SKILL.md
    update-synopsis-prompt/SKILL.md
    restore-prompt-version/SKILL.md
    sync-prompts/SKILL.md
```

- Each `prompts/*.md` master has YAML frontmatter (`prompt_key`, `consumer`,
  `has_placeholders`) plus the raw prompt body.
- `sync-to-db.py`: read master(s), bump version, insert `is_active=1`, demote
  prior active, stamp `note` from the latest git commit subject; transactional;
  `--dry-run`.
- Private GitHub remote `utono/claude-api-prompts` created via
  `gh repo create utono/claude-api-prompts --private --source=. --remote=origin --push`.

## Consumer changes

### linux-lit

- New `src/db/prompts.rs`: `active_prompt(key: &str) -> Option<String>` using the
  existing read-only `open_db()`. Returns `None` on missing row or any error.
- `src/gloss.rs`: each prompt `LazyLock<String>` loads the active template by key
  from the DB, falling back to the existing const (renamed `*_FALLBACK`, kept
  verbatim). The `format!(template, *IPA_VERSE_RULES)` composition stays;
  `template` now comes from the DB when present. The IPA fragments likewise try
  DB-first and fall back to the macro/const. `APPEND_IPA` stays as the fallback
  selector.
- `src/input/actions/synopsis.rs`: `SYNOPSIS_AMEND_PROMPT` becomes a DB lookup of
  `synopsis.amend` with the current const as fallback.

### litdb

- `scripts/improve_synopses.py`: `SYSTEM_PROMPT` reads active `synopsis.batch`
  from `lit.db`, falling back to the in-file literal. Own branch in the litdb
  repo.

## Content update (version 2)

After seeding v1 (verbatim current prompts), update masters and sync as v2:

- `gloss.teacher-generic` — strengthen the rhetorical-device instruction to
  match gloss 21741: name the device (anaphora, caesura, enjambment,
  antithesis), define it inline on first use, tie it to operative words and the
  actor's breath/drive. Add guidance that the explication should complement the
  Eleanor voice — slow, deliberate, coaxing, seductive-command delivery — cueing
  the reader toward that register.
- `synopsis.amend`, `synopsis.batch` — parallel rhetorical-device +
  voice-complement phrasing.

Gloss id 21741 itself is existing data and is left untouched.

## Testing & verification

- `sync-to-db.py` / `restore-version.py`: `--dry-run` + transactional; verify
  round-trip (sync -> list -> restore -> list) against lit.db.
- linux-lit: `cargo build` + `cargo test --bins`. Unit-test `active_prompt`:
  seed a temp DB row, assert returned; drop the row, assert fallback. Do NOT run
  the app — ask the user to `cargo run` to confirm a real gloss renders with the
  new phrasing (visual/runtime acceptance).
- litdb: `improve_synopses.py --dry-run` confirms it reads the DB prompt.

## Branch & repo mechanics

- linux-lit: feature branch `db-backed-api-prompts` off `master` (done).
- new repo `~/utono/claude-api-prompts`: `git init`, first commit,
  `gh repo create utono/claude-api-prompts --private --source=. --remote=origin --push`
  (gh authed as `utono`).
- litdb: own branch off its master.

## Out of scope

- Migrating voice-design markdown prompts (`~/utono/eleven-lit/docs/prompts/`)
  into the DB system.
- A UI inside linux-lit for editing prompts (editing is via the repo skills).
- Other litdb Python prompts beyond the batch synopsis generator.
```

# Journal `R`-rewrite uses the entry's key terms — design

**Date:** 2026-07-13
**Repos:** `linux-lit` (code) + `claude-api-prompts` (prompt master)

## Problem

The journal Q&A overlay's `R` key improves a displayed entry's *question* via
Claude (the `journal.improve-question` prompt). Today that call sends only the
bare question string as the user message — it has no awareness of the terms of
art the entry actually explains (e.g. "fee simple", "quibble"). The rewrite can
therefore blur, rename, or drop the very term that makes the entry findable and
meaningful. Those terms already exist per entry in `journal_tags`.

## Goal

When `R` improves a *displayed* entry's question, feed the entry's saved
`journal_tags` terms into the improve-question prompt as guidance, so the
rewrite preserves and sharpens those terms (keeps canonical phrasing; does not
rename or drop them).

## Scope

- **In scope:** the `R` path only — `rewrite_question_path` (both the
  question-only `q` and the `both` branch route through `improve_question`).
- **Out of scope:** brand-new asks (`submit_prompt` → `improve_question`). A new
  question has no saved entry and no tags yet, so there is nothing to inject.
  That caller passes an empty term slice and behaves exactly as today.
- **No schema change, no migration.** `journal_tags` already exists and is
  populated by the tag-journal / `spawn_retag` flow.

## Design

Two coordinated parts.

### Part 1 — prompt master (`claude-api-prompts`)

Edit `prompts/journal.improve-question.md`:

- Set `has_placeholders: true` in the frontmatter.
- Add a `{terms}` placeholder line in the body. It resolves to a
  **guidance sentence** naming the terms — not a bare list — so the model is
  told *why* they matter. Example resolved text:

  > The reader's question concerns these terms of art: fee simple, quibble.
  > Preserve them verbatim in your rewrite — keep each term's canonical
  > phrasing, and do not rename, gloss away, or drop any of them.

- When the entry has no terms, the consumer substitutes `{terms}` with the
  empty string, so the prompt reads cleanly and is byte-identical in intent to
  today's prompt (no regression). The placeholder therefore sits on its own
  paragraph so an empty substitution leaves no dangling fragment.
- The existing contract is unchanged: preserve intent, do not answer, do not add
  sub-questions, return ONLY the improved question as one plain-text line.

Then, per the repo's prompt workflow:

```bash
cd ~/utono/claude-api-prompts
git commit -am "feat: journal.improve-question — anchor rewrite to entry key terms"
python scripts/sync-to-db.py journal.improve-question
python scripts/render-prompt.py journal.improve-question   # {terms} left unresolved is expected
python scripts/list-versions.py journal.improve-question
```

`render-prompt.py` will show `{terms}` unresolved — correct, exactly like
`journal.qa` leaves `{genre}`/`{unit}` for linux-lit to fill at request time.

### Part 2 — linux-lit

`src/db/journal.rs` — **new helper:**

```rust
/// The distinct terms tagged on a single journal entry, sorted ascending.
/// Complements find_pages_by_term (term→entries); this is entry→terms, for
/// grounding the improve-question rewrite on what the entry explains.
pub fn terms_for_entry(conn: &Connection, entry_id: i64) -> Result<Vec<String>, rusqlite::Error>
```

Query: `SELECT term FROM journal_tags WHERE entry_id = ?1 ORDER BY term ASC`.
Mirrors the neighboring tag queries; unit-testable like them.

`src/input/actions/journal.rs`:

- **`improve_question` gains a `terms: &[String]` parameter.** It builds the
  guidance sentence from the terms (or `""` when empty) and substitutes it into
  the prompt template with `prompt.replace("{terms}", &terms_line)` — matching
  the existing `{ipa_rules}`/`{genre}` substitution pattern in `gloss.rs`. The
  same substitution is applied to `FALLBACK_IMPROVE_QUESTION_PROMPT` so a missing
  DB row behaves identically (symmetric fallback). The user message stays the
  bare question.
- **`FALLBACK_IMPROVE_QUESTION_PROMPT` learns `{terms}`** — the compiled fallback
  gains the same guidance paragraph + placeholder as the master, so a missing
  `api_prompts` row does not silently regress the feature.
- **`rewrite_question_path`** fetches `terms_for_entry(&conn, id)` up-front —
  captured alongside `id`/`old_q`/`answer`/`model` *before* the async
  improve-question round-trip — and passes them to `improve_question`. Fetching
  up-front (not inside the async closure) preserves the existing borrow /
  navigate-safety discipline: a mid-flight navigate cannot cross entries.
- **The new-ask caller** (`submit_prompt` → `improve_question`) passes `&[]`.

## Data flow

`R` → target chooser (`q`/`b`) → `rewrite_question_path`:
read displayed page + `terms_for_entry(id)` →
`improve_question(question, terms)` builds prompt with `{terms}` filled →
Claude returns one-line improved question →
persist improved question (unchanged from today) →
answer regen (`q`) or instruction card (`b`) (unchanged).

## Error / empty handling

- **Entry with zero tags:** `{terms}` → `""`; prompt identical in effect to
  today. No regression.
- **Tag fetch error:** treat as empty terms, log, continue — never block the
  rewrite.
- **Empty / whitespace reply:** keep the original question (existing
  `parse_improved_question`).

## Testing

- Unit test `terms_for_entry`: rows present (sorted) and absent (empty vec).
- Unit test the terms-line builder / substitution: terms present → guidance
  sentence rendered and `{terms}` gone; empty → clean prompt, no `{terms}` left.
- Headless screenshot verification per linux-lit's protocol on a tagged entry
  (e.g. the "fee simple" Rom 3.1 entry from the reporting screenshot): press
  `R`, confirm the improved question still centers "fee simple".

## Follow-up (separate, tiny)

Add `journal.improve-question` to the `update-api-prompt` skill's prompt-key
list in `claude-api-prompts/.claude/skills/update-api-prompt/SKILL.md` — it
currently lists `journal.qa` but not the improve-question key, so future
"update the improve-question prompt" requests route there cleanly.

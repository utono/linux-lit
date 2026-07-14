# Journal `R`-rewrite Uses Entry Key Terms — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `R` improves a displayed journal entry's question, feed the entry's saved `journal_tags` terms into the `journal.improve-question` prompt so the rewrite preserves and sharpens those terms of art.

**Architecture:** Add a `{terms}` placeholder to the `journal.improve-question` master prompt (synced to lit.db's `api_prompts`). In linux-lit, add an entry→terms db helper, thread a `terms: &[String]` param through `improve_question`, substitute `{terms}` with a guidance sentence (or `""` when empty), and have the `R` path (`rewrite_question_path`) fetch the displayed entry's terms up-front and pass them. New-ask callers pass `&[]`.

**Tech Stack:** Rust (rusqlite, GTK4), Python sync scripts, SQLite (lit.db).

## Global Constraints

- Source of truth for API prompts is the master in `~/utono/claude-api-prompts/prompts/<key>.md`; NEVER edit `api_prompts` rows directly — sync from the master.
- Placeholder tokens are substituted by the consumer at request time via `template.replace("{token}", value)` (matching `{ipa_rules}`/`{genre}` in `src/gloss.rs`), NOT stored resolved.
- The improve-question contract is unchanged: preserve intent, do not answer, return ONLY the improved question as one plain-text line.
- Empty terms → `{terms}` substitutes to `""`; prompt behaves byte-for-byte like today (no regression).
- linux-lit reads the active `api_prompts` row at next launch (no hot reload).

---

### Task 1: `terms_for_entry` db helper

**Files:**
- Modify: `~/utono/linux-lit/src/db/journal.rs` (add fn after `find_distinct_terms`, ~line 304; add test in the `#[cfg(test)] mod tests` block)

**Interfaces:**
- Produces: `pub fn terms_for_entry(conn: &Connection, entry_id: i64) -> Result<Vec<String>, rusqlite::Error>` — the entry's distinct tag terms, sorted ascending; empty vec when untagged.

- [ ] **Step 1: Write the failing test** (add inside `mod tests`)

```rust
#[test]
fn terms_for_entry_sorted_and_empty() {
    let conn = mem();
    conn.execute(
        "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
         VALUES (9,'Rom',3,1,'q','a','scene')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO journal_tags (entry_id, term) VALUES (9,'quibble'),(9,'fee simple')",
        [],
    )
    .unwrap();
    // sorted ascending, scoped to the entry
    assert_eq!(
        terms_for_entry(&conn, 9).unwrap(),
        vec!["fee simple".to_string(), "quibble".to_string()]
    );
    // untagged entry -> empty
    assert!(terms_for_entry(&conn, 999).unwrap().is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ~/utono/linux-lit && cargo test --lib db::journal::tests::terms_for_entry_sorted_and_empty`
Expected: FAIL — `cannot find function terms_for_entry`.

- [ ] **Step 3: Write minimal implementation** (after `find_distinct_terms`)

```rust
/// The distinct terms tagged on a single journal entry, sorted ascending.
/// Complements `find_pages_by_term` (term→entries): this is entry→terms, used
/// to ground the improve-question rewrite on what the entry actually explains.
pub fn terms_for_entry(conn: &Connection, entry_id: i64) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT term FROM journal_tags WHERE entry_id = ?1 ORDER BY term ASC")?;
    let rows = stmt.query_map([entry_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ~/utono/linux-lit && cargo test --lib db::journal::tests::terms_for_entry_sorted_and_empty`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/db/journal.rs
git commit -m "feat(journal): terms_for_entry — entry→tag-terms helper"
```

---

### Task 2: `improve_question` takes `terms` and fills `{terms}`; fallback learns it

**Files:**
- Modify: `~/utono/linux-lit/src/input/actions/journal.rs` — `FALLBACK_IMPROVE_QUESTION_PROMPT` (~47-53), `improve_question` (~64-92), the two call sites (`rewrite_question_path` ~1494, `submit_prompt` ~1646). Add a pure helper `improve_terms_line` + its unit test.

**Interfaces:**
- Consumes: `crate::db::journal::terms_for_entry` (Task 1).
- Produces:
  - `fn improve_terms_line(terms: &[String]) -> String` — guidance sentence naming the terms, or `""` when empty. Pure, unit-tested.
  - `fn improve_question(state, question: String, terms: &[String], on_done)` — new `terms` param; substitutes `{terms}` in the (DB-or-fallback) prompt.

- [ ] **Step 1: Write the failing test** (add inside the `#[cfg(test)] mod tests` block of `journal.rs`)

```rust
#[test]
fn improve_terms_line_guidance_or_empty() {
    // empty -> empty string (prompt reads clean)
    assert_eq!(super::improve_terms_line(&[]), "");
    // terms -> guidance naming them, preserve-verbatim instruction present
    let line = super::improve_terms_line(&["fee simple".to_string(), "quibble".to_string()]);
    assert!(line.contains("fee simple, quibble"), "names terms: {line}");
    assert!(line.to_lowercase().contains("preserve"), "guidance present: {line}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ~/utono/linux-lit && cargo test --lib input::actions::journal::tests::improve_terms_line_guidance_or_empty`
Expected: FAIL — `cannot find function improve_terms_line`.

- [ ] **Step 3: Implement the helper + thread `terms` through `improve_question`**

Replace `FALLBACK_IMPROVE_QUESTION_PROMPT` (keep the existing wording; append the `{terms}` paragraph so it stays symmetric with the master):

```rust
const FALLBACK_IMPROVE_QUESTION_PROMPT: &str = "\
You improve the phrasing of a reader's question about a literary work. Make it \
clear, specific, and well-formed while PRESERVING the reader's intent and \
meaning — do not answer it, do not add new sub-questions, do not change what is \
being asked. Fix grammar, tighten wording, and resolve vague references only as \
the surrounding intent allows.\n\
{terms}\n\
Return ONLY the improved question as a single line of plain text — no preamble, \
no quotes, no markdown, no explanation.";
```

Add the pure helper (place it just above `improve_question`):

```rust
/// The `{terms}` substitution for the improve-question prompt: a guidance
/// sentence naming the entry's key terms of art and telling Claude to keep them,
/// or the empty string when the entry has no tags (so the prompt reads cleanly
/// and behaves exactly as before this feature).
fn improve_terms_line(terms: &[String]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    format!(
        "The reader's question concerns these terms of art: {}. Preserve them \
         verbatim in your rewrite — keep each term's canonical phrasing, and do \
         not rename, gloss away, or drop any of them.",
        terms.join(", ")
    )
}
```

Update `improve_question` — add the `terms` param and the substitution (only the signature + the two prompt lines change):

```rust
fn improve_question(
    state: &Rc<RefCell<AppState>>,
    question: String,
    terms: &[String],
    on_done: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
) {
    let model = state.borrow().config.claude_model.clone();
    let prompt = crate::db::prompts::active_prompt("journal.improve-question")
        .unwrap_or_else(|| FALLBACK_IMPROVE_QUESTION_PROMPT.to_string())
        .replace("{terms}", &improve_terms_line(terms));
    // ...unchanged below...
```

- [ ] **Step 4: Fix the two call sites** so the crate compiles.

In `rewrite_question_path`, fetch terms up-front (before the async call) and pass them. Locate the existing model-capture block (`let model = if page.claude_model.is_empty() ...`) and, right after it, add:

```rust
    // Fetch the displayed entry's key terms up-front (like id/q/a/model) so a
    // navigate during the async improve round-trip can't cross entries.
    let terms = crate::db::queries::open_db_rw()
        .ok()
        .and_then(|conn| crate::db::journal::terms_for_entry(&conn, id).ok())
        .unwrap_or_default();
```

Then change the call from `improve_question(state, old_q, move |st, improved_q| {` to:

```rust
    improve_question(state, old_q, &terms, move |st, improved_q| {
```

In `submit_prompt`, the new-ask call has no entry/tags — pass an empty slice. Change `improve_question(state, text, move |st, improved| {` to:

```rust
    improve_question(state, text, &[], move |st, improved| {
```

- [ ] **Step 5: Run the test + build to verify**

Run: `cd ~/utono/linux-lit && cargo test --lib input::actions::journal::tests::improve_terms_line_guidance_or_empty && cargo build 2>&1 | tail -5`
Expected: test PASS; build succeeds (no unused-param / arity errors at the two call sites).

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/actions/journal.rs
git commit -m "feat(journal): R-rewrite grounds improve-question on entry key terms"
```

---

### Task 3: Prompt master `{terms}` placeholder + sync to lit.db

**Files:**
- Modify: `~/utono/claude-api-prompts/prompts/journal.improve-question.md`

**Interfaces:**
- Produces: `journal.improve-question` active row in `api_prompts` with `has_placeholders: true` and a `{terms}` token — consumed by Task 2's substitution.

- [ ] **Step 1: Edit the master** to add `has_placeholders: true` and the `{terms}` paragraph:

```markdown
---
prompt_key: journal.improve-question
consumer: linux-lit
has_placeholders: true
---

You improve the phrasing of a reader's question about a literary work. Make it clear, specific, and well-formed while PRESERVING the reader's intent and meaning — do not answer it, do not add new sub-questions, do not change what is being asked. Fix grammar, tighten wording, and resolve vague references only as the surrounding intent allows.

{terms}

Return ONLY the improved question as a single line of plain text — no preamble, no quotes, no markdown, no explanation.
```

- [ ] **Step 2: Commit the master** (the subject becomes the DB version note):

```bash
cd ~/utono/claude-api-prompts
git commit -am "feat: journal.improve-question — anchor rewrite to entry key terms"
```

- [ ] **Step 3: Sync to lit.db and verify**

```bash
cd ~/utono/claude-api-prompts
python scripts/sync-to-db.py journal.improve-question
python scripts/render-prompt.py journal.improve-question    # {terms} left unresolved is EXPECTED
python scripts/list-versions.py journal.improve-question     # new version marked active (*)
```

Expected: `list-versions` shows a new active version; `render-prompt` prints the body with `{terms}` still present (linux-lit fills it at request time).

---

### Task 4: Add `journal.improve-question` to the `update-api-prompt` skill key list

**Files:**
- Modify: `~/utono/claude-api-prompts/.claude/skills/update-api-prompt/SKILL.md` (Prompt keys list, ~line 30)

- [ ] **Step 1: Add the key** under `journal.qa`:

```markdown
- `journal.qa` — journal Q&A interlocutor
- `journal.improve-question` — improves the phrasing of a reader's `R`
  rewrite question; `{terms}` is filled with the entry's key terms
```

Also add `journal.improve-question` to the frontmatter `description` list of journal keys.

- [ ] **Step 2: Commit**

```bash
cd ~/utono/claude-api-prompts
git add .claude/skills/update-api-prompt/SKILL.md
git commit -m "docs(skill): list journal.improve-question in update-api-prompt"
```

---

### Task 5: End-to-end verification (headless) + finish branch

- [ ] **Step 1: Full test suite + build (linux-lit)**

Run: `cd ~/utono/linux-lit && cargo test --lib 2>&1 | tail -15 && cargo build 2>&1 | tail -3`
Expected: all tests pass; release/debug build succeeds.

- [ ] **Step 2: Confirm the active prompt is what linux-lit will read**

Run: `litecli ~/utono/litdb/data/lit.db -e "SELECT version, has_placeholders, is_active FROM api_prompts WHERE prompt_key='journal.improve-question' ORDER BY version;"`
Expected: newest version `is_active=1`, `has_placeholders=1`; body contains `{terms}`.

- [ ] **Step 3: Headless screenshot verification** per `~/utono/linux-lit/CLAUDE.md` Headless Verification: launch the reader in the isolated `cage` compositor on a tagged entry (the Rom 3.1 "fee simple" entry), press `R` → `q`, and confirm the improved question still centers "fee simple". Capture a screenshot as evidence.

- [ ] **Step 4: Merge branch back to master and push** (per workspace finishing-a-branch rule): verify clean tree + tests, `git checkout master`, `git merge --no-ff`, re-verify build, `git push origin master`, `git branch -d`. Do the same for the `claude-api-prompts` repo (its own commits).

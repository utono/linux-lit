# Journal Auto-Tagging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically extract and store an entry's terms in `journal_tags` whenever the user creates, rewrites, or manually edits a journal Q&A in the reader — keeping the `f` term-browse suggestions and tags-first matches current without a batch run.

**Architecture:** Fire-and-forget background tagger. After the entry is committed (save UX unchanged), spawn an async Claude call via the existing `run_claude_request` bridge using the shared `journal.extract-terms` prompt on a small model; parse `{"terms":[...]}`; upsert `journal_tags` with `source='reader-auto'` under a replace-auto-keep-manual transaction. Delete relies on `ON DELETE CASCADE` (requires enabling `PRAGMA foreign_keys`).

**Tech Stack:** Rust, gtk4-rs (`glib::spawn_future_local` via `claude_bridge::run_claude_request`), rusqlite (SQLite), serde_json, cargo test.

## Global Constraints

- Design doc: `docs/plans/2026-07-12-journal-auto-tag-design.md`. Binding decisions:
- Reuse the SAME extraction prompt as litdb: `crate::db::prompts::active_prompt("journal.extract-terms")`. Do NOT invent a new prompt.
- `parse_terms`: lowercase, trim, dedupe (order-preserving), cap at 8; tolerant (empty Vec on missing `"terms"` key or non-list) — mirrors litdb `tag_journal.py::parse_terms_result`.
- Re-tag policy: DELETE `journal_tags` rows `WHERE entry_id=? AND source IN ('backfill','reader-auto')`, then INSERT the fresh terms with `source='reader-auto'`. Rows with any OTHER source (e.g. `'manual'`) are preserved.
- On a Claude CALL ERROR, write NOTHING (leave existing tags). Only a SUCCESSFUL response (incl. `{"terms":[]}`) runs the replace transaction.
- Config: `auto_tag_journal: bool` default `true` (spawn_retag no-ops when false); `tag_extract_model: String` default `"claude-haiku-4-5-20251001"`.
- Delete cascade: `open_db_rw()` MUST set `PRAGMA foreign_keys = ON` or `journal_tags` rows orphan on entry delete.
- Verified pre-facts (do not re-derive): `save_journal_page`/`save_passage_page` already `-> Result<i64,_>` returning `last_insert_rowid()`. Edits go through `update_journal_page(conn, id, q, a, model)` with the existing `id`. `run_claude_request(state, system, user, model, on_success, on_error)` is the spawn primitive (`src/input/actions/claude_bridge.rs`). It calls `gloss::call_claude_with_prompt` off-thread and runs callbacks on the GTK main loop.
- Build: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo build`. Test: `cargo test <name>`. This is a bin-only crate — use `cargo test <name>` (no `--lib`).

---

## Task 1: `parse_terms` pure parser + module

**Files:**
- Create: `src/journal_tags.rs`
- Modify: `src/main.rs` (add `mod journal_tags;` beside the other top-level `mod` declarations)
- Test: same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub fn parse_terms(raw: &str) -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

Create `src/journal_tags.rs` with ONLY the tests first (no `parse_terms` body yet — a bare `pub fn parse_terms(_raw: &str) -> Vec<String> { unimplemented!() }` stub so the module compiles but tests fail):

```rust
//! Pure parsing of the journal term-extraction model response. The extractor
//! returns `{"terms":[...]}`; this normalizes it (lowercase, trim, dedupe,
//! cap 8). Mirrors litdb tag_journal.py::parse_terms_result. No DB, no GTK.

/// Parse the extractor's `{"terms":[...]}` reply into a clean term list:
/// lowercase, trim, dedupe (order-preserving), cap at 8. Tolerant — returns an
/// empty Vec when the JSON is unparseable, lacks a `"terms"` key, or `"terms"`
/// is not a list. Non-string / blank-after-trim elements are skipped.
pub fn parse_terms(raw: &str) -> Vec<String> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        let out = parse_terms(r#"{"terms":["Fee Simple","  freehold ","FEE SIMPLE"]}"#);
        // lowercased, trimmed, deduped (order-preserving), "fee simple" once.
        assert_eq!(out, vec!["fee simple".to_string(), "freehold".to_string()]);
    }

    #[test]
    fn empty_terms_list_ok() {
        assert!(parse_terms(r#"{"terms":[]}"#).is_empty());
    }

    #[test]
    fn missing_key_or_bad_shape_is_empty_not_panic() {
        assert!(parse_terms(r#"{"nope":[1]}"#).is_empty());
        assert!(parse_terms(r#"{"terms":"notalist"}"#).is_empty());
        assert!(parse_terms("total garbage not json").is_empty());
    }

    #[test]
    fn caps_at_eight_and_skips_blanks() {
        let raw = r#"{"terms":["a","b","c","d","e","f","g","h","i","  "]}"#;
        let out = parse_terms(raw);
        assert_eq!(out.len(), 8);
        assert_eq!(out, vec!["a","b","c","d","e","f","g","h"]);
    }
}
```

Add `mod journal_tags;` to `src/main.rs` (find the block of top-level `mod X;` lines and add it alphabetically).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test journal_tags::tests`
Expected: tests FAIL (panic `unimplemented!()`).

- [ ] **Step 3: Implement `parse_terms`**

Replace the stub body:

```rust
pub fn parse_terms(raw: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = value.get("terms").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in arr {
        let Some(s) = item.as_str() else { continue };
        let norm = s.trim().to_lowercase();
        if norm.is_empty() || out.contains(&norm) {
            continue;
        }
        out.push(norm);
        if out.len() == 8 {
            break;
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test journal_tags::tests`
Expected: all 4 pass.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit-wt/journal-auto-tag
git add src/journal_tags.rs src/main.rs
git commit -m "feat(journal): parse_terms pure parser for auto-tag extraction"
```

---

## Task 2: `replace_auto_tags` DB upsert + `foreign_keys` pragma

**Files:**
- Modify: `src/db/journal.rs` (add `replace_auto_tags`)
- Modify: `src/db/queries.rs` (`open_db_rw` sets `PRAGMA foreign_keys=ON`)
- Test: `src/db/journal.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: the existing `mem()` test helper (calls `ensure_journal_table` → `ensure_journal_tags`, so `journal_tags` exists in-memory).
- Produces: `pub fn replace_auto_tags(conn: &Connection, entry_id: i64, terms: &[String]) -> Result<(), rusqlite::Error>`

- [ ] **Step 1: Write the failing test**

Add to the tests module in `src/db/journal.rs`:

```rust
#[test]
fn replace_auto_tags_replaces_auto_preserves_manual() {
    let conn = mem();
    conn.execute(
        "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
         VALUES (5, 'Rom', 1, 1, 'q', 'a', 'scene')",
        [],
    ).unwrap();
    // Pre-existing tags: one backfill, one reader-auto (both auto), one manual.
    conn.execute(
        "INSERT INTO journal_tags (entry_id, term, source) VALUES \
         (5,'old-backfill','backfill'),(5,'old-auto','reader-auto'),(5,'keepme','manual')",
        [],
    ).unwrap();

    replace_auto_tags(&conn, 5, &["fee simple".to_string(), "freehold".to_string()]).unwrap();

    let mut got: Vec<(String, String)> = conn
        .prepare("SELECT term, source FROM journal_tags WHERE entry_id=5 ORDER BY term")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    got.sort();
    // manual survives; both auto rows gone; two new reader-auto rows present.
    assert_eq!(
        got,
        vec![
            ("fee simple".to_string(), "reader-auto".to_string()),
            ("freehold".to_string(), "reader-auto".to_string()),
            ("keepme".to_string(), "manual".to_string()),
        ]
    );
}

#[test]
fn replace_auto_tags_empty_clears_auto_only() {
    let conn = mem();
    conn.execute(
        "INSERT INTO journal_entries (id, work_abbrev, div1, div2, question, answer, scope) \
         VALUES (6, 'Rom', 1, 1, 'q', 'a', 'scene')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO journal_tags (entry_id, term, source) VALUES \
         (6,'gone','reader-auto'),(6,'stay','manual')",
        [],
    ).unwrap();

    replace_auto_tags(&conn, 6, &[]).unwrap();

    let terms: Vec<String> = conn
        .prepare("SELECT term FROM journal_tags WHERE entry_id=6 ORDER BY term")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(terms, vec!["stay".to_string()]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test replace_auto_tags`
Expected: FAIL to compile — `replace_auto_tags` not found.

- [ ] **Step 3: Implement `replace_auto_tags`**

Add near the other tag functions in `src/db/journal.rs` (e.g. after `find_distinct_terms`):

```rust
/// Replace this entry's auto-generated tags with `terms` in one transaction:
/// delete rows whose source is 'backfill' or 'reader-auto', then insert `terms`
/// with source 'reader-auto'. Tags with any other source (e.g. 'manual') are
/// preserved. An empty `terms` slice just clears the auto rows.
pub fn replace_auto_tags(
    conn: &Connection,
    entry_id: i64,
    terms: &[String],
) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM journal_tags \
         WHERE entry_id = ?1 AND source IN ('backfill', 'reader-auto')",
        rusqlite::params![entry_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO journal_tags (entry_id, term, source) \
             VALUES (?1, ?2, 'reader-auto')",
        )?;
        for term in terms {
            stmt.execute(rusqlite::params![entry_id, term])?;
        }
    }
    tx.commit()
}
```

(Note: `unchecked_transaction()` takes `&Connection` — matches the existing signature style in this module where callers pass a borrowed `Connection`.)

- [ ] **Step 4: Add the `foreign_keys` pragma**

In `src/db/queries.rs`, `open_db_rw`, add the pragma so `ON DELETE CASCADE` fires when a journal entry is deleted:

```rust
pub fn open_db_rw() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test replace_auto_tags`
Expected: both pass.

- [ ] **Step 6: Regression — existing journal DB tests**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test db::journal`
Expected: all pass (the term-browse tests + these two new ones).

- [ ] **Step 7: Commit**

```bash
cd /home/mlj/utono/linux-lit-wt/journal-auto-tag
git add src/db/journal.rs src/db/queries.rs
git commit -m "feat(journal): replace_auto_tags upsert + enable foreign_keys for tag cascade"
```

---

## Task 3: Config fields `auto_tag_journal` + `tag_extract_model`

**Files:**
- Modify: `src/config.rs` (add two fields with serde defaults)

**Interfaces:**
- Produces: `AppState`/config access to `auto_tag_journal: bool` (default true) and `tag_extract_model: String` (default `"claude-haiku-4-5-20251001"`).

- [ ] **Step 1: Read the config struct + its default pattern**

Read `src/config.rs`. Find the main config struct and how existing `bool`/`String` fields declare serde defaults (the codebase uses `#[serde(default = "...")]` helper fns or `#[serde(default)]`). Match the EXISTING pattern exactly — do not introduce a new defaulting style.

- [ ] **Step 2: Add the two fields**

Add to the config struct, mirroring the sibling fields' serde-default idiom. If the file uses named default fns:

```rust
    #[serde(default = "default_auto_tag_journal")]
    pub auto_tag_journal: bool,
    #[serde(default = "default_tag_extract_model")]
    pub tag_extract_model: String,
```

and the helpers (place beside the other `default_*` fns):

```rust
fn default_auto_tag_journal() -> bool {
    true
}
fn default_tag_extract_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}
```

Also add both fields to any manual `Default` impl / struct literal constructor for the config (so a non-serde construction path still compiles and defaults correctly). If the struct derives `Default`, ensure the derive still holds (a `String` field breaks `#[derive(Default)]` only if others don't already — mirror how existing `String` fields are handled; if there is a hand-written `Default`, add the two fields there with the same default values).

- [ ] **Step 3: Build**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo build`
Expected: compiles clean (fields unused yet → possible dead_code warnings, acceptable; Task 4 uses them). Do NOT add `#[allow(dead_code)]`.

- [ ] **Step 4: Commit**

```bash
cd /home/mlj/utono/linux-lit-wt/journal-auto-tag
git add src/config.rs
git commit -m "feat(config): auto_tag_journal (default on) + tag_extract_model (haiku)"
```

---

## Task 4: `spawn_retag` background tagger + wire into save/rewrite/edit

**Files:**
- Modify: `src/input/actions/journal.rs` (add `spawn_retag`; call it from the ask-save, rewrite, and edit completions)

**Interfaces:**
- Consumes: `crate::db::prompts::active_prompt` (returns the active prompt text, `Option<String>` or `Result` — confirm during Step 1); `crate::input::actions::claude_bridge::run_claude_request`; `crate::journal_tags::parse_terms` (Task 1); `crate::db::journal::replace_auto_tags` (Task 2); config `auto_tag_journal` / `tag_extract_model` (Task 3).
- Produces: `fn spawn_retag(state: &Rc<RefCell<AppState>>, entry_id: i64, question: String, answer: String)`

- [ ] **Step 1: Confirm the call surfaces (read-only)**

Read, in `src/input/actions/journal.rs`:
- The ask-card SAVE completion(s) that call `save_journal_page` / `save_passage_page` (grep those fn names) — capture the returned `entry_id`, and the question/answer strings available there.
- `begin_rewrite` and `begin_edit` — they call `update_journal_page(&conn, id, &q, &a, &model)`; the `id` and the final `q`/`a` are in scope there.
Read `src/db/prompts.rs::active_prompt` for its exact return type. Read how `run_claude_request`'s `on_success` closure is written elsewhere (borrow shape for `state`).

- [ ] **Step 2: Add `spawn_retag`**

Add to `src/input/actions/journal.rs`. Adjust `active_prompt`'s unwrap to its real return type found in Step 1 (shown here assuming `Option<String>`):

```rust
/// Fire-and-forget: extract this entry's terms via Claude (the shared
/// `journal.extract-terms` prompt, on `tag_extract_model`) and replace its
/// auto-generated `journal_tags`. No-op when `auto_tag_journal` is off. On a
/// call error nothing is written (existing tags survive); only a successful
/// reply runs the replace. Text is captured by value so overlapping re-edits
/// each tag their own snapshot (last commit wins — correct).
pub(crate) fn spawn_retag(
    state: &Rc<RefCell<AppState>>,
    entry_id: i64,
    question: String,
    answer: String,
) {
    let (enabled, model) = {
        let s = state.borrow();
        (s.config.auto_tag_journal, s.config.tag_extract_model.clone())
    };
    if !enabled {
        return;
    }
    let Some(prompt) = crate::db::prompts::active_prompt("journal.extract-terms") else {
        crate::logging::log("AUTO_TAG: no active journal.extract-terms prompt; skipping");
        return;
    };
    let user_msg = format!("Q: {question}\nA: {answer}");
    crate::input::actions::claude_bridge::run_claude_request(
        state,
        prompt,
        user_msg,
        model,
        // on_success: parse + write. Own its own rw connection; never touches AppState.
        move |_state, reply| {
            let terms = crate::journal_tags::parse_terms(&reply);
            match crate::db::queries::open_db_rw() {
                Ok(conn) => {
                    if let Err(e) = crate::db::journal::replace_auto_tags(&conn, entry_id, &terms) {
                        crate::logging::log(&format!("AUTO_TAG: write failed for {entry_id}: {e}"));
                    } else {
                        crate::logging::log(&format!(
                            "AUTO_TAG: entry {entry_id} tagged with {} term(s)",
                            terms.len()
                        ));
                    }
                }
                Err(e) => crate::logging::log(&format!("AUTO_TAG: open_db_rw failed: {e}")),
            }
        },
        // on_error: write NOTHING — leave existing tags intact.
        move |_state, msg| {
            crate::logging::log(&format!("AUTO_TAG: extract call failed ({msg}); tags unchanged"));
        },
    );
}
```

IMPORTANT borrow note: the closures capture `entry_id` (Copy) and are `Fn`. `run_claude_request` requires `impl Fn(...) + 'static`; `entry_id` is `i64` so both closures can capture it. Because `on_success` and `on_error` each need `entry_id`, and it is `Copy`, both `move` closures capturing it independently is fine. Do NOT capture `question`/`answer` into the closures (they were already sent in `user_msg`).

- [ ] **Step 3: Call `spawn_retag` from the three completion sites**

At each site found in Step 1, AFTER the entry is committed, add the call:

- Ask-card new-entry save (where `save_journal_page`/`save_passage_page` returns `id`): capture the returned id and call
  ```rust
  spawn_retag(state, id, question_text.clone(), answer_text.clone());
  ```
  using whatever the local variable names for the question/answer text are at that site (clone if they are borrowed / reused).
- `begin_rewrite` completion (after `update_journal_page(&conn, id, &q, &a, &model)` succeeds): `spawn_retag(state, id, q.clone(), a.clone());`
- `begin_edit` save (after `update_journal_page` succeeds): `spawn_retag(state, id, q.clone(), a.clone());`

Scope borrows so the write-connection block / `update_journal_page` `conn` is dropped before `spawn_retag` (which borrows `state`). `run_claude_request` borrows `state` immutably only briefly (for `tokio_handle`), so calling it after the rw-conn scope closes is safe. If a site holds `state.borrow_mut()` across where you add the call, close that scope first.

- [ ] **Step 4: Build**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo build`
Expected: compiles clean. The Task-3 config fields are now used (their dead_code warnings disappear). If a `active_prompt` return-type mismatch appears, fix per Step 1's finding.

- [ ] **Step 5: Full test suite (no regressions)**

Run: `cd /home/mlj/utono/linux-lit-wt/journal-auto-tag && cargo test`
Expected: all pass (Task 1 + Task 2 additions + existing suite).

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit-wt/journal-auto-tag
git add src/input/actions/journal.rs
git commit -m "feat(journal): spawn_retag background auto-tagger on create/rewrite/edit"
```

---

## Task 5: Headless end-to-end verification

**Files:** none (verification).

- [ ] **Step 1: Baseline the DB**

```bash
litecli ~/utono/litdb/data/lit.db -e "SELECT COUNT(*) FROM journal_tags WHERE source='reader-auto';"
```
Note the count (likely 0 before this feature has ever run live).

- [ ] **Step 2: Drive create → auto-tag (real Claude call)**

Follow `~/utono/linux-lit/CLAUDE.md` "Headless Verification". Launch the worktree binary in `cage` (`LIT_NO_MPV=1 LIT_DEV=1 GSK_RENDERER=cairo ...`). Open the journal overlay on a work/scene, press `r` (ask card), enter a question whose answer will name a clear term of art (e.g. ask about a legal or rhetorical term), send, and let the Q&A save. Wait ~10s for the background extract call. Then:
```bash
litecli ~/utono/litdb/data/lit.db -e "SELECT entry_id, term, source FROM journal_tags WHERE source='reader-auto' ORDER BY entry_id DESC LIMIT 10;"
```
Expected: new `reader-auto` rows for the just-created entry's terms. Also grep the fresh dev log for `AUTO_TAG:` lines confirming the tag count.

- [ ] **Step 3: Verify the new tag shows in `f` suggestions**

In the same cage session, press `f` in the journal overlay and confirm one of the just-extracted terms now appears in the suggestion list (or type its prefix and see it filter in). Screenshot; report the term seen.

- [ ] **Step 4: Verify delete cascade (foreign_keys)**

Note the new entry's id from Step 2. Delete it (in-app `D` + confirm, OR via SQL for isolation), then:
```bash
litecli ~/utono/litdb/data/lit.db -e "SELECT COUNT(*) FROM journal_tags WHERE entry_id=<that_id>;"
```
Expected: 0 (the `ON DELETE CASCADE` fired because `open_db_rw` now enables `foreign_keys`). If in-app delete uses `open_db_rw`, this validates the pragma end to end.

- [ ] **Step 5: Verify the toggle**

Set `auto_tag_journal` to `false` in `~/.config/linux-lit/config-dev.json` (no dev instance running), relaunch, create another Q&A, confirm NO new `reader-auto` rows appear and a log line notes the no-op (or simply absence of an `AUTO_TAG:` tag-count line). Restore the toggle to `true` after.

- [ ] **Step 6: Report**

Report PASS/FAIL per step with the screenshots and the litecli row output. Cleanup: `pkill -f "cage -- ./target/debug/linux-lit"` (scoped form ONLY).

---

## Self-Review notes

- **Spec coverage:** trigger events create/rewrite/edit → Task 4 Step 3 (three sites); delete → Task 2 Step 4 (`foreign_keys`) + Task 5 Step 4. Prompt reuse → Task 4 Step 2 (`active_prompt("journal.extract-terms")`). parse (lowercase/trim/dedupe/cap8/tolerant) → Task 1. replace-auto-keep-manual → Task 2. call-error-writes-nothing → Task 4 (`on_error` no-op). config toggle + model → Task 3, consumed Task 4.
- **Type consistency:** `parse_terms(&str)->Vec<String>` (T1) consumed in T4; `replace_auto_tags(&Connection,i64,&[String])` (T2) consumed in T4; `spawn_retag(&Rc<RefCell<AppState>>,i64,String,String)` (T4). `entry_id: i64` matches `save_*`'s `Result<i64,_>` and `update_journal_page`'s `id`.
- **No placeholders:** all code shown. The two `active_prompt` return-type / config-default-style adaptations are explicit read-first-then-match steps, not vague "handle it" directives.
- **YAGNI:** no new prompt, no new spawn machinery (`run_claude_request` reused), no suggestion cache.

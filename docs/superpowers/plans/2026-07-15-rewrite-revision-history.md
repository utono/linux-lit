# Rewrite Revision History, Diff-Highlight & Browsing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a journal Q&A or a gloss is rewritten by a custom prompt, store the prior version durably, word-diff the result, highlight the changed words until Escape, and let the user browse (Ctrl+Shift+n/p) and restore (Ctrl+Shift+r) prior versions.

**Architecture:** A pure, GTK-free diff+revision core (unit-tested) mirrors `src/input/overlay_search.rs` — a pure `collect`/`step`-style module plus a `gtk_ops` submodule that applies/clears an ephemeral TextTag. A new `rewrite_revisions` lit.db table (auto-migrated on DB open, litdb-owned schema) records every pre-rewrite version. The two custom-prompt completion closures (`gloss::edit_gloss`, `journal::rewrite_with_claude`) append a revision, then apply the diff highlight. Overlay Escape handlers and nav/close/rewrite paths clear it. New Ctrl+Shift+n/p/r keybinds drive view-only browsing and restore in both overlays.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite/SQLite, tokio (async Claude bridge already in place). Tests: `cargo test` unit tests (pure core, DB round-trips); headless cage e2e for on-screen verification.

## Global Constraints

- **Trigger is custom-prompt AI rewrites ONLY:** `gloss::edit_gloss` and `journal::rewrite_with_claude`. Hand-edit `:w` saves, undo, and non-custom regenerates are EXCLUDED — no revision append, no highlight.
- **litdb owns lit.db's schema.** New table defined in a litdb migration + schema.sql; linux-lit only auto-migrates idempotently (`CREATE TABLE IF NOT EXISTS` + `column_exists` probe), never as the authoritative definition. Never rename a work's abbrev with raw SQL.
- **Diff is word-level**, computed on the **rendered plain text** (post markup-strip for gloss), returning **character** offsets (GTK TextBuffer indexes by char).
- **History browsing is view-only** — it NEVER mutates the live entry. Only `Ctrl+Shift+r` (restore) writes.
- **Highlight reuses the search-match color** via existing `set_search_colors` wiring; a DISTINCT ephemeral tag so it clears independently.
- **Every keybind change updates the Ctrl+/ overlay legend** for that surface (`src/ui/{gloss,journal}_keybinds_overlay.rs` `GROUPS`).
- **Do NOT run the app** (`cargo run`) — build with `cargo build`; the user runs it. Agents may use the headless cage harness.
- **RPD layout:** shifted letters arrive as the shifted glyph name with `is_shift=true` (e.g. `"N"`, `"P"`, `"R"`), so Ctrl+Shift chords must match both the raw and shifted-glyph forms.
- Commit messages end with the Co-Authored-By / Claude-Session trailers per repo convention.

---

### Task 1: Pure word-diff core

**Files:**
- Create: `src/input/rewrite_diff.rs`
- Modify: `src/input/mod.rs` (add `pub mod rewrite_diff;`)
- Test: inline `#[cfg(test)]` in `src/input/rewrite_diff.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub fn changed_ranges(old: &str, new: &str) -> Vec<(i32, i32)>` — returns **character** offset spans within `new` covering the words that are new or changed relative to `old`, in document order, non-overlapping, ascending. Whitespace-only differences produce no ranges. Identical inputs → empty.

**Approach note:** Tokenize into words with their char-offset spans (a word = a maximal run of non-whitespace). Run a longest-common-subsequence over the word token *strings*; any `new` token not matched to an `old` token in the LCS is "changed/added" and contributes its char span. Merge adjacent changed word spans separated only by whitespace into a single range so the highlight reads as a contiguous phrase.

- [ ] **Step 1: Write the failing tests**

```rust
// src/input/rewrite_diff.rs
//! Word-level diff between a previous and new rewrite version. Pure, GTK-free:
//! returns CHARACTER-offset spans within `new` for the words that changed or were
//! added relative to `old`. Mirrors the pure/`gtk_ops` split of `overlay_search`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_changes() {
        assert!(changed_ranges("the cat sat", "the cat sat").is_empty());
    }

    #[test]
    fn single_word_substitution() {
        // "cat" -> "dog": only the middle word changed.
        // "the dog sat": offsets d=4..7
        assert_eq!(changed_ranges("the cat sat", "the dog sat"), vec![(4, 7)]);
    }

    #[test]
    fn appended_words_are_ranges() {
        // "the cat" -> "the cat sat down": "sat down" is new (chars 8..16)
        assert_eq!(changed_ranges("the cat", "the cat sat down"), vec![(8, 16)]);
    }

    #[test]
    fn adjacent_changed_words_merge_across_whitespace() {
        // both new words changed and are separated only by a space -> one range
        assert_eq!(changed_ranges("a b", "a X Y"), vec![(2, 5)]);
    }

    #[test]
    fn char_offsets_not_byte_offsets() {
        // leading multibyte char: "é the cat" -> "é the dog"
        // char offsets: é=0 sp=1 t=2 h=3 e=4 sp=5 d=6 -> dog = 6..9
        assert_eq!(changed_ranges("\u{e9} the cat", "\u{e9} the dog"), vec![(6, 9)]);
    }

    #[test]
    fn whitespace_only_change_has_no_ranges() {
        assert!(changed_ranges("the  cat", "the cat").is_empty());
    }

    #[test]
    fn unchanged_words_between_changes_are_not_highlighted() {
        // "a b c d" -> "a X c Y": b->X (2..3) and d->Y (6..7); c unchanged
        assert_eq!(changed_ranges("a b c d", "a X c Y"), vec![(2, 3), (6, 7)]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bins rewrite_diff`
Expected: FAIL — `changed_ranges` not found (won't compile).

- [ ] **Step 3: Implement the core**

```rust
/// Character-offset span of every whitespace-delimited word in `text`, in order.
fn word_spans(text: &str) -> Vec<(i32, i32, &str)> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut char_off = 0i32;
    // Walk char by char tracking both the byte index (for slicing) and char index.
    let mut i = 0usize; // char index
    let mut start: Option<(usize, i32)> = None; // (byte, char) of current word start
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    let _ = (&mut chars, &mut char_off); // (kept simple below)
    while i < bytes.len() {
        let (b, c) = bytes[i];
        if c.is_whitespace() {
            if let Some((sb, sc)) = start.take() {
                out.push((sc, i as i32, &text[sb..b]));
            }
        } else if start.is_none() {
            start = Some((b, i as i32));
        }
        i += 1;
    }
    if let Some((sb, sc)) = start.take() {
        out.push((sc, bytes.len() as i32, &text[sb..]));
    }
    out
}

/// Indices (into `new_words`) of tokens that are part of the LCS with `old_words`
/// (i.e. UNCHANGED). Everything else in `new` is changed/added.
fn lcs_matched_new_indices(old_words: &[&str], new_words: &[&str]) -> Vec<bool> {
    let n = old_words.len();
    let m = new_words.len();
    // dp[i][j] = LCS length of old[i..] and new[j..]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_words[i] == new_words[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut matched = vec![false; m];
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old_words[i] == new_words[j] {
            matched[j] = true;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// Character-offset spans within `new` covering words that changed or were added
/// relative to `old`. Adjacent changed words (separated only by whitespace) merge
/// into one range. Empty when the texts are word-for-word identical.
pub fn changed_ranges(old: &str, new: &str) -> Vec<(i32, i32)> {
    let old_spans = word_spans(old);
    let new_spans = word_spans(new);
    let old_words: Vec<&str> = old_spans.iter().map(|(_, _, w)| *w).collect();
    let new_words: Vec<&str> = new_spans.iter().map(|(_, _, w)| *w).collect();
    let matched = lcs_matched_new_indices(&old_words, &new_words);

    let mut ranges: Vec<(i32, i32)> = Vec::new();
    for (idx, (s, e, _)) in new_spans.iter().enumerate() {
        if matched[idx] {
            continue;
        }
        // Merge with the previous range if this changed word is the very next
        // token (previous range's end..this start is whitespace only).
        if let Some(last) = ranges.last_mut() {
            if idx > 0 && !matched[idx - 1] {
                last.1 = *e;
                continue;
            }
        }
        ranges.push((*s, *e));
    }
    ranges
}
```

- [ ] **Step 4: Add the module declaration**

In `src/input/mod.rs`, add alongside the other `pub mod` lines:

```rust
pub mod rewrite_diff;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins rewrite_diff`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
git add src/input/rewrite_diff.rs src/input/mod.rs
git commit -m "feat(rewrite-diff): pure word-level changed-ranges core

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 2: lit.db `rewrite_revisions` table + auto-migration

**Files:**
- Create: `~/utono/litdb/scripts/migrations/add_rewrite_revisions.sql` (litdb-owned schema)
- Modify: litdb `schema.sql` (add the table + index to the canonical schema — locate via `fd -e sql schema ~/utono/litdb`; if litdb keeps schema in a different canonical file, add there)
- Modify: `src/db/journal.rs` (add `ensure_rewrite_revisions_table`)
- Modify: `src/app/mod.rs:3043` area (call the new ensure fn at startup, next to `ensure_journal_table`)
- Test: inline `#[cfg(test)]` in `src/db/journal.rs`

**Interfaces:**
- Consumes: `crate::db::queries::column_exists` (already exists, queries.rs:871).
- Produces:
  - `pub fn ensure_rewrite_revisions_table(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error>` — idempotent create.

- [ ] **Step 1: Write the litdb migration file**

Create `~/utono/litdb/scripts/migrations/add_rewrite_revisions.sql`:

```sql
-- Durable per-entry revision history for custom-prompt rewrites of journal Q&A
-- (kind='journal', entry_id -> journal_entries.id) and glosses (kind='gloss',
-- entry_id -> glosses.id). Append-only: each row is a PRE-rewrite version.
CREATE TABLE IF NOT EXISTS rewrite_revisions (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,          -- 'journal' | 'gloss'
    entry_id     INTEGER NOT NULL,
    question     TEXT,                       -- journal only; NULL for gloss
    body         TEXT    NOT NULL,           -- answer (journal) or gloss markup
    claude_model TEXT,
    prompt       TEXT,                       -- custom instruction that produced the NEXT version
    timestamp    TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_rewrite_revisions_entry
    ON rewrite_revisions(kind, entry_id, timestamp);
```

Add the same two statements to litdb's canonical `schema.sql`.

- [ ] **Step 2: Write the failing test**

Add to `src/db/journal.rs` `#[cfg(test)]` module (mirror the existing `ensure_journal_table` tests near journal.rs:525):

```rust
#[test]
fn ensure_rewrite_revisions_table_is_idempotent() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    ensure_rewrite_revisions_table(&conn).unwrap();
    ensure_rewrite_revisions_table(&conn).unwrap(); // second call must not error
    // insert + read back one row
    conn.execute(
        "INSERT INTO rewrite_revisions (kind, entry_id, question, body, claude_model, prompt)
         VALUES ('journal', 34, 'Q?', 'old answer', 'm', 'make it shorter')",
        [],
    )
    .unwrap();
    let body: String = conn
        .query_row(
            "SELECT body FROM rewrite_revisions WHERE entry_id = 34 AND kind = 'journal'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(body, "old answer");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --bins ensure_rewrite_revisions_table_is_idempotent`
Expected: FAIL — `ensure_rewrite_revisions_table` not found.

- [ ] **Step 4: Implement the ensure fn**

Add to `src/db/journal.rs` (near `ensure_journal_table`, journal.rs:50):

```rust
/// Idempotent create of the durable rewrite-revision history table. lit.db's
/// schema is owned by litdb (see add_rewrite_revisions.sql); this mirrors it so
/// the app works before litdb re-runs, exactly like `ensure_journal_table`.
pub fn ensure_rewrite_revisions_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS rewrite_revisions (
            id           INTEGER PRIMARY KEY,
            kind         TEXT    NOT NULL,
            entry_id     INTEGER NOT NULL,
            question     TEXT,
            body         TEXT    NOT NULL,
            claude_model TEXT,
            prompt       TEXT,
            timestamp    TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_rewrite_revisions_entry
            ON rewrite_revisions(kind, entry_id, timestamp);",
    )?;
    Ok(())
}
```

- [ ] **Step 5: Call it at startup**

In `src/app/mod.rs`, immediately after the `ensure_journal_table` call (mod.rs:3043):

```rust
            let _ = crate::db::journal::ensure_rewrite_revisions_table(&conn);
```

- [ ] **Step 6: Run test + build to verify**

Run: `cargo test --bins ensure_rewrite_revisions_table_is_idempotent && cargo build`
Expected: test PASS; build succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/db/journal.rs src/app/mod.rs
git commit -m "feat(revisions): rewrite_revisions table + startup auto-migration

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

(Commit the litdb migration + schema.sql separately in the litdb repo.)

---

### Task 3: Revision DB read/write helpers

**Files:**
- Modify: `src/db/journal.rs` (add append/list functions)
- Test: inline `#[cfg(test)]` in `src/db/journal.rs`

**Interfaces:**
- Consumes: `ensure_rewrite_revisions_table` (Task 2).
- Produces:
  - `pub struct Revision { pub id: i64, pub question: Option<String>, pub body: String, pub claude_model: String, pub prompt: Option<String>, pub timestamp: String }`
  - `pub fn append_revision(conn: &Connection, kind: &str, entry_id: i64, question: Option<&str>, body: &str, claude_model: &str, prompt: Option<&str>) -> Result<(), rusqlite::Error>`
  - `pub fn list_revisions(conn: &Connection, kind: &str, entry_id: i64) -> Result<Vec<Revision>, rusqlite::Error>` — oldest→newest by timestamp,id.

- [ ] **Step 1: Write the failing test**

Add to `src/db/journal.rs` `#[cfg(test)]`:

```rust
#[test]
fn append_and_list_revisions_in_order() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    ensure_rewrite_revisions_table(&conn).unwrap();
    append_revision(&conn, "gloss", 7, None, "v1 markup", "m1", Some("shorten")).unwrap();
    append_revision(&conn, "gloss", 7, None, "v2 markup", "m2", Some("expand")).unwrap();
    // a different entry must not leak in
    append_revision(&conn, "gloss", 8, None, "other", "m", None).unwrap();
    let revs = list_revisions(&conn, "gloss", 7).unwrap();
    assert_eq!(revs.len(), 2);
    assert_eq!(revs[0].body, "v1 markup");
    assert_eq!(revs[1].body, "v2 markup");
    assert_eq!(revs[0].prompt.as_deref(), Some("shorten"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins append_and_list_revisions_in_order`
Expected: FAIL — `append_revision` / `list_revisions` / `Revision` not found.

- [ ] **Step 3: Implement**

Add to `src/db/journal.rs`:

```rust
/// One stored prior version of a journal Q&A or gloss.
#[derive(Debug, Clone)]
pub struct Revision {
    pub id: i64,
    pub question: Option<String>,
    pub body: String,
    pub claude_model: String,
    pub prompt: Option<String>,
    pub timestamp: String,
}

/// Append a pre-rewrite version to the durable history. `kind` is 'journal' or
/// 'gloss'; `question` is Some only for journal. `prompt` is the custom
/// instruction that produced the version REPLACING this one.
pub fn append_revision(
    conn: &Connection,
    kind: &str,
    entry_id: i64,
    question: Option<&str>,
    body: &str,
    claude_model: &str,
    prompt: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO rewrite_revisions (kind, entry_id, question, body, claude_model, prompt)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![kind, entry_id, question, body, claude_model, prompt],
    )?;
    Ok(())
}

/// All stored revisions for an entry, oldest → newest.
pub fn list_revisions(
    conn: &Connection,
    kind: &str,
    entry_id: i64,
) -> Result<Vec<Revision>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, question, body, claude_model, prompt, timestamp
         FROM rewrite_revisions
         WHERE kind = ?1 AND entry_id = ?2
         ORDER BY timestamp ASC, id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![kind, entry_id], |r| {
        Ok(Revision {
            id: r.get(0)?,
            question: r.get(1)?,
            body: r.get(2)?,
            claude_model: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            prompt: r.get(4)?,
            timestamp: r.get(5)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins append_and_list_revisions_in_order`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/journal.rs
git commit -m "feat(revisions): append_revision + list_revisions helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 4: Ephemeral diff-highlight tag + apply/clear (both overlays)

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (register a `rewrite_diff` tag; add apply/clear/color methods)
- Modify: `src/ui/journal_overlay.rs` (same)
- Modify: `src/app/mod.rs:1537-1538` + `src/input/actions/settings.rs:297-300` (wire the tag color via the existing search-color path)

**Interfaces:**
- Consumes: `changed_ranges` (Task 1); `overlay_search::{apply,clear}` pattern.
- Produces (on BOTH `GlossOverlay` and `JournalOverlay`):
  - `pub fn rewrite_diff_tag(&self) -> &gtk4::TextTag`
  - `pub fn apply_rewrite_diff(&self, ranges: &[(i32, i32)])` — clears then tags each char range on the overlay's buffer.
  - `pub fn clear_rewrite_diff(&self)` — removes the tag over the whole buffer.
  - `pub fn set_rewrite_diff_color(&self, color: &str)` — sets the tag background.

**Note on the gloss buffer accessor:** the gloss overlay renders through `gloss_view`; use the same buffer the search tags are applied to. In `journal_overlay` the buffer is `self.view.buffer()` (see journal_overlay.rs:889 `buffer()`); in `gloss_overlay` mirror however `search_tag` is applied (the `gloss_view` buffer).

- [ ] **Step 1: Register the tag (journal overlay)**

In `src/ui/journal_overlay.rs`, in `new` right after the search tags are built (journal_overlay.rs:424-429):

```rust
        let rewrite_diff_tag = gtk4::TextTag::builder()
            .name("rewrite_diff")
            .background("#ffe000") // placeholder; set via set_rewrite_diff_color
            .build();
        view.buffer().tag_table().add(&rewrite_diff_tag);
```

Add the field to the struct (near `search_current_tag`, journal_overlay.rs:114-115):

```rust
    rewrite_diff_tag: gtk4::TextTag,
```

And to the struct literal (near journal_overlay.rs:480):

```rust
            rewrite_diff_tag,
```

- [ ] **Step 2: Add the methods (journal overlay)**

Add near `set_search_colors` (journal_overlay.rs:904):

```rust
    /// The ephemeral rewrite diff-highlight tag.
    pub fn rewrite_diff_tag(&self) -> &gtk4::TextTag {
        &self.rewrite_diff_tag
    }

    /// Set the diff-highlight background (theme-wired via set_search_colors).
    pub fn set_rewrite_diff_color(&self, color: &str) {
        self.rewrite_diff_tag.set_background(Some(color));
    }

    /// Tag every changed-word char range on the page buffer (clears first).
    pub fn apply_rewrite_diff(&self, ranges: &[(i32, i32)]) {
        let buffer = self.view.buffer();
        self.clear_rewrite_diff();
        for (a, b) in ranges {
            let s = buffer.iter_at_offset(*a);
            let e = buffer.iter_at_offset(*b);
            buffer.apply_tag(&self.rewrite_diff_tag, &s, &e);
        }
    }

    /// Remove the diff-highlight tag over the whole buffer.
    pub fn clear_rewrite_diff(&self) {
        let buffer = self.view.buffer();
        let (s, e) = buffer.bounds();
        buffer.remove_tag(&self.rewrite_diff_tag, &s, &e);
    }
```

- [ ] **Step 3: Mirror Steps 1-2 for the gloss overlay**

In `src/ui/gloss_overlay.rs`, register a `rewrite_diff` tag on the `gloss_view` buffer beside its `search_tag` (gloss_overlay.rs:541), add the `rewrite_diff_tag` field + literal, and add the identical four methods (using the gloss_view buffer accessor that `search_tag`/`apply_hi_color` use, gloss_overlay.rs:691-709).

- [ ] **Step 4: Wire the color to the theme**

In `src/app/mod.rs` right after the two `set_search_colors` calls (mod.rs:1537-1538):

```rust
    gloss_overlay.set_rewrite_diff_color(&search_all);
    journal_overlay.set_rewrite_diff_color(&search_all);
```

In `src/input/actions/settings.rs` right after the two `set_search_colors` calls (settings.rs:297-300), add the analogous `.set_rewrite_diff_color(&search_all)` for both overlays (use the same `search_all` value computed there).

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: succeeds (no test yet — GTK methods aren't unit-testable; covered by e2e in Task 9).

- [ ] **Step 6: Commit**

```bash
git add src/ui/gloss_overlay.rs src/ui/journal_overlay.rs src/app/mod.rs src/input/actions/settings.rs
git commit -m "feat(overlays): ephemeral rewrite diff-highlight tag (apply/clear/color)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 5: Journal rewrite — append revision + apply diff-highlight

**Files:**
- Modify: `src/input/actions/journal.rs` (the `rewrite_with_claude` `on_success` closure, journal.rs:1867-1914)
- Modify: `src/app/mod.rs` (add a `pending_rewrite_diff: Option<...>` field to AppState if needed to survive re-render — see approach)

**Interfaces:**
- Consumes: `crate::db::journal::append_revision` (Task 3); `crate::input::rewrite_diff::changed_ranges` (Task 1); `JournalOverlay::apply_rewrite_diff` (Task 4).
- Produces: after a custom-prompt journal rewrite, the changed words are tagged on the re-rendered page.

**Approach:** In the `on_success` closure, `prev_answer` is already captured (journal.rs:1854-1873 stores `prev_answer` into `journal_undo`). BEFORE the DB update, append the prior version as a revision. AFTER `render_current`/`render_filtered_match` (journal.rs:1908-1911), compute `changed_ranges(&prev_answer, &revised)` against the **rendered answer text** and call `apply_rewrite_diff`. Because the journal answer renders directly (no markup strip), diff the raw `prev_answer` vs `revised`; then translate to buffer offsets by adding the char length of the rendered `Q: …\n\n` prefix that precedes the answer in the buffer body.

- [ ] **Step 1: Append the revision before the DB update**

In the `on_success` closure, immediately after `journal_undo` is set (journal.rs:1868-1873) and inside the `if let Ok(conn)` block BEFORE `update_journal_page` (journal.rs:1875), add:

```rust
                let _ = crate::db::journal::append_revision(
                    &conn,
                    "journal",
                    id,
                    Some(&prev_question),
                    &prev_answer,
                    &model_for_db,
                    Some(&instruction_owned),
                );
```

Capture `instruction` by value before the closure: near the `let question_owned = ...` line (journal.rs:1856), add `let instruction_owned = instruction.to_string();` and `move` it in.

- [ ] **Step 2: Compute the answer's buffer offset**

The journal body is `Q: {question}\n\n{answer}` (per `journal_doc`, memory `project_journal_vim_edit`). Add a small helper next to the closure to compute the prefix char length:

```rust
/// Char length of the "Q: …\n\n" prefix the journal body renders before the
/// answer, so answer-relative diff offsets can be shifted into buffer offsets.
fn answer_prefix_chars(question: &str) -> i32 {
    // "Q: " + question + "\n\n"
    ("Q: ".chars().count() + question.chars().count() + 2) as i32
}
```

(If `render_current` uses a different body assembly for the displayed page, verify the exact prefix by checking `show_page`/`journal_doc` and adjust; the prefix must match the on-screen buffer.)

- [ ] **Step 3: Apply the diff highlight after re-render**

At the end of the `on_success` closure, after the `render_current`/`render_filtered_match` branches but before the "Rewritten" toast (journal.rs:1913), add:

```rust
            let base = answer_prefix_chars(&question_owned);
            let ranges: Vec<(i32, i32)> = crate::input::rewrite_diff::changed_ranges(
                &prev_answer_for_diff, &revised,
            )
            .into_iter()
            .map(|(a, b)| (a + base, b + base))
            .collect();
            s.journal_overlay.apply_rewrite_diff(&ranges);
```

`prev_answer` is moved into the `journal_undo` set; capture a clone `let prev_answer_for_diff = prev_answer.clone();` before that set so it is still available here.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): custom-prompt rewrite appends revision + diff-highlights

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 6: Gloss rewrite — append revision + apply diff-highlight

**Files:**
- Modify: `src/input/actions/gloss.rs` (`edit_gloss` closure gloss.rs:1422-1442, and `update_and_render_gloss_in_place` gloss.rs:1006-1049)

**Interfaces:**
- Consumes: `append_revision` (Task 3); `changed_ranges` (Task 1); `GlossOverlay::apply_rewrite_diff` (Task 4).
- Produces: after a custom-prompt gloss rewrite, the changed words are tagged on the re-rendered gloss.

**Approach — offset mapping is the subtlety:** the gloss buffer shows the *stripped/rendered* text, not the raw `<verse>/<gloss>` markup. So the diff must run on the **rendered plain text of both versions**. `update_and_render_gloss_in_place` already re-renders via `show_gloss_with_color`; after it, read the overlay buffer's full text as the NEW rendered text, and render the OLD version to plain text the same way to diff against. Simplest reliable source of "old rendered text": before overwriting, capture the CURRENT buffer text (what's on screen is the old rendered gloss). Pass both into the diff.

- [ ] **Step 1: Add an optional diff argument to the in-place renderer**

Change `update_and_render_gloss_in_place` (gloss.rs:1006) to accept the pre-rewrite rendered text and the custom prompt, so it can append a revision and highlight. Add two params:

```rust
fn update_and_render_gloss_in_place(
    state_rc: &Rc<RefCell<AppState>>,
    ctx: &crate::gloss::GlossContext,
    gloss_index: usize,
    gloss_id: i64,
    full_gloss: &str,
    model_for_db: &str,
    log_msg: &str,
    diff: Option<(&str, Option<&str>)>, // (prev_rendered_text, custom_prompt)
) {
```

- [ ] **Step 2: Append the revision inside the renderer**

Inside `update_and_render_gloss_in_place`, in the existing `if let Ok(conn)` block (gloss.rs:1017) before `update_gloss`, append the PRE-rewrite raw gloss as a revision (the raw markup is the durable body; the rendered text is only for diffing):

```rust
        if let Some(g) = { /* pre-write in-memory row */ None::<&()> } {
            let _ = g; // (placeholder removed below)
        }
```

Replace that with the real capture: read the current in-memory raw text before the borrow_mut overwrite. Since the function borrows state later, capture the old raw gloss up front:

```rust
    let prev_raw = state_rc
        .borrow()
        .gloss_list
        .get(gloss_index)
        .map(|g| g.gloss_text.clone());
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let (Some(prev), Some((_, prompt))) = (prev_raw.as_ref(), diff) {
            let _ = crate::db::journal::append_revision(
                &conn, "gloss", gloss_id, None, prev, model_for_db, prompt,
            );
        }
        let _ = crate::db::queries::update_gloss(&conn, gloss_id, full_gloss, model_for_db);
        let _ = crate::db::queries::delete_gloss_audio(&conn, gloss_id);
    }
```

(Keep the existing `remove_dir_all` audio-dir purge that follows.)

- [ ] **Step 3: Apply the diff highlight after re-render**

At the end of `update_and_render_gloss_in_place`, after `show_gloss_with_color` + `set_position`/`set_citation` (gloss.rs:1036-1042) and while `s` is still borrowed, add:

```rust
    if let Some((prev_rendered, _)) = diff {
        let new_rendered = s.gloss_overlay.buffer_text_for_diff();
        let ranges = crate::input::rewrite_diff::changed_ranges(prev_rendered, &new_rendered);
        s.gloss_overlay.apply_rewrite_diff(&ranges);
    }
```

Add a `pub fn buffer_text_for_diff(&self) -> String` to `GlossOverlay` returning the `gloss_view` buffer's full text (start..end), mirroring `overlay_search::gtk_ops::buffer_text`.

- [ ] **Step 4: Pass the diff at the custom-prompt call site; None everywhere else**

In `edit_gloss`'s `on_success` (gloss.rs:1438), capture the on-screen rendered text BEFORE dispatch and pass it in. At the top of `edit_gloss` (after the state read, gloss.rs:1385), capture:

```rust
    let prev_rendered = state_rc.borrow().gloss_overlay.buffer_text_for_diff();
```

`move` `prev_rendered` and `pasted_owned` (the custom prompt) into the closure, and change the call (gloss.rs:1438) to:

```rust
            update_and_render_gloss_in_place(
                st, &ctx, gloss_index, gloss_id, &verified_text, &model_for_db,
                &format!("GLOSS: edited {} gloss {} in place", gloss_type_owned, gloss_id),
                Some((&prev_rendered, Some(&pasted_owned))),
            );
```

Update the OTHER caller of `update_and_render_gloss_in_place` to pass `None`: the hand-edit `vim_save` at gloss.rs:1099 (the only other caller). It is EXCLUDED from the feature per Global Constraints.

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/gloss.rs src/ui/gloss_overlay.rs
git commit -m "feat(gloss): custom-prompt rewrite appends revision + diff-highlights

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 7: Clear the highlight on Escape / nav / close

**Files:**
- Modify: `src/input/keymap.rs` (gloss Escape ~1968, journal Escape ~1681)
- Modify: `src/input/actions/gloss.rs` (`navigate_gloss`, `close_gloss_to_reader`)
- Modify: `src/input/actions/journal.rs` (`close_overlay`; band/block nav entry)

**Interfaces:**
- Consumes: `clear_rewrite_diff` (Task 4).
- Produces: the diff-highlight is removed on Escape (first precedence, staying in the overlay), on navigating to another gloss/entry, and on overlay close.

**Precedence rule:** on Escape, clearing the diff-highlight comes FIRST — if a diff-highlight is active, Escape clears it and stays in the overlay (does not also clear search or close). Track "is a diff-highlight active" with a bool on each overlay so Escape knows whether it consumed something.

- [ ] **Step 1: Add an `active` flag to the overlays**

In both `GlossOverlay` and `JournalOverlay`, add `rewrite_diff_active: Cell<bool>` (default false). Set `true` at the end of `apply_rewrite_diff` (when `ranges` is non-empty), `false` in `clear_rewrite_diff`. Add `pub fn rewrite_diff_active(&self) -> bool`.

- [ ] **Step 2: Gloss Escape — clear first**

In `handle_gloss_key`'s `"Escape"` arm (keymap.rs:1968), make clearing the diff the first check:

```rust
        "Escape" => {
            if state.borrow().gloss_overlay.rewrite_diff_active() {
                state.borrow().gloss_overlay.clear_rewrite_diff();
            } else if crate::input::actions::gloss::clear_overlay_search(state) {
                // cleared a live search; stay in the overlay
            } else {
                crate::input::actions::gloss::close_gloss_to_reader(state);
            }
            true
        }
```

- [ ] **Step 3: Journal Escape — clear first**

In `handle_journal_key`'s `"Escape"` arm (keymap.rs:1681):

```rust
        "Escape" => {
            if state.borrow().journal_overlay.rewrite_diff_active() {
                state.borrow().journal_overlay.clear_rewrite_diff();
            } else if crate::input::actions::journal::clear_overlay_search(state) {
                // cleared a live search; stay
            } else if state.borrow().journal.filter.is_some() {
                crate::input::actions::journal::clear_filter(state);
            } else {
                crate::input::actions::journal::close_overlay(state);
            }
            true
        }
```

- [ ] **Step 4: Clear on navigation + close**

- In `gloss::navigate_gloss` (gloss.rs) and `gloss::close_gloss_to_reader`: call `s.gloss_overlay.clear_rewrite_diff();` at entry (before the move/close renders).
- In `journal::close_overlay` and the band/block-nav entry points (the `Ctrl+n/Ctrl+p` band nav at keymap.rs:2300 handlers → their action fns): call `s.journal_overlay.clear_rewrite_diff();`.

(Clearing on "another rewrite" is automatic: `apply_rewrite_diff` clears before applying.)

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs src/input/actions/journal.rs src/ui/gloss_overlay.rs src/ui/journal_overlay.rs
git commit -m "feat(overlays): clear rewrite diff-highlight on Escape/nav/close

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 8: History browsing (Ctrl+Shift+n/p) + restore (Ctrl+Shift+r)

**Files:**
- Modify: `src/input/keymap.rs` (add Ctrl+Shift arms in both overlay handlers; guard existing `Ctrl+n/p` arms)
- Create: `src/input/actions/rewrite_history.rs` (browse state + step + restore)
- Modify: `src/input/actions/mod.rs` (declare the module)
- Modify: `src/app/mod.rs` (AppState: `rewrite_browse: Option<RewriteBrowse>`)

**Interfaces:**
- Consumes: `list_revisions` (Task 3); `changed_ranges` (Task 1); `apply_rewrite_diff`/`clear_rewrite_diff` (Task 4); `update_journal_page`/`update_gloss`/`append_revision`.
- Produces:
  - `pub struct RewriteBrowse { pub kind: &'static str, pub entry_id: i64, pub revisions: Vec<Revision>, pub pos: usize /* index into a [rev0..revN, HEAD] virtual list */ }`
  - `pub fn browse_step(state, forward: bool)` — view-only; re-renders the version at the new position with its diff-highlight vs its predecessor; shows "rev k/N" cue.
  - `pub fn browse_restore(state)` — promote the viewed version to head (append current head as a revision first), re-render, exit browse.

**Design:** the virtual list is `[oldest_revision, …, newest_revision, HEAD]`. `Ctrl+Shift+p` steps toward older, `Ctrl+Shift+n` toward newer. Browsing renders read-only into the overlay buffer (reuse the surface's render path with the revision's `body`); it never writes. `browse_restore` writes the viewed body via the normal update fn after appending the current head.

- [ ] **Step 1: Declare state + module**

In `src/app/mod.rs` AppState, add:

```rust
    /// Active read-only revision browse, if the user is stepping history.
    pub rewrite_browse: Option<crate::input::actions::rewrite_history::RewriteBrowse>,
```

Initialize `rewrite_browse: None` in the AppState constructor (near mod.rs:1950 where `vim_rewrite: None` is set for journal — find the AppState literal and add the field). In `src/input/actions/mod.rs` add `pub mod rewrite_history;`.

- [ ] **Step 2: Implement the browse module**

Create `src/input/actions/rewrite_history.rs`:

```rust
//! Read-only browsing of a journal/gloss entry's stored rewrite revisions
//! (Ctrl+Shift+n/p) and restore of the viewed version (Ctrl+Shift+r). Browsing
//! NEVER mutates the live entry; only restore writes.

use std::cell::RefCell;
use std::rc::Rc;
use crate::app::AppState;
use crate::db::journal::Revision;

pub struct RewriteBrowse {
    pub kind: &'static str, // "journal" | "gloss"
    pub entry_id: i64,
    /// oldest → newest stored revisions (HEAD is a synthetic extra position).
    pub revisions: Vec<Revision>,
    /// position in [0..=revisions.len()]; == revisions.len() means HEAD.
    pub pos: usize,
    /// current live head body/question, for the HEAD position + diff baseline.
    pub head_question: Option<String>,
    pub head_body: String,
}

impl RewriteBrowse {
    fn len(&self) -> usize { self.revisions.len() + 1 } // + HEAD
    fn is_head(&self) -> bool { self.pos == self.revisions.len() }
    fn body_at(&self, i: usize) -> &str {
        if i == self.revisions.len() { &self.head_body } else { &self.revisions[i].body }
    }
}
```

Add `open_browse(state, kind, entry_id, head_question, head_body)` that loads `list_revisions`, sets `pos = len()-1` (HEAD), returns early with a toast "No revision history" when `revisions` is empty. Add `browse_step` and `browse_restore` per the interface, rendering via the surface's existing render path and calling `apply_rewrite_diff(changed_ranges(body_at(pos-1), body_at(pos)))`. For restore: `append_revision(head)` then `update_journal_page`/`update_gloss` with the viewed body, re-render, `state.rewrite_browse = None`, toast "Restored".

(Render path: for journal reuse `render_current`-style set_text with the revision body assembled as `Q: …\n\n{body}`; for gloss reuse `render_gloss_row`/`show_gloss_with_color` with the revision markup. Match the exact helper each overlay uses so the buffer offsets align with the diff.)

- [ ] **Step 3: Guard the existing Ctrl+n/p arms**

In `keymap.rs`, the library-picker and synopsis `"n"/"p" if is_ctrl` arms must not swallow Ctrl+Shift. Two cases:
- **Synopsis** (`handle_synopsis_overlay_key`, keymap.rs:2173, arm at :2300) ALREADY receives `is_shift` — change its arms to `"n" if is_ctrl && !is_shift =>` (and `p`).
- **Library picker** (`handle_picker_key`, keymap.rs:477, arm at :396) does NOT receive `is_shift` (signature is `(state, key_name, is_ctrl, is_alt, tokio_handle, mode)`). Thread `is_shift` through: add the param to `handle_picker_key`, pass it from the dispatch site (keymap.rs:217), then guard the `n`/`p` arms with `&& !is_shift`. (The picker isn't a rewrite surface, but the guard keeps Ctrl+Shift+n/p from being consumed there if a picker is somehow focused.)

Note: the GLOSS and JOURNAL overlay handlers (`handle_gloss_key`, `handle_journal_key`) already receive `is_shift` — no threading needed there.

- [ ] **Step 4: Add the Ctrl+Shift arms (both overlays)**

In `handle_gloss_key` and `handle_journal_key`, add (matching both raw and RPD shifted-glyph forms):

```rust
        "n" | "N" if is_ctrl && is_shift => {
            crate::input::actions::rewrite_history::browse_step(state, true);
            true
        }
        "p" | "P" if is_ctrl && is_shift => {
            crate::input::actions::rewrite_history::browse_step(state, false);
            true
        }
        "r" | "R" if is_ctrl && is_shift => {
            crate::input::actions::rewrite_history::browse_restore(state);
            true
        }
```

For the FIRST `Ctrl+Shift+n/p` in an overlay (no active browse yet), `browse_step` lazily calls `open_browse` for the current entry, then steps. Resolve the current entry: gloss → `gloss_list[gloss_index].gloss_id` + rendered head text; journal → `displayed_journal_page(&s)` id/question/answer.

- [ ] **Step 5: Build**

Run: `cargo build`
Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/input/actions/rewrite_history.rs src/input/actions/mod.rs src/app/mod.rs
git commit -m "feat(history): Ctrl+Shift+n/p browse revisions, Ctrl+Shift+r restore

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

### Task 9: Keybind legends + headless e2e verification

**Files:**
- Modify: `src/ui/gloss_keybinds_overlay.rs` (`GROUPS`)
- Modify: `src/ui/journal_keybinds_overlay.rs` (`GROUPS`)

**Interfaces:**
- Consumes: nothing new.
- Produces: the Ctrl+/ legends document the three new binds; e2e confirms on-screen behavior.

- [ ] **Step 1: Add legend entries (journal)**

In `src/ui/journal_keybinds_overlay.rs` `GROUPS`, in the "Editing" group (journal_keybinds_overlay.rs:23-30), add:

```rust
        ("Ctrl+Shift+n / Ctrl+Shift+p", "browse rewrite history"),
        ("Ctrl+Shift+r", "restore browsed version"),
```

- [ ] **Step 2: Add legend entries (gloss)**

In `src/ui/gloss_keybinds_overlay.rs` `GROUPS`, add the same two entries to the editing/rewrite group (match the group naming used there).

- [ ] **Step 3: Build + unit tests**

Run: `cargo build && cargo test`
Expected: build succeeds; all unit tests (incl. Tasks 1-3) pass.

- [ ] **Step 4: Headless e2e — rewrite diff-highlight + Escape**

Per the project's headless harness (CLAUDE.md "Headless Verification"), drive a journal Q&A custom-prompt rewrite and confirm on screen: (a) changed words are tinted after the rewrite, (b) Escape clears the tint and stays in the overlay, (c) `Ctrl+Shift+p` shows a prior version with its own tint, (d) `Ctrl+Shift+r` restores. Because the rewrite needs a live Claude call, verify the highlight/browse/restore mechanics against a stored entry with existing revisions (or seed `rewrite_revisions` rows in a scratch DB copy) rather than a live rewrite. Open every capture and report what is on screen (UI review protocol).

Run (adjust binds per `keymap_config.rs`):

```bash
cd ~/utono/linux-lit && cargo build
LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  LIT_DEV=1 XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

Cleanup: `pkill -f "cage -- ./target/debug/linux-lit"`.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_keybinds_overlay.rs src/ui/journal_keybinds_overlay.rs
git commit -m "docs(keybinds): legend entries for rewrite history browse/restore

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01V7jHBBodfVJjsL3HxnphLi"
```

---

## Self-Review

**Spec coverage:**
- Trigger = custom-prompt only → Global Constraints + Task 5/6 pass `Some(diff)` only at the custom-prompt call sites, `None` elsewhere. ✓
- Durable full history in lit.db, litdb-owned, auto-migrated → Task 2. ✓
- Append prior version on each rewrite → Task 5 (journal), Task 6 (gloss). ✓
- Word-level diff on rendered text, char offsets → Task 1 (core) + Task 5/6 (rendered-text sourcing + offset mapping). ✓
- Highlight reuses search color, distinct tag → Task 4. ✓
- Clear on Escape (first precedence)/nav/close/next-rewrite → Task 7 (+ auto-clear in `apply`). ✓
- Ctrl+Shift+n/p browse (view-only) → Task 8. ✓
- Ctrl+Shift+r restore (append head first) → Task 8. ✓
- Legends updated → Task 9. ✓
- RPD shifted-glyph forms + Ctrl+n/p guard → Task 8 Steps 3-4. ✓

**Placeholder scan:** Two spots intentionally defer to on-file verification rather than guess: the exact journal body prefix (Task 5 Step 2 — verify against `journal_doc`/`show_page`) and the exact per-surface render helper for browsing (Task 8 Step 2). These are "confirm the existing helper" instructions with the fallback named, not unfilled placeholders. The gloss revision-append in Task 6 Step 2 shows the real capture code (the `None::<&()>` placeholder line is explicitly replaced in the same step).

**Type consistency:** `changed_ranges(&str,&str)->Vec<(i32,i32)>`, `append_revision`/`list_revisions`/`Revision`, and `apply_rewrite_diff(&[(i32,i32)])`/`clear_rewrite_diff`/`rewrite_diff_active` are used with identical signatures across Tasks 1-8. ✓

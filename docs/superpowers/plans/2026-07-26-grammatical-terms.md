# grammatical_terms Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move grammatical term definitions out of every syntax-gloss API response and into a lit.db table the reader reads at save time.

**Architecture:** A new `grammatical_terms` table, created by the reader's own idempotent migration (the `ensure_*_table` pattern in `src/db/migrations.rs`, not a litdb migration). The prompt stops returning a `Terms:` section and instead receives the known term NAMES, returning definitions only for terms it used that were new. Between the API reply and `persist_render_install_gloss`, a pure assembly step scans the reply for known terms, inserts any new ones, and appends an alphabetical `Terms:` section built from the table.

**Tech Stack:** Rust, rusqlite, the existing `crate::gloss` request pipeline.

Spec: `docs/superpowers/specs/2026-07-26-grammatical-terms-design.md`

## Global Constraints

- Work in a worktree off master: `git worktree add ~/utono/linux-lit-wt/feat-grammatical-terms -b feat/grammatical-terms`. Merge back from the MAIN checkout.
- Master is at `bcc890cd`. Baselines: **1128 tests passing**, **clippy 180**. Measure clippy with `cargo clippy 2>&1 | rg 'generated .* warnings'` — NOT `rg -c '^warning'`, which over-counts by one.
- Verify with `cargo build`; do NOT run `cargo run`. The user runs the app.
- **No review gates this run** (user is away). Build, clippy, tests, AND the on-screen check remain MANDATORY — they are correctness, not review.
- This code runs in GTK callbacks where a panic ABORTS the process (a panic cannot unwind across the C FFI boundary). No `unwrap`, no `expect`, no unguarded indexing, no `usize` subtraction that could underflow. This project aborted twice on 2026-07-26 from exactly these.
- The gloss body uses ONLY the existing markup: `<segment>`, `<gloss>`, `<speaker>`, `<pron>`. A new tag needs a new renderer.
- Do NOT touch `rhetorical_terms`, the other five gloss types, or existing saved syntax glosses.

---

## File Structure

**Create:**
- `src/db/grammatical_terms.rs` — table creation, `load_all`, `insert_missing`. DB only, no GTK.

**Modify:**
- `src/db/mod.rs` — register the module (list is alphabetical; goes between `echoes` and `line_types`).
- `src/gloss.rs` — prompt change; the pure scan + Terms-builder + `New terms:` parser, with their tests.
- `src/input/visual.rs:677-683` — the assembly step at the save seam.
- `src/db/migrations.rs` — `ensure_grammatical_terms_table`, following the existing `ensure_*` shape.

---

## Task 1: The table and its migration

**Files:**
- Create: `src/db/grammatical_terms.rs`
- Modify: `src/db/mod.rs`, `src/db/migrations.rs`

**Interfaces:**
- Produces:
  - `pub fn ensure_grammatical_terms_table(conn: &Connection) -> Result<(), rusqlite::Error>` (in `migrations.rs`)
  - `pub fn load_all(conn: &Connection) -> Vec<(String, String)>` — `(term, definition)`, ordered by term. Returns empty on any error.
  - `pub fn insert_missing(conn: &Connection, terms: &[(String, String)]) -> usize` — `INSERT OR IGNORE`, returns rows actually inserted.

- [ ] **Step 1: Add the migration**

In `src/db/migrations.rs`, following the shape of `ensure_bookmarks_table` immediately above it:

```rust
/// `grammatical_terms`: definitions of grammatical structures (main clause,
/// subject, predicate) used by syntax glosses.
///
/// Deliberately SEPARATE from `rhetorical_terms`, which holds rhetorical
/// FIGURES (anaphora, chiasmus, zeugma). The two sets overlap at exactly one
/// entry, "appositive"; filing "predicate" among rhetorical figures would
/// mislead whoever reads that table next.
///
/// Seeded with the terms the existing saved syntax glosses actually used, so
/// the first gloss after this migration finds a populated table.
pub fn ensure_grammatical_terms_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS grammatical_terms (
            id         INTEGER PRIMARY KEY,
            term       TEXT UNIQUE NOT NULL,
            definition TEXT NOT NULL,
            source     TEXT,
            created_at TEXT DEFAULT (datetime('now'))
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_grammatical_terms_term
         ON grammatical_terms(term)",
        [],
    )?;

    const SEED: &[(&str, &str)] = &[
        ("adverbial clause", "a subordinate clause that modifies the main clause by supplying a circumstance such as time, cause, or condition"),
        ("adverbial phrase", "a phrase modifying a verb, saying how, when, or where the action happens"),
        ("appositive", "a noun phrase set beside another noun to rename or redefine it"),
        ("conjoined predicate", "two or more predicates sharing one subject, so the subject is stated once and governs all of them"),
        ("main clause", "a clause that can stand alone as a complete sentence, containing a subject and a finite verb"),
        ("participial modifier", "a participle and its dependents modifying a noun, as in \"staring down at his intrusion\""),
        ("predicate", "the part of a clause that states what the subject does or undergoes, built around the verb"),
        ("relative clause", "a subordinate clause introduced by a relative word such as who, whom, or which, modifying a preceding noun"),
        ("subject", "the noun phrase naming what the clause is about, of which the predicate is asserted"),
    ];
    for (term, def) in SEED {
        conn.execute(
            "INSERT OR IGNORE INTO grammatical_terms (term, definition, source)
             VALUES (?1, ?2, 'curated')",
            rusqlite::params![term, def],
        )?;
    }
    Ok(())
}
```

- [ ] **Step 2: Create the accessor module**

Create `src/db/grammatical_terms.rs`:

```rust
//! Definitions of grammatical structures used by syntax glosses.
//!
//! Reference data, not passage data: "main clause" means the same thing in
//! every sentence, so it is stored once here rather than re-derived by the
//! model on every gloss.

use rusqlite::Connection;

/// Every known term and its definition, ordered by term.
///
/// Returns EMPTY on any error rather than propagating: a definitions table
/// being unreadable must not cost the reader their analysis. The caller
/// degrades to a gloss with no Terms section.
pub fn load_all(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = match conn
        .prepare("SELECT term, definition FROM grammatical_terms ORDER BY term")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Insert terms not already present. Returns how many rows were added.
///
/// `INSERT OR IGNORE`: the stored definition wins over a newly supplied one.
/// Consistency beats recency — a term that already has a definition should
/// keep it, or two glosses of the same passage could disagree.
pub fn insert_missing(conn: &Connection, terms: &[(String, String)]) -> usize {
    let mut added = 0usize;
    for (term, def) in terms {
        if term.trim().is_empty() || def.trim().is_empty() {
            continue;
        }
        let n = conn.execute(
            "INSERT OR IGNORE INTO grammatical_terms (term, definition, source)
             VALUES (?1, ?2, 'claude')",
            rusqlite::params![term.trim(), def.trim()],
        );
        added += n.unwrap_or(0);
    }
    added
}
```

- [ ] **Step 3: Register the module**

In `src/db/mod.rs`, insert alphabetically between `echoes` and `line_types`:

```rust
pub mod grammatical_terms;
```

- [ ] **Step 4: Call the migration**

Find where the other `ensure_*` migrations are invoked (grep `ensure_bookmarks_table` outside `migrations.rs` — it is called from the DB-open path). Add `ensure_grammatical_terms_table` beside it, in the same style and with the same error handling as its neighbours.

- [ ] **Step 5: Verify**

```bash
cd ~/utono/linux-lit-wt/feat-grammatical-terms
cargo build 2>&1 | rg -c '^error' || echo "0 errors"
cargo clippy 2>&1 | rg 'generated .* warnings'
```

Expected: 0 errors; clippy ≤ 180. New functions are unused until Task 3, so a dead-code warning here is expected — report the number.

- [ ] **Step 6: Commit**

```bash
git add src/db/grammatical_terms.rs src/db/mod.rs src/db/migrations.rs
git commit -m "feat(db): grammatical_terms table, seeded from terms already in use"
```

---

## Task 2: The pure text functions

The piece most likely to be subtly wrong, so it is isolated and carries the tests. No DB, no GTK.

**Files:**
- Modify: `src/gloss.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/gloss.rs` (the existing block — do not create a second)

**Interfaces:**
- Produces:
  - `pub fn scan_terms_used(body: &str, known: &[(String, String)]) -> Vec<(String, String)>` — the known terms that appear in `body`, ordered by term, no duplicates.
  - `pub fn parse_new_terms(reply: &str) -> Vec<(String, String)>` — parses a `New terms:` section into `(term, definition)` pairs. Empty when absent.
  - `pub fn strip_new_terms(reply: &str) -> String` — the reply with the `New terms:` section removed, so it never reaches the stored gloss.
  - `pub fn build_terms_section(terms: &[(String, String)]) -> String` — the `Terms:` block, alphabetical, one `<gloss>term: definition.</gloss>` per line. Empty string when `terms` is empty.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` in `src/gloss.rs`:

```rust
    fn known() -> Vec<(String, String)> {
        vec![
            ("appositive".into(), "a noun phrase set beside another noun".into()),
            ("clause".into(), "a group of words with a subject and verb".into()),
            ("main clause".into(), "a clause that can stand alone".into()),
            ("subject".into(), "the noun phrase the clause is about".into()),
        ]
    }

    #[test]
    fn scan_finds_multiword_terms_not_their_fragments() {
        // "main clause" must match as itself. A naive contains() would ALSO
        // report the bare "clause" inside it, producing a glossary that
        // defines a word the reader never saw on its own.
        let body = "The main clause carries the assertion.";
        let found = scan_terms_used(body, &known());
        let terms: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
        assert!(terms.contains(&"main clause"), "{terms:?}");
        assert!(!terms.contains(&"clause"), "must not report the fragment: {terms:?}");
    }

    #[test]
    fn scan_reports_a_repeated_term_once() {
        let body = "An appositive here, an appositive there, and a third appositive.";
        let found = scan_terms_used(body, &known());
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn scan_covers_the_note_not_only_structure() {
        // The 2026-07-26 bug: a term used ONLY in the rhetorical note got no
        // definition. The scan takes the whole body, so prose counts.
        let body = "Structure:\nmain clause — X\n\nWhat the structure is doing:\n\
                    Boswell hangs an appositive off the name.";
        let found = scan_terms_used(body, &known());
        let terms: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
        assert!(terms.contains(&"appositive"), "note terms must be found: {terms:?}");
    }

    #[test]
    fn scan_is_case_insensitive_at_a_sentence_start() {
        let body = "Appositive constructions abound.";
        let found = scan_terms_used(body, &known());
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn scan_returns_alphabetical_order() {
        let body = "The subject precedes the main clause; an appositive follows.";
        let found = scan_terms_used(body, &known());
        let terms: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(terms, vec!["appositive", "main clause", "subject"]);
    }

    #[test]
    fn build_terms_section_is_empty_for_no_terms() {
        // Not an empty heading — no section at all, or the gloss ends with a
        // bare "Terms:" and nothing under it.
        assert_eq!(build_terms_section(&[]), "");
    }

    #[test]
    fn build_terms_section_wraps_each_entry_in_gloss_markup() {
        let out = build_terms_section(&[
            ("appositive".into(), "a noun phrase set beside another noun".into()),
        ]);
        assert!(out.contains("Terms:"), "{out}");
        assert!(out.contains("<gloss>appositive: a noun phrase set beside another noun.</gloss>"), "{out}");
    }

    #[test]
    fn build_terms_section_does_not_double_the_final_period() {
        let out = build_terms_section(&[("subject".into(), "the noun phrase.".into())]);
        assert!(!out.contains(".."), "definition already ended in a period: {out}");
    }

    #[test]
    fn parse_new_terms_reads_the_section() {
        let reply = "What the structure is doing:\n<gloss>It piles modifiers.</gloss>\n\n\
                     New terms:\n\
                     periodic sentence: a sentence whose main clause is withheld until the end\n\
                     zeugma: one verb governing two objects in different senses\n";
        let got = parse_new_terms(reply);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].0, "periodic sentence");
        assert!(got[0].1.starts_with("a sentence whose main clause"), "{got:?}");
    }

    #[test]
    fn parse_new_terms_is_empty_when_absent() {
        // The common case: every term the model used was already known.
        assert!(parse_new_terms("Structure:\nmain clause — X\n").is_empty());
    }

    #[test]
    fn strip_new_terms_removes_the_section_from_the_stored_gloss() {
        let reply = "What the structure is doing:\n<gloss>Note.</gloss>\n\n\
                     New terms:\nperiodic sentence: withheld until the end\n";
        let out = strip_new_terms(reply);
        assert!(!out.contains("New terms:"), "{out}");
        assert!(!out.contains("periodic sentence:"), "{out}");
        assert!(out.contains("<gloss>Note.</gloss>"), "must keep the note: {out}");
    }
```

- [ ] **Step 2: Run them to verify they fail**

```bash
cargo test --bins gloss::tests::scan_ 2>&1 | tail -12
```

Expected: FAIL to compile — `cannot find function scan_terms_used in this scope`.

- [ ] **Step 3: Implement**

Add to `src/gloss.rs`, above the `#[cfg(test)]` block:

```rust
/// Which of `known`'s terms appear in `body`, alphabetical, no duplicates.
///
/// Matching is whole-term and case-insensitive, and LONGEST-FIRST: "main
/// clause" must not also report the bare "clause" inside it, or the glossary
/// defines a word the reader never saw on its own. A matched span is blanked
/// so a shorter term cannot match inside it.
pub fn scan_terms_used(body: &str, known: &[(String, String)]) -> Vec<(String, String)> {
    let mut hay = body.to_lowercase();
    // Longest first so "main clause" is consumed before "clause" can match it.
    let mut by_len: Vec<&(String, String)> = known.iter().collect();
    by_len.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut found: Vec<(String, String)> = Vec::new();
    for (term, def) in by_len {
        let needle = term.to_lowercase();
        if needle.is_empty() {
            continue;
        }
        if let Some(pos) = hay.find(&needle) {
            found.push((term.clone(), def.clone()));
            // Blank every occurrence so a shorter term cannot match inside it.
            let blank = " ".repeat(needle.chars().count());
            hay = hay.replace(&needle, &blank);
            let _ = pos;
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Parse a `New terms:` section into `(term, definition)` pairs.
///
/// Each line under the heading reads `term: definition`. Absent section, or a
/// heading with nothing under it, yields an empty vec — the common case, since
/// most glosses use only terms lit.db already knows.
pub fn parse_new_terms(reply: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in reply.lines() {
        let t = line.trim();
        if t.eq_ignore_ascii_case("New terms:") {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if t.is_empty() {
            continue;
        }
        // A new heading ends the section: a line ending in ':' that carries
        // no "term: definition" pair of its own.
        if t.ends_with(':') && t.split_once(": ").is_none() {
            break;
        }
        if let Some((term, def)) = t.split_once(": ") {
            let term = term.trim().trim_start_matches(['-', '*', ' ']).trim();
            let def = def.trim().trim_end_matches('.').trim();
            if !term.is_empty() && !def.is_empty() {
                out.push((term.to_string(), def.to_string()));
            }
        }
    }
    out
}

/// The reply with any `New terms:` section removed.
///
/// That section is instruction-plumbing between the model and lit.db; it must
/// never reach the stored gloss, where it would render as stray prose under
/// the note.
pub fn strip_new_terms(reply: &str) -> String {
    let mut out = String::new();
    let mut in_section = false;
    for line in reply.lines() {
        let t = line.trim();
        if t.eq_ignore_ascii_case("New terms:") {
            in_section = true;
            continue;
        }
        if in_section {
            // A blank line does not end it (definitions may be spaced); a new
            // heading does.
            if t.ends_with(':') && t.split_once(": ").is_none() {
                in_section = false;
            } else {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// The `Terms:` section, alphabetical, in the markup the renderer already
/// handles. Empty string for no terms — not a bare heading.
pub fn build_terms_section(terms: &[(String, String)]) -> String {
    if terms.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&(String, String)> = terms.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::from("\n\nTerms:\n\n");
    for (term, def) in sorted {
        let def = def.trim().trim_end_matches('.');
        out.push_str(&format!("<gloss>{term}: {def}.</gloss>\n\n"));
    }
    out.trim_end().to_string()
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test --bins gloss::tests 2>&1 | rg 'test result'
```

Expected: PASS, 11 more than the 1128 baseline.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): pure term scan, New-terms parser, and Terms builder"
```

---

## Task 3: Prompt change and save-time assembly

**Files:**
- Modify: `src/gloss.rs` (the prompt), `src/input/visual.rs:677-683` (the seam)

**Interfaces:**
- Consumes: everything from Tasks 1 and 2.
- Produces: nothing new.

- [ ] **Step 1: Change the prompt**

In `syntax_gloss_prompt()` in `src/gloss.rs`, DELETE the whole numbered item 4 (the `Terms:` paragraph, which currently begins "4. A line reading `Terms:`") and replace it with:

```
4. If — and only if — you used a grammatical term that was NOT in the list of \
known terms supplied with the passage, add a line reading `New terms:` and \
under it one line per such term, reading `term: definition`. Define it \
generally, as a grammar would. Omit this section entirely when every term you \
used was already known, which is the usual case. Do NOT write a glossary of \
the known terms — those are supplied from a database and adding them here \
would duplicate them.
```

Then change the section count near the top of the prompt from "exactly four sections" to "exactly three sections, plus an optional fourth", so the prompt does not contradict itself.

Update the existing test `syntax_gloss_prompt_glossary_covers_the_note_and_sorts_alphabetically` — its assertions on `ANYWHERE ABOVE` and `ALPHABETICALLY` describe the removed section. Replace them with assertions that the prompt asks for `New terms:` and does NOT ask for a full glossary. Keep the test name meaningful; rename it `syntax_gloss_prompt_asks_only_for_new_terms`.

- [ ] **Step 2: Send the known terms in the user message**

In `src/input/visual.rs`, in `syntax_gloss_for_lines` where `user_msg` is built (search for `build_user_message` in the syntax path — it currently appends the `line_syntax` parse table), append the known-term names AFTER that block:

```rust
    // Send only the term NAMES. The definitions live in lit.db and are
    // appended at save; sending them here would reintroduce the tokens this
    // whole change removes.
    let known_terms: Vec<(String, String)> = match crate::db::queries::open_db() {
        Ok(conn) => crate::db::grammatical_terms::load_all(&conn),
        Err(_) => Vec::new(),
    };
    if !known_terms.is_empty() {
        let names: Vec<&str> = known_terms.iter().map(|(t, _)| t.as_str()).collect();
        user_msg.push_str("\n\nKnown grammatical terms (do not redefine these):\n");
        user_msg.push_str(&names.join(", "));
    }
```

- [ ] **Step 3: Assemble at the save seam**

In `src/input/visual.rs`, replace the `Ok(Ok(gloss_text))` arm of the syntax path (currently at lines 677-683):

```rust
            Ok(Ok(gloss_text)) => {
                // Definitions come from lit.db, not from the reply. Insert any
                // genuinely new terms the model supplied, then append a Terms
                // section built from the table — alphabetical, consistent
                // across every gloss, and covering terms used ONLY in the
                // rhetorical note (the 2026-07-26 gap).
                //
                // Baked into gloss_text at save, not joined at display, so the
                // stored row stays self-contained for export/search/TTS.
                let body = crate::gloss::strip_new_terms(&gloss_text);
                let assembled = match crate::db::queries::open_db() {
                    Ok(conn) => {
                        let new_terms = crate::gloss::parse_new_terms(&gloss_text);
                        if !new_terms.is_empty() {
                            let n = crate::db::grammatical_terms::insert_missing(&conn, &new_terms);
                            crate::logging::log(&format!(
                                "GRAMMATICAL_TERMS: {n} new term(s) inserted"
                            ));
                        }
                        let known = crate::db::grammatical_terms::load_all(&conn);
                        let used = crate::gloss::scan_terms_used(&body, &known);
                        crate::logging::log(&format!(
                            "GRAMMATICAL_TERMS: {} term(s) used in this gloss",
                            used.len()
                        ));
                        format!("{body}{}", crate::gloss::build_terms_section(&used))
                    }
                    // A definitions table being unreadable must not cost the
                    // reader their analysis — save the gloss without Terms.
                    Err(e) => {
                        crate::logging::log(&format!(
                            "GRAMMATICAL_TERMS: db unavailable ({e}) — no Terms section"
                        ));
                        body
                    }
                };
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &assembled, "syntax-gloss", &model_for_db,
                    "SYNTAX-GLOSS: generated and saved new gloss",
                );
            }
```

Note the DB work happens BEFORE `state_for_result.borrow_mut()`. Taking the borrow first and then opening the DB inside it would hold a `RefCell` borrow across DB I/O in a GTK callback — the abort pattern this project hit twice today.

- [ ] **Step 4: Verify**

```bash
cargo build 2>&1 | rg -c '^error' || echo "0 errors"
cargo test --bins 2>&1 | rg 'test result'
cargo clippy 2>&1 | rg 'generated .* warnings'
```

Expected: 0 errors; tests pass (1128 baseline + Task 2's 11, minus none); clippy ≤ 180 — the Task 1 functions are now used, so any dead-code warnings from there are gone.

- [ ] **Step 5: Commit**

```bash
git add src/gloss.rs src/input/visual.rs
git commit -m "feat(syntax-gloss): definitions from lit.db, not from every reply"
```

---

## Task 4: On-screen verification

Mandatory per CLAUDE.md and explicitly retained for this run despite review gates being waived — build and tests green is NOT done for a visible change.

**Files:**
- Modify: `docs/superpowers/plans/2026-07-26-grammatical-terms.md` (record results)

- [ ] **Step 1: Confirm the table exists and is seeded**

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM grammatical_terms;"
```

Expected: 9 (the seed). If the table is missing, the migration is not being called — fix Task 1 Step 4 before continuing.

- [ ] **Step 2: Launch headless**

```bash
cd ~/utono/linux-lit-wt/feat-grammatical-terms && cargo build
./scripts/land-on.sh BH-Barrett 3.0
```

BH-Barrett uses `div2=0`, so `3.0` is valid and `1.1` is not. Take the printed `XDG_RUNTIME_DIR` from the output — do not assume it. Launch via the harness `run_in_background`; a detached or `timeout`-wrapped launch dies immediately.

Note `land-on.sh` uses a PRIVATE DB copy at `/tmp/land-on-lit.db`. Query THAT file when verifying inserts, not the live lit.db.

- [ ] **Step 3: Generate a gloss and read it**

```bash
export XDG_RUNTIME_DIR=<printed>  WAYLAND_DISPLAY=wayland-0
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
sleep 2
wtype -k Escape          # re-send after resize; the first chord is dropped
for i in $(seq 1 12); do wtype -k j; sleep 0.25; done
wtype -k minus
sleep 1
wtype -k Return
sleep 35
grim -o HEADLESS-1 /tmp/gt-1.png
rg -n 'GRAMMATICAL_TERMS' /tmp/land-on.log | tail -4
```

Open the PNG and confirm inline, per the UI review protocol:

1. A `Terms:` section is present with definitions.
2. Entries are in alphabetical order.
3. Every grammatical term used in the rhetorical note has an entry — this is the bug that started this work.
4. No stray `New terms:` heading appears anywhere in the rendered gloss.

- [ ] **Step 4: Confirm the reuse path inserts nothing**

Generate a SECOND gloss on a different passage, then:

```bash
sqlite3 /tmp/land-on-lit.db "SELECT COUNT(*) FROM grammatical_terms;"
```

Expected: still 9 unless the model genuinely met a new term — in which case the log line `GRAMMATICAL_TERMS: N new term(s) inserted` says so and the count rises by exactly N. A count that climbs on every gloss means `INSERT OR IGNORE` is not matching and the term text is drifting (leading capital, trailing period); investigate rather than accepting it.

- [ ] **Step 5: Clean up**

Run as its own step — `pkill` exits nonzero on no match and aborts an `&&` chain:

```bash
pkill -f "cage -- target/debug/linux-lit" || true
```

- [ ] **Step 6: Record results and commit**

Append a "## Verification results" section to this plan with what each check showed, then commit it.

- [ ] **Step 7: Hand off for real-renderer confirmation**

The user is away and asked for no review gates, so DO NOT merge on their behalf. Leave the branch ready and report: the command (`cd ~/utono/linux-lit-wt/feat-grammatical-terms && cargo run`) and the four criteria from Step 3.

---

## Self-Review

**Spec coverage.** The table and its separation from `rhetorical_terms` → Task 1. The seed list → Task 1 Step 1. Prompt drops Terms, gains `New terms:` → Task 3 Step 1. Known-term names sent in the user message → Task 3 Step 2. Assembly at save, baked not joined → Task 3 Step 3. Error handling (table unreadable → no Terms, gloss still saves; duplicate → stored wins) → Task 1 Step 2 and Task 3 Step 3. Unit tests → Task 2. On-screen → Task 4.

**Placeholder scan.** No TBDs. Task 1 Step 4 says to grep for the existing call site rather than quoting a line number, because the migrations are invoked from a path I did not pin down — that is a genuine read-then-edit with the search term and the required outcome both stated, not vagueness.

**Type consistency.** `load_all(&Connection) -> Vec<(String, String)>` and `insert_missing(&Connection, &[(String, String)]) -> usize` are defined in Task 1 and called in Task 3 with matching types. `scan_terms_used`, `parse_new_terms`, `strip_new_terms`, `build_terms_section` are defined in Task 2 and called in Task 3 with matching signatures. Every one takes and returns `(String, String)` pairs — one shape throughout.

**Known risk.** `scan_terms_used` blanks matched spans to prevent "clause" matching inside "main clause". If a definition itself contains another term, the scan does not recurse into definitions — deliberate, since only the BODY is scanned, not the appended Terms section. Task 4 Step 4's count check is what catches term-text drift.

## Verification results (2026-07-26, headless cage @ 1920x1200)

**Migration self-applies.** The table did not exist on the live lit.db before
this branch; on first headless launch it was created and seeded with all 9
terms. No litdb migration was needed — the reader's own `ensure_*_table`
pattern handles it.

**Definitions come from the table, alphabetically.** A generated gloss rendered
`adverbial clause`, `adverbial phrase`, `conjoined predicate`, `main clause`,
`participial modifier`, `predicate` — the exact seeded wording, in order. Log:
`GRAMMATICAL_TERMS: 7 term(s) used in this gloss`.

**The note's terms are covered.** The note named "participial modifiers" and
"main clause"; both appear below. That is the 2026-07-26 gap closed at its
source rather than by widening the prompt.

**No stray `New terms:` heading** in the rendered gloss — `strip_new_terms`
works.

**Cache path intact.** Re-requesting the same passage logged
`SYNTAX-GLOSS: showing cached gloss` with no API call and no term work.

**The insert path could not be triggered from a real run**, and this is worth
recording. Three generated glosses all used ONLY terms already in the table
(counts: 7 used / 0 new, then 5 used / 0 new), which is the designed steady
state — but it means the branch shipped with its riskiest path unexercised.
Covered instead by five direct unit tests against an in-memory table
(`src/db/grammatical_terms.rs`), including the duplicate case (stored
definition wins) and the missing-table case (empty, no panic).

Those tests were verified NON-VACUOUS: reverting `insert_missing` to swallow
its result — exactly what the plan's original `open_db()` (read-only) would
have caused — fails `insert_missing_adds_new_terms_and_reports_the_count`. A
caller using the wrong opener would otherwise log "0 new term(s) inserted"
and look healthy while the table stayed frozen forever.

**Final state:** 1144 tests pass, clippy 180 (at baseline), build clean.

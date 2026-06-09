# Age-aware default voice (Phase 2) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the gloss-TTS *default* voice (when a gloss has no manually-associated voices) depend on the speaking character's **(gender, age)** — picking among the four narration voice pairs by age band — instead of gender alone.

**Architecture:** A new `voice_catalog(voice_id, model_id, gender, age_min, age_max, role)` table, seeded with the four voice pairs and their age bands, is queried by a new `resolve_default_voice(conn, work_abbrev, speaker, is_verse)` (containment band → nearest band → `voice_for` last-resort). `characters` gains a nullable `age` column (LLM-curated by the renamed `curate_characters.py`). `play_block_tts`'s empty-set default branch swaps its single `voice_for(...)` call to `resolve_default_voice(...)`. The per-gloss voice override and the gender-only `voice_for` (now the last-resort fallback) are unchanged.

**Tech Stack:** Rust (rusqlite, `cargo test --bins` — binary-only crate; rare parallel flake → use `--test-threads=1`), SQLite (`~/utono/litdb/data/lit.db`), Python (`scripts/`, anthropic SDK).

**Spec:** `docs/superpowers/specs/2026-06-08-per-gloss-voice-set-design.md` §2 (Phase 2).

**The seed catalog (4 pairs × verse/prose = 8 rows), age bands per the user:**
- **Will** (young male) 15–25: verse `A_OP_VOICE_ID` / prose `B_VOICE_ID`
- **Petruchio** (older male) 35–45: verse `C_OP_VOICE_ID` / prose `D_VOICE_ID`
- **Willa** (young female) 12–19: verse `A_OP_F_VOICE_ID` / prose `B_F_VOICE_ID`
- **Beatrice** (female) 20–30: verse `E_OP_VOICE_ID` / prose `F_VOICE_ID`
- model for all: `OP_MODEL_ID` (`eleven_v3`).

Ages outside every band resolve to the **nearest same-gender band** (e.g. an 80-yr-old male → Petruchio; a 5-yr-old female → Willa).

---

## Task 1: `voice_catalog` table + seed

**Files:**
- Modify: `src/db/queries.rs` (add `ensure_voice_catalog_table`; tests)
- Modify: `src/app.rs` (wire into `BOOKMARKS_INIT.call_once`, ~line 2474)

- [ ] **Step 1: Write the failing test** — add to the `#[cfg(test)] mod tests` at the bottom of `src/db/queries.rs`:

```rust
    #[test]
    fn voice_catalog_seeds_four_pairs() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_voice_catalog_table(&conn).unwrap();
        // 8 rows: 4 pairs x verse/prose
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 8);
        // Petruchio prose (older male) is present with its band
        let (vid, lo, hi): (String, i64, i64) = conn
            .query_row(
                "SELECT voice_id, age_min, age_max FROM voice_catalog \
                 WHERE gender='male' AND role='prose' AND age_min=35",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(vid, crate::elevenlabs::D_VOICE_ID);
        assert_eq!((lo, hi), (35, 45));
        // idempotent: a second ensure does not duplicate rows
        ensure_voice_catalog_table(&conn).unwrap();
        let n2: i64 = conn
            .query_row("SELECT COUNT(*) FROM voice_catalog", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n2, 8);
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins voice_catalog_seeds`
Expected: FAIL — `cannot find function ensure_voice_catalog_table`.

- [ ] **Step 3: Implement** — add to `src/db/queries.rs` after `ensure_gloss_voices_table` (~line 522):

```rust
/// Ensure the voice catalog exists and is seeded with the four narration voice
/// pairs and their age bands. Used by `resolve_default_voice` to pick the
/// default voice by (gender, age). Seeding is idempotent (INSERT OR IGNORE on
/// the (voice_id, role) PK). The user can later add/adjust rows. See the
/// per-gloss-voice-set spec §2.1.
pub fn ensure_voice_catalog_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    use crate::elevenlabs::*;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS voice_catalog (
            voice_id  TEXT NOT NULL,
            model_id  TEXT NOT NULL,
            gender    TEXT NOT NULL,
            age_min   INTEGER NOT NULL,
            age_max   INTEGER NOT NULL,
            role      TEXT NOT NULL,
            label     TEXT,
            PRIMARY KEY (voice_id, role)
        );"
    )?;
    // Seed the four pairs (verse + prose each). INSERT OR IGNORE keeps it
    // idempotent and lets a user-edited row survive a re-run.
    let seed: [(&str, &str, i64, i64, &str, &str); 8] = [
        (A_OP_VOICE_ID, "male", 15, 25, "verse", "Will OP — young male verse"),
        (B_VOICE_ID,    "male", 15, 25, "prose", "Will — young male prose"),
        (C_OP_VOICE_ID, "male", 35, 45, "verse", "Petruchio OP — older male verse"),
        (D_VOICE_ID,    "male", 35, 45, "prose", "Petruchio — older male prose"),
        (A_OP_F_VOICE_ID, "female", 12, 19, "verse", "Willa OP — young female verse"),
        (B_F_VOICE_ID,    "female", 12, 19, "prose", "Willa — young female prose"),
        (E_OP_VOICE_ID, "female", 20, 30, "verse", "Beatrice OP — female verse"),
        (F_VOICE_ID,    "female", 20, 30, "prose", "Beatrice — female prose"),
    ];
    for (vid, gender, lo, hi, role, label) in seed {
        conn.execute(
            "INSERT OR IGNORE INTO voice_catalog
             (voice_id, model_id, gender, age_min, age_max, role, label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![vid, OP_MODEL_ID, gender, lo, hi, role, label],
        )?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins voice_catalog_seeds`
Expected: PASS.

- [ ] **Step 5: Wire into startup** — in `src/app.rs`, the `BOOKMARKS_INIT.call_once` block (~line 2474, after `ensure_gloss_voices_table`):

```rust
            let _ = crate::db::queries::ensure_voice_catalog_table(&conn);
```

- [ ] **Step 6: Build + full tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean; all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/db/queries.rs src/app.rs
git commit -m "feat(db): voice_catalog table seeded with the four voice pairs + age bands"
```

---

## Task 2: `characters.age` column (migration)

**Files:**
- Modify: `src/db/queries.rs` (`ensure_characters_table` — add `age` to fresh DDL + ALTER-if-missing migration; tests)

- [ ] **Step 1: Write the failing test** — add to the queries test module:

```rust
    #[test]
    fn characters_table_has_age_column() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        // age column exists and is nullable
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','GHOST','male')",
            [],
        ).unwrap();
        let age: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='HAMLET'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(age, Some(30));
        let none: Option<i64> = conn
            .query_row("SELECT age FROM characters WHERE speaker='GHOST'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn characters_table_migrates_legacy_no_age() {
        let conn = Connection::open_in_memory().unwrap();
        // legacy 3-column table (pre-age) with a row
        conn.execute_batch(
            "CREATE TABLE characters (
                work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,
                PRIMARY KEY (work_abbrev, speaker));
             INSERT INTO characters VALUES ('Ham','HAMLET','male');",
        ).unwrap();
        ensure_characters_table(&conn).unwrap(); // should ALTER ADD age
        // existing row preserved, age NULL
        let (g, a): (String, Option<i64>) = conn
            .query_row("SELECT gender, age FROM characters WHERE speaker='HAMLET'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(g, "male");
        assert_eq!(a, None);
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins characters_table_has_age characters_table_migrates`
Expected: FAIL — `no such column: age`.

- [ ] **Step 3: Implement** — replace `ensure_characters_table` (~lines 492–502) in `src/db/queries.rs`:

```rust
pub fn ensure_characters_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Fresh installs get the age column directly.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS characters (
            work_abbrev TEXT NOT NULL,
            speaker     TEXT NOT NULL,
            gender      TEXT NOT NULL,
            age         INTEGER,
            PRIMARY KEY (work_abbrev, speaker)
        );"
    )?;
    // Legacy migration: a pre-age table lacks the `age` column. ADD it (the
    // pragma probe mirrors ensure_gloss_audio_table's column-existence check).
    let has_age: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('characters') WHERE name = 'age'")?
        .exists([])?;
    if !has_age {
        conn.execute_batch("ALTER TABLE characters ADD COLUMN age INTEGER;")?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins characters_table_has_age characters_table_migrates`
Expected: PASS (both — run each separately if the multi-filter form errors: `cargo test --bins characters_table_has_age` then `cargo test --bins characters_table_migrates`).

- [ ] **Step 5: Build + full tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean; all PASS. (Existing `characters` tests that `INSERT INTO characters VALUES ('Ham','HAMLET','male')` — positional 3-value — still work because `age` is nullable and trailing.)

- [ ] **Step 6: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): add nullable characters.age column (+ legacy ALTER migration)"
```

---

## Task 3: `resolve_default_voice`

**Files:**
- Modify: `src/db/queries.rs` (add `resolve_default_voice` + `DEFAULT_AGE`; helper to read (gender, age); tests)

- [ ] **Step 1: Write failing tests** — add to the queries test module. A `seed_catalog_and_chars` helper seeds both tables, then tests cover containment / nearest / NULL-age / unknown-gender / verse-vs-prose:

```rust
    fn seed_catalog_and_chars(conn: &Connection) {
        ensure_voice_catalog_table(conn).unwrap();
        ensure_characters_table(conn).unwrap();
        // Juliet 14 (female), Lear 80 (male), Hamlet 30 (male), Nurse NULL age (female)
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Rom','JULIET','female',14)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Lr','LEAR','male',80)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender, age) VALUES ('Ham','HAMLET','male',30)", []).unwrap();
        conn.execute("INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Rom','NURSE','female')", []).unwrap();
    }

    #[test]
    fn resolve_containment_picks_the_band_containing_age() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Juliet 14 female -> Willa (12-19) verse
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "JULIET", true),
            (crate::elevenlabs::A_OP_F_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
        // Hamlet 30 male verse -> no band contains 30 (Will 15-25, Petruchio 35-45);
        // nearest is Petruchio (distance 5) vs Will (distance 5) -> tie; see nearest test.
    }

    #[test]
    fn resolve_nearest_band_when_no_containment() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Lear 80 male prose: no band contains 80; nearest male band is Petruchio
        // (35-45, distance 35) vs Will (15-25, distance 55) -> Petruchio prose (D).
        assert_eq!(
            resolve_default_voice(&conn, "Lr", "LEAR", false),
            (crate::elevenlabs::D_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_null_age_uses_default_age_40() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // Nurse female, NULL age -> DEFAULT_AGE 40. No female band contains 40
        // (Willa 12-19, Beatrice 20-30); nearest is Beatrice (dist 10) -> E_OP verse.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NURSE", true),
            (crate::elevenlabs::E_OP_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }

    #[test]
    fn resolve_unknown_gender_defaults_male() {
        let conn = Connection::open_in_memory().unwrap();
        seed_catalog_and_chars(&conn);
        // No characters row -> Unknown gender -> male; NULL age -> 40; nearest male
        // band to 40 is Petruchio (35-45 contains 40!) -> D prose.
        assert_eq!(
            resolve_default_voice(&conn, "Rom", "NOBODY", false),
            (crate::elevenlabs::D_VOICE_ID.to_string(), crate::elevenlabs::OP_MODEL_ID.to_string())
        );
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins resolve_containment resolve_nearest resolve_null_age resolve_unknown_gender`
Expected: FAIL — `cannot find function resolve_default_voice`. (Run filters individually if the multi-filter errors.)

- [ ] **Step 3: Implement** — add to `src/db/queries.rs` (near `get_character_gender`). First a private helper that reads `(gender, age)`, then `resolve_default_voice`:

```rust
/// Default age used when a character has no curated age (NULL).
const DEFAULT_AGE: i64 = 40;

/// Read (Gender, age) for a speaker. Multi-speaker (comma) / no row / error →
/// (Unknown, None). Generalizes get_character_gender to also pull age.
fn get_character_gender_age(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
) -> (crate::elevenlabs::Gender, Option<i64>) {
    if speaker.contains(',') {
        return (crate::elevenlabs::Gender::Unknown, None);
    }
    let row: Result<(String, Option<i64>), _> = conn.query_row(
        "SELECT gender, age FROM characters WHERE work_abbrev = ?1 AND speaker = ?2",
        rusqlite::params![work_abbrev, speaker],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    match row {
        Ok((g, a)) => (crate::elevenlabs::Gender::from_db(&g), a),
        Err(_) => (crate::elevenlabs::Gender::Unknown, None),
    }
}

/// Pick the default (voice_id, model_id) for a speaker by (gender, age) from the
/// voice_catalog: the narrowest band CONTAINING the age, else the NEAREST
/// same-gender band, else the legacy `voice_for` constants. `is_verse` selects
/// the verse/prose role. Unknown/neutral gender → male; missing age → DEFAULT_AGE.
pub fn resolve_default_voice(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
    is_verse: bool,
) -> (String, String) {
    let (gender, age_opt) = get_character_gender_age(conn, work_abbrev, speaker);
    // Catalog gender is 'male' | 'female'; everything not Female → male.
    let cat_gender = if gender == crate::elevenlabs::Gender::Female { "female" } else { "male" };
    let age = age_opt.unwrap_or(DEFAULT_AGE);
    let role = if is_verse { "verse" } else { "prose" };

    // 1. Containment: narrowest band that contains `age`.
    let contained: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2 AND ?3 BETWEEN age_min AND age_max
             ORDER BY (age_max - age_min) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if let Some(hit) = contained {
        return hit;
    }

    // 2. Nearest same-gender/role band by distance from `age` to [age_min,age_max]
    //    (distance 0 if inside — already handled; else age-age_max or age_min-age).
    let nearest: Option<(String, String)> = conn
        .query_row(
            "SELECT voice_id, model_id FROM voice_catalog
             WHERE gender = ?1 AND role = ?2
             ORDER BY MAX(0, age_min - ?3) + MAX(0, ?3 - age_max) ASC LIMIT 1",
            rusqlite::params![cat_gender, role, age],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if let Some(hit) = nearest {
        return hit;
    }

    // 3. Last resort (catalog empty / no same-gender voice — unreachable given
    //    the seed rows): the legacy gender-only constants.
    let (v, m) = crate::elevenlabs::voice_for(gender, is_verse);
    (v.to_string(), m.to_string())
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins resolve_containment resolve_nearest resolve_null_age resolve_unknown_gender`
Expected: PASS (4 tests; run filters individually if needed).

- [ ] **Step 5: Build + full tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean; all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): resolve_default_voice — (gender,age) -> voice via containment/nearest band"
```

---

## Task 4: Swap the default branch in `play_block_tts`

**Files:**
- Modify: `src/input/actions/gloss.rs` (`play_block_tts` default branch, line 688)

Build-verified; runtime is a user check.

- [ ] **Step 1: Swap the default-branch call.** In `src/input/actions/gloss.rs`, the empty-set `else` branch (~lines 686-689) currently is:

```rust
                } else {
                    let gender =
                        crate::db::queries::get_character_gender(&conn, &work_abbrev, &speaker);
                    let (v, m) = crate::elevenlabs::voice_for(gender, is_verse);
                    (v.to_string(), m.to_string())
                }
```

Replace it with the age-aware resolver (it returns `(String, String)` already, so no `.to_string()`):

```rust
                } else {
                    // No associated voices → age-aware default voice by
                    // (gender, age) from the voice_catalog (verse/prose by kind).
                    crate::db::queries::resolve_default_voice(
                        &conn, &work_abbrev, &speaker, is_verse,
                    )
                }
```

(The `if !voices.is_empty()` override branch and the outer `Err(_)` branch — which still calls `voice_for(Gender::Unknown, is_verse)` for the DB-open-failure case — are UNCHANGED. Only the empty-set default branch swaps.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`. (`get_character_gender` may now be unused if nothing else calls it — it's still used by `get_character_gender_age`? No — that's a separate fn. Check: `rg -n "get_character_gender\b" src/` — if `get_character_gender` is now unused outside its own def, a dead_code warning is fine; do NOT delete it.)

- [ ] **Step 3: Full tests**

Run: `cargo test --bins -- --test-threads=1 2>&1 | rg "test result"`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): default voice is now age-aware (resolve_default_voice) when no associated voice"
```

- [ ] **Step 5: User runtime check.** `cargo run`, gloss a young female speaker (Juliet) and an old male (Lear) with no associated voices; the dev log line `TTS: voice -> <id>` should show Willa for Juliet (after ages curated — Task 5) and Petruchio for Lear. Before ages are curated, all ages are NULL→40, so males→Petruchio (40 ∈ 35-45) and females→Beatrice (nearest to 40).

---

## Task 5: `curate_characters.py` — add age (rename + extend)

**Files:**
- Create: `scripts/curate_characters.py` (rename of `curate_genders.py` with age)
- Delete: `scripts/curate_genders.py`

Follows the existing script; verified by `ast.parse` (user runs it live).

- [ ] **Step 1: Create `scripts/curate_characters.py`** as a copy of `curate_genders.py` with these changes (the SYSTEM prompt asks for gender AND age; the JSON value is a dict; the table/insert handle age):

```python
#!/usr/bin/env python3
"""Populate lit.db `characters(work_abbrev, speaker, gender, age)` via Claude.

Enumerates every distinct (work_abbrev, speaker) from line_mapping for
Shakespeare works, asks Claude for each speaker's TRUE gender (ignoring
disguises) AND an approximate integer age, and loads both. Re-runnable: rows
missing a gender OR an age are (re)curated. No human-review gate. Requires
ANTHROPIC_API_KEY.

Usage:
  python scripts/curate_characters.py            # curate missing rows
  python scripts/curate_characters.py --dry-run  # print, don't write
"""
import argparse
import json
import os
import sqlite3
import sys

import anthropic  # pip install anthropic

DB_PATH = os.path.expanduser("~/utono/litdb/data/lit.db")
MODEL = "claude-opus-4-7"
BATCH = 25
DELIM = " ::: "

SYSTEM = """You assign a GENDER and an approximate AGE to each Shakespeare
character speaker name. Return ONLY a JSON object mapping each input
"work_abbrev ::: speaker" key (separated by the literal " ::: ") to an object
{"gender": <g>, "age": <n>} where:
- gender is "male" | "female" | "neutral" | "unknown".
  Use the character's TRUE gender, ignoring disguises (Viola as Cesario is
  "female"; Rosalind as Ganymede is "female"). Combined "A / B" -> shared
  gender if both same, else "neutral". Groups (ALL, LORDS, CITIZENS) ->
  "neutral". Spirits: canonical gender if clear (Hamlet's GHOST "male"),
  WITCHES "neutral", else "neutral". Unresolvable -> "unknown".
- age is your best integer estimate of the character's age in years (e.g.
  Juliet 14, Hamlet 30, Lear 80). If genuinely unknowable (a group, an
  unnamed functionary), use null.
Output JSON only, no prose."""


def ensure_table(conn):
    conn.execute(
        "CREATE TABLE IF NOT EXISTS characters ("
        " work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,"
        " age INTEGER, PRIMARY KEY (work_abbrev, speaker))"
    )
    # add age column if an older 3-col table exists
    cols = [r[1] for r in conn.execute("PRAGMA table_info(characters)").fetchall()]
    if "age" not in cols:
        conn.execute("ALTER TABLE characters ADD COLUMN age INTEGER")


def missing_rows(conn):
    # speakers with no row at all, OR a row missing its age (so a gender-only
    # prior run gets ages filled in on re-run).
    rows = conn.execute(
        "SELECT DISTINCT lm.work_abbrev, lm.speaker "
        "FROM line_mapping lm JOIN works w ON w.abbrev = lm.work_abbrev "
        "WHERE lm.speaker IS NOT NULL AND w.author = 'Shakespeare' "
        "  AND lm.work_abbrev NOT LIKE '%-%' "
        "  AND NOT EXISTS (SELECT 1 FROM characters c "
        "    WHERE c.work_abbrev = lm.work_abbrev AND c.speaker = lm.speaker "
        "      AND c.age IS NOT NULL) "
        "ORDER BY lm.work_abbrev, lm.speaker"
    ).fetchall()
    return rows


def assign_batch(client, batch):
    keys = [f"{w}{DELIM}{s}" for (w, s) in batch]
    user = (
        "Assign gender+age to each of these keys (each line is "
        "work_abbrev ::: speaker):\n" + "\n".join(keys)
    )
    resp = client.messages.create(
        model=MODEL, max_tokens=4096, system=SYSTEM,
        messages=[{"role": "user", "content": user}],
    )
    text = resp.content[0].text.strip()
    if text.startswith("```"):
        text = text.split("```", 2)[1]
        if text.startswith("json"):
            text = text[len("json"):]
        text = text.strip()
    return json.loads(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("Set ANTHROPIC_API_KEY")

    conn = sqlite3.connect(DB_PATH)
    ensure_table(conn)
    todo = missing_rows(conn)
    if not todo:
        print("characters table already has gender+age for every speaker.")
        return
    print(f"{len(todo)} speakers to (re)curate...")

    client = anthropic.Anthropic()
    written = 0
    for i in range(0, len(todo), BATCH):
        batch = todo[i:i + BATCH]
        try:
            result = assign_batch(client, batch)
        except Exception as e:
            print(f"  ...batch {i}-{i + len(batch)} FAILED ({e}); skipping "
                  f"(re-run will retry these)", file=sys.stderr)
            continue
        for (w, s) in batch:
            obj = result.get(f"{w}{DELIM}{s}", {})
            if not isinstance(obj, dict):
                obj = {}
            gender = obj.get("gender", "unknown")
            if gender not in ("male", "female", "neutral", "unknown"):
                gender = "unknown"
            age = obj.get("age", None)
            if not isinstance(age, int):
                age = None
            if args.dry_run:
                print(f"  {w}{DELIM}{s}\t{gender}\t{age}")
            else:
                conn.execute(
                    "INSERT INTO characters (work_abbrev, speaker, gender, age)"
                    " VALUES (?, ?, ?, ?)"
                    " ON CONFLICT(work_abbrev, speaker)"
                    " DO UPDATE SET gender = excluded.gender, age = excluded.age",
                    (w, s, gender, age),
                )
                written += 1
        if not args.dry_run:
            conn.commit()
        print(f"  ...{min(i + BATCH, len(todo))}/{len(todo)}")

    if not args.dry_run:
        print(f"Wrote {written} rows to characters.")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Delete the old script + chmod the new one**

```bash
git rm scripts/curate_genders.py
chmod +x scripts/curate_characters.py
```

- [ ] **Step 3: Syntax-check (do NOT run — it hits the live API)**

```bash
python -c "import ast; ast.parse(open('scripts/curate_characters.py').read()); print('OK')"
python -m py_compile scripts/curate_characters.py && echo COMPILE_OK
```
Expected: `OK` and `COMPILE_OK`.

- [ ] **Step 4: Commit**

```bash
git add scripts/curate_characters.py scripts/curate_genders.py
git commit -m "feat(scripts): curate_characters.py — gender + age (replaces curate_genders.py)"
```

- [ ] **Step 5: User runs it (live API + writes lit.db).** The `missing_rows` query re-selects every speaker whose `age IS NULL` (i.e. all the gender-only rows from the prior Phase-1 run), so it fills in ages without re-asking gender-only. Ask the user to run:

```bash
ANTHROPIC_API_KEY=… python scripts/curate_characters.py
```
Then spot-check: `sqlite3 ~/utono/litdb/data/lit.db "SELECT speaker, gender, age FROM characters WHERE speaker IN ('JULIET','HAMLET','LEAR','OPHELIA') LIMIT 10"`.

---

## Self-review notes

- **Spec coverage:** §2.1 voice_catalog + seed → Task 1 (seed uses the real age bands the user gave, not the spec's placeholder 0-120). §2.2 characters.age → Task 2. §2.3 resolve_default_voice (containment → nearest → voice_for) → Task 3. §2.4 swap default branch → Task 4. §2.5 testability → Tasks 1-3 DB tests + user checks. The curation rename/age → Task 5.
- **Deviation from spec, intentional:** the spec's §2.1 said "seed the four existing voices with broad range (0-120)". Reality after Phase 1: there are now eight voices (four pairs) with distinct age bands the user specified (Will 15-25, Petruchio 35-45, Willa 12-19, Beatrice 20-30). The plan seeds those real bands so age selection is meaningful from day one. Noted here so it's a conscious divergence.
- **Type consistency:** `resolve_default_voice(&Connection, &str, &str, bool) -> (String, String)` (Tasks 3,4). `get_character_gender_age(&Connection, &str, &str) -> (Gender, Option<i64>)` (Task 3). `ensure_voice_catalog_table(&Connection)` (Tasks 1,3-test). `DEFAULT_AGE: i64 = 40` (Task 3). Voice consts (A_OP_VOICE_ID … F_VOICE_ID, OP_MODEL_ID) referenced by their elevenlabs.rs names throughout.
- **Migration safety:** Task 2's `characters.age` ALTER uses the `pragma_table_info` probe (the established pattern). On the real dev DB — which already has the 3-col characters table populated with 1452 gender rows — the first launch ALTERs in a nullable `age` (all NULL), preserving every gender row. The Phase-1 curation data is NOT lost; Task 5's re-run fills ages via the `age IS NULL` predicate.
- **Behavior before age curation (Task 5 not yet run):** every character's age is NULL → DEFAULT_AGE 40. Male 40 ∈ Petruchio (35-45) → Petruchio; female 40 → nearest Beatrice (20-30). So until ages are curated it behaves like the Phase-1 Petruchio/Beatrice default — a safe, identical-to-now degradation. Age differentiation activates only after Task 5's re-run.
- **`get_character_gender` retained:** Task 4 swaps the default branch to `resolve_default_voice`, which uses the new `get_character_gender_age`. The old `get_character_gender` may become unused in production (the Err branch in play_block_tts still calls `voice_for` directly, not get_character_gender) — keep it (it's pub, tested); a dead_code/unused warning is acceptable, do not delete.

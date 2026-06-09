# Character gender → gendered TTS voice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `characters(work_abbrev, speaker, gender)` table populated by an LLM batch tool, and use it to select the male vs female OP/prose ElevenLabs voice when synthesizing gloss audio.

**Architecture:** A new SQLite table stores per-(work, speaker) gender. A pure `get_character_gender` query helper resolves a speaker to a `Gender` enum (defaulting to male on no-row/neutral/unknown/multi-speaker). `play_block_tts` switches among four voice-id constants by `(gender, BlockKind)` — verse→OP voice, prose→plain voice — always on model `eleven_v3`. A Python `scripts/curate_genders.py` enumerates speakers, asks Claude for gender, and loads the table out-of-band.

**Tech Stack:** Rust (rusqlite, `cargo test --bins` — binary-only crate), SQLite (`~/utono/litdb/data/lit.db`), Python (`scripts/`, sqlite3 + anthropic SDK).

**Spec:** `docs/superpowers/specs/2026-06-08-character-gender-design.md`

**The four voice IDs (from the guide's "Saved voice IDs" section / project memory):**
- A-OP male verse OP: `qIorOnPHyesnVMLvolyz`
- B male prose: `jTudAEr52RK5998TOYLM`
- A-OP-F female verse OP: `AJEmTDfBuB294lokNL10`
- B-F female prose: `EKXvXWSM0PF7VaEykbP4`
- model: `eleven_v3`

---

## Task 1: `Gender` enum + voice-id constants

**Files:**
- Modify: `src/elevenlabs.rs` (add consts near `ALICE_VOICE_ID` line 8; add `Gender` enum + `voice_for` fn; tests in the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing tests** — add to `mod tests` in `src/elevenlabs.rs`:

```rust
    #[test]
    fn voice_for_male_verse_is_aop() {
        assert_eq!(voice_for(Gender::Male, true), (A_OP_VOICE_ID, OP_MODEL_ID));
    }

    #[test]
    fn voice_for_female_prose_is_bf() {
        assert_eq!(voice_for(Gender::Female, false), (B_F_VOICE_ID, OP_MODEL_ID));
    }

    #[test]
    fn voice_for_neutral_and_unknown_default_to_male() {
        // neutral/unknown -> male set (never guess)
        assert_eq!(voice_for(Gender::Neutral, true), (A_OP_VOICE_ID, OP_MODEL_ID));
        assert_eq!(voice_for(Gender::Unknown, false), (B_VOICE_ID, OP_MODEL_ID));
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins voice_for`
Expected: FAIL — `cannot find value A_OP_VOICE_ID` / `cannot find function voice_for`.

- [ ] **Step 3: Implement** — add after the `ALICE_MODEL_ID` const (line 9) in `src/elevenlabs.rs`:

```rust
/// The four custom Voice-Design narration voices (see
/// docs/guides/elevenlabs-v3-custom-voices.md "Saved voice IDs"). All render on
/// `eleven_v3` (the only model that reads inline /IPA/ and [audio tags]).
pub const A_OP_VOICE_ID: &str = "qIorOnPHyesnVMLvolyz"; // Will OP — male verse, OP
pub const B_VOICE_ID: &str = "jTudAEr52RK5998TOYLM"; // Will — male prose
pub const A_OP_F_VOICE_ID: &str = "AJEmTDfBuB294lokNL10"; // Willa OP — female verse, OP
pub const B_F_VOICE_ID: &str = "EKXvXWSM0PF7VaEykbP4"; // Willa — female prose
pub const OP_MODEL_ID: &str = "eleven_v3";

/// A character's gender, for gendered-voice selection. `Neutral` (deliberately
/// non-gendered) and `Unknown` (unresolved) both fall back to the male voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
    Neutral,
    Unknown,
}

impl Gender {
    /// Parse the stored TEXT value; anything unrecognized → `Unknown`.
    pub fn from_db(s: &str) -> Gender {
        match s {
            "male" => Gender::Male,
            "female" => Gender::Female,
            "neutral" => Gender::Neutral,
            _ => Gender::Unknown,
        }
    }
}

/// Pick (voice_id, model_id) for a `gender` reading a verse (`is_verse=true`,
/// the OP voice) or prose (`false`, the plain voice). Neutral/Unknown default to
/// the MALE set — never guess (per the design spec / guide).
pub fn voice_for(gender: Gender, is_verse: bool) -> (&'static str, &'static str) {
    let female = gender == Gender::Female;
    let id = match (female, is_verse) {
        (false, true) => A_OP_VOICE_ID,
        (false, false) => B_VOICE_ID,
        (true, true) => A_OP_F_VOICE_ID,
        (true, false) => B_F_VOICE_ID,
    };
    (id, OP_MODEL_ID)
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins voice_for`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/elevenlabs.rs
git commit -m "feat(tts): Gender enum + four-voice constants + voice_for selector"
```

---

## Task 2: `characters` table DDL

**Files:**
- Modify: `src/db/queries.rs` (add `ensure_characters_table` near `ensure_bookmarks_table` line 472)
- Modify: `src/app.rs` (call it in the `BOOKMARKS_INIT` `call_once`, line ~2452)

- [ ] **Step 1: Write failing test** — add a `#[cfg(test)] mod` test in `src/db/queries.rs` (if no test mod exists at the bottom of the file, create one). The test creates an in-memory DB, ensures the table, and inserts/reads a row:

```rust
#[cfg(test)]
mod character_tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn ensure_characters_table_creates_usable_table() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_characters_table(&conn).unwrap();
        conn.execute(
            "INSERT INTO characters (work_abbrev, speaker, gender) VALUES ('Ham','HAMLET','male')",
            [],
        ).unwrap();
        let g: String = conn
            .query_row(
                "SELECT gender FROM characters WHERE work_abbrev='Ham' AND speaker='HAMLET'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(g, "male");
    }
}
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins ensure_characters_table`
Expected: FAIL — `cannot find function ensure_characters_table`.

- [ ] **Step 3: Implement the DDL** — add after `ensure_bookmarks_table` (line 486) in `src/db/queries.rs`:

```rust
/// Ensure the character-gender table exists. Keyed by (work_abbrev, speaker)
/// with speaker stored verbatim as it appears in line_mapping.speaker, so the
/// TTS-time lookup joins exactly with no runtime normalization. Rows are loaded
/// by scripts/curate_genders.py, not the app. See the character-gender spec.
pub fn ensure_characters_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS characters (
            work_abbrev TEXT NOT NULL,
            speaker     TEXT NOT NULL,
            gender      TEXT NOT NULL,
            PRIMARY KEY (work_abbrev, speaker)
        );"
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins ensure_characters_table`
Expected: PASS.

- [ ] **Step 5: Wire into startup** — in `src/app.rs`, the `BOOKMARKS_INIT.call_once` block (line ~2451), add the call alongside the others:

```rust
    BOOKMARKS_INIT.call_once(|| {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::ensure_bookmarks_table(&conn);
            let _ = crate::db::queries::ensure_echo_tables(&conn);
            let _ = crate::db::queries::ensure_gloss_audio_table(&conn);
            let _ = crate::db::queries::ensure_characters_table(&conn);
        }
    });
```

- [ ] **Step 6: Build + full tests**

Run: `cargo build && cargo test --bins -- --test-threads=1`
Expected: clean; all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/db/queries.rs src/app.rs
git commit -m "feat(db): characters(work_abbrev, speaker, gender) table"
```

---

## Task 3: `get_character_gender` query helper

**Files:**
- Modify: `src/db/queries.rs` (add `get_character_gender`; tests)

- [ ] **Step 1: Write failing tests** — add to `mod character_tests`:

```rust
    fn seed(conn: &Connection) {
        ensure_characters_table(conn).unwrap();
        conn.execute("INSERT INTO characters VALUES ('Ham','HAMLET','male')", []).unwrap();
        conn.execute("INSERT INTO characters VALUES ('Ham','OPHELIA','female')", []).unwrap();
        conn.execute("INSERT INTO characters VALUES ('Ham','ALL','neutral')", []).unwrap();
    }

    #[test]
    fn get_gender_resolves_known_speakers() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert_eq!(get_character_gender(&conn, "Ham", "HAMLET"), crate::elevenlabs::Gender::Male);
        assert_eq!(get_character_gender(&conn, "Ham", "OPHELIA"), crate::elevenlabs::Gender::Female);
        assert_eq!(get_character_gender(&conn, "Ham", "ALL"), crate::elevenlabs::Gender::Neutral);
    }

    #[test]
    fn get_gender_no_row_is_unknown() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert_eq!(get_character_gender(&conn, "Ham", "NOBODY"), crate::elevenlabs::Gender::Unknown);
    }

    #[test]
    fn get_gender_multi_speaker_string_is_unknown() {
        // GlossContext.speaker can be a comma-joined multi-speaker string; we
        // can't pick one gender, so it resolves Unknown (-> male fallback).
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert_eq!(get_character_gender(&conn, "Ham", "HAMLET, OPHELIA"), crate::elevenlabs::Gender::Unknown);
    }
```

- [ ] **Step 2: Run, verify FAIL**

Run: `cargo test --bins get_gender`
Expected: FAIL — `cannot find function get_character_gender`.

- [ ] **Step 3: Implement** — add to `src/db/queries.rs` (near the other read queries):

```rust
/// Resolve a (work, speaker) to a `Gender`. A multi-speaker string (contains a
/// comma — `GlossContext.speaker` joins multiple speakers that way) or a missing
/// row resolves to `Unknown`, which the voice selector maps to the male
/// fallback. Reads the `characters` table; a missing table or any error also
/// yields `Unknown` (safe default).
pub fn get_character_gender(
    conn: &Connection,
    work_abbrev: &str,
    speaker: &str,
) -> crate::elevenlabs::Gender {
    if speaker.contains(',') {
        return crate::elevenlabs::Gender::Unknown;
    }
    let row: Result<String, _> = conn.query_row(
        "SELECT gender FROM characters WHERE work_abbrev = ?1 AND speaker = ?2",
        rusqlite::params![work_abbrev, speaker],
        |r| r.get(0),
    );
    match row {
        Ok(g) => crate::elevenlabs::Gender::from_db(&g),
        Err(_) => crate::elevenlabs::Gender::Unknown,
    }
}
```

- [ ] **Step 4: Run, verify PASS**

Run: `cargo test --bins get_gender`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): get_character_gender helper (multi-speaker/no-row -> Unknown)"
```

---

## Task 4: Wire gender → voice into `play_block_tts`

**Files:**
- Modify: `src/input/actions/gloss.rs` (the voice-selection block at lines 627-651)

No automated test (needs AppState + DB + live TTS). Build-verified; runtime is a user check.

- [ ] **Step 1: Replace the config voice read with a gender→voice selection.** In `src/input/actions/gloss.rs`, the `play_block_tts` borrow block currently ends with `s.config.elevenlabs_voice_id.clone(), s.config.elevenlabs_model_id.clone()`. Replace the whole tuple-building block (lines 627-651) with:

```rust
    let (gloss_id, work_abbrev, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let gloss = match s.gloss_list.get(s.gloss_index) {
            Some(g) => g,
            None => return,
        };
        let gloss_id = gloss.gloss_id;
        let (work_abbrev, speaker) = match &s.gloss_context {
            Some(ctx) => (ctx.work_abbrev.clone(), ctx.speaker.clone()),
            None => return,
        };
        let blocks = crate::ui::gloss_overlay::gloss_blocks(&gloss.gloss_text);
        let text = match blocks.iter().find(|b| b.kind == kind && b.index == index) {
            Some(b) => b.text.clone(),
            None => return,
        };
        // Gendered voice: speaker -> gender (default male), block kind -> verse
        // vs prose. Source blocks are verse (OP voice + the /IPA/ already in the
        // text); explication blocks are prose (plain voice).
        let gender = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::get_character_gender(&conn, &work_abbrev, &speaker),
            Err(_) => crate::elevenlabs::Gender::Unknown,
        };
        let is_verse = kind == BlockKind::Source;
        let (vid, mid) = crate::elevenlabs::voice_for(gender, is_verse);
        crate::log_fmt!(
            "TTS: voice {:?} {} -> {} (speaker={}, {})",
            gender, work_abbrev, vid, speaker, if is_verse { "verse" } else { "prose" }
        );
        (
            gloss_id,
            work_abbrev,
            text,
            vid.to_string(),
            mid.to_string(),
            s.tokio_handle.clone(),
        )
    };
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | rg "^error" || echo OK`
Expected: `OK`. (If `BlockKind` isn't in scope at that point, it already is — `kind: BlockKind` is the fn param.)

- [ ] **Step 3: Full tests**

Run: `cargo test --bins -- --test-threads=1`
Expected: all PASS (the 402→Alice fallback in the synth path still applies — a paid-plan voice that 402s falls back to Alice, unchanged).

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): select gendered OP/prose voice by speaker gender + block kind"
```

- [ ] **Step 5: User runtime check (cannot self-verify).** Ask the user to `cargo run`, gloss a male-speaker passage and a female-speaker passage (after the table is populated — Task 5), and confirm the dev log shows the right voice id (`A-OP`/`B` vs `A-OP-F`/`B-F`) and the audio uses the expected gendered voice. With an empty `characters` table everything resolves Unknown → male (A-OP/B) — also a valid check (no female voice until the table is populated).

---

## Task 5: `scripts/curate_genders.py` — LLM curation tool

**Files:**
- Create: `scripts/curate_genders.py`

Follows the `scripts/build_embeddings.py` pattern (sqlite3 + an API SDK + a DB-path constant). No Rust/cargo changes. Verified by running it (user-run; it hits the live Claude API and writes the DB).

- [ ] **Step 1: Write the script.** Create `scripts/curate_genders.py`:

```python
#!/usr/bin/env python3
"""Populate lit.db `characters(work_abbrev, speaker, gender)` via Claude.

Enumerates every distinct (work_abbrev, speaker) from line_mapping for
Shakespeare works, asks Claude for each speaker's TRUE gender (ignoring
disguises), and loads the result. Re-runnable: only speakers missing from
`characters` are sent to the model. No human-review gate (see the spec's
trust model). Requires ANTHROPIC_API_KEY.

Usage:
  python scripts/curate_genders.py            # curate all missing speakers
  python scripts/curate_genders.py --dry-run  # print assignments, don't write
"""
import argparse
import json
import os
import sqlite3
import sys

import anthropic  # pip install anthropic

DB_PATH = os.path.expanduser("~/utono/litdb/data/lit.db")
MODEL = "claude-opus-4-7"
BATCH = 40  # speakers per Claude call

SYSTEM = """You assign a gender to each Shakespeare character speaker name.
Return ONLY a JSON object mapping each input "work_abbrev\\tspeaker" key to one
of: "male", "female", "neutral", "unknown". Rules:
- Use the character's TRUE gender, ignoring disguises (Viola disguised as
  Cesario is "female"; Rosalind as Ganymede is "female").
- Combined speakers "A / B": if both are the same gender, use it; if mixed or
  unclear, "neutral".
- Groups/collectives (ALL, BOTH, LORDS, CITIZENS) -> "neutral".
- Spirits/non-human: use the canonically clear gender if one exists (Hamlet's
  GHOST is "male"); the WITCHES -> "neutral"; otherwise "neutral".
- Genuinely unresolvable (an obscure unnamed role you cannot place) -> "unknown".
Output JSON only, no prose."""


def ensure_table(conn):
    conn.execute(
        "CREATE TABLE IF NOT EXISTS characters ("
        " work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,"
        " PRIMARY KEY (work_abbrev, speaker))"
    )


def missing_speakers(conn):
    rows = conn.execute(
        "SELECT DISTINCT lm.work_abbrev, lm.speaker "
        "FROM line_mapping lm JOIN works w ON w.abbrev = lm.work_abbrev "
        "WHERE lm.speaker IS NOT NULL AND w.author = 'Shakespeare' "
        "  AND lm.work_abbrev NOT LIKE '%-%' "
        "  AND NOT EXISTS (SELECT 1 FROM characters c "
        "    WHERE c.work_abbrev = lm.work_abbrev AND c.speaker = lm.speaker) "
        "ORDER BY lm.work_abbrev, lm.speaker"
    ).fetchall()
    return rows  # list of (work_abbrev, speaker)


def assign_batch(client, batch):
    keys = [f"{w}\t{s}" for (w, s) in batch]
    user = "Assign a gender to each of these work\\tspeaker keys:\n" + "\n".join(keys)
    resp = client.messages.create(
        model=MODEL, max_tokens=2048, system=SYSTEM,
        messages=[{"role": "user", "content": user}],
    )
    text = resp.content[0].text.strip()
    # tolerate a ```json fence
    if text.startswith("```"):
        text = text.split("```", 2)[1].lstrip("json").strip()
    return json.loads(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("Set ANTHROPIC_API_KEY")

    conn = sqlite3.connect(DB_PATH)
    ensure_table(conn)
    todo = missing_speakers(conn)
    if not todo:
        print("characters table already covers every speaker.")
        return
    print(f"{len(todo)} speakers to assign...")

    client = anthropic.Anthropic()
    written = 0
    for i in range(0, len(todo), BATCH):
        batch = todo[i:i + BATCH]
        result = assign_batch(client, batch)
        for (w, s) in batch:
            gender = result.get(f"{w}\t{s}", "unknown")
            if gender not in ("male", "female", "neutral", "unknown"):
                gender = "unknown"
            if args.dry_run:
                print(f"  {w}\t{s}\t{gender}")
            else:
                conn.execute(
                    "INSERT OR REPLACE INTO characters (work_abbrev, speaker, gender)"
                    " VALUES (?, ?, ?)", (w, s, gender),
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

- [ ] **Step 2: Make it executable + smoke-check syntax**

```bash
chmod +x scripts/curate_genders.py
python -c "import ast; ast.parse(open('scripts/curate_genders.py').read()); print('OK')"
```
Expected: `OK`.

- [ ] **Step 3: Commit**

```bash
git add scripts/curate_genders.py
git commit -m "feat(scripts): curate_genders.py — LLM character-gender population tool"
```

- [ ] **Step 4: User runs it (live API + writes lit.db).** Ask the user to run, ideally `--dry-run` first to eyeball assignments, then for real:

```bash
ANTHROPIC_API_KEY=… python scripts/curate_genders.py --dry-run | head -40
ANTHROPIC_API_KEY=… python scripts/curate_genders.py
```
Then spot-check: `sqlite3 ~/utono/litdb/data/lit.db "SELECT * FROM characters WHERE speaker IN ('HAMLET','OPHELIA','ALL') LIMIT 10"`.

---

## Self-review notes

- **Spec coverage:** §1 schema → Task 2. §2 population (enumerate / LLM-assign / load-directly / re-runnable) → Task 5 (the `missing_speakers` query is the re-runnable enumeration; no review gate, per the trust model). §3 consumption (resolve speaker → gender → voice, male fallback) → Tasks 1+3+4. The `gloss_audio` cache naturally re-keys on the new voice_id (no task needed — the cache already keys on voice_id/model_id, established in the IPA work).
- **Out of scope (correct):** per-scene disguise voicing, role-prefixed-generic fine assignment, non-Shakespeare works (the script's `author='Shakespeare'` filter scopes it; generalizing is future work) — all listed in the spec's Open Questions.
- **Type consistency:** `Gender` (elevenlabs.rs) used identically in Tasks 1,3,4. `voice_for(Gender, bool) -> (&'static str, &'static str)` in Tasks 1,4. `get_character_gender(&Connection, &str, &str) -> Gender` in Tasks 3,4. `ensure_characters_table(&Connection)` in Tasks 2,3,5(SQL mirror).
- **The `GlossContext.speaker` multi-speaker caveat** (comma-joined) is handled in `get_character_gender` (Task 3) → Unknown → male fallback. The DDL in the Python tool (Task 5 `ensure_table`) mirrors the Rust DDL (Task 2) byte-for-byte on columns/PK so the two can't drift.
- **Risk:** the four voice IDs are professional/library voices on `eleven_v3`; on a free/starter ElevenLabs tier they may 402 → the existing 402→Alice fallback in the synth path catches that (audible Alice instead of the gendered voice, not a crash). Noted, not blocking.
- **Empty-table behavior:** until Task 5 is run, every speaker resolves Unknown → male (A-OP/B). The feature is therefore safe to merge before curation runs; female voices activate only once the table is populated.

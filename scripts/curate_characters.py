#!/usr/bin/env python3
"""Populate lit.db `characters(work_abbrev, speaker, gender, age)` via Claude.

Enumerates every distinct (work_abbrev, speaker) from line_mapping for
Shakespeare works, asks Claude for each speaker's TRUE gender (ignoring
disguises) AND an approximate integer age, and loads both. Re-runnable: rows
missing a gender OR an age are (re)curated, so a prior gender-only run gets
ages filled in. No human-review gate (see the spec's trust model). Requires
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
BATCH = 25  # speakers per Claude call
DELIM = " ::: "  # visible key separator (won't occur in speaker names)

SYSTEM = """You assign a GENDER and an approximate AGE to each Shakespeare
character speaker name. Return ONLY a JSON object mapping each input
"work_abbrev ::: speaker" key (the parts separated by the literal " ::: ") to
an object {"gender": <g>, "age": <n>} where:
- gender is one of "male", "female", "neutral", "unknown".
  Use the character's TRUE gender, ignoring disguises (Viola disguised as
  Cesario is "female"; Rosalind as Ganymede is "female"). Combined "A / B" ->
  shared gender if both the same, else "neutral". Groups/collectives (ALL,
  BOTH, LORDS, CITIZENS) -> "neutral". Spirits/non-human: canonical gender if
  clear (Hamlet's GHOST "male"), the WITCHES -> "neutral", else "neutral".
  Genuinely unresolvable -> "unknown".
- age is your best integer estimate of the character's age in years (Juliet 14,
  Hamlet 30, Lear 80). If genuinely unknowable (a group, an unnamed
  functionary), use null.
Output JSON only, no prose."""


def ensure_table(conn):
    conn.execute(
        "CREATE TABLE IF NOT EXISTS characters ("
        " work_abbrev TEXT NOT NULL, speaker TEXT NOT NULL, gender TEXT NOT NULL,"
        " age INTEGER, PRIMARY KEY (work_abbrev, speaker))"
    )
    # Add age column if an older 3-column table exists (matches the Rust-side
    # migration in src/db/queries.rs ensure_characters_table).
    cols = [r[1] for r in conn.execute("PRAGMA table_info(characters)").fetchall()]
    if "age" not in cols:
        conn.execute("ALTER TABLE characters ADD COLUMN age INTEGER")


def missing_rows(conn):
    # Speakers with no row at all OR a row missing its age, so a prior
    # gender-only run gets ages filled in on re-run.
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
    return rows  # list of (work_abbrev, speaker)


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
        # strip a ```json ... ``` fence
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
        except Exception as e:  # malformed JSON, API error, truncation
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
            if not isinstance(age, int) or isinstance(age, bool):
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

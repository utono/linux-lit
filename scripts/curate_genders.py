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
BATCH = 25  # speakers per Claude call
DELIM = " ::: "  # visible key separator (won't occur in speaker names)

SYSTEM = """You assign a gender to each Shakespeare character speaker name.
Return ONLY a JSON object mapping each input "work_abbrev ::: speaker" key (the
parts are separated by the literal string " ::: ") to one
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
    keys = [f"{w}{DELIM}{s}" for (w, s) in batch]
    user = (
        "Assign a gender to each of these keys (each line is "
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
    todo = missing_speakers(conn)
    if not todo:
        print("characters table already covers every speaker.")
        return
    print(f"{len(todo)} speakers to assign...")

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
            gender = result.get(f"{w}{DELIM}{s}", "unknown")
            if gender not in ("male", "female", "neutral", "unknown"):
                gender = "unknown"
            if args.dry_run:
                print(f"  {w}{DELIM}{s}\t{gender}")
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

#!/usr/bin/env python3
"""Pre-compute passage embeddings for semantic echo search.

Reads Shakespeare lines from line_mapping in lit.db, groups them into
speaker turns and 2-turn exchanges, enriches with dramatic context,
embeds via Voyage AI, and stores vectors in a passage_embeddings table.

Also computes a 3-D affect vector (Valence, Arousal, Dominance) per passage
from the NRC-VAD lexicon, stored in the `sentiment` column. This is a
cheap local lookup (no API cost) used as an optional re-rank axis on top of
the semantic cosine ranking (see the "Sentiment/Affect Re-Rank Axis" section
of docs/plans/2026-05-30-semantic-echo-search-design.md).

The NRC-VAD lexicon is NOT committed (redistribution-restricted academic
license). Download it once into scripts/data/:

    cd scripts/data
    curl -L -o nrc-vad.zip \\
        https://saifmohammad.com/WebDocs/Lexicons/NRC-VAD-Lexicon.zip
    unzip -j nrc-vad.zip 'NRC-VAD-Lexicon/NRC-VAD-Lexicon.txt' -d .
    rm nrc-vad.zip

The file is tab-separated `word<TAB>valence<TAB>arousal<TAB>dominance`,
values in [0, 1], ~20k English words, no header row.
"""

import os
import re
import sys
import sqlite3
import struct
import time
from collections import defaultdict

import voyageai

DB_PATH = os.path.expanduser("~/utono/litdb/data/lit.db")
MODEL = "voyage-4-large"
BATCH_SIZE = 20
MIN_WORDS = 4

# NRC-VAD lexicon path (gitignored — see module docstring for download steps).
VAD_LEXICON_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "data", "NRC-VAD-Lexicon.txt"
)
# Neutral midpoint used when a passage contains no in-lexicon words, so the
# affect cosine stays well-defined rather than producing a zero vector.
VAD_NEUTRAL = (0.5, 0.5, 0.5)
WORD_RE = re.compile(r"[a-z']+")


def get_shakespeare_abbrevs(conn):
    # Exclude alternate editions (suffixed with a hyphen, e.g. -Amb, -BBC):
    # they duplicate canonical texts and pollute the echo search.
    rows = conn.execute(
        "SELECT abbrev FROM works WHERE author = 'Shakespeare' AND abbrev NOT LIKE '%-%'"
    ).fetchall()
    return [r[0] for r in rows]


def load_lines(conn, abbrevs):
    placeholders = ",".join("?" for _ in abbrevs)
    rows = conn.execute(
        f"SELECT work_abbrev, div1, div2, line_in_div, speaker, canonical_text "
        f"FROM line_mapping "
        f"WHERE work_abbrev IN ({placeholders}) "
        f"ORDER BY work_abbrev, div1, div2, line_in_div",
        abbrevs,
    ).fetchall()
    return rows


def group_into_turns(lines):
    turns = []
    current = None

    for work, div1, div2, line_num, speaker, text in lines:
        if not speaker or not text.strip():
            if current:
                turns.append(current)
                current = None
            continue

        scene_key = (work, div1, div2)
        if current and current["speaker"] == speaker and current["scene"] == scene_key:
            current["lines"].append(text)
            current["end_line"] = line_num
        else:
            if current:
                turns.append(current)
            current = {
                "work": work,
                "div1": div1,
                "div2": div2,
                "speaker": speaker,
                "scene": scene_key,
                "start_line": line_num,
                "end_line": line_num,
                "lines": [text],
            }

    if current:
        turns.append(current)

    return turns


def build_exchanges(turns):
    exchanges = []
    for i in range(len(turns) - 1):
        a, b = turns[i], turns[i + 1]
        if a["scene"] != b["scene"]:
            continue
        exchanges.append({
            "work": a["work"],
            "div1": a["div1"],
            "div2": a["div2"],
            "speaker": f"{a['speaker']}, {b['speaker']}",
            "start_line": a["start_line"],
            "end_line": b["end_line"],
            "text": "\n".join(a["lines"]) + "\n" + "\n".join(b["lines"]),
            "enriched": f"{a['speaker']} to {b['speaker']}: " + " ".join(a["lines"])
                + f"\n{b['speaker']} to {a['speaker']}: " + " ".join(b["lines"]),
        })
    return exchanges


def enrich_turn(turn, turns, idx):
    text = " ".join(turn["lines"])
    addressee = "?"
    for j in range(idx + 1, min(idx + 3, len(turns))):
        if turns[j]["scene"] == turn["scene"] and turns[j]["speaker"] != turn["speaker"]:
            addressee = turns[j]["speaker"]
            break
    if addressee == "?":
        for j in range(max(0, idx - 3), idx):
            if turns[j]["scene"] == turn["scene"] and turns[j]["speaker"] != turn["speaker"]:
                addressee = turns[j]["speaker"]
                break
    return f"{turn['speaker']} to {addressee}: {text}"


def embed_to_blob(embedding):
    return struct.pack(f"{len(embedding)}f", *embedding)


def vad_to_blob(vad):
    """Pack a 3-tuple (valence, arousal, dominance) as little-endian f32."""
    return struct.pack("3f", *vad)


def load_vad_lexicon(path):
    """Load NRC-VAD into a dict word -> (valence, arousal, dominance).

    Returns None if the lexicon file is absent, so the caller can degrade
    gracefully (embeddings still build; sentiment is left NULL).
    """
    if not os.path.exists(path):
        return None
    lexicon = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 4:
                continue
            word, v, a, d = parts
            try:
                lexicon[word] = (float(v), float(a), float(d))
            except ValueError:
                continue
    return lexicon


def compute_vad(text, lexicon):
    """Mean VAD over the in-lexicon words of `text`.

    Words absent from the lexicon are ignored. A passage with no in-lexicon
    words falls back to VAD_NEUTRAL so the stored vector is never all-zero.
    """
    sv = sa = sd = 0.0
    n = 0
    for word in WORD_RE.findall(text.lower()):
        hit = lexicon.get(word)
        if hit is None:
            continue
        sv += hit[0]
        sa += hit[1]
        sd += hit[2]
        n += 1
    if n == 0:
        return VAD_NEUTRAL
    return (sv / n, sa / n, sd / n)


def create_table(conn):
    conn.execute("DROP TABLE IF EXISTS passage_embeddings")
    conn.execute("""
        CREATE TABLE passage_embeddings (
            id INTEGER PRIMARY KEY,
            work_abbrev TEXT NOT NULL,
            div1 INTEGER,
            div2 INTEGER,
            speaker TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            passage_type TEXT NOT NULL,
            passage_text TEXT NOT NULL,
            enriched_text TEXT NOT NULL,
            embedding BLOB NOT NULL,
            sentiment BLOB,
            UNIQUE(work_abbrev, div1, div2, start_line, end_line, passage_type)
        )
    """)
    conn.execute("CREATE INDEX idx_pe_work ON passage_embeddings(work_abbrev)")
    conn.execute("CREATE INDEX idx_pe_type ON passage_embeddings(passage_type)")


def migrate_add_sentiment(conn):
    """Add the `sentiment` column to an existing table if it's missing.

    Idempotent: a no-op when the column already exists. Lets an already-built
    corpus gain the affect axis without dropping and re-embedding.
    """
    cols = {row[1] for row in conn.execute("PRAGMA table_info(passage_embeddings)")}
    if "sentiment" not in cols:
        conn.execute("ALTER TABLE passage_embeddings ADD COLUMN sentiment BLOB")
        conn.commit()
        print("Migrated: added `sentiment` column to passage_embeddings")


def backfill_sentiment(conn, lexicon):
    """Populate `sentiment` for rows that have an embedding but no affect vector.

    Cheap local arithmetic — no API calls — so this runs every time and brings
    any pre-existing fully-embedded rows up to date. Uses passage_text (the raw
    spoken text), not enriched_text, so speaker/addressee labels don't skew the
    affect score.
    """
    rows = conn.execute(
        "SELECT id, passage_text FROM passage_embeddings WHERE sentiment IS NULL"
    ).fetchall()
    if not rows:
        return 0
    for pe_id, passage_text in rows:
        blob = vad_to_blob(compute_vad(passage_text, lexicon))
        conn.execute(
            "UPDATE passage_embeddings SET sentiment = ? WHERE id = ?",
            (blob, pe_id),
        )
    conn.commit()
    return len(rows)


def embed_batch(vo, texts, input_type="document"):
    for attempt in range(5):
        try:
            result = vo.embed(texts, model=MODEL, input_type=input_type)
            return result.embeddings
        except Exception as e:
            err = str(e)
            if "rate limit" in err.lower() or "RateLimit" in err:
                wait = 25 if attempt == 0 else 2 ** (attempt + 2)
                print(f"  Rate limited, waiting {wait}s...")
                time.sleep(wait)
            elif attempt < 4:
                wait = 2 ** (attempt + 1)
                print(f"  Retry in {wait}s: {e}")
                time.sleep(wait)
            else:
                raise


def main():
    api_key = os.environ.get("VOYAGE_API_KEY")
    if not api_key:
        print("Error: VOYAGE_API_KEY not set")
        sys.exit(1)

    vo = voyageai.Client(api_key=api_key)
    conn = sqlite3.connect(DB_PATH)

    lexicon = load_vad_lexicon(VAD_LEXICON_PATH)
    if lexicon is None:
        print(f"Warning: NRC-VAD lexicon not found at {VAD_LEXICON_PATH}")
        print("  Sentiment vectors will be left NULL. See module docstring to download.")
    else:
        print(f"Loaded NRC-VAD lexicon: {len(lexicon)} words")

    abbrevs = get_shakespeare_abbrevs(conn)
    print(f"Found {len(abbrevs)} Shakespeare works")

    lines = load_lines(conn, abbrevs)
    print(f"Loaded {len(lines)} lines")

    turns = group_into_turns(lines)
    print(f"Grouped into {len(turns)} speaker turns")

    filtered_turns = [
        t for t in turns
        if len(" ".join(t["lines"]).split()) >= MIN_WORDS
    ]
    print(f"After filtering (<{MIN_WORDS} words): {len(filtered_turns)} turns")

    exchanges = build_exchanges(filtered_turns)
    print(f"Built {len(exchanges)} two-turn exchanges")

    # Prepare all passages for embedding
    passages = []
    for idx, turn in enumerate(filtered_turns):
        text = "\n".join(turn["lines"])
        enriched = enrich_turn(turn, filtered_turns, idx)
        passages.append({
            "work": turn["work"],
            "div1": turn["div1"],
            "div2": turn["div2"],
            "speaker": turn["speaker"],
            "start_line": turn["start_line"],
            "end_line": turn["end_line"],
            "type": "turn",
            "text": text,
            "enriched": enriched,
        })

    for ex in exchanges:
        passages.append({
            "work": ex["work"],
            "div1": ex["div1"],
            "div2": ex["div2"],
            "speaker": ex["speaker"],
            "start_line": ex["start_line"],
            "end_line": ex["end_line"],
            "type": "exchange",
            "text": ex["text"],
            "enriched": ex["enriched"],
        })

    print(f"\nTotal passages to embed: {len(passages)}")

    # Create table if not exists
    existing = conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='passage_embeddings'"
    ).fetchone()
    if not existing:
        create_table(conn)
        conn.commit()
    else:
        migrate_add_sentiment(conn)
        existing_count = conn.execute("SELECT COUNT(*) FROM passage_embeddings").fetchone()[0]
        print(f"Resuming — {existing_count} passages already embedded")

    # Build set of already-embedded keys for resume
    already_done = set()
    try:
        rows = conn.execute(
            "SELECT work_abbrev, div1, div2, start_line, end_line, passage_type "
            "FROM passage_embeddings"
        ).fetchall()
        already_done = {(r[0], r[1], r[2], r[3], r[4], r[5]) for r in rows}
    except Exception:
        pass

    # Filter out already-embedded passages
    remaining = [
        p for p in passages
        if (p["work"], p["div1"], p["div2"], p["start_line"], p["end_line"], p["type"])
        not in already_done
    ]
    print(f"Skipping {len(passages) - len(remaining)} already-embedded, {len(remaining)} remaining")

    # Embed in batches
    total_embedded = 0
    for i in range(0, len(remaining), BATCH_SIZE):
        batch = remaining[i:i + BATCH_SIZE]
        texts = [p["enriched"] for p in batch]

        embeddings = embed_batch(vo, texts)

        for p, emb in zip(batch, embeddings):
            blob = embed_to_blob(emb)
            vad_blob = (
                vad_to_blob(compute_vad(p["text"], lexicon))
                if lexicon is not None
                else None
            )
            conn.execute(
                "INSERT OR IGNORE INTO passage_embeddings "
                "(work_abbrev, div1, div2, speaker, start_line, end_line, "
                "passage_type, passage_text, enriched_text, embedding, sentiment) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (p["work"], p["div1"], p["div2"], p["speaker"],
                 p["start_line"], p["end_line"], p["type"],
                 p["text"], p["enriched"], blob, vad_blob),
            )

        total_embedded += len(batch)
        if (i // BATCH_SIZE) % 10 == 0:
            conn.commit()
            print(f"  Embedded {total_embedded}/{len(remaining)} "
                  f"({100 * total_embedded // len(remaining)}%)")

    conn.commit()

    # Backfill affect vectors for any rows still missing one (e.g. a corpus
    # embedded before the sentiment column existed). Cheap local arithmetic.
    if lexicon is not None:
        filled = backfill_sentiment(conn, lexicon)
        if filled:
            print(f"Backfilled sentiment for {filled} pre-existing rows")

    # Report
    turn_count = conn.execute(
        "SELECT COUNT(*) FROM passage_embeddings WHERE passage_type = 'turn'"
    ).fetchone()[0]
    exchange_count = conn.execute(
        "SELECT COUNT(*) FROM passage_embeddings WHERE passage_type = 'exchange'"
    ).fetchone()[0]
    print(f"\nDone! Embedded {turn_count} turns + {exchange_count} exchanges = {turn_count + exchange_count} total")

    if lexicon is not None:
        with_sentiment = conn.execute(
            "SELECT COUNT(*) FROM passage_embeddings WHERE sentiment IS NOT NULL"
        ).fetchone()[0]
        print(f"Sentiment (VAD) vectors present on {with_sentiment}/{turn_count + exchange_count} rows")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Show the OP-IPA markup linux-lit sends to ElevenLabs for a gloss's source verse.

For a given gloss id, prints each <verse> (Source) block in three forms:
  RAW     — the stored gloss_text verse line (what the DB holds)
  DISPLAY — what the reader sees (IPA span removed, word kept) — `strip_ipa`
  TTS     — what is sent to ElevenLabs v3 to synthesize (word replaced by its
            /IPA/ so each tagged word is voiced once) — `ipa_for_tts`

The DISPLAY and TTS transforms are faithful ports of the Rust functions in
`src/ui/gloss_overlay.rs` (strip_ipa / normalize_ipa_whitespace / ipa_for_tts).
If those change, update this script to match. This is read-only: it never writes
the DB or calls ElevenLabs.

Usage:
  python scripts/show-gloss-ipa-tts.py <gloss-id>
  python scripts/show-gloss-ipa-tts.py <gloss-id> --tts-only   # just the TTS lines
"""
import argparse
import os
import re
import sqlite3
import sys

DB_PATH = os.path.expanduser("~/utono/litdb/data/lit.db")
PUNCT = set(",;:.!?\n")


def _is_ipa(inner: str) -> bool:
    # An IPA span is `/…/` whose inner has >=1 non-ASCII-letter char (length
    # marks, stress marks, schwa, etc.), so `and/or` and a plain `/word/` are
    # NOT spans. Mirrors strip_ipa's `is_ipa` heuristic.
    return len(inner) > 0 and any(not (c.isascii() and c.isalpha()) for c in inner)


def _normalize_ws(text: str) -> str:
    # Collapse space runs, drop a space before ,;:.!? or newline, trim.
    # Mirrors normalize_ipa_whitespace.
    out = []
    prev_space = False
    for c in text:
        if c == " ":
            prev_space = True
            continue
        if prev_space:
            if c not in PUNCT and c != "\n":
                out.append(" ")
            prev_space = False
        out.append(c)
    return "".join(out).strip()


def strip_ipa(text: str) -> str:
    """Reader-facing form: remove each /IPA/ span, KEEP the word."""
    chars = list(text)
    out = []
    i = 0
    while i < len(chars):
        if chars[i] == "/":
            close = next((j for j in range(i + 1, len(chars)) if chars[j] == "/"), None)
            if close is not None and _is_ipa("".join(chars[i + 1 : close])):
                i = close + 1
                continue
        out.append(chars[i])
        i += 1
    return _normalize_ws("".join(out))


def ipa_for_tts(text: str) -> str:
    """TTS form: for each appended `word /IPA/`, drop the word, KEEP the /IPA/."""
    chars = list(text)
    out = []
    i = 0
    while i < len(chars):
        if chars[i] == "/":
            close = next((j for j in range(i + 1, len(chars)) if chars[j] == "/"), None)
            if close is not None and _is_ipa("".join(chars[i + 1 : close])):
                # Drop trailing spaces, then the immediately-preceding word.
                while out and out[-1] == " ":
                    out.pop()
                while out and not (out[-1] == " " or out[-1] == "/" or out[-1] in PUNCT):
                    out.pop()
                # Re-insert a separator if the IPA now abuts a prior token.
                if out and out[-1] != " " and out[-1] != "\n":
                    out.append(" ")
                out.extend(chars[i : close + 1])
                i = close + 1
                continue
        out.append(chars[i])
        i += 1
    return _normalize_ws("".join(out))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("gloss_id", type=int)
    ap.add_argument("--tts-only", action="store_true", help="print only the TTS markup")
    args = ap.parse_args()

    if not os.path.exists(DB_PATH):
        sys.exit(f"DB not found: {DB_PATH}")

    row = sqlite3.connect(DB_PATH).execute(
        "SELECT gloss_text FROM glosses WHERE id = ?", (args.gloss_id,)
    ).fetchone()
    if row is None:
        sys.exit(f"No gloss with id {args.gloss_id}")

    verses = re.findall(r"<verse>(.*?)</verse>", row[0], re.S)
    if not verses:
        sys.exit(f"Gloss {args.gloss_id} has no <verse> (source) blocks")

    if args.tts_only:
        for v in verses:
            print(ipa_for_tts(v.strip()))
        return

    print(f"Gloss {args.gloss_id} — source verse: RAW / DISPLAY / TTS markup\n")
    for n, v in enumerate(verses, 1):
        v = v.strip()
        print(f"[verse {n}]")
        print(f"  RAW     : {v}")
        print(f"  DISPLAY : {strip_ipa(v)}")
        print(f"  TTS     : {ipa_for_tts(v)}")
        print()


if __name__ == "__main__":
    main()

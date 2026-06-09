---
name: show-gloss-ipa-tts
description: Use when inspecting the OP-IPA markup linux-lit sends to ElevenLabs for a gloss's source verse, or comparing the stored verse against the reader display and the TTS text for a given gloss id
argument-hint: <gloss-id> [--tts-only]
---

# Show Gloss IPA → TTS Markup

For a gloss id, show each source `<verse>` line in three forms:

- **RAW** — the verse as stored in `lit.db` (`glosses.gloss_text`), with appended
  `/IPA/` after the operative words (e.g. `bound /baʊnd/`).
- **DISPLAY** — what the reader sees: the `/IPA/` span removed, the word kept,
  whitespace normalized (`strip_ipa`).
- **TTS** — what is actually sent to ElevenLabs v3 to synthesize the verse audio:
  each appended `word /IPA/` replaced by just `/IPA/`, so each tagged word is
  voiced once in Original Pronunciation (`ipa_for_tts`). Untagged words are kept.

DISPLAY and TTS are the inverse of each other on the same stored line: DISPLAY
keeps the word and drops the IPA; TTS keeps the IPA and drops the word.

## Steps

1. Read the gloss id from the argument (`$1`).
2. Run the helper (read-only — never writes the DB or calls ElevenLabs):

   ```bash
   python scripts/show-gloss-ipa-tts.py <gloss-id>
   ```

   Add `--tts-only` to print just the TTS markup, one verse line per row:

   ```bash
   python scripts/show-gloss-ipa-tts.py <gloss-id> --tts-only
   ```

3. Show the output to the user. If the gloss id doesn't exist or has no
   `<verse>` blocks, the helper exits non-zero with a clear message — relay it.

## Notes

- The transform logic in `scripts/show-gloss-ipa-tts.py` is a faithful port of
  the Rust functions in `src/ui/gloss_overlay.rs` (`strip_ipa`,
  `normalize_ipa_whitespace`, `ipa_for_tts`). **If those Rust functions change,
  update the script to match** — it is a mirror, not a caller.
- The actual app sends the TTS text from `play_block_tts`
  (`src/input/actions/gloss.rs`), which calls `ipa_for_tts` on the block's raw
  text before synthesis. Only `<verse>` (Source) blocks carry IPA; `<gloss>`
  explication prose never does.
- DB path: `~/utono/litdb/data/lit.db`.

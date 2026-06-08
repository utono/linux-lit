# Gloss-driven OP IPA tagging — design

**Status:** design (not implemented). Prerequisite for the custom OP voices in
`docs/guides/elevenlabs-v3-custom-voices.md`.

## Problem

The custom Original Pronunciation (OP) voices are only useful once Shakespeare
verse actually carries OP pronunciation markup (`/IPA/`) for `eleven_v3` to read
at synthesis time. That markup has to be *produced* and *stored* somewhere. The
gloss-generation pipeline is the natural producer: the explication already
analyses each passage for "operative words", rhetorical devices, and verse
structure — exactly the words that carry OP accent and metrical stress. So the
same analysis that explains a passage should drive *which* words get tagged.

Two constraints shape the design:

1. **Sparsity.** `eleven_v3` honours inline `/IPA/` only ~80–90% of the time per
   word, non-deterministically, and the misses compound across a line (a 14-tag
   line is ~0.85¹⁴ ≈ 10% all-correct in a single render). So tagging must be
   *sparse* — only the few words that carry the line — to keep best-of-N
   selection tractable. (Figures are ElevenLabs' own rough estimate, not measured
   on this corpus; treat as planning numbers.)
2. **Two audiences, two visibilities.** The *full* sparse tagging feeds TTS, but
   the reader should see only a *pedagogical subset* — the genuinely instructive
   pronunciations — with the rest hidden. No raw `/IPA/` slashes should ever
   appear in the displayed verse.

## Scope

End-to-end: the gloss prompts, the markup contract they emit, `lit.db` storage,
the renderer's strip/show behaviour, and TTS consumption + voice selection.
Out of scope: building the OP voices themselves (the guide covers that), and the
`characters(speaker, gender)` data source (a separately-gated prerequisite, see
the guide's "Female speakers" section).

---

## 1. Markup contract

The gloss output keeps the existing `<speaker>` / `<verse>` / `<gloss>` tags and
adds two things:

### Inline `/IPA/` on `<verse>` lines — the TTS tier (full, sparse)

Each `<verse>` line carries OP `/IPA/` only on the words that carry the accent —
operative words, OP vowels, metrically-stressed syllables — **per word, never
per phrase**:

```
<verse>/ɔːr/ to /tɛːk/ /aːrmz/ /əˈgɛnst/ a /sɛː/ of /ˈtrʊblz/</verse>
```

This is the **complete** set: stripped wholesale for display, fed verbatim to
TTS. IPA encodes OP vowels (rhotic finals, FACE/GOAT monophthongs, the MEAT–MEET
split, etc.) plus stress markers `ˈ`/`ˌ` where metre/rhetoric demands — but
**syllable count is governed by line structure, not IPA** (leave `-ed`/`-ion`
expansion to the metre; see the guide).

### New `<pron>` tag — the pedagogical tier (subset, with explanation)

After a verse block, a `<pron>` note names only the 1–3 most instructive
pronunciations and *why* each matters:

```
<pron>FACE monophthong: take /tɛːk/ (not the modern gliding [eɪ]). The live
MEAT–MEET split keeps sea on /sɛː/ ([ɛː], not [iː]). These are the audibly
"not-modern" vowels in the line.</pron>
```

The reader sees `<pron>` (styled note); its IPA is shown **as-is** because this
tier is *meant* to be visible. `<pron>` is **not** speech — never sent to TTS.

**Why two tags:** `<verse>` IPA is *exhaustive* (synthesis); `<pron>` is
*selective* (teaching). Same analysis, two granularities; generated together so
they stay consistent.

---

## 2. Prompt strategy

### `TEACHER_GENERIC_PROMPT` (`src/gloss.rs:194`) gains

- **Sparsity anchored to the explication's own judgment.** Tag, on each
  `<verse>` line, *only* the words already identified as operative /
  accent-bearing / metrically stressed — "the few words that carry the line, per
  word never per phrase; tagging every word destabilizes synthesis and muddies
  the teaching." This enforces sparsity via the analysis the prompt already does,
  not a raw count.
- **What to encode** — OP vowels + `ˈ`/`ˌ` stress where metre/rhetoric demands;
  leave syllable count to line structure.
- **The `<pron>` instruction** — after each verse block, name only the 1–3 most
  pedagogically striking pronunciations and say which OP feature each
  illustrates. A strict *subset* of the words tagged in `<verse>`.

### `INNER_MONOLOGUE_PROMPT` (`src/gloss.rs:25`) gains

- The **same `<verse>` IPA tagging** (the verse needs OP for TTS regardless of
  gloss type).
- **No `<pron>` note.** Inner-monologue `<gloss>` is strictly the bracketed
  cross-work echo (the prompt forbids added prose), so pronunciation notes don't
  belong. Inner-monologue = verse IPA for TTS only.

Add/edit variants (`USER_QUESTION_PROMPT`, `EDIT_GLOSS_PROMPT`,
`INNER_MONOLOGUE_ADD/EDIT_PROMPT`) inherit the same `<verse>` tagging rule so
regenerated/edited glosses stay consistent.

---

## 3. Storage (`lit.db`)

- **Store the raw gloss text exactly as emitted** — `<verse>` lines with inline
  `/IPA/` plus `<pron>` tags — in the existing `glosses.gloss_text` blob the
  renderer already parses. **No new column**; the IPA rides inline (the "inline
  storage" case from the guide — a gloss is one mixed-tag blob with no clean
  parallel-column shape).
- **Three views derived at read time** (nothing extra persisted):
  - **TTS text** = `<verse>` content with `/IPA/` *kept*, `<pron>` excluded.
  - **Reader verse** = `<verse>` content with all `/IPA/` *stripped*.
  - **Reader `<pron>`** = shown as-is.
- **`gloss_audio` cache** (keys `gloss_id`/`kind`/`paragraph_index` +
  `voice_id`/`model_id`) is structurally unchanged. **Contract:** editing a
  gloss's verse IPA invalidates that block's cached rows (extends the existing
  delete-on-edit path — "IPA changed" is a kind of edit).
- **No schema change** for the core feature; `<pron>` awareness is a renderer
  change (§4).

---

## 4. Rendering (`src/ui/gloss_overlay.rs`)

Four changes to the parser/populator that currently knows only
`<speaker>/<verse>/<gloss>`:

1. **`strip_ipa(text) -> String`** — sibling of the existing `strip_brackets`
   (`gloss_overlay.rs:1912`). Removes `/…/` spans whose contents are IPA-class
   characters, but **not** a bare literal slash (so "and/or" survives).
2. **`<verse>` renders IPA-stripped** — run verse text through `strip_ipa` before
   inserting into the GTK buffer in `populate_gloss_buffer_ex`. The reader never
   sees `/sɛː/`.
3. **Block-range matcher compares on stripped text.** `rebuild_block_ranges`
   positions the accent bar by matching block text against the *displayed*
   buffer; if display is stripped but the block keeps IPA, the match breaks.
   Fix: **`GlossBlock` carries both** a raw `text` (with IPA → TTS) and a derived
   stripped form (display + matching). One struct, two fields, divergence in one
   place.
4. **`<pron>` renders as a styled teaching note** — parsed as a new block kind,
   shown beneath its verse block in a distinct (dimmer/italic) style like the
   existing `gloss-bracket` styling. Its IPA shows as-is. `<pron>` is **not** a
   cursor/TTS block.

**TTS path (`play_block_tts`) stays on raw `text`** — source-verse blocks send
`GlossBlock.text` (IPA-bearing) to `synthesize()` unchanged; only display reads
the stripped form. `<pron>` blocks are excluded from synthesis.

Per-verse-block data flow: one stored string → `GlossBlock { text: raw_with_IPA,
display: stripped }` → display & accent-bar matcher use `display`; TTS uses
`text`. `<pron>` → shown styled, never synthesized.

---

## 5. TTS consumption, voice selection, reliability

### Voice selection (the link to the voices guide)

When synthesizing a source-verse block, pick the voice by the speaker's gender
(the guide's A-OP / A-OP-F switch):

- Resolve gloss → speaker → gender via the `characters(work_abbrev, speaker,
  gender)` table — designed in
  [Character gender in lit.db](./2026-06-08-character-gender-design.md), a
  separate prerequisite spec (gating for voice selection only, not for the IPA
  markup itself).
- **Male → A-OP**, **female → A-OP-F**, **ambiguous / `UNKNOWN` → male (A-OP)
  fallback** — never guess.
- Send the verse block's raw IPA-bearing `text` to that voice on `eleven_v3`.
- Explication (`<gloss>`) audio, if voiced, uses **B / B-F** (modern, no IPA) —
  same gender switch.

### Reliability — best-of-N

Because v3 honours IPA ~80–90% per word (a 14-tag line ≈ 10% all-correct per
render), synthesis renders **N takes** (default 2–3) per verse block and selects
one:

- **v1 — manual.** Generate N; the user auditions and keeps the best; the chosen
  MP3 is cached in `gloss_audio`. The user's ear is, for now, the only thing that
  can confirm the **OP vowel** specifically landed (see STT note below).
- **later — auto-score (flagged future enhancement).** Optionally pick
  automatically via an STT round-trip:

  **STT round-trip checking** = text → audio → text. Transcribe each of the N
  takes and score by how well the transcript matches the intended words; keep the
  best. The catch: it depends on what the STT can *hear*.

  - An **all-ElevenLabs round-trip** (v3 TTS → ElevenLabs **Scribe** STT) catches
    only **gross** misses — dropped, garbled, or wrong words — because **Scribe
    outputs ordinary orthographic text, not phonemes**: it transcribes both the
    OP `[sɛː]` and the modern `[siː]` as the word "sea", so it is **blind to the
    OP-vowel distinction** that is the whole point. ElevenLabs offers **no
    phonetic / IPA STT** (Scribe v1/v2 outputs text + timestamps + diarization +
    audio-event tags only; IPA support is input-side TTS in v3, never output-side
    STT).
  - To score the **OP vowel** itself you need a **third-party phonetic
    recognizer** that emits IPA/phoneme strings you can diff against the target
    `/IPA/`. Realistic candidates (all output phones/IPA, unlike Scribe):
    - **wav2vec2-phoneme** (Hugging Face Transformers `Wav2Vec2Phoneme`) —
      zero-shot cross-lingual phoneme recognition; English models fine-tuned on
      TIMIT output IPA directly. The most off-the-shelf option for an
      English-only pipeline.
    - **Allosaurus** — language-agnostic universal *phone* recognizer (phone-level
      CTC + allophone→phoneme mapping); good when you want raw phones independent
      of a language model.
    - **Newer multilingual IPA recognizers** (Allophant, ZIPA, MultIPA, POWSM) —
      XLS-R / ZipFormer models trained for broad IPA coverage; heavier, useful if
      multi-language phone coverage is ever needed.
    Scoring then compares the recognizer's phoneme string for each tagged word
    against the target `/IPA/` (e.g. phoneme edit distance on the tagged spans),
    and keeps the take with the best match. This is a heavier, external pipeline
    (a separate model + alignment), which is why it is deferred past v1. See
    [Phonetic STT for automatic OP take-selection](../../guides/phonetic-stt-candidate-selection.md)
    for the candidate models, the auto-score loop, scoring, and limits.

### Input cap

`eleven_v3` caps ~5,000 chars/request. A single verse block is far under, so no
per-block chunking is needed — noted only so the implementation does not batch
many blocks into one over-cap request.

### Cache invalidation

Editing a gloss's verse IPA invalidates that block's `gloss_audio` rows (extends
delete-on-edit). The key already carries `voice_id`/`model_id`, so switching a
speaker's gender (hence voice) naturally yields a distinct cached entry.

---

## Key files

- Prompts: `src/gloss.rs` (`TEACHER_GENERIC_PROMPT:194`,
  `INNER_MONOLOGUE_PROMPT:25`, and the add/edit variants).
- Parser/renderer: `src/ui/gloss_overlay.rs` (`gloss_blocks`,
  `parse_gloss_tags`, `populate_gloss_buffer_ex`, `rebuild_block_ranges`,
  `strip_brackets` as the `strip_ipa` template).
- TTS: `src/input/actions/gloss.rs` (`play_block_tts`), `src/elevenlabs.rs`
  (`synthesize`).
- Cache: `src/db/queries.rs` (`save_gloss_audio`, `find_gloss_audio`,
  `delete_gloss_audio`).

## Open questions / future work

- The `characters(speaker, gender)` data source and its name-normalization
  (combined speakers, role-prefixed/generic names, `UNKNOWN`) — designed
  separately in
  [Character gender in lit.db](./2026-06-08-character-gender-design.md).
- Auto-score selection via an external phonetic recognizer (above) — phase 2.
- Whether `<pron>` notes should also be vocalizable (probably not; they are
  reader-facing metadata).

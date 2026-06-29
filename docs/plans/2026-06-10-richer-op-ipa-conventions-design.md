# Richer OP-IPA conventions in gloss prompts — design

**Date:** 2026-06-10
**Scope:** `src/gloss.rs` prompt strings only. No DB/schema/GTK change.

## Goal

Teach the gloss model **more Original-Pronunciation (OP) features per tagged
word** — *without* tagging more words. The "tag only operative / accent-bearing
words" rule stays the governing constraint. This is reinforced, not relaxed, by
ElevenLabs' own guidance.

Motivating gloss: `21729` (*Henry VIII*, GARDINER). The OP.pdf
(`docs/guides/OP.pdf`, Paul Meier's spec of David Crystal's OP) documents several
connected-speech and consonant features the current prompts never mention.

## ElevenLabs facts that frame the design

Confirmed against current ElevenLabs docs (2026-06):

- `eleven_v3` reads inline IPA **wrapped in forward slashes** directly in the
  text — no SSML `<phoneme>` tags. This is exactly the current mechanism.
- Guidance: **"Apply selectively: only wrap specific words or phrases that need
  pronunciation control."** This is an external endorsement of the existing
  sparsity rule.
- Guidance: **"Use standard IPA symbols from the International Phonetic Alphabet
  chart"** — no whitelist/blacklist of symbols.
- **"Include stress markers: primary (ˈ) and secondary (ˌ) for multi-syllable
  words."**
- **"Verify your IPA transcription is accurate using an IPA dictionary."**
- IPA is **80–90% consistent**; "some voices interpret IPA more accurately than
  others."

Sources:
- <https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices>
- <https://help.elevenlabs.io/hc/en-us/articles/16712320194577>

## Three parts

### 1. Refactor — single source of truth

The OP convention block (~250 words) is currently copy-pasted into all six
prompts. Extract the **sound rules** into one `OP_IPA_CONVENTIONS: &str` const and
concatenate it into each prompt (`concat!` / const concatenation). New OP
features are then added in **one place**.

The const holds *what OP sounds like* only. The per-prompt **placement /
sparsity wrappers stay in each prompt**, because they already differ:

- `USER_QUESTION`, `INNER_MONOLOGUE` (×3), `EDIT_GLOSS`: "/IPA/ only inside
  `<verse>`."
- `TEACHER_GENERIC`: the stronger "NEVER write IPA in `<gloss>` prose" rule plus
  "tag sparsely — a 40-word line has far fewer than 40 tags."

### 2. New OP features (appended to the const)

Added after the existing lexical-set + rhoticity + MEAT–MEET + `-ing→/ɪn/` rules:

- **wh → /ʍ/** — aspirated in *which, when, why, whither*; *who/whom/whole* keep
  /h/.
- **-sion/-tion → /sɪən/** (fuller) — **only when the metre admits the extra
  syllable**; otherwise /ʃən/. Reuses the existing "let line structure govern
  syllable count" carve-out (same treatment as `-ed`/`-ion`).
- **h-drop + medial v/ð elision** — drop initial /h/ on unstressed
  *his/her/him/he*; elide medial /v/ and /ð/ in common words
  (*heaven /ˈhɛən/, even, devil, seven, hither*). Casual connected speech,
  applied only to a word already being tagged.
- **Weak forms** — reduce unstressed function words to their weakest form
  (*and /ən/, of /ə/, to /tə/, for /fər/, my /mɪ/, thou /ðə/*). Framed explicitly
  as **"how to render a function word IF you tag it for a connected-speech
  effect — NOT licence to tag every function word; the operative-word rule still
  governs what gets tagged."**
- **Stress markers** — one line instructing `ˈ`/`ˌ` on multi-syllable words
  (ElevenLabs explicitly recommends this; the prompts currently only model it in
  examples).

**STRUT stays pinned `/ɤ/`** (Crystal/Meier's target). Unchanged.

### 3. Out of scope

- No recoloring of existing glosses (e.g. `judgment /ˈdʒʊdʒmənt/` → `/ɤ/` in
  21729). Prompt-only change.
- No change to the sparsity rule, `<verse>`-only placement, or the
  `<pron>`/display split.
- No DB/schema change (the `op_ipa_text` column sketch is not adopted).

## Verification

- `cargo build`
- `cargo test --bins`

Prompt-string change with no GTK/render surface, so the headless e2e is not
required. Runtime proof = re-gloss a passage and listen (user-driven).

## Follow-up

Update memory `project_op_convention_block_prompt.md` to record the refactor and
the four added features.

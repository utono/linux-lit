# What `/IPA/` Is Sent to ElevenLabs (a worked example)

This guide shows, end to end, **exactly which text — and which `/IPA/` — leaves
linux-lit and reaches the ElevenLabs API** when a gloss is read aloud. It uses a
single real gloss (id `21726`, a *Henry VIII* speech by GARDINER) as the worked
example.

For the design rationale behind the two-tier markup and the OP voices, see
[`elevenlabs-v3-custom-voices.md`](elevenlabs-v3-custom-voices.md). This guide is
the **concrete data-flow companion**: not *why*, but *what bytes go out*.

## The one-sentence rule

> Only `<verse>` and non-echo `<gloss>` text is synthesized. `<verse>` is sent
> **with its inline `/IPA/` intact**; `<gloss>` prose is sent as-is (usually no
> IPA). `<speaker>` and `<pron>` are **never** sent to TTS — `<pron>` is a
> reader-only pronunciation note, so the `/IPA/` *inside a `<pron>` block never
> reaches ElevenLabs*.

## The four gloss tags

A gloss's `gloss_text` (column in the `glosses` table) is marked up with four
tags. linux-lit parses them in `src/ui/gloss_overlay.rs`
(`parse_gloss_tags` → `gloss_blocks`):

- **`<speaker>NAME</speaker>`** — the speaking character. **Dropped** from all
  block text. Not displayed in block text, not synthesized. (It still drives
  *voice selection* — see below.)
- **`<verse>…</verse>`** — a line of the source verse, carrying inline OP
  `/IPA/`. A contiguous run of `<verse>` lines becomes **one Source block**.
  **Synthesized, `/IPA/` intact.**
- **`<gloss>…</gloss>`** — an explication paragraph (editorial prose). Each
  non-echo paragraph becomes **one Explication block**. **Synthesized.** Echo
  `<gloss>` paragraphs (the bracketed cross-work echoes) are skipped entirely.
- **`<pron>…</pron>`** — a pronunciation teaching note for the reader. **Dropped:
  not a cursor stop, not TTS.** Any `/IPA/` it quotes is for the *reader's eyes
  only* and is never synthesized.

Each parsed `GlossBlock` carries two strings:

- **`text`** — the **raw** form, `/IPA/` preserved. *This is the TTS payload.*
- **`display`** — `text` with `/IPA/` stripped (`strip_ipa`). *This is what the
  reader sees in the GTK buffer.*

The split is the whole trick: `strip_ipa` is applied to build `display` and is
**never** applied to `text`, so the reader never sees `/drɛːd/` while ElevenLabs
always does.

## The source gloss (id 21726, verbatim)

```
<speaker>GARDINER</speaker>
<verse>Dread /drɛːd/ sovereign, how much are we bound /buːnd/ to heaven /ˈhɛvən/</verse>
<verse>In daily /ˈdɛːli/ thanks, that gave /gɛːv/ us such a prince,</verse>
<verse>Not only good and wise /wəɪz/, but most religious;</verse>
<verse>One that in all obedience makes the Church /tʃərtʃ/</verse>
<pron>"Dread" /drɛːd/ and "gave" /gɛːv/ show the OP FACE-vowel as a long monophthong rather than the modern diphthong /eɪ/, while "Church" /tʃərtʃ/ rings out the rhotic R that anchors Gardiner's ecclesiastical authority.</pron>
<gloss>Gardiner opens with a calculated piece of flattery aimed at King Henry, whom he addresses as "Dread sovereign" — "dread" here meaning awe-inspiring, not frightening in the modern sense. He thanks heaven for giving England a prince who is not merely virtuous and intelligent but, crucially for Gardiner's argument, "most religious." The operative words climb a ladder of value: good, wise, religious — with "religious" winning because it serves Gardiner's purpose, which is to frame Henry as the Church's defender. Rodenburg would mark "Dread," "heaven," "prince," and "religious" as the points where the breath must land with conviction.</gloss>

<speaker>GARDINER</speaker>
<verse>The chief /tʃiːf/ aim of his honor /ˈɒnər/, and to strengthen</verse>
<verse>That holy /ˈhoːli/ duty out of dear /deːr/ respect,</verse>
<verse>His royal self in judgment /ˈdʒʊdʒmənt/ comes to hear /heːr/</verse>
<verse>The cause betwixt her and this great /grɛːt/ offender.</verse>
<pron>"Dear" /deːr/ and "hear" /heːr/ preserve the MEAT vowel — the long close-mid /eː/ distinct from MEET /iː/ — and both ring their final R; "great" /grɛːt/ keeps its older open vowel, the irregular survivor of that same MEAT class.</pron>
<gloss>Gardiner now sets up the political move: because Henry makes the Church "the chief aim of his honor," the King has personally descended to judge the case against Cranmer, whom Gardiner pointedly refuses to name, calling him only "this great offender." The phrase "betwixt her and this great offender" personifies the Church as a wronged woman ("her") set against a villain — a rhetorical device called prosopopoeia, the giving of voice or person to an abstraction. Cicely Berry would have the actor feel how the smooth, deferential surface of the verse masks a prosecutor's brief: every iambic beat is laying a trap. The emotional arc moves from public piety to private accusation, and the final word, "offender," must land with quiet, weighted finality.</gloss>
```

## What the parser produces: 4 blocks → 4 TTS requests

Two `<verse>` runs + two non-echo `<gloss>` paragraphs = **four blocks**, each one
its own ElevenLabs request and its own `gloss_audio` cache row. The two `<pron>`
notes and the two `<speaker>` labels produce **no** blocks.

### Source block 0 — `kind=source, index=0` (verse voice, `/IPA/` intact)

Verse lines joined by `\n`, sent **exactly** as below:

```
Dread /drɛːd/ sovereign, how much are we bound /buːnd/ to heaven /ˈhɛvən/
In daily /ˈdɛːli/ thanks, that gave /gɛːv/ us such a prince,
Not only good and wise /wəɪz/, but most religious;
One that in all obedience makes the Church /tʃərtʃ/
```

`/IPA/` actually sent here: `/drɛːd/` `/buːnd/` `/ˈhɛvən/` `/ˈdɛːli/` `/gɛːv/`
`/wəɪz/` `/tʃərtʃ/`.

### Explication block 0 — `kind=explication, index=0` (prose voice)

No inline `/IPA/`, so raw == display. Sent as:

```
Gardiner opens with a calculated piece of flattery aimed at King Henry, whom he addresses as "Dread sovereign" — "dread" here meaning awe-inspiring, not frightening in the modern sense. He thanks heaven for giving England a prince who is not merely virtuous and intelligent but, crucially for Gardiner's argument, "most religious." The operative words climb a ladder of value: good, wise, religious — with "religious" winning because it serves Gardiner's purpose, which is to frame Henry as the Church's defender. Rodenburg would mark "Dread," "heaven," "prince," and "religious" as the points where the breath must land with conviction.
```

### Source block 1 — `kind=source, index=1` (verse voice, `/IPA/` intact)

```
The chief /tʃiːf/ aim of his honor /ˈɒnər/, and to strengthen
That holy /ˈhoːli/ duty out of dear /deːr/ respect,
His royal self in judgment /ˈdʒʊdʒmənt/ comes to hear /heːr/
The cause betwixt her and this great /grɛːt/ offender.
```

`/IPA/` actually sent here: `/tʃiːf/` `/ˈɒnər/` `/ˈhoːli/` `/deːr/` `/ˈdʒʊdʒmənt/`
`/heːr/` `/grɛːt/`.

### Explication block 1 — `kind=explication, index=1` (prose voice)

```
Gardiner now sets up the political move: because Henry makes the Church "the chief aim of his honor," the King has personally descended to judge the case against Cranmer, whom Gardiner pointedly refuses to name, calling him only "this great offender." The phrase "betwixt her and this great offender" personifies the Church as a wronged woman ("her") set against a villain — a rhetorical device called prosopopoeia, the giving of voice or person to an abstraction. Cicely Berry would have the actor feel how the smooth, deferential surface of the verse masks a prosecutor's brief: every iambic beat is laying a trap. The emotional arc moves from public piety to private accusation, and the final word, "offender," must land with quiet, weighted finality.
```

## The `/IPA/` that is NOT sent (the `<pron>` notes)

These two notes are displayed to the reader but **never synthesized**. The IPA
they quote — even though it is identical to verse IPA — does not reach
ElevenLabs, because the whole `<pron>` block is discarded before any TTS request:

```
"Dread" /drɛːd/ and "gave" /gɛːv/ show the OP FACE-vowel …      (NOT sent)
"Dear" /deːr/ and "hear" /heːr/ preserve the MEAT vowel …       (NOT sent)
```

If you want the listener to *hear* a pronunciation point the `<pron>` note
describes, that word must also carry its `/IPA/` inside a `<verse>` line (which it
does here — `/drɛːd/`, `/deːr/`, `/heːr/` all appear in the verse), or be
OP-tagged inside a `<gloss>` paragraph per the *"When the prose quotes the verse"*
exception in the voices guide.

## What the reader sees instead (the stripped display)

For contrast, the GTK buffer shows the same blocks with `/IPA/` removed by
`strip_ipa`. Source block 0 renders as:

```
Dread sovereign, how much are we bound to heaven
In daily thanks, that gave us such a prince,
Not only good and wise, but most religious;
One that in all obedience makes the Church
```

The reader never sees a single slash; ElevenLabs always does.

## Which voice each block is sent to

Voice selection is per **block kind** (verse vs prose), keyed on the **speaker's
gender and age**, resolved against the `voice_catalog` table in `lit.db`:

1. **Per-gloss override** — if a voice is attached to this gloss in the
   `gloss_voices` table, it is used for every block of the gloss.
2. **`voice_catalog` lookup** — otherwise `(gender, age, role)` where
   `role = "verse"` for Source blocks and `role = "prose"` for Explication
   blocks. Source code: `resolve_default_voice` in `src/db/queries.rs`.
3. **Hard-coded fallback** — `voice_for(gender, is_verse)` in
   `src/elevenlabs.rs` if the catalog misses.

GARDINER is male. Each character voice is now used for **both** roles (one
`voice_id`, two `voice_catalog` rows), so the male bands are:

- male, verse **and** prose → `6yZ2TgQ0ylkuKI3AMAbI` (Romeo, 15–25) or
  `8BQp5xsRbw3h92wPAOm9` (Petruchio, 35–45), depending on resolved age

So in this gloss, the two **Source** blocks and the two **Explication** blocks
all go to the **same** male voice. The only difference between them is the
*text*: the Source (verse) blocks carry OP `/IPA/`, the Explication (prose)
blocks do not — the `role=verse`/`role=prose` rows resolve to the identical
`voice_id`. The model is **always `eleven_v3`** — the only model that reads
`/IPA/` (`eleven_turbo_v2_5` is used only as a paid-plan-required fallback and
silently ignores `/IPA/`).

## Cache: 4 blocks → 4 rows

Each block's audio is cached in `gloss_audio`, keyed on
`(gloss_id, kind, paragraph_index, voice_id)`:

- `(21726, "source",      0, <verse_voice>)`
- `(21726, "explication", 0, <prose_voice>)`
- `(21726, "source",      1, <verse_voice>)`
- `(21726, "explication", 1, <prose_voice>)`

`kind` is the literal string `"source"` / `"explication"`; `paragraph_index` is
the 0-based within-kind block index. Editing the gloss's text (including its
`/IPA/`) must invalidate these rows — the same staleness contract as any
gloss-text edit.

## Source-of-truth files

- **Parse + strip:** `src/ui/gloss_overlay.rs` — `parse_gloss_tags`,
  `gloss_blocks`, `strip_ipa` (and `strip_brackets` for the line-number gutter,
  display-path only).
- **TTS payload + voice pick:** `src/input/actions/gloss.rs` — `play_block_tts`,
  `synth_via`.
- **API body:** `src/elevenlabs.rs` — `build_body` sends `{ "text", "model_id" }`
  verbatim; `voice_for` fallback constants.
- **Voice resolution + cache:** `src/db/queries.rs` — `resolve_default_voice`,
  the `gloss_audio` read/write.

# Creating Custom Eleven v3 Voices

How to design custom ElevenLabs voices with the **Eleven v3** model for narrating
`lit.db` content — English blank verse / iambic pentameter, and the prose
explication of that verse. v3 is ElevenLabs' most expressive TTS model and the
one to use when delivery (line cadence, restraint, emphasis) matters as much as
timbre.

Primary source: the ElevenLabs Voice Design prompting guide
(<https://elevenlabs.io/docs/eleven-creative/voices/voice-design#prompting-guide>)
and the v3 prompting best-practices page
(<https://elevenlabs.io/docs/best-practices/prompting/eleven-v3>).

## What v3 changes vs v2

- **Far more expressive.** v3 follows emotional and delivery direction much more
  closely than v2 — the right model when you need "measured, dignified, honouring
  the line" rather than a flat read.
- **Audio tags.** v3 interprets inline bracketed tags (`[whispers]`,
  `[sighs]`, `[sarcastic]`) as delivery instructions. v2 ignores these.
- **A stability slider that matters.** v3's stability setting trades adherence
  to the reference voice against expressive freedom (see below).
- **No SSML break tags.** v3 does **not** support `<break>` (every other
  ElevenLabs model does). Control pauses with punctuation and line structure
  instead — and, where a comma reads too lightly, with v3's own bracketed pause
  tags `[pause]` / `[short pause]` / `[long pause]` (these are v3-exclusive and
  silently ignored by other models). Reach for a `[pause]` only for a deliberate
  held caesura; routine verse pacing should come from punctuation and line
  breaks. Too many forced breaks of any kind cause instability (speed-ups,
  artifacts, stray noises).
- **IPA pronunciation.** v3 natively supports International Phonetic Alphabet
  transcription across 70+ languages — write IPA wrapped in **forward slashes**
  (`/tɛːk/`) directly in the text, no XML. This is v3-specific: the legacy
  `<phoneme>` SSML tag works only on `eleven_flash_v2` / `eleven_turbo_v2` /
  `eleven_monolingual_v1` (Eleven English V1) and is silently skipped by every
  other model — including `flash_v2_5`, `multilingual_v2`, and v3 itself. The
  slash-IPA is the lever for an Original
  Pronunciation voice (see below): the only reliable way to push v3 toward
  mid-shift vowels and rhoticity, since "Original Pronunciation" is not a
  selectable accent. v3's IPA lands ~80–90% consistently, so generate several
  takes and keep the best.
  - **Syntax to keep straight:** in v3, `[brackets]` = **delivery** (whispers,
    sighs); `/slashes/` = **pronunciation** (IPA). They are not the same thing.

## Voice Design vs. cloned voices

Two different things produce a "custom voice":

- **Voice Design (Text to Voice)** — you write a *description prompt* plus
  *preview text*, the model generates several candidate voices, you save the one
  you like. Fully synthetic; no recordings needed. This is what this guide covers.
- **Professional / instant cloning** — built from real audio recordings. There is
  no recoverable prompt for a cloned voice (e.g. `KjWPwHJWLungxeiYigoM`
  "Will – Poetical & Measured" is a `professional` clone — you can target its
  *qualities* with a Voice Design prompt but cannot copy its prompt).

### Why Voice Design, not cloning, for the OP voice

It is tempting to think a voice cloned from real recordings would produce a
better Original Pronunciation read than a synthetic one. For this pipeline it
would not — the synthetic-vs-cloned choice is **orthogonal to OP**, and Voice
Design is the better fit. Four reasons:

1. **The OP accent lives at render time, not in the voice.** This guide's
   central reframe (see *The hard constraint*) is that the OP vowels are carried
   by `/IPA/` in the narration text on every render; the saved voice — cloned or
   designed — durably keeps only **timbre and character**. A clone absorbs OP
   pronunciation no more than a designed voice does, so you would *still* tag
   `/ɔːr/`, `/tɛːk/`, `/sɛː/` word by word at render time. Cloning changes whose
   timbre you start from, never whether OP is reproduced — it leaves the hard
   part of the pipeline untouched.
2. **You'd have to source OP audio to clone OP — which is the whole problem.**
   A clone reproduces the accent of its source recordings. A faithful OP read
   needs the Crystals' dictionary and a trained actor (see *Honest caveat*); if
   you had that audio the synthesis question would be moot. Clone an ordinary
   actor instead and you inherit *their* accent (usually RP or modern), then
   fight it back toward rhotic/earthy with `/IPA/` — strictly harder than Voice
   Design, where you simply *describe* "strongly rhotic, every R sounded, earthy,
   never posh" and audition for it.
3. **The high-fidelity clone needs a tier you may not have.** Instant cloning
   (a minute or two of audio) is lower-fidelity and inherits the source's room
   tone, mic colour, and real accent. Professional cloning — the kind that could
   plausibly capture a trained OP actor — requires a paid tier
   (`professional_voice_limit > 0`) *and* the recordings. On a free/starter plan
   it is unavailable regardless.
4. **Cloning wins only when the goal is a specific person's timbre — not OP.**
   If you ever want to sound *exactly* like one named narrator (the thing a
   Voice Design prompt can only approximate), cloning is correct. But that is a
   timbre-fidelity goal, not an accent goal; it still relies on render-time
   `/IPA/` for OP and so does nothing the designed voice doesn't already do here.

**Bottom line:** use Voice Design for Voice A-OP. Reach for cloning only if you
need a particular known voice's exact timbre *and* have both the recordings and
professional-cloning access — neither of which advances OP fidelity.

## The prompt structure

ElevenLabs' recommended Voice Design prompt format:

```
Native <Language>. <Gender>, <Age range>. <Quality level>.
Persona: <2–5 words>. Emotion: <2–3 adjectives>.
<1–2 sentences about timbre, pacing, delivery>
```

More descriptive, granular prompts yield more accurate, nuanced voices. Short
prompts work for neutral voices but flatten the kind of literary delivery you
want here.

Dimensions to describe:

- **Age** — e.g. "late 40s to 50s", "elderly", "in his 80s"
- **Gender** — male / female / gender-neutral
- **Timbre** — warm, resonant, gravelly, smooth, buttery, raspy, throaty
- **Pitch** — low / normal / high-pitched
- **Accent** — name the identity; modify with "slight" or "thick" (e.g. "slight
  West Country lean", "thick Scots"). For an *unlabelable* accent like Original
  Pronunciation, describe its features instead of naming it — see Voice A-OP.
- **Pacing** — calm, deliberate, natural, drawn out, staccato
- **Emotion** — measured, contemplative, dignified, warm, conversational
- **Persona / profession** — "classical stage actor", "literary guide"
- **Audio quality** — ok / good / very good / excellent / studio / broadcast

## Audio tags (v3 only)

**Where tags act: in the render text, not the creation prompt.** Audio tags are
*delivery instructions for a render* — v3 acts on them only in the narration text
you send at synthesis time. They do **nothing** in the Voice Design **description
prompt**, which describes the voice's *identity* (timbre, character), not a
performance — write that field as plain prose, never with `[tags]`. The Voice
Design **preview text** is narration-like, so a tag there would register, but you
generally want the preview plain so you audition intrinsic timbre, not painted-on
performance (see the OP preview, which is deliberately tagless). So: describe the
voice in the prompt; reach for tags only in the text you actually render.

Inline bracketed tags direct delivery. Put a tag immediately before the text it
affects. Categories and examples:

- **Emotion / tone** — `[excited]`, `[sad]`, `[sarcastic]`, `[curious]`,
  `[serious]`, `[whispers]`
- **Non-verbal** — `[laughs]`, `[sighs]`, `[exhales]`, `[giggles]`,
  `[lip smacks]`, `[clears throat]`
- **Sound effects** — `[gunshot]`, `[clapping]`, `[explosion]` (rarely useful for
  literary narration)

Reliability notes:

- Tags work best on **v3-compatible, expressive voices**; a very "stable" or
  neutral voice may ignore them.
- Use tags sparingly for narration — a few well-placed `[whispers]` or `[sighs]`
  beat tag-heavy text, which destabilizes the read.
- For verse you usually want **no tags at all** — let the metre and punctuation
  carry the delivery (see below). Reserve tags for moments the text genuinely
  calls for.

## Stability setting

v3's stability slider is the most important knob:

- **Creative** — most expressive, follows tags and emotion hardest; least
  predictable, more prone to artifacts.
- **Natural** — balanced; the usual default for narration.
- **Robust** — most stable and consistent; least responsive to tags and emotional
  direction.

For literary narration, start at **Natural**. Move toward **Robust** if the read
wanders or adds unwanted emotion; move toward **Creative** only if it's too flat
and you need the tags to land.

The Voice Design generation panel also exposes:

- **Guidance Scale** — how strictly the voice adheres to your description. Higher
  = closer to the prompt but can reduce audio quality on niche voices; lower =
  prioritizes quality/performance over precision.
- **Loudness** — output level of the generated voice.

## Controlling delivery with text, not tags

Because v3 has no break tags, shape the read with the text itself:

- **Punctuation is timing.** Commas = short pause; periods = full stop;
  em-dashes = a held beat; ellipses (…) = a trailing pause. This is the main
  pause mechanism in v3.
- **Capitalization adds emphasis.** A WORD in caps reads with stress — use it
  surgically, never on whole lines.
- **Line breaks carry verse cadence.** Keep hard line breaks in blank verse;
  v3 lifts and breathes at line ends. Do not reflow verse into a paragraph —
  that flattens iambic pentameter into prose rhythm.

## Preview / generation text — the highest-value lever

Voice Design builds the voice from the **sample text** you give it, so the preview
must *be the kind of text you'll narrate*:

- **Verse voice → paste real blank verse from `lit.db`** with line breaks
  preserved. The model learns the metrical cadence from the sample.
- **Prose voice → paste a real explication paragraph** from `lit.db`.
- **Give a full passage, not a phrase.** Longer previews produce more stable,
  expressive results.
- **Match emotion to text.** Don't pair a "measured, contemplative" prompt with
  exclamatory sample text — alignment between prompt and preview text matters.

## Recommended setup for lit.db: two linked voices

Build **two voices that share one identity** so the listener hears a single
narrator switching modes — verse measured by the line, prose measured by the
sentence. Keep the identity baseline (a clear, resonant, higher-pitched ringing
theatrical tenor) byte-identical across both prompts; only accent register and
delivery change — rhotic OP for the verse, neutral modern for the prose.

- **Voice A-OP — verse** in Original Pronunciation (rhotic, Shakespeare-era).
  This is the primary verse voice; see its full section immediately below.
- **Voice B — prose explication**: the same voice, but reading the guide's
  *own* explanatory prose in a neutral modern register (the explication is
  editorial commentary, not stage speech, so it does **not** take OP). See below.

There is no separate RP "classical" verse voice: an RP read of Shakespeare is a
nineteenth-century anachronism (see the OP section), so the verse voice **is**
Voice A-OP.

### Saved voice IDs (built)

The four voices below have been created via Voice Design and saved. Render with
**`eleven_v3`** only (the sole model that reads `/IPA/` and `[…]` audio tags).
Verse voices take OP `/IPA/` in the narration text at render time; prose voices
are neutral modern (no `/IPA/`).

- **Will OP — Verse (A-OP)** — male verse, Original Pronunciation —
  `qIorOnPHyesnVMLvolyz`
- **Will — Prose (B)** — male prose explication, neutral modern —
  `jTudAEr52RK5998TOYLM`
- **Willa OP — Verse (A-OP-F)** — female verse, Original Pronunciation —
  `AJEmTDfBuB294lokNL10`
- **Willa — Prose (B-F)** — female prose explication, neutral modern —
  `EKXvXWSM0PF7VaEykbP4`
- **Petruchio OP — Verse (C-OP)** — male verse (older, swaggering), Original
  Pronunciation — `8BQp5xsRbw3h92wPAOm9`
- **Petruchio — Prose (D)** — male prose explication (older), neutral modern —
  `0C3liWwHU3pG3IcPThkh`
- **Beatrice OP — Verse (E-OP)** — female verse (sharp-witted, ~25), Original
  Pronunciation — `d0tyHmCGhjY1al3AD4mO`
- **Beatrice — Prose (F)** — female prose explication (~25), neutral modern —
  `FTksVX7bTBbE2R5yfiYi`

Source clone they were modelled on: **Will – Poetical & Measured** (a
`professional` clone) — `KjWPwHJWLungxeiYigoM`.

**Selection rule at render:** speaker's gender → `{Will set | Willa set}`, then
verse → OP voice + `/IPA/`, prose → plain voice. Default to the **male** set when
gender is ambiguous or unresolved. The female set is only *auto-selected* once a
speaker → gender data source exists in `lit.db` (see *Female speakers* below and
the character-gender design spec) — that data source does not exist yet.

### Female speakers: a mirrored voice set, selected by speaker gender

The A-OP / B male voice reads male characters. Female characters — Ophelia,
Portia, Lady Macbeth, Cleopatra — need their own timbre, so build a **mirrored
female set** and select the pair per gloss by the **gender of the character who
speaks the source passage**:

- **Male speaker → A-OP (verse) + B (prose explication)** — the male set
  above.
- **Female speaker → A-OP-F (verse) + B-F (prose explication)** — a female-timbre
  clone of the same two voices.

A-OP-F and B-F use the **identical pipeline** as their male counterparts — same
OP/rhotic treatment, same render-time `/IPA/`, same two-linked-voices identity
trick (one female narrator switching verse/prose modes). **Only the timbre
changes**: build them with the same description prompts but `Gender: female`,
the same `15 to 25` age as the male set, and a colour that reads as the same
*kind* of narrator — a clear, bright, resonant young female voice (a high,
ringing soprano/light-mezzo, not deep), with the same crisp articulation and
authority-beyond-its-years. Everything else in this guide — the `/IPA/` render
strings, stability, audio-tag discipline, the input cap, hiding `/IPA/` from the
reader — applies unchanged to the female set.

So the explication voice (B vs B-F) follows the *speaker's* gender too: a female
character's gloss is narrated by B-F even though the explication is editorial
commentary, so a listener hears one consistent female narrator across both the
quoted verse and its gloss. (If you would rather keep all explication in a single
narrator regardless of character gender, use B for every gloss and switch only
the *verse* voice A-OP ↔ A-OP-F — a defensible alternative; pick one and be
consistent.)

**Prerequisite: a speaker → gender data source (does not exist yet).** linux-lit
can already resolve a gloss to its **speaker name** (`passages.character`, or
`line_mapping.speaker` over the citation span — e.g. `HAMLET`, `OPHELIA`), but
**nothing in `lit.db` stores a character's gender**. Before this feature can
choose a voice you must add that mapping — designed in
[Character gender in lit.db](../superpowers/specs/2026-06-08-character-gender-design.md)
(true-gender per character, LLM-assigned, `male`/`female`/`neutral`/`unknown`,
default-to-male fallback). Its core is a small table:

```sql
CREATE TABLE characters (
  work_abbrev TEXT,
  speaker     TEXT,   -- normalized speaker name as it appears in line_mapping
  gender      TEXT,   -- 'male' | 'female' | 'neutral'
  PRIMARY KEY (work_abbrev, speaker)
);
```

populated from each play's dramatis personae, then joined on
`passages.character` / `line_mapping.speaker`. Mind the **name-normalization edge
cases** the speaker strings carry, all of which need a rule and a safe fallback:

- **Combined speakers** — `CORNELIUS / VOLTEMAND`, `BARNARDO / MARCELLUS` (two
  characters on one line).
- **Role-prefixed / generic names** — `PLAYER KING`, `FIRST PLAYER`, `KING`,
  `QUEEN`, `GHOST`.
- **Edition-qualified names** — `ANTIPHOLUS OF SYRACUSE`.
- **`UNKNOWN`** (the gloss-context fallback when no speaker resolved).

When gender is ambiguous or unresolved, fall back to the **male (A-OP/B)** set
(or a neutral voice) rather than guessing — a wrong-gender read is more jarring
than a default one. This data source is the gating work; once it exists, voice
selection is a single `gender → {A-OP/B | A-OP-F/B-F}` switch at synthesis time.

## Voice A-OP — verse in Original Pronunciation (the Shakespeare-era accent)

This is the verse voice to build: **like "Will – Poetical & Measured"
(`KjWPwHJWLungxeiYigoM`) but in the British accent commonplace in Shakespeare's
own time** — not the
Received Pronunciation of Gielgud/Olivier, which is a nineteenth-century
anachronism on the Jacobean stage. The target is **Original Pronunciation (OP)**,
the reconstruction by David and Ben Crystal of educated London speech around 1600.

### What OP actually is (and why RP is wrong here)

OP is the accent in which *Hamlet* was first spoken. Its load-bearing features
(best-attested first):

- **Rhotic.** Every written *r* is sounded and colours the vowel before it —
  *art*, *bird*, *for*, *here*. (Postvocalic *r*-loss in southern English is an
  eighteenth-century development, *after* Shakespeare.) This is the single
  feature that most separates OP from RP.
- **Caught mid-Great-Vowel-Shift.** Long vowels have started but not finished
  their migration. FACE (*name, day*) is a monophthong [ɛː], not gliding [eɪ];
  GOAT (*home, stone*) is [oː], not [əʊ]; PRICE (*time, mine*) has a centred
  onset [əɪ], MOUTH (*house, now*) [əʊ].
- **No TRAP–BATH split.** *bath, path, grass* take a short front [a] — no
  "broad a."
- **Incomplete FOOT–STRUT split.** *cut* ≈ *put*, *blood* near *good*.
- **The *wh*/*w* distinction holds** ([ʍ] in *which*, *whales*); final **-ing**
  is plain [ɪn] ("runnin'", "lovin'" as the norm, not vulgar).
- **Faster, earthier, conversational, classless.** OP runs quicker and lands
  flatter than cathedral-cadence "classical" delivery, and carries **no** social
  register — none of RP's "elevated" colour. It restores buried rhymes
  (*proved/loved*) and bawdy puns (*hour/whore/ore*; *reason/raisin*).

### The hard constraint: ElevenLabs has no "OP" accent

Voice Design cannot be told "Original Pronunciation" — it's a scholarly
reconstruction, not a labelable accent, and (per the source) listeners can't even
agree what it resembles (Irish, West Country, Scots, American, Yorkshire all get
named, and the disagreement is the point). So **do not** name OP, RP, or any
single modern accent in the description prompt. Instead:

1. **Describe OP's features in the prompt**, not a label — "rhotic", "every *r*
   sounded", "earthy", "fast and conversational", "no elevated or posh colour".
   The prompt sets **timbre and character**; that — not the vowels — is what the
   saved voice durably keeps.
2. **Carry the real pronunciation as `/IPA/` in the narration text at render
   time.** This is the key reframe: Voice Design saves a voice *identity*
   (timbre), and IPA placed in the preview is best treated as steering only
   which candidate you pick — not as something absorbed into the saved voice as
   permanent pronunciation. (This is also why the preview below is left *plain*:
   you want to audition the voice's intrinsic timbre, not vowels painted on by
   IPA.) The per-word OP vowels must be re-supplied as `/IPA/` in the **actual
   text every render** (see the worked example and the `lit.db` sketch
   below). The preview's only durable job is timbre audition.
3. **Cadence is render-time too.** Durable verse cadence is carried by the
   punctuation and hard line breaks of the narration text at render time, not by
   anything absorbed from the preview — keep the line breaks in the rendered
   verse so v3 lifts and breathes at line ends.
4. **Set realistic expectations.** v3 will *lean* toward a rhotic, earthier read
   and its IPA is ~80–90% consistent (identical IPA can vary); it will not
   deliver a phonetically exact 1600 reconstruction. Treat the result as
   OP-flavoured, generate several takes, and fix individual mis-stressed /
   mis-vowelled words by editing that word's `/IPA/` in the narration text,
   never the prompt.

### Description prompt (OP-flavoured, no accent label)

```
Native English. Male, 15 to 25. Studio quality.
Persona: Elizabethan stage player, Shakespearean narrator. Emotion: measured, dignified, earthy, quietly intense.
A clear, resonant, higher-pitched young voice — a bright, ringing theatrical tenor, light rather than a deep baritone, with crisp articulation and an effortless natural authority beyond its years. Strongly rhotic — every written R is sounded and colours the vowel before it. Vowels sit slightly archaic and old-fashioned, caught between medieval and modern. Reads at a brisk, conversational, plain-spoken pace, honouring the verse line with a light lift at each line ending, but never posh, never elevated, never refined — earthy and direct rather than cathedral-grand. The metre is felt, never hammered; sense flows across line breaks.
```

The identity baseline (`Native English. Male, 15 to 25. Studio quality.` +
"A clear, resonant, higher-pitched ringing theatrical tenor") stays byte-identical
to Voice B so the two read as one narrator — only the *accent* and *register*
change: here, rhotic, earthy, and plain for the verse; in Voice B, neutral modern
for the explanatory prose.

### Preview text — audition timbre, not vowels

The Voice Design preview's job for this voice is **timbre/character audition**:
paste a real verse passage with line breaks preserved so you can pick the
candidate that *sounds* rhotic, earthy, and plain. Do not rely on the preview to
bake in OP vowels — those are render-time work (above). A plain passage is fine
here:

```
To be, or not to be, that is the question:
Whether 'tis nobler in the mind to suffer
The slings and arrows of outrageous fortune,
Or to take arms against a sea of troubles
And by opposing end them.
```

Use **no audio tags**. Stability: **Natural** (move toward **Robust** only if the
rhotic, faster read starts adding unwanted emotion).

### Render string — OP encoded in `/IPA/` (feed this to `eleven_v3`)

At narration time, supply the per-word OP values as `/IPA/`. Tag **the few words
that carry the accent, per word, not per phrase** — over-tagging destabilizes v3.
For the *Hamlet* 3.1 passage, ~14 of ~40 words are tagged:

```
To be, /ɔːr/ not to be, that is the question:
/ˈʍɛðər/ 'tis /ˈnoːblər/ in the /məɪnd/ to /ˈsʊfər/
The slings and /ˈaroːz/ of /əʊˈtrɛːdʒəs/ /ˈfɔːrtjuːn/,
/ɔːr/ to /tɛːk/ /aːrmz/ /əˈgɛnst/ a /sɛː/ of /ˈtrʊblz/
And /bəɪ/ /əˈpoːzɪn/ end them.
```

Why those words carry the accent:

- **Rhotic finals** sound every *r* as vowel + `r`: `/ɔːr/` (*or*), `/ˈsʊfər/`
  (*suffer*), `/aːrmz/` (*arms*), `/ˈfɔːrtjuːn/` (*fortune*).
- **Mid-shift monophthongs** are the most audibly "not modern": FACE `/tɛːk/`
  (*take*, not gliding [eɪ]), GOAT `/ˈnoːblər/` (*nobler*) and `/əˈpoːzɪn/`
  (*opposing*), and the live MEAT–MEET split in `/sɛː/` (*sea* on [ɛː], not
  [iː]).
- **Centred PRICE/MOUTH onsets**: `/məɪnd/` (*mind*), `/bəɪ/` (*by*),
  `/əʊˈtrɛːdʒəs/` (*outrageous*).
- **Incomplete FOOT–STRUT split**: `/ˈsʊfər/`, `/ˈtrʊblz/`.
- ***wh* = [ʍ]**: `/ˈʍɛðər/`; **final `-ing` → [ɪn]**: `/əˈpoːzɪn/`.

**Stress markers do double duty for scansion.** Use `ˈ` (primary) / `ˌ`
(secondary) on multi-syllable words to force metrical stress phonetically instead
of only nudging punctuation — e.g. the older *revénue*:

```
/ˈrɛvɪnjuː/    (stress 1st syllable — older / metrical)
/rɪˈvɛnjuː/    (stress 2nd syllable — modern default)
```

**But let line structure, not IPA, govern syllabicity.** Leave `-ed` and `-ion`
to the metre word by word: in this passage *question* and *fortune* stay
disyllabic (both are feminine line endings — expanding `-ion` to [sɪ.ən] would
add a syllable and break the line), so they are deliberately left untagged.
Encode the segmental vowels in `/IPA/`; let syllable count decide `-ed`/`-ion`.

`/sɛː/` could be `/seː/` if the MEAT set sounds too open in audition — both are
defensible for c. 1600. Audition and pick.

### Honest caveat

This produces a voice that *leans* Early Modern — rhotic, brisker, un-posh — and
is a far better fit for Shakespeare than an RP "classical" read. It is not a
verified OP reconstruction; for that there is no synthetic shortcut, only the
Crystals' dictionary and a trained actor. Treat Voice A-OP as "Will, but rhotic
and earthy," audition the previews for timbre, and refine word-by-word in the
`/IPA/` of the **narration text** (not the prompt or preview).

### `lit.db` integration sketch

Because the IPA fires at render time, store the annotated render string as a
sibling column to the display text — the renderer never derives IPA on the fly
and the reader-facing text stays clean:

```sql
ALTER TABLE lines ADD COLUMN op_ipa_text TEXT;   -- render string, /IPA/ inline
-- display_text : "Or to take arms against a sea of troubles"          (reader-facing)
-- op_ipa_text  : "/ɔːr/ to /tɛːk/ /aːrmz/ /əˈgɛnst/ a /sɛː/ of /ˈtrʊblz/"
```

Pipeline:

1. Renderer reads `op_ipa_text` (fall back to `display_text` if null) and sends
   it to **`eleven_v3`** with the saved Voice A-OP id.
2. Preserve hard line breaks in the string passed to TTS — they carry the verse
   cadence.
3. IPA is ~80–90% consistent, so generate 2–3 takes per line/block and keep the
   best; identical input can vary.
4. Store `op_ipa_text` as UTF-8; the `/` are literal characters. Leave audio
   (`[…]`) tags out for verse — metre and punctuation carry delivery.
5. **Mind v3's input cap.** `eleven_v3` takes ~5,000 characters per request (vs
   ~10,000 for `multilingual_v2` and ~40,000 for `flash_v2_5`). A single line is
   nowhere near that, but a long speech or a multi-line block can brush it, and
   the limit is model-dependent — a v3 concern that did not exist on the v2
   models. Chunk to ≤5k chars on natural line breaks (never mid-line, to preserve
   cadence) and stitch the audio.

**Why a per-line column and not a pronunciation dictionary.** ElevenLabs
pronunciation dictionaries are the obvious "centralize it" temptation, but their
*phoneme* rules are silently skipped on v3 (same as the inline legacy `<phoneme>`
tags), so they cannot carry OP vowels at all. Their *alias* (respelling) rules do
work on every model — but they apply **globally**, which would clobber the same
word in the neutral-modern prose voice and mishandle homographs. Scoping OP to a
per-line `op_ipa_text` column keeps the accent on the verse and leaves the prose
untouched, which is why it is the better design here.

### Hiding `/IPA/` from the reader (linux-lit rendering requirement)

> **Prerequisite — build the IPA pipeline first.** These voices are only useful
> once Shakespeare verse actually carries OP `/IPA/` for `eleven_v3` to read, and
> that markup is *produced* by the gloss pipeline. Design and implement
> **[gloss-driven OP IPA tagging](../superpowers/specs/2026-06-08-gloss-ipa-tagging-design.md)**
> before (or alongside) creating the custom voices — it defines how the
> explication drives sparse per-word tagging, the two-tier (`<verse>` for TTS /
> `<pron>` for the reader) markup, `lit.db` storage, and the strip-for-display
> rendering this section summarizes. The custom-voice work consumes that markup;
> without it there is nothing for the voices to pronounce.

`/IPA/` markup is **TTS-only metadata**: ElevenLabs consumes it at synthesis
time, but the reader must never show `/sɛː/` or `/ˈrɛvɪnjuː/` on screen. There
are two ways to store it, and they differ exactly in how hiding is achieved:

- **Sibling-column storage (preferred for verse).** The `op_ipa_text` column
  above already solves hiding by construction: the IPA lives in a *separate*
  column, the reader-facing `display_text` is never contaminated, so there is
  **nothing to strip** — the display path reads `display_text`, the TTS path
  reads `op_ipa_text`. Use this wherever the source and IPA can be stored as two
  parallel strings (the verse lines).
- **Inline storage (gloss explication, and any single-field text).** Where IPA
  must sit *inside one text field* — e.g. a gloss explication that quotes a verse
  word in OP (see *When the prose quotes the verse*) — the same field feeds both
  display and TTS, so the renderer must **strip `/IPA/` for display while sending
  the raw field to TTS**. This is the case that needs code in linux-lit.

For the inline case the design is:

- **Keep the raw, IPA-bearing text as the TTS value; derive the stripped text for
  display.** Do the divergence in exactly one place so the two never drift.
- **The strip is a sibling of the existing `strip_brackets` helper** (which
  already removes `[…]` spans for line-number matching) — add a `strip_ipa`
  that removes `/…/` spans (a slash-delimited span of IPA characters; mind that a
  lone literal slash, e.g. "and/or", is not an IPA span, so the matcher should
  require IPA-class contents between the slashes).
- **Strip for display, keep raw for TTS:**
  - *Display* — strip when inserting block text into the gloss `TextView`
    buffer, and the main reading card's verse line transform, so the user never
    sees the slashes.
  - *TTS* — the gloss block's text sent to `synthesize()` must remain the raw
    IPA-bearing value; leave that path untouched.
- **Watch the block-range matcher.** The gloss overlay matches block text against
  the *displayed* buffer to position the accent bar. If display is stripped but
  the block text keeps IPA, that match breaks — so the matcher must compare on the
  **stripped** form (or blocks should carry both a raw `text` for TTS and a
  stripped form for display/matching).
- **The audio cache is unaffected.** `gloss_audio` keys on
  `(gloss_id, kind, paragraph_index)` (+ stored `voice_id`/`model_id`); the MP3
  is the synthesized-with-IPA audio. No new key field is needed — but editing a
  gloss's IPA must invalidate its cached rows, the same staleness contract any
  gloss-text edit already has.

In short: **verse → sibling column (hidden by construction); inline IPA → store
raw, strip a `/…/` span for display, send raw to TTS, and keep the accent-bar
matcher on the stripped text.** No `/IPA/` should ever reach the GTK buffer.

## Voice B — prose explication (neutral modern register)

The companion voice reads the guide's **own explanatory prose**, not Shakespeare.
The explication is editorial commentary in the present, so it takes a **neutral
modern register — no OP, no `/IPA/`**. It shares Voice A-OP's identity baseline
(same voice) so the listener hears one narrator switching modes: rhotic and
earthy for the verse, plain and contemporary for the gloss.

Description prompt (identity baseline byte-identical to Voice A-OP, delivery
different):

```
Native English. Male, 15 to 25. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational.
A clear, resonant, higher-pitched young voice — a bright, ringing theatrical tenor, light rather than a deep baritone, with crisp articulation and effortless authority beyond its years — reading explanatory prose in a neutral modern voice: a natural, even pace, sentence rhythm rather than metrical lift, gently instructive and lucid. Confident and unhurried, like an expert explaining a poem to an attentive listener.
```

Preview text — a real explication paragraph from `lit.db`, e.g.:

```
In these opening lines the speaker weighs existence against oblivion, framing the choice as a single question. The metaphors of slings, arrows, and a sea of troubles cast ordinary suffering in the language of warfare, so that endurance and resistance become two forms of courage rather than one of cowardice.
```

No `/IPA/` and no audio tags. Render with `eleven_v3` for a consistent narrator,
though plain prose tolerates `multilingual_v2` if you need it. Stability:
**Natural**.

### When the prose quotes the verse

The explication often *quotes* the source — a word or phrase lifted from the
verse (e.g. "the metaphors of **slings**, **arrows**, and **a sea of troubles**…",
or "the older **revénue**…"). Because OP is render-time `/IPA/`, you decide
per-quotation whether that fragment stays modern or echoes the verse:

- **Default: keep quotations modern.** Voice B is editorial speech in the
  present; a quoted word is being *discussed as a word*, not *performed as verse*.
  Reading "sea" as modern `[siː]` inside a modern sentence is the natural, least
  jarring choice and keeps Voice B's "no `/IPA/`" rule simple. Use this unless you
  have a specific reason not to.
- **Exception: OP-tag a quotation when the archaic sound *is* the point.** If the
  prose is explaining the pronunciation itself — the MEAT–MEET split, a rhotic
  final, an older stress like *revénue* — the listener should *hear* it. There,
  selectively inject the verse's `/IPA/` on just that quoted word
  (`/sɛː/`, `/ˈrɛvɪnjuː/`) so the gloss demonstrates what it describes. This is a
  deliberate, per-word override of the default, not a register change: the
  surrounding sentence stays modern; only the quoted token carries OP.

Mechanically this needs no new voice — it is the same render-time `/IPA/` lever
from Voice A-OP, applied to a single quoted word inside a Voice B render. Store
the choice with the explication text (the quoted token already carries its
`/IPA/` or not), so the prose renders identically every time. Keep it rare:
over-tagging Voice B reintroduces the instability you avoided by keeping it plain.

## Voices C/D — the Petruchio pair (a second, older male narrator)

A separate male family — **not** a replacement for the Will set (A/B). The
conceit: the same young player imagined **about ten years older** (`26 to 35`),
now grown into **Petruchio** from *The Taming of the Shrew* — brash, swaggering,
mercurial, commanding, witty. Letters **C/D** keep it distinct from the Will
(A/B) and Willa (A-OP-F/B-F) families.

The same pipeline applies unchanged: two linked voices sharing one identity
(verse C-OP carries OP `/IPA/` at render time; prose D is neutral modern),
render with **`eleven_v3`**, same stability/audio-tag discipline. The one
intended departure: the identity baseline is byte-identical **within this pair**
(so C-OP and D read as one narrator) but **deliberately different from the Will
baseline** — matured ~10 years, fuller and baritone-leaning rather than the
youth's light tenor. This is a *different narrator*, so a different baseline is
correct; the byte-identical rule applies only between a voice and its own
verse/prose sibling.

### Voice C-OP — Petruchio verse (Original Pronunciation)

Description / generation prompt:

```
Native English. Male, 26 to 35. Studio quality.
Persona: Petruchio, swaggering Veronese gentleman, Shakespearean stage player. Emotion: bold, mercurial, commanding, sardonic, mischievous.
A fuller, warmer, resonant young-man's voice — a baritone-leaning tenor with weight and ring, matured about ten years past youth, carrying the easy authority and swagger of a man who dominates every room. Crisp, vigorous articulation; a glint of wit and provocation under the command. Strongly rhotic — every written R is sounded and colours the vowel before it. Vowels sit slightly archaic and old-fashioned, caught between medieval and modern. Reads at a brisk, muscular, plain-spoken pace, honouring the verse line with a light lift at each line ending, but never posh, never elevated, never refined — earthy, direct, and brazen rather than cathedral-grand. The metre is felt, never hammered; sense drives hard across line breaks.
```

Preview text — audition the swagger/timbre on a real Petruchio passage (plain;
OP is render-time `/IPA/`, not baked into the preview):

```
I come to wive it wealthily in Padua;
If wealthily, then happily in Padua.
Were she as rough as are the swelling Adriatic seas,
I come to wive it wealthily in Padua,
And venture madly on a desperate mart.
```

No audio tags. Stability: **Natural** (toward Creative only if too flat to land
the swagger; toward Robust if it over-emotes).

Save-time Description (≤500 chars):

```
Native English. Male, 26 to 35. Studio quality.
Persona: Petruchio, swaggering Veronese gentleman, Shakespearean stage player. Emotion: bold, mercurial, commanding, sardonic, mischievous.
A fuller, warmer, resonant baritone-leaning tenor with weight and ring, matured ~10 years past youth, carrying easy authority and swagger. Crisp vigorous articulation, a glint of wit. Strongly rhotic — every R sounded. Brisk, muscular, plain-spoken, never posh — earthy, direct, brazen. Metre felt, never hammered.
```

### Voice D — Petruchio prose explication (neutral modern)

Same narrator as C-OP, reading editorial prose: **neutral modern, no OP, no
`/IPA/`**. Identity baseline byte-identical to C-OP; only persona/delivery
change.

Description / generation prompt:

```
Native English. Male, 26 to 35. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational, assured.
A fuller, warmer, resonant young-man's voice — a baritone-leaning tenor with weight and ring, matured about ten years past youth, carrying easy authority — reading explanatory prose in a neutral modern voice: a natural, even pace, sentence rhythm rather than metrical lift, gently instructive and lucid. Confident and unhurried, like an expert explaining a play to an attentive listener.
```

Preview text — a real explication paragraph (no IPA/tags):

```
Petruchio enters Padua frankly admitting that fortune, not love, has drawn him there. His blunt declaration that he comes to wive it wealthily turns courtship into a venture, a desperate mart, so that the comedy's romance is from the first framed as a bargain struck between bold appetite and ready wit.
```

Save-time Description (≤500 chars):

```
Native English. Male, 26 to 35. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational, assured.
A fuller, warmer, resonant baritone-leaning tenor with weight and ring, matured ~10 years past youth, carrying easy authority — reading explanatory prose in a neutral modern voice: natural even pace, sentence rhythm not metrical lift, gently instructive and lucid. Confident and unhurried.
```

### Render and selection

Render with **`eleven_v3`**: verse → C-OP id + per-word OP `/IPA/`; prose → D id,
plain. The Petruchio family is independent of the generic gender-based selection
(Will / Willa); route to C/D only where you specifically want this older,
swaggering narrator (e.g. *The Taming of the Shrew* male leads) rather than the
default Will set. Once saved, add the two IDs to *Saved voice IDs (built)* above.

## Voices E/F — the Beatrice pair (a sharp-witted female narrator)

A second female family — a character voice for **Beatrice** from *Much Ado About
Nothing*: brilliantly verbal, quick, mocking, fearless in her "skirmishes of
wit", disdainful on the surface but warm and feeling underneath. A distinct
character colour from the neutral young Willa set, the way Petruchio (C/D) is a
distinct colour from Will. Letters **E/F** keep it separate from the Will (A/B),
Willa (A-OP-F/B-F), and Petruchio (C/D) families.

Same pipeline: two linked voices sharing one identity (verse E-OP carries OP
`/IPA/` at render time; prose F is neutral modern), render with **`eleven_v3`**,
same stability/audio-tag discipline. As with the other character pairs, the
identity baseline is byte-identical **within this pair** but deliberately its own
— a bright, agile young woman's voice with a glint of mockery, not the neutral
Willa baseline. Age `22 to 28` (centred on ~25, a touch above the youthful Willa
set) to fit her worldly, commanding wit.

### Voice E-OP — Beatrice verse (Original Pronunciation)

Beatrice's wit famously plays in prose as much as verse, but for any verse she
speaks (and to keep the family symmetric) the verse voice carries OP. Description
/ generation prompt:

```
Native English. Female, 22 to 28. Studio quality.
Persona: Beatrice, quick-witted noblewoman, Shakespearean stage player. Emotion: sharp, mocking, spirited, fearless, warm beneath the wit.
A bright, agile, resonant young woman's voice — a clear soprano or light mezzo with quick, dancing articulation and a glint of mockery, fast and fearless yet warm underneath the barbs, carrying easy self-possession and the relish of a brilliant talker who always has the last word. Strongly rhotic — every written R is sounded and colours the vowel before it. Vowels sit slightly archaic and old-fashioned, caught between medieval and modern. Reads at a brisk, nimble, plain-spoken pace, honouring the verse line with a light lift at each line ending, but never posh, never elevated, never refined — earthy, direct, and quick rather than cathedral-grand. The metre is felt, never hammered; wit drives the sense across line breaks.
```

Preview text — audition the quick, mocking timbre on a real Beatrice passage
(plain; OP is render-time `/IPA/`, not baked into the preview):

```
I wonder that you will still be talking, Signior Benedick; nobody marks you.
What, my dear Lady Disdain! Are you yet living?
Is it possible disdain should die while she hath
Such meet food to feed it as Signior Benedick?
Courtesy itself must convert to disdain
If you come in her presence.
```

No audio tags. Stability: **Natural** (toward Creative only if too flat to land
the wit; toward Robust if it over-emotes).

Save-time Description (≤500 chars):

```
Native English. Female, 22 to 28. Studio quality.
Persona: Beatrice, quick-witted noblewoman, Shakespearean stage player. Emotion: sharp, mocking, spirited, fearless, warm beneath the wit.
A bright, agile, resonant soprano or light mezzo with quick dancing articulation and a glint of mockery, fast and fearless yet warm under the barbs, the relish of a brilliant talker. Strongly rhotic — every R sounded. Brisk, nimble, plain-spoken, never posh — earthy, direct, quick. Metre felt, never hammered.
```

### Voice F — Beatrice prose explication (neutral modern)

Same narrator as E-OP, reading editorial prose: **neutral modern, no OP, no
`/IPA/`**. Identity baseline byte-identical to E-OP; only persona/delivery
change (the wit settles into a lucid, engaging guide rather than a sparring
combatant).

Description / generation prompt:

```
Native English. Female, 22 to 28. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational, quick and engaging.
A bright, agile, resonant young woman's voice — a clear soprano or light mezzo with quick articulation and easy self-possession — reading explanatory prose in a neutral modern voice: a natural, even pace, sentence rhythm rather than metrical lift, gently instructive, lucid, and quietly amused. Confident and unhurried, like a sharp, engaging reader explaining a comedy to an attentive listener.
```

Preview text — a real explication paragraph (no IPA/tags):

```
Beatrice meets every overture from Benedick with a counterstroke, turning courtship into a contest of wit where neither will concede the field. Her mockery is a kind of armour, and the comedy's pleasure lies in watching that armour prove, line by line, to be the very thing that draws the two of them together.
```

Save-time Description (≤500 chars):

```
Native English. Female, 22 to 28. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational, quick and engaging.
A bright, agile, resonant soprano or light mezzo with quick articulation and easy self-possession — reading explanatory prose in a neutral modern voice: natural even pace, sentence rhythm not metrical lift, gently instructive, lucid, and quietly amused. Confident and unhurried.
```

### Render and selection

Render with **`eleven_v3`**: verse → E-OP id + per-word OP `/IPA/`; prose → F id,
plain. Like the Petruchio pair, the Beatrice family is independent of the generic
gender-based selection (Will / Willa); route to E/F only where you specifically
want this sharp-witted female narrator (e.g. *Much Ado About Nothing* — Beatrice,
or other quick, sparring female leads) rather than the default Willa set. Once
saved, add the two IDs to *Saved voice IDs (built)* above.

## Workflow

1. ElevenLabs app → **Voices → Add a new voice → Voice Design (Text to Voice)**.
2. Select the **Eleven v3** model. *Confirm v3 is actually the selectable
   generation engine in Voice Design* — the v3 guidance is otherwise oriented to
   cloning. If Voice Design auditions only on a v2-family engine, the pipeline
   still holds: the `/IPA/` fires at **render** time on `eleven_v3` regardless of
   which engine generated the identity — but the audition timbre may then differ
   slightly from the v3 render.
3. Paste the description prompt and the matching preview text.
4. Set stability to **Natural**; adjust **Guidance Scale** if the voice drifts.
5. Generate; audition the multiple previews.
6. Save the keeper — it takes one of your voice slots.
7. Repeat for the second voice, keeping the shared identity line identical.
8. For actual narration, **render with `eleven_v3`** — it is the only model that
   reads `/IPA/` and `[…]` audio tags. `eleven_multilingual_v2` / `flash_v2_5`
   support **neither** and silently drop the OP vowel control, so reserve them
   only for low-latency previews where OP fidelity does not matter. For `lit.db`
   narration (offline, fidelity-critical) there is no reason to leave v3. Note v3
   is still alpha; a real-time v3 is in development.

## Two rules that matter most for fidelity

1. **Keep the identity baseline byte-identical** across both prompts
   (`Native English. Male, 15 to 25. Studio quality.` plus "A clear,
   resonant, higher-pitched ringing theatrical tenor") so they read as one
   narrator in two modes — rhotic OP for the verse, neutral modern for the prose.
2. **Preview on the real text type** — verse sample with line breaks for the verse
   voice, a prose paragraph for the prose voice — to pick the candidate whose
   *timbre* fits. But cadence and pronunciation are **render-time** properties of
   the narration text (line breaks, punctuation, `/IPA/`), not something durably
   absorbed from the preview. The preview only auditions character; the
   narration text carries the verse.

A caveat on scansion: v3 honours your line breaks and punctuation but will not
perfectly scan iambic feet. If a specific line reads wrong-stressed, first reach
for **IPA stress markers** (`ˈ`/`ˌ`) on the offending word in the narration text,
then fall back to fixing that line's punctuation/spacing in `lit.db` — never the
voice prompt.

## Sources

- [Voice Design prompting guide](https://elevenlabs.io/docs/eleven-creative/voices/voice-design#prompting-guide)
- [Prompting Eleven v3 (best practices)](https://elevenlabs.io/docs/best-practices/prompting/eleven-v3)
- [What are Eleven v3 audio tags](https://elevenlabs.io/blog/v3-audiotags)
- [How do audio tags work with Eleven v3 (help center)](https://help.elevenlabs.io/hc/en-us/articles/35869142561297-How-do-audio-tags-work-with-Eleven-v3)
- [Do pauses and SSML phoneme tags work with the API (help center)](https://help.elevenlabs.io/hc/en-us/articles/24352686926609-Do-pauses-and-SSML-phoneme-tags-work-with-the-API)
  (v3 has no `<break>`; use `[pause]` tags; legacy `<phoneme>` is English V1 /
  Flash V2 / Turbo V2 only).

### Original Pronunciation (Voice A-OP)

- `~/Downloads/how-shakespeare-spoke.md` — survey of the OP evidence, feature
  set, and caveats this guide draws on.
- `~/Downloads/eleven-v3-op-review-and-ipa-example.md` — review notes (IPA is
  render-time not preview, lock OP renders to `eleven_v3`) and the worked IPA
  annotation + `lit.db` sketch this guide incorporates.
- [ElevenLabs — Prompting controls](https://elevenlabs.io/docs/best-practices/prompting/controls)
  (IPA in v3, `/slash/` syntax, ~80–90% consistency, `ˈ`/`ˌ` stress markers).
- [ElevenLabs — Pronunciation dictionaries](https://elevenlabs.io/docs/cookbooks/text-to-speech/pronunciation-dictionaries)
  (legacy `<phoneme>` model restrictions, silently-skipped behavior).
- David Crystal, *Pronouncing Shakespeare* (Cambridge, 2005) and *The Oxford
  Dictionary of Original Shakespearean Pronunciation* (Oxford, 2016).
- originalpronunciation.com (David & Ben Crystal); the British Library OP
  recordings.

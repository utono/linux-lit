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
sentence. Keep the identity baseline (warm, resonant baritone) byte-identical
across both prompts; only accent register and delivery change — rhotic OP for the
verse, neutral modern for the prose.

- **Voice A-OP — verse** in Original Pronunciation (rhotic, Shakespeare-era).
  This is the primary verse voice; see its full section immediately below.
- **Voice B — prose explication**: the same baritone, but reading the guide's
  *own* explanatory prose in a neutral modern register (the explication is
  editorial commentary, not stage speech, so it does **not** take OP). See below.

There is no separate RP "classical" verse voice: an RP read of Shakespeare is a
nineteenth-century anachronism (see the OP section), so the verse voice **is**
Voice A-OP.

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
Native English. Male, late 40s to 50s. Studio quality.
Persona: Elizabethan stage player, Shakespearean narrator. Emotion: measured, dignified, earthy, quietly intense.
A warm, resonant baritone, strongly rhotic — every written R is sounded and colours the vowel before it. Vowels sit slightly archaic and old-fashioned, caught between medieval and modern. Reads at a brisk, conversational, plain-spoken pace, honouring the verse line with a light lift at each line ending, but never posh, never elevated, never refined — earthy and direct rather than cathedral-grand. The metre is felt, never hammered; sense flows across line breaks.
```

The identity baseline (`Native English. Male, late 40s to 50s. Studio quality.` +
"A warm, resonant baritone") stays byte-identical to Voice B so the two read as
one narrator — only the *accent* and *register* change: here, rhotic, earthy, and
plain for the verse; in Voice B, neutral modern for the explanatory prose.

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

## Voice B — prose explication (neutral modern register)

The companion voice reads the guide's **own explanatory prose**, not Shakespeare.
The explication is editorial commentary in the present, so it takes a **neutral
modern register — no OP, no `/IPA/`**. It shares Voice A-OP's identity baseline
(same baritone) so the listener hears one narrator switching modes: rhotic and
earthy for the verse, plain and contemporary for the gloss.

Description prompt (identity baseline byte-identical to Voice A-OP, delivery
different):

```
Native English. Male, late 40s to 50s. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational.
A warm, resonant baritone, reading explanatory prose in a neutral modern voice: a natural, even pace, sentence rhythm rather than metrical lift, gently instructive and lucid. Confident and unhurried, like an expert explaining a poem to an attentive listener.
```

Preview text — a real explication paragraph from `lit.db`, e.g.:

```
In these opening lines the speaker weighs existence against oblivion, framing the choice as a single question. The metaphors of slings, arrows, and a sea of troubles cast ordinary suffering in the language of warfare, so that endurance and resistance become two forms of courage rather than one of cowardice.
```

No `/IPA/` and no audio tags. Render with `eleven_v3` for a consistent narrator,
though plain prose tolerates `multilingual_v2` if you need it. Stability:
**Natural**.

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
   (`Native English. Male, late 40s to 50s. Studio quality.` plus "A warm,
   resonant baritone") so they read as one narrator in two modes — rhotic OP for
   the verse, neutral modern for the prose.
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

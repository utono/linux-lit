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
- **No SSML break tags.** v3 does **not** support `<break>`. Control pauses with
  punctuation and line structure instead. Too many forced breaks cause
  instability (speed-ups, artifacts, stray noises).
- **IPA pronunciation.** v3 supports International Phonetic Alphabet
  transcription across 70+ languages for precise pronunciation without XML.

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
  Received Pronunciation lean")
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
sentence. Keep the identity line byte-identical across both prompts.

### Voice A — verse (iambic pentameter / blank verse)

Description prompt:

```
Native English. Male, late 40s to 50s. Studio quality.
Persona: classical stage actor, Shakespearean narrator. Emotion: measured, dignified, quietly intense.
A warm, resonant baritone with clear articulation and a slight Received Pronunciation lean. Reads at a calm, deliberate pace, honouring the line — a light lift at each line ending and a brief breath at the caesura — without sing-song or forced rhyme. The metre is felt, never hammered; sense flows across line breaks. Restrained, contemplative, never theatrical.
```

Preview text — a real verse passage with line breaks preserved, e.g.:

```
To be, or not to be, that is the question:
Whether 'tis nobler in the mind to suffer
The slings and arrows of outrageous fortune,
Or to take arms against a sea of troubles
And by opposing end them.
```

Use **no audio tags** for verse. Stability: **Natural**.

### Voice B — prose explication

Description prompt (identical identity, different delivery):

```
Native English. Male, late 40s to 50s. Studio quality.
Persona: erudite literary guide, audiobook narrator. Emotion: clear, warm, conversational.
The same warm baritone with a slight Received Pronunciation lean, but reading explanatory prose: a natural, even pace, sentence rhythm rather than metrical lift, gently instructive and lucid. Confident and unhurried, like an expert explaining a poem to an attentive listener.
```

Preview text — a real explication paragraph from `lit.db`, e.g.:

```
In these opening lines the speaker weighs existence against oblivion, framing the choice as a single question. The metaphors of slings, arrows, and a sea of troubles cast ordinary suffering in the language of warfare, so that endurance and resistance become two forms of courage rather than one of cowardice.
```

Stability: **Natural**.

## Workflow

1. ElevenLabs app → **Voices → Add a new voice → Voice Design (Text to Voice)**.
2. Select the **Eleven v3** model.
3. Paste the description prompt and the matching preview text.
4. Set stability to **Natural**; adjust **Guidance Scale** if the voice drifts.
5. Generate; audition the multiple previews.
6. Save the keeper — it takes one of your voice slots.
7. Repeat for the second voice, keeping the shared identity line identical.
8. For actual narration, render with **`eleven_multilingual_v2`** or the current
   v3 TTS model; confirm the saved voice is v3-compatible so tags apply.

## Two rules that matter most for fidelity

1. **Keep the identity line byte-identical** across both prompts
   (`Native English. Male, late 40s to 50s. Studio quality.` plus "warm baritone,
   slight RP lean") so they read as one narrator in two modes.
2. **Preview on the real text type** — verse sample with line breaks for the verse
   voice, a prose paragraph for the prose voice. The preview text is the
   cadence-trainer and the single biggest factor in whether blank verse sounds
   like verse.

A caveat on scansion: v3 honours your line breaks and punctuation but will not
perfectly scan iambic feet. If a specific line reads wrong-stressed, fix the
punctuation/spacing of that line's text in `lit.db` — not the voice prompt.

## Sources

- [Voice Design prompting guide](https://elevenlabs.io/docs/eleven-creative/voices/voice-design#prompting-guide)
- [Prompting Eleven v3 (best practices)](https://elevenlabs.io/docs/best-practices/prompting/eleven-v3)
- [What are Eleven v3 audio tags](https://elevenlabs.io/blog/v3-audiotags)
- [How do audio tags work with Eleven v3 (help center)](https://help.elevenlabs.io/hc/en-us/articles/35869142561297-How-do-audio-tags-work-with-Eleven-v3)

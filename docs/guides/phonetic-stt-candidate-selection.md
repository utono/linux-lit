# Phonetic STT for automatic OP take-selection

How linux-lit could automatically pick the best of N TTS renders of an Original
Pronunciation (OP) verse line, by transcribing each take's audio back to
**phonemes** (IPA) and scoring it against the target `/IPA/`.

This is the **phase-2 auto-score** path noted in
[gloss-driven OP IPA tagging](../superpowers/specs/2026-06-08-gloss-ipa-tagging-design.md)
§5 and the [custom-voices guide](./elevenlabs-v3-custom-voices.md). The v1 path is
manual: a human auditions the N takes and keeps the best. This guide is about
replacing the human ear with a phonetic recognizer so the selection can run in
batch over a whole work.

## Why a *phonetic* STT (and why ElevenLabs Scribe can't do this)

`eleven_v3` honours an inline IPA tag like `/sɛː/` (OP *sea*) only ~80–90% of the
time per word, non-deterministically. So we render the same line **N times** and
must keep the take where the OP pronunciations actually landed. To select
automatically we transcribe each take and compare — but **what** the transcriber
hears decides whether this works:

- **Ordinary (orthographic) STT** — including **ElevenLabs Scribe** — writes
  words in normal spelling. It transcribes both the OP `[sɛː]` and the modern
  `[siː]` as the word **"sea"**, so it is **blind to the OP-vowel distinction
  that is the whole point**. ElevenLabs offers **no phonetic/IPA STT**; Scribe
  outputs text + timestamps + diarization only. An all-ElevenLabs round-trip
  catches only *gross* misses (a dropped, garbled, or wrong word).
- **Phonetic STT** — outputs **phoneme / IPA strings** (`s ɛː`, not "sea"). Only
  this can tell `[sɛː]` from `[siː]` and therefore *score the OP vowel*. It is
  necessarily a **third-party** model; none of it comes from ElevenLabs.

So: gross-miss filtering = any STT; OP-vowel scoring = a phonetic STT.

## The candidates

All of these output phones/IPA you can diff against the target. Listed
easiest-first for an English-only linux-lit pipeline.

### 1. wav2vec2-phoneme (Hugging Face Transformers) — recommended start

A wav2vec 2.0 model fine-tuned with CTC to emit IPA phonemes
(`Wav2Vec2Phoneme` / `Wav2Vec2PhonemeCTCTokenizer`). Mature, CPU-capable,
one `pip install transformers torchaudio`. Concrete checkpoints:

- **`facebook/wav2vec2-lv-60-espeak-cv-ft`** — the canonical eSpeak-IPA model
  (wav2vec2-large-lv60 fine-tuned on CommonVoice to IPA, multi-language). Good
  general baseline. Input must be **16 kHz mono**.
- **`slplab/wav2vec2-large-robust-L2-english-phoneme-recognition`** —
  English-specific phoneme recognition; a strong default if you only ever score
  English.
- **`MultiBridge/wav2vec-LnNor-IPA-ft`** — wav2vec2-base fine-tuned on
  TIMIT + LnNor, predictions in IPA.

Sketch:

```python
import torch, torchaudio
from transformers import AutoProcessor, AutoModelForCTC

proc  = AutoProcessor.from_pretrained("facebook/wav2vec2-lv-60-espeak-cv-ft")
model = AutoModelForCTC.from_pretrained("facebook/wav2vec2-lv-60-espeak-cv-ft")

wav, sr = torchaudio.load("take1.wav")            # ElevenLabs mp3 -> decode to wav
wav = torchaudio.functional.resample(wav, sr, 16000)
logits = model(proc(wav.squeeze(), sampling_rate=16000,
                    return_tensors="pt").input_values).logits
ids = logits.argmax(-1)
ipa = proc.batch_decode(ids)[0]                   # e.g. "tə biː ɔːr nɒt tə biː …"
```

**Trade-off:** off-the-shelf, English-focused, runs on CPU; but its IPA
inventory is whatever the checkpoint was trained on (eSpeak phone set), which may
not draw every OP distinction (e.g. length marks, the `[ɛː]` of the MEAT set)
crisply — validate it on a few known OP renders before trusting its scores.

### 2. Allosaurus — universal phone recognizer

`pip install allosaurus`; a pretrained **language-agnostic** phone recognizer
(2000+ languages) using phone-level CTC.

```python
from allosaurus.app import read_recognizer
rec = read_recognizer()
phones = rec.recognize("take1.wav")               # "t ə b iː ɔː r n ɒ t …"
# rec.recognize("take1.wav", timestamp=True) -> per-phone timestamps
```

**Why it's interesting here:** it emits **raw phones independent of any language
model** (so it won't "correct" an odd OP vowel toward a modern English word), and
it can return **per-phone timestamps** — which lets you score only the *tagged*
words rather than the whole line (see *Scoring*). **Trade-off:** universal phone
set, not English-optimized; phone labels may need mapping to your IPA scheme.

### 3. Newer broad-IPA recognizers (heavier, optional)

**Allophant, ZIPA, MultIPA, POWSM** — 2025-era XLS-R / ZipFormer models trained
for wide multilingual IPA coverage. Use only if you ever need non-English phone
coverage or find (1)/(2) too coarse for the OP contrasts. They are larger and
more setup; overkill for English-only OP scoring at the start.

## How linux-lit would use it (the auto-score loop)

The target IPA already exists: it is the `/IPA/` the gloss tagged on each
`<verse>` line (the **TTS text**, slashes stripped → a target phoneme string per
tagged word). The loop per verse block:

1. **Render N takes** of the IPA-bearing verse text on `eleven_v3` (default
   N = 2–3). Decode each mp3 to 16 kHz mono wav.
2. **Recognize phonemes** for each take with the chosen model → a hypothesis IPA
   string (optionally with per-phone timestamps).
3. **Score each take** against the target (see below).
4. **Keep the best-scoring take**; write its mp3 to the `gloss_audio` cache (the
   same row a manual pick would fill). Discard the rest.
5. **Confidence gate.** If even the best take scores below a threshold, **fall
   back to flagging the line for manual audition** rather than caching a bad
   take — phonetic STT is itself imperfect, so don't let it silently approve
   garbage.

### Scoring

Compare the recognized phonemes to the target on the **tagged words only** (the
words that carry `/IPA/`), not the whole line — the untagged words are modern and
uninteresting:

- **Whole-line baseline:** phoneme edit distance (Levenshtein over phone
  sequences) between hypothesis and target; lower = better. Simple, no alignment,
  but dilutes the signal across untagged words.
- **Per-word (preferred):** use the recognizer's **timestamps** (Allosaurus, or a
  forced aligner) to extract the phones under each tagged word's time span, and
  score *those* against that word's target `/IPA/`. This directly answers "did
  `take` come out `/tɛːk/` or `/teɪk/`?" — the question that matters.
- **Normalize before comparing:** map both sides into one phone inventory
  (collapse length marks / diacritics you don't care about, unify the
  recognizer's labels with your IPA scheme) so trivial notation differences don't
  count as misses.

### Where it sits in the pipeline

This is a **batch/offline** tool, not part of the interactive reader: run it when
pre-rendering a work's verse audio, so the `gloss_audio` cache is populated with
vetted takes before a user ever plays them. It needs Python + a model download;
keep it as a `scripts/` utility invoked out-of-band, not linked into the GTK app.

## Honest limits

- **The recognizer is not ground truth.** It has its own error rate; a take it
  scores "best" can still be wrong, and a good take can be under-scored if the
  model mis-hears the OP vowel. Hence the confidence gate + manual fallback.
- **OP contrasts are subtle.** Length (`[ɛː]` vs `[ɛ]`) and the exact MEAT/MEET
  split are exactly the distinctions a general English phone model is weakest on.
  Validate the chosen model on a handful of hand-labelled OP renders and measure
  whether its scores actually track audible OP correctness before relying on it.
- **It does not improve the audio** — it only *selects* among takes v3 produced.
  If all N takes miss a word, scoring picks the least-bad; raising quality still
  means fewer tags per line (sparsity) and, if needed, more takes.

## Recommended path

Start with **wav2vec2-phoneme** (`facebook/wav2vec2-lv-60-espeak-cv-ft` or the
`slplab` English model), whole-line edit-distance scoring, a conservative
confidence gate, and manual fallback. Move to **Allosaurus + per-word timestamp
scoring** only if whole-line scoring proves too coarse to catch the OP-vowel
misses you care about. Keep manual best-of-N (the v1 path) as the always-available
fallback — the auto-scorer's job is to *reduce* manual auditioning over a large
corpus, not to be trusted unsupervised.

# OP-IPA cheat-sheet — the ~25 symbols you'll actually type

A quick reference for typing Original Pronunciation (OP) `/IPA/` tags when
fixing a word's pronunciation in a gloss (the `i` key in the gloss overlay →
type `word /IPA/`). It covers the small set of symbols that recur in
Shakespearean OP, each with an example word and the modern-vs-OP contrast.

This is for the **typing** path. You can also type a plain hint (`daily hard a`)
and let the model regenerate the IPA — but typing is precise, instant, and free,
and you'll learn the symbols fast because the system is small and regular.

To read the IPA already on a gloss's verses, use the `show-gloss-ipa-tts` skill
(`show-gloss-ipa-tts <gloss-id>`), which prints the stored/display/TTS forms.

## The worked example: "daily"

The case that motivated this sheet. A gloss had `daily /ˈdɛːli/` — the `ɛː` is the
DRESS vowel lengthened, which ElevenLabs voices as "**deh**-lee" / "deli". A
hard-a "daily" is the FACE vowel:

- OP FACE is the **long monophthong** `/ˈdeːli/` (Crystal's reconstruction), or
- the modern-leaning **diphthong** `/ˈdeɪli/` if you prefer the glide.

Either fixes the "deli" problem. Type `daily /ˈdeːli/` or `daily /ˈdeɪli/`.

## Vowels & diphthongs (lexical sets)

The single biggest source of "wrong" OP is the LLM (or your ear) defaulting to
**modern** vowels. These are the OP values to pin — `OP` is what to type, `mod`
is the modern value to avoid:

- **FACE** — *daily, gave, day* → OP `eː` (mod `eɪ`)
- **GOAT** — *go, know, so* → OP `oː` (mod `əʊ`)
- **PRICE** — *wise, time, I* → OP `əɪ` (mod `aɪ`)
- **CHOICE** — *boy, point* → OP `əɪ` (merges with PRICE)
- **MOUTH** — *house, now* → OP `əʊ` (mod `aʊ`)
- **happY** — *city, money* → OP `əɪ` (mod `i`)
- **STRUT** — *love, blood, cut* → OP `ɤ` (near FOOT `ʊ`)
- **TRAP** — *bath, path, man* → OP `a` (no broad-a split)
- **LOT** — *lot, ought, call* → OP `ɑ` (unrounded)
- **DRESS** — *dread, bed* → OP `ɛ` / `ɛː` (length varies)
- **FLEECE** — *meet, see* → OP `eː` / `iː`
- **GOOSE** — *food, true* → OP `uː`
- **KIT** — *sit, this* → OP `ɪ` (as modern)

Notes:
- **FACE/GOAT are monophthongs in OP** — the commonest fix. Long marks (`ː`) not
  glides.
- **PRICE = CHOICE = happY = `əɪ`** (centred onset) — this is why *lines/loins*
  punned and why *city* rhymes with *high* in OP.
- **MEAT–MEET still split:** a few words (*great, break, steak*) keep the older
  `[ɛː]` value while the rest of their set raised — that's why *great* is
  `/ɡrɛːt/` not `/ɡriːt/`.
- **No TRAP–BATH split:** *bath, path, grass* take the short front `a`.
- **FOOT–STRUT incomplete:** *blood/good*, *cut/put* sit close together.

## Consonants

Mostly familiar; these are the ones with non-obvious symbols:

- `ʃ` — sh (*shall*), `ʒ` — vision, `tʃ` — church, `dʒ` — judge
- `θ` — thin, `ð` — this
- `ŋ` — sing; **but `-ing` → `[ɪn]`** in OP (*lovin'*, *singin'*)
- `ʍ` — wh- aspirated (*what, which*) — distinct from `w`
- **Rhotic `r` (the headline OP feature):** every written *r* is sounded and
  colours the vowel — *art, bird, for, here*. Use `r`/`ɹ`; r-coloured schwa `ɚ`
  (*letter* `/ˈlɛtɚ/`), `ɝ` for stressed NURSE. **Always sound the r.**

## Marks

- `ˈ` — primary stress (before the stressed syllable): `/ˈdeːli/`
- `ˌ` — secondary stress (longer words)
- `ː` — length: `eː`, `ɛː`, `uː`
- `ə` — schwa (unstressed): the default reduced vowel

## How to type the symbols

These are Unicode IPA characters. Paste them from this sheet, or use your
compose key / an IPA input method. The recurring few — `ɛ ɔ ə ɪ ʊ ɤ ɑ a` (vowels),
`ː ˈ ˌ` (marks), `ʃ ʒ tʃ dʒ θ ð ŋ ʍ ɚ` (consonants) — cover almost everything.

## Sources & authority

There is **no machine-readable OP-IPA dictionary** to look up; the conventions
below are the standard, and an LLM told to "use Crystal's OP conventions"
produces consistent results because OP is a finite lexical-set system.

- **Paul Meier, *The Original Pronunciation (OP) of Shakespeare's English*** —
  free PDF, the best open spec of Crystal's system with worked line
  transcriptions: <http://www.paulmeier.com/OP.pdf>. (Source of the lexical-set
  IPA values above.)
- **David Crystal, *The Oxford Dictionary of Original Shakespearean
  Pronunciation*** (Oxford, 2016) — the authoritative ~20,000-word per-word
  reference (print / Oxford Reference subscription; not downloadable). The
  ground-truth when a word's OP is genuinely uncertain.
- **originalpronunciation.com** (David & Ben Crystal) — overview + audio demos.
- Background on the reconstruction and its evidence: see
  [how-shakespeare-spoke.md](./how-shakespeare-spoke.md) in this directory.

## See also

- [elevenlabs-v3-custom-voices.md](./elevenlabs-v3-custom-voices.md) — how
  `/IPA/` is fed to `eleven_v3` and why slashes (not SSML `<phoneme>`) are used.
- [ipa-sent-to-elevenlabs.md](./ipa-sent-to-elevenlabs.md) — the exact markup
  the app sends.

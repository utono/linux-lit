# Eleven v3 OP Voice — Review Notes & a Worked IPA Example

*Addendum to "Creating Custom Eleven v3 Voices." Two parts: (1) corrections and
refinements to the Voice A-OP strategy after checking the current ElevenLabs
docs; (2) a worked, render-ready IPA annotation of a real verse passage, with a
`lit.db` integration sketch.*

---

## Part 1 — Review notes

The literary half of the guide is sound: the OP feature set is accurate, the
instinct to *describe* OP's features rather than name an unlabelable accent is
correct, and the two-voices-one-identity architecture (identity line
byte-identical, accent/register varying) is the right design.

The load-bearing technical claim — that Eleven v3 accepts IPA — is the one worth
pressure-testing, because the entire Voice A-OP approach stands or falls on it.
**It checks out, and it is specifically a v3 capability.** Per the ElevenLabs
best-practices/controls documentation, `eleven_v3` has native IPA support across
70+ languages and reads IPA symbols wrapped in forward slashes directly in the
text, with no XML. The legacy `<phoneme>` SSML the guide correctly avoids is the
restricted mechanism: those phoneme tags work only with `eleven_flash_v2` /
`eleven_monolingual_v1` (and per some pages `eleven_turbo_v2` / `Eleven English
v1`) and are *silently skipped* by other models. So the guide is using the right
lever; the points below are refinements and one real correction.

### 1. Syntax: forward slashes, not "tags"

IPA goes in `/…/` directly in the text. The guide's phrasing ("IPA tag," "inline
IPA tag") risks conflating it with the `[…]` audio tags. State the rule flatly:

> In v3, `[brackets]` = **delivery** (whispers, sighs); `/slashes/` =
> **pronunciation** (IPA).

So the FACE-monophthong example for *take* is `/tɛːk/`, not a bracketed tag.

### 2. The IPA lever lives at *render* time, not (durably) in the preview

This is the substantive reframe. Voice Design (Text to Voice) saves a **voice
identity** (timbre/character). Whatever you put in the preview steers which
candidate you pick; it is **not** absorbed into the saved voice as permanent
pronunciation behavior. The per-word OP vowels must be re-supplied as `/IPA/` in
the **actual narration text every render**.

Practical consequence:

- The OP vowel/stress annotations belong **in the `lit.db` narration text** (or a
  generated IPA-annotated rendering layer), applied line by line at TTS time.
- The Voice Design preview's job is **timbre/character audition** — rhotic,
  earthy, plain — so the candidate you save leans the right way.
- The IPA's job is **generation-time correction** of specific words.

The guide currently frames the preview as "where v3 learns the vowels," which
overstates it. Same caveat applies to the "preview trains metrical cadence"
claim: durable cadence is carried by the **punctuation and line structure of the
narration text at render time**, not by a property absorbed from the preview.
Keep the hard line breaks in the rendered verse; that is what makes v3 lift and
breathe at line ends.

### 3. Workflow step 8 is wrong for the OP voice — and self-contradictory

The guide says to render with `eleven_multilingual_v2` *or* v3. But
`multilingual_v2` supports **neither** the `/IPA/` slashes (v3-native) **nor** the
`[…]` audio tags (v3-only); rendering OP through it silently drops the entire
vowel-control mechanism. **The OP and expressive voices must be rendered with
`eleven_v3`** to keep IPA.

The real tradeoff to name: v3 is still alpha, and ElevenLabs recommends
v2.5 Turbo / Flash for real-time use while a real-time v3 is in development. So
`multilingual_v2` / `flash` is only a fallback when you need low latency or batch
stability — and you forfeit OP fidelity if you take it. For `lit.db` narration
(offline, fidelity-critical), there is no reason to leave v3.

Corrected step 8: *render with `eleven_v3`; reserve `flash_v2_5` /
`multilingual_v2` only for low-latency previews where OP fidelity does not
matter.*

### 4. Two free wins for the scansion problem

The controls doc recommends including **stress markers** — `ˈ` for primary, `ˌ`
for secondary — on multi-syllable words, and verifying IPA against a dictionary.
That stress marking is a direct lever on the mis-stressed-line problem flagged at
the end of the guide: you can force metrical stress phonetically rather than only
nudging punctuation. Example — to land *revenue* on the metrically-stressed slot
Shakespeare often wants:

```
/ˈrɛvɪnjuː/    (stress 1st syllable — older/metrical)
/rɪˈvɛnjuː/    (stress 2nd syllable — modern default)
```

And set expectations with the real number: v3's IPA lands around **80–90%
consistency**, and identical IPA can occasionally yield different outputs, so
generate several and pick the best. That figure *strengthens* the guide's honest
caveat rather than undercutting it.

> **Net:** the OP approach is viable and the model/mechanism are right. The main
> edit is moving the IPA work from "preview-time training" to "render-time
> annotation on the `lit.db` text," and locking OP/expressive renders to
> `eleven_v3` so the IPA actually fires. (The `<break>`-unsupported claim was not
> separately re-verified, but the punctuation-as-timing guidance is correct
> regardless.)

---

## Part 2 — A worked IPA annotation

Target passage (*Hamlet* 3.1), line breaks preserved:

```
To be, or not to be, that is the question:
Whether 'tis nobler in the mind to suffer
The slings and arrows of outrageous fortune,
Or to take arms against a sea of troubles
And by opposing end them.
```

### Render-ready OP version (feed this string to `eleven_v3`)

```
To be, /ɔːr/ not to be, that is the question:
/ˈʍɛðər/ 'tis /ˈnoːblər/ in the /məɪnd/ to /ˈsʊfər/
The slings and /ˈaroːz/ of /əʊˈtrɛːdʒəs/ /ˈfɔːrtjuːn/,
/ɔːr/ to /tɛːk/ /aːrmz/ /əˈgɛnst/ a /sɛː/ of /ˈtrʊblz/
And /bəɪ/ /əˈpoːzɪn/ end them.
```

Fourteen tagged words out of ~forty — the ones that carry the accent. Everything
else is left as ordinary text, per the "tag the few, never all" rule
(over-tagging destabilizes v3). Tag **per word**, not per phrase: it is the more
reliable unit and matches the documented behavior.

### Why each word is tagged

| Word (in text) | OP feature | IPA |
|---|---|---|
| or | NORTH vowel, rhotic — *r* sounded | `/ɔːr/` |
| whether | `wh` = [ʍ] (distinct from `w`); rhotic final | `/ˈʍɛðər/` |
| nobler | GOAT monophthong [oː] (not [əʊ]); rhotic final | `/ˈnoːblər/` |
| mind | PRICE, centred onset [əɪ] (not [aɪ]) | `/məɪnd/` |
| suffer | STRUT ≈ [ʊ] (incomplete FOOT–STRUT split); rhotic final | `/ˈsʊfər/` |
| arrows | TRAP [a] + tapped *r* + GOAT [oː] | `/ˈaroːz/` |
| outrageous | MOUTH onset [əʊ] + FACE monophthong [ɛː] | `/əʊˈtrɛːdʒəs/` |
| fortune | NORTH rhotic + retained yod + GOOSE [uː] | `/ˈfɔːrtjuːn/` |
| take | FACE monophthong [ɛː] (not gliding [eɪ]) | `/tɛːk/` |
| arms | START, r-coloured [aːr] | `/aːrmz/` |
| against | DRESS vowel [ɛ] (not [eɪ]) | `/əˈgɛnst/` |
| sea | **MEAT set [ɛː]**, distinct from FLEECE — *not* [siː] | `/sɛː/` |
| troubles | STRUT [ʊ] | `/ˈtrʊblz/` |
| by | PRICE, centred onset [əɪ] | `/bəɪ/` |
| opposing | GOAT [oː] + final `-ing` → [ɪn] | `/əˈpoːzɪn/` |

The two most *characteristic* — the ones a listener will immediately register as
"not modern" — are `/sɛː/` (the live MEAT–MEET distinction; *sea* rhyming on
[ɛː], not [iː]) and the FACE/GOAT monophthongs in `/tɛːk/` and `/ˈnoːblər/`. The
rhotic finals do the steady background work.

### Two metrical judgment calls (left untagged on purpose)

- **question** stays disyllabic. The line is a feminine ending —
  *To be, or not to be, that is the QUES-tion* = 11 syllables — so do **not**
  expand `-ion` to [sɪ.ən] here; that would push it to 12 and break the line. OP
  treats `-ion` as variable and lets the metre govern. Same logic keeps
  **fortune** as a two-syllable feminine ending in line 3.
- This is the principle the guide should state generally: *encode the segmental
  OP features in `/IPA/`, but let line structure and syllable count — not IPA —
  govern `-ed` and `-ion` syllabicity, word by word.*

### Notes on the IPA itself

- Rhoticity is written as vowel + `r` (`/ˈsʊfər/`, `/aːrmz/`, `/ɔːr/`); v3 should
  sound the *r*. OP's *r* was likely a tap or approximant — left unmarked here,
  since the consistency gain from over-specifying is not worth the instability.
- `/sɛː/` could alternatively be rendered `/seː/` if the MEAT set sounds too open
  in audition; both are defensible for c. 1600. Audition and pick.
- These are **OP-flavoured** values for educated London speech ~1600, not a
  verified reconstruction. Where a word reliably mis-reads, edit that word's IPA
  in the narration string — never the voice prompt.

### `lit.db` integration sketch

Store the annotated string as a sibling column to the display text, so the
renderer never has to derive IPA on the fly and the display text stays clean for
the reader:

```sql
ALTER TABLE lines ADD COLUMN op_ipa_text TEXT;   -- render string, /IPA/ inline
-- display_text  : "Or to take arms against a sea of troubles"      (reader-facing)
-- op_ipa_text   : "/ɔːr/ to /tɛːk/ /aːrmz/ /əˈgɛnst/ a /sɛː/ of /ˈtrʊblz/"
```

Pipeline:

1. Renderer reads `op_ipa_text` (fall back to `display_text` if null) and sends it
   to `eleven_v3` with the saved Voice A-OP id.
2. Preserve hard line breaks in the string passed to TTS — they carry the verse
   cadence.
3. Because IPA is ~80–90% consistent, generate 2–3 takes per line/block and keep
   the best; identical input can vary.
4. Keep `op_ipa_text` as UTF-8; the `/` are literal characters in the stored
   string. Audio (`[…]`) tags are generally unnecessary for verse — leave them
   out and let metre and punctuation carry delivery.

This keeps the IPA work where it actually fires (render time, on real text),
versions cleanly in the database, and leaves the Voice Design preview to do only
what it can durably do: fix the narrator's timbre and general character.

---

## Sources

- ElevenLabs — Prompting best practices / controls (IPA in v3, slash syntax,
  80–90% consistency, stress markers): <https://elevenlabs.io/docs/best-practices/prompting/controls>
- ElevenLabs — Pronunciation dictionaries (legacy `<phoneme>` model restrictions;
  silently-skipped behavior): <https://elevenlabs.io/docs/cookbooks/text-to-speech/pronunciation-dictionaries>
- ElevenLabs — Voice Design prompting guide & Eleven v3 best practices (as cited
  in the parent guide).
- OP feature set and values: see companion `how-shakespeare-spoke.md`; David
  Crystal, *The Oxford Dictionary of Original Shakespearean Pronunciation* (2016).

### Verified 2026-06 (phoneme-tag vs. v3 inline-IPA review)

URLs checked when confirming that the legacy `<phoneme>`/Arpabet guidance applies
only to v2 models (`eleven_flash_v2` / `eleven_monolingual_v1`), while
`eleven_v3` — the model linux-lit renders the custom OP voices with — reads
inline `/IPA/` directly:

- <https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices>
- <https://help.elevenlabs.io/hc/en-us/articles/16712320194577-How-can-I-force-a-certain-pronunciation-of-a-word-or-name>
- <https://help.elevenlabs.io/hc/en-us/articles/24352686926609-Do-pauses-and-SSML-phoneme-tags-work-with-the-API>
- <https://elevenlabs.io/blog/eleven-v3-audio-tags-emulating-accents-with-precision>

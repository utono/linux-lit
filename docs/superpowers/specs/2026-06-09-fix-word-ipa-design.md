# Fix one word's OP-IPA in a gloss verse — design

**Status:** design (not implemented). A small, self-contained feature: a
gloss-overlay keybind that corrects the Original-Pronunciation `/IPA/` of a
single word in the cursor's source verse, drops that block's stale synthesized
audio, and re-synthesizes + plays so the user hears the fix immediately.

## Problem

The gloss verse text stores OP IPA inline, appended after the operative word —
e.g. gloss 21728 line 2 holds `In daily /ˈdɛːli/ thanks, that gave /gɛːv/ …`.
The `/ˈdɛːli/` (DRESS-length `ɛː`) makes ElevenLabs voice "daily" like "deli";
the user wants a FACE vowel (`/ˈdeɪli/` or `/ˈdeːli/`, "hard a"). Today the only
way to change an IPA is `e` (edit gloss), which re-glosses the **entire** passage
via Claude — far heavier than fixing one word, and it regenerates all the prose
too. There is no surgical "change this one word's IPA and re-voice just that
line" path.

The fix must touch three things: (1) rewrite the IPA inline in the stored
`gloss_text`, (2) delete the stale cached MP3 for that source block, (3)
re-synthesize and play. All three are mechanical once the new IPA is known; the
only design choice is **how the user supplies the corrected IPA**.

## Decisions

- **Dual input — type the IPA OR ask the LLM.** The input card accepts either:
  - `daily /ˈdeɪli/` — a word followed by a literal `/IPA/` span → used
    **verbatim**, no LLM. Precise, instant, free; the path the user graduates
    into as they learn the symbols.
  - `daily hard a` — a word followed by a plain-English hint (no slashes) →
    Claude regenerates just that word's OP IPA.
  - **Detection:** the text *after the first word* is treated as a literal IPA if
    it contains a `/…/` span (the same `is_ipa` heuristic `strip_ipa`/`ipa_for_tts`
    use: a `/…/` whose inner has ≥1 non-ASCII-letter char); otherwise it is an
    LLM hint.
- **Targets the accent-bar cursor's source block.** Like `r`/`R`, the fix acts on
  the source block the cursor is on (move `j`/`k` to the verse line first). Only
  that block's audio is deleted/re-synthesized. No-op (toast) off a source block.
- **Replaces ALL occurrences of the word in that block.** If the named word
  appears more than once in the block (e.g. a repeated word), every `word /IPA/`
  pair for it is updated to the new IPA. (A future refinement could disambiguate
  by occurrence, but all-occurrences is the default.)
- **In-place rewrite via `update_gloss`, not `save_gloss`.** The edit must keep
  the gloss's `id` (the audio cache keys on `gloss_id`), so it uses
  `update_gloss(conn, gloss_id, new_gloss_text)` (an `UPDATE … WHERE id`), NOT
  `save_gloss` (which `INSERT`s a new row).
- **Per-block audio delete, all voices.** Delete only the cached MP3 rows for
  this `(gloss_id, kind='source', paragraph_index)` — across every `voice_id`, so
  any voice re-synthesizes fresh — leaving other blocks' cache intact. This needs
  a new `delete_gloss_audio_block` query (the existing `delete_gloss_audio` nukes
  a whole gloss). Also remove the on-disk `.mp3` files for the deleted rows.
- **Auto-play after re-synthesis.** Reuse `play_source_tts_pausing_mpv` (pause
  MPV, then `play_block_tts`) so the corrected line plays immediately in the
  block's active/default voice. Cache miss → synthesize → play.
- **Trigger key `i`** (mnemonic: IPA), currently free in the gloss overlay.
- **A new `GlossPromptMode::FixIpa`** routes the existing stacked input card's
  submit to the fix handler (alongside `Add`/`Edit`).

## 1. The keybind & input card

- `i` in `handle_gloss_key` → a new `open_fix_ipa_prompt(state)` that:
  - gates on the cursor being a **Source** block (`source_block_index`, reusing
    the `r`/`R` gate; toast "Source verse only" otherwise),
  - opens the stacked input card via `show_prompt_dialog(state, GlossPromptMode::FixIpa)`,
    prompting e.g. `Fix IPA — word [/IPA/ | hint]`.
- `submit_gloss_prompt` (already routes by `gloss_prompt_mode`) gains a `FixIpa`
  arm that reads the card text and calls `fix_word_ipa(state, &text)`.
- Cancel / empty input → no-op (existing card behavior).

The card is the same shared stacked input used by Add/Edit (see the
synopsis-ask-card layout note); no new UI widget.

## 2. Parsing the card input

`fix_word_ipa(state, input)` parses `input` as `<word> <rest>`:

- `word` = the first whitespace-delimited token (the source word whose IPA to
  change), case-insensitive match against the block text.
- `rest` = everything after the first token, trimmed.
- If `rest` contains an IPA span (`/…/` with a non-ASCII-letter inner) → that span
  is the **literal** replacement IPA (`new_ipa`), used as-is, no LLM.
- Else `rest` is a plain hint → send to Claude (see §4) to get `new_ipa`.
- Empty `word` or empty `rest` → toast "Usage: word /IPA/ or word <hint>", return.

## 3. Splicing the new IPA into `gloss_text`

The cursor's Source block maps to a `<verse>` run in `gloss_text` (the
`gloss_blocks` parser already segments these; the block's `index` identifies
which source run). The splice operates on the **raw** `gloss_text` so the stored
inline IPA changes:

1. Locate the target `<verse>` line(s) for this source block index within
   `gloss_text` (the same segmentation `gloss_blocks` uses — reuse it or its
   underlying scan so the block boundaries match exactly).
2. Within those lines, find each `word /OLD_IPA/` pair where `word` matches the
   named word (case-insensitive, whole-word) and an IPA span immediately follows
   it. Replace each `/OLD_IPA/` with `/new_ipa/`. If the word has no following
   IPA span anywhere in the block → toast "No IPA for <word>", return (no write).
3. Write the edited full `gloss_text` back with
   `update_gloss(conn, gloss_id, &new_gloss_text)`.

Implementation note: a small pure helper
`replace_word_ipa(verse_text, word, new_ipa) -> Option<String>` (returns the
rewritten text, or None if no `word /IPA/` pair found) is the unit-testable core
— it takes a single verse line (or the block's text), not the whole gloss, so it
is easy to test against the screenshot case
(`"In daily /ˈdɛːli/ thanks…" , "daily", "/ˈdeɪli/"` →
`"In daily /ˈdeɪli/ thanks…"`).

## 4. LLM path (hint → IPA)

When `rest` is a hint, ask Claude for just the one word's OP IPA. A new tiny
prompt (or a focused `build_*` message) instructs: return ONLY the IPA for the
given word, in forward slashes, Original Pronunciation, honoring the hint (e.g.
"hard a" → FACE vowel). Parse the first `/…/` span from the reply as `new_ipa`;
if the reply has no parseable IPA span → toast "Could not get IPA", return. The
call reuses the existing `call_claude_with_prompt` plumbing and the gloss
overlay's loading affordance. (The literal-typed path skips this entirely.)

## 5. Delete the stale block audio + re-synthesize + play

After the `gloss_text` rewrite:

1. `delete_gloss_audio_block(conn, gloss_id, "source", block_index)` — NEW query:
   `DELETE FROM gloss_audio WHERE gloss_id=?1 AND kind=?2 AND paragraph_index=?3`
   (all voices for that block). Collect the deleted rows' `audio_path`s first and
   `std::fs::remove_file` each (best-effort; a missing file is fine).
2. Re-segment: the in-memory `gloss_list[gloss_index].gloss_text` must be updated
   to the new text so `play_block_tts` reads the corrected verse (it calls
   `gloss_blocks(&gloss.gloss_text)`). Refresh the gloss list entry (re-read via
   `find_all_glosses`, or patch the in-memory `gloss_text`) before playing.
3. `play_source_tts_pausing_mpv(state, block_index)` → cache miss (we just
   deleted it) → `play_block_tts` synthesizes the corrected line in the active/
   default voice, writes the new MP3, and plays it. MPV is paused first.

The "Synthesizing…" persistent pill (already implemented) covers the
re-synthesis wait and clears on playback.

## 6. Cheat-sheet (the typing path's reference)

Add `docs/guides/op-ipa-cheatsheet.md`: the ~25 OP symbols the user will
actually type, each with a Shakespeare example word and the modern↔OP contrast,
so the literal-IPA path is usable without external lookup. Cover at least:

- **Vowels / diphthongs:** `eɪ`/`eː` FACE (daily, gave), `əɪ` PRICE (wise, I),
  `ʊ` STRUT-class (love, blood), `ɛ`/`ɛː` DRESS & length, `ɔ`/`ɔː` THOUGHT,
  `ɑ`/`ɑː` PALM, `ə` schwa, `ɪ` KIT, `ʌ` (where used), `aʊ` MOUTH, `ɔɪ` CHOICE.
- **Consonants:** `ʃ` ʒ `tʃ` `dʒ` `θ` `ð` `ŋ`, rhotic `r` (OP is rhotic — sound
  the `r`).
- **Marks:** `ˈ` primary stress, `ˌ` secondary, `ː` length.

Cross-reference it from `docs/guides/elevenlabs-v3-custom-voices.md` and note the
`show-gloss-ipa-tts` skill as the way to read existing IPA. The cheat-sheet is a
learning aid, not load-bearing for the feature.

## Key files (when implemented)

- `src/input/keymap.rs` — bind `i` in `handle_gloss_key` (and its Ctrl+/ overlay
  entry per the keybind-overlay rule).
- `src/input/actions/gloss.rs` — `open_fix_ipa_prompt`, `fix_word_ipa`, the
  `replace_word_ipa` pure helper, the `submit_gloss_prompt` `FixIpa` arm; reuse
  `source_block_index`, `play_source_tts_pausing_mpv`, `gloss_blocks`,
  `show_tts_toast`.
- `src/app.rs` — `GlossPromptMode::FixIpa` variant.
- `src/db/queries.rs` — `delete_gloss_audio_block(conn, gloss_id, kind, index)`;
  reuse `update_gloss`.
- `docs/guides/op-ipa-cheatsheet.md` — new typing reference.

## Out of scope / future

- **Per-occurrence disambiguation** when a word repeats in a block (default is
  all occurrences).
- **Editing the IPA of an EXPLICATION word** — explication prose carries no IPA
  by design, so the fix is source-verse-only.
- **A visual IPA editor / picker** — typing or the LLM hint is the v1 input.
- **Cross-block / whole-gloss IPA pass** — this feature is single-word, single
  block; broad re-IPA stays the job of `e` (edit gloss).

## Testability

- Pure helper `replace_word_ipa(text, word, new_ipa) -> Option<String>`:
  unit-test the screenshot case (daily `ɛː`→`eɪ`), all-occurrences, whole-word
  (don't match "daily" inside "dailygram"), word-without-IPA → None, and the
  literal-vs-hint detection (`is_ipa` on `rest`).
- `delete_gloss_audio_block` DB test: seed two blocks' audio, delete one block,
  assert the other survives.
- Runtime (user check): on gloss 21728, cursor on the "daily" line, `i`, type
  `daily /ˈdeɪli/` → hear "daily" with a hard a; and `daily hard a` → LLM path
  yields a hard-a IPA and plays.

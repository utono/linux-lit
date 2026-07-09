# Vocab-Sentence Loop Mode — Design

**Date:** 2026-07-09
**Status:** Approved (brainstorming complete)

A modal drill mode: navigate to the next/previous sentence containing one or
more vocab words, play it from the sentence's start time, karaoke-highlight it,
and loop the sentence gaplessly until the user advances.

## Motivation

Ctrl+r / Ctrl+Shift+R today jump the cursor between individual vocab word
occurrences with no audio. For vocabulary review the useful unit is the
*sentence* — hearing the word in its spoken context, repeated until it sticks.
The building blocks already exist: `vocab_matches` (word + line + char span),
`sentence_bounds()` (sentence extent within a line/paragraph), and
`phrase_timestamps` spans (char-range → audio time). This mode composes them.

## Decisions (from brainstorming)

- **Loop until advance** — the sentence repeats until `n`/`p`; no auto-advance,
  no pause-at-end.
- **Entry via Ctrl+r, exit via Escape** — no new toggle key.
- **`n` = next, `p` = previous** vocab sentence (wrapping).
- **Phrase-data works only** — where the playing media has no
  `phrase_timestamps`, Ctrl+r keeps its current plain-jump behavior.
- **Sweep + sentence tint** — static sentence-width tint for the loop's
  duration, moving phrase-level karaoke sweep inside it.
- **Fully modal** — only `n`, `p`, `a`/Space (play-pause), and Escape do
  anything; every other key is swallowed.
- **MPV native ab-loop** for the loop mechanic (gapless, sample-accurate),
  not TimePos-driven seek-back.

## 1. Entry, exit, keys

- **Ctrl+r** — when the playing media has phrase-timestamp data and at least
  one vocab sentence resolves: enter VocabLoop mode on the first vocab
  sentence at/after the cursor. Otherwise (no phrase data, no vocab matches,
  or MPV not connected): today's plain vocab jump, unchanged.
- **Ctrl+Shift+R** mirrors it, entering on the previous vocab sentence before
  the cursor (same fallback).
- **In-mode** (fully modal — everything else swallowed):
  - `n` — next vocab sentence (wraps)
  - `p` — previous vocab sentence (wraps)
  - `a` / Space — play-pause
  - `Escape` or Ctrl+r — exit; ab-loop cleared, playback continues normally
    from wherever it is, normal sync resumes
- Each jump shows a toast: `vocab 3/17 — "chancery"` (position + the vocab
  word(s) in the sentence).

## 2. Data — `VocabLoopState`

Built eagerly on entry, in a new `src/input/vocab_loop.rs`:

- Group `state.vocab_matches` by sentence: for each match,
  `sentence_bounds(line_text, char_start, char_end)`; dedupe by
  `(line_index, sentence_start_char)`. A sentence with several vocab words is
  one entry carrying all its words.
- Resolve each sentence's `[start_time, end_time]` from the phrase spans of
  its line (`phrase_spans_for_line`, one query per distinct line): start =
  `start_time` of the first span intersecting the sentence's char range,
  end = `end_time` of the last. Sentences with no intersecting spans are
  dropped.
- Empty result → toast "no vocab sentences with audio", fall back to plain
  jump; the mode is never entered half-working.
- State shape:
  `Vec<VocabSentence> { line_index, sent_char_range, start_time, end_time, words }`
  plus the current index. Discarded on exit and on work/media switch.

## 3. MPV integration (ab-loop)

- Two new commands in `src/mpv/commands.rs` / `src/mpv/client.rs`:
  - `SetAbLoop(a, b)` → IPC `set_property ab-loop-a` / `ab-loop-b`
  - `ClearAbLoop` → both properties to `"no"`
- Enter/`n`/`p`: `SetAbLoop(start, end)` + `Seek(start)` + unpause. MPV loops
  the interval itself — gapless.
- `ClearAbLoop` fires on **every** exit path: Escape/Ctrl+r, work switch
  (`display_work`), media switch, and MPV quit — one `exit_vocab_loop()`
  funnel so a leaked ab-loop can never trap normal playback.
- Sync interplay: playback time never leaves the sentence, so the sync cursor
  naturally stays on its line. Defensively, while the mode is active the
  TimePos handler skips page-turn scheduling (`pending_prose_cross` etc.) so
  a sentence ending at a page boundary can't fire a turn mid-loop.

## 4. Highlight

- On entry/advance the page lands on the canonical spread for the sentence's
  line (same landing as today's vocab jump); the cursor moves to that line.
- **Sentence tint**: the full sentence char range gets the sentence-width
  tint (the LINE-mode tag) for the entire time it loops — its extent is
  always visible. Applied on jump, removed on advance/exit.
- **Phrase sweep**: the existing PHRASE-mode karaoke tint runs inside it,
  driven by TimePos as today. The mode forces sweep-inside-tint regardless of
  the work's configured `PhraseHighlightMode`, restoring the configured mode
  on exit. Each repetition the sweep resets naturally because playback time
  resets.
- Vocab-word coloring (per-work `vocab_highlight`) is untouched and shows
  through.

## 5. Testing & verification

- Pure unit tests (no GTK): sentence grouping/dedup from synthetic
  `VocabMatch`es + spans; time-range resolution including dropped span-less
  sentences; `n`/`p` wraparound. `sentence_bounds` is already covered.
- `cargo build` clean + `cargo test --bins` suite.
- Keybinds overlay cross-reference via the `update-cairo-keybinds-overlay`
  skill (Ctrl+r's `describe()` arm changes).
- Live verification in `crll` on BH-Barrett (currently the only media with
  the newest phrase grouping) — loop gaplessness and tint are audible/visual
  acceptance criteria, so the final check is the user's.

## 6. Files touched

- `src/input/vocab_loop.rs` — new: state, list build, enter/exit/advance
- `src/input/keymap.rs` — `InputMode::VocabLoop` + modal key handler
- `src/input/actions/concordance.rs` — Ctrl+r / Ctrl+Shift+R branch
- `src/mpv/commands.rs`, `src/mpv/client.rs` — ab-loop commands
- `src/main.rs` — TimePos handler: page-turn guard while mode active
- `src/input/phrase_highlight.rs` — forced mode + sentence tint
- `src/ui/keybinds_overlay.rs` — Ctrl+r detail panel update
- No `keymap.json` change (Ctrl+r keeps its existing Action).

## Out of scope

- Verse works / line-timestamp fallback (mode is phrase-data-only by
  decision; revisit if wanted later).
- Auto-advance drill loop, repetition counts, or configurable loop gaps.
- A picker listing all vocab sentences.

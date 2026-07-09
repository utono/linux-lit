# Phrase Highlight Line Mode — Design

**Date:** 2026-07-08
**Status:** Approved (brainstorming session)

## Problem

The karaoke tint during MPV narration sync (`src/input/phrase_highlight.rs`)
highlights only the currently spoken **phrase** — the sub-line span from
`phrase_timestamps`. The user wants an option to highlight a larger unit:
the whole line (verse) or the current sentence (prose), while keeping the
existing phrase behavior available.

## Decisions (from brainstorming)

- **Mode setting, not replacement.** Phrase-level highlighting stays; line-level
  is a third state alongside off/phrase.
- **Alt+p cycles Off → Phrase → Line** per class (prose vs plays/poetry),
  replacing the current on/off toggle. No new keybind.
- **Line mode still requires phrase data.** The tint is keyed off the active
  phrase span exactly as today; lines without phrase spans stay untinted in
  every mode. Line mode is NOT driven by `line_timestamps` alone.
- **Prose "line" = current sentence**, not the whole paragraph (a prose buffer
  line is a full wrapped paragraph — too large a block). Verse "line" = the
  whole buffer line.
- **Line is the default** (amended mid-implementation, 2026-07-08): the prose
  default mode is `Line` (sentence highlighting), and the legacy boolean
  `true` deserializes to `Line` — NOT `Phrase` — so a stored `true` from the
  boolean era lands on sentences (stored config values override compiled
  defaults, so mapping `true` to `Phrase` would have pinned existing configs
  to phrase-width forever). Verse default stays `Off`.

## Design

### 1. Config (`src/config.rs`)

New enum, serialized as lowercase strings:

```rust
pub enum PhraseHighlightMode { Off, Phrase, Line }
```

- The existing fields keep their names — `phrase_highlight_prose`,
  `phrase_highlight_verse` — but change type from `bool` to the enum.
- **Custom `Deserialize` accepts both** the legacy JSON boolean
  (`true` → `Phrase`, `false` → `Off`) and the new string form
  (`"off"` / `"phrase"` / `"line"`), so existing `config.json` /
  `config-dev.json` load unchanged and rewrite as strings on next exit.
- Defaults unchanged in effect: prose = `Phrase`, verse = `Off`.
- The mode enum gets a `cycle()` (Off → Phrase → Line → Off) and
  `is_on()` (`!= Off`) helper.

### 2. Alt+p handler (`src/input/keymap.rs`, `TogglePhraseHighlight` arm)

- Cycles the current class's mode via `cycle()`, saves config.
- **Clears the tint on every transition** (not just when landing on Off), so a
  stale phrase-width tint never lingers when entering Line mode; the next
  `TimePos` tick repaints at the new width.
- Toast: `Phrase highlight PHRASE (prose)` / `LINE (plays/poetry)` /
  `OFF (…)` — same `show_chapter_toast` + `PHRASE_HL:` log line as today.

### 3. Driver (`src/input/phrase_highlight.rs`)

`update_phrase_highlight` is **unchanged** through gating (mode `is_on()`
replaces the bool check), spoken-line resolution (`resolve_spoken_idx`),
cache fill (`phrase_cache`), and `phrase_at_time`. After the active span is
found, the applied character range depends on the class's mode:

- `Phrase`: `[span.start_char, span.end_char)` — exactly today's behavior.
- `Line`, verse class: `[0, line_chars)` — the whole buffer line.
- `Line`, prose class: `sentence_bounds(line_text, span.start_char,
  span.end_char)` — see below.

The dedup key `active_phrase: (bl, span_idx)` stays as-is. In Line mode,
consecutive spans within the same sentence re-apply an identical range
(a harmless remove+apply); the state machine is untouched.

### 4. `sentence_bounds` (pure helper, unit-tested)

`fn sentence_bounds(text: &str, start_char: usize, end_char: usize) -> (usize, usize)`

- All offsets are **unicode-char indices** (matching GTK iter line offsets and
  the phrase spans' Python-backfill indices), never byte indices.
- Scan **backward** from `start_char` to just after the previous sentence
  terminator; scan **forward** from `end_char` through the next terminator
  plus trailing closing quotes (`’ ” ' " )`).
- Terminators: `.` `!` `?`.
- **Abbreviation guard:** a `.` is not a terminator when the word before it is
  a common title/abbreviation (Mr, Mrs, Dr, St, Ms, Prof, etc.) — Bleak House
  prose is full of these.
- A span crossing a sentence boundary tints **both** sentences (backward from
  span start, forward from span end).
- Mis-detection is benign: a wrong-width tint, never a crash — all offsets are
  clamped by the existing `apply_phrase_tag` guards.

### 5. UI mirrors

- `src/ui/keybinds_overlay.rs` — update the Alt+p `describe()` arm to the
  3-state cycle wording (run the `update-cairo-keybinds-overlay` cross-check).
- `keymap.json` / `keymap_config.rs` — **untouched**; the action name
  `TogglePhraseHighlight` is unchanged.

## Testing

- Config round-trip tests: legacy bools deserialize to the right modes; string
  form round-trips; the existing defaults test updated for the enum.
- `sentence_bounds` unit tests: mid-sentence span; first/last sentence of a
  paragraph; abbreviation guard (`Mr. Tulkinghorn`); span crossing a sentence
  boundary; terminator followed by a closing quote.
- Existing `phrase_at_time` / `resolve_spoken_idx` tests unchanged.
- `cargo test --bins` (bin-only crate) + `cargo clippy`.
- Visual acceptance: user `crll` run — a verse work cycled to LINE (whole-line
  tint tracks narration) and a Bleak House paragraph in LINE (sentence-width
  tint). Headless cage check optional.

## Out of scope

- Line-mode highlighting driven by `line_timestamps` alone (works without
  phrase data) — explicitly declined; may revisit later.
- Whole-paragraph prose tint.
- Any change to the sync cursor / preroll behavior or `phrase_highlight_bg`
  theming.

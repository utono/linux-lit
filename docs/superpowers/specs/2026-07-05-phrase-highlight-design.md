# Phrase Highlight During Narration (Whispersync-style) — Design

**Date:** 2026-07-05
**Status:** Approved (brainstorming session)

## Goal

Kindle-immersion-reading-style karaoke highlight: while MPV narrates a work,
the phrase currently being spoken gets its own highlight inside the paragraph,
driven by `phrase_timestamps` char ranges. On by default for prose works; off
by default for plays and poetry (toggleable either way).

## What already exists

- `phrase_timestamps` (lit.db): per-phrase `(line_mapping_id, media_id,
  start_time, end_time, start_char, end_char)`. Populated for all Dickens
  novels (~490k rows); char offsets index the paragraph's text — the same
  string the buffer holds (established by the prose page-crossing work).
- `src/db/queries.rs::phrase_crossing_time` — the existing single-value phrase
  query used for page-turn scheduling; the new code follows its shape.
- The MPV `TimePos` event handler in `src/main.rs` already drives all sync
  (line highlight, page crossings) at <100ms accuracy.
- `src/input/highlight.rs` + the `cursor-line` TextTag pattern
  (`theme.rs::cursor_line_bg`) show how per-theme background tags are applied.
- `is_prose_work()` / `PROSE_TYPES` in `src/db/line_types.rs` classify works.

## Approach (chosen: A — lazy per-paragraph cache)

On each `TimePos` event during playback, if the feature is active for the
current work's class:

1. Resolve the sync-current work line (already computed by the sync path).
2. If the cached phrase list is for a different `(line_mapping_id, media_id)`,
   run one indexed query — new `queries.rs` fn `phrase_spans_for_line(conn,
   line_mapping_id, media_id) -> Vec<PhraseSpan>` — and cache it in
   `AppState` (a paragraph has a few dozen phrases; the query fires only when
   narration enters a new paragraph).
3. Binary-search the cached spans by **raw playback time** (no preroll —
   the phrase being spoken *now*; page turns keep their 0.5s lead
   independently).
4. If the active phrase index changed, move the `phrase-highlight` TextTag to
   the buffer range `paragraph line start + start_char .. + end_char`.

Rejected alternatives: (B) preload all phrases per work at media connect —
simpler lookup but a load hitch, MBs resident, and a media-switch reload path
that A gets for free; (C) tick-callback interpolation between TimePos events —
unneeded, phrases run 0.3–1.5s and the event stream is already <100ms accurate.

## Components

### State (`AppState`)

- `phrase_spans: Option<PhraseCache>` where `PhraseCache { line_mapping_id:
  i64, media_id: i64, spans: Vec<PhraseSpan> }`. A cached **empty** vec is a
  valid negative result (work/paragraph without phrase data) — do not re-query
  every tick.
- `active_phrase: Option<usize>` — last applied span index, to skip redundant
  tag moves.

### Query (`src/db/queries.rs`)

- `PhraseSpan { start_time: f64, end_time: f64, start_char: usize, end_char:
  usize }`.
- `phrase_spans_for_line(...)` — `SELECT start_time, end_time, start_char,
  end_char FROM phrase_timestamps WHERE line_mapping_id=? AND media_id=?
  ORDER BY start_time` (uses `idx_phrase_timestamps_work`).

### Time → span lookup (pure helper, unit-testable)

- `phrase_at_time(spans, t) -> Option<usize>`: the span whose
  `[start_time, end_time)` contains `t`; during an inter-phrase **gap**, hold
  the previous span (no flicker); before the first span, `None`.

### Rendering

- One `phrase-highlight` TextTag on the main buffer, background from the theme.
- Theme color: new **optional** key `phrase_highlight_bg` in
  `themes-unified.json`, read in `theme.rs` with a computed fallback —
  `cursor_line_bg` with its alpha roughly doubled (clamped) — so no themes-repo
  change is required to ship.
- Tag priority: above `cursor-line`, below selection. Coexists with vocab /
  reader-gloss tints (background-only tag).
- Char offsets: GTK TextIter offsets are unicode chars, matching the Python
  backfill's str indices. Same-string assumption already validated by the
  crossing code; clamp `end_char` to the buffer line length defensively.
- Paragraph straddling a page boundary is a non-issue: pages are scroll
  positions over one buffer, so the tag range is valid regardless of the page.

### Lifecycle

- Applied only while sync is driving (playing, not sync-suppressed).
- **Pause**: tag stays (shows where the audio stopped).
- **Cleared** on: manual navigation / any seek-suppression path, work switch,
  media switch, toggle-off. Cache invalidated on work/media switch.

### Toggle + config

- New `Action::TogglePhraseHighlight`, bound to **Alt+p**.
- Config (`src/config.rs`): `phrase_highlight_prose: bool` (serde default
  `true`), `phrase_highlight_verse: bool` (serde default `false`). Alt+p
  flips the flag for the **current work's class** (`is_prose_work()` picks
  which) and persists; a toast reports the new state (e.g. "Phrase highlight
  ON (prose)").
- Activation check per tick: cheap bool read —
  `if is_prose { cfg.phrase_highlight_prose } else { cfg.phrase_highlight_verse }`.
- Keybind wiring is the usual trio: `keymap_config.rs` default + stowed
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` + Ctrl+/ overlay
  (`keybinds_overlay.rs` KeyDef + `describe()` arm, via the
  `update-cairo-keybinds-overlay` skill).
- Dev-config gotcha applies: defaults only take effect in a `config-dev.json`
  that doesn't already have the keys; documenting here so nobody "fixes" it.

## Scope / non-goals

- Reader-side only. Plays/poetry currently have **no** `phrase_timestamps`
  rows, so toggling them on shows nothing until litdb's
  `backfill-phrase-timestamps` is run against their media — expected, not a
  bug. No litdb work in this feature.
- No change to page-turn timing, preroll, or line-highlight behavior.
- No per-work override (per-class flags only; revisit if ever needed).

## Error handling

- No phrase rows / no timestamp on line / non-synced work: feature silently
  inactive (cached empty vec), everything behaves exactly as today.
- Out-of-range char offsets: clamp to line length; log once per paragraph via
  `log_fmt!` if clamping occurred (data-quality signal for litdb).

## Testing

- Unit: `phrase_at_time` (exact hit, gap-hold, before-first, after-last),
  `phrase_spans_for_line` (in-memory SQLite, mirrors the
  `phrase_crossing_time` test), config serde defaults, class-flag selection.
- Live acceptance: user listens to a Dickens work (`crll`), verifies the
  highlight tracks the narrator with no preroll lead and clears on manual
  nav. (`test-playback-sync` false-stalls on BH, so no automated sync e2e;
  a cage screenshot during seek-driven playback can spot-check the tag
  visually if needed.)

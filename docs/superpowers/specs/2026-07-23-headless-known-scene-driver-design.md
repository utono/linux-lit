# Headless known-scene driver — design

**Backlog item #15.** A small test helper that opens a specific
work + scene + overlay deterministically, so verifying a Q&A / synopsis path
doesn't depend on wherever `last_work` happens to be.

## Problem

The synopsis and empty-state drives were flaky this session because the harness
landed on wherever `last_work` pointed. `LIT_START_WORK` / `LIT_START_POS`
already pin the *work* and *line*, but there is no deterministic way to land on
a specific **scene** and have a specific **overlay** already open — so a
Q&A/synopsis drive still has to navigate there by hand (fragile) or trust the
config's last position (non-deterministic).

## Key facts (from codebase survey)

- Startup already reads hermetic overrides: `LIT_START_WORK`
  (`src/app/mod.rs:2038`), `LIT_START_POS` (`:3416`), `LIT_START_COLUMNS`
  (`:2054`). This is the established seam — extend it, don't invent a parallel one.
- `run-fuzz.sh` already surfaces `--start-work` → `LIT_START_WORK` and threads
  `LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_LOG_PATH LIT_DB_PATH`. The env-var → flag
  plumbing pattern is set.
- A scene is a `(div1, div2)` pair; the app can resolve a line for a scene start
  from `LineMap.section_starts` (authoritative-boundary principle — do not infer
  from text).

## Decisions

Extend the existing `LIT_START_*` family with two new optional variables, read at
the same startup point:

- **`LIT_START_SCENE="div1.div2"`** — land the cursor on that scene's start line
  (resolved from `section_starts`, not text). Mutually exclusive with
  `LIT_START_POS`; if both set, `LIT_START_SCENE` wins and logs the override.
- **`LIT_START_OVERLAY="journal|synopsis|gloss"`** — after the work + scene are
  in place, open that overlay to the current position, exactly as the
  corresponding keybind would (reuse the open handlers, no duplicated logic).

A thin driver script `scripts/land-on.sh` (or a `--land WORK div1.div2 overlay`
subcommand on the existing e2e harness) sets these three vars plus the standard
hermetic env, launches under cage, and returns once the log shows the overlay
mapped — giving a deterministic starting state for any subsequent `wtype` drive.

## Components

1. **Startup env reads** — in the same block as `LIT_START_WORK` handling
   (`src/app/mod.rs`), parse `LIT_START_SCENE` (→ resolve scene-start line via
   `section_starts`, set as the pending start line) and stash a
   `pending_start_overlay` on `AppState`.
2. **Deferred overlay open** — once the work is displayed and the line map is
   built (the point where a normal launch would be interactive), if
   `pending_start_overlay` is set, dispatch the matching overlay-open action
   once, then clear it. Reuse `ToggleJournalOverlay` / synopsis / gloss open
   handlers — no new open logic.
3. **Driver script** — `scripts/land-on.sh WORK div1.div2 [overlay]`: exports
   `LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_START_WORK LIT_START_SCENE
   [LIT_START_OVERLAY] LIT_LOG_PATH LIT_DB_PATH`, launches cage, waits for the
   mapped-overlay log line, leaves the instance up for the caller to drive.
   Mirrors `run-fuzz.sh`'s env handling and DB-copy hygiene.

## Data flow

`land-on.sh WORK d1.d2 journal` → env → app startup pins work
(`LIT_START_WORK`) → resolves scene-start line (`LIT_START_SCENE` via
`section_starts`) → displays work at that line → deferred open of `journal`
overlay → logs `OVERLAY_MAPPED` → script returns → caller runs its `wtype` drive
against a known state.

## Error handling

- Unknown work / unresolvable scene: log a clear error and exit non-zero so the
  driver fails loudly instead of silently landing elsewhere.
- Overlay that has nothing to show at that scene (e.g. synopsis front-matter):
  open anyway to the empty state (that IS a valid deterministic target — the
  empty-state drive is one of the flaky cases this fixes).
- `LIT_START_SCENE` + `LIT_START_POS` both set: `LIT_START_SCENE` wins, log it.
- Must NOT rewrite the dev config's `last_work` (the exact bug this addresses) —
  the hermetic launch already avoids persisting position under
  `LIT_HEADLESS_TEST`; confirm the new vars don't reintroduce a write.

## Testing

- Run `land-on.sh` for a play scene + journal overlay and confirm (from the log +
  a grim capture) the cursor is on the scene start and the overlay is open — twice,
  asserting identical landing regardless of prior `last_work`.
- Confirm a bad scene arg exits non-zero.
- Confirm the dev config's `last_work` is unchanged after a run.

## Out of scope

- No general "open at arbitrary journal entry id" (that's the recent-Q&A path).
- No non-headless use — this is a test harness helper, gated on the hermetic env.

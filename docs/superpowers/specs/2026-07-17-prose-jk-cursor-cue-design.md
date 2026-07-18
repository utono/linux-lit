# Prose j/k Cursor Cue (no MPV seek) — Design

**Date:** 2026-07-17 (US Central)
**Status:** Approved for planning

## Problem

On prose works, the `j` / `k` main-card binds
(`Action::CursorNextDialogueNoSeek` / `CursorPrevDialogueNoSeek`) move the
cursor to the next / previous prose segment and deliberately do **not** seek
MPV (`PageChangeReason::Cursor`, which `should_seek()` excludes). That
navigation half is correct and confirmed by the live debug log
(cursor moved line 33↔34 with no `SEEK:` line).

The defect is purely visual: in non-dim prose there is **no persistent cursor
tint** (`highlight.rs` `prose_no_tint` branch skips `apply_tag(cl_tag, …)`),
and nav binds no longer flash. The only prose cursor cue is the karaoke phrase
tint painted by `seek_to_current_line` — which `j` / `k` skip along with the
seek. Result: `x` / `y` (which seek) show a visible landing; `j` / `k` (which
don't) look completely dead even though the cursor is moving.

Reported on work `TT` ("A Tale of a Tub", `work_type=prose`), a fully
timestamped prose work (271 line timestamps, 5329 phrase rows).

## Goal

Pressing `j` / `k` on a prose work paints a visible cursor cue at the landed
segment, **without** seeking MPV — so prose navigation reads the same as
`x` / `y` do, and audio keeps playing wherever it was.

Non-goal: any change to the navigation targeting, the play/verse behavior of
`j` / `k`, or the MPV-seek gating. Cursor movement already works; only the cue
is added.

## Approach (Option A — karaoke phrase tint, no seek)

`seek_to_current_line` already separates the two concerns:

- the MPV seek — `cmd_tx.try_send(MpvCommand::Seek(seek_time))`
- the karaoke tint — `phrase_highlight::paint_pending_phrase(state, base)` +
  `state.phrase_paint_hold = state.suppress_sync_until`

The `_no_seek` handlers will paint the tint **without** issuing the seek.

### Where

`src/input/navigation.rs`:
- `cursor_next_dialogue_no_seek`
- `cursor_prev_dialogue_no_seek`

After the cursor moves and `after_page_change(state, PageChangeReason::Cursor)`
runs (which repaints highlight but skips the seek), paint the pending phrase at
the **new** cursor line's timestamp start.

### Mechanism

A small helper (in `navigation.rs`, near `seek_to_current_line`) that:

1. Resolves the current cursor line's `work_line_for_buffer` → `work.lines[wi]`.
2. If that line has a `timestamp`, calls
   `phrase_highlight::paint_pending_phrase(state, ts.start)`; when it returns
   `true`, sets a hold so the tint is not immediately overwritten by a sync
   tick. Because there is no seek and thus no fresh `suppress_sync_until`, the
   hold is a short fixed duration (reuse `SYNC_SUPPRESS_SEEK`) applied to both
   `suppress_sync_until` and `phrase_paint_hold` — mirroring the seek path's
   hold pattern but self-contained. When paused (`!mpv_playing`), the existing
   sync-suppression rules keep the tint stable regardless.
3. Does **not** send any `MpvCommand`.

This helper is called at the tail of both `_no_seek` handlers, gated on
`state.is_prose()` (plays/verse keep their current behavior — they already show
a persistent line tint via the non-prose highlight branch, so they are not
invisible and need no change).

### Fallback for untimestamped prose

`paint_pending_phrase` returns `false` and paints nothing when the work has no
phrase data or the line is untimestamped. For such prose works, fall back to a
**brief cursor-line flash** using the existing prose-flash machinery
(`request_prose_flash` / `flush_pending_prose_flash`, which animate
`prose_flash_tag` with `theme.phrase_highlight_bg`). This reuses code that
still exists (it was kept for the overlay-close path) and gives a visible cue
without a persistent tint. Gate: only when `paint_pending_phrase` returned
`false` AND `state.is_prose()`.

TT and other timestamped prose take the phrase-tint path; the flash is only for
prose without timestamps.

## Data flow

```
j/k pressed
  → CursorNextDialogueNoSeek / CursorPrevDialogueNoSeek
    → cursor_next/prev_dialogue_no_seek(state)
      → next/prev_dialogue_line(...)  [unchanged: prose = every non-blank]
      → state.current_line = target
      → after_page_change(state, Cursor)   [repaints highlight, NO seek]
      → paint_prose_nav_cue(state)         [NEW]
          if is_prose:
            if line timestamped && paint_pending_phrase(state, start):
                set short phrase_paint_hold
            else:
                request_prose_flash(state)  [brief fade cue]
```

No `MpvCommand::Seek` is ever sent on this path. `should_seek()` /
`PageChangeReason::Cursor` are untouched.

## Error handling / edge cases

- **No current work / cursor line unmapped:** helper returns early, no cue (same
  as `seek_to_current_line`'s guards).
- **Translations visible:** `paint_pending_phrase` already returns `false` when
  `translations_visible`; the flash fallback should also no-op in that mode
  (translation view has its own highlight path). Guard the helper on
  `!state.translations_visible`.
- **Play / verse work:** helper no-ops (gated on `is_prose()`); existing
  persistent line tint remains the cue.
- **Vocab loop active:** the existing `after_page_change` already special-cases
  `vocab_loop`; the cue helper must not disturb an active vocab-loop tint —
  no-op when `state.vocab_loop.is_some()`.
- **Sync suppression interaction:** the short hold must not clobber a legitimate
  longer existing hold (e.g. work-load window). Follow the same
  "don't shorten an existing longer suppression" rule
  `seek_to_current_line` uses.

## Testing

- **Unit / logic:** none strictly required — the change is a paint side effect.
- **Headless e2e (primary):** drive `j` / `k` on TT via the nav-fuzz / headless
  harness and screenshot. Assert the landed paragraph shows the phrase tint and
  the `SEEK:` log line is **absent** for the j/k presses (no MPV seek). Use the
  `test-headless-navigation` skill with `--start-work TT`.
- **Log check:** confirm `ACTION: CursorNextDialogueNoSeek` is followed by a
  phrase-paint (no `SEEK:` line) for a timestamped prose work.
- **Regression:** verify `x` / `y` still seek + tint, and that plays/verse j/k
  behavior is unchanged.
- **Manual eyeball:** hand the user the exact e2e command for a final look on
  the real GL renderer (cage is software rendering); the cue is a subtle tint
  and pixel-level.

## Docs / memory follow-ups (not blocking)

- The stale `PageChangeReason::Cursor` / `Dialogue` doc comment
  (`navigation.rs` ~line 126–130) and the crossed `(h key)` / `(k key)`
  handler doc comments describe the opposite of the real bindings. Fix them in
  the same change so the next reader isn't misled.
- Update memory `project-prose-nav-flash`: prose nav binds now paint a cue
  again (phrase tint for timestamped prose, brief flash fallback otherwise) —
  the "nav binds do NOT flash / no persistent cursor tint" statement is
  superseded for the j/k no-seek path.

# Speaker-turn navigation (J / K)

**Date:** 2026-06-09
**Status:** Approved, ready for implementation plan

## Summary

Add two reader-mode keybinds that jump the cursor to the first dialogue line of
the next / previous **speaker turn** — i.e. the next time the speaker changes:

- **`J`** (Shift+j) → next speaker turn: first dialogue line of the next speech
  block whose speaker differs from the current line's speaker.
- **`K`** (Shift+k) → prev speaker turn: first dialogue line of the previous
  speech block whose speaker differs from the current line's speaker.

Both **seek the MPV audio** to the landed line (like `q` / `comma`), so the
keys are the speaker-level analog of the existing next/prev-dialogue-line keys.

## Motivation

The reader already has `q` / `comma` to step one *dialogue line* forward/back
(with audio seek) and `j` / `k` for cursor-only line movement. There is no way
to jump speech-to-speech — from one character's turn to the next. In a
multi-character scene a reader often wants to skip to "the next person who
speaks" without stepping through every wrapped continuation line of the current
speech. `J` / `K` provide that larger stride along the same axis.

## Semantics

A **speaker turn** is a maximal run of consecutive work-lines that share the
same `speaker`. "Next speaker turn" means the first dialogue line of the very
next run whose speaker differs from the current line's speaker. Re-appearances
of the current speaker later in the scene are *not* skipped — for a speaker
sequence `A A B B A C`, from inside the first `A` block, `J` lands on the first
`B` line, then the second `A` block, then `C`.

The landing line is always the **first dialogue line of the target block** in
both directions:

- `J` (forward): the boundary naturally lands on the new block's opening line.
- `K` (backward): scan back to the previous block, then to *its* first line, so
  `K` lands at the top of the previous speaker's turn (not its last line).

## Data source — authoritative, no text inference

Each `Line` carries `speaker: Option<String>`, loaded from
`line_mapping.speaker` (`src/db/queries.rs:57,65,76`). Verified against
`Rom 1.1`: every line of a speech — including wrapped continuation lines —
repeats the speaker (e.g. `GREGORY` on consecutive wrapped lines 4–5, `SAMPSON`
on 12–13). There are **no `None`-speaker continuation gaps** inside a speech for
plays/verse.

We read this field directly via `LineMap.buffer_to_work[bl]` →
`work.lines[wi].speaker`. We **never** classify buffer text to detect a speaker
change. This follows the codebase's authoritative-boundary principle
(`CLAUDE.md` → "Pagination & Scene Boundaries": if `lit.db` encodes a per-line
fact, surface it through `LineMap`/`Line` and read it).

## Algorithm

All work is in **buffer-line** space (what `current_line` holds). Speaker of a
buffer line `bl`:

- With `LineMap`: `lm.buffer_to_work[bl]` → `Some(wi)` → `work.lines[wi].speaker`.
- Mid-load fallback (no `LineMap`): `work.lines.get(bl).and_then(|l| l.speaker)`
  by buffer index — the same fallback shape as `is_chapter_at` in
  `jump_to_prev_chapter` (`navigation.rs:1022-1032`).

A line with no work mapping (blank / separator / structural) yields `None`.

### `next_speaker_turn(state, from) -> Option<usize>`

1. `cur` = speaker of `from`.
2. Scan buffer lines `from+1 .. line_count`. Skip non-dialogue lines
   (`is_dialogue_line`). Return the first dialogue line whose speaker `!= cur`.
3. None found → `None` (no-op at end of work).

### `prev_speaker_turn(state, from) -> Option<usize>`

1. `cur` = speaker of `from`.
2. Scan buffer lines `from-1 .. 0` for the first dialogue line whose speaker
   `!= cur`; call its speaker `prev`. (This is the *last* line of the previous
   block.)
3. Continue scanning back while the dialogue line's speaker `== prev`; the last
   such line is the first dialogue line of that block. Return it.
4. None found → `None` (no-op at start of work).

### Edge cases

- **`cur == None`** (front matter, stage-only region, prose with no speakers):
  "differs from `cur`" matches any line whose speaker is `Some`. For a prose
  work where no line has a speaker, both keys are no-ops.
- **No further turn** in the requested direction → no-op; cursor stays. Mirrors
  `jump_to_next_dialogue` / `jump_to_prev_dialogue` at work boundaries.
- **Multi-speaker labels** (e.g. `CORNELIUS/VOLTEMAND`) are compared as whole
  strings — a distinct label is a distinct speaker. No special handling.

## Code structure

In `src/input/navigation.rs`, mirroring the existing dialogue-nav functions:

- Two helpers `next_speaker_turn(state, from) -> Option<usize>` and
  `prev_speaker_turn(state, from) -> Option<usize>` that perform the scan and
  return a buffer line. These are unit-testable against a synthetic
  `LineMap` + work.
- Two public handlers:
  - `jump_to_next_speaker(state)` — call `next_speaker_turn`; on `Some(target)`
    set `current_line`, clear `pending_advance` / `pending_advance_ignore_bl`,
    call `scroll_after_jump_forward(state, prev_line)`, then
    `after_page_change(state, PageChangeReason::Dialogue)`.
  - `jump_to_prev_speaker(state)` — call `prev_speaker_turn`; on `Some(target)`
    set `current_line`, clear the pending fields, reset
    `prev_highlight_line`, call `scroll_after_jump_backward(state)`, then
    `after_page_change(state, PageChangeReason::Dialogue)`.

The tail (`PageChangeReason::Dialogue`) makes `after_page_change` seek MPV to
the landed line and turn the page if the target is off-screen — identical to
`jump_to_next_dialogue` / `jump_to_prev_dialogue` (`navigation.rs:864-878`,
`849-862`).

## Wiring — the four mandatory touch-points

1. **`src/input/actions/mod.rs`** — add `Action::JumpToNextSpeaker` and
   `Action::JumpToPrevSpeaker`, matching the category/metadata that sibling
   nav actions carry.
2. **`src/input/keymap.rs`** — add dispatch arms routing the two actions to
   `jump_to_next_speaker` / `jump_to_prev_speaker`.
3. **`src/input/keymap_config.rs`** — in `nav_bindings()`:
   `KeyCombo::plain("J") -> JumpToNextSpeaker`,
   `KeyCombo::plain("K") -> JumpToPrevSpeaker`. GTK delivers Shift+j/Shift+k as
   key names `"J"` / `"K"` with the shift flag stripped by `is_uppercase_letter`
   (`keymap_config.rs:131`), so plain ASCII-letter combos are correct — no RPD
   symbol mapping needed.
4. **`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`** — add the same
   two bindings, or keymap.json silently overrides the compiled defaults
   (`CLAUDE.md` → "Keybind override").
5. **`src/ui/keybinds_overlay.rs`** — add `J` / `K` to the HOME_ROW caps and a
   `describe()` arm for each, via the `update-cairo-keybinds-overlay` skill
   (carries the mandatory three-pass cross-reference).

`J` and `K` are verified unbound in the compiled defaults and in both
keymap.json files.

## Testing

### Unit (pure, no GTK) — covers correctness

Build a synthetic work + `LineMap` with a known speaker sequence including
wrapped continuation lines and interleaved stage directions, e.g.:

```
A  A  [stage]  B  B  A  C  C
```

Assert:

- From inside the first `A` block, `next_speaker_turn` lands on the first `B`
  line; again → second `A`; again → first `C` line.
- From inside the last `C` block, one `prev_speaker_turn` lands on the single
  `A` (the previous block); a second press lands on the first `B` line; a third
  lands on the first `A` line.
- `K` lands on the **first** line of the previous block, not its last (verify
  the `B B` block: `prev_speaker_turn` from the `A` after it lands on the first
  `B`, not the second).
- Boundary no-ops: `next_speaker_turn` at the last block → `None`;
  `prev_speaker_turn` at the first block → `None`.
- `None`-speaker start (front matter) and prose-with-no-speakers → `None`.

### Headless visual check (request from user — landing/page-follow)

`J` / `K` page-turn when the target is off-screen (via `after_page_change` →
`scroll_after_jump_*`). The landing computation is covered by units, but the
on-screen result (highlight visible, page follows) is a render criterion. Ask
the user to run the headless single-work launch and press `J` repeatedly
through a multi-character scene, confirming each landing is a visible
highlighted first-line of a new speaker and the page follows. No new automated
e2e test is required for correctness.

## Out of scope (YAGNI)

- Jumping to a *specific named* character (cast picker) — not requested.
- "Next NEW character" / debut navigation — not requested.
- Skipping consecutive same-speaker blocks — explicitly not wanted (every
  speaker change is a stop).

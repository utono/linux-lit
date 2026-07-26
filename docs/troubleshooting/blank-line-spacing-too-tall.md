# Dialogue Spacing Failures (plays)

Frequency-ordered. Check #1 first — it presents as "spacing is gone" rather
than "spacing is wrong", and it is load-order dependent, so it does NOT
reproduce when you open the affected play directly.

---

## 1. NO dialogue formatting at all — stale `block_indent_tiers` (2026-07-25)

### Tell

A two-column play renders with **every** dialogue affordance missing at once:
speaker labels (KING, LAERTES) in plain body text instead of small-caps, no
gap above speakers, dialogue not indented, stage directions upright instead
of italic, act/scene headers not bold, blank lines at full height.

"All of it missing" is the diagnostic signature. When gaps are merely too
tall or too short, see the sections below instead — that is a tuning problem,
this is a total no-op.

**Confirm from the log before touching any code:**

```bash
rg -n "TEXT_FILE:|FORMATTING:|TIMING: apply_dialogue" linux-lit-dev.log
```

- Healthy: `FORMATTING: applied dialogue formatting (N lines)` is present and
  `TIMING: apply_dialogue_formatting` is non-zero (~40ms for Hamlet).
- Broken: **no `FORMATTING:` line at all** and `TIMING: apply_dialogue_formatting 0ms`
  for a multi-thousand-line play — the function early-returned without
  touching the buffer.

### Root cause

`state.block_indent_tiers` left over from the PREVIOUSLY loaded work.

`apply_dialogue_formatting` (`src/app/formatting.rs`) early-returns outright
when that vec is non-empty — the guard added in `6cdc8490` so block-aware
verse typography isn't clobbered:

```rust
if !state.block_indent_tiers.is_empty() { return; }
```

The off-thread text-file fast path in `display_work_at_with_prepared`
(`src/app/mod.rs`) set `buffer` + `line_map` but never cleared the field, so
tiers from a block-aware work survived into the next work. Every branch in
`rebuild_buffer_text` cleared it; that one path did not.

Reproduction is a **work switch**, not a single load:

1. Load a block-aware work — one with non-prose `line_mapping.block_type`
   rows (BH-Barrett has 135 `heading` + 20 `blockquote`; LoJ likewise). This
   populates the tiers.
2. Switch to a text-file play (Ham-Arkangel) via the library picker.
3. The play loads through the off-thread fast path with the tiers still set.

Launching straight into the play formats correctly — which is why this
survived testing and why "it works when I open it directly" is not evidence
against it.

### Fix

`state.clear_block_typography_state()` in the off-thread fast path, alongside
the `line_map` assignment. The helper resets all three fields that are keyed
by buffer-line index and therefore must never outlive a work:
`block_indent_tiers`, `italic_offset_map`, `italic_line_spans`.

Guarded by `block_typography_reset_tests` in `src/app/mod.rs`, which asserts
every buffer-fill branch performs the reset.

### General rule

Any state keyed by **buffer-line index** must be reset by **every**
buffer-fill path. A guard that reads such state is only as safe as the least
careful fill branch. When adding a new fill path, reset all three fields —
the test will fail if you forget.

### Aftermath: pinned page tables generated while formatting was broken

Fixing #1 exposes a second, separate symptom — **one row too many at the
bottom of each column**, the last line clipped by the card edge (sometimes
with a scrollbar). The formatting is correct; the PAGINATION is stale.

`play_pages` / `prose_pages` spreads are pinned in lit.db. A table generated
while dialogue formatting was suppressed measured rows with NO speaker gaps
(14px) and NO stage-direction gaps (8px), so it packed more rows per column
than now fit once those gaps came back.

**The layout fingerprint does NOT catch this.** `layout_fingerprint()`
(`src/input/page_table.rs`) hashes font family/size, ascent/descent, char
width, window geometry, line spacing, margins, columns, and top-spacer
height — nothing about whether per-tag `pixels_above_lines` are applied. A
table generated under broken formatting therefore still reads as a valid
`PAGES: table hit` and is accepted.

Tell: `PAGES: table hit (N pages)` where the generation timestamp falls
inside the window when formatting was broken.

```bash
# Which pinned tables were generated during a suspect window?
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT work_abbrev, layout_fingerprint, page_count, generated_at
     FROM play_pages_meta
    WHERE CAST(REPLACE(generated_at,'epoch:','') AS INTEGER) > <epoch>;"
```

Fix: delete the affected rows from BOTH `play_pages` and `play_pages_meta`
(matching `work_abbrev` + exact `layout_fingerprint`); the app regenerates on
next load at that geometry. Back up lit.db first and make sure no instance is
running (it rewrites config/state on exit).

Concretely on 2026-07-25/26: Ham-Arkangel's `v4 … 1920x1200` table was
generated at 21:52, one minute before the bug report, and pinned **81**
pages. Regenerated with formatting restored it is **85** — four more spreads
for the same text. It was the only affected play table.

Regenerating headlessly has two traps worth knowing:

- `page_table_gen_attempted` is a **one-shot latch per session**
  (`generate_and_store`), so resizing AFTER the first layout settles never
  regenerates. Resize to the target geometry during startup, before the
  layout settles.
- `LIT_GEN_PAGE_TABLE=1` does not override an existing table whose
  fingerprint still matches. Delete the rows first, then let it generate.

---

# Blank Lines Between Speakers/Stage Directions Too Tall

## Symptom

In dialogue-formatted works (plays), the blank lines separating speakers,
and between dialogue and stage directions, consumed too much vertical space.
The gap was roughly a full text line tall instead of a compact separator.

## Root Cause

In `src/app.rs` `apply_dialogue_formatting()`, blank lines were detected by
`line_types::is_blank()` and then skipped with `continue`. No formatting tag
was applied, so GTK rendered them at the full font height set by the
text view's default font.

```rust
if line_types::is_blank(text) {
    continue;  // no tag applied — full-height blank line
}
```

Speaker names had a `speaker-gap` tag with `pixels_above_lines(speaker_gap * 5)`
and stage directions had `stage-direction-gap` with `pixels_above_lines(10)`.
These pixel gaps stacked on top of the blank line, doubling the visual separation.

## Fix (four rounds)

### Round 1 — shrink blank lines

Added a `blank-line` tag with `scale(0.25)` and applied it to blank lines
instead of skipping them. The small scale factor shrinks the line's rendered
height to roughly a quarter of normal.

```rust
let blank_line_tag = gtk4::TextTag::builder()
    .name("blank-line")
    .scale(0.25)
    .build();
```

The tag is registered in the cleanup list so it gets removed and recreated
when dialogue formatting is re-applied.

### Round 2 — remove redundant pixel gaps

The gap was still too large because `pixels_above_lines` on the speaker and
stage-direction tags stacked with the scaled blank line. Removed
`pixels_above_lines` from both tags entirely, and removed the now-unused
`speaker_gap` variable.

Result: too little space — speakers ran into each other with no visible
separation.

### Round 3 — restore moderate pixel gaps

Added back `pixels_above_lines(8)` to both `speaker-gap` and
`stage-direction-gap` tags. Combined with the `scale(0.25)` blank line,
this produces a compact but visible break between speakers and between
dialogue and stage directions.

### Round 4 — reduce act/scene header gap

The act/scene header (`Act 4, Scene 1`) had `pixels_above_lines(20)`, which
combined with the blank lines above and below it created too much whitespace
around scene transitions. Reduced to `pixels_above_lines(8)` to match the
speaker and stage direction tags.

## Final Values

- Blank lines: `scale(0.25)` (~quarter height)
- Speaker names: `pixels_above_lines(8)`
- Stage directions: `pixels_above_lines(8)`
- Act/scene headers: `pixels_above_lines(8)`, `weight(700)`
- `speaker_gap` variable (formerly `line_spacing * 5`): removed

## Files Changed

- `src/app.rs` — `apply_dialogue_formatting()`:
  - Added `blank-line` tag creation, cleanup registration, and application
  - Changed `speaker-gap` from `pixels_above_lines(speaker_gap * 5)` to `pixels_above_lines(8)`
  - Changed `stage-direction-gap` from `pixels_above_lines(10)` to `pixels_above_lines(8)`
  - Changed `act-scene-header` from `pixels_above_lines(20)` to `pixels_above_lines(8)`
  - Removed unused `speaker_gap` variable

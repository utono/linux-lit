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

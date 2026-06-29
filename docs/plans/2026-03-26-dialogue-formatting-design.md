# Dialogue Indentation & Tight Spacing — Design Spec

**Date:** 2026-03-26
**Status:** Draft

## Overview

Text-file mode gets two formatting enhancements applied automatically when the work contains speaker lines: dialogue indentation (non-speaker lines indented ~60px) and tight spacing (zero gap between dialogue lines, larger gap before speaker names).

## When It Applies

- **Text-file mode only** — works loaded via `text_file` path, not DB-lines mode.
- **Speaker detection gate** — scan the first ~200 buffer lines for any `is_speaker()` match. If none found, skip all formatting (pure verse like Pope, Milton gets no indentation or spacing changes).

## Dialogue Indentation

A `"dialogue-indent"` GtkTextTag with `left-margin` set to the text view's current `left_margin` + 60px (so indent is relative to the base margin).

Applied to every buffer line that is:
- Not blank (`is_blank()` returns false)
- Not a speaker line (`is_speaker()` returns false)
- Not a stage direction (`is_stage_direction()` returns false)
- Not an act/scene marker (`is_act_scene_marker()` returns false)
- Not a separator (`is_separator()` returns false)

Speaker names, stage directions, act/scene markers, separators, and blank lines stay flush with the text area's left margin.

The line classification functions already exist in `src/db/line_types.rs` and operate on raw text strings — no DB lookup needed.

## Tight Spacing

When dialogue formatting is active:

- **Global spacing** — `pixels_above_lines` and `pixels_below_lines` set to 0 on the text view. Dialogue lines within a speech block have zero gap between them.
- **Speaker gap** — A `"speaker-gap"` GtkTextTag with `pixels-above-lines` set to 20px, applied to every speaker line. Creates visual breathing room before each new speaker.
- **Stage direction gap** — A `"stage-direction-gap"` GtkTextTag with `pixels-above-lines` set to 10px, applied to stage direction lines. Moderate spacing around directions.

## Settings Overlay Interaction

When dialogue formatting is active, the `line_spacing` value from the settings overlay controls the `speaker-gap` tag's `pixels-above-lines` value instead of the global `pixels_above_lines` / `pixels_below_lines`. Adjusting line spacing in the overlay adjusts the gap between speech blocks. The global line spacing remains at 0.

When dialogue formatting is not active (DB-lines mode or no speakers detected), `line_spacing` controls global `pixels_above_lines` / `pixels_below_lines` as before.

## Implementation

### New function: `apply_dialogue_formatting`

Location: `src/app.rs` (or a new `src/formatting.rs` if app.rs is too large)

Called after buffer text is populated in `rebuild_buffer_text`, only in text-file mode.

Steps:
1. Scan first 200 buffer lines for any `is_speaker()` match. If none, return early.
2. Set `text_view.set_pixels_above_lines(0)` and `set_pixels_below_lines(0)`.
3. Create three TextTags: `"dialogue-indent"`, `"speaker-gap"`, `"stage-direction-gap"`.
4. Iterate all buffer lines. For each line, get its text and classify:
   - `is_speaker()` → apply `"speaker-gap"` tag
   - `is_stage_direction()` → apply `"stage-direction-gap"` tag
   - `is_blank()`, `is_act_scene_marker()`, `is_separator()` → no tag (flush, natural spacing from blank lines)
   - Everything else (dialogue) → apply `"dialogue-indent"` tag
5. Store a flag on AppState (`dialogue_formatting_active: bool`) so the settings overlay knows which mode to use for `line_spacing`.

### Tags

```rust
// Created once per work load, added to buffer's tag table
let base_margin = text_view.left_margin();

let indent_tag = gtk4::TextTag::builder()
    .name("dialogue-indent")
    .left_margin(base_margin + 60)
    .build();

let speaker_gap_tag = gtk4::TextTag::builder()
    .name("speaker-gap")
    .pixels_above_lines(20)
    .build();

let stage_direction_gap_tag = gtk4::TextTag::builder()
    .name("stage-direction-gap")
    .pixels_above_lines(10)
    .build();
```

### AppState changes

Add to `AppState`:
```rust
pub dialogue_formatting_active: bool,
```

Default: `false`. Set to `true` when `apply_dialogue_formatting` finds speaker lines and applies tags.

### Settings overlay changes

In `apply_settings_change` for `LineSpacing`:
- If `dialogue_formatting_active`: update the `"speaker-gap"` tag's `pixels-above-lines` property instead of global spacing
- If not active: update global `pixels_above_lines` / `pixels_below_lines` as before

### Cleanup on work change

When a new work is loaded (`display_work`), remove the formatting tags from the tag table before repopulating. Reset `dialogue_formatting_active` to `false`.

## What This Does NOT Change

- DB-lines mode rendering — unchanged
- Dim/spotlight tag behavior — unchanged, operates independently
- Search highlighting — unchanged
- Navigation (`,`/`q` dialogue jumping) — unchanged, uses `LineMap` not formatting tags
- Font sizing — unchanged
- Column width / text margins — unchanged

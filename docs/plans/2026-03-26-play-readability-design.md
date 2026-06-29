# Play Text Readability Improvements — Design Spec

**Date:** 2026-03-26
**Status:** Draft

## Overview

Enhance play text readability by styling speaker names (small caps), stage directions (italic), and scene/act headers (bold with extra spacing). Builds on the existing dialogue formatting in `apply_dialogue_formatting()`.

## Changes

All changes are in `apply_dialogue_formatting()` in `src/app.rs`. No new files needed.

### 1. Speaker Names — Small Caps

- New `speaker-name` TextTag with `variant` set to `pango::Variant::SmallCaps`
- Fallback: if the font lacks true small caps glyphs, also set `scale` to 0.85 to reduce visual weight of ALL-CAPS text
- Applied to lines matching `is_speaker()`, combined with existing `speaker-gap` tag
- The small-caps variant is available in the pango 0.20 crate already in Cargo.toml

Detection strategy for fallback: Charter (and most fonts) lack a dedicated small-caps font file, so Pango synthesizes small caps. The synthesized result already looks smaller than full caps, so start with just the variant — add `scale` only if user reports it's still too large.

### 2. Stage Directions — Italic

- New `stage-direction-style` TextTag with `style` set to `pango::Style::Italic`
- Applied to lines matching `is_stage_direction()` (e.g., `[He exits.]`, `[Enter Sebastian.]`)
- Combined with existing `stage-direction-gap` and `dialogue-indent` tags

### 3. Scene/Act Headers — Bold with Extra Spacing

- New `act-scene-header` TextTag with:
  - `weight` set to `pango::Weight::Bold` (700)
  - `pixels_above_lines` set to 20
- Applied to lines matching `is_act_scene_marker()`
- Separator lines (`is_separator()`) also get this tag

## Implementation Details

### Tag Creation (add after existing tag creation, ~line 604)

```rust
let speaker_name_tag = gtk4::TextTag::builder()
    .name("speaker-name")
    .variant(pango::Variant::SmallCaps)
    .build();

let stage_italic_tag = gtk4::TextTag::builder()
    .name("stage-direction-style")
    .style(pango::Style::Italic)
    .build();

let act_scene_tag = gtk4::TextTag::builder()
    .name("act-scene-header")
    .weight(pango::Weight::Bold.into())
    .pixels_above_lines(20)
    .build();
```

### Tag Cleanup (add to existing cleanup loop, ~line 598)

Add `"speaker-name"`, `"stage-direction-style"`, `"act-scene-header"` to the list of tags removed on re-entry.

### Tag Application (modify existing loop, ~line 628)

```
is_speaker()          → apply speaker-gap + speaker-name
is_stage_direction()  → apply stage-direction-gap + dialogue-indent + stage-direction-style
is_act_scene_marker() → apply act-scene-header (no indent, no skip)
is_separator()        → apply act-scene-header
dialogue lines        → apply dialogue-indent (unchanged)
```

## What This Does NOT Change

- Dialogue indentation and tight spacing — unchanged
- DB-lines mode — unchanged
- Dim/spotlight, search, navigation — unchanged
- Font sizing and cycling — unchanged

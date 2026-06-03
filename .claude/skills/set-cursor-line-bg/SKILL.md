---
name: set-cursor-line-bg
description: Use when adjusting the cursor line highlight color or opacity for a theme in linux-lit — changes how prominently the current line is highlighted
argument-hint: <theme-name> <rgba-value> | <theme-name> darker | <theme-name> lighter
---

# Set Cursor Line Background

Sets the `linux-lit.cursor_line_bg` field in `~/utono/themes/.config/themes/themes-unified.json` for a specific theme. This controls the highlight color of the current line in the reader.

`cursor_line_bg` drives BOTH the static current-line highlight AND the
fade-out animation that plays on the previously-current line when the cursor
moves. The fade interpolates alpha down to 0 over its hue (see
`src/input/highlight.rs`, which reads `cursor_line_bg` via
`theme::rgba_str_to_rgb`). So changing this one field updates the highlight and
its fade together — there is no separate "fade color" field. (Historically the
fade wrongly used the window `root_color`, so it read as the theme's blue
regardless of this setting; that's fixed.)

## Format

The value is an RGBA CSS string: `rgba(R, G, B, alpha)` where alpha controls intensity.

Typical alpha range for readability:
- `0.15` — very subtle
- `0.25` — moderate
- `0.30` — visible (current rose-pine-dawn default)
- `0.40` — strong

## Steps

1. Parse the argument for theme name and desired change
2. Read `themes-unified.json`, find the theme object
3. If the theme has no `linux-lit` section, create one
4. If the user gave a specific RGBA value, use it directly
5. If the user said "darker" or "lighter", read the current value (or derive from `dwl.focuscolor`) and adjust alpha by +0.05 or -0.05
6. If no color is specified, derive from `dwl.focuscolor` with the requested alpha
7. Write the `cursor_line_bg` field
8. Run `cargo build` to verify
9. Report the change — user will restart linux-lit to see the result

## Location

The theme JSON is at:

```
~/utono/themes/.config/themes/themes-unified.json
```

The field lives under the theme's `linux-lit` key:

```json
{
  "rose-pine-dawn": {
    "linux-lit": {
      "cursor_line_bg": "rgba(196, 120, 138, 0.30)"
    }
  }
}
```

## Rust fallback

If no `linux-lit.cursor_line_bg` exists, `src/theme.rs` derives one from `dwl.focuscolor` at alpha 0.13 (light) or 0.15 (dark). Adding the JSON field overrides this.

---
name: set-cursor-offset
description: Use when adjusting the vertical cursor centering position in scroll mode, changing how far from the top the current line appears
argument-hint: <percentage>
---

# Set Cursor Offset

Sets the vertical offset percentage for `center_cursor()` in scroll mode. The percentage controls how far from the top of the viewport the current line is positioned.

- `0` = line at top edge
- `25` = line at 25% from top (quarter down)
- `50` = line at center

## Steps

1. Parse the argument as an integer percentage (0-50 range is typical)
2. Convert to decimal: e.g., `25` becomes `0.25`
3. Replace all occurrences of `adj.page_size() * <old_value>` in `src/input/navigation.rs` with `adj.page_size() * <new_value>`
4. Run `cargo build` to verify
5. Report the change

## Location

All instances are in `src/input/navigation.rs`, matching the pattern:

```rust
let offset = adj.page_size() * 0.25;
```

Use `replace_all` to update all occurrences at once.

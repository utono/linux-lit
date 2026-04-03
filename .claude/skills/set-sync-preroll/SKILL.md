---
name: set-sync-preroll
description: Use when adjusting the playback sync preroll — how many seconds early the highlight jumps to the next line during MPV audio playback
argument-hint: <seconds>
---

# Set Sync Preroll

Sets `SYNC_PREROLL` in `src/input/navigation.rs`. This constant controls how many seconds before the audio reaches a line's `start_time` the highlight advances to it.

- `0.0` = highlight moves exactly when audio reaches the line
- `0.3` = highlight moves 0.3s early (gives visual lead)
- Negative values would delay the highlight past the audio position

## Steps

1. Parse the argument as a float (e.g., `0.0`, `0.3`, `0.5`)
2. Edit `src/input/navigation.rs`, replacing the `SYNC_PREROLL` constant value
3. Run `cargo build` to verify
4. Report the change

## Location

Single constant in `src/input/navigation.rs`:

```rust
pub const SYNC_PREROLL: f64 = 0.0;
```

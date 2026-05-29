---
name: configure-nav-test
description: Use when changing what the Ctrl+Shift+T in-app navigation test harness tests — switching between sync-only, jumps-only, or full test modes
argument-hint: sync-only | jumps-only | full
---

# Configure Navigation Test Harness

Modify the `build_script()` function in `src/input/nav_test.rs` to change
which navigation patterns the Ctrl+Shift+T test exercises.

## Modes

### sync-only

Test only playback-sync page turns. The script is pure `SyncAdvance` steps
(1s interval each). This walks the cursor line-by-line through the entire
work, triggering page turns naturally when the cursor passes
`last_fully_visible_line`. Best for catching scene breaks mid-page during
sync and viewport fill issues.

```rust
fn build_script() -> Vec<Step> {
    vec![Step::SyncAdvance; 40]
}
```

### jumps-only

Test only key-press navigation (x, y, 2, 3, [, {). No sync simulation.
Fast (300ms interval for all steps).

```rust
fn build_script() -> Vec<Step> {
    let mut s = Vec::new();
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.extend_from_slice(&[Step::PageBackward; 5]);
    s.extend_from_slice(&[Step::PageForward; 3]);
    s.push(Step::NextScene); s.push(Step::PageBackward);
    s.push(Step::PrevScene); s.push(Step::PageBackward);
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.push(Step::NextChapter); s.push(Step::PageBackward);
    s.push(Step::PrevChapter); s.push(Step::PageBackward);
    s
}
```

### full (default)

Both key-press navigation and sustained sync runs (20 sync advances at 1s
each, interleaved with jump sequences at 300ms each).

```rust
fn build_script() -> Vec<Step> {
    let mut s = Vec::new();
    s.extend_from_slice(&[Step::PageForward; 5]);
    s.extend_from_slice(&SYNC_RUN);
    s.extend_from_slice(&[Step::PageBackward; 5]);
    // ... (see nav_test.rs for the full default script)
    s
}
```

## How to Apply

1. Edit `src/input/nav_test.rs`
2. Replace the body of `build_script()` with the desired mode
3. Optionally adjust `MAX_STEPS` (default 500) — for sync-only with 40-step
   script, 500 steps = ~12 full passes through the work
4. `cargo build`
5. In the app: press `gg` to start from the beginning, then `Ctrl+Shift+T`

## Invariants Checked (all modes)

All 6 invariants run after every step regardless of mode:

- Forward progress on x (PageForward only)
- y round-trips x / structural jump return (PageBackward only)
- No scene break mid-page (always)
- Viewport fill > 10% (always)
- current_line is dialogue (always, plays only)

## Timing

- `Step::SyncAdvance` — 1000ms delay (simulates real playback pacing)
- All other steps — 300ms delay

## Key Files

- `src/input/nav_test.rs` — `build_script()`, `Step` enum, `MAX_STEPS`

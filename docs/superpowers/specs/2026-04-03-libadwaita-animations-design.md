# Migrate animations from manual tick callbacks to adw::TimedAnimation

**Date:** 2026-04-03

## Summary

Replace all three manual `add_tick_callback` animation sites with `adw::TimedAnimation` from libadwaita. Shorter durations, proper easing curves, automatic cleanup, and cancellation support.

## Dependency

Add `libadwaita` 0.7 (matches gtk4 0.9) to `Cargo.toml`. Call `adw::init()` at startup in `main.rs`. No need to switch window type to `adw::ApplicationWindow`.

## Animation 1 — Page-turn crossfade

- **Current:** Manual tick callback, 650ms linear, dual opacity (snapshot fades out + content fades in), causes semi-transparent ghosting.
- **New:** Fade-in-only approach. Live content stays at full opacity. Snapshot placed on top, faded out via `adw::TimedAnimation` (opacity 1.0 → 0.0, 250ms, `EaseOutCubic`). Completion callback removes snapshot from overlay.
- **Files:** `src/input/navigation.rs` (lines ~641-681)

## Animation 2 — Page-turn slide

- **Current:** Manual tick callback with linear interpolation for slide offset and opacity.
- **New:** `adw::TimedAnimation` (250ms, `EaseOutCubic`) driving the snapshot's horizontal translate via `connect_value_notify`. Snapshot slides out in page-turn direction while new content is revealed underneath. Completion callback removes snapshot.
- **Files:** `src/input/navigation.rs` (lines ~683+)

## Animation 3 — Cursor highlight fade

- **Current:** Manual tick callback, 500ms linear, animates `paragraph_background_rgba` alpha on a `TextTag`.
- **New:** `adw::TimedAnimation` (150ms, `EaseOutCubic`, value 1.0 → 0.0). `connect_value_notify` sets the tag's RGBA alpha. Completion callback removes tag from buffer. Not a direct `PropertyAnimationTarget` since `TextTag` isn't a widget property — callback-driven.
- **Files:** `src/input/navigation.rs` (lines ~922-994)

## Cancellation

Store animation handles (`adw::TimedAnimation`) in `AppState`:
- `page_turn_anim: Option<adw::TimedAnimation>` — for crossfade and slide
- `cursor_fade_anim: Option<adw::TimedAnimation>` — for cursor highlight

When a new animation fires mid-flight, call `.skip()` on the existing handle (jumps to end state, triggers completion cleanup) before starting the new one.

## Constants removed

- `CROSSFADE_MS` (650ms) — replaced by 250ms duration on TimedAnimation
- `HIGHLIGHT_FADE_MS` (500ms) — replaced by 150ms duration on TimedAnimation

## What stays the same

- Snapshot capture approach (`capture_page_snapshot`) — texture path with WidgetPaintable fallback
- `page_turn_overlay` Overlay widget structure
- `cursor_fade_tag` TextTag and its role in highlight system
- `TransitionStyle` enum and settings overlay UI
- `Instant` transition style (no animation, unchanged)

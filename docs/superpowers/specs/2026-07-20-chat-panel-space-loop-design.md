# Chat-Panel Space Loop — Design

**Date:** 2026-07-20
**Status:** Approved (brainstorming session)

## Summary

Pressing `space` while the chat transcript has focus
(`InputMode::ChatTranscript`) loops audio playback of the displayed entry's
*source passage* — the lines the gloss or journal entry was written about.
The entry may belong to a different work than the one loaded in the main card
(e.g. main card on TGV-Ambrose, panel pinned to a BH-Barrett gloss); in that
case the loop plays BH-Barrett's default media. The main card's MPV instance,
cursor, pagination, and sync state are never touched: the loop runs on a
**dedicated chat-panel MPV process**.

## Background: how the cross-work case arises

The panel's gloss/journal lists are populated only from
`ChatState.pinned_passage`, captured from the work loaded at pin time, and
**every main-card work switch wipes the panel** (`chat::on_work_switched`,
called from `display_work` at `src/app/mod.rs:3408`, resets the whole
`ChatState`). That wipe is deliberate and stays. So today the panel can only
ever show the current work; the cross-work case arrives with a planned future
workflow — an `f` finder inside the chat panel that surfaces gloss/journal
entries belonging to *other* works. This feature must be ready for it: the
handler resolves the source work from the entry itself, **never** from
`AppState.current_work`, so it is correct now (same-work by construction) and
needs no change when the cross-work finder lands.

Proven precedents combined by this feature:

- `echoes.rs::play_selected_echo` — resolves and plays a *different* work's
  media without mutating the main card (`line_id_for_location` →
  `list_media_for_work` → `line_start_time` → loadfile).
- `echoes.rs::play_source_turn` — MPV a–b loop over a passage.
- `vocab_loop` — `space` toggles pause inside a modal loop.

## Architecture: thin write-only chat player

New module `src/mpv/chat_player.rs`. It reuses `discovery.rs::launch_mpv`
to spawn a chat-owned MPV process and speaks fire-and-forget IPC only:

- **Arm:** a single `loadfile <path> replace start=A,ab-loop-a=A,ab-loop-b=B,pause=no`
  command (mpv applies per-file options atomically — no pending-seek dance).
- **Pause toggle:** `set_property pause <bool>` (or `cycle pause`).
- **Stop loop:** set `ab-loop-a`/`ab-loop-b` to `no`, `pause` to `true`.
- **Teardown:** `quit`.

No event loop, no `TimePos` handling, no channel into the app's event
bridge — structurally zero chance of polluting the main cursor-sync engine.
`src/mpv/client.rs` is untouched.

Rejected alternatives: (A) a second instance of `client::run` — its
timestamp-table/sync machinery would need per-instance neutering and its
events would flow into the main bridge; (C) generalizing `client.rs` into a
multi-instance abstraction — over-engineering for one consumer.

### Socket & process rules

- Socket: `/tmp/mpvsocket-{infix}chat-{author}-{basename}` where `infix` is
  the existing instance-slot prefix (`""` or `i{n}-`), so parallel app
  instances never share a chat player.
- `Stdio::null()` on stdin/stdout/stderr of the spawned process (standing
  gotcha: an inherited-stdio child keeps `crll`'s tee pipe open on exit).
- Spawned lazily on the first `space` arm; persists across loops for reuse.
- **Quit on:** chat panel close (Tab / any path that leaves the panel
  destroyed) and app exit. Panel close = process quit, not just loop stop.

## Source resolution

At `space` time, resolve in this order; any unresolvable step is a **no-op
with a chapter-toast** stating the reason:

1. **Entry → work + line range.** Every panel view (Gloss, Journal,
   Question) displays content about the SAME pinned passage, so resolution
   is uniform:
   - `ChatState.gloss_ctx` present: `gloss_ctx.work_abbrev` +
     `gloss_ctx.act`/`gloss_ctx.scene` (the passage's `div1`/`div2`) +
     `gloss_ctx.source_line_numbers` (per-division line numbers,
     `line_in_div` — NOT `line_mapping.id`s; first/last bound the passage).
     These are resolved to global `line_mapping.id`s at space-time via
     `line_id_for_location(conn, abbrev, div1, div2, line_in_div)` — the
     `echoes::play_selected_echo` precedent — because `line_mapping.id` is a
     global autoincrement and a `line_in_div` can never match it directly.
     This is the entry's own identity — the future cross-work finder installs
     a `gloss_ctx` for whatever it loads.
   - No `gloss_ctx` but a raw `pinned_passage` (pinned, not yet glossed):
     `div1`/`div2` + first/last `line_in_div` from
     `pinned_passage.cursor_lines`, work from `current_work` — safe here
     because a raw pin is same-work by construction (the work-switch wipe
     guarantees it). Resolved to ids the same way.
   - Neither, or empty line lists (e.g. future author-scope notes with no
     citations): toast "No source passage to play".
2. **Work → media.** `list_media_for_work(conn, abbrev)`; prefer a path
   containing `/aax-Arkangel/`, else the first item (the
   `play_selected_echo` rule). No media → toast. No media picker — the
   "default media" is always auto-selected.
3. **Range → loop points.** `a = line_start_time(first_line, media_id)`.
   `b` needs a **new standalone query** `line_end_time(conn, line_id,
   media_id)` mirroring `line_start_time` against `line_timestamps.end_time`
   (no such reader exists today). Fallback chain when the last line's
   end_time is NULL: the following line's `start_time` → play once from `a`
   with no b-point (toast notes the loop is unavailable). No `a` at all →
   toast "no timestamps for this passage".

## Loop state machine

New `ChatLoopState` on `ChatState`:

```rust
struct ChatLoopState {
    armed: Option<EntryKey>,   // identifies the entry the loop was armed on
    paused: bool,
    main_was_playing: bool,    // main MPV state captured at arm time
}
```

- **`space`, not armed:** resolve (above); pause the main MPV, recording
  `main_was_playing`; spawn/reuse the chat player; arm + play.
- **`space`, armed:** toggle pause on the chat player only.
- **Entry navigation while armed** (stepping glosses/journal entries):
  stop the loop (clear ab-loop, pause chat player, `armed = None`). The main
  MPV **stays paused** — the user is likely about to `space` the next entry.
  `main_was_playing` is preserved across nav-stops so a later full exit still
  restores correctly.
- **`Escape` in the transcript while armed:** full teardown — stop the loop
  and resume the main MPV iff `main_was_playing` (runs *before* Escape's
  normal leave-panel behavior).
- **Panel close / leaving `ChatTranscript`:** full teardown **plus `quit`**
  to the chat MPV process; resume main iff `main_was_playing`.

The main MPV is paused for the duration of any loop, so its sync engine is
naturally quiescent: no `suppress_sync_until` timers, no `SetTimestamps`
swaps, no gating changes needed.

## Keybind bookkeeping

- `space` arm added to `handle_chat_transcript_key`
  (`src/input/keymap.rs:1500`) — currently falls through to the swallow-all
  arm, so the slot is free. Chat transcript keys are hardcoded in the
  handler, so **no** `keymap_config.rs` / `keymap.json` change.
- The chat panel's own Ctrl+/ legend (`ChatKeybindsOverlay`) gets the
  `space` entry (loop source audio / pause). The main-card Ctrl+/ overlay is
  untouched (it shows main-card binds only).

## Testing

- **Unit (`cargo test --bins`):** source-resolution order and fallbacks
  (gloss vs journal identity, missing range, media preference rule, loop
  point fallback chain), and the `ChatLoopState` transitions (arm → pause
  toggle → nav-stop → exit restore, incl. `main_was_playing` preservation).
- **New DB query:** `line_end_time` unit-tested alongside `line_start_time`.
- **Manual acceptance** (audio can't be verified under the `LIT_NO_MPV`
  headless harness): pin a passage and gloss it; `space` in the panel —
  hear the passage looping on a SECOND mpv process while the main card's
  player is paused with its position intact; `space` toggles pause;
  navigate entries — loop stops, main stays paused; Escape — main resumes
  iff it was playing; close the panel — the chat MPV process is gone
  (`pgrep`-verifiable, socket `/tmp/mpvsocket-chat-*`). Cross-work play
  becomes observable only when the future `f` finder lands.

## Out of scope (explicitly deferred)

- The `f` cross-work gloss/journal finder itself (the future workflow this
  feature's resolution rule is forward-compatible with).
- Preserving the pinned panel across main-card work switches (explicitly
  rejected: every work switch keeps wiping the panel).
- Highlighting/karaoke of the looping line inside the chat panel.
- Loop-follows-navigation audition mode.
- Author-scope notes in the chat panel (if ever added, they hit the defined
  no-op toast).
- A media picker for the loop (default media only).

# MPV-side reader control for the lit instance

Date: 2026-08-03

## Goal

In the MPV window linux-lit launches, six keys do exactly what they do in the
reader. MPV becomes a remote control for the reader: navigate and timestamp
without leaving the player window. Normal (non-lit) MPV usage is untouched.

**Navigation** — moves the reader cursor *and* seeks MPV. Always active:

- `,` — previous speaker turn
- `q` — next speaker turn
- `[` — previous division (scene/chapter boundary)
- `{` — next division

**Timestamp authoring** — writes to lit.db. Only when playback sync is on:

- `b` — set start time: writes MPV's current playback position (−0.30s) to the
  line under the reader cursor
- `B` — undo the last timestamp write

All six are the RPD glyphs the keys actually emit; `[` and `{` are the
unshifted QWERTY-`2`/`3` caps, matching the unshifted-symbol pattern of `,`
and `q`.

## Non-goals

- Changing what these keys do in the reader itself. The reader's binds are
  unchanged; this only adds a second surface that reaches the same handlers.
- Changing `~/.config/mpv/input.conf`. That file is shared with ordinary video
  watching and is not modified.
- Adding binds to the chat snippet player (see Scoping).

## Architecture

Three pieces, each with one job.

### 1. A lit-owned input.conf

A small file shipped in the linux-lit repo, `assets/mpv-input.conf`:

```
, script-message lit-prev-speaker
q script-message lit-next-speaker
[ script-message lit-prev-division
{ script-message lit-next-division
b script-message lit-set-start-time
B script-message lit-undo-timestamp
```

`discovery.rs` adds `--input-conf=<path>` to the launch args. MPV merges this
*over* the user's `~/.config/mpv/input.conf`, so everything else keeps working
in the lit window: `a` pause, `o`/`e` (±2s) and `O`/`E` (±15s) seek,
`DEL`/`Ctrl+L` quit, and every lua script. Only these six lines are
overridden, and only in the lit instance.

No lua script is required, and there is no branching on window app-id. The
override ships with linux-lit, so it cannot drift out of sync with the reader.

`B` is bound as the shifted glyph with no separate modifier, which matches how
linux-lit binds it (`KeyCombo::plain("B")`, `keymap_config.rs:522`).

### 2. Return path over the existing IPC socket

`script-message` makes MPV emit a `client-message` event on its IPC socket:

```json
{"event":"client-message","args":["lit-set-start-time"]}
```

linux-lit's read loop in `src/mpv/client.rs` already reads that socket
line-by-line and parses events. This adds one `parse_client_message` alongside
the existing `parse_time_pos` / `parse_pause_state`, plus a new
`MpvEvent::ReaderAction(...)` variant carrying which of the six was pressed,
sent over the existing channel to the GTK thread.

No new transport, no new socket, no polling.

### 3. Dispatch to the existing handlers

In `src/main.rs`, the new event arm calls the very same functions `keymap.rs`
calls for the reader keys:

- `navigation::jump_to_prev_speaker` / `jump_to_next_speaker`
  (`keymap.rs:4450-4451`)
- `navigation::jump_to_prev_section` / `jump_to_next_section`
  (`keymap.rs:4454-4455`)
- `timestamps::set_start_time` (`keymap.rs:4793`) — behind the sync gate
- `timestamps::undo_timestamp` (`keymap.rs:4819`) — behind the sync gate

Parity is structural rather than duplicated: if the reader's navigation or
timestamp behavior changes, the MPV path follows automatically. Nothing in
`navigation.rs` or `timestamps.rs` is modified.

## The sync gate

`b` and `B` from MPV run only when `state.sync_enabled`
(`src/app/mod.rs:773`) is true. When sync is off the message is **silently
ignored**: logged for debugging, with no toast, no OSD, and no write.

The gate lives in the MPV dispatch arm in `main.rs`, **not** in
`timestamps.rs`. This asymmetry is deliberate. The reader's own `b`/`B` stay
unconditional, exactly as today — in the reader you can see the cursor, so an
unsynced `b` is a deliberate act. From the MPV window the cursor is invisible,
and sync-on is what makes its position predictable enough to write to blind.

## Why `b` fits the MPV window

`set_start_time` (`timestamps.rs:132`) reads the timestamp *value* from
`state.current_time_pos` — MPV's own playback position, which is exactly what
the MPV window displays. The workflow is coherent: hear the line begin, press
`b`, and that moment is written. `capture_undo_snapshot`
(`timestamps.rs:158`) means `B` reverses a mistake from the same window.

The target *line* is the reader's cursor, which the MPV window cannot show.
The four navigation binds put the cursor where it belongs first, the sync gate
keeps it tracking playback, and `B` covers errors.

## Data flow

```
key `b` in the mpv window
  -> mpv input.conf: script-message lit-set-start-time
  -> client-message event on /tmp/mpvsocket-...
  -> client.rs read loop parses it
  -> MpvEvent::ReaderAction(SetStartTime) over the existing channel
  -> main.rs: sync_enabled?
       no  -> log, drop
       yes -> timestamps::set_start_time(&mut state)
  -> current playback position written to lit.db for the cursor's line
```

## Scoping: reading player only

linux-lit launches two distinct MPV processes:

- **The reading player** — `discovery.rs:162` `launch_mpv()`, socket
  `/tmp/mpvsocket-{author}-{basename}`. The audiobook read along with.
- **The chat snippet player** — `chat_player.rs:252`, which passes a
  `chat-`-marked socket to the shared `launch_mpv_at()`. A throwaway player
  for the chat space-loop snippet.

`--input-conf` is added in `launch_mpv()` only, not in the shared
`launch_mpv_at()`. The chat player keeps stock MPV keys.

Rationale: the chat player is a snippet player with no reader cursor of its
own. Pressing `q` in its window would move the main reader's cursor mid-chat,
and `b` would write the *snippet's* playback position against whatever line
the reader cursor happened to be on — a wrong number in lit.db.

## What the lit MPV window gives up

- **`q` no longer quits.** `DEL` and `Ctrl+L` remain bound to quit
  (`~/.config/mpv/input.conf` lines 60-61).
- **No ±5s seek.** `[`/`{` were `no-osd seek ∓5 exact`. `o`/`e` (±2s) and
  `O`/`E` (±15s) remain; there is no 5s step in the lit window.
- **No video-rotate on `b`** — irrelevant for audiobooks.
- `,` playlist-prev and the chapter-control meanings of `q`, `[`, `{` are
  shadowed in the lit window only.

## Error handling

- No speaker timestamps or no division metadata: the handlers already no-op
  safely, the same as pressing the key in the reader.
- Sync off: `b`/`B` silently dropped and logged.
- `b` with no `media_id`, no work line, or on an unspoken stage direction:
  `set_start_time` / `timestamp_writable` already log and return `false`
  (`timestamps.rs:54-69`, `132-154`). No partial write.
- `B` with nothing to undo: `undo_timestamp` handles the empty-snapshot case.
- Unrecognized `client-message` args are ignored (the parser returns `None`),
  so other scripts' messages on that socket are harmless.

## Testing

- Unit-test `parse_client_message` against the exact JSON MPV emits: all six
  messages plus a negative case. Pure function, runs under
  `cargo test --bins`.
- Unit-test the sync gate: `ReaderAction(SetStartTime)` with
  `sync_enabled = false` performs no write.
- The `b`/`B` path touches lit.db; the existing `debug-timestamp-bind` skill
  covers verifying a timestamp write landed.
- The end-to-end path (a real keypress in a real MPV window) is live-only:
  verify manually by pressing each of the six keys and watching the reader.

## Keybind surface obligations

Per `CLAUDE.md`, keybind changes update every mirror in the same change. This
change adds no reader-surface binds and moves none, so `keymap_config.rs`, the
Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`), and the stowed `keymap.json` are
**unchanged**. The new surface is the MPV window, whose binds live in
`assets/mpv-input.conf` and are documented here.

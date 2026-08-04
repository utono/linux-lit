# MPV binds that emulate linux-lit binds

The MPV window linux-lit launches is not just a player — six of its keys drive
the reader. Press `q` in the MPV window and the reader's cursor jumps to the
next speaker turn, exactly as if you had pressed `q` in the reader itself. MPV
becomes a remote control, so you can navigate and timestamp without leaving the
player.

This guide explains which keys are emulated, how the emulation works, and the
handful of MPV behaviors it displaces.

Design spec:
`docs/superpowers/specs/2026-08-03-mpv-reader-binds-design.md`.

## The six binds

Four navigate. They move the reader's cursor **and** seek MPV, the same as the
reader's own keys:

- `,` — previous speaker turn
- `q` — next speaker turn
- `[` — previous division (scene/chapter boundary)
- `{` — next division

Two author timestamps, writing to lit.db:

- `b` — set start time: writes MPV's current playback position (minus 0.30s)
  to the line under the reader's cursor
- `B` — undo the last timestamp write

These are the RPD glyphs the keys actually emit. On that layout `[` and `{` are
the unshifted QWERTY-`2`/`3` caps, which is why they sit alongside `,` and `q`
as unshifted symbol keys rather than needing Shift.

## Why `b` fits a player window

`set_start_time` reads the timestamp *value* from the current playback
position — precisely what the MPV window is showing you. The workflow is
therefore coherent: hear a line begin, press `b`, and that moment is written.

What it writes *to* is the reader's cursor line, which the MPV window cannot
show. The four navigation binds put the cursor where it belongs first, playback
sync keeps it tracking the audio, and `B` reverses a mistake without switching
windows.

## The sync gate

`b` and `B` from the MPV window run **only when playback sync is on**. With
sync off, the keypress is silently ignored — logged, but no toast, no OSD, and
no database write.

The reader's own `b` and `B` have no such gate; they work whether or not sync
is on. The asymmetry is deliberate. In the reader you can see the cursor, so an
unsynced `b` is a deliberate act. From the MPV window the cursor is invisible,
and sync-on is what makes its position predictable enough to write to blind.

Accordingly the gate lives in the MPV dispatch arm in `src/main.rs`, **not** in
`src/input/timestamps.rs`. Anyone adding a gate to `timestamps.rs` would change
the reader's behavior too, which is not the intent.

## How the emulation works

MPV cannot call into linux-lit. The bridge reuses the IPC socket linux-lit
already reads, so there is no new transport, no second socket, and no polling.

**1. A lit-owned input.conf.** `assets/mpv-input.conf` ships in this repo and
binds the six keys to `script-message` commands:

```
, script-message lit-prev-speaker
q script-message lit-next-speaker
[ script-message lit-prev-division
{ script-message lit-next-division
b script-message lit-set-start-time
B script-message lit-undo-timestamp
```

`src/mpv/discovery.rs` passes it as `--input-conf` at launch. MPV merges it
*over* `~/.config/mpv/input.conf`, so every other bind in your personal config
still works in that window — `a` pause, `o`/`e` and `O`/`E` seek, `DEL` and
`Ctrl+L` quit, and all your lua scripts. Only these six lines are overridden.

**2. The return trip.** `script-message` makes MPV emit a `client-message`
event on its IPC socket. Captured from a live MPV, the six payloads are exactly:

```
{"event":"client-message","args":["lit-prev-speaker"]}
{"event":"client-message","args":["lit-next-speaker"]}
{"event":"client-message","args":["lit-prev-division"]}
{"event":"client-message","args":["lit-next-division"]}
{"event":"client-message","args":["lit-set-start-time"]}
{"event":"client-message","args":["lit-undo-timestamp"]}
```

`parse_client_message` in `src/mpv/client.rs` parses these beside the existing
`parse_time_pos` and `parse_pause_state`, and sends an
`MpvEvent::ReaderAction(..)` over the channel the reader already listens on.
Unknown message names return `None`, so other scripts sharing that socket are
ignored.

**3. Dispatch.** `src/main.rs` matches the event and calls the *same functions*
`src/input/keymap.rs` calls for the reader's own keys —
`jump_to_prev_speaker` / `jump_to_next_speaker`, `jump_to_prev_section` /
`jump_to_next_section`, `set_start_time`, and `undo_timestamp`.

That last point is the load-bearing one. Parity is **structural, not
duplicated**: the MPV path calls the reader's handlers rather than
reimplementing them, so changing what `q` does in the reader automatically
changes what `q` does in MPV. There is no second copy to keep in sync.

## Reading player only

linux-lit launches two MPV processes, and only one gets these binds.

The **reading player** (`launch_mpv` in `src/mpv/discovery.rs`, socket
`/tmp/mpvsocket-{author}-{basename}`) is the audiobook you read along with. It
gets `--input-conf`.

The **chat snippet player** (`src/mpv/chat_player.rs`, a `chat-`-marked socket)
plays back short passages for the chat feature. It does **not**.

The two share `launch_mpv_at`, which takes an explicit `reader_binds: bool` so
the distinction is visible at each call site. The exclusion matters: pressing
`q` in the snippet window would move the main reader's cursor mid-chat, and `b`
would write the *snippet's* playback position against whatever line the cursor
happened to be on — a wrong number in lit.db.

## What these binds displace

Three MPV behaviors change in the reading player's window:

- **`q` no longer quits.** Use `DEL` or `Ctrl+L`, both already bound to quit in
  `~/.config/mpv/input.conf`.
- **No ±5s seek.** `[` and `{` were `seek ∓5 exact`. `o`/`e` (±2s) and `O`/`E`
  (±15s) remain, so there is no 5-second step in this window.
- **No video-rotate on `b`** — irrelevant for audiobooks.

Also shadowed, in this window only: `,` playlist-prev, and the chapter-control
meanings of `q`, `[`, and `{`.

Your own `~/.config/mpv/input.conf` is never modified. Ordinary video watching
outside linux-lit is completely unaffected.

## What happens when things go wrong

Every failure path is quiet and non-destructive:

- No speaker timestamps or no division metadata — the handlers no-op, the same
  as pressing the key in the reader.
- Sync off — `b`/`B` dropped and logged.
- `b` with no media, no work line, or on an unspoken stage direction —
  `timestamp_writable` refuses and logs. No partial write.
- `B` with nothing to undo — handled by the empty-snapshot case.
- `assets/mpv-input.conf` missing — MPV simply launches with your own binds, as
  it did before this feature existed. Not an error.

## Verifying it works

The log records both the launch and every keypress:

```bash
rg 'MPV_BIND|MPV: reader binds' linux-lit-dev.log
```

`MPV: reader binds via <path>` appears once at launch and confirms the conf was
found. Each subsequent keypress logs `MPV_BIND: <Action> from mpv window`, or
`MPV_BIND: <Action> ignored — sync off` when the gate rejects `b`/`B`.

A running instance predates any rebuild, so restart the reader before testing a
change.

To confirm MPV itself parses the conf and emits the right events without
involving the reader at all, run a throwaway instance and drive it over IPC:

```bash
mpv --input-conf=assets/mpv-input.conf --idle --no-terminal \
  --input-ipc-server=/tmp/lit-bindtest.sock --vo=null --ao=null &
sleep 3
(printf '{"command":["keypress","q"]}\n'; sleep 3) \
  | socat - UNIX-CONNECT:/tmp/lit-bindtest.sock
```

Pressing `q` prints `{"event":"client-message","args":["lit-next-speaker"]}`.
Clean up in a **separate** command, so a non-zero exit cannot abort it:

```bash
pkill -f 'lit-bindtest' ; command rm -f /tmp/lit-bindtest.sock
```

That pattern matches only the throwaway instance's distinctive socket name. Do
not broaden it to `pkill -f mpv`, which would kill the live reading player too.
Expect exit status 144 from `pkill` here — it matches its own shell's process
group, so anything chained after it with `&&` silently never runs.

The socket must stay open for a few seconds after the keypress: the
`client-message` event arrives asynchronously, after the command's own
`"error":"success"` reply, and a client that disconnects immediately never sees
it.

## Adding or changing a bind

Two files must agree, and a unit test enforces it:

1. Add the line to `assets/mpv-input.conf`.
2. Add the message name and its `ReaderAction` variant to
   `parse_client_message` in `src/mpv/client.rs`, the enum in
   `src/mpv/commands.rs`, and the dispatch arm in `src/main.rs`.

`test_shipped_input_conf_matches_parser` reads the shipped conf and asserts
every `script-message` name in it parses, so a typo on either side fails the
build rather than silently dead-keying that bind. It also pins the count, so a
new bind requires updating the expected total deliberately.

Note that `MpvEvent`'s match in `main.rs` is exhaustive: adding a variant
without its dispatch arm is a compile error (`E0004`), not a silent no-op. The
`#[allow(dead_code)]` on that enum silences unused *variants*, not incomplete
matches.

**These binds are not reader keybinds.** They live entirely in
`assets/mpv-input.conf`, so changing them does **not** touch
`src/input/keymap_config.rs`, the Ctrl+/ overlay in
`src/ui/keybinds_overlay.rs`, or the stowed `keymap.json` — the three mirrors
that `CLAUDE.md` requires updating for reader binds. Conversely, moving a
*reader* bind does not automatically move its MPV counterpart: the handler
stays shared, but the key that reaches it is set here.

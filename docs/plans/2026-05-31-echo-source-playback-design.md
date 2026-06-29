# Echo / source playback in the echoes overlay

**Date:** 2026-05-31
**Status:** Approved design, pending implementation plan

## Problem

In the echoes overlay, the user wants to audition echoes without leaving the
source turn's reader view:

- **`a`** (currently unbound) — play the selected echo's audio in the existing
  MPV instance, without opening (displaying) the echo's work. Re-pressing `a` on
  the same echo pauses/resumes it. The source-turn loop stays armed so it can be
  restored.
- **`Tab`** (currently `toggle_echo_playback`) — reload the source-turn media,
  re-arm the source AB-loop, and play from the source turn's first line.
- **`Escape`** — unchanged: close the overlay (its current behavior).

## Current behavior (verified in source)

- `handle_echoes_overlay_key` (`src/input/keymap.rs`): `Tab` → `toggle_echo_playback`;
  `Return` → `jump_to_selected_echo` (opens the echo's work); `Escape` → hide +
  clear `echo_overlay_*` + clear AB-loop + return to Reader; `n`/`p` → move
  selection. `a` is unbound (falls through `_ => true`).
- `toggle_echo_playback` (`src/input/actions/echoes.rs`): when on the turn's work,
  resolves the turn's `(a, b)` timestamps from `echo_session.turn_key` and either
  pauses (if playing) or sets an AB-loop + seeks + plays. This is what `Tab` does
  today.
- `switch_mpv_to_current_line` (`echoes.rs`): picks the loaded work's
  `/aax-Arkangel/` media from `current_work.media_paths`/`media_ids` and seeks to
  a line's `timestamp.start - SEEK_PREROLL`.
- MPV commands (`src/mpv/commands.rs`): `LoadFileAndSeek(path, t)`,
  `LoadFileSeekPaused(path, t)`, `SetAbLoop{a, b}`, `ClearAbLoop`, `Seek(f64)`,
  `TogglePause`.
- `StoredEchoLink` (`src/db/queries.rs`): `link_id`, `echo_work_abbrev`,
  `echo_div1`, `echo_div2`, `echo_start_line`, `echo_text`, … — **no timestamp**.
- `line_id_for_location(conn, work_abbrev, div1, div2, line_in_div) -> Option<i64>`
  exists. `list_media_for_work(conn, abbrev) -> Vec<MediaItem>` exists
  (`MediaItem { media_id, path, display_name, priority }`).
- There is **no** single-line timestamp lookup; `line_timestamps` holds
  `start_time` keyed by `line_mapping_id` + `media_id`.
- `AbRepeatState` (`src/ab_repeat.rs`): `a_time`, `b_time`, `loop_active`, … .
- `EchoSession.turn_key: EchoTurnKey` carries the source turn's
  `work_abbrev`/`div1`/`div2`/`start_line`/`end_line`.
- Constants in `echoes.rs`: `TURN_PREROLL = 0.5`; `navigation::SEEK_PREROLL`.

## Design

### New DB query

Add to `src/db/queries.rs`:

```rust
/// Look up a single line's start time for a given media file.
pub fn line_start_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT start_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}
```

### New AppState field

Track which echo is currently playing, for the pause/resume toggle:

```rust
// src/app.rs, AppState
pub echo_playing_link: Option<i64>, // link_id of the echo currently playing via `a`
```

Initialize to `None` in the AppState constructor. Reset to `None` whenever the
overlay closes (Escape handler) and when `Tab` restores source playback.

### `a` → `play_selected_echo` (new fn in `echoes.rs`)

```
pub(crate) fn play_selected_echo(state_rc, tokio_handle):
  link = current selected StoredEchoLink (echo_overlay_links[echo_overlay_index]); else return
  if state.echo_playing_link == Some(link.link_id):
      send TogglePause; log "ECHOES: toggled echo playback"; return   # pause/resume same echo
  # New echo:
  open_db; line_id = line_id_for_location(conn, link.echo_work_abbrev, div1, div2, echo_start_line)
  media = first list_media_for_work(conn, echo_work_abbrev) path containing "/aax-Arkangel/"
          (fallback: highest-priority media; if none, toast "No media for echo" and return)
  start = line_start_time(conn, line_id, media.media_id); if None -> toast + return
  seek = (start - navigation::SEEK_PREROLL).max(0.0)
  send ClearAbLoop           # don't loop the source turn while auditioning the echo
  send LoadFileAndSeek(media.path, seek)
  state.ab_repeat.loop_active = false   # reflect that the turn loop is no longer the active MPV loop
  state.echo_playing_link = Some(link.link_id)
  state.suppress_sync_until = now + 500ms
  log "ECHOES: playing echo <abbrev> line_id=<id> @<seek>"
```

Notes:
- The DB work runs on the Tokio handle the same way other echo DB access does
  (`open_db` is synchronous SQLite; mirror the existing pattern — these lookups
  are cheap point queries, run inline like `jump_to_selected_echo`'s
  `line_id_for_location` call, which is synchronous).
- **Source-turn state preserved:** do NOT clear `echo_session` or
  `ab_repeat.a_time`/`b_time`. Only `loop_active` is set false (the loop is no
  longer the live MPV loop, but the remembered `(a, b)` stays so `Tab` can re-arm
  it). The reader display is untouched (still the source work).

### `Tab` → `play_source_turn` (rewrite of `toggle_echo_playback`)

Rename/repurpose `toggle_echo_playback` to `play_source_turn`. Always restore the
source turn (no pause-toggle — the user's answer was "re-arm the turn loop"):

During the overlay `current_work` is the **source work** (the overlay opens from
the displayed work; no `display_work` runs). Today's `Tab` uses `SetAbLoop`/`Seek`
with **no `LoadFile`**, relying on the source media already being the loaded MPV
file. Because the new `a` swaps MPV to the *echo's* media, `Tab` must reload the
source media first.

```
pub(crate) fn play_source_turn(state_rc):
  resolve (a, b) from echo_session.turn_key against current_work (existing logic
    in toggle_echo_playback); if no range -> plain TogglePause + log; return
  # Pick the source work's Arkangel media (same pattern as
  # switch_mpv_to_current_line: current_work.media_paths.zip(media_ids)
  # .find(|p| p.contains("/aax-Arkangel/"))); fall back to first media path.
  source_media = <Arkangel path of current_work> ; if none -> SetAbLoop+Seek only (media already loaded)
  loop_a = (a - TURN_PREROLL).max(0.0)
  send SetAbLoop { a: loop_a, b }
  send LoadFileAndSeek(source_media, loop_a)   # reload source media (a may have swapped MPV away)
  # LoadFileAndSeek resumes playback; no separate TogglePause needed.
  state.ab_repeat.a_time = Some(a); b_time = Some(b); loop_active = true
  state.echo_playing_link = None
  state.suppress_sync_until = now + 500ms
  log "ECHOES: re-armed source turn loop [loop_a, b]"
```

Note on AB-loop + LoadFile ordering: send `SetAbLoop` before `LoadFileAndSeek` so
the loop is set when the file loads, matching how `toggle_echo_playback` sets the
loop then seeks. The plan should verify the MPV client applies an AB-loop set
before a `loadfile` (if not, send `SetAbLoop` after the load completes via the
same deferred mechanism the codebase uses for post-loadfile seeks — see
`pending_loadfile_seek` in `CLAUDE.md`).

### `Escape` — unchanged behavior, plus reset

Keep the current Escape handler (hide, clear `echo_overlay_*`, clear AB-loop,
return to Reader). Add `s.echo_playing_link = None` to its cleanup block.

### Wiring (`handle_echoes_overlay_key`)

- Add arm: `"a" => { play_selected_echo(state, tokio_handle); true }`.
- Change `"Tab"` arm to call `play_source_turn(state)`.
- `Escape` arm: add `s.echo_playing_link = None`.

### Footer hint

Update the echoes hint text (`show_echoes` in `gloss_overlay.rs`) to reflect the
new binds, e.g.: `Esc close · a play echo · Tab play turn · n/p select · Enter
open work · c copy · s curate · R refresh`.

## Error handling

- Missing echo line location, missing media, or missing timestamp → log + a
  toast via `gloss_overlay.show("…", "")` (matching `jump_to_selected_echo`'s
  failure path), and return without changing playback.
- MPV not connected: the `cmd_tx.try_send` calls are best-effort (same as
  existing code); if disconnected they no-op. No special handling beyond what the
  current echo playback does.

## Out of scope

- Auto-advancing to the next echo when one finishes.
- Looping the echo line (the user chose pause/resume toggle, not loop).
- Changing `Return` (open work) or `R` (refresh) behavior.
- Any change to reader-mode playback.

## Testing

- Unit: `line_start_time` returns the stored start for a (line, media) pair and
  `None` when absent — testable against a temp SQLite db if the suite has a
  fixture pattern; otherwise covered by manual verification (the echo DB layer is
  not currently unit-tested).
- Manual (user runs `cargo run`):
  - `a` on an echo → its media loads and plays from the echo line; reader still
    shows the source turn; overlay stays open.
  - `a` again on the same echo → pauses; `a` again → resumes.
  - `a` on a different echo → switches to that echo's media/line.
  - `Tab` → source-turn media reloads, plays from the turn's first line, loops
    the turn; `echo_playing_link` cleared.
  - `Escape` → closes overlay (unchanged).
- `cargo build` + `cargo clippy` clean.

## Open items for the plan

- Verify the MPV client (`src/mpv/client.rs`) applies a `SetAbLoop` issued just
  before a `LoadFileAndSeek` against the newly loaded file. If the loop must be
  set after load, route it through the existing post-loadfile deferral
  (`pending_loadfile_seek` pattern). This affects only `play_source_turn`'s
  command ordering, not the overall design.

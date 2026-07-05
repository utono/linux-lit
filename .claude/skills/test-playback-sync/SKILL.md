---
name: test-playback-sync
description: Use when verifying that MPV-driven playback sync turns the page at the right moment — after changing sync boundary checks, page-table landings, CursorSync handling, or timestamps — or when a sync stall/late-turn bug needs an automated headless reproduction with a real mpv
argument-hint: <ABBR> [<ABBR>...] | all-arkangel [--boundaries N]
---

# Test Playback Sync (real MPV, headless)

End-to-end timing test for sync page turns. Unlike the nav-fuzz's simulated
`SyncAdvance`, this drives the REAL chain: mpv time-pos → IPC observe →
CursorSync event → boundary check → page turn.

## Run

```bash
.claude/skills/test-playback-sync/run-sync-test.sh --boundaries 2 MND-Arkangel R2-Arkangel
```

For every Shakespeare play (the complete Arkangel edition set):

```bash
.claude/skills/test-playback-sync/run-sync-test.sh --boundaries 2 $(sqlite3 ~/utono/litdb/data/lit.db "SELECT w.abbrev FROM works w WHERE w.work_type='play' AND w.abbrev LIKE '%-Arkangel' ORDER BY w.abbrev;")
```

Run from the repo root. No e2e-env.sh wrapper needed (the script runs its own
dbus-run-session per work). Expect ~30-45s per work; run long sweeps in the
background and read the summary at the end.

## What one boundary check does

1. Private HEADLESS mpv: launched with the display env stripped
   (`env -u WAYLAND_DISPLAY -u DISPLAY`), `--no-config` (no user scripts /
   watch-later), `--ao=null --vo=null --no-video --force-window=no --pause`,
   on a private socket; the DB COPY's media path is rewritten to a symlink so
   `derive_socket_path` can only produce the test's socket
   (`/tmp/mpvsocket-music-<abbrev>-m<id>.m4b`) — the live player is
   unreachable by construction and no window can ever map.
2. Reader in an isolated cage (`LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_SYNC_TEST=1`,
   own XDG_RUNTIME_DIR / log / DB). `LIT_SYNC_TEST` is what re-enables MPV
   discovery+connect under the headless guard (src/mpv/discovery.rs,client.rs).
3. `x` (next page), `y` (back — cursor lands on the previous page's LAST
   dialogue line; its `SEEK: … start=S_LAST` log line is the time reference),
   `Tab` (play).
4. PASS iff `PAGE_TURN` logs within 60s AND mpv's time-pos at the turn is
   within [-0.6s, +2.5s] of the next timestamped line's `start_time`
   (`MIN(start_time) > S_LAST` for that media_id). A `NO_TIMESTAMP` landing
   line downgrades to turn-happened-only (the pending_advance path has no
   exact reference).

## Reading failures

- `no page turn within 60s (sync stall)` — the boundary-check bug class:
  see debug-playback-sync's "Table-mode boundary read" Previously Fixed entry.
- `delta` large positive — turn fired late (live-vs-table boundary drift, or
  a timestamp gap: check `line_timestamps` monotonicity for that media).
- `app never connected to test mpv` — LIT_SYNC_TEST gate or socket-path
  derivation drifted (recompute: author segment is `music` for non-Music
  paths; see derive_socket_path).
- Works are tested at cage's default 720p geometry; the app self-generates a
  page table into the DB COPY, so table mode is active and self-consistent.
  Real lit.db is never written.

## Cleanup

Automatic (trap): kills only its own mpv/cage PIDs, removes its temp base.
Stray check: `pgrep -af 'mpvsocket-music-.*-m[0-9]*\.m4b'`.

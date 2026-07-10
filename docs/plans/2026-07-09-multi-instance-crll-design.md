# Multi-instance linux-lit (crll) — Design

**Date:** 2026-07-09
**Status:** Approved (Approach A of three considered)

## Goal

Let `crll` open two or more linux-lit instances, each with its own MPV
player(s), so a second instance can read a different work while the first keeps
playing. MPV windows stay on dwl tag 10; reader windows stay on tag 3. It is
accepted that a newly launched instance opens on the shared MRU work and the
user then switches it to the work they actually want.

## Why this is currently impossible

Four blockers, found in code:

- **GTK single-instance app id.** `main.rs` builds the Application with id
  `com.utono.linux-lit.dev` (dev) and default flags. A second launch forwards
  `activate` to the running process over D-Bus and exits. This is the hard
  blocker.
- **MPV socket hijack.** Sockets derive purely from the media path
  (`/tmp/mpvsocket-{author}-{basename}`, `src/mpv/discovery.rs`). Instance 2,
  starting on the MRU work, discovers instance 1's live socket and connects to
  it (`display_work`, `src/app/mod.rs` discovery block). When the user then
  switches instance 2 to another work, the `already_connected` path sends
  `loadfile` + `Pause` to that shared MPV — replacing the file instance 1 is
  playing.
- **Shared debug log.** Both instances clear and write
  `linux-lit-dev.log`; the second launch truncates the first's log.
- **Config writeback.** Each instance rewrites `config-dev.json` wholesale on
  exit. Whichever instance exits last reverts the other's reading positions
  (`work_positions`) to the stale values it loaded at its own startup.

Verified non-blockers: the `mpvsocket-*` directory sweep in `discovery.rs` is
test-only code; nothing in the exit path sends a quit-all to MPV (players
deliberately outlive the app for reattach); snapshot cache writes are
temp+rename atomic; lit.db reads are `SQLITE_OPEN_READ_ONLY` and the write path
already sets WAL (multiple processes coexist today with the headless harness).

## Design

### 1. Instance slot module (`src/instance.rs`, new)

Each process auto-assigns itself a **slot number** (1, 2, 3, ...) at startup.

- `acquire()` runs first thing in `main()`, before logging init (the log
  filename depends on the slot). It tries `slot-1.lock`, `slot-2.lock`, ...
  under `$XDG_RUNTIME_DIR/linux-lit/` (fallback `/tmp/linux-lit-$UID/`), taking
  the first file where `std::fs::File::try_lock()` succeeds (std flock wrapper,
  no new dependency). The locked `File` is stored in a `OnceLock` for process
  lifetime, so the OS releases the slot on any exit including crashes — no
  stale-slot cleanup exists or is needed.
- `slot()` returns the assigned number; `suffix()` returns `""` for slot 1 and
  `"-i{n}"`-style fragments where needed.
- **Slot 1 is byte-identical to today's behavior** — no suffixes anywhere.
  Single-instance use, MPV reattach-after-restart, and headless test tooling
  are untouched.
- Slot files are shared between dev and release builds. Side benefit: a dev
  instance running beside a release instance now takes slot 2 and gets its own
  sockets — today those two collide.
- `LIT_INSTANCE=n` env override pins a slot for deterministic test/debug runs.
  If that slot's lock is already held, log the conflict and fall back to the
  normal ascending scan.

### 2. MPV socket namespacing (`derive_socket_path`)

- Slot 1: unchanged — `/tmp/mpvsocket-{author}-{basename}` and
  `/tmp/mpvsocket-ytdlp-{author}-{basename}`.
- Slot n >= 2: `/tmp/mpvsocket-i{n}-{author}-{basename}` and
  `/tmp/mpvsocket-i{n}-ytdlp-{author}-{basename}`.

Discovery (`find_socket_for_work`) only probes paths it derives, so an instance
can only ever find, connect to, stale-clean, or `loadfile`-replace **its own**
players. This closes the hijack path. Instance 2 opening on the MRU work
launches its own (paused) MPV on tag 10; switching works reuses that player via
`loadfile` as normal. Reattach-after-restart works per slot: relaunching and
landing on the same slot re-derives the same socket names.

MPV keeps `wayland-app-id=mpv-lit`, so the dwl tag-10 rule is unaffected.

### 3. GTK multi-instance + window title

- `main.rs`: add `.flags(gio::ApplicationFlags::NON_UNIQUE)` to the
  Application builder. Same app id (dwl tag-3 rule keeps matching), but each
  process gets its own GApplication instead of forwarding activate.
- The single `set_title` site (`src/app/mod.rs`, `"{title} — linux-lit"`)
  appends ` [{slot}]` when slot > 1, e.g. `Hamlet — linux-lit [2]`.

### 4. Logging + the crll alias

- App log: slot 1 keeps `linux-lit-dev.log` / `linux-lit-release.log`; slot n
  uses `linux-lit-dev-{n}.log` / `linux-lit-release-{n}.log`. `LIT_LOG_PATH`
  still overrides everything (fuzz/e2e tooling unaffected).
- Stderr tee in the alias: the alias cannot know the slot the app will take,
  so it uses a cheap heuristic — if no `target/debug/linux-lit` process is
  running, tee to the canonical `linux-lit-dev-stderr.log`; otherwise tee to
  `linux-lit-dev-stderr-2.log`. Cosmetic only; a mis-guess just names the file
  imperfectly. The `check-stderr-log` skill keeps working for the primary
  instance. Alias lives in `~/utono/shell-config/.config/shell/alias-mlj`.

### 5. Config merge-on-save (`src/config.rs`)

`save()` currently writes the instance's whole in-memory snapshot; with two
instances, whoever exits last reverts the other's reading positions. The fix
lives inside `save()` so all call sites (~15) inherit it:

- A module-level static `Mutex<Vec<String>>` of **dirty work abbrevs** (a Vec
  with linear dedup rather than a HashSet: `Mutex::new(Vec::new())` is
  const-constructible in a static; the set holds a handful of abbrevs).
  Every site that updates `work_positions` for a work also marks that abbrev
  dirty; a parallel session-opened list (in recency order) feeds
  `recent_works`.
- **Implementation addendum (2026-07-09):** positions live in THREE per-work
  maps, not one — `work_positions` (legacy line-number fallback),
  `work_position_ids` (line-id keyed, primary), and `last_gloss`. All three
  get the identical dirty-key overlay in `merge_configs`.
- On save: re-read the config file fresh from disk. For `work_positions`,
  start from the file's map and overwrite only dirty keys with this instance's
  values. For `recent_works`, move this session's opened works to the front of
  the file's list, dedupe, truncate to the existing cap.
- Everything else — font size, theme, `last_work`, `previous_work`, all scalar
  settings — stays last-writer-wins. That is the natural MRU semantic: the
  last-closed instance's work becomes the next launch's work, which the user
  confirmed is expected.
- `LIT_HEADLESS_TEST` still suppresses writes entirely. The atomic temp+rename
  write is kept.

## Error handling

- Slot dir uncreatable or every lock attempt fails (pathological): fall back
  to slot 1 with a `log_always` line — degrade to today's behavior rather than
  refusing to start.
- `LIT_INSTANCE` conflict: logged fallback to the ascending scan, never a hard
  error.
- Config merge read failure (file missing or corrupt JSON): fall back to
  writing the full snapshot, exactly as today.

## Out of scope

- lit.db concurrency hardening (busy_timeout tuning) — WAL is already set on
  the write path and multiple processes coexist today.
- Per-instance config profiles (Approach C, rejected: instance 2 would lose
  recent-works/position context; shared-MRU startup is desired).
- Any change to which dwl tags windows land on.

## Testing

- Unit: extend the existing `derive_socket_path` tests with slot-suffixed
  variants; test the config merge logic (dirty-key overlay, recent_works
  merge) as pure functions.
- Headless: launch two instances under the cage harness with `LIT_INSTANCE=1`
  and `LIT_INSTANCE=2` and separate `LIT_LOG_PATH`s; assert distinct socket
  paths (`mpvsocket-...` vs `mpvsocket-i2-...`) and that a work-switch in
  instance 2 never writes to instance 1's socket.
- Standard gates: `cargo build`, `cargo test --bins`, `cargo clippy`.
- Live acceptance (user): two terminals, `crll` in each; second window titled
  `[2]`; each drives its own MPV on tag 10; close in either order and confirm
  both works' reading positions survive in `config-dev.json`.

## Alternatives considered

- **B — alias/env only:** `crll` counts running processes with pgrep and
  exports `LIT_INSTANCE` + `LIT_LOG_PATH`. Rejected: process-count slot
  detection is racy (close instance 1 while 2 runs → next launch collides with
  slot 2), bare launches outside the alias get no isolation, and it still
  needs the config merge — so it saves almost nothing.
- **C — full per-instance profiles:** separate `config-dev-{n}.json` and total
  isolation. Rejected: strictly worse UX (no shared recent works/positions);
  the shared-MRU startup behavior is explicitly wanted.

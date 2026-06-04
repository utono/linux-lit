# Headless Testing

How the `headless-test` skill
(`.claude/skills/headless-test/`) drives linux-lit's GUI with no monitor and
without touching a live `cargo run` session — for screenshot/clip verification
and for the randomized navigation fuzz. Read this when a headless run won't
start, stalls, surfaces a stray window, or you need to understand the
`LIT_DB_PATH` / `LIT_LOG_PATH` env overrides.

## The two things the skill does

1. **Screenshot UI tests** — launch the reader in a throwaway headless `cage`,
   inject keybinds with `wtype`, screenshot with `grim`, and assert the reading
   card never clips its first/last line. Driven by
   `.claude/skills/headless-test/run-headless-test.sh`.
2. **Navigation fuzz** — auto-start the app's in-process nav-test harness
   (`src/input/nav_test.rs`) in a randomized mode that runs ~750 seeded-random
   jumps and checks an invariant after each (on-page landing, y round-trip, no
   mid-page scene break, viewport fill, cursor-is-dialogue). Driven by env vars,
   not the screenshot script.

Both run **inside a nested headless compositor** so they never collide with the
user's dwl session or seat.

## The launch stack (and why each piece exists)

```
scripts/e2e-env.sh           # dbus session bus + AT-SPI registry + software GL
  └── cage                   # single-client headless Wayland compositor
        └── target/debug/linux-lit   # the app, with GSK_RENDERER=cairo etc.
```

- **`cage`, not bare dwl/sway** — linux-lit only lays out and paints once it has
  a configured, focused, fullscreen surface. cage gives the single client
  exactly that. Bare wlroots on the headless backend leaves the window unsized,
  so the reveal hits its "load may be stuck" fallback and renders blank.
- **`scripts/e2e-env.sh`** wraps the command in a private `dbus-run-session` and
  starts the AT-SPI registry. Without it the app aborts right after
  `STARTUP: main entry` (no a11y/dbus). A *bare* `cage -- linux-lit` will not
  work — always go through this wrapper.
- **`GSK_RENDERER=cairo`** is mandatory: the default Vulkan/ngl renderer loses
  its surface on the headless backend and the app aborts with a stack overflow.
  `WLR_RENDERER=pixman` keeps wlroots on software rendering too.
- **`LIT_HEADLESS_TEST=1`** makes `launch_mpv` skip MPV entirely — otherwise
  MPV's window covers the reader in the test compositor and leaks a process.

## Driving and capturing (screenshot mode)

cage opens a fresh Wayland socket (usually `wayland-0`/`wayland-1` under the
run's `XDG_RUNTIME_DIR`). The script waits for it, waits for the app to map and
report `TEST_VIEWPORT_RECT` (the reading pane's window-space rectangle — the app
logs it on reveal because `sourceview5::View` exposes no AT-SPI Text interface),
then:

- `grim` screenshots the active output → `target/ui/<label>_N.png`.
- `wtype` injects keys (virtual-keyboard protocol — works without owning the
  seat, unlike `ydotool`/libinput).
- `scripts/check_line_clipping.py --region` asserts the first/last line aren't
  clipped (fail-closed: if numpy/pillow can't import, it fails).

`target/ui/` is auto-cleaned at the start of every run, so it only holds the
current run's captures. See the skill's `SKILL.md` for the `--label` / `--step`
/ `--setup` / `--no-clip` / `--region` flags.

## The environment overrides

These let an **isolated** run (notably the fuzz) avoid every shared resource a
live `cargo run` session holds. Unset, the app behaves exactly as normal.

### `LIT_DB_PATH` — private database copy

- **Default:** the app reads `~/utono/litdb/data/lit.db` (read-only), the same
  file the live session uses (`src/db/queries.rs::db_path`).
- **Why override:** SQLite serializes access with file locks. A headless run
  that queries the DB (every scene jump loads scene-synopsis/concordance data)
  contends with the live session for those locks. In practice this **stalls the
  fuzz right after the first scene jump** — the process sits idle (blocked on a
  lock), not spinning, with no panic. Only step 0 logs, then nothing.
- **What it does:** when `LIT_DB_PATH` is set and non-empty, `db_path()` returns
  it verbatim. Point it at a copy:

  ```bash
  cp ~/utono/litdb/data/lit.db /tmp/fuzz-lit.db
  # then launch with LIT_DB_PATH=/tmp/fuzz-lit.db
  ```

  The run reads its own copy; no shared lock, no contention. This is the fix for
  the "fuzz hangs at step 0" symptom — confirmed: with the private copy the fuzz
  runs hundreds of steps; without it, it stops at step 0.

### `LIT_LOG_PATH` — private log file

- **Default:** dev builds clear and write `~/utono/linux-lit/linux-lit-dev.log`;
  release builds use `linux-lit-release.log` (`src/main.rs`).
- **Why override:** the app truncates its log on launch. A second instance
  sharing the path clobbers the live session's log, and the contention can kill
  the cage. The fuzz also needs its own log to read back `NAV_TEST:` lines
  cleanly.
- **What it does:** when `LIT_LOG_PATH` is set, `main` uses it as the log path
  instead of the default. Point it at e.g. `/tmp/fuzz-nav.log`.

### `LIT_NAV_FUZZ` — auto-start the fuzz

- `LIT_NAV_FUZZ=1` auto-starts the nav-test harness ~6s after launch (once the
  work has loaded) and selects the long random `fuzz` script instead of the
  fixed `jumps-only` script. Without it, the harness only runs on the
  `Ctrl+Shift+T` keybind with the fixed script.

## What "a fuzz run" is

A **fuzz run** feeds the navigation code a long stream of *randomized* jumps and
checks, after every single one, that a set of invariants still holds — instead of
hand-scripting "press x, then y, expect page 3." It finds edge cases a human
would never think to script. linux-lit's fuzz lives in the app itself
(`src/input/nav_test.rs`): when started in its random mode it generates ~750
**seeded**-random jumps (`x`, `y`, `2`, `3`, `gg`, `G`, chapter jumps — seeded so
a failure is reproducible) and after each one asserts:

- the cursor landed on the page that's actually visible (on-page landing),
- `y` round-trips `x`,
- no act/scene marker sits mid-page,
- the viewport is at least 10% full,
- the cursor is on a dialogue line.

Each violation is logged as `NAV_TEST: FAIL step=N <Action> <reason>`. A run is
"clean" when there are no FAIL lines. (The fuzz found, e.g., the `G`-to-end
off-page landing and the right-column mid-page scene break.)

## How to run the fuzz

Use the `headless-test` skill's bundled launcher. It builds, makes a private DB
copy, sets all the env overrides, launches an isolated cage, runs for ~5.5 min,
kills its own cage by PID, and prints a failure summary. It's safe to run **even
while a live `cargo run` session is open** — it touches no shared file. Always
go through the env wrapper (`e2e-env.sh`, which supplies dbus + AT-SPI):

```bash
cd ~/utono/linux-lit
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz.sh
# shorter run while iterating:
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz.sh --secs 90
```

It writes the run log to `/tmp/fuzz-nav.log` and the cage PID to
`/tmp/fuzz_pid.txt`, and warns on stderr if the fuzz is still at ≤1 step after
25 s (the classic DB-lock-contention stall — see below).

Watch progress / triage at any time:

```bash
rg -c "NAV_TEST: step" /tmp/fuzz-nav.log     # how many steps have run
rg "NAV_TEST: FAIL" /tmp/fuzz-nav.log \
  | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
# one failure in context:  rg "NAV_TEST" /tmp/fuzz-nav.log | rg -B2 "FAIL step=124"
```

Stop early — kill **only** the recorded PID. Never `pkill -f
target/debug/linux-lit`; it would also signal a live session:

```bash
kill "$(cat /tmp/fuzz_pid.txt)"
```

The launcher is `~/utono/linux-lit/.claude/skills/headless-test/run-fuzz.sh`; if
the fuzz stays at 1 step it logs `NAV_TEST: step=0` then nothing (CPU idle, no
panic) — almost always `LIT_DB_PATH` wasn't honored, i.e. it's contending on the
shared `lit.db` lock. Confirm `/tmp/fuzz-lit.db` exists and is being passed.

The fuzz tuning (seeded LCG, 400 ms cadence so layout settles, `MAX_STEPS`, the
per-step invariants) lives in `src/input/nav_test.rs`; the page-navigation
behaviour it checks is documented in `page-turning-mechanics.md`.

## Targeted navigation trace (manual key injection)

To pin down a *specific* nav behaviour ("does `k` page back at the left-column
top?"), drive keys with `wtype` in an isolated cage and grep the log — and use a
before/after screenshot to confirm the visible result. Launch the app exactly
like the fuzz (private DB + log, `LIT_HEADLESS_TEST=1`, through `e2e-env.sh`) but
**without** `LIT_NAV_FUZZ`, then inject keys after the window maps. Hard-won
lessons:

- **Pick the work + start position via `config-dev.json`, not the live config.**
  In dev mode (`LIT_DEV=1`, which every headless launch sets) the app reads
  `~/.config/linux-lit/config-dev.json` — a *separate* file from the live
  session's `config.json` (`src/config.rs::config_path`). Set its `last_work`
  and `work_positions` to land the test on a specific work and spread (e.g.
  `"last_work": "AWW"`, `"work_positions": { "AWW": 4342 }` to start near All's
  Well's EPILOGUE). This is how the empty-right-column EPILOGUE bug was
  reproduced — the default work is whatever `config-dev.json` last held, which is
  often a different (prose) work than the one you're debugging. Editing it is
  safe; it never touches the live `config.json`. **Caveat:** a headless run
  rewrites `config-dev.json` on exit (it persists its own last position), so
  re-set it before each run if the position drifted.
- **The app resumes near the document END.** Press `g g` first to reset to the
  top, or a forward jump may be a silent no-op (`x`/`q`/`j` do nothing past the
  last line) and your test never reaches a page boundary. Give `gg` ~0.5 s to
  settle before the next key.
- **`wtype` drops keys when hammered.** Space presses ≥0.18–0.25 s apart; at
  0.13 s some are silently lost and your counts come out short — which can look
  like "the page didn't turn" when really the keypress never landed.
- **The FIRST keypress after launch is often dropped** (window not yet focused).
  Wait ~11 s after launching, then send a throwaway warm-up key (e.g. `j` then
  `k` to return) before the real sequence. If a single decisive keypress produces
  **no `ACTION:` line at all** in the log, it never landed — not a code bug; add
  the warm-up / more settle and retry.
- **Stale-binary trap: rebuild, THEN launch — never overlap them.** If you launch
  the cage in the same step that (or right after a step that) rebuilds, the cage
  may exec the *previous* binary. Symptom: a guard you just added is present in
  the source and the binary built fine, but the run behaves as if it's missing.
  Run `cargo build` to completion first, then launch as a separate step. (Several
  "the fix didn't work" dead-ends this session were just this.)
- **`G`/`gg` from a resume state may not land where you expect.** Jumping to end
  with `G` from a resumed position occasionally lands on the opening rather than
  the tail — don't assume; `grim` and check. To reach a specific spread reliably,
  set `config-dev.json`'s `work_positions` to a line on/near it instead of
  navigating there.
- **One page-turn per boundary crossing is correct.** Don't read "few
  `NAV_PAGE_FWD` for many `j`" as a bug: a two-column spread holds ~40–80 lines,
  so dozens of `j` cross only one boundary. Compare the cursor line to the
  spread's `page_end`, not to the keypress count.
- **Grep the always-on nav logs first, but confirm by screenshot.** `NAV_PAGE_FWD`
  / `NAV_PAGE_BACK` / `NAV_SCENE_FWD` / `NAV_SCENE_BACK` print each page turn with
  `current` / `old_top` / `new_top`; `ACTION:` prints each dispatched key. A
  temporary one-line probe in `is_line_fully_visible`'s two-column branch
  (logging `line` / `page_top` / `page_end`) is the fastest way to see *why* a
  turn did or didn't fire. But a "no turn fired" log can still hide a layout bug
  (e.g. the EPILOGUE rendering in the *wrong* column) — `grim` a `/tmp/before.png`
  and `/tmp/after.png` around the key sequence and read them. That's how the
  empty-right-column EPILOGUE behaviour was distinguished from a clean spread:
  the log said the turn was suppressed, but the screenshot showed which column
  the tail actually landed in.

A typical drive sequence (inside the launch script, after the socket is up):

```bash
export WAYLAND_DISPLAY=... XDG_RUNTIME_DIR=...    # the cage's socket + rt dir
for n in $(seq 1 5); do wtype -k k; sleep 0.28; done   # cursor up onto left col
grim /tmp/mid.png
for n in $(seq 1 10); do wtype -k j; sleep 0.30; done  # forward across boundary
grim /tmp/after.png
```

## Process hygiene — do NOT `pkill` by binary name

cage is headless (offscreen), but a launch that detaches or fails its cleanup
can briefly surface a window, and several can pile up across debugging
iterations. The cage shares the user's display server context enough that a
stray instance is disruptive.

- The launcher records the cage PID in `/tmp/fuzz_pid.txt`. **Kill exactly that
  PID:** `kill "$(cat /tmp/fuzz_pid.txt)"`.
- **Never** `pkill -f target/debug/linux-lit` — that pattern also matches the
  user's live `cargo run` session and will signal it.
- **Telling the live session apart from a test instance:** a test app is a child
  of a `cage` process; the user's `cargo run` session is a child of a `zsh` and
  has a long `ELAPSED`. But note a test cage and the live session can share the
  *same* parent zsh (the one Claude Code runs commands under), so don't identify
  by parent alone — combine signals:

  ```bash
  ps -eo pid,ppid,etime,cmd | rg "[t]arget/debug/linux-lit"   # elapsed + parent
  ps -o pid,cmd -p <PPID>                                     # is the parent a cage?
  ```

  A standalone `target/debug/linux-lit` with a multi-minute `ELAPSED` and a
  non-cage parent is the live session — leave it alone.
- When done, confirm nothing stray remains (the live session may still show):

  ```bash
  pgrep -af "cage -- ./target/debug/linux-lit"   # test cages — should be empty
  ```

## Symptom → cause quick reference

- **App aborts right after `STARTUP: main entry`** → launched without
  `scripts/e2e-env.sh` (no dbus/a11y), or `GSK_RENDERER=cairo` not set.
- **Fuzz logs only `NAV_TEST: step=0` then nothing, CPU idle** → DB lock
  contention with the live session; set `LIT_DB_PATH` to a private copy.
- **Blank reader / "load may be stuck"** → ran under bare dwl/sway instead of
  cage; the surface never got sized.
- **Live session's `linux-lit-dev.log` got clobbered** → a headless run shared
  the log path; set `LIT_LOG_PATH`.
- **Stray reader window on screen** → a leaked cage instance; kill by recorded
  PID, confirm `pgrep -f "cage -- ./target/debug/linux-lit"` is empty.

## Key files

- `.claude/skills/headless-test/SKILL.md` — the skill (flags, fuzz recipe).
- `.claude/skills/headless-test/run-headless-test.sh` — the screenshot driver.
- `scripts/e2e-env.sh` — dbus + AT-SPI + software-GL wrapper.
- `scripts/check_line_clipping.py` — fail-closed clipping detector.
- `src/db/queries.rs::db_path` — honors `LIT_DB_PATH`.
- `src/main.rs` — honors `LIT_LOG_PATH`.
- `src/input/nav_test.rs` — the nav-test harness (fuzz + invariants);
  `LIT_NAV_FUZZ` auto-start lives in `src/app.rs`.
- `docs/troubleshooting/page-turning-mechanics.md` — the navigation behaviour
  the fuzz verifies.

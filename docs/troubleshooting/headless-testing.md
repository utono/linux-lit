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

The three env overrides above make a fuzz run safe to launch **even while a live
`cargo run` session is open** — it touches no shared file. Steps:

1. **Build** the debug binary the cage will run:

   ```bash
   cd ~/utono/linux-lit && cargo build
   ```

2. **Prepare the isolated launcher.** It copies the DB, sets all the env
   overrides, runs for ~5.5 min, and kills its own cage by recorded PID:

   ```bash
   : > /tmp/fuzz-nav.log
   cp ~/utono/litdb/data/lit.db /tmp/fuzz-lit.db
   cat > /tmp/fuzz-launch.sh <<'EOF'
   #!/usr/bin/env bash
   RT="$(mktemp -d)"
   XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo \
     LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NAV_FUZZ=1 \
     LIT_LOG_PATH=/tmp/fuzz-nav.log LIT_DB_PATH=/tmp/fuzz-lit.db \
     cage -- ./target/debug/linux-lit >"$RT/cage.log" 2>&1 &
   CAGE=$!; echo "$CAGE" > /tmp/fuzz_pid.txt
   for _ in $(seq 1 330); do ps -p "$CAGE" >/dev/null 2>&1 || break; sleep 1; done
   kill "$CAGE" 2>/dev/null; rm -rf "$RT" 2>/dev/null
   EOF
   chmod +x /tmp/fuzz-launch.sh
   ```

3. **Launch it through the env wrapper** (provides dbus + AT-SPI):

   ```bash
   ./scripts/e2e-env.sh /tmp/fuzz-launch.sh &     # ~5.5 min, backgrounded
   ```

4. **Watch progress** (the fuzz auto-starts ~6 s after launch):

   ```bash
   rg -c "NAV_TEST: step" /tmp/fuzz-nav.log    # how many steps have run
   ```

   If this stays at `1` for more than a few seconds, the run stalled — almost
   always DB lock contention, meaning `LIT_DB_PATH` wasn't honored (re-check the
   copy exists and the env var is set).

5. **Triage the failures** by category once it has run a few hundred steps:

   ```bash
   rg "NAV_TEST: FAIL" /tmp/fuzz-nav.log \
     | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
     | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
   ```

   For a specific failure, look at the raw line and the steps around it:
   `rg "NAV_TEST" /tmp/fuzz-nav.log | rg -B2 "FAIL step=124"`.

6. **Stop early** (optional) — kill **only** the recorded PID:

   ```bash
   kill "$(cat /tmp/fuzz_pid.txt)"
   ```

   Never `pkill -f target/debug/linux-lit` — that also matches a live session.

The fuzz tuning (seeded LCG, 400 ms cadence so layout settles, `MAX_STEPS`, the
per-step invariants) lives in `src/input/nav_test.rs`; the page-navigation
behaviour it checks is documented in `page-turning-mechanics.md`.

## Process hygiene — do NOT `pkill` by binary name

cage is headless (offscreen), but a launch that detaches or fails its cleanup
can briefly surface a window, and several can pile up across debugging
iterations. The cage shares the user's display server context enough that a
stray instance is disruptive.

- The launcher records the cage PID in `/tmp/fuzz_pid.txt`. **Kill exactly that
  PID:** `kill "$(cat /tmp/fuzz_pid.txt)"`.
- **Never** `pkill -f target/debug/linux-lit` — that pattern also matches the
  user's live `cargo run` session and will signal it.
- When done, confirm nothing stray remains:

  ```bash
  pgrep -af "cage -- ./target/debug/linux-lit"   # should be empty
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

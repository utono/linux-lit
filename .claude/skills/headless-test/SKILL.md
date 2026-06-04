---
name: headless-test
description: Use when verifying linux-lit's GUI headlessly — screenshotting the reader/overlay, injecting keybinds (j/k/x/y/gg/G/h/Escape), or checking the reading card for top/bottom line clipping without a monitor or the live session
argument-hint: --label NAME [--setup "KEYS"] [--step "KEYS"]… [--no-clip] [--region X,Y,W,H]
---

# Headless UI test

Drive linux-lit inside a throwaway headless `cage`, inject keybinds, screenshot,
and assert the reading card never clips its first/last line — all off-screen, on
its own Wayland socket, never touching the live session.

## Run it

Always go through the env wrapper (provides software GL + dbus + the AT-SPI
registry the artifacts want):

```bash
./scripts/e2e-env.sh .claude/skills/headless-test/run-headless-test.sh \
  --label clip --step "g g" --step "x x x" --step "+shift:g"
```

- `--label NAME` — output basename under `target/ui/`.
- `--step "KEYS"` — one checkpoint: inject these keys, settle, capture. Repeat
  for each checkpoint. A `_0` baseline is captured before any `--step`.
- `--setup "KEYS"` — keys to run once after launch, before the first capture
  (e.g. `--setup "h"` to open the synopsis overlay).
- `--no-clip` — screenshot + review only; skip the clipping assertion (for UI
  with no text pane, e.g. pickers/overlays).
- `--region X,Y,W,H` — force the clip region (e.g. for the overlay); default is
  the reading viewport the app reports.
- `--settle MS` — pause after each step before capture (default 500).

**Key tokens** (space-separated within a `--step`): a bare token is one xkb
keysym (`j`, `x`, `g`, `Escape`, `Next`); `+mod:key` is a chord in one press
(`+shift:g` → Shift+G, `+ctrl:Home`). Repeat a token to repeat the key.

**RPD keymap** (what to inject for common nav): line `j`/`k`, page `x`/`y`, top
`g g` (sequential), end `+shift:g`, next/prev chapter `braceleft`/`bracketleft`,
synopsis overlay `h`, close `Escape`. Keys hit the window's global controller —
no focus step needed.

## After running

Artifacts land in `target/ui/<label>_N.png` (+ `_clip.png`, `.clip.json`).
`target/ui/` is auto-cleaned at the start of each run, so it only ever holds the
current run's captures. **Open every PNG and report what you see inline** in your
reply (window title, panels, on-screen text, and whether anything clips) — that
is the verification step. No written review file is required.

## Navigation fuzz mode

A randomized navigation stress test that drives the app's in-process nav-test
harness (`src/input/nav_test.rs`) and verifies every jump lands correctly. It
runs ~750 deterministic-random steps (x/y/2/3/gg/G/chapter), and after each
checks: forward progress on x, y round-trips, **landing on-page** (cursor within
the visible range after a jump — catches G/3 mis-landings), no scene break
mid-page, viewport fill ≥10%, and cursor-is-dialogue. Failures are logged as
`NAV_TEST: FAIL …` with the step and reason.

Run it isolated (its OWN log + runtime dir, never the live `cargo run` session):

```bash
RT="$(mktemp -d)"; LOG=/tmp/fuzz-nav.log; : > "$LOG"
cat > /tmp/fuzz-launch.sh <<EOF
#!/usr/bin/env bash
XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo \\
  LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NAV_FUZZ=1 LIT_LOG_PATH="$LOG" \\
  cage -- ./target/debug/linux-lit >"$RT/cage.log" 2>&1 &
sleep 320; kill \$! 2>/dev/null
EOF
chmod +x /tmp/fuzz-launch.sh
./scripts/e2e-env.sh /tmp/fuzz-launch.sh &     # ~5 min run
# then summarize:
rg "NAV_TEST: FAIL" /tmp/fuzz-nav.log | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
```

Key points:
- **`LIT_NAV_FUZZ=1`** auto-starts the fuzz ~6s after launch (once the work
  loads) and selects the long random script; **`LIT_LOG_PATH`** redirects the log
  so it never collides with a live session's `linux-lit-dev.log` (sharing it
  kills the cage). The PRNG is seeded (deterministic) so a failure replays.
- Drive it through **`e2e-env.sh`** (dbus + a11y) — a bare `cage` launch aborts
  right after `STARTUP: main entry`.
- Each step waits 400ms so GTK layout settles; faster cadence makes
  pixel-dependent checks (`column_split`, `jump_to_end`) read stale heights and
  report layout-instability false positives.
- The fixed (non-random) `jumps-only` script still runs via `Ctrl+Shift+T`
  without `LIT_NAV_FUZZ`.

## Why these settings (do not "simplify" away)

The script bakes in hard-won requirements; changing them silently breaks rendering:

- **cage, not dwl/sway** — linux-lit only paints once it has a configured
  fullscreen surface; bare wlroots leaves it unsized → blank ("load may be stuck").
- **`GSK_RENDERER=cairo`** — the default Vulkan/ngl renderer aborts headless.
- **`LIT_HEADLESS_TEST=1`** — `launch_mpv` skips MPV (else its window covers the
  reader and leaks a process).
- **Region from the app** — `sourceview5::View` exposes no AT-SPI Text interface,
  so the detector can't auto-find the pane; the app logs `TEST_VIEWPORT_RECT` on
  reveal and the harness reads it. (For the overlay, pass `--region` explicitly.)

## Common mistakes

- No `e2e-env.sh` wrapper → the detector can't import numpy/pillow, fails closed.
- Overlay tested with the default region → it isn't the reading pane; use
  `--setup "h"` + `--region` (or `--no-clip`).
- Wrong keysym for shifted keys → use `+shift:g`, `+shift:braceleft`; a bare
  wrong token silently no-ops.

See also `tests/line_clipping.rs` (cargo-test form) and the "Automated UI tests"
section of `CLAUDE.md`.

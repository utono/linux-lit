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

---
name: headless-test
description: Use when verifying linux-lit's GUI headlessly — screenshotting the reader/overlay, injecting keybinds (j/k/x/y/gg/G/h/Escape), or checking the reading card for top/bottom line clipping without a monitor or the live session
argument-hint: --label NAME [--setup "KEYS"] [--step "KEYS"]… [--no-clip] [--region X,Y,W,H]
---

# Headless UI test

Drive linux-lit inside a throwaway headless `cage`, inject keybinds, screenshot,
and assert the reading card never clips its first/last line — all off-screen, on
its own Wayland socket, never touching the live session.

## Verify ALL Shakespeare works (give the user these two commands)

The agent's headless runs are unreliable in some sandboxes (long runs get killed).
When a full cross-work check is wanted, **hand the user this exact copy-paste
block** to run in their own terminal — it builds, then fuzzes every Folger
Shakespeare play (all navigation binds — `x` `y` `G` `gg` `2` `3` `,` `q` — from
every act/scene boundary) and reports any play with an underfilled/empty right
column or other failure:

```bash
cd ~/utono/linux-lit && cargo build
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh --secs-each 70
```

Then ask them to paste `/tmp/fuzz-all-works-summary.txt` (one line per play:
`<ABBR> steps=N FAIL=N UNBALANCED=N RIGHT_EMPTY=N`). Any line with `FAIL>0`,
`UNBALANCED>0`, or `RIGHT_EMPTY>0` is a real bug; its full log is at
`/tmp/fuzz-<ABBR>.log`. The whole sweep takes ~`70s × number of plays` (≈45 min
for the ~37 plays). Details below under *Sweep ALL Shakespeare works*.

### Fix-loop (`--stop-on-first-fail`): stop → fix → re-run

The whole-sweep loop is **user-runs → agent-fixes → user-re-runs** (the agent
can't reliably run the fuzz itself). To make each cycle fast, use
`--stop-on-first-fail`: the sweep halts at the FIRST play that fails, prints that
play's FAIL categories, and prints the exact re-run command. This avoids waiting
45 min when there's one obvious bug.

```bash
cd ~/utono/linux-lit && cargo build
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh \
  --stop-on-first-fail --secs-each 70
```

Loop: user runs this → it stops at the first failing play and shows the
categories → user pastes them → agent fixes the bug + rebuilds → user re-runs the
same command. Repeat until it gets through all plays with no FAIL. (Drop
`--stop-on-first-fail` for the final confirming full sweep.)

**Binary-freshness race — commit BEFORE telling the user to rebuild.** The agent
commits in a background shell that can lag ~30 s. If the user's `cargo build`
runs before the commit lands, they test one-commit-behind code and the SAME
numbers reappear (byte-identical, same `[NNNNms]` timestamp) — looking like "the
fix didn't work" when it just wasn't built. Always (a) confirm `git log -1` shows
the new commit before saying "rebuild", and (b) when the numbers are suspiciously
unchanged, check `stat -c %y target/debug/linux-lit` vs the commit time: if the
binary is OLDER, it's stale — `touch src/…` + rebuild.

**The per-step diagnostic loop is how page-boundary bugs actually get fixed.**
Don't reason about `column_split` / `next_page_top` / `prev_page_top` from the
failure message alone — the geometry is non-obvious and viewport-height-dependent.
Add a temporary `PPT_DBG:`/`JTE_DBG:`-style `log_fmt!` to the suspect function
that dumps the walk: each `probe`/`top` with its `split`, `page_end`,
`next_page_top`, and `buffer_line_text`, plus the gap/overlap lines with their
`is_dialogue_line` flag. Commit it, have the user re-run, read the log. The line
TEXT is what disambiguates (e.g. a 3-line `y GAP` whose gap is
`blank / '[They exit.]' / blank` is a benign scene-transition; a 5-line gap of
real dialogue is a bug). Remove the diagnostic once the cause is pinned. This
loop solved the entire `y`-at-scene/final-boundary class.

**Gaps/overlaps: only DIALOGUE counts.** A `y GAP`/`y OVERLAP` or `UNBALANCED`
is benign when the affected lines are scene-transition (blanks, `[X exits.]`,
ACT/SCENE markers) rather than dialogue — the invariants gate on
`is_dialogue_line`. Two boundary cases are inherently un-tileable and are exempt:
a right column that **clamped at a scene break** (the next scene starts the next
spread — the chosen reading model), and `y` **from the forward-pulled final
spread** (`last_page_top` pulls it off the natural chain, so a small seam is
unavoidable).

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

A navigation stress test that drives the app's in-process nav-test harness
(`src/input/nav_test.rs`) and verifies every jump lands correctly. Each run is
**1400 steps: a deterministic coverage prelude, then a random body.**

- **Coverage prelude** (`build_coverage_prelude`) — drives **every** nav action
  (x/y/2/3/gg/G/chapter/`q`/`j`/`,`/`k`) from **every** structural anchor on
  **every** run: from the work start, the work end, each of the first 24 scene
  boundaries, and 3 mid-page anchors. This is what makes the fuzz test *all
  scenarios* instead of sampling them — a random seed alone leaves real bugs
  (like "G from a distant page lands on the wrong final spread") hidden behind a
  lucky "0 failures". To cover a newly-added action, add its `Step` to
  `ALL_STEPS`.
- **Random body** — a deterministic-random mix weighted toward structural jumps
  and gg/G, plus **boundary-stress motifs** (`G x x x`, `G y y y`, double
  end/top, dialogue-walk into the tail then reverse) for combinatorial depth on
  top of the guaranteed coverage.

Two severities are logged:

- **`NAV_TEST: FAIL …`** — a hard correctness invariant. These must be 0:
  forward progress on x, y never goes backward-then-forward (round-trips),
  **landing on-page** (cursor within the visible range after a jump — catches
  G/3 mis-landings), **right column never empty** unless the tail truly can't
  fill it, **left column never underfilled** before the end, **jump-to-end
  reaches the work's end** (no dialogue left below the spread — catches the
  final-spread-too-early / orphaned-EPILOGUE bug), and a real-path
  cursor-is-dialogue check.
- **`NAV_TEST: WARN …`** — a harness-approximation note, *not* a product bug.
  The `SearchJump` step is a simulation (it snaps to `next_dialogue_line`), so a
  landing on a non-dialogue line like `[All exit.]`, and the mid-page
  scene-break / return-mismatch checks, are downgraded to WARN so they don't
  mask real FAILs. A clean run is **0 FAIL, a handful of WARNs**.

### How to run it (via the skill)

Use the bundled `run-fuzz.sh` — it builds, makes a private DB copy, launches an
isolated cage with all the env overrides, kills its own cage by PID, and prints a
failure summary. Always go through `e2e-env.sh`.

**Full sweep** (run this for a real check — the whole 1400-step coverage prelude
+ random body; needs ~10 min, so size the window so it isn't cut short):

```bash
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz.sh --secs 600 --start-work AWW --start-pos 50
```

`--secs` is a wall-clock cap, not a step count: at ~400 ms/step a short window
ends the run early (e.g. `--secs 90` ≈ 200 steps, only the start of the prelude).
For the complete sweep use `--secs 600` or more. The hermetic-start flags make
the run reproducible from the command alone:

- `--start-work AWW` — which work to load (a play with an EPILOGUE exercises the
  final-spread/clip edge cases). Without it, the dev config's last work is used.
- `--start-pos 50` — start line; jumping `G` from here is a genuine long jump.
- `--seed 0x...` — pin the LCG seed to replay a specific run (the seed is printed
  at run start: `NAV_TEST: seed=0x...`).

**Shorter run while iterating** (won't complete the full prelude, but fast):

```bash
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz.sh --secs 90
```

**Sweep ALL Shakespeare works** (verify every play, not just one — page geometry
is work-specific, so a bug can hide in a play with a different act/scene
structure, e.g. an underfilled right column at a scene boundary):

```bash
./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh --secs-each 70
```

It fuzzes each Folger Shakespeare play in its own isolated cage and prints a
per-work summary (`steps FAIL UNBALANCED RIGHT_EMPTY`), flagging any work with a
hard FAIL or an underfilled/empty right column. Flagged work logs are saved to
`/tmp/fuzz-<ABBR>.log` for inspection. Use this after any pagination change that
could affect spread balance.

It writes the log to `/tmp/fuzz-nav.log` and the cage PID to `/tmp/fuzz_pid.txt`.
Stop early with `kill "$(cat /tmp/fuzz_pid.txt)"` — **never** `pkill -f
target/debug/linux-lit`, which also signals a live `cargo run` session. (Test
instances carry a `--headless-test` marker, so
`pkill -f 'linux-lit --headless-test'` targets only them, never the live
session.)

**Diagnosing a wrong-spread bug?** Don't guess line numbers from the rendered
text. **(0) CHECK VIEWPORT HEIGHT FIRST** — page-turn math depends on
`text_view.height()` (logged as `widget_h=`), and the headless cage is usually a
*different height* than the user's session, so it computes *different boundaries*
and the bug may not reproduce. If a screenshot bug won't reproduce headless, the
fastest fix is an **in-app `*_DBG:` `log_fmt!`** in the suspect function dumping
`widget_h`, the chosen top, and `column_split` of a few candidate tops (each with
`split`/`page_end`/`next_page_top`/`would_empty_right_column` + line text) — build,
have the **user** reproduce once, read `linux-lit-dev.log`. (1) Or run a **unique
copy** of the binary with its own log so you can never read a stale one
(`cp target/debug/linux-lit /tmp/lit-dbg-$(date +%s)` then run that file directly
— `run-fuzz.sh`'s rebuild can leave a stale binary running on the old log).
(2) Log boundary line numbers **with their text** (`buffer_line_text`). Full
recipe: *headless-testing.md → "Diagnosing a specific page-boundary bug"* (§3 is
the viewport-height lesson).

**Final-spread bugs are a SET of paths, not one.** FIVE independent code paths can
land on the work's last spread, and fixing one does NOT fix the others — verify
each separately: **startup** (`app.rs` resume + `snap_near_end_to_canonical` after
layout), **G** (`jump_to_end`→`last_page_top`), **x** (`page_forward`), **j**
(`scroll_after_jump_forward`), **y** (`page_backward`→`prev_page_top`). They must
all resolve to the SAME canonical spread (`last_page_top` — tail dialogue left,
full EPILOGUE right). The recurring failure mode near a short trailing section
(EPILOGUE): a path picks an **underfilled** spread one boundary short (right
column has 2-3 lines, EPILOGUE cut off) because `would_empty_right_column` is
*false* (the column isn't empty, just short). The redirect test must therefore be
"candidate overlaps the final region" (`column_split(cand).next_page_top > anchor`)
— NOT just "would empty". When a final-spread fix lands, re-test all five.

Re-triage an existing log at any time. Hard failures first (must be 0), then
the soft warns:

```bash
# hard FAILs — must be 0
rg "NAV_TEST: FAIL" /tmp/fuzz-nav.log | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
# soft WARNs — harness-approximation, expected to be non-zero
rg "NAV_TEST: WARN" /tmp/fuzz-nav.log | sed -E 's/.*WARN step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
# one failure in context: rg "NAV_TEST" /tmp/fuzz-nav.log | rg -B2 "FAIL step=124"
```

When a FAIL appears, read the cursor-vs-visible numbers in the message
(`cursor=N not in visible [lo, hi]`): if the cursor is *past* `hi` near the end
of the work, it's the final-spread/EPILOGUE class — the page anchored on the
last full two-column spread but the cursor was placed on a line the spread's
right column clamped off (see *page-turning-mechanics.md*, "G / jump-to-end").

### Why it's isolated (do not "simplify" away)

- **`LIT_DB_PATH`** points the app at a private copy of `lit.db`. Sharing the
  real file with a live session causes SQLite lock contention that **stalls the
  fuzz right after the first scene jump** — the process sits idle (blocked on a
  lock), only `NAV_TEST: step=0` logs, CPU near 0, no panic. `run-fuzz.sh` warns
  if it sees ≤1 step after 25s. This was the single hardest bug to diagnose;
  always keep the DB copy.
- **`LIT_LOG_PATH`** gives the run its own log; sharing the live session's
  `linux-lit-dev.log` (which is truncated on launch) clobbers it and can kill
  the cage.
- **`LIT_NAV_FUZZ=1`** auto-starts the fuzz ~6s after the work loads and selects
  the long random script. The PRNG is seeded (deterministic) so a failure
  replays on re-run.
- Drive it through **`e2e-env.sh`** (dbus + a11y) — a bare `cage` launch aborts
  right after `STARTUP: main entry`.
- **cage is headless (offscreen)**, but a launch that detaches or fails cleanup
  can briefly surface a window and several can pile up. `run-fuzz.sh` reaps its
  own cage via a trap; afterwards confirm
  `pgrep -f "cage -- ./target/debug/linux-lit"` is empty.
- Each step waits 400ms so GTK layout settles; faster cadence makes
  pixel-dependent checks (`column_split`, `jump_to_end`) read stale heights and
  report layout-instability false positives.
- The fixed (non-random) `jumps-only` script still runs via `Ctrl+Shift+T`
  without `LIT_NAV_FUZZ`.

## Targeted navigation trace (manual key injection)

For pinning down a *specific* nav behaviour (e.g. "does `k` page back at the
left-column top?"), drive keys manually and grep the log rather than
screenshotting. Lessons from doing this:

- **Pick the work + spread via `config-dev.json`** (the dev config, `LIT_DEV=1`),
  NOT the live `config.json`. Set `last_work` + `work_positions` to land on a
  specific spread (e.g. AWW near its EPILOGUE). A headless run rewrites it on
  exit, so re-set it before each run.
- **Rebuild, THEN launch — as separate steps.** Launching the cage in/right after
  a build step can exec the *previous* binary; the symptom is "my new guard is in
  the source and builds, but the run ignores it." This wasted real time.
- **The FIRST keypress after launch is often dropped** (no focus yet). Wait ~11s,
  send a throwaway warm-up key, then the real sequence. A decisive key with **no
  `ACTION:` line** never landed — add warm-up/settle, don't assume a code bug.
- **The app resumes near the document END.** Press `g g` first to reset to the
  top, or a forward jump may be a no-op (`x`/`q`/`j` do nothing past the last
  line) and your test reaches no boundary. If `gg`/`G` seems not to take or lands
  oddly, `grim` and check rather than trusting it.
- **`wtype` drops keys when hammered.** Space presses ≥0.18–0.25s apart; at
  0.13s some are silently lost and your counts come out short.
- **One page-turn per boundary crossing is correct.** Don't read "few
  `NAV_PAGE_FWD` for many `j`" as a bug — a two-column spread can hold ~40–80
  lines, so dozens of `j` cross only one boundary. Compare the cursor line to the
  spread's `page_end`, not to the keypress count.
- **Grep the always-on nav logs**, not screenshots: `NAV_PAGE_FWD` /
  `NAV_PAGE_BACK` / `NAV_SCENE_FWD` / `NAV_SCENE_BACK` show each page turn with
  `current` / `old_top` / `new_top`; `ACTION:` shows each dispatched key. A quick
  `is_line_fully_visible` probe (temporarily log `line`/`page_top`/`page_end` in
  its two-column branch) is the fastest way to see why a turn did or didn't fire.

Pattern (run through `e2e-env.sh`, private DB + log like the fuzz):

```bash
sleep 8                                   # let the window map
export WAYLAND_DISPLAY=... XDG_RUNTIME_DIR=...   # the cage socket
wtype -k g; sleep 0.2; wtype -k g; sleep 0.5     # reset to top
for n in $(seq 1 40); do wtype -k j; sleep 0.2; done
# then: rg "NAV_PAGE_FWD|ACTION: CursorNextDialogue" /tmp/<log>
```

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

#!/usr/bin/env bash
# run-headless-test.sh — drive a headless UI test of linux-lit inside cage.
#
# Launches the app in an isolated headless `cage`, optionally injects a sequence
# of keybinds, screenshots the result(s), and (unless --no-clip) runs the
# fail-closed line-clipping detector against the app-reported reading viewport.
#
# Why this exists / hard-won gotchas baked in (do NOT "simplify" these away):
#   * cage, not bare dwl/sway — bare wlroots leaves the window unsized and the
#     reader never paints (blank, 5s "load may be stuck" fallback).
#   * GSK_RENDERER=cairo is MANDATORY — default Vulkan/ngl aborts headless.
#   * LIT_DEV=1 + LIT_HEADLESS_TEST=1 — dev log + MPV is skipped (else MPV's
#     window covers the reader and leaks a process).
#   * region via the app's TEST_VIEWPORT_RECT log line — sourceview5::View has
#     no AT-SPI Text interface, so the clipping detector can't auto-find it.
#
# Usage:
#   run-headless-test.sh --label NAME [options]
#
# Options:
#   --label NAME        Output basename under target/ui/ (default: headless).
#   --setup "KEYS"      wtype key sequence to run ONCE after launch, before the
#                       first capture (e.g. to open an overlay: "h"). Optional.
#   --step "KEYS"       A checkpoint: inject these keys, settle, capture. Repeat
#                       --step for each checkpoint. The app is also captured ONCE
#                       before any --step (the `_0` baseline). Each --step value
#                       is space-separated key tokens (see below). Omit all
#                       --step to capture a single frame with no input.
#   --no-clip           Screenshot + review only; skip the clipping assertion
#                       (use for non-text UI, e.g. pickers/overlays w/o a pane).
#   --region X,Y,W,H    Force the clip region instead of the app-reported one
#                       (use for the overlay or a sub-pane).
#   --settle MS         Pause after each step before capture (default 500).
#
# Key tokens (passed to wtype): a bare token is `wtype -k <token>` (one keysym,
#   e.g. j, x, g, Escape, Next). Prefix with `+mod:` for a chord in one wtype
#   call, e.g. `+shift:g` → Shift+G, `+ctrl:Home`. Use real xkb keysym names.
#   Prefix with `@` to type literal characters (wtype character mode), e.g.
#   `@A` — REQUIRED for handlers that match the uppercase/shifted CHARACTER:
#   `+shift:a` delivers keysym `a` + a shift modifier, which key+shift reader
#   binds see but a handler matching the literal `A` never does.
#   Repeated keys = repeat the token, e.g. --step "g g" sends g then g.
#
# Examples:
#   # main reading card, clipping at top / 3 pages down / end:
#   run-headless-test.sh --label clip --step "g g" --step "x x x" --step "+shift:g"
#   # synopsis overlay, no main-pane clip assert:
#   run-headless-test.sh --label synopsis --setup "h" --no-clip
#
# Run THROUGH the env wrapper so numpy/pillow + the a11y/dbus session exist:
#   ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh …
set -uo pipefail

usage() {
  cat >&2 <<'USAGE'
headless-test — drive linux-lit's GUI headlessly (screenshot/clip tests + nav fuzz).

ALWAYS run through the env wrapper (dbus + AT-SPI + software GL):
  ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh [options]

Screenshot / clipping test (this script):
  --label NAME       output basename under target/ui/ (default: headless)
  --setup "KEYS"     keys to inject once after launch, before the first capture
  --step "KEYS"      a checkpoint: inject keys, settle, capture. Repeat per step.
                     (a _0 baseline is captured before any --step)
  --no-clip          screenshot + review only; skip the line-clipping assertion
  --region X,Y,W,H   force the clip region (default: the app-reported viewport)
  --settle MS        pause after each step before capture (default 500)
  --start-work ABBR  hermetic start work (LIT_START_WORK) — pin the work under
                     test instead of the config's last_work
  --start-pos N      hermetic start line (LIT_START_POS)

  Isolated from a live session: private log (/tmp/headless-test.log via
  LIT_LOG_PATH), private DB copy (/tmp/headless-lit.db via LIT_DB_PATH), no
  config writes (LIT_HEADLESS_TEST), no MPV.

  Key tokens (space-separated within a --step): a bare token is one xkb keysym
  (j, x, g, Escape); `+mod:key` is a chord (+shift:g -> Shift+G); `@TEXT` types
  literal characters (@A — needed when the handler matches the uppercase
  character itself, since +shift:a delivers lowercase-with-shift). Repeat to repeat.

  Examples:
    ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh \
      --label clip --step "g g" --step "x x x" --step "+shift:g"
    ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-headless-test.sh \
      --label synopsis --setup "h" --no-clip

Navigation fuzz test (separate launcher in this skill):
  ALL SHAKESPEARE WORKS (~45 min — fuzz every Folger play, x/y/G/gg/2/3/,/q
  from every act/scene boundary; flags underfilled/empty right columns):
    cd ~/utono/linux-lit && cargo build
    ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz-all-works.sh --secs-each 70
    → summary: /tmp/fuzz-all-works-summary.txt ; flagged logs: /tmp/fuzz-<ABBR>.log
  FULL SWEEP, ONE WORK (~10 min, all 1400 steps):
    ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh \
      --secs 600 --start-work AWW --start-pos 50
  Quick run while iterating (~200 steps, start of the prelude only):
    ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --secs 90

    A deterministic coverage prelude (every nav action from every structural
    anchor) + a seeded-random body, with per-step invariant checks (on-page
    landing, no empty/underfilled column, jump-to-end reaches the end, no line
    clipping). Isolated (private DB copy + log) so it's safe alongside a live
    `cargo run`. --secs is a wall-clock cap, not a step count: short windows end
    early. Flags: --start-work AWW, --start-pos 50, --seed 0x... (replay).
    Triage:
      rg "NAV_TEST: FAIL" /tmp/fuzz-nav.log | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
        | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn
    Stop early: kill "$(cat /tmp/fuzz_pid.txt)"   (never pkill the binary by name;
    test instances carry a --headless-test marker)

See docs/troubleshooting/headless-testing.md for the full guide.
USAGE
}

# No arguments → show usage (including how to run the fuzz) and exit.
if [[ $# -eq 0 ]]; then
  usage
  exit 0
fi

# --- must run from the repo root (holds Cargo.toml, scripts/, target/) --------
if [[ ! -f Cargo.toml || ! -x scripts/e2e-env.sh ]]; then
  echo "error: run from the linux-lit repo root (need Cargo.toml + scripts/e2e-env.sh)" >&2
  exit 64
fi

LABEL="headless"
SETUP=""
STEPS=()          # one checkpoint per --step "KEYS" (argv array; no delimiter parsing)
NO_CLIP=0
FORCE_REGION=""
SETTLE_MS=500
START_WORK="${LIT_START_WORK:-}"
START_POS="${LIT_START_POS:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --label)  LABEL="$2"; shift 2 ;;
    --setup)  SETUP="$2"; shift 2 ;;
    --step)   STEPS+=("$2"); shift 2 ;;
    --no-clip) NO_CLIP=1; shift ;;
    --region) FORCE_REGION="$2"; shift 2 ;;
    --settle) SETTLE_MS="$2"; shift 2 ;;
    --start-work) START_WORK="$2"; shift 2 ;;
    --start-pos)  START_POS="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "error: unknown option '$1'" >&2; echo >&2; usage; exit 64 ;;
  esac
done

BIN=target/debug/linux-lit
# Private per-run log via LIT_LOG_PATH. The old scheme truncated and grepped the
# SHARED slot-1 dev log — with a live `cargo run` session holding slot 1, this
# run takes slot 2 and writes linux-lit-dev-2.log, so the TEST_VIEWPORT_RECT
# grep matched nothing AND the truncation wiped the user's live log.
RUN_LOG=/tmp/headless-test.log
# Private DB copy — sharing lit.db with a live session causes SQLite lock
# contention (same reason run-fuzz.sh copies it).
DB_SRC="$HOME/utono/litdb/data/lit.db"
DB_COPY=/tmp/headless-lit.db
UIDIR=target/ui
mkdir -p "$UIDIR"

# Auto-clean stale artifacts so a run only ever contains THIS turn's captures
# (no leftover screenshots from prior, unrelated runs to wade back through).
rm -f "$UIDIR"/*.png "$UIDIR"/*.clip.json "$UIDIR"/*.json \
      "$UIDIR"/*.suspects.txt 2>/dev/null || true

# --- build -------------------------------------------------------------------
echo "[headless-test] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

# --- fresh isolated runtime dir + cage --------------------------------------
echo "[headless-test] copying DB → $DB_COPY" >&2
\cp -f "$DB_SRC" "$DB_COPY" || { echo "DB copy failed" >&2; exit 1; }
: > "$RUN_LOG"
RT="$(mktemp -d)"
cleanup() { kill "$CAGE_PID" 2>/dev/null || true; rm -rf "$RT" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

# Hermetic-start overrides (only set when requested, so an empty value never
# overrides the config's last_work).
HERMETIC=()
[[ -n "$START_WORK" ]] && HERMETIC+=("LIT_START_WORK=$START_WORK")
[[ -n "$START_POS"  ]] && HERMETIC+=("LIT_START_POS=$START_POS")

env -u WAYLAND_DISPLAY \
  XDG_RUNTIME_DIR="$RT" \
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_RENDERER_ALLOW_SOFTWARE=1 \
  WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 \
  GDK_BACKEND=wayland GSK_RENDERER=cairo \
  LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_LOG_PATH="$RUN_LOG" LIT_DB_PATH="$DB_COPY" \
  "${HERMETIC[@]}" \
  cage -- "$BIN" --headless-test >"$RT/cage.log" 2>&1 &
CAGE_PID=$!

# --- wait for cage's wayland socket -----------------------------------------
SOCK=""
for _ in $(seq 1 50); do
  SOCK="$(ls "$RT"/wayland-* 2>/dev/null | grep -v '\.lock' | head -1)"
  [[ -n "$SOCK" ]] && break
  sleep 0.1
done
[[ -z "$SOCK" ]] && { echo "error: cage socket never appeared" >&2; exit 1; }
export XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY="$(basename "$SOCK")"
echo "[headless-test] cage socket: $WAYLAND_DISPLAY" >&2

# --- wait for the reader to reveal (and learn the viewport rect) -------------
REGION="$FORCE_REGION"
for _ in $(seq 1 80); do
  if grep -q "TEST_VIEWPORT_RECT" "$RUN_LOG" 2>/dev/null; then break; fi
  sleep 0.1
done
if [[ -z "$REGION" ]]; then
  RECT_LINE="$(grep "TEST_VIEWPORT_RECT" "$RUN_LOG" 2>/dev/null | tail -1)"
  if [[ -n "$RECT_LINE" ]]; then
    # "…TEST_VIEWPORT_RECT x y w h" → "x,y,w,h"
    REGION="$(echo "$RECT_LINE" | sed -E 's/.*TEST_VIEWPORT_RECT ([0-9]+) ([0-9]+) ([0-9]+) ([0-9]+).*/\1,\2,\3,\4/')"
  fi
fi
echo "[headless-test] region: ${REGION:-<none>}" >&2
sleep 1  # let the first paint settle

# --- inject one wtype key group ("g g", "+shift:g", or "@A" tokens) ----------
inject() {
  local group="$1" tok mods key m j
  local -a args MS
  for tok in $group; do
    if [[ "$tok" == @?* ]]; then
      # character mode: type the literal text (needed for handlers matching an
      # uppercase/shifted CHARACTER — `+shift:a` only delivers keysym a + shift)
      wtype -s 150 -d 15 "${tok#@}" </dev/null 2>/dev/null
    elif [[ "$tok" == +*:* ]]; then
      mods="${tok%%:*}"; mods="${mods#+}"; key="${tok##*:}"
      args=()
      IFS=',' read -ra MS <<< "$mods"
      for m in "${MS[@]}"; do args+=(-M "$m"); done
      args+=(-k "$key")
      for ((j=${#MS[@]}-1; j>=0; j--)); do args+=(-m "${MS[$j]}"); done
      wtype "${args[@]}" </dev/null 2>/dev/null
    else
      wtype -s 150 -k "$tok" </dev/null 2>/dev/null
    fi
  done
}

# --- capture one checkpoint: screenshot + (optional) clip assert -------------
FAILED=0
capture() {
  local name="$1"
  local png="$UIDIR/$name.png"
  grim "$png" 2>/dev/null || { echo "grim failed for $name" >&2; FAILED=1; return; }
  python3 scripts/annotate_ui.py --shot "$png" >/dev/null 2>&1 || true  # best-effort
  echo "[headless-test] captured $png ($(stat -c %s "$png") bytes)" >&2
  if [[ "$NO_CLIP" == 0 && -n "$REGION" ]]; then
    if python3 scripts/check_line_clipping.py --shot "$png" --region "$REGION"; then
      echo "[headless-test] $name: no clipping" >&2
    else
      echo "[headless-test] $name: CLIPPING DETECTED (or uneval) — see ${png%.png}_clip.png" >&2
      FAILED=1
    fi
  fi
}

# settle: convert integer milliseconds to a fractional-second sleep.
settle() { sleep "$(awk "BEGIN{printf \"%.3f\", $SETTLE_MS/1000}")"; }

# --- run: optional setup, then capture-before, then per-checkpoint -----------
[[ -n "$SETUP" ]] && { inject "$SETUP"; settle; }

if [[ ${#STEPS[@]} -eq 0 ]]; then
  capture "$LABEL"
else
  capture "${LABEL}_0"        # baseline before any keys
  i=1
  for step in "${STEPS[@]}"; do
    inject "$step"            # space-separated key tokens for this checkpoint
    settle
    capture "${LABEL}_${i}"
    i=$((i+1))
  done
fi

echo "[headless-test] artifacts in $UIDIR/ — open the PNGs and report what you see inline" >&2
exit "$FAILED"

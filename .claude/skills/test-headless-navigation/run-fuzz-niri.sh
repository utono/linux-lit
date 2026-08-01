#!/usr/bin/env bash
# run-fuzz-niri.sh — run linux-lit's randomized navigation fuzz under the REAL
# window manager (niri), then print a failure summary. The niri counterpart of
# run-fuzz.sh; that script (cage) remains the default.
#
# Why a separate runner: cage is a kiosk compositor that force-fullscreens its
# single client. niri tiles, and honors client-side decorations — so a window
# that carries a title bar loses ~37px of height here that it would never lose
# under cage. Since pagination keys on the TEXT VIEW height, that is exactly the
# kind of difference worth exercising under the WM the user actually runs.
#
# niri has NO headless backend (it is Smithay, not wlroots: WLR_BACKENDS is
# ignored and it falls back to the TTY backend and panics). So it runs NESTED:
#
#     cage (wlroots, headless, pixman)   <- provides the parent display
#       └── niri (winit backend)         <- the WM under test
#             └── linux-lit              <- the app, driven by LIT_NAV_FUZZ
#
# The fuzz drives itself IN-PROCESS (LIT_NAV_FUZZ=1, src/input/nav_test.rs), so
# unlike the UI e2e tests this needs no wtype/grim — only a correctly sized,
# focused, fullscreen surface.
#
# MUST be launched through the env wrapper (dbus + AT-SPI), e.g.:
#   ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz-niri.sh --start-work R2-Arkangel
#
# ALWAYS pass --start-work: without it the run loads AND REWRITES the dev
# config's last_work, silently moving which work you are reading.
#
# Options:
#   --secs N        how long to let the fuzz run (default 330 ≈ 5.5m).
#   --start-work W  work abbrev to open (e.g. R2-Arkangel). Strongly recommended.
#   --start-pos P   starting position.
#   --seed N        nav RNG seed (repeatable runs).
#   --size WxH      output size (default 1920x1236 = production geometry).
#
# Output:
#   /tmp/fuzz-nav-niri.log   the run's log (NAV_TEST: lines live here).
#   /tmp/fuzz_niri_pid.txt   the outer cage PID.
set -uo pipefail

if [[ ! -f Cargo.toml || ! -x scripts/e2e-env.sh ]]; then
  echo "error: run from the linux-lit repo root (need Cargo.toml + scripts/e2e-env.sh)" >&2
  exit 64
fi
command -v niri >/dev/null || { echo "error: niri not installed" >&2; exit 69; }
command -v cage >/dev/null || { echo "error: cage not installed (niri needs a parent display)" >&2; exit 69; }

SECS=330
# 1920x1236, NOT 1200: pagination keys on the TEXT VIEW height, and only 1236
# reproduces production's text_view.height=1098 (1200 gives 1062 — a 36px miss
# that changes the page grid and can hide a bug entirely). See CLAUDE.md.
SIZE=1920x1236
START_WORK="${LIT_START_WORK:-}"
START_POS="${LIT_START_POS:-}"
NAV_SEED="${LIT_NAV_SEED:-}"
GEN_PAGE_TABLE="${LIT_GEN_PAGE_TABLE:-}"
NO_PAGE_TABLE="${LIT_NO_PAGE_TABLE:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --secs) SECS="$2"; shift 2 ;;
    --start-work) START_WORK="$2"; shift 2 ;;
    --start-pos) START_POS="$2"; shift 2 ;;
    --seed) NAV_SEED="$2"; shift 2 ;;
    --size) SIZE="$2"; shift 2 ;;
    *) echo "error: unknown option '$1'" >&2; exit 64 ;;
  esac
done
OUT_W="${SIZE%x*}"
OUT_H="${SIZE#*x}"

BIN=target/debug/linux-lit
LOG=/tmp/fuzz-nav-niri.log
CFG="$PWD/tests/harness/niri-test.kdl"
DB_SRC="$HOME/utono/litdb/data/lit.db"
DB_COPY=/tmp/fuzz-lit-niri.db

[[ -f "$CFG" ]] || { echo "error: niri test config missing at $CFG" >&2; exit 66; }

echo "[fuzz-niri] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

# Private DB copy — sharing lit.db with a live session causes SQLite lock
# contention that stalls the fuzz. Use sqlite3's online .backup, not cp: a raw
# cp of a WAL database mid-write yields a torn copy and the app aborts.
# A SEPARATE copy from run-fuzz.sh's, so both runners can run concurrently.
echo "[fuzz-niri] copying DB → $DB_COPY" >&2
rm -f "$DB_COPY" "$DB_COPY-wal" "$DB_COPY-shm"
sqlite3 "$DB_SRC" ".backup '$DB_COPY'" || { echo "DB copy failed" >&2; exit 1; }
: > "$LOG"

# Short runtime dir, NOT mktemp's default: niri's IPC socket is
# <runtime>/niri.<display>.<pid>.sock and the whole path must fit in SUN_LEN.
# A long path makes every `niri msg` fail with "path must be shorter than
# SUN_LEN", which presents as an unfullscreenable window rather than an error.
RT="$(mktemp -d /tmp/fznr-XXXX)"
chmod 700 "$RT"

cleanup() {
  [ -n "${CAGE:-}" ] && kill -- "-${CAGE}" 2>/dev/null
  kill "${CAGE:-0}" 2>/dev/null || true
  # Unambiguous marker: the live session carries no --headless-test arg. Never
  # `pkill -f target/debug/linux-lit` — that would kill the user's instance.
  pkill -f 'linux-lit --headless-test' 2>/dev/null || true
  # Kill anything still holding an FD into $RT before removing it, or the
  # kernel keeps the ~628M DB copy's space until those FDs close and /tmp
  # eventually fills. (Same hazard as the cage runner.)
  if [ -n "${RT:-}" ] && [ -d "$RT" ]; then
    for fd in /proc/[0-9]*/fd; do
      if readlink "$fd"/* 2>/dev/null | grep -q "^$RT"; then
        kill -9 "$(basename "$(dirname "$fd")")" 2>/dev/null
      fi
    done
  fi
  rm -rf "$RT" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

HERMETIC=()
[[ -n "$START_WORK" ]] && HERMETIC+=("LIT_START_WORK=$START_WORK")
[[ -n "$START_POS"  ]] && HERMETIC+=("LIT_START_POS=$START_POS")
[[ -n "$NAV_SEED"   ]] && HERMETIC+=("LIT_NAV_SEED=$NAV_SEED")
[[ -n "$GEN_PAGE_TABLE" ]] && HERMETIC+=("LIT_GEN_PAGE_TABLE=$GEN_PAGE_TABLE")
[[ -n "$NO_PAGE_TABLE"  ]] && HERMETIC+=("LIT_NO_PAGE_TABLE=$NO_PAGE_TABLE")

if [[ -z "$START_WORK" ]]; then
  echo "[fuzz-niri] WARNING: no --start-work; this run will load AND REWRITE the" \
       "dev config's last_work." >&2
fi

# `setsid` puts the stack in its own process group so cleanup reaps cage, niri,
# and the app together. `-u WAYLAND_DISPLAY` keeps the OUTER cage on its
# headless backend rather than nesting a visible window in the live session.
# niri itself ignores the WLR_* vars (Smithay) but needs software GL for winit.
setsid env -u WAYLAND_DISPLAY -u DISPLAY -u NIRI_SOCKET \
  XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo GDK_BACKEND=wayland \
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_RENDERER_ALLOW_SOFTWARE=1 \
  WLR_LIBINPUT_NO_DEVICES=1 WLR_HEADLESS_OUTPUTS=1 \
  LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe \
  LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NAV_FUZZ=1 \
  LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB_COPY" \
  "${HERMETIC[@]}" \
  cage -- niri -c "$CFG" -- "$BIN" --headless-test >"$RT/stack.log" 2>&1 &
CAGE=$!
echo "$CAGE" > /tmp/fuzz_niri_pid.txt
echo "[fuzz-niri] cage pid=$CAGE (own pgroup), niri nested, log: $LOG" >&2

# Wait for niri's IPC socket, which is also the readiness signal for the WM.
NIRI_SOCK=""
for _ in $(seq 1 60); do
  NIRI_SOCK=$(find "$RT" -maxdepth 1 -name 'niri.*.sock' 2>/dev/null | head -1)
  [[ -n "$NIRI_SOCK" ]] && break
  sleep 0.5
done
if [[ -z "$NIRI_SOCK" ]]; then
  echo "[fuzz-niri] ERROR: niri IPC socket never appeared — did niri fall back" \
       "to the TTY backend? See $RT/stack.log" >&2
  sed -n '1,40p' "$RT/stack.log" >&2
  exit 1
fi
NIRI_DISPLAY="$(basename "$NIRI_SOCK" | cut -d. -f2)"
echo "[fuzz-niri] niri up on $NIRI_DISPLAY (ipc: $NIRI_SOCK)" >&2

# Resize the OUTER cage output. niri's winit output inherits its size from the
# parent surface, so a `mode` in the niri config is inert — only this moves it.
# Target cage's own display (wayland-0), not niri's: niri implements no
# wlr-output-management, so pointing wlr-randr at it would find nothing.
CAGE_OUT=$(XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY=wayland-0 wlr-randr 2>/dev/null \
  | awk 'NF && $0 !~ /^[[:space:]]/ {print $1; exit}')
if [[ -n "$CAGE_OUT" ]]; then
  XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY=wayland-0 \
    wlr-randr --output "$CAGE_OUT" --custom-mode "${OUT_W}x${OUT_H}" >/dev/null 2>&1 \
    && echo "[fuzz-niri] output $CAGE_OUT → ${OUT_W}x${OUT_H}" >&2 \
    || echo "[fuzz-niri] WARNING: resize to ${OUT_W}x${OUT_H} failed" >&2
else
  echo "[fuzz-niri] WARNING: no cage output found; running at the 1280x720 default" >&2
fi
sleep 2

# Fullscreen the reader. niri TILES — it does not force-fullscreen like cage —
# and with decorations honored a tiled window loses title-bar height. The fuzz
# reads text_view.height() for its page grid, so it must run fullscreen to see
# production geometry.
#
# `fullscreen-window` is a TOGGLE and niri 26.04 reports no fullscreen flag, so
# state cannot be queried — verify by RESULT instead: check the achieved
# text_view height in the log and toggle at most twice.
niri_msg() { NIRI_SOCKET="$NIRI_SOCK" XDG_RUNTIME_DIR="$RT" niri msg "$@" 2>/dev/null; }
text_view_h() { grep -o 'text_view.height changed [-0-9]* -> [0-9]*' "$LOG" 2>/dev/null | tail -1 | awk '{print $NF}'; }

for attempt in 1 2; do
  niri_msg action fullscreen-window >/dev/null
  sleep 3
  h="$(text_view_h)"
  # Production geometry is ~1098. Anything near it means fullscreen took.
  if [[ -n "$h" ]] && (( h > 900 )); then
    echo "[fuzz-niri] fullscreen OK (text_view.height=$h)" >&2
    break
  fi
  echo "[fuzz-niri] attempt $attempt: text_view.height=${h:-none}; re-toggling" >&2
done
h_final="$(text_view_h)"
if [[ -z "$h_final" ]] || (( h_final <= 900 )); then
  echo "[fuzz-niri] WARNING: text_view.height=${h_final:-none} (expected ~1098)." \
       "The page grid will NOT match production; treat results with suspicion." >&2
fi

# Stall guard, same as the cage runner: the fuzz auto-starts ~6s in.
warned=0
for ((i = 0; i < SECS; i++)); do
  ps -p "$CAGE" >/dev/null 2>&1 || { echo "[fuzz-niri] stack exited at ${i}s" >&2; break; }
  if (( i == 25 && warned == 0 )); then
    steps=$(grep -c "NAV_TEST: step" "$LOG" 2>/dev/null || echo 0)
    if (( steps <= 1 )); then
      echo "[fuzz-niri] WARNING: only $steps step(s) after 25s — likely a stall" \
           "(DB lock contention? window never fullscreened? check $RT/stack.log)." >&2
    fi
    warned=1
  fi
  sleep 1
done

kill "$CAGE" 2>/dev/null || true
sleep 1

steps=$(grep -c "NAV_TEST: step" "$LOG" 2>/dev/null || echo 0)
fails=$(grep -c "NAV_TEST: FAIL" "$LOG" 2>/dev/null || echo 0)
echo >&2
echo "[fuzz-niri] done: $steps steps, $fails failures (compositor: niri, text_view.height=${h_final:-unknown})" >&2
echo "[fuzz-niri] failure summary by category:" >&2
grep "NAV_TEST: FAIL" "$LOG" 2>/dev/null \
  | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn >&2 || true
echo "[fuzz-niri] full log: $LOG" >&2

#!/usr/bin/env bash
# land-on.sh — deterministically launch linux-lit at a KNOWN work + scene, with
# an optional overlay already open, in an isolated headless cage. Leaves the
# instance running so a caller can `wtype`-drive it from a known starting state
# (no dependence on wherever config's `last_work`/position happen to point).
#
# Backlog item #15 (headless known-scene driver). Mirrors run-fuzz.sh's env
# hygiene: a private DB copy (never the shared lit.db), a private log, and the
# hermetic LIT_START_* / LIT_DEV / LIT_HEADLESS_TEST env — so a run is fully
# reproducible and never rewrites the dev config's last_work (config::save
# no-ops under LIT_HEADLESS_TEST).
#
# Usage:
#   scripts/land-on.sh WORK div1.div2 [journal|synopsis|gloss]
#
# Example:
#   scripts/land-on.sh Ham 1.1 journal
#
# On success (prints to stderr, then exits 0 leaving the cage running):
#   [land-on] WAYLAND_DISPLAY=wayland-1  (drive it: WAYLAND_DISPLAY=… wtype …)
#   [land-on] log=/tmp/land-on.log  pid=NNNN
# Stop it later:  kill "$(cat /tmp/land-on_pid.txt)"   (or the scoped pkill)
#
# Exit codes:
#   0  landed (OVERLAY_MAPPED, or TEST_VIEWPORT_RECT when no overlay requested)
#  64  bad usage / malformed scene arg
#   1  build/DB-copy failure, cage never mapped, or the app aborted at startup
set -uo pipefail

if [[ ! -f Cargo.toml ]]; then
  echo "error: run from the linux-lit repo root (need Cargo.toml)" >&2
  exit 64
fi

WORK="${1:-}"
SCENE="${2:-}"
OVERLAY="${3:-}"

if [[ -z "$WORK" || -z "$SCENE" ]]; then
  echo "usage: scripts/land-on.sh WORK div1.div2 [journal|synopsis|gloss]" >&2
  exit 64
fi
# Scene must be exactly div1.div2 (two non-negative integers). The app validates
# again and logs an ERROR + skips mapping on a bad scene, but reject the obvious
# cases here so the driver fails loudly before spending a launch.
if [[ ! "$SCENE" =~ ^[0-9]+\.[0-9]+$ ]]; then
  echo "error: scene '$SCENE' must be div1.div2 (e.g. 1.1)" >&2
  exit 64
fi
if [[ -n "$OVERLAY" && ! "$OVERLAY" =~ ^(journal|synopsis|gloss)$ ]]; then
  echo "error: overlay '$OVERLAY' must be journal|synopsis|gloss" >&2
  exit 64
fi

BIN=target/debug/linux-lit
LOG=/tmp/land-on.log
DB_SRC="$HOME/utono/litdb/data/lit.db"
DB_COPY=/tmp/land-on-lit.db

echo "[land-on] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

# Private DB copy via sqlite3 .backup (a raw cp of a live WAL DB yields a torn
# copy that aborts the app at startup — same rationale as run-fuzz.sh).
echo "[land-on] copying DB → $DB_COPY" >&2
rm -f "$DB_COPY" "$DB_COPY-wal" "$DB_COPY-shm"
sqlite3 "$DB_SRC" ".backup '$DB_COPY'" || { echo "DB copy failed" >&2; exit 1; }
: > "$LOG"

RT="$(mktemp -d)"
CAGE=""
# On any early failure (map-wait timeout / abort) tear the cage down so the
# script never leaks a headless instance. On SUCCESS we deliberately leave the
# cage running for the caller, so the trap is DISABLED before the success exit.
cleanup() {
  [ -n "${CAGE:-}" ] && kill -- "-${CAGE}" 2>/dev/null
  kill "${CAGE:-0}" 2>/dev/null || true
  pkill -f 'linux-lit --headless-test' 2>/dev/null || true
  rm -rf "$RT" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# Hermetic-start env: forwarded explicitly (see run-fuzz.sh — dbus/ambient
# inheritance isn't guaranteed). LIT_START_OVERLAY only set when requested.
HERMETIC=("LIT_START_WORK=$WORK" "LIT_START_SCENE=$SCENE")
[[ -n "$OVERLAY" ]] && HERMETIC+=("LIT_START_OVERLAY=$OVERLAY")

# Force wlroots onto its HEADLESS backend and drop the inherited WAYLAND_DISPLAY
# so cage runs entirely off-screen (never nests a visible window on the live
# session). setsid → own process group so cleanup can kill the whole tree. The
# `--headless-test` arg is an inert process-table marker for scoped cleanup.
setsid env -u WAYLAND_DISPLAY \
  XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman \
  LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NO_MPV=1 \
  LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB_COPY" \
  "${HERMETIC[@]}" \
  cage -- "$BIN" --headless-test >"$RT/cage.log" 2>&1 &
CAGE=$!
echo "$CAGE" > /tmp/land-on_pid.txt
echo "[land-on] cage pid=$CAGE (own pgroup)  work=$WORK scene=$SCENE overlay=${OVERLAY:-<none>}" >&2

# Success marker: OVERLAY_MAPPED when an overlay was requested, otherwise the
# reveal-time TEST_VIEWPORT_RECT (work displayed + interactive).
if [[ -n "$OVERLAY" ]]; then
  WANT="OVERLAY_MAPPED: $OVERLAY"
else
  WANT="TEST_VIEWPORT_RECT"
fi

# Wait up to ~30s for the marker (cold first build already happened; startup +
# layout settle + the 500ms deferred overlay open take a few seconds).
DEADLINE=$((SECONDS + 30))
while (( SECONDS < DEADLINE )); do
  if ! ps -p "$CAGE" >/dev/null 2>&1; then
    echo "[land-on] ERROR: cage exited before mapping (see $RT/cage.log / $LOG)" >&2
    grep -iE "ERROR|abort|panic" "$LOG" 2>/dev/null | tail -5 >&2 || true
    exit 1
  fi
  # A resolvable-scene failure logs "STARTUP: ERROR LIT_START_SCENE=…": fail fast.
  if grep -q "STARTUP: ERROR LIT_START_SCENE" "$LOG" 2>/dev/null; then
    echo "[land-on] ERROR: scene did not resolve —" >&2
    grep "STARTUP: ERROR LIT_START_SCENE" "$LOG" >&2
    exit 1
  fi
  if grep -qF "$WANT" "$LOG" 2>/dev/null; then
    WD="$(basename "$(ls -t "$RT"/wayland-* 2>/dev/null | grep -v '\.lock$' | head -1)" 2>/dev/null)"
    echo "[land-on] landed: '$WANT'" >&2
    grep -E "LIT_START_SCENE=.* resolved to buffer line|CURSOR_LINE:|OVERLAY_MAPPED:" "$LOG" 2>/dev/null | tail -4 >&2 || true
    echo "[land-on] XDG_RUNTIME_DIR=$RT  WAYLAND_DISPLAY=${WD:-<check $RT>}" >&2
    echo "[land-on] log=$LOG  pid=$CAGE  (stop: kill \"\$(cat /tmp/land-on_pid.txt)\")" >&2
    # Leave the cage running for the caller — disable the teardown trap. $RT is
    # intentionally left in place (the live wayland socket lives there).
    trap - EXIT INT TERM
    exit 0
  fi
  sleep 0.5
done

echo "[land-on] ERROR: '$WANT' not seen within 30s (see $LOG / $RT/cage.log)" >&2
tail -8 "$LOG" 2>/dev/null >&2 || true
exit 1

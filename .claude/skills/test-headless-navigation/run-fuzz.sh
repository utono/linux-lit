#!/usr/bin/env bash
# run-fuzz.sh — run linux-lit's randomized navigation fuzz in an isolated
# headless cage, then print a failure summary. Safe to run alongside a live
# `cargo run` session: it uses a private DB copy and log, never the shared ones.
#
# A "fuzz run" drives the in-process nav-test harness (src/input/nav_test.rs) in
# its random mode: ~750 seeded-random jumps (x/y/2/3/gg/G/chapter/search — the
# search step runs the REAL `/` path via execute_search_with_query), with an
# invariant checked after each (on-page landing, y round-trip, no mid-page scene
# break, viewport fill, no full-card blank clip, cursor-is-dialogue). Each
# violation logs `NAV_TEST: FAIL`.
#
# MUST be launched through the env wrapper (dbus + AT-SPI), e.g.:
#   ./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh [--secs N]
#
# A bare run (no e2e-env.sh) aborts the app right after `STARTUP: main entry`.
#
# Options:
#   --secs N   how long to let the fuzz run before stopping (default 330 ≈ 5.5m).
#
# Output:
#   /tmp/fuzz-nav.log   the run's log (NAV_TEST: lines live here).
#   /tmp/fuzz_pid.txt   the cage PID (to stop early: kill "$(cat /tmp/fuzz_pid.txt)").
set -uo pipefail

if [[ ! -f Cargo.toml || ! -x scripts/e2e-env.sh ]]; then
  echo "error: run from the linux-lit repo root (need Cargo.toml + scripts/e2e-env.sh)" >&2
  exit 64
fi

SECS=330
# Hermetic start overrides (forwarded explicitly into the cage env below, since
# e2e-env.sh re-execs under dbus-run-session and ambient inheritance isn't
# guaranteed). Accept as flags OR inherit from the environment if already set.
START_WORK="${LIT_START_WORK:-}"
START_POS="${LIT_START_POS:-}"
NAV_SEED="${LIT_NAV_SEED:-}"
# Page-table generation/fallback toggles (src/input/page_table.rs) — same
# forwarding problem as the hermetic-start vars above: e2e-env.sh's
# dbus-run-session re-exec doesn't guarantee ambient env inheritance, so these
# must be threaded through explicitly if set in the calling shell.
GEN_PAGE_TABLE="${LIT_GEN_PAGE_TABLE:-}"
NO_PAGE_TABLE="${LIT_NO_PAGE_TABLE:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --secs) SECS="$2"; shift 2 ;;
    --start-work) START_WORK="$2"; shift 2 ;;
    --start-pos) START_POS="$2"; shift 2 ;;
    --seed) NAV_SEED="$2"; shift 2 ;;
    *) echo "error: unknown option '$1'" >&2; exit 64 ;;
  esac
done

BIN=target/debug/linux-lit
LOG=/tmp/fuzz-nav.log
DB_SRC="$HOME/utono/litdb/data/lit.db"
DB_COPY=/tmp/fuzz-lit.db

echo "[fuzz] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

# Private DB copy — sharing lit.db with a live session causes SQLite lock
# contention that stalls the fuzz right after the first scene jump. Use
# sqlite3's online .backup, not cp: a raw cp of a WAL database mid-write by a
# live session yields a torn copy ("malformed database schema ... invalid
# rootpage") and the app aborts at startup.
echo "[fuzz] copying DB → $DB_COPY" >&2
rm -f "$DB_COPY" "$DB_COPY-wal" "$DB_COPY-shm"
sqlite3 "$DB_SRC" ".backup '$DB_COPY'" || { echo "DB copy failed" >&2; exit 1; }
: > "$LOG"

RT="$(mktemp -d)"
# Cleanup kills the whole cage process group (setsid below) AND, as a belt-and-
# braces backstop, any linux-lit carrying our unambiguous `--headless-test`
# marker — which by construction never matches the live `cargo run` session
# (it has no such marker). We still never `pkill -f target/debug/linux-lit`.
cleanup() {
  [ -n "${CAGE:-}" ] && kill -- "-${CAGE}" 2>/dev/null
  kill "${CAGE:-0}" 2>/dev/null || true
  pkill -f 'linux-lit --headless-test' 2>/dev/null || true
  # The cage session spawns dbus/xdg-portal/at-spi daemons that keep $RT/cage.log
  # open; without killing them, `rm -rf "$RT"` unlinks the dir but the kernel
  # holds the 628M lit.db space until those FDs close — over many runs the tmpfs
  # backing /tmp fills to 0 and later runs fail with ENOSPC. Kill any process
  # still holding an FD into $RT, THEN remove it, so the space is reclaimed.
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

# Force wlroots onto its HEADLESS backend and unset the inherited
# WAYLAND_DISPLAY. Without this, cage sees the live session's wayland-0 in the
# environment, picks the Wayland backend, and nests as a VISIBLE window on the
# user's dwl instead of running offscreen (a leaked reader window). The headless
# backend + pixman software renderer keeps it entirely off-screen.
#
# `setsid` puts the cage in its own process group so cleanup can kill the whole
# tree even if it detached. The `--headless-test` arg is a process-table marker
# (the app runs GTK with empty argv, so it ignores it) that lets cleanup/pgrep
# target ONLY test instances, never the live session.
# Build the hermetic-start env list (only assign vars that are set, so an unset
# override doesn't force an empty value).
HERMETIC=()
[[ -n "$START_WORK" ]] && HERMETIC+=("LIT_START_WORK=$START_WORK")
[[ -n "$START_POS"  ]] && HERMETIC+=("LIT_START_POS=$START_POS")
[[ -n "$NAV_SEED"   ]] && HERMETIC+=("LIT_NAV_SEED=$NAV_SEED")
[[ -n "$GEN_PAGE_TABLE" ]] && HERMETIC+=("LIT_GEN_PAGE_TABLE=$GEN_PAGE_TABLE")
[[ -n "$NO_PAGE_TABLE"  ]] && HERMETIC+=("LIT_NO_PAGE_TABLE=$NO_PAGE_TABLE")

setsid env -u WAYLAND_DISPLAY \
  XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman \
  LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NAV_FUZZ=1 \
  LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB_COPY" \
  "${HERMETIC[@]}" \
  cage -- "$BIN" --headless-test >"$RT/cage.log" 2>&1 &
CAGE=$!
echo "$CAGE" > /tmp/fuzz_pid.txt
echo "[fuzz] cage pid=$CAGE (own pgroup), running up to ${SECS}s (log: $LOG)" >&2

# Stall guard: the fuzz auto-starts ~6s in. If it never gets past step 1, the DB
# copy probably wasn't honored (lock contention) — report and bail early.
warned=0
for ((i = 0; i < SECS; i++)); do
  ps -p "$CAGE" >/dev/null 2>&1 || { echo "[fuzz] cage exited at ${i}s" >&2; break; }
  if (( i == 25 && warned == 0 )); then
    steps=$(grep -c "NAV_TEST: step" "$LOG" 2>/dev/null || echo 0)
    if (( steps <= 1 )); then
      echo "[fuzz] WARNING: only $steps step(s) after 25s — likely a stall " \
           "(DB lock contention? check LIT_DB_PATH / the copy)." >&2
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
echo "[fuzz] done: $steps steps, $fails failures" >&2
echo "[fuzz] failure summary by category:" >&2
grep "NAV_TEST: FAIL" "$LOG" 2>/dev/null \
  | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn >&2 || true
echo "[fuzz] full log: $LOG  (e.g. rg 'NAV_TEST: FAIL' $LOG | rg -B2 'step=NNN')" >&2

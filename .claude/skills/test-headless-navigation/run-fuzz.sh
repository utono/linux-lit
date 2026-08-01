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
#   --geometry WxH
#              output size to fuzz at (default 1920x1200 = production). Pass
#              `--geometry none` to keep cage's 1280x720 default — but see the
#              720p warning in the GEOMETRY note below before trusting its
#              failures.
#
# Output:
#   /tmp/fuzz-nav.log   the run's log (NAV_TEST: lines live here).
#   /tmp/fuzz_pid.txt   the cage PID (to stop early: kill "$(cat /tmp/fuzz_pid.txt)").
#
# GEOMETRY — why this defaults to production size, not cage's default:
#
# cage's headless backend comes up at 1280x720. Pagination keys off the TEXT
# VIEW height, so fuzzing at 720p is not "the same test on a smaller screen" —
# it is a DIFFERENT page grid. Two concrete consequences, both of which made the
# fuzz weaker than it looked:
#
#   1. The layout fingerprint embeds the geometry, so at 720p no STORED
#      play_pages table ever matches. The app logged `PAGES: no table … 1280x720`
#      and then `PAGES: generated N pages`, i.e. every historical fuzz run
#      exercised a table generated on the fly, never the stored production
#      tables the user actually reads against. Table-mode bugs that only exist
#      in the stored data could not reproduce.
#   2. 720p yields text_view.height = 648 vs production's 1128 — a different
#      number of rows per column, so page boundaries fall elsewhere entirely.
#
# 1920x1200 → text_view.height 1128 is the pair to hit, and it is MEASURED, not
# assumed. Two independent confirmations:
#   * the user's own logs (linux-lit-dev.log / -release) carry `1920x1200`;
#   * every current-schema stored table in lit.db is keyed to it —
#     `v5|Charis|…|1920x1200|…|1128` (`SELECT layout_fingerprint FROM
#     play_pages_meta`).
#
# NOTE for anyone porting the number from elsewhere: CLAUDE.md's cage guidance
# says 1920x1236 → 1098, and that is correct for the CARGO harness
# (tests/harness/mod.rs), whose window is DECORATED — the titlebar eats ~66px.
# The reader builds its window `.decorated(false)`, so the same output height
# leaves a taller text view. Using 1236 here produced height=1164, which matched
# NO stored fingerprint and silently fell back to a generated table — exactly
# the failure mode this resize exists to eliminate. The run VERIFIES the
# achieved height below rather than trusting wlr-randr's exit status.
#
# `--geometry none` is for debugging the harness itself, NOT for finding bugs.
# At 720p an OLD stored table (`v4|…|1280x720`) still fingerprint-matches, but
# its rows were laid out for a taller viewport, so every page overflows the
# 648px view: a 60s R2-Arkangel run reported 260 failures, all
# LEFT/RIGHT COLUMN CLIPPED, versus 0 for the identical run at 1920x1200. Those
# are geometry artifacts, not product bugs. Treat any 720p failure as suspect
# until it reproduces at production geometry.
set -uo pipefail

if [[ ! -f Cargo.toml || ! -x scripts/e2e-env.sh ]]; then
  echo "error: run from the linux-lit repo root (need Cargo.toml + scripts/e2e-env.sh)" >&2
  exit 64
fi

SECS=330
# Production geometry (see the GEOMETRY note in the header). `none` opts out.
GEOMETRY=1920x1200
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
    --geometry) GEOMETRY="$2"; shift 2 ;;
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

# Resize cage's headless output to production geometry BEFORE the fuzz starts.
#
# Timing is the whole game here. The app auto-starts the fuzz on a fixed 6s
# timer (src/app/mod.rs, LIT_NAV_FUZZ), and the resize is only honored once
# cage has a mapped surface — so this must land inside that window, then leave
# time for the app's RESIZE_TICK to re-settle the layout and regenerate the page
# table. The first table is built at 720p and DROPPED on resize; driving before
# the regeneration means fuzzing a table that no longer matches the screen.
if [[ "$GEOMETRY" != none ]]; then
  if ! command -v wlr-randr >/dev/null 2>&1; then
    echo "[fuzz] ERROR: wlr-randr not found — cannot set production geometry." >&2
    echo "[fuzz]        install wlr-randr, or pass --geometry none to fuzz at 720p." >&2
    exit 70
  fi
  # cage mints its own socket in $RT; find it rather than assuming wayland-0
  # (the live session's socket may also be visible and grabbing it would drive
  # the user's real desktop).
  WL_SOCK=""
  for ((i = 0; i < 40; i++)); do
    for s in "$RT"/wayland-*; do
      [[ -S "$s" ]] || continue
      WL_SOCK="$(basename "$s")"
      break
    done
    [[ -n "$WL_SOCK" ]] && break
    sleep 0.1
  done
  if [[ -z "$WL_SOCK" ]]; then
    echo "[fuzz] ERROR: cage never created a wayland socket in $RT" >&2
    exit 70
  fi
  # Give the surface a moment to map; wlr-randr against a compositor with no
  # mapped client is accepted but the client never reconfigures.
  sleep 1
  OUT_NAME="$(XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY="$WL_SOCK" wlr-randr 2>/dev/null \
    | awk 'NF && $0 !~ /^[[:space:]]/ {print $1; exit}')"
  OUT_NAME="${OUT_NAME:-HEADLESS-1}"
  if XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY="$WL_SOCK" \
       wlr-randr --output "$OUT_NAME" --custom-mode "$GEOMETRY" >/dev/null 2>&1; then
    echo "[fuzz] output $OUT_NAME → $GEOMETRY" >&2
  else
    echo "[fuzz] ERROR: wlr-randr failed to set $GEOMETRY on $OUT_NAME" >&2
    exit 70
  fi
fi

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

# Report the geometry the run ACTUALLY achieved, and which pagination engine it
# exercised. Both are read from the log rather than assumed: wlr-randr's exit
# status says the request was accepted, not that the client reconfigured, and
# the whole point of the resize is to hit the STORED tables ("table hit") rather
# than a freshly generated one ("generated"). A run reporting `generated` at
# production geometry means the stored table's fingerprint didn't match — stale
# tables, a font change, or a work with none stored (see validate-play-pages).
if [[ "$GEOMETRY" != none ]]; then
  tv_h=$(grep -o "RESIZE_TICK: text_view.height changed [0-9]* -> [0-9]*" "$LOG" 2>/dev/null \
    | tail -1 | awk '{print $NF}')
  if [[ -n "$tv_h" ]]; then
    echo "[fuzz] geometry: $GEOMETRY → text_view.height=$tv_h (production is 1128)" >&2
    if [[ "$GEOMETRY" == 1920x1200 && "$tv_h" != 1128 ]]; then
      echo "[fuzz] WARNING: text_view.height=$tv_h != 1128 — the page grid does NOT" >&2
      echo "[fuzz]          match production, so boundary findings may not transfer." >&2
    fi
  else
    echo "[fuzz] WARNING: no RESIZE_TICK height change logged — the resize may not" >&2
    echo "[fuzz]          have reached the client; run likely fuzzed at 720p." >&2
  fi
fi
# Engine state AFTER the resize only. The app loads a table at startup (720p),
# then the resize invalidates it — so counting the whole log mixes the discarded
# startup table in with the one actually fuzzed and can report a reassuring
# "table hit" for a run that really used a generated table. Cut the log at the
# last RESIZE_TICK height change and report only what follows.
if [[ "$GEOMETRY" != none ]]; then
  resize_ln=$(grep -n "RESIZE_TICK: text_view.height changed" "$LOG" 2>/dev/null \
    | tail -1 | cut -d: -f1)
else
  resize_ln=1
fi
engine=$(tail -n "+${resize_ln:-1}" "$LOG" 2>/dev/null \
  | grep -o "PAGES: \(table hit\|generated\|fallback\)" | sort | uniq -c | tr '\n' ' ')
if [[ -n "$engine" ]]; then
  echo "[fuzz] pagination engine (post-resize): $engine" >&2
  if [[ "$GEOMETRY" != none && "$engine" != *"table hit"* ]]; then
    echo "[fuzz] NOTE: no stored-table hit at production geometry — this work's" >&2
    echo "[fuzz]       table is absent or stale for this fingerprint (see" >&2
    echo "[fuzz]       validate-play-pages). The fuzz ran on a generated table." >&2
  fi
fi
echo "[fuzz] failure summary by category:" >&2
grep "NAV_TEST: FAIL" "$LOG" 2>/dev/null \
  | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) /\1: /' \
  | sed -E 's/[0-9]+/N/g' | sort | uniq -c | sort -rn >&2 || true
echo "[fuzz] full log: $LOG  (e.g. rg 'NAV_TEST: FAIL' $LOG | rg -B2 'step=NNN')" >&2

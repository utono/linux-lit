#!/usr/bin/env bash
# run-fuzz-all-works.sh — run the navigation fuzz across EVERY Shakespeare Folger
# play and report any work whose canonical spreads leave the right column
# underfilled or empty (or any other hard NAV_TEST FAIL).
#
# Usage (always through the env wrapper):
#   # fast triage sweep (~150 steps/play, ~45 min total) — finds suspects:
#   ./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh [--secs-each N]
#   # thorough "whole-orchard" sweep (full ~1400-step prelude+body/play, ~6 h):
#   ./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh --secs-each 600
#
# Each work runs in its OWN isolated cage (private DB copy + log, headless
# offscreen, --headless-test marker) for --secs-each seconds. Steps run at
# ~400ms each, so the budget directly bounds coverage:
#   --secs-each 70  (default) ≈ 150 steps ≈ first ~17% of the coverage prelude —
#                   a FAST TRIAGE pass. Reaches the start/end-anchor actions and
#                   the first few scene boundaries; a FAIL=0 here means "no early
#                   failure", NOT "clean". Good for finding suspects across all
#                   plays in one ~45-min sitting.
#   --secs-each 600           ≈ the full 1400-step run (whole coverage prelude:
#                   every action from every act/scene boundary + mid-page anchors,
#                   plus the random/boundary-stress body). This is the THOROUGH
#                   whole-orchard sweep — a FAIL=0 here is a real clean bill of
#                   health. Costs ~600s × ~37 plays ≈ 6 h, so it is NOT the
#                   default; run it overnight or to confirm a fix across all works.
# Per-work results are tallied; a final summary lists every work with
# UNBALANCED / RIGHT-COLUMN-EMPTY / other FAILs. To confirm ONE play thoroughly
# without the 6-h sweep, use the single-work script: run-fuzz.sh --secs 600
# --start-work <ABBR>.
#
# Why this exists: a wrong-spread / underfilled-column bug can be work-specific
# (different act/scene structure, EPILOGUE vs no EPILOGUE, prose vs verse). One
# work passing doesn't prove the others do — so sweep them all.

set -uo pipefail

if [[ ! -f Cargo.toml || ! -x scripts/e2e-env.sh ]]; then
  echo "error: run from the linux-lit repo root" >&2; exit 64
fi

SECS_EACH=70
STOP_ON_FAIL=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --secs-each) SECS_EACH="$2"; shift 2 ;;
    --stop-on-first-fail) STOP_ON_FAIL=1; shift ;;
    *) echo "error: unknown option '$1'" >&2; exit 64 ;;
  esac
done

BIN=target/debug/linux-lit
DB_SRC="$HOME/utono/litdb/data/lit.db"
SUMMARY=/tmp/fuzz-all-works-summary.txt
: > "$SUMMARY"

# Free a per-work temp dir for real. Each cage run copies the 628M lit.db into
# $RT and spawns dbus/xdg-portal/at-spi daemons that keep $RT/cage.log open. A
# bare `rm -rf "$RT"` unlinks the dir but the kernel holds the space until those
# FDs close — so across ~37 works the tmpfs backing /tmp fills to 0 and every
# later run silently fails (ENOSPC, ~2 steps). Kill anything still holding an FD
# into $RT, THEN remove it, so space is actually reclaimed.
purge_rt() {
  local rt="$1" pid
  [ -n "$rt" ] && [ -d "$rt" ] || return 0
  for fd in /proc/[0-9]*/fd; do
    pid=$(basename "$(dirname "$fd")")
    if readlink "$fd"/* 2>/dev/null | grep -q "^$rt"; then
      kill -9 "$pid" 2>/dev/null
    fi
  done
  rm -rf "$rt" 2>/dev/null
}

# Preflight: bail before filling the tmpfs. /tmp is often a 32G tmpfs; each work
# needs ~630M ($RT copy of lit.db). Refuse to start if free space < 2G, and tell
# the operator how to recover — usually leftover dirs from a prior run whose
# daemons still pin them.
free_kb=$(df -Pk /tmp | awk 'NR==2{print $4}')
if [ "${free_kb:-0}" -lt 2097152 ]; then
  echo "[all-works] ERROR: /tmp has < 2G free ($((free_kb/1024))M). Each work needs ~630M." >&2
  echo "[all-works] Reclaim it (kills daemons pinning leftover dirs, then deletes them):" >&2
  echo "  .claude/skills/headless-test/free-test-space.sh" >&2
  exit 70
fi

echo "[all-works] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

# Shakespeare Folger plays only (exclude Ambrose/BBC variants and non-Shakespeare).
WORKS=$(sqlite3 "$DB_SRC" \
  "SELECT abbrev FROM works WHERE work_type='play' AND author='Shakespeare' \
   AND abbrev NOT LIKE '%-Amb' AND abbrev NOT LIKE '%-BBC' ORDER BY abbrev;")

total=0; flagged=0
for w in $WORKS; do
  total=$((total+1))
  RT="$(mktemp -d)"
  DB="$RT/lit.db"; LOG="$RT/nav.log"
  cp "$DB_SRC" "$DB"
  echo "[all-works] ($total) fuzzing $w for ${SECS_EACH}s…" >&2
  # Launch in its own process group; kill the group on timeout.
  setsid env -u WAYLAND_DISPLAY XDG_RUNTIME_DIR="$RT" GSK_RENDERER=cairo \
    WLR_BACKENDS=headless WLR_RENDERER=pixman \
    LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_NAV_FUZZ=1 \
    LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB" \
    LIT_START_WORK="$w" LIT_START_POS=50 \
    cage -- "$BIN" --headless-test >"$RT/cage.log" 2>&1 &
  pid=$!
  # Let it run, then tear down the whole group.
  sleep "$SECS_EACH"
  kill -- "-${pid}" 2>/dev/null || kill "$pid" 2>/dev/null
  sleep 1
  pkill -f "$BIN --headless-test" 2>/dev/null

  # `grep -c` on a missing/odd file can emit a multi-line "0\n0"; force a single
  # integer so the `(( ))` arithmetic below never sees a bad token.
  count() { grep -c "$1" "$LOG" 2>/dev/null | head -1 | tr -dc '0-9'; }
  steps=$(count "NAV_TEST: step"); steps=${steps:-0}
  unbal=$(count "UNBALANCED SPREAD"); unbal=${unbal:-0}
  empty=$(count "RIGHT COLUMN EMPTY"); empty=${empty:-0}
  fails=$(count "NAV_TEST: FAIL"); fails=${fails:-0}
  line="$w steps=$steps FAIL=$fails UNBALANCED=$unbal RIGHT_EMPTY=$empty"
  echo "$line" >> "$SUMMARY"
  if (( fails > 0 || unbal > 0 || empty > 0 )); then
    flagged=$((flagged+1))
    echo "[all-works]   FLAGGED: $line" >&2
    # Keep this work's log for inspection.
    cp "$LOG" "/tmp/fuzz-$w.log" 2>/dev/null || true
    # Desktop notification so you don't have to watch the terminal. The sweep runs
    # under dbus-run-session (its OWN bus) and often a temp XDG_RUNTIME_DIR, so
    # point notify-send at the user's REAL session bus at /run/user/<uid>/bus.
    if command -v notify-send >/dev/null 2>&1; then
      _userbus="/run/user/$(id -u)/bus"
      [ -S "$_userbus" ] && \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$_userbus" \
          notify-send -u critical -a "linux-lit fuzz" \
            "fuzz FAIL: $w" "FAIL=$fails UNBALANCED=$unbal RIGHT_EMPTY=$empty — see /tmp/fuzz-$w.log" \
            2>/dev/null || true
    fi
    if (( STOP_ON_FAIL )); then
      purge_rt "$RT"
      echo >&2
      echo "[all-works] === STOPPED at first failing work: $w ===" >&2
      echo "[all-works] FAIL categories in /tmp/fuzz-$w.log:" >&2
      grep "NAV_TEST: FAIL" "/tmp/fuzz-$w.log" 2>/dev/null \
        | sed -E 's/.*FAIL step=[0-9]+ ([A-Za-z]+) ([A-Z -]+):.*/  \1 \2/' \
        | sort | uniq -c | sort -rn >&2
      echo "[all-works] Fix the bug, rebuild, then re-run:" >&2
      echo "  cd ~/utono/linux-lit && cargo build" >&2
      echo "  ./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh --stop-on-first-fail --secs-each $SECS_EACH" >&2
      exit 1
    fi
  fi
  purge_rt "$RT"
done

echo >&2
echo "[all-works] === SUMMARY ($total works, $flagged flagged) ===" >&2
sort -t= -k3 -rn "$SUMMARY" >&2 || cat "$SUMMARY" >&2
echo "[all-works] full per-work summary: $SUMMARY; flagged work logs: /tmp/fuzz-<ABBR>.log" >&2

# Final desktop notification: clean sweep vs flagged.
if command -v notify-send >/dev/null 2>&1; then
  _userbus="/run/user/$(id -u)/bus"
  if [ -S "$_userbus" ]; then
    if (( flagged == 0 )); then
      DBUS_SESSION_BUS_ADDRESS="unix:path=$_userbus" \
        notify-send -a "linux-lit fuzz" "fuzz: ALL CLEAN ✓" "$total works, 0 failures" 2>/dev/null || true
    else
      DBUS_SESSION_BUS_ADDRESS="unix:path=$_userbus" \
        notify-send -u critical -a "linux-lit fuzz" "fuzz: $flagged/$total works flagged" "see $SUMMARY" 2>/dev/null || true
    fi
  fi
fi

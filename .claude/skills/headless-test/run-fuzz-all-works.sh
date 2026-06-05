#!/usr/bin/env bash
# run-fuzz-all-works.sh — run the navigation fuzz across EVERY Shakespeare Folger
# play and report any work whose canonical spreads leave the right column
# underfilled or empty (or any other hard NAV_TEST FAIL).
#
# Usage (always through the env wrapper):
#   ./scripts/e2e-env.sh .claude/skills/headless-test/run-fuzz-all-works.sh [--secs-each N]
#
# Each work runs in its OWN isolated cage (private DB copy + log, headless
# offscreen, --headless-test marker) for --secs-each seconds (default 70 — long
# enough for the coverage prelude to visit every act/scene boundary, where the
# underfilled-right-column cases live). Per-work results are tallied; a final
# summary lists every work with UNBALANCED / RIGHT-COLUMN-EMPTY / other FAILs.
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

  steps=$(grep -c "NAV_TEST: step" "$LOG" 2>/dev/null || echo 0)
  unbal=$(grep -c "UNBALANCED SPREAD" "$LOG" 2>/dev/null || echo 0)
  empty=$(grep -c "RIGHT COLUMN EMPTY" "$LOG" 2>/dev/null || echo 0)
  fails=$(grep -c "NAV_TEST: FAIL" "$LOG" 2>/dev/null || echo 0)
  line="$w steps=$steps FAIL=$fails UNBALANCED=$unbal RIGHT_EMPTY=$empty"
  echo "$line" >> "$SUMMARY"
  if (( fails > 0 || unbal > 0 || empty > 0 )); then
    flagged=$((flagged+1))
    echo "[all-works]   FLAGGED: $line" >&2
    # Keep this work's log for inspection.
    cp "$LOG" "/tmp/fuzz-$w.log" 2>/dev/null || true
    if (( STOP_ON_FAIL )); then
      rm -rf "$RT"
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
  rm -rf "$RT"
done

echo >&2
echo "[all-works] === SUMMARY ($total works, $flagged flagged) ===" >&2
sort -t= -k3 -rn "$SUMMARY" >&2 || cat "$SUMMARY" >&2
echo "[all-works] full per-work summary: $SUMMARY; flagged work logs: /tmp/fuzz-<ABBR>.log" >&2

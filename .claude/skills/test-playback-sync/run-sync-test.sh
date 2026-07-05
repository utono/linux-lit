#!/usr/bin/env bash
# run-sync-test.sh — real-MPV playback-sync page-turn timing test, headless.
#
# For each work: launch a PRIVATE mpv (--ao=null, no audio device needed) on a
# private socket, run the reader in an isolated cage against a DB COPY whose
# media path is rewritten to a symlink (so the derived socket can only be the
# test's own), then per boundary: x (next page), y (back — cursor lands on the
# previous page's last dialogue line), Tab (play), and assert the page turns
# when mpv crosses the start_time of the next page's first timestamped line.
#
#   PASS: PAGE_TURN logged and |time-pos(at turn) - next start_time| <= TOL
#   FAIL: no turn within TURN_TIMEOUT (the stall bug class), or turn far from
#         the expected start_time.
#
# Usage:
#   .claude/skills/test-playback-sync/run-sync-test.sh [--boundaries N] ABBR [ABBR...]
#   .claude/skills/test-playback-sync/run-sync-test.sh all-arkangel     # every -Arkangel play
#   .claude/skills/test-playback-sync/run-sync-test.sh all              # every timestamped play edition
#
# Requires: cage, wtype, mpv, python3, sqlite3. Never touches the live
# session: own XDG_RUNTIME_DIR, own DB copy, own mpv, own log; cleanup kills
# only its own PIDs.
set -uo pipefail

BOUNDARIES=2
TURN_TIMEOUT=60   # s to wait for the sync page turn after Tab
TOL_EARLY=1.8     # SYNC_GAP_PREROLL (1.5s deliberate early advance across an
                  # audio gap, see mpv/client.rs) + event jitter
TOL_LATE=2.5      # detection latency + line-poll cadence
WORKS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --boundaries) BOUNDARIES="$2"; shift 2 ;;
    *) WORKS+=("$1"); shift ;;
  esac
done
[[ ${#WORKS[@]} -gt 0 ]] || { echo "usage: run-sync-test.sh [--boundaries N] ABBR...|all-arkangel|all" >&2; exit 64; }

[[ -f Cargo.toml ]] || { echo "error: run from the linux-lit repo root" >&2; exit 64; }
BIN=target/debug/linux-lit
DB_SRC="$HOME/utono/litdb/data/lit.db"

# Expand selector tokens into work lists (every timestamped play edition).
expand_selector() {
  local filter="$1"
  sqlite3 "$DB_SRC" "SELECT DISTINCT w.abbrev FROM works w
    JOIN media_files m ON m.work_abbrev = w.abbrev
    WHERE w.work_type='play' $filter
      AND EXISTS (SELECT 1 FROM line_timestamps t WHERE t.media_id = m.id)
    ORDER BY w.abbrev;"
}
EXPANDED=()
for w in "${WORKS[@]}"; do
  case "$w" in
    all-arkangel) while IFS= read -r a; do EXPANDED+=("$a"); done < <(expand_selector "AND w.abbrev LIKE '%-Arkangel'") ;;
    all)          while IFS= read -r a; do EXPANDED+=("$a"); done < <(expand_selector "") ;;
    *)            EXPANDED+=("$w") ;;
  esac
done
WORKS=("${EXPANDED[@]}")
echo "[sync-test] ${#WORKS[@]} work(s): ${WORKS[*]}" >&2

echo "[sync-test] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

BASE="$(mktemp -d /tmp/synctest.XXXXXX)"
DB="$BASE/lit.db"
MDIR="$BASE/media"; mkdir -p "$MDIR"
echo "[sync-test] DB copy -> $DB" >&2
\cp -f "$DB_SRC" "$DB" || { echo "DB copy failed" >&2; exit 1; }

MPV_PID=""; CAGE_PID=""
cleanup_work() {
  [[ -n "$MPV_PID" ]] && kill "$MPV_PID" 2>/dev/null
  [[ -n "$CAGE_PID" ]] && { kill -- "-$CAGE_PID" 2>/dev/null; kill "$CAGE_PID" 2>/dev/null; }
  MPV_PID=""; CAGE_PID=""
}
cleanup() {
  cleanup_work
  # Reap anything still holding FDs into $BASE (dbus/portal daemons), then rm.
  if [[ -d "$BASE" ]]; then
    for fd in /proc/[0-9]*/fd; do
      if readlink "$fd"/* 2>/dev/null | grep -q "^$BASE"; then
        kill -9 "$(basename "$(dirname "$fd")")" 2>/dev/null
      fi
    done
    command rm -rf "$BASE" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

# --- helpers ---------------------------------------------------------------

# wait_log LOG OFFSET REGEX TIMEOUT_S -> prints first matching line, rc 1 on timeout
wait_log() {
  local log="$1" off="$2" re="$3" t="$4" line
  for ((i = 0; i < t * 10; i++)); do
    line=$(tail -c +"$((off + 1))" "$log" 2>/dev/null | grep -E -m1 "$re") && { echo "$line"; return 0; }
    sleep 0.1
  done
  return 1
}

mpv_time_pos() { # SOCKET -> prints time-pos or empty
  python3 - "$1" <<'EOF' 2>/dev/null
import json, socket, sys
s = socket.socket(socket.AF_UNIX); s.settimeout(2); s.connect(sys.argv[1])
s.sendall(b'{"command":["get_property","time-pos"]}\n')
buf = b''
import time
t0 = time.time()
while time.time() - t0 < 2:
    buf += s.recv(65536)
    for line in buf.decode(errors='replace').splitlines():
        try: d = json.loads(line)
        except Exception: continue
        if d.get('error') == 'success' and 'data' in d:
            print(d['data']); sys.exit(0)
EOF
}

key() { WAYLAND_DISPLAY="$WLSOCK" XDG_RUNTIME_DIR="$WRT" wtype "$@"; }

# --- per-work test ---------------------------------------------------------

declare -a RESULTS
FAILS=0

for W in "${WORKS[@]}"; do
  # Pick the BEST-ALIGNED media (most timestamps) — a work can carry several
  # media rows and the sparse ones (H5 media 2: two rows vs 2934 on media 63)
  # make the timing test meaningless.
  row=$(sqlite3 "$DB" "SELECT m.id||'|'||m.path FROM media_files m
        WHERE m.work_abbrev='$W'
          AND EXISTS(SELECT 1 FROM line_timestamps t WHERE t.media_id=m.id)
        ORDER BY (SELECT COUNT(*) FROM line_timestamps t WHERE t.media_id=m.id) DESC
        LIMIT 1;")
  if [[ -z "$row" ]]; then RESULTS+=("SKIP  $W (no timestamped media)"); continue; fi
  MID="${row%%|*}"; REAL="${row#*|}"
  if [[ ! -f "$REAL" ]]; then RESULTS+=("SKIP  $W (media file missing: $REAL)"); continue; fi

  # Rewrite ALL of this work's media rows into $MDIR so every derivable socket
  # is private; symlink only the timestamped one (others dangle -> not probed).
  sqlite3 "$DB" "UPDATE media_files SET path='$MDIR/'||work_abbrev||'-m'||id||'.m4b' WHERE work_abbrev='$W';"
  LINK="$MDIR/$W-m$MID.m4b"
  ln -sf "$REAL" "$LINK"
  # derive_socket_path: no ~/Music|~/rips|~/yt-dlp-mlj prefix -> author 'music'.
  SOCKET="/tmp/mpvsocket-music-$W-m$MID.m4b"
  command rm -f "$SOCKET"

  # Fully headless, hermetic mpv: no display access (env -u), no user config /
  # lua scripts / watch-later state (--no-config), null audio+video outputs,
  # never a window (cover art in an m4b is a video track — --no-video +
  # --force-window=no keep it windowless even if a VO initializes).
  env -u WAYLAND_DISPLAY -u DISPLAY mpv --no-config \
    --input-ipc-server="$SOCKET" \
    --ao=null --vo=null --no-video --force-window=no \
    --pause --no-terminal --really-quiet "$LINK" &
  MPV_PID=$!
  for ((i = 0; i < 50; i++)); do [[ -S "$SOCKET" ]] && break; sleep 0.1; done
  [[ -S "$SOCKET" ]] || { RESULTS+=("FAIL  $W (mpv socket never appeared)"); ((FAILS++)); cleanup_work; continue; }

  WRT="$BASE/xdg-$W"; mkdir -p "$WRT"; chmod 700 "$WRT"
  LOG="$BASE/$W.log"; : > "$LOG"
  setsid env -u WAYLAND_DISPLAY \
    XDG_RUNTIME_DIR="$WRT" GSK_RENDERER=cairo \
    WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
    LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_SYNC_TEST=1 \
    LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB" \
    LIT_START_WORK="$W" LIT_START_POS=0 \
    dbus-run-session -- cage -- "$BIN" --headless-test >"$BASE/$W-cage.log" 2>&1 &
  CAGE_PID=$!

  # Wait for reveal + MPV connect.
  if ! wait_log "$LOG" 0 "STARTUP: revealing vbox" 30 >/dev/null; then
    RESULTS+=("FAIL  $W (app never revealed)"); ((FAILS++)); cleanup_work; continue
  fi
  WLSOCK=$(basename "$(ls "$WRT"/wayland-* 2>/dev/null | grep -v lock | head -1)")
  if ! wait_log "$LOG" 0 "MPV: connected" 20 >/dev/null; then
    RESULTS+=("FAIL  $W (app never connected to test mpv)"); ((FAILS++)); cleanup_work; continue
  fi
  sleep 2  # focus settle; first wtype is dropped otherwise

  # Ensure playback sync is ON (s toggles; log line states the result).
  for ((i = 0; i < 3; i++)); do
    off=$(stat -c%s "$LOG"); key "s"
    line=$(wait_log "$LOG" "$off" "SYNC: (enabled|disabled)" 5) || continue
    [[ "$line" == *enabled* ]] && break
  done
  if [[ "${line:-}" != *enabled* ]]; then
    RESULTS+=("FAIL  $W (could not enable sync)"); ((FAILS++)); cleanup_work; continue
  fi

  wfail=0
  for ((b = 1; b <= BOUNDARIES; b++)); do
    off=$(stat -c%s "$LOG"); key "x"
    wait_log "$LOG" "$off" "PAGE_TURN|BOTTOM_CLIP" 5 >/dev/null
    off=$(stat -c%s "$LOG"); key "y"
    sleep 1
    # Last-line seek from the y landing gives the pre-boundary start time.
    seekline=$(tail -c +"$((off + 1))" "$LOG" | grep -E "SEEK: line=" | tail -1)
    S_LAST=""
    if [[ "$seekline" =~ start=([0-9.]+) ]]; then S_LAST="${BASH_REMATCH[1]}"; fi

    off=$(stat -c%s "$LOG"); key -k Tab
    if ! wait_log "$LOG" "$off" "MPV playback: playing" 5 >/dev/null; then
      RESULTS+=("FAIL  $W b$b (Tab did not start playback)"); ((wfail++)); ((FAILS++)); continue
    fi
    # A sync-driven page move logs PAGE_TURN (advance path), SYNC_SCENE_SCROLL
    # (scene-transition snap, which does NOT emit PAGE_TURN), or SYNC_PARA_TURN.
    turn=$(wait_log "$LOG" "$off" "PAGE_TURN:|SYNC_SCENE_SCROLL:|SYNC_PARA_TURN:" "$TURN_TIMEOUT")
    if [[ -z "$turn" ]]; then
      seg=$(tail -c +"$((off + 1))" "$LOG")
      # Suppression is only the verdict when it was never cleared: a single
      # suppressed event can race in before the playback-start clear.
      if grep -q "remaining=86" <<<"$seg" \
         && ! grep -q "cleared indefinite suppression" <<<"$seg"; then
        RESULTS+=("FAIL  $W b$b (CursorSync suppressed for the whole wait — indefinite-suppression bug)")
        ((wfail++)); ((FAILS++))
      elif [[ -z "$S_LAST" ]]; then
        # Untimestamped landing: no seek was sent, audio may be in dead air
        # (front matter) — sync being alive but idle isn't a failure.
        RESULTS+=("WARN  $W b$b (untimestamped landing; no timestamped audio crossed in ${TURN_TIMEOUT}s — not counted)")
      else
        RESULTS+=("FAIL  $W b$b (no page turn within ${TURN_TIMEOUT}s — sync stall)")
        ((wfail++)); ((FAILS++))
      fi
      key -k Tab; continue
    fi
    T=$(mpv_time_pos "$SOCKET")
    key -k Tab  # pause again
    if [[ -n "$S_LAST" && -n "$T" ]]; then
      S_NEXT=$(sqlite3 "$DB" "SELECT MIN(start_time) FROM line_timestamps
               WHERE media_id=$MID AND start_time > $S_LAST + 0.05;")
      if [[ -n "$S_NEXT" ]]; then
        d=$(python3 -c "print(round($T - $S_NEXT, 2))")
        ok=$(python3 -c "print(1 if -$TOL_EARLY <= $T - $S_NEXT <= $TOL_LATE else 0)")
        if [[ "$ok" == 1 ]]; then
          RESULTS+=("PASS  $W b$b (turn at ${T}s, next-line start ${S_NEXT}s, delta ${d}s)")
        else
          RESULTS+=("FAIL  $W b$b (turn at ${T}s vs next-line start ${S_NEXT}s, delta ${d}s)")
          ((wfail++)); ((FAILS++))
        fi
        continue
      fi
    fi
    # Untimestamped landing line (pending_advance path) — turn happened is the assertion.
    RESULTS+=("PASS  $W b$b (turned; no timed reference — untimestamped landing line)")
  done
  wait_log "$LOG" "$(stat -c%s "$LOG")" "MPV playback: paused" 3 >/dev/null
  # Preserve this work's app log for diagnosis when any of its boundaries
  # failed (the temp base is removed on exit).
  if (( wfail > 0 )); then
    mkdir -p /tmp/synctest-logs
    \cp -f "$LOG" "/tmp/synctest-logs/$W.log" 2>/dev/null
    \cp -f "$BASE/$W-cage.log" "/tmp/synctest-logs/$W-cage.log" 2>/dev/null
  fi
  cleanup_work
  sleep 1
done

echo
echo "=== playback-sync test results (boundaries=$BOUNDARIES) ==="
printf '%s\n' "${RESULTS[@]}"
echo "=== ${#RESULTS[@]} checks, $FAILS failures ==="
exit $((FAILS > 0 ? 1 : 0))

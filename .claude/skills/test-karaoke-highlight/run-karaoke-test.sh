#!/usr/bin/env bash
# run-karaoke-test.sh — headless karaoke (phrase-highlight) sweep test.
#
# Drives the REAL chain for any work: private mpv → TimePos → the phrase
# sweep in src/input/phrase_highlight.rs, with LIT_DEBUG_KARAOKE=1 so every
# decision the sweep makes is traced. Two things are checked per work:
#
#   1. LIVENESS — does the tint actually advance during playback?
#      PASS needs >= MIN_ADVANCES distinct `KARAOKE: paint` lines. Zero paints
#      while the heartbeat still ticks is the "karaoke stopped working" bug;
#      the gate-* / resolve-miss / span-miss trace lines say which cause.
#
#   2. CLAUSE WIDTH — how the grouper cut the spoken lines. Reported (not
#      asserted): the phrase-duration distribution, and the share of spans
#      under SHORT_SECS. This is the tuning number for a reader who takes a
#      long period — one with two or more relative clauses — as a single unit;
#      a high short-share means the sweep chops mid-clause.
#
# Usage:
#   .claude/skills/test-karaoke-highlight/run-karaoke-test.sh [opts] ABBR [ABBR...]
#   .claude/skills/test-karaoke-highlight/run-karaoke-test.sh all-prose
#
# Options:
#   --secs N       seconds of playback to observe per work (default 25)
#   --short N      "too short for a long-clause reader" threshold (default 1.5)
#   --start-line N line_mapping_id to start at (default: first timestamped)
#   --keep-log     copy the traced app log to /tmp/karaoke-logs/ even on PASS
#
# Requires: cage, wtype, mpv, python3, sqlite3. Never touches the live
# session: own XDG_RUNTIME_DIR, own DB copy, own mpv, own log.
set -uo pipefail

SECS=25
SHORT_SECS=1.5
MIN_ADVANCES=3
START_LINE=""
SKIP=0
AT_FRAC=""   # 0.0-1.0: measure this far into the timestamped text
KEEP_LOG=0
WORKS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --secs)       SECS="$2"; shift 2 ;;
    --short)      SHORT_SECS="$2"; shift 2 ;;
    --start-line) START_LINE="$2"; shift 2 ;;
    --skip)       SKIP="$2"; shift 2 ;;
    --at)         AT_FRAC="$2"; shift 2 ;;
    --keep-log)   KEEP_LOG=1; shift ;;
    *)            WORKS+=("$1"); shift ;;
  esac
done
[[ ${#WORKS[@]} -gt 0 ]] || {
  echo "usage: run-karaoke-test.sh [--secs N] [--short N] ABBR...|all-prose" >&2; exit 64; }

[[ -f Cargo.toml ]] || { echo "error: run from the linux-lit repo root" >&2; exit 64; }
BIN=target/debug/linux-lit
DB_SRC="$HOME/utono/litdb/data/lit.db"

# all-prose = every work with phrase data (the karaoke sweep needs spans, and
# phrase rows exist only where the backfill ran).
EXPANDED=()
for w in "${WORKS[@]}"; do
  case "$w" in
    all-prose)
      while IFS= read -r a; do EXPANDED+=("$a"); done < <(sqlite3 "$DB_SRC" "
        SELECT DISTINCT w.abbrev FROM works w
        JOIN work_media_associations wma ON wma.work_abbrev = w.abbrev
        WHERE EXISTS (SELECT 1 FROM phrase_timestamps p WHERE p.media_id = wma.media_id)
        ORDER BY w.abbrev;") ;;
    *) EXPANDED+=("$w") ;;
  esac
done
WORKS=("${EXPANDED[@]}")
echo "[karaoke] ${#WORKS[@]} work(s): ${WORKS[*]}" >&2

echo "[karaoke] building…" >&2
cargo build >&2 || { echo "build failed" >&2; exit 1; }

BASE="$(mktemp -d /tmp/karaoketest.XXXXXX)"
DB="$BASE/lit.db"
MDIR="$BASE/media"; mkdir -p "$MDIR"
\cp -f "$DB_SRC" "$DB" || { echo "DB copy failed" >&2; exit 1; }

MPV_PID=""; CAGE_PID=""
cleanup_work() {
  [[ -n "$MPV_PID" ]] && kill "$MPV_PID" 2>/dev/null
  [[ -n "$CAGE_PID" ]] && { kill -- "-$CAGE_PID" 2>/dev/null; kill "$CAGE_PID" 2>/dev/null; }
  MPV_PID=""; CAGE_PID=""
}
cleanup() {
  cleanup_work
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

wait_log() { # LOG OFFSET REGEX TIMEOUT_S
  local log="$1" off="$2" re="$3" t="$4" line
  for ((i = 0; i < t * 10; i++)); do
    line=$(tail -c +"$((off + 1))" "$log" 2>/dev/null | grep -E -m1 "$re") && { echo "$line"; return 0; }
    sleep 0.1
  done
  return 1
}

key() { WAYLAND_DISPLAY="$WLSOCK" XDG_RUNTIME_DIR="$WRT" wtype "$@"; }

declare -a RESULTS
FAILS=0

for W in "${WORKS[@]}"; do
  # Pick the media the APP would pick: `wma.priority DESC`, matching
  # queries.rs. Ordering by phrase count instead selects a different EDITION
  # than the one the app loads timestamps from — and each edition has its own
  # line_timestamps, so the reader seeks with one edition's times while the
  # other plays (PP: line 61 is 20.79s on m241 but 58.61s on m245). The log
  # tell is `MPV discovery: switching active media_id from Some(X) to Y`.
  # Phrase rows are still required — without them there is no sweep to test.
  row=$(sqlite3 "$DB" "SELECT m.id||'|'||m.path FROM media_files m
        JOIN work_media_associations wma ON wma.media_id = m.id
        WHERE wma.work_abbrev='$W'
          AND EXISTS(SELECT 1 FROM phrase_timestamps p WHERE p.media_id=m.id)
        ORDER BY wma.priority DESC, m.id ASC
        LIMIT 1;")
  if [[ -z "$row" ]]; then RESULTS+=("SKIP  $W (no media with phrase_timestamps)"); continue; fi
  MID="${row%%|*}"; REAL="${row#*|}"
  if [[ ! -f "$REAL" ]]; then RESULTS+=("SKIP  $W (media file missing: $REAL)"); continue; fi

  # Private socket, exactly as run-sync-test.sh does: rewrite this media's path
  # into $MDIR so derive_socket_path can only produce the test's own socket.
  sqlite3 "$DB" "UPDATE media_files SET path='$MDIR/m'||id||'.m4b' WHERE id=$MID;"
  LINK="$MDIR/m$MID.m4b"
  ln -sf "$REAL" "$LINK"
  SOCKET="/tmp/mpvsocket-music-m$MID.m4b"
  command rm -f "$SOCKET"

  # Start at a line that HAS phrase spans, so the sweep has data from tick one.
  if [[ -n "$START_LINE" ]]; then
    SEEK_T=$(sqlite3 "$DB" "SELECT MIN(start_time) FROM phrase_timestamps
             WHERE media_id=$MID AND line_mapping_id=$START_LINE;")
  elif [[ -n "$AT_FRAC" ]]; then
    # Fractional landing: the Nth percentile of this media's timestamped lines.
    # Front matter and a narrated table of contents sit at the very start, so
    # measuring clause width anywhere past them needs a positional jump — the
    # keyboard walk cannot cover thousands of TOC lines in a 25s run.
    SEEK_T=$(sqlite3 "$DB" "SELECT start_time FROM line_timestamps
             WHERE media_id=$MID ORDER BY start_time
             LIMIT 1 OFFSET CAST(
               (SELECT COUNT(*) FROM line_timestamps WHERE media_id=$MID) * $AT_FRAC AS INTEGER);")
  else
    SEEK_T=$(sqlite3 "$DB" "SELECT MIN(p.start_time) FROM phrase_timestamps p
             JOIN line_timestamps t ON t.line_mapping_id=p.line_mapping_id AND t.media_id=p.media_id
             WHERE p.media_id=$MID;")
  fi
  [[ -n "$SEEK_T" ]] || SEEK_T=0

  # NOTE on cursor/audio alignment: `resolve_spoken_idx` only walks +-8 work
  # lines around the CURSOR, so audio playing far from where the reader landed
  # produces a long run of `resolve-miss` (the sweep holds its last tint) until
  # sync drags the cursor into range. That is a harness artifact, not a bug —
  # so rather than force the cursor (LIT_START_POS is not honored on every load
  # path, e.g. DB-join works without a text_file), the audio is seeked to the
  # READER's own start line once the app reports it. See the `q` seek below.

  env -u WAYLAND_DISPLAY -u DISPLAY mpv --no-config \
    --input-ipc-server="$SOCKET" \
    --ao=null --vo=null --no-video --force-window=no \
    --pause --no-terminal --really-quiet ${AT_FRAC:+--start="$SEEK_T"} "$LINK" &
  MPV_PID=$!
  for ((i = 0; i < 50; i++)); do [[ -S "$SOCKET" ]] && break; sleep 0.1; done
  [[ -S "$SOCKET" ]] || { RESULTS+=("FAIL  $W (mpv socket never appeared)"); ((FAILS++)); cleanup_work; continue; }

  WRT="$BASE/xdg-$W"; mkdir -p "$WRT"; chmod 700 "$WRT"
  LOG="$BASE/$W.log"; : > "$LOG"
  # LIT_DEBUG_KARAOKE=1 is the whole point: it turns on the KARAOKE: trace.
  setsid env -u WAYLAND_DISPLAY \
    XDG_RUNTIME_DIR="$WRT" GSK_RENDERER=cairo \
    WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
    LIT_DEV=1 LIT_HEADLESS_TEST=1 LIT_SYNC_TEST=1 LIT_DEBUG_KARAOKE=1 \
    LIT_LOG_PATH="$LOG" LIT_DB_PATH="$DB" \
    LIT_START_WORK="$W" LIT_START_POS=0 \
    dbus-run-session -- cage -- "$BIN" --headless-test >"$BASE/$W-cage.log" 2>&1 &
  CAGE_PID=$!

  if ! wait_log "$LOG" 0 "STARTUP: revealing vbox" 30 >/dev/null; then
    RESULTS+=("FAIL  $W (app never revealed)"); ((FAILS++)); cleanup_work; continue
  fi
  # Cage's wayland socket can appear slightly after the app reveals; an `ls`
  # that runs too early resolves to nothing and EVERY wtype then goes nowhere
  # (the app renders fine and logs zero KEY: lines — MobyDick failed this way
  # while passing in isolation). Poll until it exists.
  # Test the PATH, not the basename: `basename ""` yields "." (a truthy,
  # useless value), so an empty-glob check on the basename silently produced a
  # bogus WAYLAND_DISPLAY and every wtype went nowhere.
  # Reset per work: WLSOCK is loop-scoped state, and a stale value from the
  # previous work points at a dead socket in a removed runtime dir — every
  # wtype then goes nowhere and the app logs zero KEY: lines.
  WLSOCK=""
  for ((i = 0; i < 40; i++)); do
    for cand in "$WRT"/wayland-*; do
      [[ -S "$cand" ]] || continue
      WLSOCK="${cand##*/}"; break
    done
    [[ -n "$WLSOCK" ]] && break
    sleep 0.25
  done
  # Prove keys actually reach THIS instance before relying on them: a focus or
  # socket problem is otherwise indistinguishable from a karaoke failure.
  for ((i = 0; i < 12; i++)); do
    off=$(stat -c%s "$LOG")
    key -k Escape
    wait_log "$LOG" "$off" "KEY:" 2 >/dev/null && break
    sleep 0.5
  done
  if [[ -z "$WLSOCK" ]]; then
    RESULTS+=("FAIL  $W (cage wayland socket never appeared — no keys deliverable)")
    ((FAILS++)); cleanup_work; continue
  fi
  if ! wait_log "$LOG" 0 "MPV: connected" 20 >/dev/null; then
    RESULTS+=("FAIL  $W (app never connected to test mpv)"); ((FAILS++)); cleanup_work; continue
  fi
  sleep 2  # focus settle; first wtype is dropped otherwise

  # Guard the wrong-edition trap: if the app ended up on a different media than
  # the one we wired the socket to, its line_timestamps belong to another
  # edition and every timing number below would be meaningless.
  if grep -qE "MPV discovery: switching active media_id from Some\([0-9]+\) to $MID\b" "$LOG"; then
    prev=$(grep -oE "switching active media_id from Some\([0-9]+\)" "$LOG" | tail -1 | grep -oE '[0-9]+')
    if [[ -n "$prev" && "$prev" != "$MID" ]]; then
      RESULTS+=("SKIP  $W (edition mismatch: app default media $prev, phrase media $MID — timings not comparable)")
      cleanup_work; continue
    fi
  fi

  # A VERSE work launches in cursor-line mode, where the sweep is deliberately
  # off (prose launches in karaoke); Alt+p flips that class's axis. Probe the
  # trace already written — do NOT block here, or the later `playing` line can
  # slip past the offset the play-toggle check stamps.
  if grep -qE 'KARAOKE: gate-off .*cursor_line_mode=true' "$LOG"; then
    key -M alt -k p -m alt; sleep 1
  fi

  # --- align cursor and narration -----------------------------------------
  #
  # The sweep only resolves a spoken line within +-8 work lines of the CURSOR,
  # so the two must start together. Two keys, two distinct jobs:
  #
  #   Q (JumpToNextDialogue) STEPS the cursor. In prose it advances line by
  #     line, but does NOT re-seek mpv when already on a dialogue line — so it
  #     emits no SEEK and must never be waited on for one.
  #   q (JumpToNextSpeaker) SEEKS mpv to the cursor's line. In prose with no
  #     speaker changes it does not move the cursor, which is what we want once
  #     the cursor is parked where we intend to measure.
  #
  # Front matter is untimestamped, and a `q` there logs
  # `NO_TIMESTAMP suppress=86400s` — an INDEFINITE sync suppression that also
  # stops the sweep — so keep seeking until a SEEK carries a real `start=`.
  skip_left=$SKIP
  for ((i = 0; i < skip_left; i++)); do
    key -M shift -k q -m shift
    sleep 0.15
  done

  aligned=0
  if [[ -n "$AT_FRAC" ]]; then
    # mpv already starts mid-book (--start), so there is nothing to align by
    # keyboard: playback sync PULLS the cursor onto the narrated line during
    # the measured run itself. Any q-walk here would drag the cursor back to
    # the front matter, and a play/pause pre-roll only risks leaving playback
    # in the wrong state — so do neither. The first seconds of the measured
    # window cover the convergence (the analysis tolerates a couple of early
    # stale paints; `misses` reports if convergence never happened).
    aligned=1
  else
    for ((i = 0; i < 60; i++)); do
      off=$(stat -c%s "$LOG"); key "q"
      line=$(wait_log "$LOG" "$off" "SEEK: line=" 3) || {
        # No seek: already on a timestamped dialogue line with mpv in position.
        # Step one line and retry rather than declaring failure.
        key -M shift -k q -m shift; sleep 0.15; continue
      }
      if [[ "$line" != *NO_TIMESTAMP* ]]; then aligned=1; break; fi
    done
  fi
  if (( aligned == 0 )); then
    RESULTS+=("SKIP  $W (no timestamped line reachable by Q from the start line)")
    # Preserve the log: an alignment failure is itself a finding (front matter
    # longer than the walk, or the key never reaching the app).
    mkdir -p /tmp/karaoke-logs
    \cp -f "$LOG" "/tmp/karaoke-logs/$W-align-fail.log" 2>/dev/null
    cleanup_work; continue
  fi
  sleep 1

  # Retry the play toggle: the first wtype after focus changes is occasionally
  # dropped, and a lost keypress is a harness flake, not a karaoke failure.
  started=0
  for ((i = 0; i < 4; i++)); do
    off=$(stat -c%s "$LOG")
    key "a"
    if wait_log "$LOG" "$off" "MPV playback: playing" 8 >/dev/null; then started=1; break; fi
    # Distinguish "key never arrived" (focus/socket problem) from "key arrived
    # but playback did not start" — only the first is worth re-sending.
    if ! grep -q 'KEY: name=a' "$LOG"; then sleep 2; else sleep 1; fi
  done
  off=$(stat -c%s "$LOG")
  if (( started == 0 )); then
    # Preserve the log — an early FAIL is exactly the case that needs one.
    mkdir -p /tmp/karaoke-logs
    \cp -f "$LOG" "/tmp/karaoke-logs/$W-playfail.log" 2>/dev/null
    RESULTS+=("FAIL  $W (play toggle did not start playback)"); ((FAILS++)); cleanup_work; continue
  fi
  sleep "$SECS"
  key "a"   # pause

  SEG="$BASE/$W.seg"
  tail -c +"$((off + 1))" "$LOG" > "$SEG"

  # --- verdict + clause-width profile -------------------------------------
  read -r VERDICT DETAIL < <(python3 - "$SEG" "$MIN_ADVANCES" "$SHORT_SECS" <<'EOF'
import re, sys
seg, min_adv, short = open(sys.argv[1], encoding='utf-8', errors='replace').read(), int(sys.argv[2]), float(sys.argv[3])
paints = re.findall(r'KARAOKE: paint .*?\(([\d.]+)s\).*?text="(.*?)"', seg)
durs = [float(d) for d, _ in paints]
def count(p): return len(re.findall(p, seg))
if len(paints) < min_adv:
    causes = []
    for tag in ('gate-off', 'gate-hold', 'gate-suppressed', 'resolve-miss',
                'span-miss', 'no-media', 'no-buffer-line'):
        n = count(r'KARAOKE: ' + tag)
        if n: causes.append(f'{tag}={n}')
    hb = count(r'KARAOKE: tick')
    print(f"FAIL advances={len(paints)}(<{min_adv})|heartbeats={hb}|" +
          (','.join(causes) or 'no-trace-lines'))
else:
    durs_s = sorted(durs)
    med = durs_s[len(durs_s)//2]
    shortn = sum(1 for d in durs if d < short)
    pct = round(100 * shortn / len(durs))
    words = [len(t.split()) for _, t in paints]
    # Surface misses even on PASS: a sweep that paints a few times but spends
    # most ticks holding a stale tint is "live" by the advance count and still
    # broken on screen.
    miss = count(r'KARAOKE: resolve-miss') + count(r'KARAOKE: span-miss')
    print(f"PASS advances={len(paints)}|median={med:.2f}s|"
          f"mean={sum(durs)/len(durs):.2f}s|max={max(durs):.2f}s|"
          f"under{short}s={pct}%|median_words={sorted(words)[len(words)//2]}|"
          f"misses={miss}")
EOF
) || { VERDICT=FAIL; DETAIL="analysis-error"; }

  if [[ "$VERDICT" == PASS ]]; then
    RESULTS+=("PASS  $W  ${DETAIL//|/  }")
  else
    RESULTS+=("FAIL  $W  ${DETAIL//|/  }")
    ((FAILS++))
  fi

  if [[ "$VERDICT" != PASS || $KEEP_LOG == 1 ]]; then
    mkdir -p /tmp/karaoke-logs
    \cp -f "$LOG" "/tmp/karaoke-logs/$W.log" 2>/dev/null
    grep -E 'KARAOKE:' "$LOG" > "/tmp/karaoke-logs/$W-karaoke.log" 2>/dev/null
  fi
  cleanup_work
  # Let the previous work's mpv + cage fully exit before the next launches.
  # A 1s gap was not enough in a 4-work sweep: the next reader occasionally
  # came up while the old instance still held its socket/compositor, and its
  # play toggle silently did nothing (works that PASS individually then FAIL
  # only in a batch). Wait for the socket to disappear, with a bounded fallback.
  for ((i = 0; i < 40; i++)); do
    [[ -S "$SOCKET" ]] || break
    sleep 0.25
  done
  sleep 2
done

echo >&2
echo "===== karaoke sweep =====" >&2
for r in "${RESULTS[@]}"; do echo "$r" >&2; done
echo >&2
if (( FAILS > 0 )); then
  echo "$FAILS failure(s). Traces: /tmp/karaoke-logs/<ABBR>-karaoke.log" >&2
  exit 1
fi
echo "all karaoke sweeps live." >&2
[[ $KEEP_LOG == 1 ]] && echo "Traces: /tmp/karaoke-logs/" >&2
exit 0

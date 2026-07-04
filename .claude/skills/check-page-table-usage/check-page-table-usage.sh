#!/usr/bin/env bash
# Which pagination engine is a RUNNING linux-lit instance using for a work?
# Reads each instance's /proc environ to find its real log (LIT_LOG_PATH >
# LIT_DEV -> dev log > release log), then reports the latest PAGES: evidence
# plus a lit.db cross-check. Read-only.
set -u
DB="$HOME/utono/litdb/data/lit.db"
REPO="$HOME/utono/linux-lit"
ABBR="${1:-}"

pids=$(pgrep -f 'target/(debug|release)/linux-lit' || true)
[ -z "$pids" ] && { echo "No running linux-lit instance."; exit 1; }

found_any=0
for pid in $pids; do
  [ -r "/proc/$pid/environ" ] || continue
  env_blob=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null) || continue
  kind="live"
  grep -q '^LIT_HEADLESS_TEST=' <<<"$env_blob" && kind="headless-test"
  log=$(grep '^LIT_LOG_PATH=' <<<"$env_blob" | head -1 | cut -d= -f2-)
  if [ -z "$log" ]; then
    if grep -q '^LIT_DEV=' <<<"$env_blob"; then
      log="$REPO/linux-lit-dev.log"
    else
      log="$REPO/linux-lit-release.log"
    fi
  fi
  no_table_env=""
  grep -q '^LIT_NO_PAGE_TABLE=' <<<"$env_blob" && no_table_env=" [LIT_NO_PAGE_TABLE set: table path disabled]"
  found_any=1
  echo "=== pid $pid ($kind)$no_table_env"
  echo "    log: $log"
  if [ ! -r "$log" ]; then
    echo "    log not readable — no verdict."
    continue
  fi

  # Latest load-status line (per work if given). Cleared on each app launch,
  # so this reflects THIS instance unless another instance shares the file.
  if [ -n "$ABBR" ]; then
    status=$(rg "PAGES: (table hit .* for $ABBR\$|generated .* for $ABBR .*|no table for $ABBR .*|fallback .*)" "$log" 2>/dev/null | tail -1)
  else
    status=$(rg "PAGES: (table hit|generated|no table|fallback)" "$log" 2>/dev/null | tail -1)
  fi
  lastpage=$(rg "PAGES: page [0-9]+/[0-9]+" "$log" 2>/dev/null | tail -1)

  # Stale-log guard: each launch truncates its log; when two instances shared
  # the same path the content interleaves. Detect it structurally: more than
  # one session start, or timestamps that jump backwards.
  stale_note=""
  sessions=$(rg -c "STARTUP: main entry" "$log" 2>/dev/null || echo 0)
  if [ "${sessions:-0}" -gt 1 ]; then
    stale_note="    WARNING: log holds $sessions interleaved sessions — evidence may belong to ANOTHER instance that shared this path (the stale-log trap). Restart the app for trustworthy evidence."
  elif rg -o '^\[ *([0-9]+)ms\]' -r '$1' "$log" 2>/dev/null | awk 'p>$1{exit 1}{p=$1}' ; then
    :
  else
    stale_note="    WARNING: log timestamps are non-monotonic — two instances interleaved in this file. Restart the app for trustworthy evidence."
  fi

  if [ -n "$lastpage" ]; then
    echo "    ENGINE: TABLE (navigation is table-driven)"
    echo "    last:   $lastpage"
    [ -n "$status" ] && echo "    load:   $status"
  elif grep -Eq "table hit|generated" <<<"${status:-}"; then
    echo "    ENGINE: TABLE (loaded/just generated; no page turn logged yet — press x or y and re-run to confirm)"
    echo "    load:   $status"
  elif [ -n "$status" ]; then
    echo "    ENGINE: LIVE (fallback)"
    echo "    load:   $status"
  else
    echo "    ENGINE: unknown — no PAGES: lines in this log (work not loaded since launch, non-play, or the log belongs to another instance)."
  fi
  [ -n "$stale_note" ] && echo "$stale_note"

  # Cross-check lit.db against the fingerprint the instance printed, if any.
  fp=$(rg -o 'fp=v1\|[^ ]*' "$log" 2>/dev/null | tail -1 | cut -d= -f2-)
  if [ -n "$ABBR" ]; then
    meta=$(sqlite3 "$DB" "SELECT page_count || ' pages, validated=' || validated || ', fp=' || layout_fingerprint FROM play_pages_meta WHERE work_abbrev='$ABBR';" 2>/dev/null)
    if [ -n "$meta" ]; then
      echo "    lit.db: $ABBR has stored table(s):"
      while IFS= read -r m; do echo "            $m"; done <<<"$meta"
      if [ -n "$fp" ] && ! grep -qF "$fp" <<<"$meta"; then
        echo "            NOTE: instance fingerprint $fp matches NONE of these -> live engine + lazy regeneration."
      fi
    else
      echo "    lit.db: no stored table for $ABBR (will generate on first 2-col load)."
    fi
  fi
done
[ "$found_any" = 1 ] || { echo "No inspectable linux-lit instance (permission?)."; exit 1; }

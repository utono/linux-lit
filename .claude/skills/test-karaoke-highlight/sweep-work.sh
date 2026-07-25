#!/usr/bin/env bash
# sweep-work.sh — walk ONE work's entire timestamped text through the karaoke
# runner, window by window, and report the phrase-width profile per window plus
# a whole-work roll-up.
#
# Where run-karaoke-test.sh samples a single position (--at 0.5), this covers
# every passage: it splits the work into N windows and runs one measured pass
# at each. Playback speed (--speed) is the accelerant — mpv advances through
# time-pos faster, so a given wall-clock second covers proportionally more
# text. Span timestamps are absolute, so speed does not distort the widths
# being measured (only how fast the sweep walks them).
#
# Usage:
#   .claude/skills/test-karaoke-highlight/sweep-work.sh TT [--windows N]
#       [--secs N] [--speed X] [--short N]
set -uo pipefail

WORK=""
WINDOWS=10
SECS=20
SPEED=3.0
SHORT=1.5
while [[ $# -gt 0 ]]; do
  case "$1" in
    --windows) WINDOWS="$2"; shift 2 ;;
    --secs)    SECS="$2"; shift 2 ;;
    --speed)   SPEED="$2"; shift 2 ;;
    --short)   SHORT="$2"; shift 2 ;;
    *)         WORK="$1"; shift ;;
  esac
done
[[ -n "$WORK" ]] || { echo "usage: sweep-work.sh ABBR [--windows N] [--secs N] [--speed X]" >&2; exit 64; }
[[ -f Cargo.toml ]] || { echo "error: run from the linux-lit repo root" >&2; exit 64; }

RUNNER=".claude/skills/test-karaoke-highlight/run-karaoke-test.sh"
OUT=/tmp/karaoke-sweep-$WORK
command rm -rf "$OUT"; mkdir -p "$OUT"

echo "[sweep] $WORK: $WINDOWS windows x ${SECS}s at ${SPEED}x" >&2

for ((w = 0; w < WINDOWS; w++)); do
  # Spread windows across the body, skipping the leading 2% (front matter and
  # any narrated table of contents) and the trailing 2%.
  frac=$(python3 -c "print(round(0.02 + (0.96 * $w / max(1,$WINDOWS-1)), 4))")
  echo "[sweep] window $((w+1))/$WINDOWS at $frac" >&2
  "$RUNNER" --secs "$SECS" --at "$frac" --speed "$SPEED" --short "$SHORT" \
    --keep-log "$WORK" >"$OUT/w$w.out" 2>&1
  grep -E "^(PASS|FAIL|SKIP)" "$OUT/w$w.out" | sed "s/^/  [$frac] /" >&2
  # Preserve this window's trace before the next run overwrites it.
  \cp -f "/tmp/karaoke-logs/$WORK-karaoke.log" "$OUT/w$w-karaoke.log" 2>/dev/null
done

# --- whole-work roll-up ----------------------------------------------------
echo >&2
echo "===== $WORK whole-work profile =====" >&2
cat "$OUT"/w*-karaoke.log 2>/dev/null > "$OUT/all-karaoke.log"
python3 - "$OUT/all-karaoke.log" "$SHORT" <<'EOF' >&2
import re, sys
txt = open(sys.argv[1], encoding='utf-8', errors='replace').read()
short = float(sys.argv[2])
# Dedupe: overlapping windows can paint the same span twice.
seen, durs, texts = set(), [], []
for d, t in re.findall(r'KARAOKE: paint .*?\(([\d.]+)s\).*?text="(.*?)"', txt):
    if t in seen:
        continue
    seen.add(t); durs.append(float(d)); texts.append(t)
if not durs:
    print("no paints captured"); sys.exit(1)
durs_s = sorted(durs)
n = len(durs)
pct = lambda p: durs_s[min(n-1, int(n*p))]
shortn = sum(1 for d in durs if d < short)
words = sorted(len(t.split()) for t in texts)
print(f"distinct phrases : {n}")
print(f"median           : {pct(0.5):.2f}s")
print(f"mean             : {sum(durs)/n:.2f}s")
print(f"p10 / p90        : {pct(0.10):.2f}s / {pct(0.90):.2f}s")
print(f"max              : {max(durs):.2f}s")
print(f"under {short}s      : {shortn} ({round(100*shortn/n)}%)")
print(f"median words     : {words[n//2]}")
print()
# A mid-clause cut is the failure mode that matters: a span ending in a word
# with no terminal punctuation, immediately followed by more of the clause.
openers = [t for t in texts
           if t and not re.search(r'[.,;:!?’”")\]-]$', t.strip())]
print(f"spans ending mid-clause (no terminal punctuation): "
      f"{len(openers)} ({round(100*len(openers)/n)}%)")
for t in openers[:8]:
    print(f"  [{t}]")
EOF

echo >&2
echo "traces: $OUT/" >&2

#!/usr/bin/env bash
# require-ui-review.sh — Claude Code Stop hook.
#
# Blocks the agent from ending the turn until it has examined the rendered UI
# and written target/ui/ui-review.md reflecting the latest screenshots. Exit 2
# blocks the stop and feeds the stderr back to Claude as the reason to keep
# working; exit 0 lets it finish.
#
# Wire it up in .claude/settings.json (see settings.snippet.json).

set -uo pipefail

INPUT=$(cat 2>/dev/null || true)

# Loop guard: if this stop hook already forced a continuation, don't re-block,
# otherwise a refusing agent could spin forever.
if command -v jq >/dev/null 2>&1; then
  [ "$(printf '%s' "$INPUT" | jq -r '.stop_hook_active // false')" = "true" ] && exit 0
fi

UIDIR=target/ui
REVIEW="$UIDIR/ui-review.md"

shopt -s nullglob
shots=("$UIDIR"/*.png)
suspect_files=("$UIDIR"/*.suspects.txt)
shopt -u nullglob

# Filter out the *_annotated.png copies from the "must reference" set.
real_shots=()
for s in "${shots[@]}"; do
  case "$s" in *_annotated.png) ;; *) real_shots+=("$s");; esac
done

# No UI was captured this session -> nothing to gate on.
[ ${#real_shots[@]} -eq 0 ] && exit 0

block() {
  {
    echo "BLOCKED: the UI was rendered but not reviewed."
    echo "$1"
    echo
    echo "Open each screenshot in $UIDIR (including the *_annotated.png"
    echo "overlays), then write $REVIEW. It must:"
    echo "  - reference every screenshot by filename"
    echo "  - state what is visible in each (window title, panels, text)"
    echo "  - address every entry in *.suspects.txt by name, saying whether"
    echo "    the pixels show a real layout bug or a false positive"
    echo "The turn cannot complete until that review exists and is current."
  } >&2
  exit 2
}

[ -f "$REVIEW" ] || block "Missing $REVIEW."

# Review must be at least as new as the newest screenshot.
newest=$(ls -t "${real_shots[@]}" | head -1)
if [ "$newest" -nt "$REVIEW" ]; then
  block "$REVIEW is older than $newest — it predates the latest render."
fi

# Every screenshot must be referenced.
for s in "${real_shots[@]}"; do
  base=$(basename "$s")
  grep -qF "$base" "$REVIEW" || block "Review never mentions $base."
done

# Every machine-detected suspect must be addressed by its identifier.
for sf in "${suspect_files[@]}"; do
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    tok=${line%%$'\t'*}
    grep -qF "$tok" "$REVIEW" || block "Review must address layout suspect: $tok"
  done < "$sf"
done

exit 0

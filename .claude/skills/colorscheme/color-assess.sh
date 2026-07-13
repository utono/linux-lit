#!/usr/bin/env bash
# colorscheme skill backing script — assess/edit/create reader colors with a
# contrast gate that reuses the app's OWN contrast machinery (via the
# #[cfg(test)] `contrast_report_harness` in src/theme.rs), so the verdict
# matches production.
#
# Usage:
#   color-assess.sh assess [--theme NAME] KEY #HEX [KEY #HEX ...]
#   color-assess.sh edit   --theme NAME  KEY #HEX [KEY #HEX ...] [--force]
#   color-assess.sh create --theme NAME  KEY #HEX [KEY #HEX ...]   (writes only after caller confirms)
#
# Friendly KEYs -> themes-unified.json linux-lit keys:
#   root->root_color  phrase->phrase_highlight_bg  cursor-line->cursor_line_bg
#   gloss->reader_gloss  gloss-cursor->reader_gloss_cursor  vocab-fg->vocab_fg
#   search->(search-highlight pair, derived from phrase_highlight_bg)
#
# assess is read-only. edit refuses on a FAIL unless --force. create writes only
# when invoked with --write (the skill derives+reports first, then re-invokes
# with --write after the user approves). All writes back up the JSON, touch ONLY
# the named theme's linux-lit block, and validate JSON after.
set -euo pipefail

REPO="$HOME/utono/linux-lit"
THEMES_JSON="$HOME/utono/themes/.config/themes/themes-unified.json"

MODE="${1:-}"; shift || true
[ -n "$MODE" ] || { echo "usage: color-assess.sh assess|edit|create [--theme NAME] KEY #HEX ..." >&2; exit 2; }

THEME="default"; FORCE=0; WRITE=0
declare -a PAIRS=()   # "friendlykey=#hex"
while [ $# -gt 0 ]; do
  case "$1" in
    --theme) THEME="${2:?--theme needs a name}"; shift 2;;
    --force) FORCE=1; shift;;
    --write) WRITE=1; shift;;
    -*) echo "unknown flag: $1" >&2; exit 2;;
    *)
      key="$1"; val="${2:?key '$1' needs a #hex or rgba(...) value}"
      PAIRS+=("$key=$val"); shift 2;;
  esac
done
[ "${#PAIRS[@]}" -gt 0 ] || { echo "no KEY #HEX pairs given" >&2; exit 2; }

# Build the harness spec (comma-joined key=val), map validated below by the
# harness (unknown keys -> CONTRAST_ERROR). Keep rgba(...) values intact.
spec=""
for p in "${PAIRS[@]}"; do spec="${spec:+$spec,}$p"; done

# --- Contrast gate: run the app's harness against the resolved theme ---
report="$(cd "$REPO" && LIT_CONTRAST_THEME="$THEME" LIT_CONTRAST_COLORS="$spec" \
  cargo test contrast_report_harness -- --nocapture 2>/dev/null | grep -E '^CONTRAST')"

echo "=== contrast report (theme: $THEME) ==="
echo "$report" | grep -E '^CONTRAST ' | while read -r _ key vs surface ratio floor verdict; do
  printf '  %-14s vs %-4s  %6s / %-5s  %s\n' "$key" "$surface" "$ratio" "$floor" "$verdict"
done
summary_line="$(echo "$report" | grep -E '^CONTRAST_SUMMARY' || true)"
nfail="$(echo "$summary_line" | awk '{print $3}')"; nfail="${nfail:-0}"
if echo "$report" | grep -q '^CONTRAST_ERROR'; then
  echo "$report" | grep '^CONTRAST_ERROR' >&2
  echo "aborting: unknown color key(s). valid: root phrase cursor-line gloss gloss-cursor vocab-fg search" >&2
  exit 2
fi
if [ "$nfail" = "0" ]; then echo "SUMMARY: PASS (all pairs clear their floor)"; else echo "SUMMARY: FAIL ($nfail pair(s) below floor)"; fi

# assess stops here (read-only).
[ "$MODE" = "assess" ] && exit $([ "$nfail" = "0" ] && echo 0 || echo 1)

# edit/create: gate on contrast unless forced.
if [ "$nfail" != "0" ] && [ "$FORCE" != "1" ]; then
  echo "refusing to write: $nfail pair(s) fail contrast. Re-run with --force to write anyway (the app will rewrite failing colors at load via ensure_gloss_color_min)." >&2
  exit 1
fi

# create derives+reports first; it only writes when re-invoked with --write.
if [ "$MODE" = "create" ] && [ "$WRITE" != "1" ]; then
  echo "create: derived scheme reported above. Re-run with --write (after approval) to persist to theme '$THEME'."
  exit 0
fi

# --- Write path (edit, or create --write): parse->modify->serialize, backed up ---
[ -f "$THEMES_JSON" ] || { echo "themes JSON not found: $THEMES_JSON" >&2; exit 2; }
cp -f "$THEMES_JSON" "$THEMES_JSON.bak"

# Map friendly keys -> JSON keys and write into <theme>.linux-lit via python (safe JSON edit).
python3 - "$THEMES_JSON" "$THEME" "$spec" <<'PY'
import json, sys
path, theme, spec = sys.argv[1], sys.argv[2], sys.argv[3]
MAP = {
    "root": "root_color", "phrase": "phrase_highlight_bg",
    "cursor-line": "cursor_line_bg", "gloss": "reader_gloss",
    "gloss-cursor": "reader_gloss_cursor", "vocab-fg": "vocab_fg",
}  # 'search' is derived from phrase_highlight_bg at load — not a stored key.
d = json.load(open(path))
themes = d.get("themes", d)
if theme not in themes:
    themes[theme] = {"meta": {}, "linux-lit": {}}
ll = themes[theme].setdefault("linux-lit", {})
changed = []
for pair in spec.split(","):
    if not pair or "=" not in pair:
        continue
    k, v = pair.split("=", 1)
    k, v = k.strip(), v.strip()
    if k == "search":
        continue  # derived, nothing to store
    jk = MAP.get(k)
    if not jk:
        print(f"skip unknown key {k}", file=sys.stderr); continue
    ll[jk] = v
    changed.append(f"{jk}={v}")
json.dump(d, open(path, "w"), indent=2, ensure_ascii=False)
open(path, "a").write("\n")
print("WROTE theme '%s' linux-lit: %s" % (theme, ", ".join(changed)))
PY

# Validate JSON after write; restore backup on failure.
if ! python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$THEMES_JSON"; then
  echo "post-write JSON invalid — restoring backup" >&2
  cp -f "$THEMES_JSON.bak" "$THEMES_JSON"
  exit 1
fi
echo "OK: $THEMES_JSON updated (backup at $THEMES_JSON.bak)."

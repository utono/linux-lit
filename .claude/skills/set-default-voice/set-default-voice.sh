#!/usr/bin/env bash
# Set or report linux-lit's default male/female ElevenLabs voice.
#
#   set-default-voice.sh                          # print current defaults + usage
#   set-default-voice.sh <male|female> <ID> [Name] # set default for a gender
#
# Updates BOTH the const in src/elevenlabs.rs and the live voice_catalog rows in
# lit.db. The MCP voice lookup is done by the agent, not here (see SKILL.md).
set -euo pipefail

REPO="$HOME/utono/linux-lit"
SRC="$REPO/src/elevenlabs.rs"
DB="$HOME/utono/litdb/data/lit.db"

# const name -> (gender, catalog age band) for the two gender defaults
MALE_CONST="BENEDICK_VOICE_ID"
FEMALE_CONST="IMOGEN_VOICE_ID"
FEMALE_AGE_MIN=20; FEMALE_AGE_MAX=30   # female default band (also all-prose)
MALE_AGE_MIN=26;   MALE_AGE_MAX=34     # male default band

const_id() {  # $1 = const name -> prints the current voice id from the source
  rg -o "pub const $1: &str = \"([^\"]+)\"" -r '$1' "$SRC"
}

db_rows() {   # $1 = gender -> prints "voice_id|role|label" rows from lit.db
  [ -f "$DB" ] || { echo "  (lit.db not found at $DB)"; return; }
  sqlite3 "$DB" \
    "SELECT voice_id||'  '||role||'  '||COALESCE(label,'') FROM voice_catalog
     WHERE gender='$1' ORDER BY age_min, role;" 2>/dev/null \
    | sed 's/^/  /' || echo "  (voice_catalog query failed)"
}

print_defaults() {
  echo "Current default voices (linux-lit):"
  echo
  echo "MALE   const $MALE_CONST = $(const_id "$MALE_CONST")"
  echo "       lit.db voice_catalog (gender=male):"
  db_rows male
  echo
  echo "FEMALE const $FEMALE_CONST = $(const_id "$FEMALE_CONST")  (also all-prose narrator)"
  echo "       lit.db voice_catalog (gender=female):"
  db_rows female
  echo
  echo "Usage: set-default-voice.sh <male|female> <VOICE_ID> [Name]"
  echo "  (resolve VOICE_ID via the ElevenLabs MCP server first — see SKILL.md)"
}

# --- no args: report and exit ---
if [ $# -eq 0 ]; then
  print_defaults
  exit 0
fi

[ $# -ge 2 ] || { echo "error: need <male|female> <VOICE_ID> [Name]" >&2; exit 2; }
GENDER="$1"; NEW_ID="$2"; NAME="${3:-}"

case "$GENDER" in
  male)   CONST="$MALE_CONST";   AGE_MIN=$MALE_AGE_MIN;   AGE_MAX=$MALE_AGE_MAX ;;
  female) CONST="$FEMALE_CONST"; AGE_MIN=$FEMALE_AGE_MIN; AGE_MAX=$FEMALE_AGE_MAX ;;
  *) echo "error: gender must be 'male' or 'female', got '$GENDER'" >&2; exit 2 ;;
esac

OLD_ID="$(const_id "$CONST")"
echo "Setting $GENDER default: $CONST  $OLD_ID -> $NEW_ID"

# 1. Edit the const value in src/elevenlabs.rs (value only; identifier unchanged).
command sed -i -E \
  "s|(pub const $CONST: &str = \")[^\"]+(\";)|\1$NEW_ID\2|" "$SRC"
echo "  src/elevenlabs.rs updated"

# 2. Update the live voice_catalog rows for that gender's default band.
if [ -f "$DB" ]; then
  CHANGED=$(sqlite3 "$DB" \
    "UPDATE voice_catalog SET voice_id='$NEW_ID'
       WHERE gender='$GENDER' AND age_min=$AGE_MIN AND age_max=$AGE_MAX;
     SELECT changes();")
  echo "  lit.db voice_catalog rows updated: $CHANGED"
else
  echo "  WARNING: $DB not found — live DB NOT updated (verse will use old voice)" >&2
fi

echo
echo "Next: cargo build && cargo test --bins elevenlabs resolve"
[ -n "$NAME" ] && echo "If the character NAME changed to '$NAME', also rename the const" \
  "$CONST + its uses/comments/tests + docs by hand (see SKILL.md)."
exit 0

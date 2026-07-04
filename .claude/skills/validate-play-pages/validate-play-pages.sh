#!/usr/bin/env bash
# Read-only audit of lit.db play_pages tables (structural invariants only —
# fit/determinism need live geometry and are enforced at generation time).
set -euo pipefail
DB="$HOME/utono/litdb/data/lit.db"
ABBR="${1:---all}"

works() {
  if [[ "$ABBR" == "--all" ]]; then
    sqlite3 "$DB" "SELECT DISTINCT work_abbrev FROM play_pages_meta ORDER BY 1;"
  else
    echo "$ABBR"
  fi
}

fail=0
any=0
for w in $(works); do
  for fp in $(sqlite3 "$DB" "SELECT layout_fingerprint FROM play_pages_meta WHERE work_abbrev='$w';"); do
    any=1
    meta=$(sqlite3 -separator ' | ' "$DB" \
      "SELECT page_count, generated_at, validated FROM play_pages_meta
       WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    rowcount=$(sqlite3 "$DB" \
      "SELECT count(*) FROM play_pages WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    # sanity: split/end ordering per row, join sanity against line_mapping
    bad_rows=$(sqlite3 "$DB" "
      SELECT count(*) FROM play_pages p
      LEFT JOIN line_mapping ls ON ls.id = p.left_start_id
      LEFT JOIN line_mapping le ON le.id = p.end_id
      WHERE p.work_abbrev='$w' AND p.layout_fingerprint='$fp'
        AND (ls.id IS NULL OR le.id IS NULL
             OR ls.work_abbrev NOT IN (SELECT work_abbrev FROM line_mapping WHERE id=p.left_start_id)
             OR p.end_id < p.left_start_id);")
    # coverage: page N+1's left_start_id must be > page N's end_id (ids are
    # document-ordered within a work), with no page_no gaps.
    #
    # EXCEPTION (mirrors validate_spreads in src/input/page_table.rs): the
    # FINAL page is the canonical G/final-x anchor (last_page_top),
    # forward-pulled to fill both columns, and is deliberately allowed to
    # overlap the chain's last natural page (documented "benign seam" — a `y`
    # from the end doesn't tile with it). That pair only passes if it's still
    # strictly progressing: final.left_start > prev.left_start AND
    # final.end >= prev.end. Any OTHER consecutive pair overlapping is a real
    # coverage bug.
    max_page_no=$(sqlite3 "$DB" \
      "SELECT max(page_no) FROM play_pages WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    overlaps=$(sqlite3 "$DB" "
      WITH o AS (SELECT page_no, left_start_id, end_id FROM play_pages
                 WHERE work_abbrev='$w' AND layout_fingerprint='$fp' ORDER BY page_no)
      SELECT count(*) FROM o a JOIN o b ON b.page_no = a.page_no + 1
      WHERE b.left_start_id <= a.end_id
        AND NOT (
          b.page_no = $max_page_no
          AND b.left_start_id > a.left_start_id
          AND b.end_id >= a.end_id
        );")
    # page_no gaps: a contiguous run from min..max has (max-min+1) == count.
    # Any mismatch means a missing or duplicated page_no.
    gaps=$(sqlite3 "$DB" "
      SELECT (MAX(page_no) - MIN(page_no) + 1) - COUNT(*)
      FROM play_pages WHERE work_abbrev='$w' AND layout_fingerprint='$fp';")
    status=PASS
    [[ "$bad_rows" != "0" || "$overlaps" != "0" || "$gaps" != "0" ]] && { status=FAIL; fail=1; }
    [[ "$rowcount" != "$(echo "$meta" | cut -d'|' -f1 | tr -d ' ')" ]] && { status=FAIL; fail=1; }
    echo "$w [$status] fp=$fp rows=$rowcount meta=($meta) bad_rows=$bad_rows overlaps=$overlaps gaps=$gaps"
  done
done

if [[ "$any" == "0" ]]; then
  echo "No play_pages_meta rows found (nothing generated yet, or ABBR='$ABBR' has no table)."
fi

echo
echo "NOTE: db_fingerprint staleness and fit/determinism are enforced by the"
echo "app at load/generation time (a stale table logs 'PAGES: fallback' and is"
echo "regenerated on next load). To force regeneration:"
echo "  sqlite3 \"$DB\" \"DELETE FROM play_pages WHERE work_abbrev='<ABBR>'; DELETE FROM play_pages_meta WHERE work_abbrev='<ABBR>';\""
exit $fail

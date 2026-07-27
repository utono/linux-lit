---
name: validate-play-pages
description: Use when auditing lit.db play_pages tables after a litdb re-import, font or layout change, or suspected play pagination drift — checks structural invariants (coverage, ordering, row/meta consistency) read-only and reports PASS/STALE/FAIL per work
argument-hint: <ABBR> | --all
---

Run the backing script (read-only; never writes lit.db):

    .claude/skills/validate-play-pages/validate-play-pages.sh --all
    .claude/skills/validate-play-pages/validate-play-pages.sh MND

Interpretation:
- **PASS** — rows are structurally sound for that (work, layout fingerprint).
- **FAIL** — overlapping/missing/malformed rows: delete that work's rows (the
  script prints the command) and let the app regenerate on next load.
- Staleness vs. the current text (db_fingerprint) and the geometric fit/
  determinism invariants are enforced by the app itself at load/generation —
  a stale table logs `PAGES: fallback (...)` in the app log and is replaced
  on the next load of that play at the pinned layout.

The fingerprint string is human-readable
(`v5|family|size|ascent|descent|charw|WxH|spacing|margins|cols|top_spacer|view_h`),
so when a table is unexpectedly missing you can see which layout input moved.
`WxH` is the toplevel WINDOW size; `view_h` is the CARD's `text_view.height()`,
which is what the fit check validates against (added in `v5`, 2026-07-27 — see
clip-prevention.md #12).

**PASS does not mean "fits".** This script checks STRUCTURE (coverage,
ordering, overlaps), not whether a column fits the CURRENT card. A table can
report PASS and still render a clipped last line; the decisive tell for that is
a `BOTTOM_CLIP_EXACT` log line whose `total` exceeds `widget_h` with `clip=0`.

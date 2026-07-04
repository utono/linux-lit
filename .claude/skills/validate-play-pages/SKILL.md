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
(`v1|family|size|ascent|descent|charw|WxH|spacing|margins|cols`), so when a
table is unexpectedly missing you can see which layout input moved.

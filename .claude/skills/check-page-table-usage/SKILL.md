---
name: check-page-table-usage
description: Use when checking whether a currently running linux-lit instance is using the pinned play_pages tables or the live pagination engine for a work — e.g. after restarting crll, after a font/resolution change, or when page turns feel like they recomputed
argument-hint: <ABBR>
---

Run the backing script (read-only; inspects processes, logs, and lit.db):

```bash
.claude/skills/check-page-table-usage/check-page-table-usage.sh MND-Arkangel
```

```bash
.claude/skills/check-page-table-usage/check-page-table-usage.sh
```

With an abbrev it filters the load evidence to that work and cross-checks
lit.db's stored tables; without one it reports the latest engine evidence per
running instance.

How it decides — per running instance (live and headless-test are labeled
separately) it resolves the instance's REAL log from `/proc/<pid>/environ`
(`LIT_LOG_PATH` beats `LIT_DEV` → `linux-lit-dev.log` beats the release log;
never trust a tail of the wrong log), then:

- `PAGES: page N/M …` lines → **TABLE**: navigation itself is table-driven.
  This is the strongest signal; the load-time line can go stale (toggling
  translations or scroll mode silently disables the table per keypress).
- `PAGES: table hit (N pages)` with no page line yet → table loaded; press
  `x` or `y` once and re-run to confirm turns use it.
- `PAGES: fallback (…)` / `no table for <ABBR> fp=…` → **LIVE** engine, with
  the reason (fingerprint mismatch, stale db_fingerprint, unmappable rows).
- `LIT_NO_PAGE_TABLE` in the instance's environment is reported — the table
  path is force-disabled regardless of the log.

The lit.db cross-check lists the work's stored tables and warns when the
instance's printed fingerprint matches none of them (that instance will run
live and lazily regenerate at its own geometry).

Interpretation notes: the log is cleared on every app launch, so evidence is
from the CURRENT session; "unknown" usually means the work hasn't been loaded
since launch, isn't a two-column play, or two instances share one log file
(the stale-instance trap — prefer instances whose environ carries
`LIT_LOG_PATH`). Shared/contaminated logs are detected automatically —
multiple `STARTUP: main entry` lines or non-monotonic timestamps print a
WARNING that the evidence may belong to another instance; restart the app
and re-run for a clean verdict.

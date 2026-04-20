# Page Backward (`y`) Sometimes Fails Silently

## Symptom

Pressing `y` to turn back one page sometimes does nothing — no visible
change, no log entry, no error. Reproducible by:

1. Opening a work that resumes mid-book from a saved position, then pressing
   `y` immediately.
2. After pressing `y` repeatedly to page back through all the history the
   current session had accumulated, the next `y` silently does nothing.

## Root Cause

`page_backward` in `src/input/navigation.rs` relies entirely on
`state.page_history` — a stack populated only when the user (or MPV auto
advance) turns a page forward. Three ways the stack can be empty while
there is still content above the current page:

- The work was loaded with a `saved_line` mid-book (`display_work_at` calls
  `state.page_history.clear()` before setting `page_top_line`).
- The user used `jump_to_line` or a concordance target, which pushes only
  the page they came from, not pages preceding it.
- The user paged forward N times and then paged back N+ times; the stack is
  now empty, yet earlier pages exist in the work.

In all three cases, `page_history.pop()` returned `None` and the function
returned silently.

Log evidence (from `linux-lit-dev.log`):

```
KEY: name=y ctrl=false shift=false alt=false
KEY: name=y ctrl=false shift=false alt=false
KEY: name=y ctrl=false shift=false alt=false
```

Three `y` presses, zero `PAGE_BWD` entries.

## Fix

When the history stack is empty, fall back to computing a previous page by
stepping back one viewport-height (`lines_per_page(state)`) from the current
`page_top_line`. The rest of `page_backward` then runs normally, finding
the next dialogue line from that point and backing up for speaker headers.
If `page_top_line == 0` we genuinely are at the start and still return, but
now with a log entry.

## Files Changed

- `src/input/navigation.rs` — `page_backward` has a history-empty fallback
  that computes `fallback_top = page_top_line.saturating_sub(lpp)` and logs
  the event so future failures are easier to diagnose.

## How to Verify

1. Open a work with a mid-book saved position.
2. Press `y` immediately — the page should step backward by roughly one
   viewport's worth of content.
3. Page forward several times with `x`, then back with `y` until nothing
   happens. Press `y` again — you should continue moving back rather than
   getting stuck.
4. Keep pressing `y` until you reach `page_top_line == 0`; the next press
   should log `PAGE_BWD: no history and at start of work` and stop.

# Virtual Page Prefix on Line Label

## Problem

The bottom-of-screen overlay label (`page_line_label` in `src/app.rs`) currently shows
only a line identifier for prose works (e.g. `1234`). There is no indication of which
viewport-page the reader is on. Users want a page prefix so the label reads
`<page> - <line_id>` (e.g. `1 - 1234`, `34 - 5678`).

Plays already show a meaningful citation (`I.i.15`, `Prologue 31`); they need no change.

## Goals

- Prose (and any non-play work) shows `<page> - <line_id>` in the bottom overlay.
- Plays remain exactly as they are today.
- Page number reflects the viewport-page that contains the **currently highlighted line**,
  counted from line 0 of the work as page 1.
- Page is recomputed on every label refresh, regardless of which navigation key
  triggered the refresh (j/k, comma/q, gg/G, MPV sync, scroll).
- Page numbering matches what j-presses from the start of the work would produce —
  no separate definition of "page."

## Non-goals

- No change to play citations.
- No persistent or printed-edition page numbers (DB-backed pagination is out of scope).
- No caching of page boundaries in v1.
- No window-resize-stable page numbering — pages depend on viewport size, by design.

## Behavior

| Work type | Label format | Example |
|-----------|--------------|---------|
| Play (with valid act/scene/line) | citation only | `I.i.15`, `III.ii.187` |
| Play (prologue/epilogue) | citation only | `Prologue 31`, `Epilogue 14` |
| Play (fallback `line.citation`) | citation only | (whatever DB stores) |
| Anything else (prose, poetry, etc.) | `<page> - <line_id>` | `1 - 1234`, `34 - 5678` |

The page prefix updates whenever `page_label_text_for_buffer` is called — i.e., on every
navigation event that refreshes the label (already wired in existing call sites, no new
hooks needed).

## Algorithm

Page is computed by replaying the same logic as the j key from line 0 forward:

```text
page = 1
top  = 0
loop:
    if top > target_line or top >= line_count: break
    next_top = compute_next_page_top(state, top)   // existing j-key logic
    if next_top > target_line: break               // target is on this page
    if next_top <= top: break                      // safety: no progress
    top = next_top
    page += 1
return page
```

`compute_next_page_top` already exists in `src/input/navigation.rs` and is what j uses to
find the next page boundary (handles descender guard, dialogue-trim, etc.). Reusing it
guarantees the prefix and j-keys stay in sync by construction.

**Cost.** O(pages-before-target) cheap iterations of the same arithmetic j already does
once per keypress. For a 25k-line novel at ~30 lines/page, ~830 iterations per label
refresh. No rendering, no GTK round-trips — sub-millisecond expected. No caching in v1.

## Code changes

Three files, all in `src/`:

1. **`src/ui/page_label.rs`** — add a free function:
   ```rust
   pub fn format_prose_label(page: usize, line_id: i64) -> String {
       format!("{} - {}", page, line_id)
   }
   ```
   Existing `format_play_citation` and `to_roman` are untouched.

2. **`src/input/navigation.rs`** — add a pure helper next to `compute_next_page_top`:
   ```rust
   pub fn viewport_page_for_line(state: &AppState, target_line: usize) -> usize { ... }
   ```
   Implements the loop above. Returns 1 for an empty work or `target_line == 0`.

3. **`src/app.rs`**, in `page_label_text_for_buffer` (currently at line 193) — replace
   the prose branch:
   ```rust
   // before:
   return Some(format!("{}", line.id));
   // after:
   let page = crate::input::navigation::viewport_page_for_line(self, idx);
   return Some(crate::ui::page_label::format_prose_label(page, line.id));
   ```
   `idx` here is the buffer line that resolved to a real work line (the existing
   forward-scan past spacers is preserved). The play branch is unchanged.

## Edge cases

- **Empty work or `target_line == 0`** → page = 1.
- **`target_line` past end of buffer** → loop breaks naturally; returns the last
  computed page (defensive — not expected in normal operation).
- **Window resize between navigations** → next label refresh recomputes against the new
  viewport size. Prefix may shift by ±1; this is acceptable and matches the chosen
  semantics (page is viewport-defined, not stable).
- **Translation overlay inserts/removes lines** → buffer-line indices already remap
  via the existing `map_line_after_insert` / `map_line_before_insert` plumbing for
  `page_top_line`. The prefix recomputes from line 0 each refresh, so it picks up the
  new layout for free.
- **Spacer lines** → `page_label_text_for_buffer` already scans forward to a real work
  line; that scanned `idx` is what the page is computed against. Consistent.
- **`compute_next_page_top` returns a value `<= top`** → safety break, returns current
  page. Prevents infinite loops if the helper ever fails to advance.

## Testing

- **Unit test** `format_prose_label` in `src/ui/page_label.rs`:
  - `(1, 1234)` → `"1 - 1234"`
  - `(34, 5678)` → `"34 - 5678"`
- **Integration**: `viewport_page_for_line` requires a live `AppState` with a real
  `gtk4::TextView` (descender-guard math reads pixel metrics), so unit-testing it
  directly is impractical. Instead, extend the existing `test-prose-navigation`
  headless test (skill: `test-prose-navigation`) with one assertion: after N j-presses
  on a known prose work, the label prefix reads `N+1`. This exercises the helper
  end-to-end through the same code path the user hits.
- `cargo build` and `cargo clippy` must pass.

## Out of scope / deferred

- Caching of page boundaries (add only if measurement shows a perf problem).
- Stable-across-resize page numbering (would require a different definition of "page").
- DB-backed printed-edition pages (not currently stored).
- Showing the page prefix for plays (explicitly excluded by user).

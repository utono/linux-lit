# MRU-Reactive Search via n/N — Design

**Date:** 2026-06-06
**Branch context:** builds on `bookmark-chapter-canonical-spread` (commit ccea579)

## Problem

After the user opens search with `/`, types a pattern, and presses Escape to
cancel search mode, pressing `n`/`N` is a silent no-op (unless a concordance is
active). The user wants `n`/`N` to *reactivate* search using the most-recently-used
(MRU) pattern, navigate to the canonical spread containing the next/previous
match, and highlight the matched line — without ever wrapping around the ends.

## Goals

1. The last non-empty search pattern persists for the session as the MRU pattern.
2. After Escape (matches cleared), `n`/`N` re-runs the MRU search against the
   current work, rebuilds match highlights, and navigates.
3. `n`/`N` never wrap. At the boundaries they stop and show an edge toast:
   - `n` past the last match → **right-aligned** toast: "No later occurrence of '<pattern>'".
   - `N` before the first match → **left-aligned** toast: "No earlier occurrence of '<pattern>'".
4. The first target after reactivation is the first match **at or after** the
   current cursor line. A match already on the current spread but ahead of the
   cursor is selected first ("if the pattern already appears on the current
   spread and has not yet been navigated to, navigate to it").
5. Navigation lands on the **canonical spread** (`canonical_page_top_for`),
   consistent with bookmark/chapter jumps. If the target match is already fully
   visible on the current spread, move the cursor/highlight only — no
   re-pagination, no flash.

## Non-goals (YAGNI)

- No disk persistence of the MRU pattern (session-only, in `AppState`).
- No change to concordance priority: when a concordance is active, `n`/`N`
  still drive concordance navigation, unchanged.
- No change to MPV behavior: `n`/`N` continue to seek + resume via
  `seek_and_resume`; reactivation behaves identically.
- No new keybinds; `/`, `n`, `N` routing is unchanged.

## Decisions (confirmed with user)

| Question | Decision |
|----------|----------|
| Landing spread | Canonical spread (`canonical_page_top_for`) |
| Match state after Escape | Re-run search on MRU pattern (rebuild matches) |
| First target | First match at/after current line |
| MRU scope | Session-wide; re-search per work; no disk persistence |
| Wrap at ends | Never wrap; stop + edge toast (applies to active AND reactivated search) |
| Same-spread target | Move cursor only, keep spread (no re-paginate) |
| Reactivation highlights | Full highlights (dim-all + orange-current), like a fresh search |
| MPV on reactivation | Keep current behavior (seek + resume) |

## Components & changes

### `src/app.rs`

- **`AppState`**: add `pub last_search_query: Option<String>`. Initialized
  `None` in the constructor. Set by `execute_search` on a non-empty query.
  NOT cleared by `clear_search` or Escape.
- **Widgets**: add two edge-toast labels (added as overlays on
  `authorship_picker.overlay`, same pattern as `chapter_toast`/`speed_toast`):
  - `pub search_edge_toast_left: gtk4::Label` — `halign: Start`, `valign: End`,
    `margin_start: 24`, `margin_bottom: 32`, css class `chapter-toast`,
    `visible: false`.
  - `pub search_edge_toast_right: gtk4::Label` — `halign: End`, `valign: End`,
    `margin_end: 24`, `margin_bottom: 32`, css class `chapter-toast`,
    `visible: false`.
  - A separate left toast (not reusing `speed_toast`) so search edge messages
    never clobber the playback-speed toast text.

### `src/input/search.rs`

- **`execute_search`**: after reading a non-empty `query`, store
  `state.last_search_query = Some(query.to_string())`.
- **Extract `goto_match_idx(state, new_idx)`** — the shared body currently
  duplicated in `next_match`/`prev_match`: remove current highlight → set
  `search_match_idx` → set `current_line` → apply current highlight → update
  counter → `push_page_back_dedup` → canonical land → `seek_and_resume`.
  - **Canonical land** replaces the bare `update_highlight_and_center` call,
    mirroring `navigation::jump_to_line`: if
    `viewport::is_line_fully_visible(state, line)`, call `update_highlight` and
    set `current_line` without changing the page; else compute
    `top = canonical_page_top_for(state, line)` and `set_page_instant(state, top)`
    (scroll mode falls back to `center_cursor`, same as `jump_to_line`).
- **`next_match`**: if `search_match_idx + 1 >= total`, show the right edge toast
  and return (no move). Else `goto_match_idx(state, search_match_idx + 1)`.
- **`prev_match`**: if `search_match_idx == 0`, show the left edge toast and
  return. Else `goto_match_idx(state, search_match_idx - 1)`.
- **New `pub fn reactivate_and_step(state_rc: &Rc<RefCell<AppState>>, forward: bool)`**:
  - If `search_matches` is non-empty → just call `next_match`/`prev_match` on a
    `borrow_mut()` (current behavior).
  - Else if `last_search_query` is `Some(pat)` → reactivate. Set the search bar
    entry text to `pat` and collect matches + apply full highlights (the
    match-collection + `apply_highlights` portion of `execute_search`), WITHOUT
    auto-navigating. Then seed the index and land based on `forward`:
    - **`forward == true` (`n`)**: select the first match with
      `line_index >= current_line`; if none, the first match ≥ cursor does not
      exist, so the cursor is already past every match → show the right edge
      toast (boundary crossed, no later occurrence). Otherwise
      `goto_match_idx(first_at_or_after)`. This satisfies "navigate to the match
      already on the current spread if not yet navigated to" — the first match
      at/after the cursor is chosen.
    - **`forward == false` (`N`)**: select the last match with
      `line_index <= current_line`; if none exists → left edge toast. Otherwise
      `goto_match_idx(last_at_or_before)`. (If the only matches are after the
      cursor, `N` reports "no earlier occurrence".)
    - Seeding by cursor position (not modulo from a stale index) is what makes a
      match already visible on the current spread the natural first target.
  - Else (`search_matches` empty AND `last_search_query` None) → no-op (today's
    behavior; nothing to reactivate).
- **`edge_toast(state, side: Side, query: &str)`** helper: sets the
  corresponding label text + visible, schedules a 3s `timeout_add_local_once`
  to hide it (mirrors `show_chapter_toast`).
- **`clear_search`**: unchanged — still clears matches/highlights but must NOT
  touch `last_search_query`.

### `src/input/keymap.rs`

- `SearchNextMatch` arm: keep `concordance_state.is_some()` branch first
  (unchanged). Replace the `else if !search_matches.is_empty()` branch with a
  call to `search::reactivate_and_step(state, true)`.
- `SearchPrevMatch` arm: same, `reactivate_and_step(state, false)`.
- The in-search-mode `handle_search_key` and the `OpenSearch`/Escape paths are
  unchanged (Escape still restores position and clears matches; it no longer
  needs to clear the MRU since `clear_search` does not touch it).

## Data flow

```
/  → OpenSearch → search bar shows, live execute_search per keystroke
       └─ execute_search stores last_search_query = Some(pattern)
Esc → clear_search (matches/highlights gone) + restore position; MRU kept
n  → SearchNextMatch → reactivate_and_step(true)
       ├─ matches present? → next_match (stop+toast at end)
       └─ matches empty + MRU present? → collect+highlight MRU, land first match ≥ cursor
N  → SearchPrevMatch → reactivate_and_step(false)
       └─ matches empty + MRU present? → collect+highlight MRU, land last match ≤ cursor
```

## Edge cases

- **MRU set but pattern has zero matches in current work**: `execute_search`
  leaves `search_matches` empty and sets counter `0/0`; `reactivate_and_step`
  then has nothing to navigate. Show no toast (no boundary crossed) — the empty
  counter is the only feedback, matching a fresh zero-result search.
- **Work switched since MRU was set**: `reactivate_and_step` re-runs against the
  newly loaded `current_work`/buffer, so offsets are always fresh. Confirmed
  decision: MRU is session-wide.
- **Single match in work**: `n` and `N` both immediately hit the boundary and
  show the respective edge toast after the first landing.
- **Concordance active**: untouched — concordance navigation still wins.

## Testing

Per `CLAUDE.md`, landing/pagination/toast positioning are visual-only and have
no pure unit tests. Plan:

- `cargo build` + `cargo test --bins` to cover the pure logic that exists
  (index-stop math via any extractable helper, MRU storage). Add a small unit
  test for the no-wrap index decision if it can be isolated from GTK state.
- Ask the user to run the e2e / manual launch to verify on-screen:
  1. `/pattern` → Return → Escape → `n` lands on canonical spread, line
     highlighted.
  2. `n` repeatedly to the last match → right toast, no wrap.
  3. `N` to the first match → left toast, no wrap.
  4. A match already on the current spread is selected by `n` without a flash
     (cursor moves, spread unchanged).
  5. MRU survives a Ctrl+p work switch; `n` re-searches in the new work.

## Files touched

- `src/app.rs` — `AppState` field, two toast widgets, constructor init.
- `src/input/search.rs` — MRU store, `goto_match_idx`, no-wrap, edge toasts,
  `reactivate_and_step`.
- `src/input/keymap.rs` — `SearchNextMatch`/`SearchPrevMatch` dispatch.

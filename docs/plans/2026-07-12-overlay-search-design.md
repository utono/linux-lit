# In-Overlay Regex Search + `f`-Term Highlighting — Design

> Design/spec. Next step: `superpowers:writing-plans` → an implementation plan
> under `docs/plans/`. Canonical location is `docs/plans/` (this file).

## Goal

Give the journal and gloss overlays a find-in-view search:

1. `/` in the journal or gloss overlay runs a **regex search of the currently
   shown entry/gloss**; `n`/`N` step to the next/previous match within it.
2. The `f` term/tag (journal term-browse) becomes the active search pattern and
   is **highlighted in every journal entry in the picked set** — re-applied as
   each entry is shown while `Ctrl+n`/`Ctrl+p` walk the set.
3. `Escape` exits search mode and clears the highlights. **After Escape, `n`/`N`
   reactivate the most-recently-used pattern** and resume stepping.

Workflow: press `f` → enter a term or select a tag → the term is highlighted in
each entry of the matched set as you step through it with `Ctrl+n`/`Ctrl+p`;
or press `/` to search the current entry with a different regex.

## Model (decided)

- **One unified "overlay search mode", one active pattern**, set two ways:
  - `f` (journal only): the chosen term becomes the pattern AND loads the match
    set (existing `activate_filter`). Highlighted in each entry as shown.
  - `/` (journal + gloss): type a regex for the *currently shown* view.
- **`n`/`N` step matches WITHIN the current entry** (scrolling the overlay to
  each). **`Ctrl+n`/`Ctrl+p` remain between-entry** nav of the journal set
  (already built — `nav_page`'s filter branch).
- **Highlight scope:** the term is highlighted in each entry **as it is shown**
  (re-applied on every entry render); one entry is visible at a time.
- **`/` in gloss:** plain find-in-current-gloss (no set concept there).
- **MRU:** Escape clears highlights but remembers the pattern; a later `n`/`N`
  revives it (mirrors reader search `reactivate_and_step`).
- **Last-set-wins:** `/` replaces the active pattern for the current entry;
  after a `/`, Ctrl+n/p re-apply the `/` pattern (it is now the MRU). One
  active pattern at a time.

## Architecture

Each overlay owns its own `TextView`/buffer (separate from the reader's
`state.buffer`), and already registers TextTags on it. So overlay search tags
the OVERLAY buffer, with an overlay-local match list — NOT the reader search
state. The regex/step logic is lifted from `src/input/search.rs` (already
regex-based with next/prev/reactivate) but parameterized over the overlay
buffer + tag.

Reuse the existing reader `search_bar` WIDGET (`src/ui/search_bar.rs`:
`show/hide/query/set_text/update_counter`) for the `/` input — but keep the
overlay in its OWN input mode (do NOT route through the reader
`InputMode::Search`, whose `handle_search_key` operates on `state.buffer`), so
the overlay's render/highlight/context stays intact.

## Components

### 1. `src/input/overlay_search.rs` (new — pure-ish, unit-tested)

Operates on a passed `&gtk4::TextBuffer` + `&gtk4::TextTag`; no AppState.

```rust
pub struct OverlaySearch {
    pub pattern: String,
    pub is_regex: bool,
    pub matches: Vec<(i32, i32)>, // char-offset spans in the current buffer
    pub current: usize,
}

/// Compile `pattern` (regex; on invalid regex, literal-substring fallback +
/// the caller toasts), collect all matches in `buffer`, apply `tag` to all and
/// `current_tag` to match 0. Empty pattern or zero matches → matches empty.
pub fn set_pattern(
    buffer: &TextBuffer, tag: &TextTag, current_tag: &TextTag, pattern: &str,
) -> OverlaySearch;

/// Move `current` by ±1 (clamp, no wrap), move `current_tag` to it, and return
/// the new current span so the caller can scroll the TextView to it.
pub fn step(
    s: &mut OverlaySearch, buffer: &TextBuffer, current_tag: &TextTag, forward: bool,
) -> Option<(i32, i32)>;

/// Re-tag `buffer` for `s.pattern` (called on each entry render so the f-term
/// lights up in every entry). Recomputes matches against the new buffer.
pub fn reapply(s: &mut OverlaySearch, buffer: &TextBuffer, tag: &TextTag, current_tag: &TextTag);

/// Remove all search tags from `buffer`.
pub fn clear(buffer: &TextBuffer, tag: &TextTag, current_tag: &TextTag);
```

### 2. `src/ui/journal_overlay.rs` + `src/ui/gloss_overlay.rs`

- Register `search_tag` + `search_current_tag` on the overlay buffer (like the
  existing markdown/highlight tags). Color from the theme's `selection_bg` via
  the same `apply_theme_to_state` overlay-highlight path (so Ctrl+t recolors
  live highlights too — see the theme-cycle feature just shipped).
- Accessors: `pub fn buffer(&self) -> gtk4::TextBuffer`,
  `pub fn scroll_to_char_offset(&self, off: i32)`, and tag getters.

### 3. `src/input/actions/journal.rs` (+ `gloss.rs`)

- `JournalState` gains `search: Option<OverlaySearch>` and
  `last_pattern: Option<String>` (MRU). Gloss overlay gains the equivalent.
- `activate_filter` (the `f`-flow) ALSO seeds `search` from the chosen term and
  applies it to the first rendered entry.
- `render_filtered_match` (and the normal band render) call
  `overlay_search::reapply` when `search.is_some()`, so each shown entry lights
  up.
- New handlers: `open_overlay_search` (opens the search_bar for `/`),
  `confirm_overlay_search` (Enter → `set_pattern`), `step_overlay_search`
  (n/N), `clear_overlay_search` (Escape / exit), `revive_overlay_search`
  (post-Escape n/N → MRU).

### 4. `src/input/keymap.rs`

- In `handle_journal_key` and `handle_gloss_key`:
  - `/` → `open_overlay_search`.
  - `n` / `N` → `step_overlay_search(forward/back)`; if no active search but
    `last_pattern` is set → `revive_overlay_search` (MRU) then step.
  - `Escape` precedence (journal): search-active → clear search (stay); else
    filter-active → clear filter; else close overlay. (Chains onto the existing
    two-stage Esc.) Gloss: search-active → clear search; else existing close.
- The `/`-bar input mode: a new `InputMode` (e.g. `OverlaySearchInput`) whose
  Enter calls `confirm_overlay_search` and Escape cancels back to the overlay;
  reuse the `search_bar` widget for display.
- Filter-gate: `/`, `n`, `N` are SAFE-under-filter (they act on the overlay
  buffer, like j/k) — do NOT add them to the mutating gate set.

## Edge cases

- **Bad regex** → literal-substring fallback + toast; never a crash.
- **Zero matches** in the current entry → toast "No matches"; pattern stays
  active (Ctrl+n/p to another entry may find some).
- **Empty pattern** (`f` empty box, or `/` then empty Enter) → no search mode.
- **Borrow safety** (recurring hazard this session): the `/`-bar Enter, the
  n/N handlers, and any new `search_bar` `connect_*` must NOT hold
  `state.borrow_mut()` across a signal emission or a dispatch. Scope borrows;
  use `try_borrow()` in any signal closure (see the picker-crash gotcha in
  `ac` / the memory bank).
- **Theme cycling** recolors the search tags live (overlay highlight path).
- **`f`-set + `/`:** last-set-wins; `/`'s pattern becomes MRU.

## Testing

- **Unit** (`cargo test`, headless `TextBuffer`): `set_pattern`
  (regex + literal fallback, empty), `step` (clamp-no-wrap), `reapply`
  (re-tags against a new buffer), MRU revive.
- **Headless e2e:**
  1. `f` → term → highlighted in the entry; Ctrl+n/p → highlighted in each
     entry visited.
  2. `/` in journal → regex → n/N step within the entry, overlay scrolls.
  3. `/` in gloss → regex search the current gloss.
  4. Escape clears the highlight; then `n` revives the MRU pattern.
  5. Borrow-safety: no crash on repeated `/` → Esc → `/` and reopen.
- Real-renderer eyeball (highlight color + scroll) handed to the user.

## Files

- Create: `src/input/overlay_search.rs` + register `mod`.
- Modify: `src/ui/journal_overlay.rs`, `src/ui/gloss_overlay.rs` (search tags +
  accessors).
- Modify: `src/input/actions/journal.rs` (+ `gloss.rs`) — state, handlers,
  reapply-on-render, `activate_filter` seeding.
- Modify: `src/input/keymap.rs` — `/`, n/N, Escape precedence, the search-input
  mode, filter-gate exclusion.
- Modify: `src/input/actions/settings.rs` `apply_theme_to_state` — set the new
  overlay search-tag colors (so theme cycling covers them).

## Non-goals

- No cross-entry match walking on n/N (that stays Ctrl+n/p between entries).
- No per-entry match tally in the footer (highlight-as-shown only).
- No change to the reader-mode `/` search (`state.buffer`); this is a parallel
  overlay-buffer search that REUSES the regex/step logic and the search_bar
  widget, not the reader Search mode.

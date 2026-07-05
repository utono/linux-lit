# Concordance Picker Crash and Scroll Failures

Date: 2026-04-05

## Symptoms

1. **SIGABRT crash** when selecting a word from the Vocab Words picker (Ctrl+Shift+P)
2. **Blank screen** when loading a work via library picker (Ctrl+P) or concordance cross-work jump — text scrolled past end of buffer
3. **Wrong cursor position** after concordance jump to a different work — `current_line` set to work-line index instead of buffer-line index

## Root Cause 1: RefCell Double Borrow (SIGABRT)

**File:** `src/input/keymap.rs`

`concordance_word_picker.show()` calls `set_text("")` on its search entry, which fires the GTK `connect_changed` signal synchronously. The signal handler calls `state.borrow()`, but the caller still held `state.borrow_mut()` — causing a RefCell double-borrow panic. Since this panic crosses a GTK FFI boundary (signal trampoline), Rust can't unwind and calls `abort()`.

**Stack trace signature:**
```
#8  panic_nounwind
#10 panic_cannot_unwind
#11 EditableExt::connect_changed::changed_trampoline
#12 g_closure_invoke
```

**Affected pickers:**
- `concordance_word_picker.show()` — calls `set_text("")` internally
- `media_picker.show()` — calls `set_text("")` internally
- `concordance_picker.show()` — safe, does NOT call `set_text("")`

**Fix:** Drop the `borrow_mut()` before calling `show()` on any picker whose `show()` method triggers `set_text()`:

```rust
// BEFORE (crashes):
let mut s = state_clone.borrow_mut();
s.concordance_word_picker.set_words(words);
s.concordance_word_picker.show();  // set_text("") → connect_changed → borrow() → PANIC

// AFTER (safe):
{
    let mut s = state_clone.borrow_mut();
    s.concordance_word_picker.set_words(words);
}
state_clone.borrow().concordance_word_picker.show();
```

**General rule:** Any GTK widget method that triggers a signal (e.g. `set_text`, `set_active`, `emit`) must not be called while `borrow_mut()` is active on the same `RefCell`, if any signal handler for that widget calls `borrow()` or `borrow_mut()`.

## Root Cause 2: Stale GTK Layout After Buffer Rebuild

**Files:** `src/input/navigation.rs`, `src/input/keymap.rs`

After `display_work()` rebuilds the buffer text (especially for large works like Bleak House with 39751 lines), GTK's text layout engine hasn't computed line positions yet. Calling `scroll_to_iter()` or `center_cursor()` immediately (or even at 50ms/250ms) fails silently — the scroll lands at the wrong position or past the end of content, producing a blank screen.

**Why the initial app load worked but the library picker didn't:**

The initial load (in `app.rs`) uses:
```rust
display_work(&mut s, work);
// ...
glib::idle_add_local_once(move || {
    restore_cursor(&mut state_clone.borrow_mut());
});
```

`idle_add_local_once` waits for one full main loop iteration (letting GTK process layout), then `restore_cursor` adds another 100ms timeout. Total: ~1 main loop iteration + 100ms.

The library picker was calling `display_work` which internally used `update_highlight_deferred_scroll` with only a 50ms timeout — not enough for large buffers.

**Fix:** The library picker now uses the same pattern as the initial load:
```rust
display_work(&mut s, work);
// Drop borrow, then defer scroll
glib::idle_add_local_once(move || {
    navigation::restore_cursor(&mut state_clone.borrow_mut());
});
```

## Root Cause 3: Work-Line vs Buffer-Line Index Confusion

**File:** `src/input/navigation.rs`

The original `concordance_position_cursor` set `state.current_line = idx` where `idx` came from `work.lines.iter().position(...)` — a work-line index. But `current_line` must be a buffer-line index. When a `line_map` is present (text-file works), these differ significantly.

**Fix:** Use `line_map.work_to_buffer[work_idx]` to convert work-line index to buffer-line index.

## Root Cause 4: MPV Sync Interference During Cross-Work Jump

**File:** `src/input/navigation.rs`

When a concordance jump loads a different work, `concordance_seek()` sends `ResumeAndSeek` to MPV. Once `loading_work` clears (at 50ms), the MPV sync callback fires and calls `center_cursor()` on stale GTK layout, scrolling to the wrong position.

**Fix:** Set `suppress_sync_until` for 500ms in `concordance_position_after_load`, preventing MPV sync from interfering until the deferred scroll callback has fired.

## Changes Summary

- `src/input/keymap.rs`: Drop `borrow_mut()` before `concordance_word_picker.show()` and `media_picker.show()`; library picker uses `idle_add_local_once` + `restore_cursor` for deferred scroll
- `src/input/navigation.rs`: Split `concordance_position_cursor` into same-work (immediate scroll) and cross-work (`concordance_position_after_load` with deferred scroll + sync suppression); added `concordance_resolve_indices` helper for work-to-buffer index mapping; `update_highlight_deferred_scroll` re-scrolls at 200ms as safety net
- `src/concordance.rs`: `advance()`/`retreat()` wrap around instead of stopping at boundaries
- `src/text_file_map.rs`: Build sentence groups for all works, not just prose

## Diagnostic Logging Added

- `PICKER:` — library picker selection, DB load timing, post-display_work state
- `CONC_JUMP:` — concordance target, same-work vs cross-work path, preloaded vs async
- `CONC_POS:` — buffer index resolution, deferred scroll scheduling and firing

---

## Root Cause 5: Picker Takes ~10s to Open (First Open Per Author)

Date: 2026-07-04

### Symptom

Pressing the concordance-picker bind takes about **10 seconds** to show the
picker on the first press for a given author. Subsequent presses are instant.

### Root Cause: Picker Appearance Gated on a Full-Corpus Tokenize

**Files:** `src/input/actions/concordance.rs` (`open_picker`),
`src/db/concordance.rs` (`load_concordance_words`)

`open_picker` has two branches:

- **Cached** — reads `AppState.concordance_word_cache`; if the author matches, it
  calls `set_words` + `show` + `open_picker_mode` **fully synchronously** (no
  `.await`). Instant.
- **Uncached** (first open per author) — spawns `load_concordance_words` on a
  blocking thread and shows the picker **only after** it returns.

`load_concordance_words` builds a stopword-filtered word-frequency list across
**all of the author's works**:

```sql
SELECT lm.normalized_text
FROM line_mapping lm
JOIN works w ON w.abbrev = lm.work_abbrev
WHERE w.author = ?1
```

then tokenizes every returned line in Rust (split on non-alphanumerics,
`to_lowercase()` per token → a new `String` allocation, HashMap insert).

### Evidence (measured, not guessed)

For author `Shakespeare`:

- The query returns **426,695 lines across 136 works** (edition variants like
  `Cym`, `Cym-Amb`, `Cym-BBC` each count, which inflates the row set).
- The **SQL alone is ~0.47s** — it uses `idx_work` on `line_mapping.work_abbrev`
  (`EXPLAIN QUERY PLAN` → `SEARCH lm USING INDEX idx_work`). Not the bottleneck.
- The **full build is ~10s** — the Rust tokenize loop (millions of `to_lowercase`
  allocations + HashMap inserts) dominates. Confirmed deterministically: the four
  `db::concordance::tests::concordance_words_*` unit tests, which each call
  `load_concordance_words` against the real DB, finish in **10.31s**
  (`cargo test --bins concordance_words`).

So the ~10s is the Rust tokenize, and the picker's appearance was blocked on it.
There is **no** precomputed concordance/word-frequency table in `lit.db` (the
`vocab_*` tables are unrelated).

### Fix: Warm the Word-List Cache at Work Load

**Files:** `src/input/actions/concordance.rs` (new `warm_word_cache`),
`src/app/mod.rs` (call site in the work-load path)

New `warm_word_cache(state, tokio_handle)` runs the same build in the background,
keyed by author, and stores it in `concordance_word_cache` — the exact field the
`open_picker` cache-hit branch reads. It is called right after
`display_work_at_with_prepared` sets `current_work`, so the ~10s runs in the
background as soon as a work loads. When the picker bind is later pressed,
`open_picker` finds the cache populated and takes its synchronous show path —
the picker opens instantly.

Safeguards in `warm_word_cache`:

- **No-op if already cached** for the current author (the build only runs when
  the author changes, so it is safe to call on every work load).
- **Discards a stale result** — if the user switched to a different author while
  the build ran, or an intervening `open_picker` already populated the cache, the
  finished words are dropped instead of clobbering the current author's cache.

```rust
// src/app/mod.rs — after the work's current_work is set:
display_work_at_with_prepared(&mut s, work, target_line_id, prepared);
// (borrow dropped)
crate::input::actions::concordance::warm_word_cache(&state_clone, &handle);
```

### Known Limitation

Warming does **not** help the very first bind press if it happens within ~10s of
a work loading (before the background build finishes) — that press still waits
for the remainder. Every press after the build completes, and after any later
work by the same author, is instant.

Alternatives considered but not taken (heavier changes): open the picker
immediately with a "Loading…" placeholder and populate on completion; or make
the build itself fast via a precomputed `lit.db` word-frequency table / on-disk
per-author cache / a lower-allocation tokenizer. Warming was chosen as the
smallest change that keeps the full corpus list and makes the common case
(pressing the bind well after load) instant.

### Diagnostic Logging Added

- `CONC_WARM:` — logs when the background word-cache build starts for an author,
  and when it finishes (`cached N words; …` or `discarded …`). Use it to confirm
  the cache is warm before pressing the bind.

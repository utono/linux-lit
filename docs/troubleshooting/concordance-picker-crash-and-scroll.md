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

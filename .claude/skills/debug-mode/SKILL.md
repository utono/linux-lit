---
name: debug-mode
description: Use when toggling debug logging on or off in linux-lit, or when debug logs are empty and need to be enabled before reproducing a bug
---

# Debug Mode

Toggle debug logging in linux-lit with **Ctrl+d**. Debug mode is off by default — no log output is written until enabled.

## Runtime Toggle

- **Ctrl+d** toggles debug mode on/off
- Writes `DEBUG_MODE: on` or `DEBUG_MODE: off` to the log file (always, regardless of mode)
- All `log_fmt!()` and `crate::logging::log()` calls are gated behind debug mode
- `crate::logging::log_always()` writes regardless of debug mode (used for the toggle message itself)

## Log File

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

## Debugging Workflow

1. Reproduce the issue location (navigate to the page/line where the bug occurs)
2. Press **Ctrl+d** to enable debug mode — look for `DEBUG_MODE: on` in the log
3. Reproduce the bug (press the keybind that fails, let playback sync stall, etc.)
4. Press **Ctrl+d** again to disable — reduces log noise
5. Read the log for the relevant prefixes

## Log Prefixes by Debugging Skill

**debug-navigation-sync** (`,` `q` `j` `k` keybinds):
- `NAV_PREV:`, `NAV_NEXT:`, `NAV_BACK:`, `NAV_PAGE_FWD:`, `NAV_PAGE_BACK:`, `SEEK:`

**debug-playback-sync** (playback sync page turns):
- `CURSOR_SYNC:`, `SYNC_ADVANCE:`, `PAGE_TURN:`, `PAGE_FWD:`

**General:**
- `KEY:` — every keypress (noisy, only useful for verifying key routing)

## Implementation

- `src/logging.rs` — `AtomicBool` flag, `set_debug_mode()`, `debug_mode()`, `log_always()`
- `src/input/keymap.rs` — Ctrl+d handler calls `set_debug_mode(!debug_mode())`

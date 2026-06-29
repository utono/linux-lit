# External Theme Reload via SIGUSR1

**Date:** 2026-03-28

## Problem

linux-lit only loads its theme at startup. When the system theme changes externally (via `set-theme.sh`, `cycle-theme.sh`, `theme-selector.sh`, or `cycle-wallpaper.sh`), the app shows stale colors until restarted.

## Solution

Add SIGUSR1 signal handling to linux-lit. On receiving the signal, re-read `.current_theme` and re-resolve the full theme from `themes-unified.json`, then reapply via the existing `apply_theme_to_state()` path. Modify the shell scripts to send the signal.

## Architecture

### Signal Flow

```
cycle-wallpaper.sh  ──┐
set-theme.sh        ──┤── pkill -USR1 linux-lit
                      │
                      ▼
Tokio thread: signal listener
                      │
                      ▼ evt_tx.send(ThemeChanged)
                      │
GTK main loop: match MpvEvent::ThemeChanged
                      │
                      ▼
read .current_theme → load_theme() → apply_theme_to_state()
```

### Components

**1. New MpvEvent variant**

Add `ThemeChanged` to the `MpvEvent` enum in `src/mpv/commands.rs`. No payload — the handler reads the current theme name fresh from disk.

**2. Tokio signal listener**

In `src/main.rs`, before the MPV client `run()` call, spawn a separate Tokio task:

```rust
let signal_evt_tx = evt_tx.clone();
tokio::spawn(async move {
    let mut sig = tokio::signal::unix::signal(SignalKind::user_defined1()).unwrap();
    loop {
        sig.recv().await;
        let _ = signal_evt_tx.send(MpvEvent::ThemeChanged).await;
    }
});
```

This runs independently from the MPV client loop. Uses a clone of `evt_tx` so both can send events to GTK.

**3. GTK event handler**

In the existing `glib::spawn_future_local` match block in `src/main.rs`, add:

```rust
MpvEvent::ThemeChanged => {
    let theme_name = crate::theme::current_theme_name();
    let theme = if theme_name.is_empty() {
        crate::theme::load_theme("gruvbox-material")
    } else {
        crate::theme::load_theme(&theme_name)
    };
    crate::input::keymap::apply_theme_to_state(&mut state, theme);
}
```

This reuses the exact same code path as the in-app Settings overlay theme change.

**4. Shell script modifications**

Add `pkill -USR1 linux-lit` to two scripts:

- `~/.config/themes/cycle-wallpaper.sh` — after updating rootcolor MRU and signaling dwl
- `~/.config/themes/set-theme.sh` — after applying theme to all apps

`pkill` exits silently if no matching process exists, so this is safe when linux-lit isn't running.

## Scope

### In scope

- SIGUSR1 listener in Tokio runtime
- ThemeChanged event variant
- GTK handler that re-resolves and reapplies theme
- `pkill -USR1 linux-lit` added to `cycle-wallpaper.sh` and `set-theme.sh`

### Out of scope

- File watching / inotify (not needed with signal approach)
- Renaming MpvEvent to a more general name (separate refactor)
- Theme change animation/transition effects
- D-Bus interface

## Testing

- Build linux-lit, run it
- Run `pkill -USR1 linux-lit` manually — verify theme reloads from current `.current_theme`
- Change theme via `set-theme.sh` — verify linux-lit updates
- Cycle wallpaper via Super+Shift+Backslash — verify rootcolor updates in linux-lit
- Verify no crash when signal received during settings overlay, search, or other UI states

# 8BitDo Micro Gamepad → linux-lit Integration

## Goal

Use the 8BitDo Micro Bluetooth gamepad to drive linux-lit reader actions
(next/prev dialogue, page turns, playback, etc.) without remapping existing
keyboard bindings and without colliding with normal typing.

## Resolution

Implemented as a background evdev reader in `src/input/gamepad.rs`, wired
into `main.rs` after window construction. The pad is used in **gamepad
mode** (not keyboard mode), so events arrive as `BTN_*` / `ABS_*` in a
namespace completely separate from keyboard input — no collision with
Kanata, typing, or existing linux-lit keybinds.

## How It Got Here

### 1. Legacy service removed from install flow

- `nvim-micro-gamepad.service` was already disabled
  (`systemctl --user is-enabled` → `disabled`). The service was built for
  the old lit + nvim + MPV reader workflow that linux-lit replaces.
- Patched `~/utono/ccinstall` so future CachyOS installs do **not**
  re-enable it:
  - `.claude/commands/ccinstall/configure-gamepad.md` — removed the
    `systemctl --user enable nvim-micro-gamepad.service/.path` activation
    blocks.
  - `CLAUDE.md` — deprecation note pointing at linux-lit.
  - `gamepad/hardware-gamepad-setup.md`, `gamepad/micro-gamepad-nvim.md`,
    `gamepad/8bitdo-micro-gamepad.md`,
    `docs/gamepad-ps-5-controller.md` — deprecation banners at the top of
    each. Content preserved for reference.

### 2. Keyboard-mode investigation (rejected)

In its default keyboard-HID mode (product ID `2DC8:9021`), the pad emits
`KEY_H` / `KEY_J` / `KEY_R` / etc. After Kanata's RPD remapping, those
land in apps as QWERTY letters (`d`, `h`, `p`, `i`, `c`, `m`, `r`, `o`,
`b`, `u`). Attempted adding plain-letter match arms to
`src/input/keymap.rs` but abandoned because:

- The letters collide with existing linux-lit binds (e.g., `h` already
  toggles `vocab_popup_auto` at keymap.rs:1621).
- They also collide with normal typing everywhere else on the system.
- Changing existing binds to accommodate the pad was not acceptable.

### 3. Mode switch to gamepad HID

The Micro has a physical mode switch (documented in
`~/Downloads/Micro-Bluetooth-gamepad-8.pdf`). Flipping it changed the
product ID to `2DC8:9020` and changed the HID descriptor from Keyboard
to Gamepad. First connection attempt failed —
`Bluetooth: hci0: Opcode 0x0401 failed: -16` (EBUSY), caused by stale
pairing data from the previous mode.

Fixed by re-pairing from scratch in `bluetoothctl`:
```
remove <old-mac>
scan on
pair <new-mac>
trust <new-mac>
connect <new-mac>
```

After re-pair, kernel bound it as `hid-generic` → Gamepad:
```
input: 8BitDo Micro gamepad as /devices/virtual/misc/uhid/0005:2DC8:9020.0007/input/input31
hid-generic 0005:2DC8:9020.0007: input,hidraw2: BLUETOOTH HID v1.00 Gamepad
```

### 4. Button inventory (via `sudo evtest`)

| evdev code | Name | Physical button |
|---|---|---|
| 304 | `BTN_SOUTH` | A (bottom face) |
| 305 | `BTN_EAST` | B (right face) |
| 307 | `BTN_NORTH` | X (top face, Switch-style labeling) |
| 308 | `BTN_WEST` | Y (left face, Switch-style labeling) |
| 310 | `BTN_TL` | L shoulder |
| 311 | `BTN_TR` | R shoulder |
| 312 | `BTN_TL2` | ZL trigger (also emits `ABS_BRAKE`) |
| 313 | `BTN_TR2` | ZR trigger (also emits `ABS_GAS`) |
| 314 | `BTN_SELECT` | Select (minus) |
| 315 | `BTN_START` | Start (plus) |
| 316 | `BTN_MODE` | Home |
| `ABS_X` (0/127/255) | — | D-pad horizontal |
| `ABS_Y` (0/127/255) | — | D-pad vertical |

The **Star** button does not emit any event in gamepad mode on this unit
— ignored.

### 5. Implementation

- Added `evdev = "0.12"` to `Cargo.toml`.
- Created `src/input/gamepad.rs`:
  - Background thread enumerates evdev devices, opens the one whose name
    equals `8BitDo Micro gamepad`, and calls `fetch_events()` in a loop.
  - Auto-reconnects: if the device is missing or `fetch_events` errors
    (pad disconnects / reconnects), the outer loop re-enumerates every
    5 seconds.
  - D-pad axes are reduced to three states per axis (low / center / high)
    and an action only fires on crossing into a non-center state, so a
    single press dispatches once, not per-report.
  - Button-press actions fire on `value == 1` only (press, not release).
  - Events are forwarded to the GTK main loop via a
    `tokio::sync::mpsc::channel`, read in `glib::spawn_future_local` —
    matches the pattern used by the rest of the codebase.
- `src/main.rs` calls `crate::input::gamepad::spawn(state.clone())` right
  after `app::build_window`.
- No udev rule needed — the `mlj` user is already in the `input` group.

## Current Button → Action Mapping

| Button | evdev | Action | Keyboard equivalent |
|---|---|---|---|
| A | `BTN_SOUTH` | `set_chapter` (toggle) | `.` |
| B | `BTN_EAST` | `jump_to_next_dialogue` | `q` |
| X | `BTN_NORTH` | `jump_to_prev_dialogue` | `,` |
| Y | `BTN_WEST` | `toggle_playback` (MPV play/pause) | `Tab` |
| D-pad Up | `ABS_Y` < 64 | toggle playback speed (1.0x ↔ 1.3x) | `+` |
| D-pad Down | `ABS_Y` > 192 | toggle translations | `i` |
| D-pad Left | `ABS_X` < 64 | seek -3.5s | `o` |
| D-pad Right | `ABS_X` > 192 | `set_start_time` + `cursor_next_dialogue` | `u` / Right |
| Select | `BTN_SELECT` | toggle sync (cursor follows MPV) | `s` |
| Start | `BTN_START` | `jump_to_prev_chapter` | `[` |
| Home | `BTN_MODE` | `jump_to_next_chapter` | `{` |

### Unbound buttons

These are detected by the reader but not mapped to any action yet:

- L shoulder (`BTN_TL`)
- R shoulder (`BTN_TR`)
- ZL trigger (`BTN_TL2`)
- ZR trigger (`BTN_TR2`)

Add entries in `key_to_action()` and `dispatch()` in
`src/input/gamepad.rs` to assign them.

## Files

- `src/input/gamepad.rs` — evdev reader and action dispatcher.
- `src/input/mod.rs` — registers the `gamepad` module.
- `src/main.rs` — calls `gamepad::spawn` after `build_window`.
- `Cargo.toml` — adds `evdev = "0.12"`.

## Testing

- `cargo build` passes with no errors (9 pre-existing dead-code warnings
  unrelated to the gamepad module).
- Manual test: turn on pad, confirm it connects via
  `bluetoothctl info E4:17:D8:A1:F4:F6`, launch `cargo run`, press
  buttons. `linux-lit-dev.log` will contain `GAMEPAD: found at ...` and
  `GAMEPAD: action=<Variant>` lines.

## If The Pad Stops Working

1. Battery — the Micro goes to sleep after idle; wake by pressing any
   button.
2. Re-pair if needed:
   ```
   bluetoothctl
   remove E4:17:D8:A1:F4:F6
   scan on
   pair <new-mac>
   trust <new-mac>
   connect <new-mac>
   ```
3. Verify a `Gamepad` input device exists:
   ```
   sudo dmesg | grep -i 8bitdo | tail -5
   ls /dev/input/js*
   ```
   If the dmesg line says `Keyboard` instead of `Gamepad`, the mode
   switch on the pad is in the wrong position — flip it and re-pair.
4. Check log: `tail -f ~/utono/linux-lit/linux-lit-dev.log | grep GAMEPAD`.

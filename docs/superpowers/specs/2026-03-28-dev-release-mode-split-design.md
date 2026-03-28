# Dev/Release Mode Split — Step 1

## Goal

Allow a release binary and a dev instance (`cargo run`) to run simultaneously without interfering with each other's config or logs.

## Mode Detection

- `.cargo/config.toml` sets `LIT_DEV=1` via `[env]` section
- `cargo run` automatically gets dev mode; `./target/release/linux-lit` does not
- `main.rs` checks `std::env::var("LIT_DEV")` at startup to determine mode

## Per-Mode Paths

| | Dev (`LIT_DEV=1`) | Release (no env var) |
|---|---|---|
| Config | `~/.config/linux-lit/config-dev.json` | `~/.config/linux-lit/config.json` |
| Log | `~/utono/linux-lit/linux-lit-dev.log` | `~/utono/linux-lit/linux-lit-release.log` |
| App ID | `com.utono.linux-lit.dev` | `com.utono.linux-lit` |
| Clear log | On launch | On launch |

## Changes Required

### New file: `.cargo/config.toml`

```toml
[env]
LIT_DEV = "1"
```

### `src/main.rs`

- Read `LIT_DEV` env var to determine mode
- Select log path and application ID based on mode
- Pass mode info to config layer

### `src/config.rs`

- `load` and `save` use the config path determined by mode (dev or release)
- Config schema unchanged (flat struct, single window)

## What Does Not Change

- Single-window behavior (multi-window is Step 2)
- AppState, keymap, navigation, MPV logic
- Config schema
- Database path

## Shell Aliases

In `~/utono/shell-config/.config/shell/alias-mlj`:

- `cbr` → `cargo build --release` (build release binary, run from project dir)
- `llit` → `~/utono/linux-lit/target/release/linux-lit` (launch release instance from anywhere)
- `cr` already exists → `cd ~/utono/linux-lit && cargo run` (dev mode)

## Result

User can `cbr` to build, `llit` to launch for reading, and `cr` for dev. Each mode has its own config, log, and GTK application ID.

# linux-lit

GTK4 Rust literature reader with e-reader pagination, MPV audio sync, and vim-style navigation.

## Debug Log

The app writes debug logs to:

- **Dev build** (`cargo run`): `~/utono/linux-lit/linux-lit-dev.log`
- **Release build**: `~/utono/linux-lit/linux-lit-release.log`

The log is cleared on every app launch. Use `log_fmt!()` macro (from `src/logging.rs`) to add log lines.

When fixing bugs, **always read the log first** before proposing changes:

```bash
cat ~/utono/linux-lit/linux-lit-dev.log
```

## Build & Run

Verify changes compile with `cargo build` but do not run the app — the user will run `cargo run` themselves.

**Important:** `cargo run` is for development only. Only run one instance at a time — multiple instances share the same log file and database, and restarting one won't update the other.

```bash
cargo build
```

## Testing

```bash
cargo test
cargo clippy
```

## Key Files

- `src/main.rs` — entry point, Tokio runtime, channel bridge
- `src/app.rs` — GTK4 window, AppState, display_work
- `src/config.rs` — ~/.config/linux-lit/config.json persistence
- `src/input/keymap.rs` — key event routing, gg state machine
- `src/input/navigation.rs` — cursor movement, page turns, scroll logic
- `src/db/queries.rs` — SQLite queries (list_works, load_work)
- `src/db/line_types.rs` — dialogue classification
- `src/ui/library_picker.rs` — Ctrl+p work picker with fuzzy filter
- `src/logging.rs` — file-based debug logging

## Keyboard Layout

The user's keyboard layout is Real Programmers Dvorak, defined in `~/utono/rpd`. Keys like `[` and `{` are on separate physical keys (not shift-related). Check the layout when adding keybinds.

## Searching for Keybinds

When searching for a keybind in linux-lit, **always check source** — primarily `src/input/keymap.rs` and the handlers in `src/input/` it dispatches to. **Do not use the `keybinds-search` skill or query `~/utono/keybinds/keybinds.db`** for this project; that database is not the source of truth for linux-lit binds and may be stale or incomplete. The Rust source is authoritative.

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-write)
- Themes: `~/utono/themes/.config/themes/themes-unified.json`
- Config: `~/.config/linux-lit/config.json`

# linux-lit

GTK4 Rust literature reader with e-reader pagination, MPV audio sync, and vim-style navigation.

## Debug Log

The app writes debug logs to:

```
~/utono/linux-lit/linux-lit.log
```

The log is cleared on every app launch. Use `log_fmt!()` macro (from `src/logging.rs`) to add log lines.

When fixing bugs, **always read the log first** before proposing changes:

```bash
cat ~/utono/linux-lit/linux-lit.log
```

## Build & Run

```bash
cd ~/utono/linux-lit && cargo run 2>&1
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

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-only)
- Themes: `~/utono/themes/.config/themes/themes-unified.json`
- Config: `~/.config/linux-lit/config.json`

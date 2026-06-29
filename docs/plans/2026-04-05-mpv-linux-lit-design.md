# mpv-linux-lit Design Spec

**Date:** 2026-04-05
**Project:** ~/utono/mpv-linux-lit (new repository)
**Approach:** Fresh codebase, copy modules selectively from linux-lit

## Purpose

A fast GTK4+Rust literature reader optimized for quick .txt file loading and instant work switching. Unlike linux-lit, it skips the expensive line-mapping step (associating every text line with a database timestamp). Instead, timestamps are looked up on-demand when the user presses Tab.

## Key Differences from linux-lit

- No `text_file_map.rs` — no line-to-timestamp pre-mapping
- No gutter/sign column
- No e-reader pagination — scroll mode only
- No visual selection mode, no Ollama integration
- Buffer ring for holding multiple works in memory with instant switching
- On-demand playback via Tab (normalized text query)
- `,`/`q` navigate dialogue lines without seeking MPV
- Pre-launched MPV instances (one per work, via `lit-prelaunch-mpv.sh`)

## Architecture

### Two-Runtime Design

- **GTK4 main thread:** UI, key events, text rendering
- **Tokio thread:** MPV IPC connections (one task per open work), async I/O
- **Channel bridge:** `tokio::sync::mpsc` for GTK-to-Tokio commands, `glib::spawn_future_local` for Tokio-to-GTK events

### Buffer Ring

```
BufferRing {
    buffers: Vec<LoadedWork>,
    active: usize,
}

LoadedWork {
    abbrev: String,
    title: String,
    author: String,
    text: String,              // Raw .txt file content
    lines: Vec<Line>,          // From DB (for timestamp lookup, dialogue detection)
    media_socket: Option<PathBuf>,
    cursor_line: usize,
    vocab_words: HashSet<String>,
}
```

- Works added to ring via Ctrl+p picker
- No ring size limit (works are lightweight)
- Cursor position preserved per work on switch

### Display Pipeline (on work load/switch)

1. Read .txt file from disk
2. `buffer.set_text()` — set GTK buffer
3. Apply dialogue formatting (speaker detection, indentation, smallcaps)
4. Apply vocab highlighting (from `work_vocab` table)
5. Restore cursor position and scroll
6. Connect to work's MPV socket (lazy, kept alive while in ring)

No line mapping, no gutter build, no timestamp pre-scan.

### On-Demand Playback (Tab)

1. Get current buffer line text
2. Normalize (lowercase, strip brackets, collapse whitespace)
3. Query lit.db:
   ```sql
   SELECT lt.start_time
   FROM line_timestamps lt
   JOIN line_mapping lm ON lt.line_mapping_id = lm.id
   WHERE lm.work_abbrev = ?
     AND lm.normalized_text = ?
   ORDER BY lt.start_time ASC
   LIMIT 1
   ```
4. If found, send seek + resume to active work's MPV socket
5. If no match, do nothing

Single indexed query, sub-millisecond.

**Duplicate text trade-off:** Multiple lines may share identical normalized text. `LIMIT 1 ORDER BY start_time` picks the first occurrence. Acceptable given the simplicity goal.

### MPV Connection Management

- Socket paths derived using same convention as `lit-prelaunch-mpv.sh`: `/tmp/mpvsocket-{author}-{filename}`
- Truncation with SHA256 hash suffix at 95 chars
- One Tokio task per connected socket, running concurrently
- Only the active work's task sends CursorSync events to UI
- Connections established lazily on first switch, kept alive while in ring

### Work Discovery

- Library picker (Ctrl+p) queries `works` table filtered to rows where `text_file IS NOT NULL`
- .txt file path read from `text_file` column in `works` table

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k` | Cursor down/up |
| `h`/`l` | Page left/right |
| `gg`/`G` | Jump to start/end |
| `/` | Search |
| `n`/`N` | Next/prev search match |
| `Space` | Play/pause MPV |
| `[`/`]` | Seek back/forward |
| `Tab` | Play from current line's start_time |
| `-`/`_` | Next/prev work in buffer ring |
| `,`/`q` | Prev/next dialogue line (no seek) |
| `r`/`R` | Concordance word cycling |
| `Ctrl+p` | Library picker |
| `Ctrl+Shift+p` | Concordance word picker |
| `Ctrl+Alt+p` | Concordance occurrence list |
| `Ctrl+/` | Keybinds overlay |
| `\` | Vocab popup |
| `Ctrl+l` | Toggle dialogue indentation |
| `Ctrl+f` | Filter by speaker |
| `f`/`F` | Font cycle forward/backward |

## Module Structure

```
src/
  main.rs              # Entry point, Tokio runtime, channel bridge
  app.rs               # GTK4 window, AppState, display_work, buffer ring
  config.rs            # ~/.config/mpv-linux-lit/config.json
  theme.rs             # Theme loading from themes-unified.json
  logging.rs           # File-based debug logging
  mode.rs              # Dev vs Release detection
  db/
    models.rs          # Work, Line, Timestamp structs
    queries.rs         # list_works, load_work, timestamp lookup
    line_types.rs      # Dialogue classification
    concordance.rs     # Concordance queries
  input/
    keymap.rs          # Key dispatcher, gg state machine
    navigation.rs      # Cursor, scrolling, dialogue nav
    timestamps.rs      # Tab -> on-demand playback
    search.rs          # / search
  mpv/
    client.rs          # Tokio async IPC, one task per socket
    discovery.rs       # Socket path derivation (match lit-prelaunch-mpv.sh)
    commands.rs        # MpvCommand & MpvEvent enums
  ui/
    library_picker.rs  # Ctrl+p (filtered to text_file works)
    search_bar.rs      # Search UI
    settings_overlay.rs
    keybinds_overlay.rs
    vocab_popup.rs
    concordance_picker.rs
    concordance_bar.rs
```

## Dependencies

```toml
gtk4 = "0.9"
libadwaita = "0.7"
sourceview5 = "0.9"
pango = "0.20"
glib = "0.20"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.33", features = ["bundled"] }
regex = "1"
sha2 = "0.10"
```

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-write)
- Themes: `~/utono/themes/.config/themes/themes-unified.json`
- Config: `~/.config/mpv-linux-lit/config.json`
- Literature: `~/utono/literature/` (.txt files referenced by `works.text_file`)

## UI Features Included

- Theme system (themes-unified.json) with font cycling
- Dialogue formatting (speaker detection, indentation, smallcaps)
- Vocab highlighting from lit.db
- Concordance picker and word cycling
- Translation display
- Search bar
- Settings overlay
- Keybinds overlay (Ctrl+/)
- Window title shows work title + ring position (e.g., "Hamlet [2/5]")

## UI Features Excluded

- Gutter / sign column
- E-reader pagination (scroll mode only)
- Visual selection mode / action popup
- Ollama integration
- Loading mask overlay (loads are fast enough)
- A/B repeat

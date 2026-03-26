# linux-lit Design Specification

**Date:** 2026-03-26
**Status:** Approved
**Port of:** ~/utono/macos-lit (LitReader for macOS)

## Overview

linux-lit is a single-window GTK4 literature reader for Arch Linux, written in Rust. It renders literary texts with proportional serif typography, syncs cursor position with MPV audio playback via Unix domain sockets, and supports vim-style navigation. It is a Linux port of the macOS LitReader design.

## Technology Stack

- **Language:** Rust
- **UI framework:** gtk4-rs (GTK4 Rust bindings)
- **Async runtime:** Tokio (background thread for MPV socket I/O and DB queries)
- **Database:** rusqlite (SQLite, read-only access to lit.db)
- **Text rendering:** GtkTextView with Pango font descriptions
- **Serialization:** serde + serde_json
- **Bridge:** glib::MainContext::channel (Tokio → GTK), tokio::sync::mpsc (GTK → Tokio)

### Key Crates

- `gtk4` — UI framework
- `rusqlite` — SQLite access
- `tokio` — async runtime, unix sockets
- `serde`, `serde_json` — JSON parsing
- `pango` — font control via gtk4-rs bindings

## Architecture

### Runtime Model

Two runtimes connected by typed channels:

- **GTK4 main loop** — owns all UI: window, GtkTextView, overlays (theme picker, library picker, search)
- **Tokio runtime** — runs on a background thread, owns MPV socket I/O and async DB queries
- **Channel bridge** — `tokio::sync::mpsc` channels for UI → Tokio commands; `glib::MainContext::channel` for Tokio → GTK events

### Data Flow for Playback Sync

```
MPV socket → Tokio task (time-pos observer) → glib channel → GTK main loop → timestamp lookup → update cursor highlight
```

### Commands (UI → Tokio)

- `Seek(f64)` — seek to timestamp
- `TogglePause` — toggle pause state
- `LoadFile(String)` — load new media file
- `Connect(String)` — connect to socket path
- `Disconnect`

### Events (Tokio → GTK)

- `CursorSync(usize)` — move cursor to line index
- `ConnectionStatus(bool)` — connected/disconnected
- `PlaybackState(bool)` — playing/paused

## Text Rendering & Display

- GtkTextView in read-only mode
- Proportional serif fonts via Pango: Georgia, Palatino, Garamond, Noto Serif, Liberation Serif (graceful fallback)
- Centered text column: left/right margins calculated to achieve ~700-750px content width, recalculated on window resize
- Line spacing 1.6x via `pixels_above_lines` / `pixels_below_lines`
- No editable text, no cursor blink, no scrollbar, no line numbers
- Keyboard events intercepted at window level via GtkEventControllerKey

### Cursor Line Highlight

- A GtkTextTag applied to the current line's text range
- Tag sets `background` to the theme's CursorLine color (with alpha for subtle tint)
- Previous line's tag removed before new one applied

### Line Content

- Each line from `line_mapping.canonical_text` rendered as a paragraph in the text buffer
- Speaker lines, stage directions, and dialogue all rendered visually
- Comma/q navigation skips non-dialogue lines
- No per-word styling in v1 (future: vocab word highlighting via additional GtkTextTags)

### Font Cycling

- Ordered list of serif fonts in config.json
- `Ctrl+Shift+f` cycles through available fonts
- Font applied by updating the GtkTextView's Pango font description
- Current font + size persisted to config.json

## Database Layer

### Connection

`rusqlite::Connection` opened once at startup against `~/utono/litdb/data/lit.db` (read-only). All queries run via `tokio::task::spawn_blocking` to avoid blocking the GTK loop.

### Core Queries

- **Library listing:** `SELECT abbrev, title, author, work_type FROM works ORDER BY title`
- **Load work:** `SELECT canonical_text, normalized_text, speaker, source_file, source_line FROM line_mapping WHERE work_abbrev = ? ORDER BY div1, div2, line_in_div`
- **Timestamps:** `SELECT lm.rowid, lt.start_time, lt.end_time, lt.media_id FROM line_mapping lm JOIN line_timestamps lt ON lt.line_mapping_id = lm.rowid WHERE lm.work_abbrev = ?`
- **Media lookup:** `SELECT mf.path FROM media_files mf JOIN work_media_associations wma ON wma.media_id = mf.id WHERE wma.work_abbrev = ? ORDER BY wma.priority`

### In-Memory Model

After loading a work, all data is held in memory:

```rust
struct Work {
    abbrev: String,
    title: String,
    author: String,
    work_type: String,
    lines: Vec<Line>,
    timestamps: Vec<Timestamp>,  // sorted by start_time for binary search
    media_paths: Vec<String>,
}

struct Line {
    id: i64,                     // line_mapping rowid
    text: String,                // canonical_text
    normalized: String,          // for search matching
    speaker: Option<String>,
    is_dialogue: bool,           // precomputed from line_types logic
    timestamp: Option<TimeRange>,
}

struct TimeRange {
    start: f64,
    end: f64,
}
```

### Dialogue Classification

Precomputed on work load, ported from `lit`'s `line_types.lua`:

- **Blank:** empty or whitespace-only
- **Speaker:** all-uppercase name pattern (e.g., `HAMLET.`, `FIRST GENTLEMAN`)
- **Stage direction:** wrapped in brackets `[...]` (non-prose works only)
- **Act/scene marker:** starts with `ACT `, `SCENE `, `PROLOGUE`, `EPILOGUE`
- **Separator:** starts with `=`
- **Prose works:** all non-blank lines treated as dialogue

A line is dialogue if it matches none of the above skip patterns.

## MPV Integration

### Socket Discovery (5-step priority chain)

Ported from `lit`'s `mpv_sockets.lua`:

1. **Deterministic prediction** — derive socket path from media file path in the database: `/tmp/mpvsocket-<author>-<basename>`. For yt-dlp media: `mpvsocket-ytdlp-<author>-<basename>`. If path > 95 chars: truncate to 87 chars + 7-char SHA256 suffix
2. **Scan fallback** — list `/tmp/mpvsocket-*`, stat each to confirm socket type
3. **IPC probe** — query each live socket's `path` property, match against work's media files
4. **Single-socket fallback** — if exactly one socket exists in `/tmp`, use it
5. **Fail gracefully** — no socket found, app works as pure reader

### MPV Launch

When no live socket exists for the current work's media, spawn headless MPV:

```
mpv --input-ipc-server=<derived-socket> --pause --no-video --no-terminal <media-path>
```

Poll for socket existence at 50ms intervals, up to 3 seconds, then connect.

### Tokio MPV Task

- Connects via `tokio::net::UnixStream`
- Registers `time-pos` observer: `{"command":["observe_property",1,"time-pos"]}`
- Reads newline-delimited JSON responses in a loop
- On each `time-pos` event: binary search the timestamps vec, send line index to GTK via glib channel
- Handles connection drops gracefully — sets status to disconnected, retries on next user action

### JSON-RPC Protocol

Commands are newline-terminated JSON over Unix domain socket:
- Get property: `{"command":["get_property","path"]}`
- Seek: `{"command":["seek",<time>]}`
- Pause toggle: `{"command":["cycle","pause"]}`
- Observe: `{"command":["observe_property",<id>,"<name>"]}`

Responses: `{"data":<value>,"request_id":<id>,"error":"success"}`

## Theme System

### Source

`~/utono/themes/.config/themes/themes-unified.json` — 35 themes, loaded at startup.

### Color Mapping

- `dwl.rootcolor` → window background (outer frame behind text area)
- `kitty.background` → text area background
- `nvim.highlights.Cursor.guibg` → cursor indicator color
- `nvim.highlights.Cursor.guifg` → cursor text color
- `nvim.highlights.CursorLine.guibg` → current line background tint
- `nvim.highlights.VocabWord.guifg` → vocab word color (future use)
- `nvim.text_foreground` → main text foreground color (must be added to themes JSON)

### Application

Theme colors applied via GtkCssProvider — dynamically generated CSS string injected on theme change. The cursor line highlight uses a GtkTextTag with the CursorLine background color.

### Theme Switcher (`Ctrl+Shift+\`)

- Modal overlay with filtered list of theme names
- Type-to-filter with fuzzy matching
- `j`/`k` or arrow keys to navigate, Enter to confirm, Escape to revert
- Live preview: CSS applied immediately as selection changes
- Persisted to config.json on confirm

### Startup

Load last-used theme from config.json. Fall back to first theme in JSON file if missing.

## Navigation & Keybindings

### Key Event Handling

A `GtkEventControllerKey` attached to the window. All key events intercepted before reaching GtkTextView. A state machine tracks multi-key sequences (`gg`, leader combos).

### Keymap

**Navigation (no modifier):**
- `j` / `k` — move cursor one line down/up, scroll to keep visible
- `gg` — jump to first line (two-key sequence with timeout)
- `G` — jump to last line
- `Ctrl+d` / `Ctrl+u` — half-page down/up
- `Ctrl+f` / `Ctrl+b` — full-page down/up
- `,` — prev dialogue line (skip non-dialogue, seek MPV with 200ms preroll)
- `q` — next dialogue line (same filtering + seek)
- `/` — open search overlay
- `n` / `N` — next/prev search match

**MPV playback:**
- `Tab` — toggle pause
- `,` / `q` — after dialogue jump, seek MPV to `start_time - 0.2s`

**Leader (`Space`):**
- Reserved for future compound bindings

**App controls:**
- `Ctrl+Shift+\` — theme switcher
- `Ctrl+Shift+f` — cycle font
- `Ctrl+Shift+!` — font size down
- `Ctrl+Shift+|` — font size up
- `Ctrl+Shift+0` — reset font size
- `Ctrl+p` — library picker
- `Ctrl+Shift+l` — quit

### Dialogue Jump Logic

Ported from `lit`'s `line_types.lua` and `navigation.lua`:

- Line type predicates precomputed per line on work load (`is_dialogue` field)
- `,` scans backward from cursor, skips non-dialogue lines
- `q` scans forward from cursor, skips non-dialogue lines
- After landing on dialogue line, seek MPV to `start_time - 0.2s` if timestamp exists

## Library Picker (`Ctrl+p`)

- Modal overlay (same pattern as theme switcher)
- Lists all works from the `works` table: title, author, work_type
- Type-to-filter with fuzzy matching
- `j`/`k` or arrow keys to navigate, Enter to open, Escape to dismiss
- Opening a work replaces current buffer — loads all lines, timestamps, and media paths

## Search (`/`)

- Inline search bar at bottom of window
- Searches `normalized_text` across loaded lines (in-memory)
- Highlights all matches with a GtkTextTag
- `n`/`N` cycles through matches, scrolling to each
- Escape dismisses search and clears highlights

## Configuration

### File Location

`~/.config/linux-lit/config.json`

### Schema

```json
{
  "theme": "gruvbox-material-dark-medium",
  "font_family": "Georgia",
  "font_size": 18,
  "last_work": "ham",
  "last_line": 342
}
```

### Behavior

- Created with defaults on first launch if missing
- Updated on theme change, font change, and work close/quit
- Human-editable, suitable for version control via tty-dotfiles/stow

## Project File Structure

```
~/utono/linux-lit/
  Cargo.toml
  src/
    main.rs              # GTK app entry, Tokio runtime setup, channel bridge
    app.rs               # GtkApplication setup, window creation
    ui/
      mod.rs
      text_view.rs       # GtkTextView config, cursor highlight, font application
      theme_picker.rs    # Ctrl+Shift+\ overlay
      library_picker.rs  # Ctrl+p overlay
      search.rs          # / search bar
    db/
      mod.rs
      models.rs          # Work, Line, Timestamp structs
      queries.rs         # All SQL queries
      line_types.rs      # Dialogue classification predicates
    mpv/
      mod.rs
      discovery.rs       # Socket path derivation, scanning, probing
      client.rs          # Tokio UnixStream, JSON-RPC, time-pos observer
      commands.rs        # Command/Event enums for channel bridge
    input/
      mod.rs
      keymap.rs          # Key event handler, state machine for gg/leader
      navigation.rs      # j/k, scroll, dialogue jump, search next/prev
    theme/
      mod.rs
      loader.rs          # Parse themes-unified.json
      css.rs             # Generate GTK CSS from theme colors
    config.rs            # Load/save ~/.config/linux-lit/config.json
  docs/
```

## External Dependencies

All cross-platform, no changes needed from macOS:

- **Database:** `~/utono/litdb/data/lit.db` (212 MB, 133 works, 309K lines)
- **Themes:** `~/utono/themes/.config/themes/themes-unified.json` (35 themes)
- **Media files:** `~/Music/<author>/<title>.*` (MP3, M4B)
- **MPV:** installed system-wide, socket convention unchanged on Linux

### Prerequisite

The `nvim.text_foreground` field must be added to all 35 themes in `themes-unified.json` before theme rendering can be fully implemented. This field provides the main text foreground color, extracted from each Neovim colorscheme's Normal highlight group.

## Non-Goals (v1)

- Sign column (chapter markers, timestamp indicators)
- Gloss/vocab annotation display
- AB loop / chunk playback controls
- Vocab word highlighting
- Text editing
- Multi-window support
- Period key chapter marking (`.` from `lit`)
- `f`/`F` character-following navigation

# linux-lit Implementation Guide

**Date:** 2026-03-26
**Spec:** docs/2026-03-26-linux-lit-spec.md
**Audience:** AI coding agent (Claude Code or similar)

This document provides a phased implementation plan for building linux-lit from the spec. Each phase is self-contained and produces a testable milestone. Complete phases in order.

## Prerequisites

Before starting implementation:

1. Ensure the following are installed on Arch Linux:
   - `rust` (rustup with stable toolchain)
   - `gtk4` (system package)
   - `mpv`
   - `sqlite`

2. Verify external data exists:
   - `~/utono/litdb/data/lit.db` — SQLite database (read-only access)
   - `~/utono/themes/.config/themes/themes-unified.json` — theme definitions

3. The `nvim.text_foreground` field must be present in themes-unified.json. If missing, extract the Normal foreground color from each Neovim colorscheme and add it to all 35 theme entries.

## Phase 1: Project Scaffold & Window

**Goal:** A GTK4 window opens with an empty GtkTextView configured for reading.

**Steps:**

1. Initialize Cargo project in `~/utono/linux-lit/`:
   - `Cargo.toml` with dependencies: `gtk4`, `tokio` (features: full), `rusqlite` (features: bundled), `serde`, `serde_json`
   - Create `src/main.rs`, `src/app.rs`

2. In `main.rs`:
   - Initialize GTK application
   - Spawn Tokio runtime on a background thread
   - Create the glib channel bridge (Tokio → GTK) and mpsc channel (GTK → Tokio)
   - Connect `activate` signal to window creation

3. In `app.rs`:
   - Create `GtkApplicationWindow` with default size 1000x800
   - Create `GtkTextView` in read-only mode
   - Configure: no cursor blink, no editable, wrap mode none
   - Set left/right margins for centered ~700px column
   - Set `pixels_above_lines` / `pixels_below_lines` for 1.6x line spacing
   - Set initial Pango font description to "Georgia 18" (or first available serif)
   - Attach `GtkEventControllerKey` to window (stub handler, logs key presses)
   - No scrollbar: set GtkScrolledWindow policy to never/external

**Test:** Run `cargo run`. A window appears with an empty, serif-font text area. Key presses logged to stdout.

## Phase 2: Database & Work Loading

**Goal:** Open the library picker, select a work, display its text.

**Steps:**

1. Create `src/db/models.rs`:
   - Define structs per spec: `Work`, `Line`, `TimeRange`, `Timestamp { line_id: i64, start: f64, end: f64, media_id: i64 }`
   - Derive `Debug`, `Clone` as needed

2. Create `src/db/line_types.rs`:
   - Port dialogue classification from `lit`'s `line_types.lua`:
     - `is_blank(text) -> bool` — empty or whitespace-only
     - `is_speaker(text) -> bool` — two patterns:
       1. Simple: `^[A-Z][A-Z\s.\-']+\.?$` (min 2 chars after stripping trailing period)
       2. With stage direction: `^[A-Z][A-Z\s\-']*,?\s*\[.*\]\.?$`
     - `is_stage_direction(text) -> bool` — regex: `^\[.*\]$` (non-prose only)
     - `is_act_scene_marker(text) -> bool` — starts with ACT/SCENE/PROLOGUE/EPILOGUE
     - `is_separator(text) -> bool` — starts with `=`
     - `is_prose_work(work_type) -> bool` — true for: `novel`, `essay_collection`, `prose_book`, `prose`
     - `classify_line(text, is_prose: bool) -> bool` — for prose works, return true if non-blank; for non-prose, return true if none of the above patterns match
   - Reference: `/home/mlj/utono/lit/plugins/lua/lit_keymaps/line_types.lua`

3. Create `src/db/queries.rs`:
   - Open `rusqlite::Connection` to `~/utono/litdb/data/lit.db` with `OpenFlags::SQLITE_OPEN_READ_ONLY`
   - `list_works() -> Vec<(String, String, String, String)>` — abbrev, title, author, work_type
   - `load_work(abbrev: &str) -> Work` — runs load work + timestamps + media queries, precomputes `is_dialogue` for each line
   - All queries wrapped in functions that can be called from `spawn_blocking`

4. Create `src/ui/library_picker.rs`:
   - Modal overlay widget listing works
   - Type-to-filter with fuzzy matching
   - `j`/`k` and arrow key navigation, Enter to select, Escape to dismiss

5. Wire it up:
   - On app startup or `Ctrl+p`: show library picker
   - On work selection: call `load_work` via spawn_blocking, populate GtkTextBuffer with all lines
   - Each line is a paragraph (appended with newline)

**Test:** Run app, press `Ctrl+p`, filter for "ham", select Hamlet. Full text of Hamlet appears in the serif-rendered text view.

## Phase 3: Navigation

**Goal:** Vim-style movement through the text.

**Steps:**

1. Create `src/input/keymap.rs`:
   - Key event handler connected to GtkEventControllerKey
   - State machine for multi-key sequences:
     - Track pending `g` for `gg` with ~500ms timeout
     - Track `Space` for leader prefix (future use)
   - Route key events to navigation functions

2. Create `src/input/navigation.rs`:
   - `move_cursor(direction: i32)` — move by N lines, update cursor highlight
   - `jump_to_start()` / `jump_to_end()` — gg / G
   - `scroll_half_page(direction: i32)` — Ctrl+d / Ctrl+u
   - `scroll_full_page(direction: i32)` — Ctrl+f / Ctrl+b
   - `jump_to_prev_dialogue()` / `jump_to_next_dialogue()` — `,` / `q`
     - Scan backward/forward skipping lines where `is_dialogue == false`
   - All movement functions scroll the text view to keep cursor line visible

3. Create `src/ui/text_view.rs` cursor highlight logic:
   - Maintain a `current_line: usize` state
   - On navigation: remove GtkTextTag from previous line, apply to new line
   - Tag uses theme's CursorLine background color

**Test:** Load a work. `j`/`k` moves line by line with highlight. `gg`/`G` jump to start/end. `,`/`q` skip stage directions and speaker lines, landing only on dialogue.

## Phase 4: Theme System

**Goal:** Load themes, apply colors, switch themes with live preview.

**Steps:**

1. Create `src/theme/loader.rs`:
   - Parse `~/utono/themes/.config/themes/themes-unified.json`
   - Extract color fields per spec mapping (dwl.rootcolor, kitty.background, etc.)
   - Return `Vec<Theme>` where each Theme has name + all mapped colors

2. Create `src/theme/css.rs`:
   - `generate_css(theme: &Theme) -> String` — produces GTK CSS:
     - `window { background-color: <rootcolor>; }`
     - `textview text { background-color: <kitty.background>; color: <text_foreground>; }`
     - Font family and size from current config
   - Apply via `GtkCssProvider::load_from_string`, add to display

3. Create `src/ui/theme_picker.rs`:
   - Modal overlay listing theme names (same pattern as library picker)
   - Fuzzy filter, j/k navigation
   - On selection change: apply CSS immediately (live preview)
   - Enter confirms and persists to config, Escape reverts to previous theme

4. Create `src/config.rs`:
   - Load/save `~/.config/linux-lit/config.json`
   - Create with defaults if missing
   - Serde structs matching the config schema

**Test:** Launch app. Theme applied from config. `Ctrl+Shift+\` opens picker. Typing filters themes. Arrow through themes — colors change live. Enter persists, Escape reverts.

## Phase 5: MPV Integration

**Goal:** Connect to MPV, sync cursor with playback, toggle pause, seek on dialogue jump.

**Steps:**

1. Create `src/mpv/discovery.rs`:
   - `derive_socket_path(media_path: &str) -> String` — port the deterministic naming from `lit`'s `mpv_sockets.lua`:
     - Extract author dir and basename from media path
     - Format: `/tmp/mpvsocket-<author>-<basename>`
     - yt-dlp prefix variant
     - Truncation + SHA256 suffix for paths > 95 chars
   - `scan_sockets() -> Vec<PathBuf>` — list `/tmp/mpvsocket-*`, stat to confirm socket type
   - `probe_socket(path: &Path) -> Option<String>` — connect and query `path` property
   - `find_socket_for_work(work: &Work) -> Option<PathBuf>` — run the 5-step priority chain
   - `launch_mpv(socket_path: &str, media_path: &str)` — spawn headless MPV, poll for socket

2. Create `src/mpv/commands.rs`:
   - Define `MpvCommand` enum: `Seek(f64)`, `TogglePause`, `LoadFile(String)`, `Connect(String)`, `Disconnect`
   - Define `MpvEvent` enum: `CursorSync(usize)`, `ConnectionStatus(bool)`, `PlaybackState(bool)`

3. Create `src/mpv/client.rs`:
   - Tokio task that:
     - Receives `MpvCommand` from mpsc channel
     - Manages `UnixStream` connection
     - On `Connect`: connect to socket, register `time-pos` observer
     - Read loop: parse newline-delimited JSON, on `time-pos` events binary-search timestamps vec, send `CursorSync(line_index)` to glib channel
     - On `TogglePause`: send `{"command":["cycle","pause"]}`
     - On `Seek`: send `{"command":["seek",<time>,"absolute"]}`
     - Handle disconnects: send `ConnectionStatus(false)`, await reconnect command

4. Wire into UI:
   - On work load: call `find_socket_for_work`, if none found and media exists, call `launch_mpv`, then send `Connect` command
   - On `CursorSync` event from glib channel: update cursor position + highlight
   - `Tab` key: send `TogglePause` command
   - `,`/`q` dialogue jump: after moving cursor, send `Seek(start_time - 0.2)` command

**Test:** Load a work that has timestamps and media. MPV launches (or connects to existing). Play audio — cursor follows line by line. Press Tab — playback pauses/resumes. Press `,` — jumps to prev dialogue and audio seeks.

## Phase 6: Search

**Goal:** In-buffer text search with highlighting.

**Steps:**

1. Create `src/ui/search.rs`:
   - Search bar widget at bottom of window (hidden by default)
   - `/` key shows it, Escape hides it
   - On text input: search `normalized_text` across all loaded lines
   - Highlight all matches with a dedicated GtkTextTag (distinct from cursor highlight)
   - Track match positions in a vec
   - `n` — move to next match, scroll to it
   - `N` — move to previous match
   - Escape clears highlights and hides search bar

**Test:** Load a work. Press `/`, type "to be". All matches highlighted. `n`/`N` cycles through them. Escape clears.

## Phase 7: Font Management

**Goal:** Cycle fonts, adjust size, persist preferences.

**Steps:**

1. Extend `src/config.rs`:
   - `font_family` and `font_size` fields (already in schema)
   - Default font list (canonical, matches spec): `["Georgia", "Noto Serif", "Liberation Serif", "Palatino", "Garamond", "DejaVu Serif"]`

2. Extend `src/ui/text_view.rs`:
   - `apply_font(family: &str, size: u32)` — update Pango font description on GtkTextView
   - `cycle_font()` — advance to next font in list, wrap around, apply
   - `change_font_size(delta: i32)` — adjust size, clamp to 8..72, apply
   - `reset_font_size()` — set to default (18), apply
   - After any change: persist to config.json

3. Wire keybindings:
   - `Ctrl+Shift+f` → `cycle_font()`
   - `Ctrl+Shift+!` → `change_font_size(-1)`
   - `Ctrl+Shift+|` → `change_font_size(1)`
   - `Ctrl+Shift+0` → `reset_font_size()`

**Test:** `Ctrl+Shift+f` cycles through available serif fonts. `Ctrl+Shift+|` increases size. Quit and relaunch — font persists.

## Phase 8: Polish & Startup

**Goal:** Smooth startup flow, last-work restoration, quit behavior.

**Steps:**

1. Startup flow:
   - Load config.json (create with defaults if missing)
   - Load themes
   - Apply saved theme
   - If `last_work` set: load that work, scroll to `last_line`
   - If no `last_work`: show library picker

2. On quit (`Ctrl+Shift+l`):
   - Save current work abbrev and line to config.json
   - Disconnect from MPV socket cleanly
   - Exit GTK application

3. Window resize:
   - Recalculate left/right margins to maintain centered ~700px column

4. Error handling:
   - Database not found: show error dialog with path, exit
   - Themes file not found: fall back to hardcoded dark theme
   - MPV connection failure: log warning, continue as pure reader
   - Invalid config.json: reset to defaults, log warning

**Test:** Full end-to-end: launch app, auto-loads last work, theme applied, audio syncs, navigate with vim keys, switch themes, change fonts, quit, relaunch — all state restored.

## Reference: Source Material

When implementing, consult these files from the existing `lit` Neovim plugin:

- **Line types:** `/home/mlj/utono/lit/plugins/lua/lit_keymaps/line_types.lua`
- **Dialogue navigation:** `/home/mlj/utono/lit/plugins/lua/lit_keymaps/navigation.lua`
- **MPV socket discovery:** `/home/mlj/utono/lit/plugins/lua/lit_core/mpv_sockets.lua`
- **MPV communication:** `/home/mlj/utono/lit/plugins/lua/lit_core/mpv_communication.lua`
- **MPV media resolution:** `/home/mlj/utono/lit/plugins/lua/lit_core/mpv_media.lua`
- **macOS design spec:** `/home/mlj/utono/macos-lit/docs/2026-03-24-litreader-design.md`

## Reference: Database Schema

Key tables in `~/utono/litdb/data/lit.db`:

- `works` — PK: `abbrev`. Fields: `title`, `author`, `work_type`, `div1_label`, `div2_label`
- `line_mapping` — FK: `work_abbrev`. Fields: `div1`, `div2`, `line_in_div`, `canonical_text`, `normalized_text`, `source_file`, `source_line`, `speaker`
- `line_timestamps` — FK: `line_mapping_id`, `media_id`. Fields: `start_time`, `end_time`, `is_chapter`, `is_scene_start`
- `media_files` — Fields: `path`, `work_abbrev`, `duration_seconds`
- `work_media_associations` — FK: `work_abbrev`, `media_id`. Fields: `display_name`, `priority`

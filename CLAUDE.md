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

- `src/main.rs` — entry point, Tokio runtime, channel bridge, MPV event loop (TimePos, PlaybackState, ConnectionStatus)
- `src/app.rs` — GTK4 window, AppState, display_work, clear_display, prepare_text_for_display
- `src/config.rs` — ~/.config/linux-lit/config.json persistence
- `src/input/keymap.rs` — key event routing, gg state machine, dispatch_action
- `src/input/keymap_config.rs` — compiled-in default keybinds, keymap.json loader
- `src/input/navigation.rs` — cursor movement, page turns, scroll logic
- `src/input/actions/mod.rs` — Action enum with all reader-mode actions
- `src/input/actions/concordance.rs` — concordance picker, cross-work navigation, r/R handlers
- `src/input/actions/pickers.rs` — library/media/bookmark picker open/confirm handlers
- `src/input/highlight.rs` — update_highlight, update_highlight_and_center
- `src/input/scroll.rs` — set_page, set_page_instant, center_cursor
- `src/concordance.rs` — ConcordanceState, ConcordanceHit, advance/retreat
- `src/db/queries.rs` — SQLite queries (list_works, load_work)
- `src/db/concordance.rs` — find_word_occurrences, load_concordance_words
- `src/db/stopwords.rs` — English stopword list for concordance filtering
- `src/db/line_types.rs` — dialogue classification
- `src/mpv/client.rs` — MPV IPC command handler (Seek, LoadFile, ResumeAndSeek, Connect, Quit)
- `src/mpv/commands.rs` — MpvCommand and MpvEvent enums
- `src/mpv/discovery.rs` — derive_socket_path, find_socket_for_work, launch_mpv
- `src/ui/library_picker.rs` — Ctrl+p work picker with fuzzy filter
- `src/ui/concordance_picker.rs` — Ctrl+\ concordance word picker
- `src/ui/media_picker.rs` — Ctrl+Shift+M media file picker
- `src/logging.rs` — file-based debug logging

## Keyboard Layout

The user's keyboard layout is Real Programmers Dvorak (RPD), defined in
`~/utono/rpd`. **Always check `~/utono/rpd` when adding or changing keybinds** —
on RPD, characters like `[`, `{`, `(`, and `4` may sit on separate physical
keys (not shift-related), and the GTK key name a physical key emits is not
always obvious from the character. Consult the layout there to map a character
to its physical key and the GTK key name to use in `keymap_config.rs` /
`keymap.json` (e.g. `(` → `parenleft`, `'` → `apostrophe`).

## Searching for Keybinds

When searching for a keybind in linux-lit, **always check source** — primarily `src/input/keymap.rs` and the handlers in `src/input/` it dispatches to. **Do not use the `keybinds-search` skill or query `~/utono/keybinds/keybinds.db`** for this project; that database is not the source of truth for linux-lit binds and may be stale or incomplete. The Rust source is authoritative.

## Concordance System

Cross-work concordance navigation for searching word occurrences across an author's works.

- **Ctrl+\\** — opens concordance picker with stopword-filtered word list for the current author
- **r / R** — next/prev concordance hit (cross-work, loads new work in-place). Falls back to "no concordance active" toast if no word selected. Seeks MPV to the hit line's own start time (not sentence start).
- **Ctrl+r / Ctrl+Shift+R** — next/prev vocab word jump (always, ignores concordance state)
- Word list is cached per author in `AppState.concordance_word_cache`
- Cross-work jumps open the media picker so the user chooses the audio file
- Single-media works auto-select without showing the picker
- `concordance_state` persists until a new word is selected

Key files: `src/input/actions/concordance.rs`, `src/concordance.rs`, `src/db/concordance.rs`, `src/ui/concordance_picker.rs`

### Keybind override: keymap.json takes precedence

Compiled-in defaults in `keymap_config.rs` are overridden by `~/.config/linux-lit/keymap.json` (stowed from `~/tty-dotfiles/linux-lit/`). When changing keybinds, **always update both files** or the JSON will silently override your compiled changes.

## MPV Integration

- MPV is reused across work switches via `loadfile replace` (no new process)
- `AppState.mpv_connected` tracks whether an IPC connection is active
- `AppState.mpv_playing` tracks playback state
- `AppState.pending_loadfile_seek` stores a deferred seek that fires on the first `TimePos` event after `loadfile` (event-driven, not timer-based)
- Socket paths are derived from media file paths: `/tmp/mpvsocket-{author}-{basename}`
- `display_work` skips MPV discovery when `skip_mpv_discovery` is set (used by concordance cross-work jumps that open the media picker instead)

Key files: `src/mpv/client.rs`, `src/mpv/commands.rs`, `src/mpv/discovery.rs`

### Scrolling after jumps

Use `update_highlight_and_center` (not `center_cursor` alone) when jumping the cursor to a distant line. `center_cursor` only sets the GTK vadjustment but doesn't update `page_top_line`, so the e-reader pagination state gets out of sync. `update_highlight_and_center` calls `set_page_instant` which updates both.

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-write)
- Themes: `~/utono/themes/.config/themes/themes-unified.json`
- Config: `~/.config/linux-lit/config.json`

## Keymap Configuration

Reader keybindings are loaded from `~/.config/linux-lit/keymap.json` at
startup. If the file is missing or malformed, linux-lit falls back to
compiled-in defaults (see `src/input/keymap_config.rs:default_reader_bindings`).

### Stow workflow

The canonical default keymap is shipped as a stow package at
`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`. Deploy with:

```bash
cd ~/tty-dotfiles && stow linux-lit
```

Restart linux-lit; the new bindings take effect on next launch.

### Customizing bindings

Edit `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (the stow
source). Each binding is an object: `{"key": "x", "action": "PageForward"}`.
Optional modifier flags: `"ctrl": true`, `"shift": true`, `"alt": true`.

Available actions are the variants of `crate::input::actions::Action` —
see `src/input/actions/mod.rs`. Unknown action names are skipped at load
with a logged warning; malformed JSON falls back to compiled-in defaults
entirely.

User overrides take precedence over defaults; bindings not present in
the JSON keep their compiled-in default.

## Reference Codebases

When debugging or designing features that overlap with other ebook readers, consult these read-only checkouts at `~/Documents/repos/linux-lit/`. They are reference material, not dependencies — **never import code, only patterns**. Re-clone with `git clone <url>` into that directory if missing.

Pick the reference by problem area, not by language:

- **Pagination / clipping / page-turn math** → `foliate-js/`
- **Audio-text sync (the closest analog to linux-lit's MPV workflow)** → `lue/` first, then `html5-audio-read-along/`, then `transcript-tracer-js/`
- **Vim-style EPUB reading in Rust** → `bk/`
- **Whisper-driven word timestamps & per-document audio storage model** → `openreader/`
- **Annotations / highlights / location addressing / selection-tools UX** → `foliate/`

### foliate — `~/Documents/repos/linux-lit/foliate/` + `foliate-js/`

GNOME ebook reader, JavaScript/GJS + WebKitGTK + libadwaita (~8-10k LOC shell + ~9-11k LOC vendored renderer). Different rendering stack (CSS multi-column inside a WebView), but solves many of the same problems linux-lit faces.

- **Pagination edge cases** (clipped descenders, last-fully-visible-line, partial bottom lines, scroll-vs-page mode) — `foliate-js/paginator.js` (~44 KB). Different engine, transferable algorithm.
- **Location addressing** (portable bookmarks, sub-line precision, cross-device sync) — `foliate-js/epubcfi.js` (~13 KB) is the standard EPUB CFI implementation. Reference design if linux-lit ever needs more than `line_mapping.id`.
- **Annotations / highlights data model** — `foliate/src/annotations.js` (~25 KB): bookmark + named-color highlight + note schema, CFI-anchored, with export.
- **Selection-tools pattern** (Wiktionary, Wikipedia, translate as isolated modules with a uniform interface) — `foliate/src/selection-tools.js` and `foliate/src/selection-tools/*.html`.
- **EPUB Media Overlays / SMIL audio sync** — `foliate-js` SMIL modules. Reference only if importing timestamps from EPUB3 audiobooks.
- **Theme JSON schema** — `foliate/src/themes.js` and the user-themes-as-JSON pattern.
- **Not useful for:** library management (per-book JSON, no SQLite), library picker UI (WebView-based), vim navigation, MPV-driven sync, settings overlay (GSettings).

Quick map: app entry `foliate/src/main.js`, `app.js`. Reader: `foliate/src/reader/reader.html` + `reader.js`. Largest file: `foliate/src/book-viewer.js` (~47 KB).

### lue — `~/Documents/repos/linux-lit/lue/`

Terminal ebook reader (Python, ~1.5k LOC) with **word-level TTS sync** — the closest in-language analog to linux-lit's audio/text sync workflow. Modular by responsibility, easy to read in one sitting.

- `lue/audio.py` — playback control (mirrors what linux-lit/mpv-linux-lit does)
- `lue/tts_manager.py` — TTS engine integration; reference for sync state machine
- `lue/timing_calculator.py` — **highest-value file**: how to map text positions to audio time and back. Read this when debugging linux-lit's deferred page-turn or stall-on-seek issues.
- `lue/content_parser.py` — EPUB/PDF/DOCX/HTML/RTF/TXT/MD ingestion. Reference if linux-lit ever ingests anything beyond `lit.db`.
- `lue/progress_manager.py` — bookmark/last-position persistence. Compare to linux-lit's `page_history` and bookmark schema.
- `lue/input_handler.py` — keybind dispatch in a TUI. Different from GTK but the dispatch shape is similar.

### bk — `~/Documents/repos/linux-lit/bk/`

Terminal EPUB reader in Rust (~1163 LOC across 3 files). Closest Rust-language analog. Tiny enough to read end-to-end.

- `src/main.rs` (426 lines) — argv handling, key event loop, vim-style keymap dispatch. Compare to `src/input/keymap.rs`.
- `src/view.rs` (444 lines) — viewport/scroll/page state. Compare to `src/input/navigation.rs` and `src/app.rs`'s display logic.
- `src/epub.rs` (~9.8 KB) — EPUB unzip + chapter splitting. Reference if linux-lit ever ingests EPUB.

### openreader — `~/Documents/repos/linux-lit/openreader/`

Next.js/TypeScript web app (~30k LOC) with **whisper.cpp word timestamps** and per-document audio. Most of it is unrelated to linux-lit (auth, S3 uploads, Drizzle ORM), but the audio-sync pieces are the most direct reference for linux-lit's manual-timestamp + sync workflow.

- `src/hooks/audio/` — audio playback hooks, time-update handling, seek behavior. Read when debugging playback sync stalls.
- `src/components/player/` — the read-along UI: word/line highlight driven by audio time. Compare to linux-lit's cursor advancement under MPV sync.
- `src/hooks/epub/` and `src/hooks/html/` — content-to-timestamp mapping, chunked. Useful pattern even though linux-lit's chunks come from `lit.db`, not whisper.
- **Skip:** auth, billing, S3, Drizzle, Tailwind, anything outside `hooks/audio`, `hooks/epub`, `components/player`.

### html5-audio-read-along — `~/Documents/repos/linux-lit/html5-audio-read-along/`

Tiny (~11 KB JS total) read-along demo: word-level highlight synced to `<audio>` with click-to-seek.

- `read-along.js` (8.6 KB) — the entire algorithm: word spans with `data-begin`/`data-end`, audio `timeupdate` → highlight current word, click span → seek audio. Read this when designing click-to-seek or rewriting linux-lit's per-word highlight loop.
- `index.html` — example markup format (XML-ish word spans).

### transcript-tracer-js — `~/Documents/repos/linux-lit/transcript-tracer-js/`

Single-file (`transcript-tracer.js`, 20 KB) library for syncing audio/video with text using **WebVTT timestamps**.

- Reference for: WebVTT parsing as a sync data format (an alternative to linux-lit's per-line SQLite timestamps if linux-lit ever needs to import/export sync data), and the active-cue → highlight loop.
- See `examples/` for usage patterns.

### How to use these references

1. Identify the problem (pagination edge case, sync stall, bookmark schema, etc.).
2. Pick the reference from the bullets at the top of this section.
3. Read the named file end-to-end before grepping — these are small enough.
4. Translate the **algorithm or schema**, never the code. linux-lit is Rust + GTK4 + SQLite + MPV — not JS, not curses, not WebView.
5. If the reference disagrees with linux-lit's current approach, that's a design question — don't silently change linux-lit to match. Surface the tradeoff.

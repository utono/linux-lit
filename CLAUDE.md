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

## Headless Verification (agent self-check)

The standing rule is "do not run the app — the user runs `cargo run`." The
exception below lets an agent verify GUI changes **without touching the user's
live session**, by running the reader inside a throwaway headless compositor
(`cage`) on its own Wayland socket and screenshotting it with `grim`.

**Why this and not the live dwl:** on the user's dwl session a new reader window
opens on a non-visible tag, so `grim` (which captures the *active* output) can't
see it, and the user's seat is already owned — so `ydotool` and
`WLR_BACKENDS=headless,libinput` both fail with "seat busy". A nested headless
`cage` sidesteps all of that.

**Required tools** (already installed): `cage`, `grim`, `wtype`. Documented in
`~/utono/ccinstall/paclists/`.

### Launch the reader headless

```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
```

- `GSK_RENDERER=cairo` is **mandatory**: the default GTK renderer tries Vulkan,
  loses its surface on the headless backend, and the reader aborts with a Rust
  stack overflow. Cairo (software) renders cleanly.
- `WLR_RENDERER=pixman` keeps wlroots on software rendering too.
- Cage opens a fresh Wayland socket, normally `wayland-1` (it does **not** honor
  a `WAYLAND_DISPLAY` you pass in for its own server socket — check
  `ls /run/user/1000/wayland-*` for the new one).

### Capture and drive

Wait for the socket, then give the window ~3s to map and gain focus before
sending keys (premature `wtype` is dropped — this caused early false negatives):

```bash
export WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000
grim /tmp/shot.png                 # screenshot the reader
wtype "3"                          # send keystrokes to the focused reader
```

- `wtype` works (virtual-keyboard protocol, no seat needed) **once the window is
  focused**; `ydotool`/libinput do not (seat owned by dwl).
- Then `Read` the PNG to inspect the result.

### Useful key sequences for verification

- `3` / `2` — next / previous chapter (jumps the cursor onto the `CHAPTER N`
  heading). Front matter (before Chapter 1) has chapter number 0 and no
  synopsis, so `h` shows nothing there — advance into a chapter first.
- `h` — open the synopsis overlay for the current chapter; `Ctrl+g` glosses.
- `j` / `k` — scroll. While an overlay is open these scroll the overlay; with no
  overlay they scroll the reading buffer. To stress overlay top/bottom clipping,
  open the overlay then `j` repeatedly to reach the last line.
- `Escape` — close the overlay.

### Clean up

```bash
pkill -f "cage -- ./target/debug/linux-lit"; pkill -f target/debug/linux-lit
```

(`ydotoold` is not needed for this flow; only `cage` + `wtype` + `grim`.)

For the **automated** equivalent of this manual self-check, see *Automated UI
tests* below — it wraps the same cage + grim + wtype flow in `cargo test` and
adds a fail-closed line-clipping assertion.

## Automated UI tests (cargo)

`tests/harness/mod.rs` + `tests/smoke.rs` + `tests/line_clipping.rs` are a
headless UI test harness: each test runs the app inside its **own isolated
`cage`** (a temp `XDG_RUNTIME_DIR`, never the live session), screenshots with
`grim`, drives input with `wtype`, and asserts the main reading card never clips
its first/last line.

```bash
# everything (provides the a11y bus + software GL the artifacts want):
./scripts/e2e-env.sh cargo test -- --ignored --nocapture

# just the clipping invariant:
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

Tests are `#[ignore]`d so a bare `cargo test` stays green without cage/grim/wtype.
Deps (pacman/AUR): `cage`, `grim`, `wtype`, `python-pillow`, `python-numpy`,
`at-spi2-core`, `dbus` (the AT-SPI bits are only needed by `annotate_ui.py`'s
best-effort overlay; the clipping detector itself is pure-pixel).

Design notes (so you don't re-derive them):

- **cage, not bare dwl/sway.** linux-lit only lays out + paints once it gets a
  configured, focused, fullscreen surface. cage gives the single client exactly
  that; bare dwl/sway on the headless backend leave the window unsized so the
  reveal hits its 5s "load may be stuck" fallback and renders blank.
- **`GSK_RENDERER=cairo` is mandatory** (set by the harness): the default
  Vulkan/ngl renderer loses its surface on the headless backend and the app
  aborts with a stack overflow.
- **MPV is skipped in tests.** The harness sets `LIT_HEADLESS_TEST=1`;
  `launch_mpv` then does not spawn MPV at all — otherwise its window covers the
  reader in the test compositor and the process leaks across runs.
- **Region via the app, not AT-SPI.** linux-lit's `sourceview5::View` exposes no
  AT-SPI Text interface, so the clipping detector can't auto-find the pane. On
  reveal (under `LIT_HEADLESS_TEST`) the app logs `TEST_VIEWPORT_RECT x y w h`
  (window == screenshot coords); the harness reads it and passes `--region`.
- **Keys** are RPD: top `gg` (two presses), page `x`/`y`, end `shift+G`, line
  `j`/`k`. They land on the window's global capture-phase controller — no
  Tab-focus step.
- Scope: the tests cover the **main reading card**. The synopsis/gloss overlay
  has its own scroll/clip path and would need an `h`-open step + its own region.

## UI review protocol

After any e2e run, screenshots land in `target/ui/` (auto-cleaned at the start
of each run, so the directory only holds the current run's captures). **Open
every PNG — and any `_clip.png` overlay — and report what you see inline** in
your reply: quote the on-screen text and call out any clipping or layout problem
by eye. A passing exit code is not enough; clipping/layout bugs are caught by
looking. No written review file is required (there is no longer a `Stop` hook
gating this).

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
- `src/mpv/discovery.rs` — derive_socket_path, find_socket_for_work, launch_mpv (skips MPV under `LIT_HEADLESS_TEST`)
- `tests/harness/mod.rs` — headless cage harness: screenshot/input/clipping helpers
- `tests/line_clipping.rs` — the core no-clip invariant (top/mid/end)
- `scripts/check_line_clipping.py` — fail-closed pixel line-clipping detector (`--region`)
- `scripts/e2e-env.sh` — headless WLR/GTK env + dbus + AT-SPI registry wrapper
- `src/input/scroll.rs::emit_test_viewport_rect` — logs `TEST_VIEWPORT_RECT` for the harness
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

## Pagination & Scene Boundaries

**Scene/section boundaries are authoritative metadata, not inferred from text.**
A boundary is exactly where a line's `(div1, div2)` changes (act, scene). At load,
`build_line_map` precomputes `LineMap.section_starts: Vec<bool>` (the FIRST buffer
line of each `(div1,div2)` run); all pagination reads it via
`AppState::is_section_start` / the `section_break_fn` closure threaded into the
pure helpers in `viewport.rs` (`clamp_at_section_break`, `back_up_for_speaker`,
the right-column "begins a new scene" check, `scene_header_top`, `scene_snap_top`).

**Do NOT re-infer a boundary from buffer text** (`line_types::is_act_scene_marker`
/ `is_separator`) in any pagination path. Those text classifiers are for BUILDING
the bitmap, for *display* (title bar, synopsis), and as a mid-load fallback only.
Re-inferring structure that the DB already encodes is what caused the long
`y GAP` / wrong-spread bug class; the fix was to read `(div1,div2)`. General rule:
if `lit.db` already encodes a per-line fact (boundary, chapter, dialogue,
spoken-status), surface it through `LineMap`/`Line` and read it — never
reconstruct it by classifying buffer text. See
`docs/troubleshooting/page-turning-mechanics.md` → "The authoritative-boundary
principle" and the snapshot version (`snapshot.rs SNAPSHOT_VERSION`) which must be
bumped when `LineMap`'s serialized shape changes.

**The rule applies to test assertions too, not just pagination.** The nav-fuzz
UNBALANCED-SPREAD check in `nav_test.rs` exempted scene-clamped spreads by
classifying buffer text (`is_act_scene_marker`/`is_separator`), so it flagged a
short right column at any boundary whose new scene opens on a stage direction +
speaker with no `ACT`/`SCENE` chrome line. **2H6, Cor, and Ham were all the same
false-positive class** — real `(div1,div2)` boundaries (e.g. 2H6 4.7→4.8) that
production's `clamp_at_section_break` clamps correctly but the text-classifying
exemption missed. The fix was to make the test read the authoritative
`section_starts` bitmap via `s.is_section_start` (the same source production
clamps on). Lesson: when a per-work nav-fuzz FAIL is an `UNBALANCED`/short-column
at a scene edge, first ask whether the *assertion* (not production) is
re-inferring the boundary from text.

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

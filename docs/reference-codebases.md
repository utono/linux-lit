# Reference Codebases

Read-only checkouts at `~/Documents/repos/linux-lit/` for debugging or designing
features that overlap with other ebook readers. They are reference material, not
dependencies — **never import code, only patterns**. Re-clone with
`git clone <url>` into that directory if missing.

Pick the reference by problem area, not by language:

- **Pagination / clipping / page-turn math** → `foliate-js/`
- **Audio-text sync (the closest analog to linux-lit's MPV workflow)** → `lue/` first, then `html5-audio-read-along/`, then `transcript-tracer-js/`
- **Vim-style EPUB reading in Rust** → `bk/`
- **Whisper-driven word timestamps & per-document audio storage model** → `openreader/`
- **Annotations / highlights / location addressing / selection-tools UX** → `foliate/`

## foliate — `~/Documents/repos/linux-lit/foliate/` + `foliate-js/`

GNOME ebook reader, JavaScript/GJS + WebKitGTK + libadwaita (~8-10k LOC shell + ~9-11k LOC vendored renderer). Different rendering stack (CSS multi-column inside a WebView), but solves many of the same problems linux-lit faces.

- **Pagination edge cases** (clipped descenders, last-fully-visible-line, partial bottom lines, scroll-vs-page mode) — `foliate-js/paginator.js` (~44 KB). Different engine, transferable algorithm.
- **Location addressing** (portable bookmarks, sub-line precision, cross-device sync) — `foliate-js/epubcfi.js` (~13 KB) is the standard EPUB CFI implementation. Reference design if linux-lit ever needs more than `line_mapping.id`.
- **Annotations / highlights data model** — `foliate/src/annotations.js` (~25 KB): bookmark + named-color highlight + note schema, CFI-anchored, with export.
- **Selection-tools pattern** (Wiktionary, Wikipedia, translate as isolated modules with a uniform interface) — `foliate/src/selection-tools.js` and `foliate/src/selection-tools/*.html`.
- **EPUB Media Overlays / SMIL audio sync** — `foliate-js` SMIL modules. Reference only if importing timestamps from EPUB3 audiobooks.
- **Theme JSON schema** — `foliate/src/themes.js` and the user-themes-as-JSON pattern.
- **Not useful for:** library management (per-book JSON, no SQLite), library picker UI (WebView-based), vim navigation, MPV-driven sync, settings overlay (GSettings).

Quick map: app entry `foliate/src/main.js`, `app.js`. Reader: `foliate/src/reader/reader.html` + `reader.js`. Largest file: `foliate/src/book-viewer.js` (~47 KB).

## lue — `~/Documents/repos/linux-lit/lue/`

Terminal ebook reader (Python, ~1.5k LOC) with **word-level TTS sync** — the closest in-language analog to linux-lit's audio/text sync workflow. Modular by responsibility, easy to read in one sitting.

- `lue/audio.py` — playback control (mirrors what linux-lit/mpv-linux-lit does)
- `lue/tts_manager.py` — TTS engine integration; reference for sync state machine
- `lue/timing_calculator.py` — **highest-value file**: how to map text positions to audio time and back. Read this when debugging linux-lit's deferred page-turn or stall-on-seek issues.
- `lue/content_parser.py` — EPUB/PDF/DOCX/HTML/RTF/TXT/MD ingestion. Reference if linux-lit ever ingests anything beyond `lit.db`.
- `lue/progress_manager.py` — bookmark/last-position persistence. Compare to linux-lit's `page_history` and bookmark schema.
- `lue/input_handler.py` — keybind dispatch in a TUI. Different from GTK but the dispatch shape is similar.

## bk — `~/Documents/repos/linux-lit/bk/`

Terminal EPUB reader in Rust (~1163 LOC across 3 files). Closest Rust-language analog. Tiny enough to read end-to-end.

- `src/main.rs` (426 lines) — argv handling, key event loop, vim-style keymap dispatch. Compare to `src/input/keymap.rs`.
- `src/view.rs` (444 lines) — viewport/scroll/page state. Compare to `src/input/navigation.rs` and the display logic in `src/app/mod.rs`.
- `src/epub.rs` (~9.8 KB) — EPUB unzip + chapter splitting. Reference if linux-lit ever ingests EPUB.

## openreader — `~/Documents/repos/linux-lit/openreader/`

Next.js/TypeScript web app (~30k LOC) with **whisper.cpp word timestamps** and per-document audio. Most of it is unrelated to linux-lit (auth, S3 uploads, Drizzle ORM), but the audio-sync pieces are the most direct reference for linux-lit's manual-timestamp + sync workflow.

- `src/hooks/audio/` — audio playback hooks, time-update handling, seek behavior. Read when debugging playback sync stalls.
- `src/components/player/` — the read-along UI: word/line highlight driven by audio time. Compare to linux-lit's cursor advancement under MPV sync.
- `src/hooks/epub/` and `src/hooks/html/` — content-to-timestamp mapping, chunked. Useful pattern even though linux-lit's chunks come from `lit.db`, not whisper.
- **Skip:** auth, billing, S3, Drizzle, Tailwind, anything outside `hooks/audio`, `hooks/epub`, `components/player`.

## html5-audio-read-along — `~/Documents/repos/linux-lit/html5-audio-read-along/`

Tiny (~11 KB JS total) read-along demo: word-level highlight synced to `<audio>` with click-to-seek.

- `read-along.js` (8.6 KB) — the entire algorithm: word spans with `data-begin`/`data-end`, audio `timeupdate` → highlight current word, click span → seek audio. Read this when designing click-to-seek or rewriting linux-lit's per-word highlight loop.
- `index.html` — example markup format (XML-ish word spans).

## transcript-tracer-js — `~/Documents/repos/linux-lit/transcript-tracer-js/`

Single-file (`transcript-tracer.js`, 20 KB) library for syncing audio/video with text using **WebVTT timestamps**.

- Reference for: WebVTT parsing as a sync data format (an alternative to linux-lit's per-line SQLite timestamps if linux-lit ever needs to import/export sync data), and the active-cue → highlight loop.
- See `examples/` for usage patterns.

## How to use these references

1. Identify the problem (pagination edge case, sync stall, bookmark schema, etc.).
2. Pick the reference from the bullets at the top.
3. Read the named file end-to-end before grepping — these are small enough.
4. Translate the **algorithm or schema**, never the code. linux-lit is Rust + GTK4 + SQLite + MPV — not JS, not curses, not WebView.
5. If the reference disagrees with linux-lit's current approach, that's a design question — don't silently change linux-lit to match. Surface the tradeoff.

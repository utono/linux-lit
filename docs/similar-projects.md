# Similar Projects: Auto-Scroll Synced Playback & Word-Level Highlighting

Projects on GitHub featuring audio-synced auto-scroll and/or word-level highlighting during playback, sorted by stars within each category.

## Document/Book Readers

- **[audiobookshelf](https://github.com/advplyr/audiobookshelf)** (12,310 stars, JS) — Self-hosted audiobook/podcast server with ebook reading and synced highlighting
- **[foliate](https://github.com/johnfactotum/foliate)** (8,117 stars, JS/GJS) — GTK ebook reader with TTS and highlighting (GNOME)
- **[ttu-ebook-reader](https://github.com/ttu-ttu/ebook-reader)** (969 stars, Svelte) — Online ebook reader with Yomichan support
- **[lue](https://github.com/superstarryeyes/lue)** (713 stars, Python) — Terminal ebook reader with word-level TTS sync, supports EPUB/PDF/DOCX/HTML/RTF/TXT/MD
- **[openreader](https://github.com/richardr1126/openreader)** (301 stars, TypeScript) — Self-hosted read-along reader with whisper.cpp word timestamps, EPUB/PDF/DOCX
- **[ttu-whispersync](https://github.com/Renji-XD/ttu-whispersync)** (78 stars, Svelte) — Audiobook listening synced with ttu ebook-reader

## Browser Extensions

- **[read-aloud](https://github.com/ken107/read-aloud)** (1,643 stars, JS) — Browser extension that reads webpages aloud with highlighting

## Libraries / Building Blocks

- **[RealtimeTTS](https://github.com/KoljaB/RealtimeTTS)** (3,835 stars, Python) — Real-time text-to-speech library with low latency streaming
- **[html5-audio-read-along](https://github.com/westonruter/html5-audio-read-along)** (193 stars, JS) — Word-level highlight synced to audio, click-to-seek
- **[react-speech-highlight](https://github.com/albirrkarim/react-speech-highlight-demo)** (187 stars, JS) — React/Vanilla lib for TTS with word/sentence highlighting
- **[transcript-tracer-js](https://github.com/samuelbradshaw/transcript-tracer-js)** (22 stars, JS) — Sync audio/video with text using WebVTT timestamps

## Rust E-Readers / Text Viewers

- **[bk](https://github.com/aeosynth/bk)** (330 stars) — Terminal EPUB reader with vim bindings, incremental search, single binary
- **[peep](https://github.com/ryochack/peep)** (169 stars) — CLI text viewer like less that works in a small terminal pane
- **[pidif](https://github.com/bjesus/pidif)** (30 stars) — Lightweight PDF reader using GTK4 + Rust, built for touch devices
- **[rust-pager](https://github.com/Riey/rust-pager)** (25 stars) — Pager in Rust with vim-like keybindings and search
- **[Crust](https://github.com/orhnk/Crust)** (10 stars) — Lightweight fast text editor in Rust + GTK4
- **[MView6](https://github.com/newinnovations/MView6)** (9 stars) — High-performance PDF/ebook viewer with single/dual-page layout, Rust + GTK4
- **[repy](https://github.com/newptcai/repy)** (6 stars) — Terminal EPUB reader (Rust port of epy) with TUI navigation, bookmarks, SQLite backend

## Rust Libraries

- **[termimad](https://github.com/Canop/termimad)** (1,156 stars) — Library for rendering Markdown in terminal with wrapping/scrolling
- **[epub-rs](https://github.com/danigm/epub-rs)** (129 stars) — Rust library for reading EPUB files

## Notes

- **Most relevant to linux-lit's audio-sync approach:** openreader (whisper-based word timestamps), ttu-whispersync (audiobook sync with ebook display), and foliate (GTK-based reader with TTS)
- **Closest Rust analog:** bk (terminal EPUB reader with vim bindings)
- Star counts as of 2026-04-02

## Recently Added linux-lit Features With E-Reader Analogues

Features added to linux-lit since the original survey, mapped to comparable functionality in mainstream e-readers and the projects above. Each entry notes the linux-lit commit area and where similar UX exists elsewhere.

### Reading & Navigation

- **Bookmarks with star glyph in gutter, picker, and cycle keybinds** (`64f0fba`, `bb6e379`, `ca547c0`) — Standard in Kindle, Kobo, Apple Books, foliate, and audiobookshelf. linux-lit's gutter glyph echoes Kobo's left-margin bookmark indicator; the bookmark picker with relative timestamps mirrors foliate's annotations panel.
- **Page history for exact page-backward navigation** (`6a0af48`, `a507608`) — Comparable to Kindle's "Back" button and foliate's location history, which restore the prior viewport rather than recomputing it.
- **Virtual page numbering and act.scene.line citations** (`1edec9a`, `5f6c475`, `43b239f`) — Page numbers for reflowable text are standard in Kindle ("Location"/"Page") and Kobo. Act.scene.line references for plays match the citation overlay in foliate's drama-aware EPUBs and the Folger Shakespeare reader.
- **Speaker-aware page turns and dialogue navigation (j/k)** (`915bece`, `94b333b`, `ebebd84`) — Per-speaker stepping is uncommon in mass-market readers but appears in dramatic-text tools; closest analogues are scripture readers (verse-by-verse step) and karaoke/lyrics apps.
- **Chapter navigation with `[`/`{` and chapter glyph in gutter** (`3f3610a`, `af5d1b1`) — Every major reader has chapter jump (Kindle "Go To", Kobo TOC, foliate sidebar). The gutter glyph for chapter starts is similar to Marginalia/Pretext-style printed chapter rules.

### Pagination & Layout

- **Adaptive card layout with symmetric margins and three-card spacers** (`dd68614`, `b929a14`, `ac428bc`) — Kindle Paperwhite, Kobo, and Apple Books all use a centered text card with adjustable margins. The split-card spacer approach resembles Readwise Reader's "page card" treatment.
- **Crossfade / slide / instant page transitions via libadwaita animations** (`3256490`, `9ff0a86`, `c4d27a0`) — Kindle ("Page Refresh"), Apple Books (curl/slide/none), and Kobo (none/slide) expose the same set of transition styles as a user setting.
- **Eliminate clipped descenders by capping page line count and measuring real line height** (`b01d021`, `f172ea8`, `c85f66a`) — All paginating e-readers face this; foliate and Calibre Viewer both reflow to avoid orphaned half-lines, which is the same problem this solves.
- **Responsive library picker with header, breadcrumb, and footer hints** (`1fb61b8`, `b110b93`, `fe5a4c7`) — Mirrors foliate's library grid and Kobo's home screen, both of which adapt to window size and surface contextual hints.

### Audio & Sync

- **Playback sync toggle with status icon** (`e7f4689`) — Voice Dream, Speechify, audiobookshelf, and Apple Books' read-along all expose a sync on/off toggle with a persistent indicator.
- **Manual timestamp setting (u/p/P), undo (U), and `source='manual'` tracking** (`f8e1436`, `08fcfeb`, `426ff85`) — Manual word/line alignment is the editing workflow used in Kindle Whispersync authoring tools and in audiobookshelf's chapter editor; consumer readers don't expose it.
- **Sync survives relative audio seeks; deferred page-turn during playback** (`e5ee3c5`, `9de7cdb`, `2250c5b`) — Apple Books and Speechify both keep highlight synced through scrubbing; deferred page turn (waiting until end of current line before flipping) matches Speechify's "auto-scroll on sentence end" behavior.
- **Pause MPV when toggling translations / close translations on chapter jump** (`966c3bd`) — Kindle X-Ray and Apple Books' built-in dictionary likewise pause TTS when an overlay opens.

### Vocabulary & Annotations

- **Vocab popup with paragraph-aligned positioning, auto-hide crossfade, and theme tinting** (`f326e59`, `d4014d0`, `0bda95c`) — Closest to Kindle's word lookup card, Kobo's dictionary popup, and ttu-ebook-reader's Yomichan integration. The paragraph-anchored popup is unusual; Yomichan and Apple Books anchor to the word itself.
- **Concordance picker with cross-work cycling and word copy/collect** (`db79e5a`, `1755583`) — Kindle's Vocabulary Builder and Readwise both surface per-word context across a library; cross-work cycling resembles ttu-ebook-reader's word-frequency view.
- **Word cycling + clipboard copy (`w`, `\`, `#`)** (`6fb723c`, `e8eebb3`, `ff6cb63`) — Kindle's "Copy" sheet and Apple Books' selection menu cover the copy step; rapid word cycling for clipboard capture is closer to language-learning tools (Migaku, LingQ) than to general readers.

### Input & Theming

- **8BitDo Micro gamepad support with overlay and keybind swap** (`93d9891`) — Kindle Oasis page-turn buttons, Kobo Libra/Sage hardware buttons, and BOOX devices with bluetooth remotes all expose a similar "remote page-turn" surface; gamepad-as-remote is a niche pattern.
- **Theme-aware gutter and cursor highlight derived from `root_color`** (`796afe9`, `b46f0f7`, `d4014d0`) — Kindle, Kobo, and foliate all retint UI chrome (cursor, selection, sign column) to match the active theme/sepia mode.
- **Settings overlay with transition style picker and Esc-to-revert** (`af9a344`, `0f58ff3`) — Standard pattern: Kindle "Aa" menu, Kobo Reading Settings, Apple Books page setup. Reverting on Esc rather than committing is closer to foliate's modal preference dialogs.

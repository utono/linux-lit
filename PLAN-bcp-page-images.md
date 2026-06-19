# PLAN: BCP1549 page-image display synced to playback

## Goal

When reading **BCP1549** in the main card, let the user toggle the card from the
rendered `.txt` to the **page-scan PNG** for the current position. As MPV
playback advances the cursor (existing sync), the displayed page image follows
along — the image tracks the cursor, and because playback drives the cursor,
the image tracks playback.

Page scans: `~/utono/literature/BCP/cummings-brian/1549/IMG_1181.PNG` …
`IMG_1316.PNG` (136 leaves). BCP1549 has **2404** canonical lines
(`line_mapping`, keyed `work_abbrev,div1,div2,line_in_div`).

## Decisions (from the user)

- **Display model:** *toggle* text ⇆ image (a keybind swaps the card content).
- **Page selection:** *image follows the cursor* (playback moves the cursor).
- **Granularity:** *page-level ranges* — per PNG, the first/last canonical line
  on that page (~136 rows), NOT a per-line path.
- **Storage:** *new lit.db table*, surfaced through `Line`/`LineMap` like other
  per-line facts.
- **Mapping production:** *manual-assisted calibration mode* in the reader (step
  pages, mark each PNG's start line; app records ranges).
- **Image fit:** *fit to card* (scaled to card width, aspect preserved, scroll
  if taller) — mirror how the synopsis overlay sizes to the card.
- **Audio:** TTS generated later (ElevenLabs, existing gloss/echo path), then
  timestamped. NOT a blocker for the image feature (see Dependencies).

## Dependencies & current state (verified)

- BCP1549 work row: `work_type='bible_book'`, `text_file=.../TEI/bcp-1549.txt`.
  Renders today via the BCP sentence-split path (`app.rs:3486`
  prepare/`build_line_map_bcp`, `apply_bcp_formatting` `app.rs:3826`).
  See [[project_bcp_db_load_path]] / [[project_bcp_renders_from_text_file]].
- **No media file** associated and **0 line timestamps** for BCP1549, so there
  is currently nothing to "sync to." The feature is built to follow the
  **cursor**; playback sync then works for free once timestamps exist, because
  sync moves the same cursor (`main.rs:362` `MpvEvent::TimePos` →
  `update_highlight_and_center`). **The image/calibration work does NOT depend
  on the audio** and can land first.
- TEI has **no `<pb/>` page breaks** and there is **no index file**; only a prose
  note ("page images n1185–n1200 … not transcribed"). So the line→page map must
  be *produced*, not extracted — hence calibration.

## Components

### 1. lit.db schema — `page_images` table (litdb repo)

Page-range granularity:

```
CREATE TABLE page_images (
    id INTEGER PRIMARY KEY,
    work_abbrev TEXT NOT NULL,
    image_path TEXT NOT NULL,        -- relative to the work's image dir
    page_order INTEGER NOT NULL,     -- 1..N display order (IMG_1181=1, …)
    start_line_id INTEGER REFERENCES line_mapping(id),  -- first canonical line on this leaf
    end_line_id   INTEGER REFERENCES line_mapping(id),  -- last (inclusive); derived
    UNIQUE(work_abbrev, page_order)
);
```

- Work's image dir: add a `works.image_dir` column (robust/queryable) vs
  deriving from `text_file`'s parent. Lean: explicit column. Decide in step 1.
- `end_line_id` derived = the line just before page N+1's start. Calibration
  captures each page's **start line** only; ranges close up on save.
- Author a forward litdb migration + helpers `load_page_images(conn, abbrev)`
  and `save_page_image_start(conn, abbrev, page_order, start_line_id)`.

### 2. linux-lit: load + expose ranges

- `db/queries.rs::load_page_images(abbrev) -> Vec<PageImage{path,order,start_line_id,end_line_id}>`.
- Resolve `start/end_line_id` → buffer-line range via `LineMap.work_to_buffer`
  so lookup is "current buffer line → page".
- `AppState`: `page_images: Vec<PageImage>` (loaded in `display_work` for BCP
  works with rows) and `image_mode: bool`.
- **Snapshot:** load `page_images` OUTSIDE the snapshot (cheap query) to avoid
  coupling; if any of it ends up cached in `LineMap`, bump
  `snapshot::SNAPSHOT_VERSION`.

### 3. linux-lit: image widget in the card (toggle)

- `gtk4::Picture` (aspect-preserving scaling) as an `add_overlay` panel on the
  outer overlay, hidden by default — per [[feedback_picker_overlay_not_chain]]
  do NOT splice into the size-bearing card chain.
- Fit-to-card: size to the authoritative card width (reuse the
  `overlay_card_size`/`apply_card_sizing` width logic, NOT allocated width).
  `set_can_shrink(true)` + contain; wrap in `ScrolledWindow` if scaled height
  exceeds the card.
- `Action::ToggleImageView`, gated to BCP works with `page_images`. Pick a free
  RPD key — CHECK `~/utono/rpd` + `keymap_config.rs`. Update BOTH
  `keymap_config.rs` and the stow `keymap.json`, AND the Ctrl+/ overlay
  (`update-cairo-keybinds-overlay` skill) per CLAUDE.md.

### 4. linux-lit: cursor → page → image swap

- `page_image_for_buffer_line(buf_line) -> Option<&PageImage>`: the page whose
  `[start,end]` buffer range contains the cursor line.
- While `image_mode` is on, on each cursor move set the Picture to that page's
  PNG; reload only when the page changes (track `current_page_order`).
- Hook the SAME place the highlight updates: after `update_highlight_and_center`
  and after the playback-sync advance in `main.rs:362` `TimePos`. One hook makes
  image-follows-cursor and image-follows-playback fall out together.

### 5. linux-lit: calibration mode (produces the data)

- `InputMode::PageCalibration` + `Action::EnterPageCalibration`.
- UX: enter → show page 1 (IMG_1181) as the image with a hint bar. User moves
  the **text cursor** to the first line on the shown page, presses "mark start"
  → records `(page_order, start_line_id=current line id)`, advances to next page
  image. Repeat through 136 pages.
- On each mark: upsert `page_images`; recompute `end_line_id` as predecessor of
  the next start. Write live to lit.db (like `u`/`.` timestamp binds) and reload
  `AppState.page_images`.
- Edge cases: continuation page with no new line → "same as previous"; re-mark
  to correct; first page defaults to the work's first canonical line; final page
  (IMG_1316, no successor) needs an explicit end or "to last line."

### 6. (Deferred) audio + timestamps

- Generate TTS (ElevenLabs `eleven_v3`, [[project_elevenlabs_voice_ids]] /
  existing gloss-TTS path), associate media (`work_media_associations`), set
  per-line timestamps (the `u` bind or a batch importer). Once present, playback
  moves the cursor and the image follows with NO extra image code. Independent
  track; can run in parallel.

## Build order (recommended)

1. **litdb:** `page_images` table + migration + load/save queries.
2. **linux-lit:** load `page_images`, add the `Picture` overlay + `image_mode`
   toggle (fit-to-card). Verify it shows page 1 on toggle (hardcode page 1 until
   calibration exists).
3. **linux-lit:** calibration mode to populate ranges; wire cursor→page→image
   swap (step 4).
4. Manual-verify in `cargo run` (BCP work, toggle image, j/k changes page at the
   right boundaries) — visual acceptance → headless e2e / ask the user per
   CLAUDE.md.
5. **(later)** TTS + timestamps; confirm playback drives the page image.

## Open questions for step 1

- `works.image_dir` column vs derive-from-`text_file`-parent. (Lean: column.)
- Generalize `page_images` for other works (schema already work-agnostic).
- Calibration "mark start" key; explicit end for the final page.

## Verification

- litdb: migration applies; `load_page_images('BCP1549')` rows contiguous and
  cover all 2404 lines after calibration.
- linux-lit: `cargo test --bins` for pure helpers
  (`page_image_for_buffer_line`, range close-up). Visual/sync → `e2e-env.sh` or
  user-run `cargo run` (do not self-run).
- Keybind change → keymap.json + Ctrl+/ overlay + tests (CLAUDE.md).

## NOTE

The unrelated completed plan for the ElevenLabs voice picker lives in `PLAN.md`;
this new plan is kept separate to avoid clobbering it.

# Read-aloud explications in the gloss overlay (ElevenLabs TTS)

**Date:** 2026-06-08
**Status:** Design approved, ready for implementation plan

## Goal

Let the reader hear the **explication paragraphs** of a gloss read aloud. An
explication paragraph is a `<gloss>` prose paragraph (the teacher's commentary,
3–6 sentences) — *not* the quoted `<speaker>`/`<verse>` source lines, and *not*
an echo-bracket `["quote" — Source]` gloss.

Audio is produced by the **ElevenLabs** text-to-speech API, cached permanently
on disk, and associated with the paragraph in `lit.db` so re-reading never
re-hits the API.

A **paragraph cursor** is added to the gloss overlay so the reader can pick
which explication paragraph is read. Pressing **Space** reads the paragraph the
cursor is on.

## Non-goals (YAGNI)

- No in-app voice picker (voice is a config field).
- No streaming playback (fetch whole MP3, then play).
- No re-synthesis when the configured voice changes (delete the dir to force it).
- No TTS for verse, speaker names, synopsis, or echo glosses — explication
  paragraphs only.
- No audio under the headless test harness (no audio device in `cage`).

## User-facing behavior

1. Open a gloss overlay (`h` then `Ctrl+g`, or `Ctrl+g` directly on a passage).
2. A moving **left accent bar** marks the current explication paragraph — the
   `<gloss>` prose paragraph nearest the viewport vertical center. `j`/`k`/`gg`/
   `G` scroll as today; the cursor follows the scroll automatically.
3. Press **Space**:
   - If audio is currently playing, **stop** it (Space is a toggle).
   - Otherwise, read the cursor's explication paragraph aloud.
     - Cache hit → play the stored MP3 immediately.
     - Cache miss → footer shows "Synthesizing…", the API is called
       asynchronously, the MP3 is written to disk + recorded in `lit.db`, then
       played. On error a toast appears (e.g. "Set ELEVENLABS_API_KEY").
4. Closing the overlay (`Escape`/`n`) stops any playing audio.

## Data flow (Space pressed)

1. Resolve the cursor's explication paragraph → `(gloss_id, paragraph_index)`.
2. If `tts.is_playing()` → `tts.stop()` and return (toggle off).
3. `find_gloss_audio(conn, gloss_id, paragraph_index)`:
   - **Hit** (path on disk) → `tts.play_file(path)`. No async, no API.
   - **Miss** → spawn async: `elevenlabs::synthesize(text, voice, model)` →
     `create_dir_all` → write MP3 → `save_gloss_audio(...)` → back on the GTK
     thread, `tts.play_file(path)`.

## Component design

### 1. Paragraph cursor (computed, not stored)

The cursor is the explication paragraph whose buffer-line range is nearest the
viewport vertical center. It is *recomputed on every scroll*, not held as
navigation state — matching the existing "cursor auto-follows scroll" model and
adding almost no new state.

**Buffer build** (`src/ui/gloss_overlay.rs`, `populate_gloss_buffer_ex`):
extend it to collect, for each explication paragraph (a `GlossElement::Gloss`
whose text is **not** an echo-bracket — i.e. `split_echo` returns `None`), a
record:

```rust
struct ParaRange {
    paragraph_index: i32,   // 0-based among explication paragraphs in this gloss
    start_line: i32,        // buffer line
    end_line: i32,          // buffer line
}
```

These are stored on `GlossOverlay` as
`explication_paras: Rc<RefCell<Vec<ParaRange>>>`, populated by
`show_gloss_with_color` and **cleared** in `show_echoes`/`show_synopsis`/
`show_glossing` (TTS is gloss-only).

**Cursor resolution** — new method on `GlossOverlay`:

```rust
pub fn current_explication_para(&self) -> Option<i32>
```

Reads the vadjustment center (`value + page_size/2.0`) in content space, maps it
to a buffer line via the existing `display_rows`/`iter_location` machinery, and
returns the `paragraph_index` whose `[start_line, end_line]` range contains or is
nearest that line. Returns `None` if there are no explication paragraphs.

**Visual treatment** — reuse the existing left accent bar (`bar_ranges` +
`bar_drawing` draw_func), the same mechanism that already marks a selected echo.
On every scroll (inside `scroll_gloss`, and the vadjustment `value_changed`
handler that already calls `queue_draw`), recompute the cursor paragraph and set
`bar_ranges` to its `[start_line, end_line]` span. No new drawing code.

### 2. Storage

**New `lit.db` table** (additive migration via `CREATE TABLE IF NOT EXISTS`,
matching `ensure_bookmarks_table` in `src/db/queries.rs`):

```sql
CREATE TABLE IF NOT EXISTS gloss_audio (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    gloss_id        INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
    paragraph_index INTEGER NOT NULL,
    audio_path      TEXT NOT NULL,
    voice_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(gloss_id, paragraph_index)
);
CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);
```

- Added by a new `ensure_gloss_audio_table(conn)` called from the same place
  other `ensure_*` tables are ensured at DB open.
- `voice_id`/`model_id` recorded so a cache hit is honest: a stored row from an
  earlier voice still plays its original audio. The `audio_path` is the source
  of truth; re-synth on voice change is out of scope.
- `ON DELETE CASCADE` removes audio rows when a gloss row is deleted.

**Filesystem layout:**

```
~/Music/glosses/<work-abbrev>/<gloss-id>/<paragraph-index>.mp3
```

e.g. `~/Music/glosses/Ham/4823/0.mp3`. `<work-abbrev>` comes from
`gloss_context.work_abbrev` (already `-Amb`-normalized). The directory is created
with `std::fs::create_dir_all` before writing.

**New queries** (`src/db/queries.rs`):

```rust
pub fn find_gloss_audio(conn: &Connection, gloss_id: i64, paragraph_index: i32)
    -> Result<Option<String>, rusqlite::Error>;          // returns audio_path

pub fn save_gloss_audio(conn: &Connection, gloss_id: i64, paragraph_index: i32,
    audio_path: &str, voice_id: &str, model_id: &str)
    -> Result<(), rusqlite::Error>;                       // INSERT OR REPLACE
```

### 3. ElevenLabs client (`src/elevenlabs.rs`)

Mirrors `src/claude.rs`:

```rust
pub enum ElevenLabsError { MissingApiKey, Timeout, RateLimited, ApiError(String) }
impl std::fmt::Display for ElevenLabsError { /* MissingApiKey => "Set ELEVENLABS_API_KEY environment variable", ... */ }

/// POST text, return raw MP3 bytes.
pub async fn synthesize(text: &str, voice_id: &str, model_id: &str)
    -> Result<Vec<u8>, ElevenLabsError>;
```

- API key from `ELEVENLABS_API_KEY` (env, like `ANTHROPIC_API_KEY`).
- `POST https://api.elevenlabs.io/v1/text-to-speech/{voice_id}`, header
  `xi-api-key`, header `Accept: audio/mpeg`, body `{ "text", "model_id" }`.
- Returns `response.bytes()` (the MP3), not JSON.
- `reqwest::Client` with a 60s timeout, `rustls-tls` (already enabled in
  `Cargo.toml`). 429 → `RateLimited`, non-2xx → `ApiError(HTTP {status}: {body})`,
  `is_timeout()` → `Timeout`.

### 4. Audio playback (`src/tts.rs`, rodio)

```rust
pub struct TtsPlayer { /* OutputStream + OutputStreamHandle + Sink */ }
impl TtsPlayer {
    pub fn new() -> Self;                  // no-op stub under LIT_HEADLESS_TEST
    pub fn play_file(&self, path: &Path);  // stop current, decode + play
    pub fn stop(&self);
    pub fn is_playing(&self) -> bool;      // !sink.empty()
}
```

- `rodio` added to `Cargo.toml` (pulls `cpal` + an MP3 decoder).
- One `TtsPlayer` lives on `AppState`, created at startup. The `OutputStream`
  **must be kept alive** for the app's lifetime (rodio stops audio if the stream
  is dropped) — held as an `AppState` field.
- Under `LIT_HEADLESS_TEST`, `TtsPlayer::new` returns a stub whose methods do
  nothing (no audio device in `cage`) — same gating pattern MPV uses in
  `src/mpv/discovery.rs`.

### 5. Config (`src/config.rs`)

Add two fields with `#[serde(default = ...)]` so existing `config.json` files
keep loading:

```rust
pub elevenlabs_voice_id: String,   // default: "21m00Tcm4TlvDq8ikWAM" (Rachel, ElevenLabs' stock default voice)
pub elevenlabs_model_id: String,   // default: "eleven_turbo_v2_5"
```

### 6. Wiring

**Keybind** (`src/input/keymap.rs`, `handle_gloss_key`):

- Add a `"space"` arm that calls
  `crate::input::actions::gloss::read_current_paragraph(state)`.
- Guard against the stacked add/edit input card: when the ask card is open and
  focus is on the input (`ask_open && ask_focus == AskFocus::Ask`), let Space
  fall through so it types a literal space. Otherwise Space triggers read-aloud.

**Action handler** (`src/input/actions/gloss.rs`), new
`read_current_paragraph(state_rc: &Rc<RefCell<AppState>>)`:

1. `if state.tts.is_playing() { state.tts.stop(); return; }` (toggle off).
2. `let para_index = state.gloss_overlay.current_explication_para()?;`
3. `gloss_id` from `gloss_list[gloss_index]`; `work_abbrev` from
   `gloss_context`. Extract the paragraph's **plain text** (the matching
   `GlossElement::Gloss` content) for synthesis.
4. `find_gloss_audio(...)`:
   - **Hit** → `state.tts.play_file(path)`.
   - **Miss** → footer hint "Synthesizing…"; `glib::spawn_future_local` +
     `tokio_handle.spawn` (the pattern `add_gloss` uses): `synthesize` →
     `create_dir_all` → write MP3 → `save_gloss_audio` → on the GTK thread
     `play_file`. On `Err`, show a toast via `chapter_toast` (the existing
     mechanism in `show_no_concordance_toast`).

**Cleanup:**

- `delete_current_gloss` (`src/input/actions/gloss.rs`) — after deleting the
  gloss row, best-effort `std::fs::remove_dir_all` of
  `~/Music/glosses/<abbrev>/<gloss-id>/` so audio doesn't outlive its gloss.
- Overlay close (`handle_gloss_key` Escape/`n` arm and `GlossOverlay::hide`) —
  call `tts.stop()` so narration doesn't bleed into the reader.

**Footer hint** — append `· Space read aloud` to the gloss hint string set in
`show_gloss_with_color`.

**Ctrl+/ overlay** — per project rule, add Space to the gloss-overlay section of
`src/ui/keybinds_overlay.rs` (keycap + `describe()` arm) using the
`update-cairo-keybinds-overlay` skill.

## Testing

Pure-logic units only (no audio device, no live API, no GTK measurement under
`cargo test --bins`):

- `elevenlabs.rs`: request-body construction and error mapping.
- Paragraph-range extraction in `populate_gloss_buffer_ex`: given a gloss XML
  string, assert the correct `(paragraph_index, line range)` records come out,
  with echo-bracket glosses excluded.
- `find_gloss_audio`/`save_gloss_audio` against a temp SQLite (matching existing
  `queries.rs` test style), including the `UNIQUE(gloss_id, paragraph_index)`
  upsert and cascade-on-delete.
- `current_explication_para` center-resolution if factorable into a pure helper
  that takes row geometry + paragraph ranges (the GTK-coupled wrapper stays
  untested).

**Not unit-tested** (require an audio device, the live API, or a mapped surface;
verified manually by the user, since they cannot run under the headless
harness):

- Actual ElevenLabs synthesis round-trip.
- rodio playback / Space toggle.
- The moving accent bar in a rendered, scrolled gloss card.

The user runs these manually; runtime verification of read-aloud is explicitly
theirs (per the project's "do not run the app" rule and the headless-test
limitations for audio/visual criteria).

## Files touched

- `Cargo.toml` — add `rodio`.
- `src/config.rs` — `elevenlabs_voice_id`, `elevenlabs_model_id`.
- `src/elevenlabs.rs` — **new**, TTS API client.
- `src/tts.rs` — **new**, rodio playback (headless stub).
- `src/main.rs` — module declarations.
- `src/app.rs` — `TtsPlayer` + `OutputStream` fields on `AppState`, init.
- `src/db/queries.rs` — `ensure_gloss_audio_table`, `find_gloss_audio`,
  `save_gloss_audio`; call `ensure_gloss_audio_table` at DB open.
- `src/ui/gloss_overlay.rs` — `explication_paras`, `ParaRange`,
  `current_explication_para`, accent-bar-follows-cursor, footer hint, clear in
  echo/synopsis/glossing modes, `stop()` on hide.
- `src/input/keymap.rs` — `"space"` arm in `handle_gloss_key`; `tts.stop()` on
  overlay close.
- `src/input/actions/gloss.rs` — `read_current_paragraph`; audio-dir cleanup in
  `delete_current_gloss`.
- `src/ui/keybinds_overlay.rs` — Space in the gloss-overlay section.

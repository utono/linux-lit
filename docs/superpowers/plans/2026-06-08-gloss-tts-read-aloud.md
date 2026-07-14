# Gloss-Overlay TTS Read-Aloud Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read a gloss's explication paragraphs aloud via ElevenLabs TTS, with a paragraph cursor in the gloss overlay and a permanent per-paragraph MP3 cache associated in `lit.db`.

**Architecture:** A computed paragraph cursor (the `<gloss>` prose paragraph nearest the viewport center, marked by the existing left accent bar that follows scroll) is read aloud on Space. Audio is fetched from ElevenLabs (async, `reqwest`, mirroring `src/claude.rs`), cached at `~/Music/glosses/<abbrev>/<gloss-id>/<para-index>.mp3` and recorded in a new `gloss_audio` table, then played in-process via `rodio`. Cache hits skip the API.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite (bundled SQLite), reqwest (rustls-tls), tokio, glib, rodio (new).

---

## Reference facts (verified against the codebase)

- API-client pattern: `src/claude.rs` — `reqwest::Client` 60s timeout, env key, `ClaudeError` enum with `Display`.
- Async bridge pattern: `src/input/actions/gloss.rs::add_gloss` — `glib::spawn_future_local(async { tokio_handle.spawn(async { ... }).await })`, then mutate `AppState` on the GTK thread.
- Toast: `AppState.chapter_toast` (a `gtk4::Label`); set text + `set_visible(true)`, hide after 3s via `glib::timeout_add_local_once` (see `show_no_concordance_toast` in `src/input/actions/concordance.rs`).
- DB migration pattern: `ensure_bookmarks_table` in `src/db/queries.rs`, called from a `std::sync::Once` block in `src/app.rs:2410` (alongside `ensure_echo_tables`).
- `SavedGloss.gloss_id: i64` is the `glosses.id`.
- Gloss buffer build: `populate_gloss_buffer_ex` in `src/ui/gloss_overlay.rs` walks `parse_gloss_tags` → `GlossElement::{Speaker,Verse,Gloss}`. An explication paragraph is a `Gloss` whose text is NOT an echo bracket (`split_echo(text).is_none()`).
- Accent bar: `GlossOverlay.bar_ranges: Rc<RefCell<Vec<BarRange>>>`, drawn by `bar_drawing`; the scrolled-window `vadjustment().connect_value_changed` already calls `bar_for_scroll.queue_draw()`.
- Config: fields use `#[serde(default = "fn")]`; default fns live at the bottom of `src/config.rs`.
- Module decls live in `src/main.rs` (`mod claude;`, `mod gloss;`, etc.).
- Build only — do NOT run the app. `cargo build` / `cargo test --bins` for verification. Runtime read-aloud is verified manually by the user (audio device + live API + mapped surface are unavailable to the agent / headless harness).

---

## Task 1: Add `gloss_audio` table + queries

**Files:**
- Modify: `src/db/queries.rs`
- Test: `src/db/queries.rs` (inline `#[cfg(test)]` module, matching existing query tests)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `src/db/queries.rs` (create the module if none exists for queries; otherwise append):

```rust
#[test]
fn gloss_audio_roundtrip_and_upsert() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    // gloss_audio references glosses(id); create a minimal glosses table for the FK.
    conn.execute_batch(
        "CREATE TABLE glosses (id INTEGER PRIMARY KEY);
         INSERT INTO glosses (id) VALUES (4823);",
    )
    .unwrap();
    ensure_gloss_audio_table(&conn).unwrap();

    // Miss before insert.
    assert_eq!(find_gloss_audio(&conn, 4823, 0).unwrap(), None);

    // Insert, then hit.
    save_gloss_audio(&conn, 4823, 0, "/tmp/a/0.mp3", "voiceA", "modelA").unwrap();
    assert_eq!(
        find_gloss_audio(&conn, 4823, 0).unwrap(),
        Some("/tmp/a/0.mp3".to_string())
    );

    // Upsert: same (gloss_id, paragraph_index) replaces the path.
    save_gloss_audio(&conn, 4823, 0, "/tmp/a/0b.mp3", "voiceB", "modelB").unwrap();
    assert_eq!(
        find_gloss_audio(&conn, 4823, 0).unwrap(),
        Some("/tmp/a/0b.mp3".to_string())
    );

    // Distinct paragraph_index is a separate row.
    save_gloss_audio(&conn, 4823, 1, "/tmp/a/1.mp3", "voiceA", "modelA").unwrap();
    assert_eq!(
        find_gloss_audio(&conn, 4823, 1).unwrap(),
        Some("/tmp/a/1.mp3".to_string())
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins gloss_audio_roundtrip_and_upsert`
Expected: FAIL — `cannot find function ensure_gloss_audio_table` (and the other two).

- [ ] **Step 3: Write minimal implementation**

Add to `src/db/queries.rs` (near `ensure_bookmarks_table`):

```rust
/// Ensure the gloss_audio table exists (per-explication-paragraph TTS cache).
pub fn ensure_gloss_audio_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS gloss_audio (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            gloss_id        INTEGER NOT NULL REFERENCES glosses(id) ON DELETE CASCADE,
            paragraph_index INTEGER NOT NULL,
            audio_path      TEXT NOT NULL,
            voice_id        TEXT NOT NULL,
            model_id        TEXT NOT NULL,
            timestamp       DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(gloss_id, paragraph_index)
        );
        CREATE INDEX IF NOT EXISTS idx_gloss_audio_gloss_id ON gloss_audio(gloss_id);",
    )?;
    Ok(())
}

/// Return the cached audio path for a gloss paragraph, if any.
pub fn find_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    paragraph_index: i32,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT audio_path FROM gloss_audio WHERE gloss_id = ?1 AND paragraph_index = ?2",
        rusqlite::params![gloss_id, paragraph_index],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Insert or replace the audio path for a gloss paragraph.
pub fn save_gloss_audio(
    conn: &Connection,
    gloss_id: i64,
    paragraph_index: i32,
    audio_path: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO gloss_audio (gloss_id, paragraph_index, audio_path, voice_id, model_id)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(gloss_id, paragraph_index)
         DO UPDATE SET audio_path = excluded.audio_path,
                       voice_id   = excluded.voice_id,
                       model_id   = excluded.model_id,
                       timestamp  = CURRENT_TIMESTAMP",
        rusqlite::params![gloss_id, paragraph_index, audio_path, voice_id, model_id],
    )?;
    Ok(())
}
```

If `OptionalExtension` is not already imported in `queries.rs`, add `use rusqlite::OptionalExtension;` (it provides `.optional()`). Check the top of the file first; only add if missing.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins gloss_audio_roundtrip_and_upsert`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): gloss_audio table + find/save queries for TTS cache"
```

---

## Task 2: Call the migration at startup

**Files:**
- Modify: `src/app.rs:2410-2415` (the `BOOKMARKS_INIT` `Once` block)

- [ ] **Step 1: Add the ensure call**

In `src/app.rs`, the `Once` block currently reads:

```rust
    static BOOKMARKS_INIT: std::sync::Once = std::sync::Once::new();
    BOOKMARKS_INIT.call_once(|| {
        if let Ok(conn) = crate::db::queries::open_db_rw() {
            let _ = crate::db::queries::ensure_bookmarks_table(&conn);
            let _ = crate::db::queries::ensure_echo_tables(&conn);
        }
    });
```

Add one line inside the `if let`:

```rust
            let _ = crate::db::queries::ensure_gloss_audio_table(&conn);
```

so all three `ensure_*` calls run together.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(db): ensure gloss_audio table at startup"
```

---

## Task 3: ElevenLabs TTS client

**Files:**
- Create: `src/elevenlabs.rs`
- Modify: `src/main.rs` (add `mod elevenlabs;`)
- Test: inline `#[cfg(test)]` in `src/elevenlabs.rs`

- [ ] **Step 1: Write the failing test**

Create `src/elevenlabs.rs` with ONLY a test first (so it fails to compile against missing items):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_messages() {
        assert_eq!(
            ElevenLabsError::MissingApiKey.to_string(),
            "Set ELEVENLABS_API_KEY environment variable"
        );
        assert_eq!(
            ElevenLabsError::ApiError("boom".into()).to_string(),
            "TTS API error: boom"
        );
    }

    #[test]
    fn request_url_uses_voice_id() {
        assert_eq!(
            tts_url("21m00Tcm4TlvDq8ikWAM"),
            "https://api.elevenlabs.io/v1/text-to-speech/21m00Tcm4TlvDq8ikWAM"
        );
    }

    #[test]
    fn request_body_has_text_and_model() {
        let body = build_body("hello", "eleven_turbo_v2_5");
        assert_eq!(body["text"], "hello");
        assert_eq!(body["model_id"], "eleven_turbo_v2_5");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins --features "" error_display_messages 2>&1 | head` (it will fail to compile — `mod elevenlabs` not declared, items missing).
Expected: FAIL — unresolved module / items.

- [ ] **Step 3: Write minimal implementation**

Prepend to `src/elevenlabs.rs` (above the test module):

```rust
use std::fmt;

#[derive(Debug)]
pub enum ElevenLabsError {
    MissingApiKey,
    Timeout,
    RateLimited,
    ApiError(String),
}

impl fmt::Display for ElevenLabsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevenLabsError::MissingApiKey => {
                write!(f, "Set ELEVENLABS_API_KEY environment variable")
            }
            ElevenLabsError::Timeout => write!(f, "TTS request timed out"),
            ElevenLabsError::RateLimited => write!(f, "TTS rate limited — try again"),
            ElevenLabsError::ApiError(msg) => write!(f, "TTS API error: {}", msg),
        }
    }
}

fn tts_url(voice_id: &str) -> String {
    format!("https://api.elevenlabs.io/v1/text-to-speech/{}", voice_id)
}

fn build_body(text: &str, model_id: &str) -> serde_json::Value {
    serde_json::json!({ "text": text, "model_id": model_id })
}

/// Synthesize `text` to MP3 bytes via ElevenLabs. Key from ELEVENLABS_API_KEY.
pub async fn synthesize(
    text: &str,
    voice_id: &str,
    model_id: &str,
) -> Result<Vec<u8>, ElevenLabsError> {
    let api_key =
        std::env::var("ELEVENLABS_API_KEY").map_err(|_| ElevenLabsError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;

    let response = client
        .post(tts_url(voice_id))
        .header("xi-api-key", &api_key)
        .header("accept", "audio/mpeg")
        .header("content-type", "application/json")
        .json(&build_body(text, model_id))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ElevenLabsError::Timeout
            } else {
                ElevenLabsError::ApiError(e.to_string())
            }
        })?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ElevenLabsError::RateLimited);
    }
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(ElevenLabsError::ApiError(format!("HTTP {}: {}", status, text)));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ElevenLabsError::ApiError(e.to_string()))?;
    Ok(bytes.to_vec())
}
```

Add to `src/main.rs` after `mod claude;`:

```rust
mod elevenlabs;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins error_display_messages request_url_uses_voice_id request_body_has_text_and_model`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/elevenlabs.rs src/main.rs
git commit -m "feat(tts): ElevenLabs synthesize client (mirrors claude.rs)"
```

---

## Task 4: Add `rodio` and the `TtsPlayer`

**Files:**
- Modify: `Cargo.toml`
- Create: `src/tts.rs`
- Modify: `src/main.rs` (add `mod tts;`)

No unit test: `TtsPlayer` needs an audio device (absent in CI/headless). Verify by compiling; behavior is verified manually.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml` under `[dependencies]`, after the `reqwest` line, add:

```toml
rodio = { version = "0.19", default-features = false, features = ["symphonia-mp3"] }
```

- [ ] **Step 2: Write the player**

Create `src/tts.rs`:

```rust
use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// In-process audio player for gloss TTS clips. Holds the rodio output stream
/// alive for the app's lifetime (dropping it stops all audio). A no-op stub
/// under LIT_HEADLESS_TEST, where there is no audio device.
pub struct TtsPlayer {
    inner: Option<Inner>,
}

struct Inner {
    // The stream must be kept alive; rodio drops audio if it is dropped.
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
    sink: RefCell<Option<rodio::Sink>>,
}

impl TtsPlayer {
    pub fn new() -> Self {
        if std::env::var("LIT_HEADLESS_TEST").is_ok() {
            return TtsPlayer { inner: None };
        }
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => TtsPlayer {
                inner: Some(Inner {
                    _stream: stream,
                    handle,
                    sink: RefCell::new(None),
                }),
            },
            Err(e) => {
                crate::logging::log(&format!("TTS: no audio output device: {}", e));
                TtsPlayer { inner: None }
            }
        }
    }

    /// Stop any current clip and play the MP3 at `path`.
    pub fn play_file(&self, path: &Path) {
        let inner = match &self.inner {
            Some(i) => i,
            None => return,
        };
        self.stop();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                crate::logging::log(&format!("TTS: open {} failed: {}", path.display(), e));
                return;
            }
        };
        let decoder = match rodio::Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(e) => {
                crate::logging::log(&format!("TTS: decode failed: {}", e));
                return;
            }
        };
        match rodio::Sink::try_new(&inner.handle) {
            Ok(sink) => {
                sink.append(decoder);
                *inner.sink.borrow_mut() = Some(sink);
            }
            Err(e) => crate::logging::log(&format!("TTS: sink failed: {}", e)),
        }
    }

    pub fn stop(&self) {
        if let Some(inner) = &self.inner {
            if let Some(sink) = inner.sink.borrow_mut().take() {
                sink.stop();
            }
        }
    }

    /// True while a clip is still playing.
    pub fn is_playing(&self) -> bool {
        match &self.inner {
            Some(inner) => inner
                .sink
                .borrow()
                .as_ref()
                .map(|s| !s.empty())
                .unwrap_or(false),
            None => false,
        }
    }
}

impl Default for TtsPlayer {
    fn default() -> Self {
        Self::new()
    }
}
```

Add to `src/main.rs` after `mod tts;`'s alphabetical neighbor (place near `mod theme;`):

```rust
mod tts;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean (rodio downloaded on first build).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/tts.rs src/main.rs
git commit -m "feat(tts): rodio TtsPlayer (in-process playback, headless stub)"
```

---

## Task 5: Config fields for voice + model

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add the struct fields**

In `src/config.rs`, inside `pub struct Config`, after the `claude_model` field, add:

```rust
    #[serde(default = "default_elevenlabs_voice_id")]
    pub elevenlabs_voice_id: String,
    #[serde(default = "default_elevenlabs_model_id")]
    pub elevenlabs_model_id: String,
```

- [ ] **Step 2: Add the default fns**

After `default_claude_model` near the bottom of `src/config.rs`:

```rust
fn default_elevenlabs_voice_id() -> String {
    // Rachel — ElevenLabs' stock default voice.
    "21m00Tcm4TlvDq8ikWAM".to_string()
}

fn default_elevenlabs_model_id() -> String {
    "eleven_turbo_v2_5".to_string()
}
```

If `Config` has a hand-written `impl Default` (not derived) or a `Default`-constructing helper that lists every field, add the two fields there too, using the same default fns. Check for `impl Default for Config` first; if it uses `..Default::default()` or `#[derive(Default)]` no change is needed.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): elevenlabs_voice_id + elevenlabs_model_id"
```

---

## Task 6: Explication paragraph ranges from the gloss buffer

**Files:**
- Modify: `src/ui/gloss_overlay.rs`
- Test: inline `#[cfg(test)]` in `src/ui/gloss_overlay.rs`

This extracts, from a gloss XML string, the explication paragraphs (non-echo `<gloss>` elements) in order. We factor the pure logic out of the GTK-coupled `populate_gloss_buffer_ex` so it is unit-testable.

- [ ] **Step 1: Write the failing test**

Add to a `#[cfg(test)]` module in `src/ui/gloss_overlay.rs`:

```rust
#[cfg(test)]
mod explication_tests {
    use super::*;

    #[test]
    fn extracts_only_non_echo_gloss_paragraphs() {
        let gloss = "<speaker>HAMLET</speaker>\n\
                     <verse>To be, or not to be</verse>\n\
                     <gloss>This is the teacher's first explication.</gloss>\n\
                     <gloss>[\"a quote\" — Macbeth 1.1]</gloss>\n\
                     <gloss>Second explication paragraph here.</gloss>";
        let paras = explication_paragraphs(gloss);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].0, 0); // paragraph_index
        assert_eq!(paras[0].1, "This is the teacher's first explication.");
        assert_eq!(paras[1].0, 1);
        assert_eq!(paras[1].1, "Second explication paragraph here.");
    }

    #[test]
    fn no_explications_when_all_echoes() {
        let gloss = "<speaker>HAMLET</speaker>\n\
                     <verse>To be</verse>\n\
                     <gloss>[\"q\" — Lr 1.1]</gloss>";
        assert!(explication_paragraphs(gloss).is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins extracts_only_non_echo_gloss_paragraphs`
Expected: FAIL — `cannot find function explication_paragraphs`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/ui/gloss_overlay.rs` (near `parse_gloss_tags`):

```rust
/// The explication paragraphs of a gloss, in order: `(paragraph_index, text)`
/// for each `<gloss>` element that is NOT an echo bracket. These are the
/// read-aloud targets. Echo glosses (`["quote" — Source]`) are excluded.
pub fn explication_paragraphs(gloss: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut idx = 0i32;
    for el in parse_gloss_tags(gloss) {
        if let GlossElement::Gloss(text) = el {
            if split_echo(&text).is_none() {
                out.push((idx, text.trim().to_string()));
                idx += 1;
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins explication_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): explication_paragraphs extractor (non-echo gloss prose)"
```

---

## Task 7: Track paragraph buffer-line ranges + the cursor

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

No new unit test (GTK-coupled). Verify by compiling. `current_explication_para` is exercised manually.

- [ ] **Step 1: Add the `ParaRange` struct and the field**

In `src/ui/gloss_overlay.rs`, near the top with `BarRange`/`LineNumber`:

```rust
/// Buffer-line span of one explication paragraph, for the read-aloud cursor.
struct ParaRange {
    paragraph_index: i32,
    start_line: i32,
    end_line: i32,
}
```

Add a field to `pub struct GlossOverlay`:

```rust
    /// Explication paragraphs of the currently shown gloss, with their buffer
    /// line spans. Drives the read-aloud cursor (the paragraph nearest the
    /// viewport center). Empty in echo/synopsis/glossing modes.
    explication_paras: Rc<RefCell<Vec<ParaRange>>>,
```

Initialize it in `GlossOverlay::new`:

```rust
        let explication_paras: Rc<RefCell<Vec<ParaRange>>> = Rc::new(RefCell::new(Vec::new()));
```

and add `explication_paras,` to the `GlossOverlay { ... }` constructor literal.

- [ ] **Step 2: Populate ranges in `show_gloss_with_color`; clear elsewhere**

In `show_gloss_with_color`, after `populate_gloss_buffer` is called and `self.gloss_view` holds the text, compute ranges by walking the explication paragraphs and locating each one's text in the buffer. Add this helper to `impl GlossOverlay`:

```rust
    /// Recompute `explication_paras` line spans from the current buffer + gloss
    /// text. Each explication paragraph is a single inserted buffer line range;
    /// we find it by scanning buffer lines for the paragraph's first text.
    fn rebuild_explication_ranges(&self, gloss: &str) {
        let paras = explication_paragraphs(gloss);
        let buffer = self.gloss_view.buffer();
        let line_count = buffer.line_count();
        let mut ranges: Vec<ParaRange> = Vec::new();
        let mut search_from = 0i32;
        for (pidx, text) in paras {
            // Match on the paragraph's first non-empty trimmed line.
            let needle = text.lines().next().unwrap_or("").trim();
            if needle.is_empty() {
                continue;
            }
            let mut found: Option<i32> = None;
            for line in search_from..line_count {
                if let Some(start) = buffer.iter_at_line(line) {
                    let mut end = start.clone();
                    if !end.ends_line() {
                        end.forward_to_line_end();
                    }
                    let line_text = buffer.text(&start, &end, false);
                    if line_text.as_str().trim().starts_with(needle) {
                        found = Some(line);
                        break;
                    }
                }
            }
            if let Some(start_line) = found {
                // A paragraph occupies one logical buffer line (it may wrap
                // visually). end_line == start_line for the cursor's purposes.
                ranges.push(ParaRange {
                    paragraph_index: pidx,
                    start_line,
                    end_line: start_line,
                });
                search_from = start_line + 1;
            }
        }
        *self.explication_paras.borrow_mut() = ranges;
    }
```

Call it at the end of `show_gloss_with_color` (after `populate_gloss_buffer`, before `reset_scroll_top`):

```rust
        self.rebuild_explication_ranges(gloss);
```

Clear it in the other show modes — add `self.explication_paras.borrow_mut().clear();` near the top of `show_echoes`, `show_synopsis`, `show_glossing`, and `show_loading_message` (alongside their existing `synopsis_label_ranges` clears).

- [ ] **Step 3: Add the cursor resolver**

Add to `impl GlossOverlay`:

```rust
    /// The explication paragraph nearest the viewport vertical center, by
    /// `paragraph_index`. None when the current card has no explication
    /// paragraphs (echoes/synopsis/empty gloss).
    pub fn current_explication_para(&self) -> Option<i32> {
        let ranges = self.explication_paras.borrow();
        if ranges.is_empty() {
            return None;
        }
        let adj = self.gloss_scrolled.vadjustment();
        let center_y = adj.value() + adj.page_size() / 2.0;
        let buffer = self.gloss_view.buffer();
        // Map each paragraph's start line to a content-space y, pick the nearest.
        let mut best: Option<(i32, f64)> = None;
        for r in ranges.iter() {
            if let Some(iter) = buffer.iter_at_line(r.start_line) {
                let (y, h) = self.gloss_view.line_yrange(&iter);
                let mid = (y + self.gloss_view.top_margin()) as f64 + h as f64 / 2.0;
                let dist = (mid - center_y).abs();
                if best.map(|(_, d)| dist < d).unwrap_or(true) {
                    best = Some((r.paragraph_index, dist));
                }
            }
        }
        best.map(|(idx, _)| idx)
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 5: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): paragraph cursor — track line ranges, resolve center paragraph"
```

---

## Task 8: Accent bar follows the cursor on scroll

**Files:**
- Modify: `src/ui/gloss_overlay.rs`

No unit test (visual). Verify by compiling; appearance verified manually.

- [ ] **Step 1: Add a method to set the bar to the cursor paragraph**

Add to `impl GlossOverlay`:

```rust
    /// Move the left accent bar to the current cursor explication paragraph and
    /// repaint. No-op when there are no explication paragraphs.
    fn mark_cursor_paragraph(&self) {
        let idx = match self.current_explication_para() {
            Some(i) => i,
            None => return,
        };
        let span = self
            .explication_paras
            .borrow()
            .iter()
            .find(|r| r.paragraph_index == idx)
            .map(|r| (r.start_line, r.end_line));
        if let Some((start_line, end_line)) = span {
            *self.bar_ranges.borrow_mut() = vec![BarRange { start_line, end_line }];
            self.bar_drawing.queue_draw();
        }
    }
```

- [ ] **Step 2: Call it on scroll**

In `scroll_gloss`, `scroll_gloss_to_top`, `scroll_gloss_to_bottom`, after the existing `self.bar_drawing.queue_draw();`, add:

```rust
        self.mark_cursor_paragraph();
```

Also call `self.mark_cursor_paragraph();` once at the end of `show_gloss_with_color` (after `rebuild_explication_ranges`) so the bar appears immediately on open.

Note: `show_gloss_with_color` currently sets `bar_ranges` from `populate_gloss_buffer` (the echo-selection mechanism). For a teacher-generic gloss those ranges are empty, so overwriting with the cursor paragraph is correct. (Echo selection uses `show_echoes`, a different path, which clears `explication_paras` and never calls `mark_cursor_paragraph`.)

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs
git commit -m "feat(gloss): accent bar follows read-aloud cursor on scroll"
```

---

## Task 9: `TtsPlayer` on `AppState`

**Files:**
- Modify: `src/app.rs`

No unit test. Verify by compiling.

- [ ] **Step 1: Add the field**

In `src/app.rs`, inside `pub struct AppState`, near `gloss_overlay`, add:

```rust
    pub tts: crate::tts::TtsPlayer,
```

- [ ] **Step 2: Construct it**

Where `AppState { ... }` is built (the literal near `gloss_overlay,`), add:

```rust
        tts: crate::tts::TtsPlayer::new(),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(tts): TtsPlayer on AppState"
```

---

## Task 10: `read_current_paragraph` action

**Files:**
- Modify: `src/input/actions/gloss.rs`

No unit test (audio device + network + GTK). Verify by compiling; behavior verified manually.

- [ ] **Step 1: Add the handler**

Add to `src/input/actions/gloss.rs`:

```rust
/// Space in the gloss overlay: read the cursor's explication paragraph aloud.
/// Toggles: if audio is playing, stop. Otherwise play the cached MP3, or
/// synthesize it via ElevenLabs (async), cache it, and play.
pub(crate) fn read_current_paragraph(state_rc: &Rc<RefCell<AppState>>) {
    {
        let s = state_rc.borrow();
        if s.tts.is_playing() {
            s.tts.stop();
            return;
        }
    }

    // Resolve cursor paragraph -> (gloss_id, paragraph_index, work_abbrev, text).
    let (gloss_id, para_index, work_abbrev, text, voice_id, model_id, tokio_handle) = {
        let s = state_rc.borrow();
        let para_index = match s.gloss_overlay.current_explication_para() {
            Some(i) => i,
            None => return,
        };
        let gloss = match s.gloss_list.get(s.gloss_index) {
            Some(g) => g,
            None => return,
        };
        let gloss_id = gloss.gloss_id;
        let work_abbrev = match &s.gloss_context {
            Some(ctx) => ctx.work_abbrev.clone(),
            None => return,
        };
        let paras = crate::ui::gloss_overlay::explication_paragraphs(&gloss.gloss_text);
        let text = match paras.iter().find(|(i, _)| *i == para_index) {
            Some((_, t)) => t.clone(),
            None => return,
        };
        (
            gloss_id,
            para_index,
            work_abbrev,
            text,
            s.config.elevenlabs_voice_id.clone(),
            s.config.elevenlabs_model_id.clone(),
            s.tokio_handle.clone(),
        )
    };

    // Cache hit?
    if let Ok(conn) = crate::db::queries::open_db() {
        if let Ok(Some(path)) = crate::db::queries::find_gloss_audio(&conn, gloss_id, para_index) {
            if std::path::Path::new(&path).exists() {
                state_rc.borrow().tts.play_file(std::path::Path::new(&path));
                return;
            }
        }
    }

    // Miss: synthesize asynchronously.
    show_tts_toast(state_rc, "Synthesizing\u{2026}");
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let voice = voice_id.clone();
        let model = model_id.clone();
        let synth_text = text.clone();
        let result = tokio_handle
            .spawn(async move { crate::elevenlabs::synthesize(&synth_text, &voice, &model).await })
            .await;

        match result {
            Ok(Ok(bytes)) => {
                let dir = gloss_audio_dir(&work_abbrev, gloss_id);
                let path = dir.join(format!("{}.mp3", para_index));
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    crate::logging::log(&format!("TTS: mkdir {} failed: {}", dir.display(), e));
                    show_tts_toast(&state_for_result, "Could not save audio");
                    return;
                }
                if let Err(e) = std::fs::write(&path, &bytes) {
                    crate::logging::log(&format!("TTS: write {} failed: {}", path.display(), e));
                    show_tts_toast(&state_for_result, "Could not save audio");
                    return;
                }
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    let _ = crate::db::queries::save_gloss_audio(
                        &conn,
                        gloss_id,
                        para_index,
                        &path.to_string_lossy(),
                        &voice_id,
                        &model_id,
                    );
                }
                state_for_result.borrow().tts.play_file(&path);
                crate::logging::log(&format!(
                    "TTS: synthesized gloss {} para {}",
                    gloss_id, para_index
                ));
            }
            Ok(Err(e)) => {
                crate::logging::log(&format!("TTS: synth error: {}", e));
                show_tts_toast(&state_for_result, &e.to_string());
            }
            Err(e) => {
                crate::logging::log(&format!("TTS: tokio join error: {}", e));
            }
        }
    });
}

/// `~/Music/glosses/<work-abbrev>/<gloss-id>/`
fn gloss_audio_dir(work_abbrev: &str, gloss_id: i64) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join("Music")
        .join("glosses")
        .join(work_abbrev)
        .join(gloss_id.to_string())
}

fn show_tts_toast(state_rc: &Rc<RefCell<AppState>>, msg: &str) {
    let s = state_rc.borrow();
    s.chapter_toast.set_text(msg);
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}
```

Make `gloss_audio_dir` `pub(crate)` if Task 11 references it from another module; here both uses are in this file, so file-private is fine — but the delete cleanup (Task 11) is in this same file, so keep it private.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): read_current_paragraph — cache lookup + async synth + play"
```

---

## Task 11: Delete audio dir when a gloss is deleted

**Files:**
- Modify: `src/input/actions/gloss.rs` (`delete_current_gloss`)

- [ ] **Step 1: Add cleanup**

In `delete_current_gloss`, after `delete_gloss(&conn, gloss_id)` succeeds and before/around the existing log line, add a best-effort directory removal. You need the `work_abbrev`; it is available from `s.gloss_context`. Insert:

```rust
        if let Some(ctx) = s.gloss_context.as_ref() {
            let dir = gloss_audio_dir(&ctx.work_abbrev, gloss_id);
            let _ = std::fs::remove_dir_all(&dir);
        }
```

Place it right after the `delete_gloss` call (the DB `ON DELETE CASCADE` removes the `gloss_audio` rows; this removes the files). `gloss_id` and `s` are already in scope there.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src/input/actions/gloss.rs
git commit -m "feat(tts): remove cached audio dir on gloss delete"
```

---

## Task 12: Space keybind + stop-on-close

**Files:**
- Modify: `src/input/keymap.rs` (`handle_gloss_key`)

- [ ] **Step 1: Add the Space arm (respecting the ask card)**

In `handle_gloss_key`, the ask-card block already returns early for Tab / Ctrl+Return / Escape and falls through (`return false`) when `ask_focus == AskFocus::Ask`. Space must type a literal space when the input holds focus, so add the read-aloud arm AFTER that block (it only runs when the ask card is closed or focus is on the gloss, not the input).

Add to the final `match key_name` in `handle_gloss_key`, before the `_ => true` catch-all:

```rust
        "space" => {
            crate::input::actions::gloss::read_current_paragraph(state);
            true
        }
```

(When the ask card is open with input focus, the earlier `if ask_focus == AskFocus::Ask { return false; }` already fired, so this arm is unreachable in that state — Space types normally.)

- [ ] **Step 2: Stop audio when the overlay closes**

In `handle_gloss_key`, the `"Escape" | "n"` arm hides the overlay. Add a stop at the top of that arm, before `s.gloss_overlay.hide();`:

```rust
            s.tts.stop();
```

Also add `tts.stop()` to the other overlay-close paths to be safe — in `src/input/actions/gloss.rs::toggle_overlay`, in the branch that hides the overlay (the `if input_mode == GlossOverlay` block), add before `s.gloss_overlay.hide();`:

```rust
        s.tts.stop();
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Run the full pure-logic suite**

Run: `cargo test --bins`
Expected: PASS (all existing tests plus the new Task 1 / Task 3 / Task 6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs src/input/actions/gloss.rs
git commit -m "feat(tts): Space reads cursor paragraph; stop audio on overlay close"
```

---

## Task 13: Footer hint + Ctrl+/ overlay

**Files:**
- Modify: `src/ui/gloss_overlay.rs` (footer hint string)
- Modify: `src/ui/keybinds_overlay.rs` (via the update-cairo-keybinds-overlay skill)

- [ ] **Step 1: Update the gloss footer hint**

In `show_gloss_with_color`, the hint is set to:

```
"Esc close · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage"
```

Change it to include Space:

```rust
        self.hint.set_text("Esc close · Space read aloud · a add · e edit · d delete · c copy id · Ctrl+n/p gloss · Alt+n/p passage");
```

- [ ] **Step 2: Update the Ctrl+/ overlay**

Invoke the `update-cairo-keybinds-overlay` skill to add the Space bind to the gloss-overlay section of `src/ui/keybinds_overlay.rs`: add/adjust the Space key's `KeyDef` so its action shows "read aloud (gloss overlay)", and add a `describe()` arm pointing to `read_current_paragraph — src/input/actions/gloss.rs`. The skill carries the mandatory three-pass cross-reference; follow it.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src/ui/gloss_overlay.rs src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): Space read-aloud in gloss footer + Ctrl+/ overlay"
```

---

## Task 14: Document packages + final verification

**Files:**
- Modify: `~/utono/ccinstall/paclists/*.csv` (if a new system package is needed — rodio uses ALSA on Linux)

- [ ] **Step 1: Ensure ALSA dev headers (rodio's cpal backend)**

rodio's default ALSA backend needs `alsa-lib` (runtime, usually present) and, to build, `alsa-lib` headers. On CachyOS these ship in the `alsa-lib` package (already a base dep). Confirm the build succeeded in Task 4; if it failed with `alsa-sys`/`pkg-config` errors, install:

```bash
paru -S --needed alsa-lib
```

and run `/update-paclist install alsa-lib` (category: `system-libraries.csv`). If the build already succeeded, skip — do not install redundantly.

- [ ] **Step 2: Full build + test**

Run:
```bash
cargo build
cargo test --bins
cargo clippy
```
Expected: build clean, tests pass, no new clippy errors.

- [ ] **Step 3: Hand off to the user for runtime verification**

The read-aloud path needs an audio device, the live ElevenLabs API, and a mapped surface — none available to the agent. Ask the user to:
1. `export ELEVENLABS_API_KEY=...`
2. `cargo run`, open a teacher-generic gloss (`h` → `Ctrl+g`, or `Ctrl+g` on a passage), scroll so an explication paragraph is centered (accent bar beside it), press **Space**.
3. Confirm: audio plays; the file appears at `~/Music/glosses/<abbrev>/<gloss-id>/<n>.mp3`; pressing Space again stops it; re-pressing on the same paragraph plays instantly (cache hit, no delay); `Escape` stops audio.

- [ ] **Step 4: Commit any paclist change**

```bash
git -C ~/utono/ccinstall add paclists/system-libraries.csv
git -C ~/utono/ccinstall commit -m "docs(paclist): alsa-lib for linux-lit rodio TTS" || true
```

(Only if Step 1 actually installed a package.)

---

## Self-review notes

- **Spec coverage:** paragraph cursor (Tasks 6–8), Space toggle (Task 12), per-paragraph cache + `lit.db` (Tasks 1–2, 10), `~/Music/glosses/<abbrev>/<gloss-id>/<n>.mp3` (Task 10 `gloss_audio_dir`), ElevenLabs client + env key (Task 3), config voice/model (Task 5), rodio + headless stub (Task 4, 9), cleanup on delete + close (Tasks 11–12), footer + Ctrl+/ overlay (Task 13), tests pure-logic only with manual runtime handoff (Task 14). All spec sections map to a task.
- **Type consistency:** `find_gloss_audio`/`save_gloss_audio` signatures identical in Task 1 and Task 10/11; `explication_paragraphs` returns `Vec<(i32, String)>` in Task 6 and is consumed the same way in Tasks 7 and 10; `current_explication_para() -> Option<i32>` consistent across Tasks 7, 8, 10; `gloss_audio_dir(&str, i64)` consistent in Tasks 10 and 11; `TtsPlayer::{new,play_file,stop,is_playing}` consistent across Tasks 4, 9, 10, 12.
- **No placeholders:** every code step shows the full code; commands and expected results are concrete.

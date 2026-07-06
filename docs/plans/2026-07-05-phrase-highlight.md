# Phrase Highlight During Narration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Karaoke-style highlight of the phrase currently being narrated by MPV, driven by `phrase_timestamps` char ranges — on by default for prose, off by default for plays/poetry, toggled by Alt+p.

**Architecture:** A new `src/input/phrase_highlight.rs` module holds the pure lookup logic (span-at-time with gap-hold, spoken-line resolution near the sync cursor) plus the runtime update fn called from the existing `MpvEvent::TimePos` handler in `src/main.rs`. Phrase spans are lazily queried per `(line_mapping_id, media_id)` and cached in `AppState`; a `phrase-highlight` TextTag (span `background`, not `paragraph_background`) paints the range. Spec: `docs/plans/2026-07-05-phrase-highlight-design.md`.

**Tech Stack:** Rust, GTK4 TextTag/TextIter, rusqlite against `~/utono/litdb/data/lit.db`, serde config.

## Global Constraints

- Verify with `cargo build` / `cargo test`; NEVER `cargo run` (user launches via `crll`).
- Check cargo's own exit code — never pipe through `tail`/`head` before checking (`cargo test | tail` masked red suites twice).
- Timing uses **raw** playback time — do NOT add `SYNC_PREROLL` anywhere in this feature.
- `~/.config/linux-lit/keymap.json` overrides compiled defaults — keybind changes must land in BOTH `keymap_config.rs` and the stow source `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.
- Any keybind change must also update the Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`): KeyDef extras AND a `describe()` arm (use the `update-cairo-keybinds-overlay` skill's three-pass check).
- Commit after each task; work on branch `feat/phrase-highlight` off `master`.

---

### Task 0: Branch

- [ ] **Step 1: Create the feature branch**

```bash
cd ~/utono/linux-lit && git checkout -b feat/phrase-highlight
```

---

### Task 1: `phrase_spans_for_line` query

**Files:**
- Modify: `src/db/queries.rs` (add below `phrase_crossing_time`, ~line 692)
- Test: same file, `mod tests` (mirror `phrase_crossing_time_picks_first_phrase_past_offset`, ~line 3267)

**Interfaces:**
- Produces: `pub struct PhraseSpan { pub start_time: f64, pub end_time: f64, pub start_char: usize, pub end_char: usize }` (derives `Debug, Clone, Copy, PartialEq`) and `pub fn phrase_spans_for_line(conn: &Connection, line_mapping_id: i64, media_id: i64) -> Vec<PhraseSpan>` (empty vec = no rows).

- [ ] **Step 1: Write the failing test** (in `src/db/queries.rs` tests module, next to the existing phrase test)

```rust
#[test]
fn phrase_spans_for_line_returns_ordered_spans() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE phrase_timestamps (
             id INTEGER PRIMARY KEY, line_mapping_id INTEGER, media_id INTEGER,
             start_time REAL, end_time REAL, start_char INTEGER, end_char INTEGER);
         INSERT INTO phrase_timestamps
             (line_mapping_id, media_id, start_time, end_time, start_char, end_char)
         VALUES (7, 3, 12.0, 13.5, 20, 40),
                (7, 3, 10.0, 11.8, 0, 20),
                (7, 3, 15.0, 17.0, 40, 60),
                (8, 3, 99.0, 99.5, 0, 10),
                (7, 4, 50.0, 51.0, 0, 20);",
    )
    .unwrap();
    let spans = phrase_spans_for_line(&conn, 7, 3);
    assert_eq!(spans.len(), 3);
    // Ordered by start_time regardless of insert order.
    assert_eq!(spans[0], PhraseSpan { start_time: 10.0, end_time: 11.8, start_char: 0, end_char: 20 });
    assert_eq!(spans[1].start_char, 20);
    assert_eq!(spans[2].end_char, 60);
    // No rows -> empty vec (valid negative result).
    assert!(phrase_spans_for_line(&conn, 999, 3).is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test phrase_spans_for_line_returns_ordered_spans 2>&1 | rg "error|FAILED|passed"; echo exit=$?`
Expected: compile error — `phrase_spans_for_line` not found.

- [ ] **Step 3: Write minimal implementation** (below `phrase_crossing_time`)

```rust
/// One phrase's audio window + char range within its line's canonical text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhraseSpan {
    pub start_time: f64,
    pub end_time: f64,
    pub start_char: usize,
    pub end_char: usize,
}

/// All phrase spans for one (line, media), ordered by start_time. Empty vec =
/// no phrase_timestamps rows for the pair — callers cache the negative result
/// so works without phrase data stay inert with no per-tick re-query.
pub fn phrase_spans_for_line(
    conn: &Connection,
    line_mapping_id: i64,
    media_id: i64,
) -> Vec<PhraseSpan> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT start_time, end_time, start_char, end_char FROM phrase_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2 ORDER BY start_time",
    ) else {
        return Vec::new();
    };
    let rows = stmt.query_map(rusqlite::params![line_mapping_id, media_id], |row| {
        Ok(PhraseSpan {
            start_time: row.get(0)?,
            end_time: row.get(1)?,
            start_char: row.get::<_, i64>(2)?.max(0) as usize,
            end_char: row.get::<_, i64>(3)?.max(0) as usize,
        })
    });
    match rows {
        Ok(r) => r.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test phrase_spans_for_line_returns_ordered_spans; echo exit=$?`
Expected: `test result: ok. 1 passed`, `exit=0`

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs && git commit -m "feat(sync): phrase_spans_for_line query for phrase highlight"
```

---

### Task 2: Pure lookup logic (`phrase_highlight.rs` module)

**Files:**
- Create: `src/input/phrase_highlight.rs`
- Modify: `src/input/mod.rs` (add `pub mod phrase_highlight;` alongside the other module decls)

**Interfaces:**
- Consumes: `crate::db::queries::PhraseSpan` (Task 1).
- Produces: `pub fn phrase_at_time(spans: &[PhraseSpan], pos: f64) -> Option<usize>`; `pub fn resolve_spoken_idx(ts_of: impl Fn(usize) -> Option<(f64, f64)>, len: usize, cursor_wi: usize, pos: f64) -> Option<usize>`; `pub struct PhraseCache { pub line_mapping_id: i64, pub media_id: i64, pub spans: Vec<PhraseSpan> }`.

- [ ] **Step 1: Create the module with failing tests**

```rust
//! Karaoke-style spoken-phrase highlight during MPV narration sync.
//!
//! Driven from the `MpvEvent::TimePos` handler at **raw** playback time (no
//! SYNC_PREROLL — the sync cursor leads the narration by the preroll, so the
//! spoken line is resolved independently near the cursor). Spans come from
//! `phrase_timestamps` via `queries::phrase_spans_for_line`, cached per
//! (line_mapping_id, media_id) in `AppState.phrase_cache`.

use crate::db::queries::PhraseSpan;

/// Cached phrase spans for the (line, media) currently being narrated. An
/// EMPTY `spans` vec is a valid negative result (work/paragraph without
/// phrase data) — kept so we don't re-query every TimePos tick.
pub struct PhraseCache {
    pub line_mapping_id: i64,
    pub media_id: i64,
    pub spans: Vec<PhraseSpan>,
}

/// How many work lines around the sync cursor to scan when resolving which
/// line is actually being spoken at raw time. The cursor leads by at most one
/// line (SYNC_PREROLL) in normal sync; 8 tolerates gap-jumps and stale cursors.
const SPOKEN_LINE_WALK: usize = 8;

/// Index of the phrase active at `pos`: the LAST span whose start_time <= pos.
/// Holds through inter-phrase gaps and past the final span's end (no flicker;
/// the next paragraph's spans take over once the spoken line advances).
/// None before the first span starts.
pub fn phrase_at_time(spans: &[PhraseSpan], pos: f64) -> Option<usize> {
    if spans.is_empty() || pos < spans[0].start_time {
        return None;
    }
    let n = spans.partition_point(|sp| sp.start_time <= pos);
    Some(n - 1)
}

/// Resolve which work line is being SPOKEN at raw time `pos`, scanning a
/// bounded window around the sync cursor's work line. Returns the last
/// timestamped line in the window whose start <= pos (timestamps are
/// monotonic, so the scan breaks at the first future line). None when the
/// narration is behind every timestamped line in the window.
pub fn resolve_spoken_idx(
    ts_of: impl Fn(usize) -> Option<(f64, f64)>,
    len: usize,
    cursor_wi: usize,
    pos: f64,
) -> Option<usize> {
    let lo = cursor_wi.saturating_sub(SPOKEN_LINE_WALK);
    let hi = (cursor_wi + SPOKEN_LINE_WALK + 1).min(len);
    let mut best = None;
    for i in lo..hi {
        if let Some((start, _end)) = ts_of(i) {
            if start <= pos {
                best = Some(i);
            } else {
                break;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start_time: f64, end_time: f64, start_char: usize, end_char: usize) -> PhraseSpan {
        PhraseSpan { start_time, end_time, start_char, end_char }
    }

    #[test]
    fn phrase_at_time_basic_gap_hold_and_edges() {
        let spans = vec![
            span(10.0, 11.8, 0, 20),
            span(12.0, 13.5, 20, 40),
            span(15.0, 17.0, 40, 60),
        ];
        assert_eq!(phrase_at_time(&spans, 9.9), None); // before first
        assert_eq!(phrase_at_time(&spans, 10.0), Some(0)); // exact start
        assert_eq!(phrase_at_time(&spans, 11.0), Some(0)); // inside
        assert_eq!(phrase_at_time(&spans, 11.9), Some(0)); // gap: hold prev
        assert_eq!(phrase_at_time(&spans, 14.0), Some(1)); // gap: hold prev
        assert_eq!(phrase_at_time(&spans, 16.0), Some(2));
        assert_eq!(phrase_at_time(&spans, 99.0), Some(2)); // past end: hold last
        assert_eq!(phrase_at_time(&[], 5.0), None); // empty
    }

    #[test]
    fn resolve_spoken_idx_walks_near_cursor() {
        // Lines 0..6; lines 2 and 5 untimestamped (e.g. chapter headings).
        let ts = [
            Some((0.0, 4.0)),
            Some((5.0, 9.0)),
            None,
            Some((10.0, 14.0)),
            Some((15.0, 19.0)),
            None,
            Some((20.0, 24.0)),
        ];
        let f = |i: usize| ts.get(i).copied().flatten();
        // Cursor leads (preroll): cursor on 4, narration still on 3.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 4, 12.0), Some(3));
        // Cursor in step: pos inside cursor line's window.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 3, 12.0), Some(3));
        // Cursor lags: narration moved ahead.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 3, 21.0), Some(6));
        // Inter-line gap: pos between line 3 end and line 4 start -> hold 3.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 4, 14.5), Some(3));
        // Before everything in window -> None.
        assert_eq!(resolve_spoken_idx(f, ts.len(), 0, -1.0), None);
        // Empty work.
        assert_eq!(resolve_spoken_idx(f, 0, 0, 12.0), None);
    }
}
```

- [ ] **Step 2: Register the module** — in `src/input/mod.rs` add `pub mod phrase_highlight;` next to the other `pub mod` lines.

- [ ] **Step 3: Run tests**

Run: `cargo test phrase_highlight::; echo exit=$?` (falls back to `cargo test phrase_at_time resolve_spoken` if the module filter matches nothing)
Expected: 2 passed, `exit=0`

- [ ] **Step 4: Commit**

```bash
git add src/input/phrase_highlight.rs src/input/mod.rs
git commit -m "feat(sync): pure phrase-at-time + spoken-line resolution for phrase highlight"
```

---

### Task 3: Config flags

**Files:**
- Modify: `src/config.rs` — struct fields (~line 40-105 region), the manual `Default for Config` impl (~line 200 region), a `default_*` fn, and a test.

**Interfaces:**
- Produces: `config.phrase_highlight_prose: bool` (default `true`), `config.phrase_highlight_verse: bool` (default `false`).

- [ ] **Step 1: Write the failing test** (in config.rs's tests module; create one if absent)

```rust
#[test]
fn phrase_highlight_defaults_prose_on_verse_off() {
    let cfg: Config = serde_json::from_str("{}").unwrap();
    assert!(cfg.phrase_highlight_prose);
    assert!(!cfg.phrase_highlight_verse);
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test phrase_highlight_defaults; echo exit=$?` → compile error, fields missing.

- [ ] **Step 3: Implement** — add to the `Config` struct (near the other display toggles):

```rust
    /// Karaoke spoken-phrase highlight during narration sync, per work class.
    /// Prose defaults ON; plays/poetry default OFF (no phrase_timestamps data
    /// yet for verse — forward-looking). Alt+p toggles the current class.
    #[serde(default = "default_phrase_highlight_prose")]
    pub phrase_highlight_prose: bool,
    #[serde(default)]
    pub phrase_highlight_verse: bool,
```

Add the default fn next to its siblings:

```rust
fn default_phrase_highlight_prose() -> bool {
    true
}
```

Add to the `Default for Config` impl body:

```rust
            phrase_highlight_prose: true,
            phrase_highlight_verse: false,
```

- [ ] **Step 4: Run** — `cargo test phrase_highlight_defaults; echo exit=$?` → 1 passed, exit=0.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs && git commit -m "feat(config): phrase_highlight_prose/verse flags (prose on, verse off)"
```

---

### Task 4: Theme color, TextTag, AppState fields, buffer_line_for_work

**Files:**
- Modify: `src/theme.rs` — `Theme` struct (~line 14), parse fn (after `cursor_line_bg`, ~line 159), fallback `Default`-ish literal (~line 238).
- Modify: `src/app/mod.rs` — tag creation (~line 965, after `cursor_fade_tag`), `AppState` struct (near `cursor_line_tag` ~line 247), state init (~line 1566/1609), new method next to `work_line_for_buffer` (~line 703).
- Modify: `src/input/actions/settings.rs` — theme-change refresh (~line 292).

**Interfaces:**
- Produces: `theme.phrase_highlight_bg: String`; `state.phrase_tag: gtk4::TextTag`; `state.phrase_cache: Option<crate::input::phrase_highlight::PhraseCache>`; `state.active_phrase: Option<(usize, usize)>` (buffer_line, span_idx); `AppState::buffer_line_for_work(&self, work_idx: usize) -> Option<usize>`.

- [ ] **Step 1: theme.rs** — struct field:

```rust
    /// Spoken-phrase background during narration sync (karaoke highlight).
    pub phrase_highlight_bg: String,
```

Parse (immediately after the `cursor_line_bg` binding; optional per-theme key, fallback derives the cursor hue at a stronger alpha):

```rust
    let phrase_highlight_bg = str_field(lit, "phrase_highlight_bg").unwrap_or_else(|| {
        let (r, g, b) = rgba_str_to_rgb(&cursor_line_bg);
        format!(
            "rgba({}, {}, {}, 0.28)",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8
        )
    });
```

Add `phrase_highlight_bg,` to the constructed Theme and `phrase_highlight_bg: "rgba(255, 255, 255, 0.22)".to_string(),` to the hardcoded fallback theme (~line 238).

- [ ] **Step 2: app/mod.rs tag** (after `cursor_fade_tag` is added, ~line 969) — span `background`, NOT `paragraph_background` (the tint covers only the phrase's chars):

```rust
    let phrase_tag = gtk4::TextTag::builder()
        .name("phrase-highlight")
        .background(&theme.phrase_highlight_bg)
        .build();
    buffer.tag_table().add(&phrase_tag);
```

- [ ] **Step 3: AppState fields** (near `cursor_line_tag: gtk4::TextTag`):

```rust
    pub phrase_tag: gtk4::TextTag,
    /// Cached phrase spans for the (line, media) being narrated. Empty spans
    /// vec = cached negative result; see phrase_highlight.rs.
    pub phrase_cache: Option<crate::input::phrase_highlight::PhraseCache>,
    /// Last applied phrase (buffer_line, span_idx) — skips redundant re-tags.
    pub active_phrase: Option<(usize, usize)>,
```

Initialize in the AppState construction where `cursor_line_tag,` is listed: `phrase_tag,` and near `media_id: None,`: `phrase_cache: None,` / `active_phrase: None,`.

- [ ] **Step 4: buffer_line_for_work** (below `work_line_for_buffer`, mirroring its fallback):

```rust
    /// Inverse of `work_line_for_buffer`: the buffer line rendering work line
    /// `work_idx`. Identity when no line map is loaded (DB-rendered works).
    pub fn buffer_line_for_work(&self, work_idx: usize) -> Option<usize> {
        if let Some(ref lm) = self.line_map {
            lm.work_to_buffer.get(work_idx).copied()
        } else {
            let count = self.current_work.as_ref().map_or(0, |w| w.lines.len());
            if work_idx < count { Some(work_idx) } else { None }
        }
    }
```

- [ ] **Step 5: settings.rs theme refresh** (next to the `cursor_line_tag` set_property at ~292):

```rust
    state.phrase_tag.set_property("background", &theme.phrase_highlight_bg);
```

- [ ] **Step 6: Build** — `cargo build 2>&1 | rg "^error" ; echo exit=$?` → no errors (warnings ok), then `cargo test; echo exit=$?` still green.

- [ ] **Step 7: Commit**

```bash
git add src/theme.rs src/app/mod.rs src/input/actions/settings.rs
git commit -m "feat(sync): phrase-highlight tag, theme color, AppState cache fields"
```

---

### Task 5: Runtime update + TimePos hook + lifecycle clears

**Files:**
- Modify: `src/input/phrase_highlight.rs` — add the runtime fns.
- Modify: `src/main.rs` — call from `MpvEvent::TimePos` (~line 639, after the `pending_advance` block, BEFORE the `ov_moved` drop).
- Modify: `src/app/mod.rs` — reset cache in `display_work` where `state.media_id = work.media_id;` (~line 2695).

**Interfaces:**
- Consumes: Tasks 1-4 (`phrase_spans_for_line`, `PhraseCache`, `phrase_at_time`, `resolve_spoken_idx`, `state.phrase_tag/phrase_cache/active_phrase`, `buffer_line_for_work`, config flags).
- Produces: `pub fn update_phrase_highlight(s: &mut AppState, pos: f64)`; `pub fn clear_phrase_highlight(s: &mut AppState)`.

- [ ] **Step 1: Runtime fns** (append to `phrase_highlight.rs`):

```rust
use crate::app::AppState;
use gtk4::prelude::*;

/// Per-TimePos driver. Gates: class flag (prose vs verse), sync on, not
/// loading, translations hidden (inflated buffer misaligns offsets), not
/// sync-suppressed (manual seeks/nav clear the tint). Pause KEEPS the last
/// phrase visible — it marks where the audio stopped.
pub fn update_phrase_highlight(s: &mut AppState, pos: f64) {
    let enabled = if s.is_prose() {
        s.config.phrase_highlight_prose
    } else {
        s.config.phrase_highlight_verse
    };
    let suppressed = s
        .suppress_sync_until
        .map(|until| std::time::Instant::now() < until)
        .unwrap_or(false);
    if !enabled || !s.sync_enabled || s.loading_work.get() || s.translations_visible || suppressed
    {
        clear_phrase_highlight(s);
        return;
    }
    if !s.mpv_playing {
        return;
    }
    let Some(media) = s.media_id else {
        clear_phrase_highlight(s);
        return;
    };
    let Some(cursor_wi) = s.work_line_for_buffer(s.current_line) else {
        return;
    };
    // The sync cursor leads by SYNC_PREROLL, so resolve the line actually
    // being spoken at raw `pos` in a bounded window around the cursor.
    let spoken = {
        let Some(work) = s.current_work.as_ref() else { return };
        let lines = &work.lines;
        resolve_spoken_idx(
            |i| lines.get(i).and_then(|l| l.timestamp.as_ref()).map(|t| (t.start, t.end)),
            lines.len(),
            cursor_wi,
            pos,
        )
        .map(|wi| (wi, lines[wi].id))
    };
    let Some((spoken_wi, line_id)) = spoken else {
        clear_phrase_highlight(s);
        return;
    };
    let cache_stale = s
        .phrase_cache
        .as_ref()
        .map(|c| c.line_mapping_id != line_id || c.media_id != media)
        .unwrap_or(true);
    if cache_stale {
        let spans = crate::db::queries::open_db()
            .map(|conn| crate::db::queries::phrase_spans_for_line(&conn, line_id, media))
            .unwrap_or_default();
        crate::logging::log(&format!(
            "PHRASE_HL: cache fill line_id={} media={} spans={}",
            line_id,
            media,
            spans.len()
        ));
        s.phrase_cache = Some(PhraseCache { line_mapping_id: line_id, media_id: media, spans });
    }
    let hit = s
        .phrase_cache
        .as_ref()
        .and_then(|c| phrase_at_time(&c.spans, pos).map(|i| (c.spans[i], i)));
    let Some((span, span_idx)) = hit else {
        clear_phrase_highlight(s);
        return;
    };
    let Some(bl) = s.buffer_line_for_work(spoken_wi) else {
        return;
    };
    if s.active_phrase == Some((bl, span_idx)) {
        return;
    }
    apply_phrase_tag(s, bl, span.start_char, span.end_char);
    s.active_phrase = Some((bl, span_idx));
}

/// Remove the phrase tint everywhere. Cheap no-op when nothing is applied.
pub fn clear_phrase_highlight(s: &mut AppState) {
    if s.active_phrase.is_none() {
        return;
    }
    let (bs, be) = s.buffer.bounds();
    s.buffer.remove_tag(&s.phrase_tag, &bs, &be);
    s.active_phrase = None;
}

/// Move the tag to `[start_char, end_char)` of buffer line `bl`, clamped to
/// the line's char count (GTK iter offsets are unicode chars, matching the
/// Python backfill's str indices; clamping guards data drift).
fn apply_phrase_tag(s: &AppState, bl: usize, start_char: usize, end_char: usize) {
    let buffer = &s.buffer;
    let (bs, be) = buffer.bounds();
    buffer.remove_tag(&s.phrase_tag, &bs, &be);
    let Some(line_start) = buffer.iter_at_line(bl as i32) else {
        return;
    };
    let line_chars = {
        let mut e = line_start;
        if !e.ends_line() {
            e.forward_to_line_end();
        }
        e.line_offset().max(0) as usize
    };
    let sc = start_char.min(line_chars);
    let ec = end_char.min(line_chars).max(sc);
    if ec == sc {
        return;
    }
    let mut a = line_start;
    a.set_line_offset(sc as i32);
    let mut b = line_start;
    b.set_line_offset(ec as i32);
    buffer.apply_tag(&s.phrase_tag, &a, &b);
}
```

- [ ] **Step 2: main.rs hook** — in `MpvEvent::TimePos(pos)`, after the `pending_advance` block closes (~line 639) and BEFORE `// Mirror the advanced cursor into the translation overlay.`:

```rust
                        // Karaoke phrase highlight tracks the RAW time (the
                        // sync cursor above leads by SYNC_PREROLL; this must not).
                        crate::input::phrase_highlight::update_phrase_highlight(&mut s, pos);
```

- [ ] **Step 3: display_work reset** — in `src/app/mod.rs` right after `state.media_id = work.media_id;` (~2695):

```rust
    state.phrase_cache = None;
    state.active_phrase = None;
```

- [ ] **Step 4: Build + full test** — `cargo build; echo exit=$?` then `cargo test; echo exit=$?` → both exit=0.

- [ ] **Step 5: Commit**

```bash
git add src/input/phrase_highlight.rs src/main.rs src/app/mod.rs
git commit -m "feat(sync): drive spoken-phrase highlight from TimePos at raw time"
```

---

### Task 6: Toggle action, keybind, overlay, stow JSON

**Files:**
- Modify: `src/input/actions/mod.rs` — `Action` enum variant + `Category::Media` arm (~line 216-226) + `name()` arm (~line 346-362). (`parse_action` is a serde round-trip — the derive covers JSON loading; no parser change.)
- Modify: `src/input/keymap_config.rs` — `(KeyCombo::alt("p"), Action::TogglePhraseHighlight),` in the media bindings group (near `alt("backslash")` ~line 300).
- Modify: `src/input/keymap.rs` — dispatch arm (next to `ToggleVocabHighlight` ~line 2909).
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — add the binding object.
- Modify: `src/ui/keybinds_overlay.rs` — `p` KeyDef extras (~line 55) + `describe()` arm (~line 302 region). Run the update-cairo-keybinds-overlay skill's three-pass cross-check.

**Interfaces:**
- Consumes: `clear_phrase_highlight` (Task 5), config flags (Task 3), `show_chapter_toast` (existing transient toast in `src/input/navigation.rs`), `crate::config::save` (existing).
- Produces: `Action::TogglePhraseHighlight`, bound to Alt+p.

- [ ] **Step 1: Action variant** — add `TogglePhraseHighlight,` to the enum; add to the `Category::Media` OR-chain; add `Action::TogglePhraseHighlight => "TogglePhraseHighlight",` to `name()`. If the enum has a keybinds-overlay description helper or an exhaustive test over variants, extend it (compiler/test will say).

- [ ] **Step 2: Default binding** — in `keymap_config.rs` media bindings:

```rust
        (KeyCombo::alt("p"), Action::TogglePhraseHighlight),
```

- [ ] **Step 3: Dispatch arm** in `keymap.rs` (RPD note: `p` is a plain letter key; `alt` passes through unmodified):

```rust
        TogglePhraseHighlight => {
            let mut s = state.borrow_mut();
            let is_prose = s.is_prose();
            let now_on = if is_prose {
                s.config.phrase_highlight_prose = !s.config.phrase_highlight_prose;
                s.config.phrase_highlight_prose
            } else {
                s.config.phrase_highlight_verse = !s.config.phrase_highlight_verse;
                s.config.phrase_highlight_verse
            };
            crate::config::save(&s.config);
            if !now_on {
                crate::input::phrase_highlight::clear_phrase_highlight(&mut s);
            }
            let text = format!(
                "Phrase highlight {} ({})",
                if now_on { "ON" } else { "OFF" },
                if is_prose { "prose" } else { "plays/poetry" },
            );
            crate::input::navigation::show_chapter_toast(&s, &text);
            crate::logging::log(&format!("PHRASE_HL: toggled {}", text));
        }
```

(If `show_chapter_toast` has a different visibility/signature, use the same toast idiom the `ToggleVocabHighlight`-adjacent arms use — check the nearby `show_toast`/toast call in keymap.rs and match it.)

- [ ] **Step 4: Stow keymap.json** — in `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` add alongside the ToggleVocabHighlight line:

```json
    {"key": "p", "alt": true, "action": "TogglePhraseHighlight"},
```

(The file is stowed — editing the tty-dotfiles copy edits `~/.config/linux-lit/keymap.json` through the symlink; verify with `ls -l ~/.config/linux-lit/keymap.json`.)

- [ ] **Step 5: Ctrl+/ overlay** — `p` KeyDef gains the Alt slot:

```rust
    key("p", "P", "nudge \u{2212}0.2", "P: +0.2", &[("S-C-p", "conc word"), ("M-p", "phrase hl")]),
```

`describe()` arm:

```rust
        "phrase hl" => "Toggle the karaoke spoken-phrase highlight during narration \
            sync for the current work's class (prose defaults ON; plays/poetry OFF; \
            persisted in config). -> TogglePhraseHighlight arm — src/input/phrase_highlight.rs",
```

Then run the three-pass cross-reference from the update-cairo-keybinds-overlay skill (no blank slot hides a real binding; no label names the wrong action; every label has a describe arm).

- [ ] **Step 6: Build + test** — `cargo build; echo exit=$?`, `cargo test; echo exit=$?` → exit=0.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs src/ui/keybinds_overlay.rs
git commit -m "feat(keybind): Alt+p toggles phrase highlight per work class"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && git commit -m "linux-lit: Alt+p TogglePhraseHighlight" && cd ~/utono/linux-lit
```

---

### Task 7: Full verification + merge

- [ ] **Step 1: Full suite + clippy** — run each and check ITS OWN exit code (no pipes before checking):

```bash
cargo test
cargo clippy
```

Expected: exit 0 for both (fix anything red before proceeding).

- [ ] **Step 2: Merge to master per house rules**

```bash
git checkout master && git merge --no-ff feat/phrase-highlight -m "Merge feat/phrase-highlight: karaoke spoken-phrase highlight during narration sync"
cargo test
git push origin master
git branch -d feat/phrase-highlight
```

- [ ] **Step 3: Live acceptance (user)** — relaunch `crll` on a Dickens work with audio: the spoken phrase gets the stronger tint tracking the narrator exactly (no lead); page turns still lead by 0.5s; Alt+p toggles with toast; grep `PHRASE_HL:` in `linux-lit-dev.log` for cache fills.

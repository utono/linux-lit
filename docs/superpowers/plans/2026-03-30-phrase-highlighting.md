# Phrase-Level WhisperSync Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Highlight groups of 3-5 words as they are spoken during audio playback, replacing the dim/undim model with a background-highlighted active phrase.

**Architecture:** A Python script aligns whisperX words to Gutenberg text and groups them into phrases stored in a new `phrase_timestamps` DB table. The Rust app loads phrases at work-open, binary searches on each time_pos event, and applies/removes a background TextTag on the active phrase's character range.

**Tech Stack:** Python (difflib, sqlite3), Rust (GTK4 TextTag, sourceview5), SQLite

---

### Task 1: Create phrase_timestamps table

**Files:**
- Create: `~/utono/litdb/scripts/migrations/add_phrase_timestamps.sql`

- [ ] **Step 1: Write the migration SQL**

Create `~/utono/litdb/scripts/migrations/add_phrase_timestamps.sql`:

```sql
CREATE TABLE IF NOT EXISTS phrase_timestamps (
    id INTEGER PRIMARY KEY,
    line_mapping_id INTEGER NOT NULL,
    media_id INTEGER NOT NULL,
    start_time REAL NOT NULL,
    end_time REAL NOT NULL,
    start_char INTEGER NOT NULL,
    end_char INTEGER NOT NULL,
    FOREIGN KEY (line_mapping_id) REFERENCES line_mapping(id),
    FOREIGN KEY (media_id) REFERENCES media_files(id)
);

CREATE INDEX IF NOT EXISTS idx_phrase_timestamps_work
    ON phrase_timestamps(line_mapping_id, media_id);

CREATE INDEX IF NOT EXISTS idx_phrase_timestamps_time
    ON phrase_timestamps(media_id, start_time);
```

- [ ] **Step 2: Run the migration**

```bash
sqlite3 ~/utono/litdb/data/lit.db < ~/utono/litdb/scripts/migrations/add_phrase_timestamps.sql
```

- [ ] **Step 3: Verify**

```bash
sqlite3 ~/utono/litdb/data/lit.db ".schema phrase_timestamps"
```

Expected: table and indices created.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/litdb && git add scripts/migrations/add_phrase_timestamps.sql
git commit -m "feat: add phrase_timestamps table for word-level highlighting"
```

---

### Task 2: Build phrase timestamps Python script

**Files:**
- Create: `~/utono/litdb/scripts/build_phrase_timestamps.py`

- [ ] **Step 1: Create the script with word alignment and phrase grouping**

Create `~/utono/litdb/scripts/build_phrase_timestamps.py`:

```python
#!/usr/bin/env python3
"""Build phrase_timestamps from whisperX word-level data aligned to Gutenberg text.

Aligns whisperX words to line_mapping canonical_text at word granularity,
groups them into phrases (punctuation-aware, time-gapped, max 5 words),
and writes to phrase_timestamps table.

Usage:
    python build_phrase_timestamps.py <WORK_ABBREV> <MEDIA_ID> <WHISPERX_JSON> [--dry-run]
"""

import argparse
import difflib
import json
import re
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common.db_utils import LIT_DB

PHRASE_MAX_WORDS = 5
SILENCE_GAP = 0.3  # seconds — break phrase on gaps longer than this
BREAK_AFTER_PUNCT = set(',.;:!?')


def normalize(text: str) -> str:
    """Lowercase, strip punctuation, collapse whitespace."""
    return re.sub(r'\s+', ' ', re.sub(r'[^\w\s]', '', text.lower())).strip()


def load_whisperx_words(whisperx_json: Path) -> list[dict]:
    """Load word-level entries from whisperX JSON with timestamp interpolation."""
    data = json.loads(whisperx_json.read_text(encoding='utf-8'))
    segments = data.get('segments', [])

    raw: list[dict] = []
    for seg in segments:
        seg_start = seg.get('start', 0.0)
        seg_end = seg.get('end', 0.0)
        for w in seg.get('words', []):
            raw.append({
                'word': w.get('word', ''),
                'start': w.get('start', None),
                'end': w.get('end', None),
                '_seg_start': seg_start,
                '_seg_end': seg_end,
            })

    # Normalise sentinel values
    for entry in raw:
        if entry['start'] is not None and entry['start'] < 0:
            entry['start'] = None
        if entry['end'] is not None and entry['end'] < 0:
            entry['end'] = None

    # Forward interpolation
    for i in range(1, len(raw)):
        if raw[i]['start'] is None and raw[i - 1]['end'] is not None:
            raw[i]['start'] = raw[i - 1]['end']

    # Backward interpolation
    for i in range(len(raw) - 2, -1, -1):
        if raw[i]['end'] is None and raw[i + 1]['start'] is not None:
            raw[i]['end'] = raw[i + 1]['start']
        if raw[i]['start'] is None and raw[i]['end'] is not None:
            raw[i]['start'] = raw[i]['end']

    # Segment boundary fallback
    for entry in raw:
        if entry['start'] is None:
            entry['start'] = entry['_seg_start']
        if entry['end'] is None:
            entry['end'] = entry['_seg_end']

    return raw


def align_words(
    gut_lines: list[dict],
    wx_words: list[dict],
) -> list[dict]:
    """Align whisperX words to Gutenberg text at word granularity.

    Returns a list of aligned word dicts:
    {line_mapping_id, char_start, char_end, start_time, end_time, word}

    Each entry maps one Gutenberg word to its whisperX timing.
    """
    # Build Gutenberg word stream with character positions
    gut_word_stream: list[str] = []       # normalized words
    gut_word_meta: list[dict] = []        # {line_mapping_id, char_start, char_end, raw_word}

    for line in gut_lines:
        text = line['canonical_text']
        lm_id = line['id']
        # Find each word's character offset in canonical_text
        for m in re.finditer(r'\S+', text):
            raw_word = m.group()
            gut_word_stream.append(normalize(raw_word))
            gut_word_meta.append({
                'line_mapping_id': lm_id,
                'char_start': m.start(),
                'char_end': m.end(),
                'raw_word': raw_word,
            })

    # Build whisperX normalized word stream
    wx_norm: list[str] = [normalize(w['word']) for w in wx_words]

    print(f'  Gutenberg words: {len(gut_word_stream):,}')
    print(f'  WhisperX words:  {len(wx_norm):,}')
    print('  Running SequenceMatcher alignment...')

    sm = difflib.SequenceMatcher(None, gut_word_stream, wx_norm, autojunk=False)
    opcodes = sm.get_opcodes()

    # Build aligned word list
    aligned: list[dict] = []
    for op, i1, i2, j1, j2 in opcodes:
        if op == 'equal':
            for offset in range(i2 - i1):
                gi = i1 + offset
                wi = j1 + offset
                meta = gut_word_meta[gi]
                aligned.append({
                    'line_mapping_id': meta['line_mapping_id'],
                    'char_start': meta['char_start'],
                    'char_end': meta['char_end'],
                    'start_time': wx_words[wi]['start'],
                    'end_time': wx_words[wi]['end'],
                    'raw_word': meta['raw_word'],
                })
        elif op == 'replace':
            gut_span = i2 - i1
            wx_span = j2 - j1
            for offset in range(gut_span):
                gi = i1 + offset
                wx_offset = round(offset * wx_span / gut_span) if gut_span else 0
                wi = j1 + min(wx_offset, wx_span - 1)
                meta = gut_word_meta[gi]
                aligned.append({
                    'line_mapping_id': meta['line_mapping_id'],
                    'char_start': meta['char_start'],
                    'char_end': meta['char_end'],
                    'start_time': wx_words[wi]['start'],
                    'end_time': wx_words[wi]['end'],
                    'raw_word': meta['raw_word'],
                })
        # 'insert' and 'delete' opcodes: skip (no alignment)

    matched = len(aligned)
    total = len(gut_word_stream)
    print(f'  Aligned: {matched:,}/{total:,} words ({100*matched/total:.1f}%)')

    return aligned


def ends_with_punct(word: str) -> bool:
    """Check if a word ends with phrase-breaking punctuation."""
    stripped = word.rstrip()
    return bool(stripped) and stripped[-1] in BREAK_AFTER_PUNCT


def group_into_phrases(aligned_words: list[dict]) -> list[dict]:
    """Group aligned words into phrases.

    Phrase breaks occur at:
    - Punctuation (comma, period, semicolon, colon, !, ?)
    - Silence gap > SILENCE_GAP seconds between words
    - Max PHRASE_MAX_WORDS words per phrase

    Returns list of phrase dicts:
    {line_mapping_id, start_time, end_time, start_char, end_char}

    A phrase that spans a line break is split into multiple rows
    sharing the same start_time/end_time.
    """
    if not aligned_words:
        return []

    phrases: list[dict] = []
    current_words: list[dict] = []

    def flush_phrase():
        if not current_words:
            return
        # Group by line_mapping_id
        by_line: dict[int, list[dict]] = {}
        for w in current_words:
            by_line.setdefault(w['line_mapping_id'], []).append(w)

        phrase_start = current_words[0]['start_time']
        phrase_end = current_words[-1]['end_time']

        for lm_id, words in by_line.items():
            phrases.append({
                'line_mapping_id': lm_id,
                'start_time': phrase_start,
                'end_time': phrase_end,
                'start_char': words[0]['char_start'],
                'end_char': words[-1]['char_end'],
            })

    for i, word in enumerate(aligned_words):
        current_words.append(word)

        should_break = False

        # Break after punctuation
        if ends_with_punct(word['raw_word']):
            should_break = True

        # Break on max words
        if len(current_words) >= PHRASE_MAX_WORDS:
            should_break = True

        # Break on silence gap before next word
        if not should_break and i + 1 < len(aligned_words):
            next_word = aligned_words[i + 1]
            gap = next_word['start_time'] - word['end_time']
            if gap > SILENCE_GAP:
                should_break = True

        if should_break:
            flush_phrase()
            current_words = []

    flush_phrase()  # final phrase

    return phrases


def main() -> None:
    parser = argparse.ArgumentParser(
        description='Build phrase_timestamps from whisperX word data')
    parser.add_argument('work_abbrev')
    parser.add_argument('media_id', type=int)
    parser.add_argument('whisperx_json', type=Path)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()

    if not args.whisperx_json.exists():
        print(f'ERROR: {args.whisperx_json} not found', file=sys.stderr)
        sys.exit(1)

    conn = sqlite3.connect(str(LIT_DB), timeout=30)

    # Load Gutenberg lines
    rows = conn.execute("""
        SELECT id, canonical_text
        FROM line_mapping
        WHERE work_abbrev = ?
        ORDER BY div1, COALESCE(div2, 0), line_in_div
    """, (args.work_abbrev,)).fetchall()

    if not rows:
        print(f'ERROR: No lines for {args.work_abbrev}', file=sys.stderr)
        sys.exit(1)

    gut_lines = [{'id': r[0], 'canonical_text': r[1]} for r in rows]
    print(f'Loaded {len(gut_lines):,} lines from line_mapping')

    # Load whisperX words
    wx_words = load_whisperx_words(args.whisperx_json)
    print(f'Loaded {len(wx_words):,} whisperX words')

    # Align
    aligned = align_words(gut_lines, wx_words)

    # Group into phrases
    phrases = group_into_phrases(aligned)
    print(f'Built {len(phrases):,} phrases')

    if args.dry_run:
        print('\nDRY RUN — sample phrases:')
        for p in phrases[:15]:
            text_row = conn.execute(
                "SELECT canonical_text FROM line_mapping WHERE id = ?",
                (p['line_mapping_id'],)).fetchone()
            text = text_row[0] if text_row else ''
            snippet = text[p['start_char']:p['end_char']]
            print(f"  {p['start_time']:8.2f}s-{p['end_time']:8.2f}s  "
                  f"chars [{p['start_char']}:{p['end_char']}]  \"{snippet}\"")
        conn.close()
        return

    # Clear existing phrases for this work+media
    conn.execute("""
        DELETE FROM phrase_timestamps
        WHERE media_id = ?
        AND line_mapping_id IN (
            SELECT id FROM line_mapping WHERE work_abbrev = ?
        )
    """, (args.media_id, args.work_abbrev))

    # Insert
    conn.executemany("""
        INSERT INTO phrase_timestamps
            (line_mapping_id, media_id, start_time, end_time, start_char, end_char)
        VALUES (?, ?, ?, ?, ?, ?)
    """, [(p['line_mapping_id'], args.media_id, p['start_time'], p['end_time'],
           p['start_char'], p['end_char']) for p in phrases])

    conn.commit()
    print(f'Inserted {len(phrases):,} phrase_timestamps rows')

    # Verify
    count = conn.execute("""
        SELECT COUNT(*) FROM phrase_timestamps
        WHERE media_id = ? AND line_mapping_id IN (
            SELECT id FROM line_mapping WHERE work_abbrev = ?
        )
    """, (args.media_id, args.work_abbrev)).fetchone()[0]
    time_range = conn.execute("""
        SELECT MIN(start_time), MAX(end_time) FROM phrase_timestamps
        WHERE media_id = ? AND line_mapping_id IN (
            SELECT id FROM line_mapping WHERE work_abbrev = ?
        )
    """, (args.media_id, args.work_abbrev)).fetchone()
    print(f'Verify: {count:,} phrases, {time_range[0]:.1f}s - {time_range[1]:.1f}s')

    conn.close()


if __name__ == '__main__':
    main()
```

- [ ] **Step 2: Make executable and test dry-run on BH**

```bash
chmod +x ~/utono/litdb/scripts/build_phrase_timestamps.py
```

Find BH's media_id and whisperX JSON:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT mf.id, mf.path FROM media_files mf JOIN work_media_associations wma ON mf.id = wma.media_id WHERE wma.work_abbrev = 'BH' ORDER BY wma.priority DESC LIMIT 1"
```

Find whisperX JSON (in same directory as media, under whisperx-cache/):

```bash
fd whisperX ~/Music/dickens-charles/whisperx-cache/ --type f | head -5
```

Dry-run:

```bash
~/utono/litdb/.venv/bin/python3 ~/utono/litdb/scripts/build_phrase_timestamps.py BH <MEDIA_ID> <WHISPERX_JSON> --dry-run
```

Expected: prints word counts, alignment stats, sample phrases with character offsets.

- [ ] **Step 3: Run for real**

```bash
~/utono/litdb/.venv/bin/python3 ~/utono/litdb/scripts/build_phrase_timestamps.py BH <MEDIA_ID> <WHISPERX_JSON>
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/litdb && git add scripts/build_phrase_timestamps.py
git commit -m "feat: add build_phrase_timestamps.py for WhisperSync phrase alignment"
```

---

### Task 3: Add Phrase struct and load phrases in Rust

**Files:**
- Modify: `src/db/models.rs`
- Modify: `src/db/queries.rs`

- [ ] **Step 1: Add Phrase struct to models.rs**

Add after the `Timestamp` struct in `src/db/models.rs`:

```rust
#[derive(Debug, Clone)]
pub struct Phrase {
    pub line_id: i64,
    pub start_time: f64,
    pub end_time: f64,
    pub start_char: usize,
    pub end_char: usize,
}
```

Add `phrases` field to the `Work` struct:

```rust
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub text_file: Option<String>,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
    pub media_id: Option<i64>,
    pub phrases: Vec<Phrase>,
}
```

- [ ] **Step 2: Load phrases in queries.rs**

In `load_work()`, after step 5c (spoken status), add step 5d:

```rust
    // 5d. Load phrase timestamps for the active media
    let phrases: Vec<super::models::Phrase> = if let Some(mid) = media_id {
        let mut phrase_stmt = conn.prepare(
            "SELECT pt.line_mapping_id, pt.start_time, pt.end_time, \
             pt.start_char, pt.end_char \
             FROM phrase_timestamps pt \
             JOIN line_mapping lm ON pt.line_mapping_id = lm.id \
             WHERE lm.work_abbrev = ?1 AND pt.media_id = ?2 \
             ORDER BY pt.start_time",
        )?;
        phrase_stmt
            .query_map(rusqlite::params![abbrev, mid], |row| {
                Ok(super::models::Phrase {
                    line_id: row.get(0)?,
                    start_time: row.get(1)?,
                    end_time: row.get(2)?,
                    start_char: row.get::<_, i64>(3)? as usize,
                    end_char: row.get::<_, i64>(4)? as usize,
                })
            })?
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
```

Add `phrases` to the `Work` return at the end of `load_work()`:

```rust
    Ok(Work {
        abbrev: abbrev.to_string(),
        title,
        author,
        work_type,
        text_file,
        lines,
        timestamps,
        media_paths,
        media_id,
        phrases,
    })
```

Handle missing table gracefully — if `phrase_timestamps` doesn't exist yet, treat as empty. Wrap the query in a match:

```rust
    let phrases: Vec<super::models::Phrase> = if let Some(mid) = media_id {
        match conn.prepare(
            "SELECT pt.line_mapping_id, pt.start_time, pt.end_time, \
             pt.start_char, pt.end_char \
             FROM phrase_timestamps pt \
             JOIN line_mapping lm ON pt.line_mapping_id = lm.id \
             WHERE lm.work_abbrev = ?1 AND pt.media_id = ?2 \
             ORDER BY pt.start_time",
        ) {
            Ok(mut stmt) => {
                stmt.query_map(rusqlite::params![abbrev, mid], |row| {
                    Ok(super::models::Phrase {
                        line_id: row.get(0)?,
                        start_time: row.get(1)?,
                        end_time: row.get(2)?,
                        start_char: row.get::<_, i64>(3)? as usize,
                        end_char: row.get::<_, i64>(4)? as usize,
                    })
                }).ok()
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
            }
            Err(_) => Vec::new(), // table doesn't exist yet
        }
    } else {
        Vec::new()
    };
```

- [ ] **Step 3: Build and test**

```bash
cargo build
cargo test
```

Expected: compiles, all tests pass. Log should show phrase count when opening a work with phrase data.

- [ ] **Step 4: Add log line for phrase count**

In `src/app.rs` `display_work()`, after the line map logging, add:

```rust
if let Some(ref work) = state.current_work {
    if !work.phrases.is_empty() {
        crate::logging::log(&format!("PHRASES: loaded {} phrases", work.phrases.len()));
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add src/db/models.rs src/db/queries.rs src/app.rs
git commit -m "feat: load phrase_timestamps into Work.phrases"
```

---

### Task 4: Add phrase_tag and phrase highlighting state

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add phrase_tag to tag table**

In `src/app.rs` `build_window()`, after the `vocab_tag` definition (around line 234), add:

```rust
    let phrase_tag = gtk4::TextTag::builder()
        .name("phrase-highlight")
        .background(if theme.is_light {
            "rgba(66, 133, 244, 0.20)"
        } else {
            "rgba(100, 180, 255, 0.25)"
        })
        .build();
    buffer.tag_table().add(&phrase_tag);
```

- [ ] **Step 2: Add phrase state fields to AppState**

Add to the `AppState` struct:

```rust
    pub phrase_tag: gtk4::TextTag,
    pub current_phrase: Option<usize>,
    pub phrase_playing: bool,
```

Initialize in `build_window()` where AppState is constructed:

```rust
    phrase_tag,
    current_phrase: None,
    phrase_playing: false,
```

- [ ] **Step 3: Build to verify**

```bash
cargo build
```

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: add phrase_tag and phrase tracking state to AppState"
```

---

### Task 5: Phrase highlight on TimePos events

**Files:**
- Modify: `src/main.rs`
- Modify: `src/input/navigation.rs`

- [ ] **Step 1: Add phrase binary search helper**

Add to `src/input/navigation.rs`:

```rust
/// Find the phrase index for a given time position using binary search.
pub fn find_phrase_for_time(phrases: &[crate::db::models::Phrase], time_pos: f64) -> Option<usize> {
    if phrases.is_empty() {
        return None;
    }
    let idx = phrases.partition_point(|p| p.start_time <= time_pos);
    if idx == 0 {
        return None;
    }
    let phrase = &phrases[idx - 1];
    if time_pos <= phrase.end_time {
        Some(idx - 1)
    } else {
        None
    }
}
```

- [ ] **Step 2: Add apply/remove phrase highlight function**

Add to `src/input/navigation.rs`:

```rust
/// Apply phrase highlight tag to the active phrase, removing from previous position.
pub fn update_phrase_highlight(state: &mut AppState, new_phrase_idx: Option<usize>) {
    let buffer = &state.buffer;
    let tag = &state.phrase_tag;

    // Remove old phrase highlight
    if state.current_phrase.is_some() {
        let (buf_start, buf_end) = buffer.bounds();
        buffer.remove_tag(tag, &buf_start, &buf_end);
    }

    state.current_phrase = new_phrase_idx;

    let phrase_idx = match new_phrase_idx {
        Some(idx) => idx,
        None => return,
    };

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    let phrase = match work.phrases.get(phrase_idx) {
        Some(p) => p,
        None => return,
    };

    // Find buffer line for this phrase's line_id
    let work_idx = work.lines.iter().position(|l| l.id == phrase.line_id);
    let buffer_line = match work_idx {
        Some(wi) => {
            if let Some(ref lm) = state.line_map {
                if wi < lm.work_to_buffer.len() {
                    lm.work_to_buffer[wi]
                } else {
                    return;
                }
            } else {
                wi
            }
        }
        None => return,
    };

    // Apply tag to character range
    if let Some(line_start) = buffer.iter_at_line(buffer_line as i32) {
        let mut start_iter = line_start;
        start_iter.set_line_offset(phrase.start_char as i32);

        let mut end_iter = line_start;
        end_iter.set_line_offset(phrase.end_char as i32);

        buffer.apply_tag(tag, &start_iter, &end_iter);
    }

    // Update current_line to track the phrase's line for scrolling
    if state.current_line != buffer_line {
        state.current_line = buffer_line;
        ensure_visible_no_highlight(state);
    }
}

/// Remove phrase highlighting and revert to sentence dim model.
pub fn exit_phrase_mode(state: &mut AppState) {
    if state.phrase_playing {
        let (buf_start, buf_end) = state.buffer.bounds();
        state.buffer.remove_tag(&state.phrase_tag, &buf_start, &buf_end);
        state.current_phrase = None;
        state.phrase_playing = false;
        // Restore sentence highlighting
        update_highlight(state);
    }
}
```

- [ ] **Step 3: Hook into TimePos event handler in main.rs**

In `src/main.rs`, in the `MpvEvent::TimePos(pos)` handler (around line 207), add phrase lookup before the existing `pending_advance` logic:

```rust
                    MpvEvent::TimePos(pos) => {
                        let mut s = state_for_events.borrow_mut();
                        s.current_time_pos = pos;

                        // Phrase highlighting during playback
                        if let Some(ref work) = s.current_work {
                            if !work.phrases.is_empty() {
                                let new_idx = crate::input::navigation::find_phrase_for_time(
                                    &work.phrases, pos,
                                );
                                if new_idx != s.current_phrase {
                                    if !s.phrase_playing {
                                        // Entering phrase mode — remove dim tags
                                        let (bs, be) = s.buffer.bounds();
                                        s.buffer.remove_tag(&s.dim_tag, &bs, &be);
                                        s.phrase_playing = true;
                                    }
                                    crate::input::navigation::update_phrase_highlight(
                                        &mut s, new_idx,
                                    );
                                }
                            }
                        }

                        // Existing pending_advance logic below...
```

- [ ] **Step 4: Exit phrase mode on pause**

In the `MpvEvent::PlaybackState(playing)` handler in `src/main.rs` (around line 201), add phrase mode exit when playback stops:

```rust
                    MpvEvent::PlaybackState(playing) => {
                        crate::logging::log(&format!(
                            "MPV playback: {}",
                            if playing { "playing" } else { "paused" }
                        ));
                        if !playing {
                            let mut s = state_for_events.borrow_mut();
                            crate::input::navigation::exit_phrase_mode(&mut s);
                        }
                    }
```

- [ ] **Step 5: Build and test**

```bash
cargo build
cargo test
```

- [ ] **Step 6: Commit**

```bash
git add src/input/navigation.rs src/main.rs
git commit -m "feat: phrase highlighting on TimePos with binary search and background tag"
```

---

### Task 6: Update wizard-gutenberg skill

**Files:**
- Modify: `~/utono/litdb/.claude/skills/wizard-gutenberg/SKILL.md`

- [ ] **Step 1: Add phrase building step**

After Step 9 (Fix sentence boundaries), add:

```markdown
### Step 10: Build phrase timestamps

```bash
~/utono/litdb/.venv/bin/python3 ~/utono/litdb/scripts/build_phrase_timestamps.py \
    <WORK_ABBREV> <MEDIA_ID> \
    ~/Music/<author-dir>/whisperx-cache/<audio-stem>.whisperX-transcript-medium.en.json
```

Dry-run first:

```bash
~/utono/litdb/.venv/bin/python3 ~/utono/litdb/scripts/build_phrase_timestamps.py \
    <WORK_ABBREV> <MEDIA_ID> \
    ~/Music/<author-dir>/whisperx-cache/<audio-stem>.whisperX-transcript-medium.en.json \
    --dry-run
```
```

Renumber the existing Step 10 (Test in linux-lit) to Step 11.

- [ ] **Step 2: Commit**

```bash
cd ~/utono/litdb && git add .claude/skills/wizard-gutenberg/SKILL.md
git commit -m "feat: add phrase timestamps step to wizard-gutenberg"
```

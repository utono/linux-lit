# Ollama Sentence Boundary Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace text-heuristic sentence grouping with Ollama-powered detection, stored in lit.db and consumed by the Rust app.

**Architecture:** A Python script (`detect_sentences.py`) sends paragraph batches to Ollama (`qwen2.5:7b`), parses sentence boundary responses, and writes `sentence_start_time`/`sentence_end_time` to `line_timestamps`. The Rust app loads these values and builds `sentence_groups` from them, falling back to text heuristics when DB data is absent.

**Tech Stack:** Python 3 (requests, sqlite3), Ollama API, Rust (rusqlite)

---

### Task 1: Create detect_sentences.py — CLI and DB loading

**Files:**
- Create: `~/utono/litdb/scripts/detect_sentences.py`

- [ ] **Step 1: Create the script with argparse and DB loading**

```python
#!/usr/bin/env python3
"""Detect sentence boundaries in a prose work using Ollama.

Sends paragraphs to Ollama (qwen2.5:7b) and writes sentence_start_time /
sentence_end_time to line_timestamps.
"""

import argparse
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common.db_utils import LIT_DB

OLLAMA_ENDPOINT = "http://localhost:11434"
OLLAMA_MODEL = "qwen2.5:7b"

SYSTEM_PROMPT = (
    "Given numbered lines from a paragraph of literary text, identify "
    "where each sentence begins. Output ONLY the line numbers where a new "
    "sentence starts, one per line. Line 1 always starts a sentence."
)


def load_lines(db_path, abbrev):
    """Load line_mapping rows for a work, ordered by div1, div2, line_in_div.

    Returns list of dicts: {id, line_in_div, canonical_text, div1, div2}.
    """
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT id, line_in_div, canonical_text, div1, div2 "
        "FROM line_mapping WHERE work_abbrev = ? "
        "ORDER BY div1, div2, line_in_div",
        (abbrev,),
    ).fetchall()
    conn.close()
    if not rows:
        print(f"Error: no lines found for work '{abbrev}'", file=sys.stderr)
        sys.exit(1)
    return [dict(r) for r in rows]


def load_timestamps(db_path, abbrev, media_id=None):
    """Load start_time/end_time from line_timestamps, keyed by line_mapping_id.

    If media_id is None, uses the highest-priority media.
    Returns (ts_map, resolved_media_id) where ts_map is {line_mapping_id: (start, end)}.
    """
    conn = sqlite3.connect(db_path)

    if media_id is None:
        row = conn.execute(
            "SELECT wma.media_id FROM work_media_associations wma "
            "WHERE wma.work_abbrev = ? ORDER BY wma.priority DESC LIMIT 1",
            (abbrev,),
        ).fetchone()
        if row:
            media_id = row[0]

    if media_id is None:
        conn.close()
        print(f"Warning: no media found for work '{abbrev}'", file=sys.stderr)
        return {}, None

    rows = conn.execute(
        "SELECT line_mapping_id, start_time, end_time FROM line_timestamps "
        "WHERE media_id = ?",
        (media_id,),
    ).fetchall()
    conn.close()

    ts_map = {}
    for r in rows:
        if r[1] is not None and r[2] is not None:
            ts_map[r[0]] = (r[1], r[2])
    return ts_map, media_id


def group_into_paragraphs(lines):
    """Group lines into paragraphs (split on blank canonical_text).

    Returns list of lists, each inner list is a paragraph of line dicts.
    """
    paragraphs = []
    current = []
    for line in lines:
        text = (line["canonical_text"] or "").strip()
        if not text:
            if current:
                paragraphs.append(current)
                current = []
        else:
            current.append(line)
    if current:
        paragraphs.append(current)
    return paragraphs


def parse_args():
    parser = argparse.ArgumentParser(
        description="Detect sentence boundaries using Ollama"
    )
    parser.add_argument("abbrev", help="Work abbreviation (e.g. BH)")
    parser.add_argument("--media-id", type=int, default=None,
                        help="Media ID (default: highest priority)")
    parser.add_argument("--endpoint", default=OLLAMA_ENDPOINT,
                        help=f"Ollama endpoint (default: {OLLAMA_ENDPOINT})")
    parser.add_argument("--model", default=OLLAMA_MODEL,
                        help=f"Ollama model (default: {OLLAMA_MODEL})")
    parser.add_argument("--dry-run", action="store_true",
                        help="Preview boundaries without writing to DB")
    return parser.parse_args()


def main():
    args = parse_args()
    lines = load_lines(LIT_DB, args.abbrev)
    ts_map, media_id = load_timestamps(LIT_DB, args.abbrev, args.media_id)
    paragraphs = group_into_paragraphs(lines)
    print(f"Loaded {len(lines)} lines, {len(paragraphs)} paragraphs, "
          f"{len(ts_map)} timestamps (media_id={media_id})")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Test the script loads data correctly**

```bash
python3 ~/utono/litdb/scripts/detect_sentences.py BH --dry-run
```

Expected: prints line/paragraph/timestamp counts without errors.

- [ ] **Step 3: Commit**

```bash
cd ~/utono/litdb
git add scripts/detect_sentences.py
git commit -m "feat: add detect_sentences.py skeleton with DB loading"
```

---

### Task 2: Add Ollama API call and response parsing

**Files:**
- Modify: `~/utono/litdb/scripts/detect_sentences.py`

- [ ] **Step 1: Add the Ollama query function**

Add after the `SYSTEM_PROMPT` constant:

```python
import requests
import json


def query_ollama(endpoint, model, paragraph_lines):
    """Send a paragraph to Ollama and get sentence start line numbers.

    Args:
        endpoint: Ollama API base URL
        model: model name (e.g. "qwen2.5:7b")
        paragraph_lines: list of line dicts with "canonical_text"

    Returns list of 1-based line numbers where sentences start,
    or None if the request failed.
    """
    numbered = "\n".join(
        f"{i+1}: {line['canonical_text']}"
        for i, line in enumerate(paragraph_lines)
    )

    try:
        resp = requests.post(
            f"{endpoint}/api/generate",
            json={
                "model": model,
                "system": SYSTEM_PROMPT,
                "prompt": numbered,
                "stream": False,
            },
            timeout=60,
        )
        resp.raise_for_status()
    except requests.ConnectionError:
        print("Error: Ollama not running — start with: systemctl start ollama",
              file=sys.stderr)
        sys.exit(1)
    except requests.Timeout:
        print(f"Warning: timeout on paragraph ({len(paragraph_lines)} lines), skipping",
              file=sys.stderr)
        return None
    except requests.RequestException as e:
        print(f"Warning: request error: {e}, skipping", file=sys.stderr)
        return None

    text = resp.json().get("response", "")
    return parse_boundary_response(text, len(paragraph_lines))


def parse_boundary_response(text, line_count):
    """Parse Ollama response into list of 1-based sentence start line numbers.

    Extracts integers from the response, filters to valid range [1, line_count].
    Returns sorted list. Line 1 is always included.
    """
    starts = set()
    for token in text.split():
        token = token.strip().rstrip(".")
        try:
            n = int(token)
            if 1 <= n <= line_count:
                starts.add(n)
        except ValueError:
            continue
    starts.add(1)  # Line 1 always starts a sentence
    return sorted(starts)
```

- [ ] **Step 2: Add sentence group builder from start lines**

```python
def build_sentence_groups_from_starts(paragraph_lines, start_numbers):
    """Convert sentence start line numbers into groups of line dicts.

    Args:
        paragraph_lines: list of line dicts
        start_numbers: sorted list of 1-based line numbers where sentences start

    Returns list of lists, each inner list is a sentence (list of line dicts).
    """
    groups = []
    for i, start in enumerate(start_numbers):
        end = start_numbers[i + 1] if i + 1 < len(start_numbers) else len(paragraph_lines) + 1
        group = paragraph_lines[start - 1 : end - 1]
        if group:
            groups.append(group)
    return groups
```

- [ ] **Step 3: Test parse_boundary_response manually**

Add a quick test at the bottom (temporarily) or run in Python REPL:

```bash
python3 -c "
import sys; sys.path.insert(0, '$HOME/utono/litdb/scripts')
from detect_sentences import parse_boundary_response
assert parse_boundary_response('1\n1\n7', 10) == [1, 7]
assert parse_boundary_response('1\n3\n5\n', 8) == [1, 3, 5]
assert parse_boundary_response('garbage text 2 more 5', 6) == [1, 2, 5]
assert parse_boundary_response('', 5) == [1]
print('All parse tests passed')
"
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/litdb
git add scripts/detect_sentences.py
git commit -m "feat: add Ollama query and response parsing to detect_sentences"
```

---

### Task 3: Wire up main loop with dry-run and DB write

**Files:**
- Modify: `~/utono/litdb/scripts/detect_sentences.py`

- [ ] **Step 1: Replace the main function with full processing loop**

Replace the `main()` function:

```python
def write_sentence_times(db_path, media_id, sentence_groups, ts_map, dry_run):
    """Write sentence_start_time / sentence_end_time for each sentence group.

    For each group, sentence_start_time = start_time of first line,
    sentence_end_time = end_time of last line.
    """
    if dry_run:
        return

    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    for group in sentence_groups:
        line_ids = [line["id"] for line in group]
        # Find first line with start_time, last line with end_time
        first_start = None
        last_end = None
        for line in group:
            ts = ts_map.get(line["id"])
            if ts is not None:
                if first_start is None:
                    first_start = ts[0]
                last_end = ts[1]

        if first_start is None or last_end is None:
            continue

        placeholders = ",".join("?" for _ in line_ids)
        cursor.execute(
            f"UPDATE line_timestamps SET sentence_start_time = ?, sentence_end_time = ? "
            f"WHERE line_mapping_id IN ({placeholders}) AND media_id = ?",
            [first_start, last_end] + line_ids + [media_id],
        )
    conn.commit()
    conn.close()


def main():
    args = parse_args()
    lines = load_lines(LIT_DB, args.abbrev)
    ts_map, media_id = load_timestamps(LIT_DB, args.abbrev, args.media_id)
    paragraphs = group_into_paragraphs(lines)
    print(f"Loaded {len(lines)} lines, {len(paragraphs)} paragraphs, "
          f"{len(ts_map)} timestamps (media_id={media_id})")

    if not ts_map:
        print("Error: no timestamps found — run map_gutenberg_timestamps first",
              file=sys.stderr)
        sys.exit(1)

    all_groups = []
    for idx, para in enumerate(paragraphs):
        if len(para) == 1:
            # Single-line paragraph is trivially one sentence
            all_groups.append(para)
            if args.dry_run:
                line = para[0]
                print(f"Paragraph {idx+1} (line {line['line_in_div']}): 1 sentence (single line)")
            continue

        starts = query_ollama(args.endpoint, args.model, para)
        if starts is None:
            # Ollama failed — treat whole paragraph as one sentence
            all_groups.append(para)
            if args.dry_run:
                print(f"Paragraph {idx+1}: skipped (Ollama error), 1 group (fallback)")
            continue

        groups = build_sentence_groups_from_starts(para, starts)
        all_groups.extend(groups)

        if args.dry_run:
            first_line = para[0]["line_in_div"]
            last_line = para[-1]["line_in_div"]
            print(f"Paragraph {idx+1} (lines {first_line}-{last_line}): "
                  f"{len(groups)} sentences")
            for si, group in enumerate(groups):
                g_first = group[0]["line_in_div"]
                g_last = group[-1]["line_in_div"]
                print(f"  Sentence {si+1}: lines {g_first}-{g_last}")

        # Progress
        if not args.dry_run and (idx + 1) % 50 == 0:
            print(f"  Processed {idx+1}/{len(paragraphs)} paragraphs...")

    if not args.dry_run:
        write_sentence_times(LIT_DB, media_id, all_groups, ts_map, args.dry_run)
        total_sentences = len(all_groups)
        print(f"Done: wrote sentence times for {total_sentences} sentences "
              f"across {len(paragraphs)} paragraphs")
    else:
        print(f"\nDry run complete: {len(all_groups)} sentences detected "
              f"across {len(paragraphs)} paragraphs")
```

- [ ] **Step 2: Test dry-run with Ollama running**

```bash
python3 ~/utono/litdb/scripts/detect_sentences.py BH --dry-run 2>&1 | head -30
```

Expected: prints paragraph-by-paragraph sentence breakdown. Verify sentence counts look reasonable (most paragraphs should have 2-10 sentences).

- [ ] **Step 3: Test actual write on a small work or BH**

```bash
# Check current state
sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM line_timestamps WHERE sentence_start_time IS NOT NULL AND media_id = (SELECT media_id FROM work_media_associations WHERE work_abbrev='BH' ORDER BY priority DESC LIMIT 1)"

# Run the real write
python3 ~/utono/litdb/scripts/detect_sentences.py BH

# Verify data was written
sqlite3 ~/utono/litdb/data/lit.db "SELECT lt.sentence_start_time, lt.sentence_end_time, lm.canonical_text FROM line_timestamps lt JOIN line_mapping lm ON lt.line_mapping_id = lm.id WHERE lm.work_abbrev='BH' AND lt.sentence_start_time IS NOT NULL ORDER BY lm.line_in_div LIMIT 20"
```

Expected: rows now have `sentence_start_time` and `sentence_end_time` populated. Lines in the same sentence share the same values.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/litdb
git add scripts/detect_sentences.py
git commit -m "feat: complete detect_sentences.py with Ollama integration and DB write"
```

---

### Task 4: Rust — load sentence times from DB

**Files:**
- Modify: `~/utono/linux-lit/src/db/models.rs:35-40`
- Modify: `~/utono/linux-lit/src/db/queries.rs:80-119`

- [ ] **Step 1: Add sentence time fields to TimeRange**

In `src/db/models.rs`, add sentence time fields to `TimeRange`:

```rust
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
    pub sentence_start: Option<f64>,
    pub sentence_end: Option<f64>,
}
```

- [ ] **Step 2: Update the timestamp query in queries.rs**

In `src/db/queries.rs`, modify the SQL at line 81 to also select sentence times:

Change:
```rust
"SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id \
 FROM line_timestamps lt \
 JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
 WHERE lm.work_abbrev = ?1",
```

To:
```rust
"SELECT lt.line_mapping_id, lt.start_time, lt.end_time, lt.media_id, \
 lt.sentence_start_time, lt.sentence_end_time \
 FROM line_timestamps lt \
 JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
 WHERE lm.work_abbrev = ?1",
```

- [ ] **Step 3: Update the Timestamp struct and TimeRange construction**

Add sentence fields to the `Timestamp` struct in `models.rs`:

```rust
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
    pub sentence_start: Option<f64>,
    pub sentence_end: Option<f64>,
}
```

Update the query_map closure in `queries.rs` (~line 87):

```rust
Ok(Timestamp {
    line_id: row.get(0)?,
    start: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
    end: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
    media_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
    sentence_start: row.get::<_, Option<f64>>(4)?,
    sentence_end: row.get::<_, Option<f64>>(5)?,
})
```

Update the TimeRange construction in the ts_map loop (~line 114):

```rust
ts_map.entry(ts.line_id).or_insert(TimeRange {
    start: ts.start,
    end: ts.end,
    sentence_start: ts.sentence_start,
    sentence_end: ts.sentence_end,
});
```

- [ ] **Step 4: Build and fix any compilation errors**

```bash
cd ~/utono/linux-lit
cargo build 2>&1
```

Expected: compiles. Any places that construct `TimeRange` without the new fields will need updating — search for `TimeRange {` and add the new fields.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/db/models.rs src/db/queries.rs
git commit -m "feat: load sentence_start_time/sentence_end_time from DB"
```

---

### Task 5: Rust — build sentence_groups from DB data

**Files:**
- Modify: `~/utono/linux-lit/src/text_file_map.rs:68-193`

- [ ] **Step 1: Add a function to build sentence groups from DB sentence times**

Add before `build_sentence_groups`:

```rust
/// Build sentence groups from DB-provided sentence_start_time values.
///
/// Groups consecutive buffer lines that share the same sentence_start_time.
/// Returns None if no sentence time data exists (triggers text-heuristic fallback).
fn build_sentence_groups_from_db(
    buffer_to_work: &[Option<usize>],
    work_lines: &[Line],
) -> Option<Vec<Range<usize>>> {
    // Check if any lines have sentence time data
    let has_data = work_lines.iter().any(|l| {
        l.timestamp.as_ref().and_then(|t| t.sentence_start).is_some()
    });
    if !has_data {
        return None;
    }

    let mut groups: Vec<Range<usize>> = Vec::new();
    let mut group_start: Option<usize> = None;
    let mut current_sentence_start: Option<f64> = None;

    for (buf_idx, work_idx_opt) in buffer_to_work.iter().enumerate() {
        let sentence_start = work_idx_opt
            .and_then(|wi| work_lines[wi].timestamp.as_ref())
            .and_then(|t| t.sentence_start);

        match (sentence_start, current_sentence_start) {
            (Some(ss), Some(css)) if (ss - css).abs() < 0.001 => {
                // Same sentence — extend the group
            }
            (Some(ss), _) => {
                // New sentence — close previous group if any
                if let Some(start) = group_start {
                    groups.push(start..buf_idx);
                }
                group_start = Some(buf_idx);
                current_sentence_start = Some(ss);
            }
            (None, _) => {
                // No sentence data (blank line, unmapped line) — close group
                if let Some(start) = group_start {
                    groups.push(start..buf_idx);
                }
                group_start = None;
                current_sentence_start = None;
            }
        }
    }
    // Close trailing group
    if let Some(start) = group_start {
        groups.push(start..buffer_to_work.len());
    }

    Some(groups)
}
```

- [ ] **Step 2: Update build_line_map to prefer DB data**

In `build_line_map`, replace lines 179-184:

```rust
// Sentence groups: prefer DB-provided sentence times, fall back to text heuristics
let sentence_groups = if is_prose {
    build_sentence_groups_from_db(&buffer_to_work, work_lines)
        .unwrap_or_else(|| build_sentence_groups(file_lines))
} else {
    Vec::new()
};
```

- [ ] **Step 3: Build and verify**

```bash
cd ~/utono/linux-lit
cargo build 2>&1
```

Expected: compiles cleanly.

- [ ] **Step 4: Run existing tests**

```bash
cd ~/utono/linux-lit
cargo test sentence 2>&1
```

Expected: all existing sentence group tests still pass (they test the text-heuristic path which is unchanged).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/text_file_map.rs
git commit -m "feat: build sentence_groups from DB sentence times when available"
```

---

### Task 6: Update wizard-gutenberg SKILL.md

**Files:**
- Modify: `~/utono/litdb/.claude/skills/wizard-gutenberg/SKILL.md`

- [ ] **Step 1: Add Step 8 after Step 7**

Add a new section after Step 7 (populate spoken status):

```markdown
### Step 8: Detect sentence boundaries (optional)

Requires Ollama running with `qwen2.5:7b`. Skip if unavailable.

Check Ollama is available:
```bash
curl -s http://localhost:11434/api/tags | python3 -c "import sys,json; models=[m['name'] for m in json.load(sys.stdin)['models']]; print('qwen2.5:7b available' if any('qwen2.5:7b' in m for m in models) else 'qwen2.5:7b NOT found — run: ollama pull qwen2.5:7b')"
```

Dry run first:
```bash
python3 ~/utono/litdb/scripts/detect_sentences.py WORK_ABBREV --dry-run 2>&1 | head -40
```

Review sentence counts per paragraph — most should have 1-10 sentences. If results look reasonable, run for real:

```bash
python3 ~/utono/litdb/scripts/detect_sentences.py WORK_ABBREV
```

Verify:
```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT COUNT(*) FROM line_timestamps lt JOIN line_mapping lm ON lt.line_mapping_id = lm.id WHERE lm.work_abbrev = 'WORK_ABBREV' AND lt.sentence_start_time IS NOT NULL"
```
```

- [ ] **Step 2: Commit**

```bash
cd ~/utono/litdb
git add .claude/skills/wizard-gutenberg/SKILL.md
git commit -m "feat: add Step 8 (Ollama sentence detection) to wizard-gutenberg"
```

---

### Task 7: End-to-end verification

- [ ] **Step 1: Run detect_sentences on BH**

```bash
python3 ~/utono/litdb/scripts/detect_sentences.py BH
```

- [ ] **Step 2: Verify DB data**

```bash
sqlite3 ~/utono/litdb/data/lit.db "
SELECT lt.sentence_start_time, lt.sentence_end_time, lm.canonical_text
FROM line_timestamps lt
JOIN line_mapping lm ON lt.line_mapping_id = lm.id
WHERE lm.work_abbrev='BH' AND lm.div1=1 AND lm.div2=0
AND lt.sentence_start_time IS NOT NULL
ORDER BY lm.line_in_div
LIMIT 20
"
```

Expected: lines in the same sentence share identical `sentence_start_time` / `sentence_end_time` values.

- [ ] **Step 3: Build and run linux-lit**

```bash
cd ~/utono/linux-lit
cargo build
```

Launch the app, open BH, and verify:
- Sentences highlight as individual units (not entire paragraphs)
- `,` and `q` navigate between sentences correctly
- The long "On such an afternoon..." paragraph splits into multiple sentences

- [ ] **Step 4: Commit any remaining changes**

```bash
cd ~/utono/linux-lit
git add -A
git commit -m "feat: Ollama-powered sentence boundary detection"
```

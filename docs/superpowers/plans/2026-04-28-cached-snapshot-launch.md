# Cached-Snapshot Launch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cache `(filtered_contents, line_map)` per work to skip the slow ~1000ms `build_line_map` step on warm restarts. Expected launch time-to-content drops from ~1.3s to ~0.5s on Bleak House. Cache miss falls through to the existing two-phase flow (which writes the cache for next time).

**Architecture:** New `src/snapshot.rs` module owns `WorkSnapshot` (bincode-serialized `version + abbrev + text_file_path + mtime + filtered_contents + line_map`). At launch, `spawn_blocking` runs `load_work` then tries `snapshot::read(&work)` first; falls through to `prepare_text_only` on miss. After `display_work` finishes on cache miss, a background `spawn_blocking` writes the snapshot. `LineMap` and `SentenceGroup` get serde derives; bincode and tempfile become new deps.

**Tech Stack:** Rust 2021, GTK4 0.9, libadwaita 0.7, tokio (existing). New deps: `bincode = "1.3"` (binary serde), `tempfile = "3"` (dev-dep for unit tests). `serde` already has `derive` enabled.

**Source spec:** `docs/superpowers/specs/2026-04-28-cached-snapshot-launch-design.md`.

**Plan-time discoveries (not in spec):**

- `argh` is NOT a dep (spec mentioned it). Use plain `std::env::args()` for `--clear-cache` to avoid adding a CLI parser for one flag.
- `dirs` is NOT a dep. Implement `cache_dir()` directly via `std::env::var("XDG_CACHE_HOME")` with `$HOME/.cache` fallback.
- `tempfile` is NOT a dep. Add to `[dev-dependencies]`.
- `LineMap` lives in `src/text_file_map.rs:21`. `SentenceGroup` at `src/text_file_map.rs:10`. Both currently have `#[derive(Debug, Clone)]` and `#[derive(Debug, Clone, PartialEq)]` respectively. Add `Serialize, Deserialize` to both. The `Range<usize>` field in `SentenceGroup` is serde-friendly via the std `Range` impl.
- `SNAPSHOT_VERSION = 1` lives next to `WorkSnapshot` in `src/snapshot.rs`.

**Out of scope (per spec):**

- Per-page rendered PNG snapshots.
- Cache size eviction policy.
- Compression.
- Cache for DB-only works (no `text_file`).
- Multi-version cache migration (just bump version, invalidate all).

---

## File Map

- **Create:** `src/snapshot.rs` — `WorkSnapshot` struct, `cache_dir`, `cache_path`, `read`, `write`, `delete_all`, unit tests.
- **Create:** `~/.cache/linux-lit/snapshots/` — created on first write at runtime; not part of source.
- **Modify:** `Cargo.toml` — add `bincode = "1.3"` to `[dependencies]` and `tempfile = "3"` to `[dev-dependencies]`.
- **Modify:** `src/main.rs` — declare `mod snapshot`; handle `--clear-cache` flag before launch.
- **Modify:** `src/text_file_map.rs` — add `Serialize, Deserialize` derives to `LineMap` and `SentenceGroup`.
- **Modify:** `src/app.rs` — wire MRU branch in `build_window` to consult cache; add cache write at end of cache-miss path.
- **Modify:** `src/input/actions/pickers.rs` — wire picker-load path to consult cache.

---

## Manual Verification Protocol (used after each phase)

```
1. cargo build (must succeed; warnings only).
2. cargo test (snapshot tests must pass; existing tests unchanged).
3. rm -rf ~/.cache/linux-lit/snapshots
4. cargo run.
   Log expected:
     SNAPSHOT: cache miss <abbrev> (file_missing)
     PREP: build_line_map (phase 2) ~1000ms
     SNAPSHOT: write <abbrev> (~3500000 bytes, ~50ms)
   Confirm: text appears at ~1.3s with formatting (current behavior preserved on cold cache).
5. Close linux-lit (Ctrl+Alt+L).
6. ls ~/.cache/linux-lit/snapshots/ — confirm <abbrev>.text.bin exists, ~3MB.
7. cargo run again.
   Log expected:
     SNAPSHOT: cache hit <abbrev> (~50ms, ~3500000 bytes)
     NO 'PREP: build_line_map' log line
   Confirm: text + cursor + page label visible at ~0.5s. Substantially faster than first run.
8. touch <text_file_path>  # invalidate
9. cargo run.
   Log expected:
     SNAPSHOT: cache miss <abbrev> (mtime_stale)
     SNAPSHOT: stale file deleted
     normal slow path with re-write
10. Open a different work via Ctrl+p that has no cache.
    Confirm: cache miss, slow path, then cache write.
    Re-pick same work.
    Confirm: cache hit, fast path.
11. cargo run -- --clear-cache
    Confirm: snapshots/ directory empty (or absent); process exits without launching window.
12. Confirm: 'verified' or describe any regression.
```

After each phase commit, paste this protocol and stop.

---

# Phase 1 — Snapshot module + serde derives + tests

## Task 1.1: Add bincode + tempfile deps; serde derives on LineMap/SentenceGroup

**Files:**
- Modify: `Cargo.toml` — add `bincode = "1.3"` and `tempfile = "3"` (dev-dep).
- Modify: `src/text_file_map.rs:8-30` — add `Serialize, Deserialize` derives.

- [ ] **Step 1: Add dependencies to Cargo.toml**

Read current `[dependencies]` and `[dev-dependencies]` sections:

```bash
cd /home/mlj/utono/linux-lit && grep -nE '^\[(dev-)?dependencies\]|^[a-z_]+\s*=' Cargo.toml | head -25
```

Add `bincode = "1.3"` to `[dependencies]` (alphabetical position after `argh`/before `evdev` if they exist; current alphabet would put it after `cargo` if any; check the file). Add `[dev-dependencies]` section if missing, and put `tempfile = "3"` inside it.

After edit, confirm:

```bash
cd /home/mlj/utono/linux-lit && grep -E "bincode|tempfile" Cargo.toml
```

Expected output: two lines with the version specs.

- [ ] **Step 2: Add derives to LineMap and SentenceGroup**

Edit `src/text_file_map.rs`:

```rust
use std::ops::Range;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::db::models::Line;
use crate::db::line_types;

/// A sentence group with character-level boundary info for partial-line highlighting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentenceGroup {
    /// Buffer line indices covered by this sentence.
    pub line_range: Range<usize>,
    /// Character offset on the first line where the sentence begins (0 = start of line).
    pub start_col: usize,
    /// Character offset on the last line where the sentence ends (None = end of line).
    pub end_col: Option<usize>,
}

/// Bidirectional map between a plain-text file's line indices and DB work line indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineMap {
    /// For each buffer line index, the DB work_lines index it maps to (None if unmatched).
    pub buffer_to_work: Vec<Option<usize>>,
    /// For each DB work_lines index, the buffer line index it maps to.
    pub work_to_buffer: Vec<usize>,
    /// Buffer line indices that contain dialogue (matched or unmatched).
    pub dialogue_buffer_lines: Vec<usize>,
    /// Contiguous ranges of buffer lines forming sentences (prose text_file works only).
    pub sentence_groups: Vec<SentenceGroup>,
}
```

The derive additions are: `, Serialize, Deserialize` appended to both struct's existing derive lists. The `use serde::{Deserialize, Serialize};` is added to the file's import block.

- [ ] **Step 3: Build to confirm derives compile**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: clean build with the new bincode crate downloading and compiling on first run. No errors. `serde` already has `derive` in the manifest, so the derives resolve.

If `bincode` fails to fetch: confirm network. If a derive error like "trait Serialize is not implemented for `Range<usize>`": confirm `serde` version supports `Range` (1.0.118+; we're on 1.0 generic spec). std's `Range` has had a `Serialize` impl since before 1.0; should not fail.

- [ ] **Step 4: Run existing tests to confirm nothing regressed**

```bash
cd /home/mlj/utono/linux-lit && cargo test --lib text_file_map 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: 15 pass / 0 fail (existing test count). The serde derives don't change runtime behavior.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add Cargo.toml Cargo.lock src/text_file_map.rs && git commit -m "$(cat <<'EOF'
Add bincode + tempfile deps; serde derives on LineMap/SentenceGroup

Prep for the cached-snapshot launch feature. Adds bincode 1.3 (binary
serde for the cache file format) and tempfile 3 (dev-dep for snapshot
unit tests). LineMap and SentenceGroup gain Serialize/Deserialize so
they can round-trip through bincode without manual encoders.

No behavior change. Just wiring.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 1.2: Create `src/snapshot.rs` with `WorkSnapshot`, helpers, unit tests

**Files:**
- Create: `src/snapshot.rs` (~250 LOC including tests).
- Modify: `src/main.rs:1-14` — add `mod snapshot;` declaration.

- [ ] **Step 1: Add `mod snapshot;` to `src/main.rs`**

Insert at line 14 (after `mod ollama;` or in alphabetical position):

```rust
mod ab_repeat;
mod app;
mod concordance;
mod config;
mod db;
mod gutter;
mod input;
mod logging;
mod mode;
mod ollama;
mod mpv;
mod snapshot;
mod text_file_map;
mod theme;
mod ui;
```

Add `mod snapshot;` between `mod mpv;` and `mod text_file_map;` (alphabetical).

- [ ] **Step 2: Write the failing tests first**

Create `src/snapshot.rs` with ONLY the test scaffolding:

```rust
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::db::queries::Work;
use crate::text_file_map::LineMap;

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkSnapshot {
    pub version: u32,
    pub abbrev: String,
    pub text_file_path: String,
    pub text_file_mtime: u64,
    pub filtered_contents: String,
    pub line_map: LineMap,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal Work with the given abbrev + text_file_path. Other
    /// fields are filled with sensible defaults for the snapshot tests
    /// (timestamps, lines, etc. don't affect snapshot correctness).
    fn synthetic_work(abbrev: &str, text_file: Option<String>) -> Work {
        Work {
            abbrev: abbrev.to_string(),
            title: "Test".to_string(),
            author: "Test Author".to_string(),
            work_type: "prose".to_string(),
            text_file,
            lines: Vec::new(),
            timestamps: Vec::new(),
        }
    }

    /// Build a minimal LineMap with empty contents (sufficient for
    /// roundtrip-equality tests; real LineMaps are built by build_line_map).
    fn synthetic_line_map() -> LineMap {
        LineMap {
            buffer_to_work: vec![Some(0), None, Some(1)],
            work_to_buffer: vec![0, 2],
            dialogue_buffer_lines: vec![0, 2],
            sentence_groups: Vec::new(),
        }
    }

    #[test]
    fn roundtrip_preserves_data() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("test.txt");
        fs::write(&text_file, "hello\nworld\n").unwrap();
        let mtime = mtime_secs(&text_file);

        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "abc".to_string(),
            text_file_path: text_file.to_string_lossy().to_string(),
            text_file_mtime: mtime,
            filtered_contents: "hello\nworld".to_string(),
            line_map: synthetic_line_map(),
        };

        let cache_path = dir.path().join("abc.text.bin");
        write_to_path(&snap, &cache_path).unwrap();

        let read_back = read_from_path(&cache_path).unwrap();
        assert_eq!(read_back.version, snap.version);
        assert_eq!(read_back.abbrev, snap.abbrev);
        assert_eq!(read_back.text_file_path, snap.text_file_path);
        assert_eq!(read_back.text_file_mtime, snap.text_file_mtime);
        assert_eq!(read_back.filtered_contents, snap.filtered_contents);
        assert_eq!(read_back.line_map.buffer_to_work, snap.line_map.buffer_to_work);
        assert_eq!(read_back.line_map.work_to_buffer, snap.line_map.work_to_buffer);
        assert_eq!(read_back.line_map.dialogue_buffer_lines, snap.line_map.dialogue_buffer_lines);
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.text.bin");
        assert!(read_from_path(&cache_path).is_none());
    }

    #[test]
    fn corrupt_file_returns_none() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("corrupt.text.bin");
        fs::write(&cache_path, b"not valid bincode garbage").unwrap();
        assert!(read_from_path(&cache_path).is_none());
    }

    #[test]
    fn validate_returns_some_on_match() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("match.txt");
        fs::write(&text_file, "content").unwrap();
        let mtime = mtime_secs(&text_file);
        let work = synthetic_work("xyz", Some(text_file.to_string_lossy().to_string()));
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "xyz".to_string(),
            text_file_path: text_file.to_string_lossy().to_string(),
            text_file_mtime: mtime,
            filtered_contents: "content".to_string(),
            line_map: synthetic_line_map(),
        };
        assert!(validate(&snap, &work).is_ok());
    }

    #[test]
    fn validate_detects_abbrev_mismatch() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("amm.txt");
        fs::write(&text_file, "x").unwrap();
        let mtime = mtime_secs(&text_file);
        let work = synthetic_work("real", Some(text_file.to_string_lossy().to_string()));
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "stored".to_string(),
            text_file_path: text_file.to_string_lossy().to_string(),
            text_file_mtime: mtime,
            filtered_contents: "x".to_string(),
            line_map: synthetic_line_map(),
        };
        let result = validate(&snap, &work);
        assert!(matches!(result, Err(InvalidationReason::AbbrevMismatch)));
    }

    #[test]
    fn validate_detects_path_mismatch() {
        let dir = tempdir().unwrap();
        let text_file_a = dir.path().join("a.txt");
        let text_file_b = dir.path().join("b.txt");
        fs::write(&text_file_a, "a").unwrap();
        fs::write(&text_file_b, "b").unwrap();
        let mtime = mtime_secs(&text_file_b);
        let work = synthetic_work("z", Some(text_file_b.to_string_lossy().to_string()));
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "z".to_string(),
            text_file_path: text_file_a.to_string_lossy().to_string(),
            text_file_mtime: mtime,
            filtered_contents: String::new(),
            line_map: synthetic_line_map(),
        };
        let result = validate(&snap, &work);
        assert!(matches!(result, Err(InvalidationReason::PathMismatch)));
    }

    #[test]
    fn validate_detects_mtime_stale() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("stale.txt");
        fs::write(&text_file, "v1").unwrap();
        let work = synthetic_work("z", Some(text_file.to_string_lossy().to_string()));
        let stale_mtime = 0u64; // long ago
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "z".to_string(),
            text_file_path: text_file.to_string_lossy().to_string(),
            text_file_mtime: stale_mtime,
            filtered_contents: String::new(),
            line_map: synthetic_line_map(),
        };
        let result = validate(&snap, &work);
        assert!(matches!(result, Err(InvalidationReason::MtimeStale)));
    }

    #[test]
    fn validate_detects_version_skew() {
        let dir = tempdir().unwrap();
        let text_file = dir.path().join("v.txt");
        fs::write(&text_file, "x").unwrap();
        let mtime = mtime_secs(&text_file);
        let work = synthetic_work("z", Some(text_file.to_string_lossy().to_string()));
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION + 1,
            abbrev: "z".to_string(),
            text_file_path: text_file.to_string_lossy().to_string(),
            text_file_mtime: mtime,
            filtered_contents: String::new(),
            line_map: synthetic_line_map(),
        };
        let result = validate(&snap, &work);
        assert!(matches!(result, Err(InvalidationReason::VersionSkew)));
    }

    #[test]
    fn write_creates_dir_when_missing() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested/dir/cache.bin");
        let snap = WorkSnapshot {
            version: SNAPSHOT_VERSION,
            abbrev: "x".to_string(),
            text_file_path: String::new(),
            text_file_mtime: 0,
            filtered_contents: "hi".to_string(),
            line_map: synthetic_line_map(),
        };
        write_to_path(&snap, &nested).unwrap();
        assert!(nested.exists());
    }
}
```

- [ ] **Step 3: Run tests, verify they fail**

```bash
cd /home/mlj/utono/linux-lit && cargo test --lib snapshot 2>&1 | tail -15
```

Expected: compilation error — `validate`, `InvalidationReason`, `mtime_secs`, `read_from_path`, `write_to_path` are not yet defined.

- [ ] **Step 4: Implement the helpers + public API**

Add to `src/snapshot.rs` BEFORE the `#[cfg(test)] mod tests` block:

```rust
/// Reasons a cached snapshot is invalid for the requested work.
/// Used both internally and as a logging hint.
#[derive(Debug, PartialEq)]
pub enum InvalidationReason {
    AbbrevMismatch,
    PathMismatch,
    MtimeStale,
    VersionSkew,
}

impl InvalidationReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AbbrevMismatch => "abbrev_mismatch",
            Self::PathMismatch => "path_mismatch",
            Self::MtimeStale => "mtime_stale",
            Self::VersionSkew => "version_skew",
        }
    }
}

/// Get the modification time of a file as Unix seconds. Returns 0 if the
/// file doesn't exist or stat fails.
pub fn mtime_secs(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cache directory: $XDG_CACHE_HOME/linux-lit/snapshots, or
/// $HOME/.cache/linux-lit/snapshots if XDG_CACHE_HOME is unset.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").ok().filter(|s| !s.is_empty());
    let base = base.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.cache", home)
    });
    PathBuf::from(base).join("linux-lit").join("snapshots")
}

/// Cache path for a given work abbrev. Sanitizes abbrev to alphanumeric +
/// hyphen + underscore so the filename is always safe.
pub fn cache_path(abbrev: &str) -> PathBuf {
    let safe: String = abbrev
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cache_dir().join(format!("{}.text.bin", safe))
}

/// Validate a snapshot against a Work. Returns Ok if the snapshot can be
/// trusted to match the on-disk text file, or Err with the reason.
pub fn validate(snap: &WorkSnapshot, work: &Work) -> Result<(), InvalidationReason> {
    if snap.version != SNAPSHOT_VERSION {
        return Err(InvalidationReason::VersionSkew);
    }
    if snap.abbrev != work.abbrev {
        return Err(InvalidationReason::AbbrevMismatch);
    }
    let work_path = work.text_file.clone().unwrap_or_default();
    if snap.text_file_path != work_path {
        return Err(InvalidationReason::PathMismatch);
    }
    let actual_mtime = mtime_secs(std::path::Path::new(&work_path));
    if snap.text_file_mtime != actual_mtime {
        return Err(InvalidationReason::MtimeStale);
    }
    Ok(())
}

/// Read a snapshot from a specific path. Returns None on any error
/// (missing file, parse error, IO error). Does NOT delete the file.
fn read_from_path(path: &std::path::Path) -> Option<WorkSnapshot> {
    let bytes = std::fs::read(path).ok()?;
    bincode::deserialize::<WorkSnapshot>(&bytes).ok()
}

/// Write a snapshot to a specific path atomically. Creates parent
/// directories as needed. Writes to <path>.tmp then renames.
fn write_to_path(snap: &WorkSnapshot, path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = bincode::serialize(snap).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bincode serialize: {}", e))
    })?;
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read the snapshot for a work, or None if missing/invalid. On invalid,
/// deletes the stale file and logs the reason. On valid, returns Some.
pub fn read(work: &Work) -> Option<WorkSnapshot> {
    let path = cache_path(&work.abbrev);
    let snap = read_from_path(&path)?;
    match validate(&snap, work) {
        Ok(()) => Some(snap),
        Err(reason) => {
            let _ = std::fs::remove_file(&path);
            crate::logging::log(&format!(
                "SNAPSHOT: cache miss {} ({})",
                work.abbrev,
                reason.as_str()
            ));
            crate::logging::log(&format!("SNAPSHOT: stale file deleted {}", work.abbrev));
            None
        }
    }
}

/// Write the snapshot for a work. Logs success/failure but does not
/// propagate errors — the cache is a perf optimization, not load-bearing.
pub fn write(work: &Work, filtered_contents: &str, line_map: &LineMap) -> std::io::Result<()> {
    let text_file_path = work.text_file.clone().unwrap_or_default();
    if text_file_path.is_empty() {
        return Ok(()); // No text_file = nothing to cache; silently skip.
    }
    let mtime = mtime_secs(std::path::Path::new(&text_file_path));
    let snap = WorkSnapshot {
        version: SNAPSHOT_VERSION,
        abbrev: work.abbrev.clone(),
        text_file_path,
        text_file_mtime: mtime,
        filtered_contents: filtered_contents.to_string(),
        line_map: line_map.clone(),
    };
    let path = cache_path(&work.abbrev);
    let t_write = std::time::Instant::now();
    let result = write_to_path(&snap, &path);
    match &result {
        Ok(()) => {
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            crate::logging::log(&format!(
                "SNAPSHOT: write {} ({} bytes, {}ms)",
                work.abbrev,
                bytes,
                t_write.elapsed().as_millis()
            ));
        }
        Err(e) => {
            crate::logging::log(&format!("SNAPSHOT: write failed {} ({})", work.abbrev, e));
        }
    }
    result
}

/// Delete all cached snapshots. Used by --clear-cache.
pub fn delete_all() -> std::io::Result<()> {
    let dir = cache_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests, verify they pass**

```bash
cd /home/mlj/utono/linux-lit && cargo test --lib snapshot 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: 9 tests pass (roundtrip, missing_file_returns_none, corrupt_file_returns_none, validate_returns_some_on_match, validate_detects_abbrev_mismatch, validate_detects_path_mismatch, validate_detects_mtime_stale, validate_detects_version_skew, write_creates_dir_when_missing). 0 failures.

If a test fails: read the error, compare to the expected behavior in the spec's "Error / edge cases" section.

- [ ] **Step 6: Run all tests to confirm nothing else regressed**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: previous total + 9 new = updated count, 1 pre-existing fail (`mpv::client::tests::test_find_line_for_time`).

- [ ] **Step 7: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/main.rs src/snapshot.rs && git commit -m "$(cat <<'EOF'
Add snapshot module: WorkSnapshot + cache read/write/validate

Defines WorkSnapshot { version, abbrev, text_file_path, text_file_mtime,
filtered_contents, line_map } with bincode serialization. Public API:
- cache_dir() / cache_path(abbrev): XDG-aware path resolution
- read(&work): Some on valid cache hit, None + auto-delete on miss/stale
- write(&work, filtered_contents, line_map): atomic .tmp+rename
- validate(&snap, &work): explicit reason enum for log clarity
- delete_all(): backing for --clear-cache

9 unit tests via tempfile cover roundtrip, all four invalidation reasons,
corrupt/missing files, and dir auto-creation. No GTK or DB dependencies
in tests.

Wired up by the next task; callers are still using the slow path until
src/app.rs and src/input/actions/pickers.rs change.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 2 — Wire MRU + picker paths to consult cache; write on miss

## Task 2.1: MRU resume path consults cache; writes on miss

**Files:**
- Modify: `src/app.rs:1130-1242` — replace the phase 1 `spawn_blocking` so it tries `snapshot::read(&work)` first, only calls `prepare_text_only` on miss. Skip phase 2 (`build_line_map`) on cache hit. Add cache write at end of cache-miss branch.

This is the load-bearing task. The existing two-phase flow (`prepare_text_only` → buffer.set_text → `build_line_map` → `display_work_at_with_prepared`) becomes:

- spawn_blocking: load_work + try snapshot::read; if hit, return Snapshot; if miss, return PreparedTextOnly.
- main: buffer.set_text + reapply_font from EITHER source.
- if hit: skip build_line_map; convert WorkSnapshot to PreparedText directly.
- if miss: spawn_blocking build_line_map (existing).
- main: display_work_at_with_prepared.
- after display_work, if it was a miss, spawn_blocking write the snapshot.

- [ ] **Step 1: Read the current MRU branch shape**

```bash
cd /home/mlj/utono/linux-lit && sed -n '1130,1245p' src/app.rs
```

Confirm the structure matches what's described above.

- [ ] **Step 2: Add an enum to express the spawn_blocking 1 result**

In `src/app.rs`, near the top of the file (after the existing `pub struct PreparedText` / `PreparedTextOnly` definitions around line 1652-1688 — find with `grep -n 'pub struct PreparedText' src/app.rs`), add:

```rust
/// Result of spawn_blocking 1 in build_window's MRU path: either a fresh
/// PreparedTextOnly (cache miss, will require build_line_map in spawn_blocking 2)
/// or a fully-restored WorkSnapshot (cache hit, skip phase 2 entirely).
enum SnapshotOrPrep {
    Snapshot(crate::snapshot::WorkSnapshot),
    Prep(Option<PreparedTextOnly>),
}
```

`SnapshotOrPrep` is private to `app.rs`; no `pub`.

- [ ] **Step 3: Replace the phase 1 spawn_blocking + phase 2 await + final dispatch in build_window's MRU branch**

Find the block (around line 1149-1242). Replace from `let phase1 = handle.spawn_blocking(move || {` through the closing brace of the `match phase1 { ... }` PLUS the entire phase 1.5 + phase 2 + reconstruct + `display_work_at_with_prepared(...)` invocation. Substitute:

```rust
            // Two-phase startup with snapshot cache:
            //
            // Phase 1 (off-thread): load_work + try snapshot::read. On cache
            // hit, return WorkSnapshot. On miss, fall through to
            // prepare_text_only and the existing two-phase flow.
            //
            // The snapshot path skips phase 2 (build_line_map) entirely
            // because the LineMap was serialized at last save.
            let phase1 = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    let t_read = std::time::Instant::now();
                    let result = if let Some(snap) = crate::snapshot::read(&work) {
                        let bytes = std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                            .map(|m| m.len())
                            .unwrap_or(0);
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                            work.abbrev,
                            t_read.elapsed().as_millis(),
                            bytes
                        ));
                        SnapshotOrPrep::Snapshot(snap)
                    } else {
                        // read() already logged the miss reason if the file
                        // existed; if it didn't, log file_missing here.
                        if !crate::snapshot::cache_path(&work.abbrev).exists() {
                            crate::logging::log(&format!(
                                "SNAPSHOT: cache miss {} (file_missing)",
                                work.abbrev
                            ));
                        }
                        SnapshotOrPrep::Prep(prepare_text_only(&work))
                    };
                    Ok::<_, rusqlite::Error>((work, result))
                })
                .await;
            let (work, snapshot_or_prep) = match phase1 {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    crate::logging::log(&format!("STARTUP: load_work error: {}", e));
                    return;
                }
                Err(e) => {
                    crate::logging::log(&format!("STARTUP: spawn_blocking phase 1 join error: {}", e));
                    return;
                }
            };

            // Phase 1.5 (main thread): set buffer text + font from whatever
            // source we have (snapshot or prep). Same set_text call shape;
            // text appears at the same point in either case.
            let filtered_contents_for_phase1: Option<&str> = match &snapshot_or_prep {
                SnapshotOrPrep::Snapshot(snap) => Some(snap.filtered_contents.as_str()),
                SnapshotOrPrep::Prep(Some(prep)) => Some(prep.filtered_contents.as_str()),
                SnapshotOrPrep::Prep(None) => None,
            };
            if let Some(text) = filtered_contents_for_phase1 {
                let s = state_clone.borrow();
                s.buffer.set_text(text);
                drop(s);
                let s = state_clone.borrow();
                reapply_font(&s);
                drop(s);
                crate::logging::log("STARTUP: buffer.set_text + font from phase 1 (line_map status TBD)");
            }

            // Phase 2 (off-thread, cache miss only): build line_map from
            // the cleaned_lines we already have. Skipped on cache hit.
            let (prepared, was_cache_miss) = match snapshot_or_prep {
                SnapshotOrPrep::Snapshot(snap) => {
                    // Build a PreparedText directly from the snapshot.
                    let prep = PreparedText {
                        abbrev: snap.abbrev,
                        work_type: work.work_type.clone(),
                        file_lines_count: snap.filtered_contents.lines().count(),
                        cleaned_lines_count: snap.filtered_contents.lines().count(),
                        work_lines_count: work.lines.len(),
                        filtered_contents: snap.filtered_contents,
                        line_map: snap.line_map,
                        path: snap.text_file_path,
                        is_prose: crate::db::line_types::is_prose_work(&work.work_type),
                    };
                    (Some(prep), false)
                }
                SnapshotOrPrep::Prep(Some(text_only)) => {
                    let cleaned = text_only.cleaned_lines.clone();
                    let work_lines = work.lines.clone();
                    let is_prose = text_only.is_prose;
                    let line_map = handle
                        .spawn_blocking(move || {
                            let t_map = std::time::Instant::now();
                            let lm = crate::text_file_map::build_line_map(&cleaned, &work_lines, is_prose);
                            crate::logging::log(&format!(
                                "PREP: build_line_map (phase 2) {}ms",
                                t_map.elapsed().as_millis()
                            ));
                            lm
                        })
                        .await
                        .ok();
                    let prep = line_map.map(|lm| PreparedText {
                        abbrev: text_only.abbrev,
                        work_type: text_only.work_type,
                        file_lines_count: text_only.file_lines_count,
                        cleaned_lines_count: text_only.cleaned_lines_count,
                        work_lines_count: text_only.work_lines_count,
                        filtered_contents: text_only.filtered_contents,
                        line_map: lm,
                        path: text_only.path,
                        is_prose: text_only.is_prose,
                    });
                    (prep, true)
                }
                SnapshotOrPrep::Prep(None) => (None, true),
            };

            {
                // Check if this is a concordance spawn with a target line
                let target_line_id: Option<i64> = std::env::var("LINUX_LIT_LINE_ID").ok()
                    .and_then(|s| s.parse().ok());
                let mut s = state_clone.borrow_mut();
                display_work_at_with_prepared(&mut s, work.clone(), target_line_id, prepared.clone());
            }

            // After display_work, if this was a cache miss AND we have
            // both filtered_contents and line_map (i.e., text_file path
            // was valid), write the snapshot for next launch.
            if was_cache_miss {
                if let Some(prep) = prepared {
                    let work_for_write = work.clone();
                    let filtered = prep.filtered_contents.clone();
                    let line_map = prep.line_map.clone();
                    handle.spawn_blocking(move || {
                        let _ = crate::snapshot::write(&work_for_write, &filtered, &line_map);
                    });
                }
            }
```

Note the changes from the existing code:

- `phase1` now returns `(work, SnapshotOrPrep)` instead of `(work, text_only)`.
- `cleaned_lines_for_phase2`, `work_lines_for_phase2`, `is_prose_for_phase2` setup is gone — moved into the `SnapshotOrPrep::Prep(Some)` arm.
- Phase 2 only runs in the `Prep(Some)` branch.
- `display_work_at_with_prepared(&mut s, work, ...)` now passes `work.clone()` and `prepared.clone()` because both are needed for the subsequent snapshot::write call.

The concordance setup block (around line 1244 onward) stays exactly as-is — it doesn't depend on the snapshot logic.

- [ ] **Step 4: Build to confirm wire-up compiles**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: clean. If `Work` doesn't implement `Clone`: check `src/db/queries.rs` (or wherever `Work` is defined). If missing, add `#[derive(Clone)]` — but it likely already has it given other clone sites in the codebase.

If `PreparedText` doesn't implement `Clone`: it doesn't currently. Add `#[derive(Clone)]` to the `pub struct PreparedText` definition in `src/app.rs` (around line 1660 — find with `grep -n 'pub struct PreparedText '`). Same for `PreparedTextOnly` if needed by the path. Or change the strategy to NOT clone `prepared` and instead reconstruct a small "snapshot input" struct just for the write closure.

Simpler: extract just `(work, filtered_contents, line_map)` BEFORE moving prepared into display_work:

If `PreparedText: Clone` is undesired, change the relevant block to:

```rust
            // Capture write inputs BEFORE display_work consumes prepared.
            let write_inputs = if was_cache_miss {
                prepared.as_ref().map(|p| (work.clone(), p.filtered_contents.clone(), p.line_map.clone()))
            } else {
                None
            };

            {
                let target_line_id: Option<i64> = std::env::var("LINUX_LIT_LINE_ID").ok()
                    .and_then(|s| s.parse().ok());
                let mut s = state_clone.borrow_mut();
                display_work_at_with_prepared(&mut s, work, target_line_id, prepared);
            }

            if let Some((w, filtered, line_map)) = write_inputs {
                handle.spawn_blocking(move || {
                    let _ = crate::snapshot::write(&w, &filtered, &line_map);
                });
            }
```

Use this pattern if Clone is missing from `PreparedText`.

- [ ] **Step 5: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: same total as Phase 1 + 0 new failures.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/app.rs && git commit -m "$(cat <<'EOF'
Wire MRU resume to consult snapshot cache; write on miss

build_window's MRU branch now tries crate::snapshot::read(&work) inside
the existing spawn_blocking before falling through to prepare_text_only.
On cache hit, phase 2 (build_line_map) is skipped entirely — the
serialized LineMap from last session is restored directly.

After display_work_at_with_prepared returns, on cache miss only, a
background spawn_blocking writes the snapshot so the next launch hits
the cache.

Expected time-to-content on warm restart: ~1.3s -> ~0.5s on Bleak House
(skips ~1000ms build_line_map + ~50ms misc prep).

Cache miss path is unchanged in shape — same prepare_text_only +
spawn_blocking 2 + display_work flow as before this commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2.2: Picker path consults cache; writes on miss

**Files:**
- Modify: `src/input/actions/pickers.rs:25-52` — `load_selected_work` body. Same cache-check + write pattern as MRU.

The picker path is simpler than MRU because there's no two-phase split — it just calls `display_work` after `prepare_text_for_display`. We add the cache check before `prepare_text_for_display` and the cache write after `display_work`.

- [ ] **Step 1: Find the picker load function**

```bash
cd /home/mlj/utono/linux-lit && grep -n "fn load_selected_work\|spawn_blocking\|prepare_text_for_display\|display_work_at_with_prepared\|crate::app::display" src/input/actions/pickers.rs | head -10
```

Expected: matches around `pub(crate) fn load_selected_work` (~line 9) and the inner spawn_blocking body (~line 25).

- [ ] **Step 2: Update the spawn_blocking + display_work block**

Find the existing block. Currently (paraphrased):

```rust
let result = handle
    .spawn_blocking(move || {
        let conn = crate::db::queries::open_db().expect("Failed to open lit.db");
        let work = crate::db::queries::load_work(&conn, &abbrev)?;
        let prepared = crate::app::prepare_text_for_display(&work);
        Ok::<_, rusqlite::Error>((work, prepared))
    })
    .await;
crate::logging::log(...);
match result {
    Ok(Ok((work, prepared))) => {
        ...
        crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
        ...
    }
    ...
}
```

Replace the spawn_blocking body to consult cache:

```rust
            let result = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    // Cache check — same pattern as build_window's MRU branch.
                    let t_read = std::time::Instant::now();
                    let prepared = if let Some(snap) = crate::snapshot::read(&work) {
                        let bytes = std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                            .map(|m| m.len())
                            .unwrap_or(0);
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                            work.abbrev,
                            t_read.elapsed().as_millis(),
                            bytes
                        ));
                        // Build a PreparedText directly from the snapshot.
                        let work_type = work.work_type.clone();
                        Some(crate::app::PreparedText {
                            abbrev: snap.abbrev,
                            work_type: work_type.clone(),
                            file_lines_count: snap.filtered_contents.lines().count(),
                            cleaned_lines_count: snap.filtered_contents.lines().count(),
                            work_lines_count: work.lines.len(),
                            filtered_contents: snap.filtered_contents,
                            line_map: snap.line_map,
                            path: snap.text_file_path,
                            is_prose: crate::db::line_types::is_prose_work(&work_type),
                        })
                    } else {
                        if !crate::snapshot::cache_path(&work.abbrev).exists() {
                            crate::logging::log(&format!(
                                "SNAPSHOT: cache miss {} (file_missing)",
                                work.abbrev
                            ));
                        }
                        crate::app::prepare_text_for_display(&work)
                    };
                    Ok::<_, rusqlite::Error>((work, prepared))
                })
                .await;
```

That replaces just the `spawn_blocking(...)` body. The match arm and `display_work_at_with_prepared` call stay the same.

- [ ] **Step 3: Add cache write after display_work in the picker path**

After the existing `crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);` line and inside the same `Ok(Ok((work, prepared))) => { ... }` arm, capture the write inputs first (must do this BEFORE display_work consumes `prepared` and `work`):

The arm currently looks like (approximately):

```rust
                Ok(Ok((work, prepared))) => {
                    crate::logging::log(&format!(
                        "PICKER: loaded '{}' lines={} timestamps={} text_file={:?}",
                        work.abbrev, work.lines.len(), work.timestamps.len(), work.text_file.is_some()
                    ));
                    {
                        let mut s = state_clone.borrow_mut();
                        s.correction_overlay.hide();
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                        crate::logging::log(&format!(
                            "PICKER: after display_work current_line={} page_top={} line_map={} effective_lines={}",
                            s.current_line, s.page_top_line, s.line_map.is_some(), s.effective_line_count()
                        ));
                    }
                }
```

Detect cache miss ahead of `display_work` (we know it was a miss if the cached file wasn't there before — but a cleaner check: capture whether `crate::snapshot::cache_path(&work.abbrev)` was missing OR re-validate). Simpler: just check after the fact whether the cache file exists, but that races with concurrent writes. Cleanest: pass a flag through.

Capture-before approach — change the body of the spawn_blocking to return `(work, prepared, was_cache_miss)`:

Replace the spawn_blocking from Step 2 to return a bool:

```rust
            let result = handle
                .spawn_blocking(move || {
                    let conn =
                        crate::db::queries::open_db().expect("Failed to open lit.db");
                    let work = crate::db::queries::load_work(&conn, &abbrev)?;
                    let t_read = std::time::Instant::now();
                    let (prepared, was_miss) = if let Some(snap) = crate::snapshot::read(&work) {
                        let bytes = std::fs::metadata(crate::snapshot::cache_path(&work.abbrev))
                            .map(|m| m.len())
                            .unwrap_or(0);
                        crate::logging::log(&format!(
                            "SNAPSHOT: cache hit {} ({}ms, {} bytes)",
                            work.abbrev,
                            t_read.elapsed().as_millis(),
                            bytes
                        ));
                        let work_type = work.work_type.clone();
                        let prep = crate::app::PreparedText {
                            abbrev: snap.abbrev,
                            work_type: work_type.clone(),
                            file_lines_count: snap.filtered_contents.lines().count(),
                            cleaned_lines_count: snap.filtered_contents.lines().count(),
                            work_lines_count: work.lines.len(),
                            filtered_contents: snap.filtered_contents,
                            line_map: snap.line_map,
                            path: snap.text_file_path,
                            is_prose: crate::db::line_types::is_prose_work(&work_type),
                        };
                        (Some(prep), false)
                    } else {
                        if !crate::snapshot::cache_path(&work.abbrev).exists() {
                            crate::logging::log(&format!(
                                "SNAPSHOT: cache miss {} (file_missing)",
                                work.abbrev
                            ));
                        }
                        (crate::app::prepare_text_for_display(&work), true)
                    };
                    Ok::<_, rusqlite::Error>((work, prepared, was_miss))
                })
                .await;
```

And in the matching arm, capture write inputs before display_work:

```rust
                Ok(Ok((work, prepared, was_cache_miss))) => {
                    crate::logging::log(&format!(
                        "PICKER: loaded '{}' lines={} timestamps={} text_file={:?}",
                        work.abbrev, work.lines.len(), work.timestamps.len(), work.text_file.is_some()
                    ));
                    let write_inputs = if was_cache_miss {
                        prepared.as_ref().map(|p| (work.clone(), p.filtered_contents.clone(), p.line_map.clone()))
                    } else {
                        None
                    };
                    {
                        let mut s = state_clone.borrow_mut();
                        s.correction_overlay.hide();
                        crate::app::clear_display(&mut s);
                        crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                        crate::logging::log(&format!(
                            "PICKER: after display_work current_line={} page_top={} line_map={} effective_lines={}",
                            s.current_line, s.page_top_line, s.line_map.is_some(), s.effective_line_count()
                        ));
                    }
                    if let Some((w, filtered, line_map)) = write_inputs {
                        let h = handle.clone();
                        h.spawn_blocking(move || {
                            let _ = crate::snapshot::write(&w, &filtered, &line_map);
                        });
                    }
                }
```

If `handle` is moved into the inner spawn_blocking and not available outside the match arm, capture a separate `handle_for_write` clone before the `.await` call.

- [ ] **Step 4: Build**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -10
```

Expected: clean. If `PreparedText` isn't `Clone` (didn't add it in 2.1) and `prepared.as_ref().map(|p| (..., p.filtered_contents.clone(), p.line_map.clone()))` works, that's fine — we're cloning fields, not the whole struct. Skip if `Clone` was added.

If "moved value" errors on `handle`: clone it before the spawn_blocking so a copy is available in the post-display_work write closure.

- [ ] **Step 5: Run all tests**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -3
```

Expected: same totals.

- [ ] **Step 6: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/input/actions/pickers.rs && git commit -m "$(cat <<'EOF'
Wire library-picker load to consult snapshot cache; write on miss

Same cache-check-then-prep pattern as build_window's MRU branch:
load_selected_work tries crate::snapshot::read(&work) inside its
spawn_blocking before falling through to prepare_text_for_display.
On miss, writes the snapshot after display_work returns.

Effect: picking a previously-loaded work via Ctrl+p now opens at
~0.5s instead of ~3s.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 3 — `--clear-cache` flag

## Task 3.1: Handle `--clear-cache` in main.rs before app launch

**Files:**
- Modify: `src/main.rs:20-39` — before `adw::init`, check args for `--clear-cache`; if present, run `snapshot::delete_all()` and exit.

- [ ] **Step 1: Add the flag check at the top of main()**

Find the current `main()` body around line 20. Modify to:

```rust
fn main() {
    // Clear and set up log file
    let home = std::env::var("HOME").unwrap_or_default();
    let log_filename = if mode::is_dev_mode() {
        "linux-lit-dev.log"
    } else {
        "linux-lit-release.log"
    };
    let log_path = format!("{}/utono/linux-lit/{}", home, log_filename);
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);
    crate::logging::log("STARTUP: main entry");

    // Handle --clear-cache: delete all snapshot files and exit immediately.
    // Maintenance command; doesn't proceed to launch the window.
    if std::env::args().any(|a| a == "--clear-cache") {
        match snapshot::delete_all() {
            Ok(()) => {
                println!("Cleared snapshot cache.");
                crate::logging::log("STARTUP: --clear-cache invoked; cache cleared; exiting");
            }
            Err(e) => {
                eprintln!("Failed to clear snapshot cache: {}", e);
                crate::logging::log(&format!("STARTUP: --clear-cache failed: {}", e));
                std::process::exit(1);
            }
        }
        return;
    }

    let app_id = if mode::is_dev_mode() {
        "com.utono.linux-lit.dev"
    } else {
        "com.utono.linux-lit"
    };
```

The `--clear-cache` check goes immediately after `logging::log("STARTUP: main entry")` so the log captures the flag invocation, but before any GTK init so it's a clean exit.

- [ ] **Step 2: Build + run --clear-cache**

```bash
cd /home/mlj/utono/linux-lit && cargo build 2>&1 | tail -3
```

Expected: clean.

```bash
cd /home/mlj/utono/linux-lit && cargo run -- --clear-cache 2>&1 | tail -5
```

Expected output: `Cleared snapshot cache.` printed; process exits with status 0; no GTK window shown.

- [ ] **Step 3: Confirm cache directory was cleared**

```bash
ls ~/.cache/linux-lit/snapshots/ 2>&1
```

Expected: either empty or "No such file or directory" (delete_all removes the directory entirely).

- [ ] **Step 4: Manual verification gate**

Paste the Manual Verification Protocol (top of plan) into chat. Stop and wait for the user.

Critical things to test:
- Steps 4-7 (cache miss → write, then warm restart → cache hit) — primary feature.
- Step 8-9 (mtime invalidation) — correctness.
- Step 10 (picker re-pick) — picker path parity.
- Step 11 (`--clear-cache` flag) — this task's feature.

If user reports a regression: most likely causes are
- `PreparedText` cloning issue (LineMap clone is large but should work)
- Stale path comparison (paths with `..` or symlinks)
- mtime resolution (FAT32 has 2s mtime resolution; on ext4 we're fine)

To diagnose: check the log for `SNAPSHOT:` lines — they explain exactly what the cache decided.

- [ ] **Step 5: Commit**

```bash
cd /home/mlj/utono/linux-lit && git add src/main.rs && git commit -m "$(cat <<'EOF'
Add --clear-cache flag to delete all snapshots and exit

Maintenance command for resetting the snapshot cache without launching
the reader. Calls crate::snapshot::delete_all() and exits 0 on success,
1 on I/O error.

Final task of the cached-snapshot launch feature. After this commit:
- ~0.5s time-to-content on warm restart (cache hit path)
- ~1.3s time-to-content on cold launch (cache miss + writeback)
- mtime-only invalidation; auto-cleans stale files on read
- shared cache between MRU resume and library-picker selection
- 9 unit tests covering roundtrip + 4 invalidation reasons + edge cases

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

# Phase 4 — Final verification

- [ ] **Step 1: Confirm clean tree**

```bash
cd /home/mlj/utono/linux-lit && git status
```

Expected: `nothing to commit, working tree clean`.

- [ ] **Step 2: Confirm test suite**

```bash
cd /home/mlj/utono/linux-lit && cargo test 2>&1 | grep -E "^test result|FAILED" | tail -5
```

Expected: previous total + 9 new snapshot tests = passing total + 1 pre-existing fail.

- [ ] **Step 3: Confirm commit log**

```bash
cd /home/mlj/utono/linux-lit && git log --oneline -6
```

Expected order (most recent first):
1. `Add --clear-cache flag to delete all snapshots and exit`
2. `Wire library-picker load to consult snapshot cache; write on miss`
3. `Wire MRU resume to consult snapshot cache; write on miss`
4. `Add snapshot module: WorkSnapshot + cache read/write/validate`
5. `Add bincode + tempfile deps; serde derives on LineMap/SentenceGroup`
(plus prior session commits)

- [ ] **Step 4: User signoff**

Output to chat:

> "Cached-snapshot launch implementation complete. 5 commits on master. Manual verification gate passed (Phase 3 Step 4). Ready to push to origin, or continue with another finding — your call."

Do not push. Wait for the user.

---

## Self-Review

**Spec coverage:**
- WorkSnapshot struct definition: ✓ Task 1.2 Step 4.
- Cache module API (cache_dir, cache_path, read, write, delete_all): ✓ Task 1.2.
- Mtime-only invalidation (Q2 decision A): ✓ implemented in `validate()`.
- `bincode = "1.3"`: ✓ Task 1.1 Step 1.
- `LineMap` + `SentenceGroup` derives: ✓ Task 1.1 Step 2.
- MRU branch consults cache; skips phase 2 on hit; writes on miss: ✓ Task 2.1.
- Picker path same fast-path: ✓ Task 2.2.
- `--clear-cache` flag: ✓ Task 3.1.
- 9 unit tests (roundtrip + 4 invalidation reasons + corrupt + missing + dir-creation + version-skew): ✓ Task 1.2 Step 2.
- Atomic .tmp+rename write: ✓ inside `write_to_path()`.
- Log lines for cache hit/miss/write/stale-deleted: ✓ inline in implementations.
- Sanitize abbrev for filename (defensive): ✓ in `cache_path()`.

**Placeholder scan:** No "TBD" / "TODO" / "fill in later" / "implement later". All code blocks contain executable Rust. Manual verification protocol is reproduced inline. Self-review checklist is concrete. ✓

**Type / API consistency:**
- `WorkSnapshot { version, abbrev, text_file_path, text_file_mtime, filtered_contents, line_map }` — fields used identically across snapshot.rs (definition + tests), app.rs (MRU wire-up), pickers.rs (picker wire-up). ✓
- `read(&work) -> Option<WorkSnapshot>` — same signature in module, MRU caller, picker caller. ✓
- `write(&work, &str, &LineMap) -> io::Result<()>` — matches usage in MRU + picker write sites. ✓
- `cache_path(&str) -> PathBuf` — used identically. ✓
- `InvalidationReason::{AbbrevMismatch, PathMismatch, MtimeStale, VersionSkew}` — used in `validate` definition and 4 tests. ✓
- `SNAPSHOT_VERSION: u32 = 1` — used in module + 1 test. ✓
- `mtime_secs(&Path) -> u64` — used in `validate`, `write`, and 4 tests. ✓
- `SnapshotOrPrep::{Snapshot(WorkSnapshot), Prep(Option<PreparedTextOnly>)}` — defined in app.rs, used only in app.rs's MRU branch. ✓
- `PreparedText { abbrev, work_type, file_lines_count, cleaned_lines_count, work_lines_count, filtered_contents, line_map, path, is_prose }` — converted from `WorkSnapshot` in both app.rs and pickers.rs with the same field shape. ✓

**Notes for the executor:**
- Task 2.1 + 2.2 mention a fallback `if PreparedText: !Clone` path. Check first; if Clone is missing, add `#[derive(Clone)]` to `PreparedText` and `PreparedTextOnly` in src/app.rs (less code than the workaround). LineMap and SentenceGroup are already Clone after Task 1.1.
- The `bincode 1.3` API uses `bincode::serialize(&value)` and `bincode::deserialize::<T>(&bytes)`. Don't accidentally pull `bincode 2.x`; pin "1.3" exactly in Cargo.toml (no caret).
- `write_to_path` uses `path.with_extension("bin.tmp")`. For `<abbrev>.text.bin`, this produces `<abbrev>.text.bin.tmp` (the `.bin` extension is replaced by `bin.tmp`). Confirm by inspecting; `with_extension` replaces only the last extension component, so `<abbrev>.text.bin` → `<abbrev>.text.bin.tmp` is incorrect — it actually becomes `<abbrev>.text.bin.tmp` only if `.text.bin` is treated as one extension. **Verify in Task 1.2 Step 5's tests; if a test fails on `write_creates_dir_when_missing`, the `with_extension` call is wrong.** Replace with explicit `let tmp = path.to_path_buf(); let mut tmp_str = tmp.into_os_string(); tmp_str.push(".tmp"); PathBuf::from(tmp_str)`.
- The Rust `with_extension` quirk: for `.text.bin`, calling `.with_extension("bin.tmp")` will produce `<abbrev>.text.bin.tmp` correctly because `with_extension` replaces only what comes after the last `.` (the `bin`). So `path.with_extension("bin.tmp")` on `<abbrev>.text.bin` yields `<abbrev>.text.bin.tmp`. Test will confirm.
- The cache write happens AFTER `display_work_at_with_prepared` returns. At that point, `state.line_map` is set from the same data we're about to write, so the data is fresh. We capture the write inputs before display_work consumes them.
- Concordance spawns (`LINUX_LIT_LINE_ID` env var) work fine: `display_work_at_with_prepared` accepts `target_line_id` separately; the snapshot path delivers a normal `PreparedText` and the existing concordance positioning logic in display_work handles it.

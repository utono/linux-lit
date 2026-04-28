# Cached-Snapshot Launch Design

**Status:** Approved (brainstorm 2026-04-28).
**Source memory:** `~/.claude/projects/-home-mlj-utono-linux-lit/memory/project_cached_snapshot_launch.md`
**Out of scope:** Per-page rendered PNG/GdkTexture snapshots (was C in Q1; rejected as over-engineered). Pixel-perfect splash. Multi-version cache support beyond bincode-derived skipping.

---

## Problem

Linux-lit's two-phase launch shows text at ~1.3s on Bleak House (39482 buffer lines). Phase 1 reads + cleans the text file; phase 2 builds the line_map. Both run off the GTK main thread but the result still takes ~1.3-3s before the user sees content.

Most reference readers hide content behind a spinner until ready (foliate, openreader). One pattern none implement, documented in iOS UIKit reader apps: cache the parsed display state on exit, paint it at first launch, parse fresh in background. Skips the slow parse entirely on warm restarts.

Linux-lit's bottleneck is `build_line_map` (~1000ms on Bleak House — 71k `normalize()` calls). If we serialize the result of build_line_map to disk on first parse, subsequent launches read it from disk (~50ms) and skip the rebuild.

---

## Reference shape

iOS book readers serialize layout / position state to disk on exit, restore on launch. None of linux-lit's reference codebases do this:

- `bk` is synchronous, parses on every launch (TUI, fast enough that no cache is needed).
- `lue` parses on every launch (TUI).
- `foliate` hides content behind an overlay until WebKit finishes parsing the EPUB; no cache.
- `openreader` shows a spinner until IndexedDB resolves; no cache for parsed layout.

Linux-lit's snapshot pattern is therefore borrowed from outside the reference set. The justification is direct: build_line_map's output is deterministic from `(text_file_contents, work.lines)` — caching it is correct and the cache-miss path is the existing slow path.

---

## Architecture

### Cache layout

One file per work at `~/.cache/linux-lit/snapshots/<abbrev>.text.bin`. The cache is a binary file containing a single `WorkSnapshot` value serialized via bincode.

```rust
#[derive(serde::Serialize, serde::Deserialize)]
pub struct WorkSnapshot {
    pub version: u32,
    pub abbrev: String,
    pub text_file_path: String,
    pub text_file_mtime: u64,           // unix seconds
    pub filtered_contents: String,
    pub line_map: crate::text_file_map::LineMap,
}

pub const SNAPSHOT_VERSION: u32 = 1;
```

`<abbrev>` is `Work::abbrev` (already used as the config.json `last_work` key, so the lookup is direct). Unknown / future versions invalidate the cache.

### Cache module

New `src/snapshot.rs` exposes:

```rust
pub fn cache_dir() -> PathBuf;                                  // ~/.cache/linux-lit/snapshots
pub fn cache_path(abbrev: &str) -> PathBuf;                     // ~/.cache/linux-lit/snapshots/<abbrev>.text.bin
pub fn read(work: &Work) -> Option<WorkSnapshot>;               // None on miss / stale / corrupt
pub fn write(work: &Work, filtered_contents: &str,              // creates dir, writes atomically
             line_map: &LineMap) -> std::io::Result<()>;
pub fn delete_all() -> std::io::Result<()>;                     // for --clear-cache
```

`read` validates four ways: file exists, bincode parses, `cached.abbrev == work.abbrev`, `cached.text_file_path == work.text_file.unwrap_or_default()`, `cached.text_file_mtime == fs::metadata(path).modified()` truncated to seconds. Any mismatch → return None AND delete the stale file. Bincode errors → return None AND delete.

`write` creates `~/.cache/linux-lit/snapshots/` recursively if missing, writes to `<abbrev>.text.bin.tmp`, renames to `<abbrev>.text.bin` atomically.

### Validation policy

Per Q2 decision A: **mtime-only invalidation**. The snapshot is invalidated only if the text file's mtime changed. Font/theme/window-size changes do NOT invalidate (the layout will resettle on first paint; cache-restored content remains readable in the meantime).

### Tech stack

- `bincode = "1.3"` — new dep. Pinned to 1.3 for stability (2.0 has API churn).
- `serde` (already a dep, derive feature already enabled).
- `dirs` for `cache_dir()` — likely already a transitive dep; if not, add it.

`LineMap` and its sub-types (`SentenceGroup`) need `#[derive(Serialize, Deserialize)]`. They're already in `src/text_file_map.rs` with all-public fields of Vec/primitive types — derives compile cleanly.

---

## Integration

### MRU resume path (src/app.rs build_window)

Currently:
```
spawn_blocking 1: load_work + prepare_text_only(work)
main: buffer.set_text + reapply_font          (~1.3s)
spawn_blocking 2: build_line_map
main: display_work_at_with_prepared           (~3s)
```

After cache integration:
```
spawn_blocking 1:
    load_work
    if let Some(snap) = snapshot::read(&work):
        return (work, SnapshotOrPrep::Snapshot(snap))
    else:
        return (work, SnapshotOrPrep::Prep(prepare_text_only(&work)))

main thread, snapshot branch:
    buffer.set_text(snap.filtered_contents)
    reapply_font(state)
    line_map_for_phase2 = Some(snap.line_map)  // skip phase 2 entirely
    display_work_at_with_prepared(state, work, target_line_id, snap.into_prepared())
                                                          (~0.5s — text + line_map ready)

main thread, prep branch:
    same as today: phase-1 set_text + reapply_font, then await spawn_blocking 2 for
    build_line_map, then display_work_at_with_prepared
    AFTER display_work returns:
        spawn_blocking { snapshot::write(&work, &filtered_contents, &line_map) }
        (background — doesn't block UI)
```

A new enum or simple `Either` type carries either Snapshot or Prep through the await:

```rust
enum SnapshotOrPrep {
    Snapshot(WorkSnapshot),
    Prep(Option<PreparedTextOnly>),  // None = no text_file; fall back to default path
}
```

`WorkSnapshot` converts to the existing `PreparedText` struct via a `into_prepared(&self, work: &Work)` method that fills in the count fields by computing them from the snapshot's content (`cleaned_lines_count = self.filtered_contents.lines().count()`, `work_lines_count = work.lines.len()`, etc.). The counts are used only for log lines; small drift is harmless.

### Picker path (src/input/actions/pickers.rs)

Per Q6 decision A: same fast path. `load_selected_work` already does `spawn_blocking { load_work + prepare_text_for_display }`. Replace with the same cache-check-then-prep flow as MRU. Cache write also covers picker path (writes at end of display_work, regardless of which entry point invoked it).

### Cache write site

Inside the existing `glib::spawn_future_local` for the MRU and picker paths, AFTER `display_work_at_with_prepared` returns, AND only when this was a cache miss (not when we restored from snapshot — re-writing the same data is wasted I/O).

```rust
// MRU path, after display_work, only on cache miss:
if let SnapshotOrPrep::Prep(Some(prep)) = snapshot_or_prep {
    let abbrev = work.abbrev.clone();
    let text_file = work.text_file.clone();
    // line_map was moved into state during display_work; clone it back out
    let line_map = state_clone.borrow().line_map.clone();
    if let (Some(path), Some(lm)) = (text_file, line_map) {
        let filtered = prep.filtered_contents;
        let work_for_snap = work.clone();
        handle.spawn_blocking(move || {
            let _ = crate::snapshot::write(&work_for_snap, &filtered, &lm);
        });
    }
}
```

The clone of `line_map` is ~1MB of memory but happens off the critical visible-content path. The spawn_blocking write is ~50-100ms of disk I/O on a background thread.

### --clear-cache flag

Add to `argh` arg parsing in `src/main.rs`. When present, `snapshot::delete_all()` runs before app launch, then app proceeds normally.

```rust
#[derive(argh::FromArgs)]
#[argh(description = "linux-lit ebook reader")]
struct Args {
    #[argh(switch, description = "clear all cached snapshots and exit")]
    clear_cache: bool,
}
```

If `--clear-cache` is passed, delete cache and EXIT (don't proceed to launch — matches the user's mental model of "clear cache" being a maintenance command, not a launch flag).

### Logging

New log lines for visibility:
- `SNAPSHOT: cache hit {abbrev} ({read_ms}ms, {bytes} bytes)` on read success
- `SNAPSHOT: cache miss {abbrev} ({reason})` on read failure (file_missing / parse_error / mtime_stale / path_mismatch / abbrev_mismatch / version_skew)
- `SNAPSHOT: stale file deleted {abbrev}` after auto-cleanup
- `SNAPSHOT: write {abbrev} ({bytes} bytes, {write_ms}ms)` on write success
- `SNAPSHOT: write failed {abbrev} ({error})` on write error (logged but non-fatal)

---

## Error / edge cases

1. **First-run, cache dir doesn't exist.** Read returns None. Write creates dir recursively.
2. **Stale mtime.** read() detects mismatch, deletes file, returns None. Slow path runs, writes new snapshot.
3. **text_file path changed.** Same as stale: delete + None + slow path.
4. **abbrev collision** (paranoia). Validation catches it; return None.
5. **Bincode parse error** (corrupt file, schema skew). Return None, delete file.
6. **Disk full / permission denied on write.** Log warning, continue normally. Next launch is slow but functional.
7. **Work has no text_file (DB-only).** `prepare_text_for_display` already returns None for these. Don't cache; nothing to cache.
8. **Concordance spawn** (LINUX_LIT_LINE_ID set). Cache restoration applies; `target_line_id` overrides the saved cursor position via the existing `display_work_at_with_prepared` flow. No special-casing needed.
9. **Concurrent linux-lit instances writing the same cache.** Atomic rename via .tmp file; last writer wins; readers always see one valid snapshot.

---

## Tests

### Unit tests (in `src/snapshot.rs::tests`)

Use `tempfile::tempdir()` to isolate filesystem effects. Add `tempfile = "3"` to `[dev-dependencies]` if not already there.

1. **roundtrip** — build a synthetic `WorkSnapshot` with non-empty filtered_contents and a small line_map, write to tmpdir, read back via mocked `Work`, assert equality field-by-field.
2. **mtime_mismatch_returns_none_and_deletes** — write snapshot for a sentinel file, change the file's mtime via `set_file_mtime`, read should return None AND the cache file should be gone.
3. **path_mismatch_returns_none** — snapshot has path A on disk; Work has path B; read returns None.
4. **abbrev_mismatch_returns_none** — snapshot's stored abbrev differs from request's abbrev; return None.
5. **corrupt_file_returns_none_and_deletes** — write garbage bytes to the cache path, read returns None and deletes.
6. **missing_file_returns_none** — no file at the path; read returns None silently (no delete attempt).
7. **write_creates_dir_when_missing** — point `cache_dir()` at a non-existent tmpdir subpath, write succeeds.
8. **delete_all_clears_dir** — write 3 snapshots, call delete_all, assert directory is empty.
9. **version_skew_returns_none_and_deletes** — write a snapshot, edit the on-disk version field to SNAPSHOT_VERSION + 1, read returns None and deletes.

### Manual verification protocol

```
1. cargo build (must succeed; new bincode + tempfile deps).
2. cargo test (must include the new snapshot tests; expect ~9 new passing).
3. rm -rf ~/.cache/linux-lit/snapshots
4. cargo run.
   Log expected: "SNAPSHOT: cache miss ... (file_missing)"
                 normal phase 1 + phase 2 flow runs
                 "SNAPSHOT: write ... bytes" after display_work
5. ls ~/.cache/linux-lit/snapshots/ — confirm <abbrev>.text.bin exists.
6. Close linux-lit (Ctrl+Alt+L).
7. cargo run again.
   Log expected: "SNAPSHOT: cache hit ... ({}ms, {} bytes)"
                 NO "PREP: build_line_map" log line
                 text + cursor + page label visible at ~0.5s
8. touch <text_file_path>
9. cargo run.
   Log expected: "SNAPSHOT: cache miss ... (mtime_stale)"
                 "SNAPSHOT: stale file deleted"
                 normal slow path with re-write
10. Open a different work via Ctrl+p that has no cache.
    Confirm: cache miss, slow path, then cache write.
    Re-pick same work.
    Confirm: cache hit, fast path.
11. cargo run -- --clear-cache
    Confirm: snapshots/ directory empty (or absent) after exit.
12. Confirm: 'verified' or describe any regression.
```

---

## Effort estimate

S-M. ~3 commits:

1. **Add snapshot module + bincode dep + LineMap derives + unit tests** (~150 LOC + tests).
2. **Wire MRU + picker paths to cache-check-then-prep + cache-write at end of display_work** (~80 LOC of dispatch changes).
3. **--clear-cache flag in main.rs** (~20 LOC).

Total: ~250 LOC + ~150 LOC of tests. ~half a day of focused work.

---

## Out of scope (deferred)

- Multi-version cache migration (just bump version, invalidate everything).
- Per-page rendered PNG snapshot (Q1 option C — over-engineered).
- Cache size eviction (Q7 option B/C — not needed for ~10s of works).
- Snapshot of cursor/page state (already lives in config.json, no change).
- Snapshot for non-text_file works (DB-only) — no large parse to cache.
- Compressing the snapshot (3.5MB on Bleak House is fine; compression saves ~50% but adds complexity for marginal gain).

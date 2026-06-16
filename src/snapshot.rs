use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::db::models::Work;
use crate::text_file_map::LineMap;

// Bumped to 5: build_section_starts now pins a sonnet_sequence boundary to its
// bare-number heading (is_stanza_number), changing the cached section_starts
// values for Son. The serialized shape is unchanged, but stale snapshots hold
// the old (wrong) bitmap; the version bump forces a rebuild.
//
// NB: BCP works are loaded straight from the DB (text_file is NULL) and never
// hit the snapshot cache, so the sentence-per-line split needs no version bump —
// it has no cached representation to invalidate.
pub const SNAPSHOT_VERSION: u32 = 5;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkSnapshot {
    pub version: u32,
    pub abbrev: String,
    pub text_file_path: String,
    pub text_file_mtime: u64,
    pub filtered_contents: String,
    pub line_map: LineMap,
}

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
    let mut tmp_str = path.to_path_buf().into_os_string();
    tmp_str.push(".tmp");
    let tmp = PathBuf::from(tmp_str);
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
            media_paths: Vec::new(),
            media_ids: Vec::new(),
            media_id: None,
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
            chapter_breaks: vec![],
            section_starts: vec![true, false, false],
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

    #[test]
    fn delete_all_clears_dir() {
        let dir = tempdir().unwrap();
        // Point cache_dir at our tmpdir for the duration of this test.
        // Note: env var is process-wide; ok in a single-threaded test
        // (cargo test runs each test in its own thread but std env access
        // is unsafe across threads — use a serial-test if flaky in CI).
        std::env::set_var("XDG_CACHE_HOME", dir.path());

        // Create the snapshots dir under cache_dir() and write 3 files.
        let snapshots_dir = cache_dir();
        std::fs::create_dir_all(&snapshots_dir).unwrap();
        std::fs::write(snapshots_dir.join("a.text.bin"), b"a").unwrap();
        std::fs::write(snapshots_dir.join("b.text.bin"), b"b").unwrap();
        std::fs::write(snapshots_dir.join("c.text.bin"), b"c").unwrap();
        assert_eq!(std::fs::read_dir(&snapshots_dir).unwrap().count(), 3);

        delete_all().unwrap();

        // Directory should be gone OR empty (delete_all uses remove_dir_all).
        assert!(!snapshots_dir.exists() || std::fs::read_dir(&snapshots_dir).unwrap().count() == 0);

        std::env::remove_var("XDG_CACHE_HOME");
    }
}

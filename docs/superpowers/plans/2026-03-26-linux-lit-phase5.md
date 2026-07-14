# linux-lit Phase 5: MPV Integration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect to MPV via Unix domain sockets, sync cursor position with audio playback, toggle pause, and seek on dialogue jumps.

**Architecture:** A Tokio task on the background thread manages the MPV socket connection. It receives commands (Seek, TogglePause, Connect) from the GTK thread via `tokio::sync::mpsc`, and sends events (CursorSync, ConnectionStatus) back via another mpsc channel drained by `glib::spawn_future_local`. The time-pos observer pattern uses MPV's `observe_property` command to receive position updates, then binary-searches the loaded work's timestamps to find the current line.

**Tech Stack:** tokio (UnixStream, async I/O), serde_json (JSON-RPC), sha2 (SHA256 for socket path hashing)

**Depends on:** Phase 1-3 (complete) — GTK4 window, database, navigation, channel bridge

---

## File Structure

```
~/utono/linux-lit/src/
  mpv/
    mod.rs              # Modified: add re-exports for new modules
    commands.rs         # Existing: MpvCommand, MpvEvent enums (already defined)
    discovery.rs        # NEW: socket path derivation, scanning, probing
    client.rs           # NEW: Tokio task for MPV IPC (connect, observe, command dispatch)
  app.rs                # Modified: wire MPV on work load, handle CursorSync events
  input/keymap.rs       # Modified: add Tab key for pause toggle, seek after dialogue jump
```

---

### Task 1: Add SHA2 Dependency and Create Socket Discovery

**Files:**
- Modify: `Cargo.toml`
- Create: `src/mpv/discovery.rs`
- Modify: `src/mpv/mod.rs`

Socket path derivation follows the `lit` plugin's convention so linux-lit can connect to sockets created by `lit` or `socket-play-fzf.sh`.

- [ ] **Step 1: Add sha2 crate to Cargo.toml**

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Create `src/mpv/discovery.rs`**

```rust
use sha2::{Digest, Sha256};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

/// Derive the deterministic socket path for a media file.
/// Matches the convention in lit's mpv_sockets.lua.
pub fn derive_socket_path(media_path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();

    // Extract author directory from path
    let author = extract_author(media_path, &home);
    let basename = Path::new(media_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    // yt-dlp prefix
    let is_ytdlp = media_path.contains("/yt-dlp-mlj/");
    let socket_path = if is_ytdlp {
        format!("/tmp/mpvsocket-ytdlp-{}-{}", author, basename)
    } else {
        format!("/tmp/mpvsocket-{}-{}", author, basename)
    };

    // Truncate + hash if > 95 chars
    if socket_path.len() > 95 {
        let prefix = &socket_path[..87];
        let mut hasher = Sha256::new();
        hasher.update(socket_path.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        format!("{}-{}", prefix, &hash[..7])
    } else {
        socket_path
    }
}

/// Extract author directory name from a media path.
fn extract_author(media_path: &str, home: &str) -> String {
    // Try ~/Music/{author}/
    let music_prefix = format!("{}/Music/", home);
    if let Some(rest) = media_path.strip_prefix(&music_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    // Try ~/rips/{author}/
    let rips_prefix = format!("{}/rips/", home);
    if let Some(rest) = media_path.strip_prefix(&rips_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    // Try ~/yt-dlp-mlj/{author}/
    let ytdlp_prefix = format!("{}/yt-dlp-mlj/", home);
    if let Some(rest) = media_path.strip_prefix(&ytdlp_prefix) {
        if let Some(author) = rest.split('/').next() {
            return author.to_string();
        }
    }
    "music".to_string()
}

/// Scan /tmp for mpvsocket-* files that are Unix sockets.
pub fn scan_sockets() -> Vec<PathBuf> {
    let mut sockets = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("mpvsocket-") {
                let path = entry.path();
                // Verify it's a socket (requires FileTypeExt on Unix)
                if let Ok(meta) = std::fs::symlink_metadata(&path) {
                    if meta.file_type().is_socket() {
                        sockets.push(path);
                    }
                }
            }
        }
    }
    sockets.sort();
    sockets
}

/// Find the best socket for a work by checking its media paths.
/// Returns the first socket path that exists as a file.
pub fn find_socket_for_work(media_paths: &[String]) -> Option<PathBuf> {
    // Step 1: Deterministic prediction — derive socket path for each media file
    for media_path in media_paths {
        let socket_path = derive_socket_path(media_path);
        let path = PathBuf::from(&socket_path);
        if path.exists() {
            return Some(path);
        }
    }

    // Step 2: Scan fallback — if exactly one socket exists, use it
    let all_sockets = scan_sockets();
    if all_sockets.len() == 1 {
        return Some(all_sockets.into_iter().next().unwrap());
    }

    None
}

/// Launch headless MPV for a media file and return the socket path.
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    let _ = std::process::Command::new("mpv")
        .arg(format!("--input-ipc-server={}", socket_path))
        .arg("--pause")
        .arg("--no-video")
        .arg("--no-terminal")
        .arg(media_path)
        .spawn();
    socket_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_socket_path_music() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/Music/shakespeare-william/Hamlet.m4b", home);
        let socket = derive_socket_path(&path);
        assert!(socket.starts_with("/tmp/mpvsocket-shakespeare-william-"));
        assert!(socket.contains("Hamlet.m4b"));
        assert!(!socket.contains("ytdlp"));
    }

    #[test]
    fn test_derive_socket_path_ytdlp() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/yt-dlp-mlj/some-author/video.mp4", home);
        let socket = derive_socket_path(&path);
        assert!(socket.starts_with("/tmp/mpvsocket-ytdlp-some-author-"));
    }

    #[test]
    fn test_derive_socket_path_truncation() {
        let home = std::env::var("HOME").unwrap();
        let long_name = "a".repeat(100);
        let path = format!("{}/Music/author/{}.m4b", home, long_name);
        let socket = derive_socket_path(&path);
        assert!(socket.len() <= 95);
    }

    #[test]
    fn test_scan_sockets_runs() {
        // Just verify it doesn't crash — actual sockets may or may not exist
        let _sockets = scan_sockets();
    }
}
```

- [ ] **Step 3: Update `src/mpv/mod.rs`**

```rust
pub mod client;
pub mod commands;
pub mod discovery;
pub use commands::{MpvCommand, MpvEvent};
```

Note: `client.rs` doesn't exist yet — create an empty stub file so this compiles.

- [ ] **Step 3b: Update `src/mpv/commands.rs` to add `SetTimestamps` variant**

```rust
use std::collections::HashMap;

/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvCommand {
    Seek(f64),
    TogglePause,
    LoadFile(String),
    Connect(String),
    Disconnect,
    /// Update the timestamp data used for playback-to-line sync.
    SetTimestamps {
        timestamps: Vec<(i64, f64, f64)>,       // (line_id, start, end) sorted by start
        line_id_to_index: HashMap<i64, usize>,   // line_mapping.id -> index in work.lines
    },
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test mpv::discovery`
Expected: All 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/mpv/
git commit -m "feat: add MPV socket discovery with deterministic path derivation"
```

---

### Task 2: Create MPV Client (Tokio Task)

**Files:**
- Create: `src/mpv/client.rs`

The client is a Tokio task that manages the Unix socket connection to MPV. It processes commands from the GTK thread and sends events back.

- [ ] **Step 1: Create `src/mpv/client.rs`**

```rust
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::commands::{MpvCommand, MpvEvent};

/// Run the MPV client task. Receives commands, manages connection, sends events.
pub async fn run(
    mut cmd_rx: mpsc::Receiver<MpvCommand>,
    evt_tx: mpsc::Sender<MpvEvent>,
) {
    let mut reader: Option<BufReader<tokio::net::unix::OwnedReadHalf>> = None;
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;
    let mut timestamps: Vec<(i64, f64, f64)> = Vec::new();
    let mut line_id_to_index: HashMap<i64, usize> = HashMap::new();

    loop {
        // Two modes: connected (read from socket + commands) or disconnected (commands only)
        if let Some(ref mut r) = reader {
            let mut line_buf = String::new();
            tokio::select! {
                result = r.read_line(&mut line_buf) => {
                    match result {
                        Ok(0) | Err(_) => {
                            // EOF or error — disconnected
                            reader = None;
                            writer = None;
                            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                        }
                        Ok(_) => {
                            if let Some(pos) = parse_time_pos(&line_buf) {
                                if let Some(idx) = find_line_for_time(pos, &timestamps, &line_id_to_index) {
                                    let _ = evt_tx.send(MpvEvent::CursorSync(idx)).await;
                                }
                            }
                            if let Some(paused) = parse_pause_state(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::PlaybackState(!paused)).await;
                            }
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index).await;
                }
            }
        } else {
            // Disconnected — wait for commands only
            match cmd_rx.recv().await {
                Some(cmd) => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx, &mut timestamps, &mut line_id_to_index).await;
                }
                None => break, // Channel closed
            }
        }
    }
}

async fn handle_command(
    cmd: MpvCommand,
    reader: &mut Option<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: &mut Option<tokio::net::unix::OwnedWriteHalf>,
    evt_tx: &mpsc::Sender<MpvEvent>,
    timestamps: &mut Vec<(i64, f64, f64)>,
    line_id_to_index: &mut HashMap<i64, usize>,
) {
    match cmd {
        MpvCommand::Connect(path) => {
            match connect_and_observe(&path).await {
                Ok((r, w)) => {
                    *reader = Some(r);
                    *writer = Some(w);
                    let _ = evt_tx.send(MpvEvent::ConnectionStatus(true)).await;
                    crate::logging::log(&format!("MPV: connected to {}", path));
                }
                Err(e) => {
                    crate::logging::log(&format!("MPV: connect failed: {}", e));
                    let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                }
            }
        }
        MpvCommand::Disconnect => {
            *reader = None;
            *writer = None;
            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
        }
        MpvCommand::TogglePause => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["cycle","pause"]}"#).await;
            }
        }
        MpvCommand::Seek(time) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","time-pos",{}]}}"#, time);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::LoadFile(path) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["loadfile","{}"]}}"#, path.replace('"', r#"\""#));
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::SetTimestamps { timestamps: ts, line_id_to_index: map } => {
            *timestamps = ts;
            *line_id_to_index = map;
            crate::logging::log(&format!("MPV: loaded {} timestamps", timestamps.len()));
        }
    }
}

/// Connect to MPV socket and register time-pos + pause observers.
async fn connect_and_observe(
    path: &str,
) -> Result<
    (BufReader<tokio::net::unix::OwnedReadHalf>, tokio::net::unix::OwnedWriteHalf),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let stream = UnixStream::connect(path).await?;
    let (read_half, mut write_half) = stream.into_split();

    // Register observers
    send_command(&mut write_half, r#"{"command":["observe_property",1,"time-pos"]}"#).await?;
    send_command(&mut write_half, r#"{"command":["observe_property",2,"pause"]}"#).await?;

    Ok((BufReader::new(read_half), write_half))
}

/// Send a newline-terminated JSON command to MPV.
async fn send_command(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    cmd: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writer.write_all(cmd.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

/// Parse a time-pos property change event from MPV JSON output.
fn parse_time_pos(line: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "time-pos" {
        v.get("data")?.as_f64()
    } else {
        None
    }
}

/// Parse a pause property change event.
fn parse_pause_state(line: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "pause" {
        v.get("data")?.as_bool()
    } else {
        None
    }
}

/// Binary search sorted timestamps to find which line corresponds to the given playback time.
/// Uses 0.3s preroll — highlights the line slightly before its start_time.
fn find_line_for_time(
    time_pos: f64,
    timestamps: &[(i64, f64, f64)], // (line_id, start, end) sorted by start
    line_id_to_index: &HashMap<i64, usize>,
) -> Option<usize> {
    let preroll = 0.3;
    let effective_time = time_pos + preroll;

    // Binary search: find the last timestamp whose start <= effective_time
    let idx = timestamps.partition_point(|ts| ts.1 <= effective_time);
    if idx == 0 {
        return None;
    }
    let (line_id, _, _) = timestamps[idx - 1];
    line_id_to_index.get(&line_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_pos() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":123.456}"#;
        assert_eq!(parse_time_pos(line), Some(123.456));
    }

    #[test]
    fn test_parse_time_pos_null() {
        let line = r#"{"event":"property-change","id":1,"name":"time-pos","data":null}"#;
        assert_eq!(parse_time_pos(line), None);
    }

    #[test]
    fn test_parse_pause_state() {
        let line = r#"{"event":"property-change","id":2,"name":"pause","data":true}"#;
        assert_eq!(parse_pause_state(line), Some(true));
    }

    #[test]
    fn test_find_line_for_time() {
        let timestamps = vec![
            (10, 1.0, 2.0),
            (20, 3.0, 4.0),
            (30, 5.0, 6.0),
        ];
        let map: HashMap<i64, usize> = [(10, 0), (20, 1), (30, 2)].into();

        // At time 0.5 (+ 0.3 preroll = 0.8): before first timestamp
        assert_eq!(find_line_for_time(0.5, &timestamps, &map), None);

        // At time 1.0 (+ 0.3 = 1.3): first line
        assert_eq!(find_line_for_time(1.0, &timestamps, &map), Some(0));

        // At time 2.5 (+ 0.3 = 2.8): still first line (before second starts at 3.0)
        assert_eq!(find_line_for_time(2.5, &timestamps, &map), Some(0));

        // At time 2.8 (+ 0.3 = 3.1): second line (preroll puts us past 3.0)
        assert_eq!(find_line_for_time(2.8, &timestamps, &map), Some(1));

        // At time 5.0 (+ 0.3 = 5.3): third line
        assert_eq!(find_line_for_time(5.0, &timestamps, &map), Some(2));
    }
}
```

- [ ] **Step 2: Verify tests pass**

Run: `cargo test mpv::client`
Expected: All 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/mpv/client.rs
git commit -m "feat: add MPV client with socket IPC, time-pos observer, and timestamp sync"
```

---

### Task 3: Wire MPV into the Application

**Files:**
- Modify: `src/app.rs` — add `cmd_tx` to AppState, start MPV client on work load
- Modify: `src/main.rs` — pass `cmd_tx` to `build_window`, start event processing
- Modify: `src/input/keymap.rs` — add Tab for pause toggle, seek after dialogue jump

This task connects everything: when a work loads, find/launch MPV and connect. Tab toggles pause. Comma/q seek after jump.

- [ ] **Step 1: Add `cmd_tx` and `tokio_handle` to AppState**

In `src/app.rs`, add to the `AppState` struct:

```rust
    pub cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
    pub tokio_handle: tokio::runtime::Handle,
```

Update `build_window` signature to accept `cmd_tx: tokio::sync::mpsc::Sender<MpvCommand>`. The `tokio_handle` is already a parameter — store it in AppState too.

In `main.rs`, remove the `std::mem::forget(cmd_tx)` line. Instead, pass `cmd_tx` to `build_window` (it will be stored in AppState, keeping the channel open). The `cmd_rx` is moved into the Tokio background thread as before.

- [ ] **Step 2: Start MPV connection on work load**

In `src/app.rs`, in `display_work()`, after loading the work, start MPV discovery and connection:

```rust
    // MPV: send timestamp data and find/launch socket
    {
        let mut ts_data: Vec<(i64, f64, f64)> = work.timestamps.iter()
            .map(|t| (t.line_id, t.start, t.end))
            .collect();
        ts_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut id_to_idx: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for (i, line) in work.lines.iter().enumerate() {
            id_to_idx.insert(line.id, i);
        }
        let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::SetTimestamps {
            timestamps: ts_data,
            line_id_to_index: id_to_idx,
        });
    }

    if !work.media_paths.is_empty() {
        let media_paths = work.media_paths.clone();
        let cmd_tx = state.cmd_tx.clone();
        let tokio_handle = state.tokio_handle.clone();

        glib::spawn_future_local(async move {
            let socket_path = tokio_handle.spawn_blocking(move || {
                // Try to find existing socket
                if let Some(path) = crate::mpv::discovery::find_socket_for_work(&media_paths) {
                    return path.to_string_lossy().to_string();
                }
                // Launch MPV for first media file
                let launched = crate::mpv::discovery::launch_mpv(&media_paths[0]);
                // Wait for socket to appear
                for _ in 0..60 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if std::path::Path::new(&launched).exists() {
                        return launched;
                    }
                }
                launched
            }).await.unwrap_or_default();

            if !socket_path.is_empty() {
                let _ = cmd_tx.send(crate::mpv::MpvCommand::Connect(socket_path)).await;
            }
        });
    }
```

Note: This also requires adding `tokio_handle` to AppState.

- [ ] **Step 3: Process MpvEvent::CursorSync in main.rs**

Replace the stub event receiver in `main.rs` with one that updates the cursor:

```rust
    // Process MPV events
    let state_for_events = Rc::clone(&state);
    glib::spawn_future_local(async move {
        while let Some(event) = evt_rx.recv().await {
            match event {
                MpvEvent::CursorSync(line_idx) => {
                    let mut s = state_for_events.borrow_mut();
                    if s.current_line != line_idx {
                        s.current_line = line_idx;
                        crate::input::navigation::update_highlight_and_ensure_visible(&mut s);
                    }
                }
                MpvEvent::ConnectionStatus(connected) => {
                    crate::logging::log(&format!("MPV connection: {}", connected));
                }
                MpvEvent::PlaybackState(playing) => {
                    crate::logging::log(&format!("MPV playback: {}", if playing { "playing" } else { "paused" }));
                }
            }
        }
    });
```

- [ ] **Step 4: Start the MPV client task on the Tokio runtime**

In `main.rs`, inside the background thread's `block_on`, spawn the MPV client task instead of just draining commands:

```rust
    std::thread::spawn(move || {
        rt.block_on(async move {
            crate::mpv::client::run(cmd_rx, evt_tx).await;
        });
    });
```

The client starts with empty timestamps. When a work loads, `display_work()` sends `MpvCommand::SetTimestamps { ... }` with the work's timestamp data. This updates the client's sync data for the new work.

- [ ] **Step 5: Add Tab key for pause toggle in keymap.rs**

In the single-key match block:

```rust
        "Tab" => {
            let cmd_tx = state.borrow().cmd_tx.clone();
            let _ = cmd_tx.try_send(crate::mpv::MpvCommand::TogglePause);
            true
        }
```

- [ ] **Step 6: Add seek after dialogue jump**

In `navigation.rs`, after `jump_to_prev_dialogue` and `jump_to_next_dialogue` move the cursor, send a seek command if the line has a timestamp:

```rust
    // After setting state.current_line in dialogue jump:
    if let Some(ts) = &work.lines[state.current_line].timestamp {
        let seek_time = (ts.start - 0.2).max(0.0);
        let _ = state.cmd_tx.try_send(crate::mpv::MpvCommand::Seek(seek_time));
    }
```

- [ ] **Step 7: Add `update_highlight_and_ensure_visible` to navigation.rs**

A public function that both updates the highlight and ensures the cursor is on the page. Used by the CursorSync event handler:

```rust
pub fn update_highlight_and_ensure_visible(state: &mut AppState) {
    update_highlight(state);
    ensure_cursor_on_page(state);
}
```

- [ ] **Step 8: Verify compilation and tests**

Run: `cargo check && cargo test`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs src/main.rs src/input/keymap.rs src/input/navigation.rs src/mpv/
git commit -m "feat: wire MPV into app — Tab toggles pause, dialogue jump seeks, cursor syncs with playback"
```

---

### Task 4: Polish and Cleanup

- [ ] **Step 1: Run clippy**

```bash
cargo clippy 2>&1 | grep "warning:" | grep -v "generated"
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt
```

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for Phase 5"
```

---

## Phase 5 Acceptance Criteria

1. On work load: if media files exist, MPV socket is found or MPV is launched
2. Tab toggles MPV pause/resume
3. During playback: cursor follows the audio line by line (via time-pos observer)
4. Comma/q dialogue jump: after moving cursor, MPV seeks to `start_time - 0.2s`
5. Connection status logged
6. All tests pass, no clippy warnings

## Implementation Notes

- The `MpvCommand` enum is updated with `SetTimestamps` variant for syncing timestamp data
- The Tokio runtime is on a background thread; `cmd_tx` is stored in AppState (keeping channel alive)
- `std::mem::forget(cmd_tx)` in main.rs is removed — `cmd_tx` is passed to `build_window` instead
- Socket discovery is synchronous (runs via `spawn_blocking`) since it does filesystem I/O
- The MPV client runs as an async task on the Tokio runtime, not as a separate thread

## Notes for Phase 6

- The MPV connection state could be shown in a status bar
- The seek preroll (0.2s) matches `lit`'s `SEEK_PREROLL` constant
- The sync preroll (0.3s) matches `lit`'s `SYNC_PREROLL` — line is highlighted slightly before playback reaches it

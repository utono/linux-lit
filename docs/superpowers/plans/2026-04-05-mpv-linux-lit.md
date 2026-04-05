# mpv-linux-lit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a fast GTK4+Rust literature reader (mpv-linux-lit) with buffer ring for instant work switching and on-demand timestamp playback via Tab.

**Architecture:** Fresh Rust+GTK4 project at `~/utono/mpv-linux-lit`, copying individual modules from `~/utono/linux-lit`. Two-runtime design: GTK4 main thread for UI, Tokio thread for MPV IPC. Buffer ring holds multiple loaded works in memory. No line mapping, no gutter, scroll mode only.

**Tech Stack:** Rust, GTK4 0.9, libadwaita 0.7, sourceview5 0.9, Tokio 1, rusqlite 0.33, pango 0.20

**Source project:** `~/utono/linux-lit` (referred to as "linux-lit" throughout)

---

### Task 1: Create Repository and Project Skeleton

**Files:**
- Create: `~/utono/mpv-linux-lit/Cargo.toml`
- Create: `~/utono/mpv-linux-lit/src/main.rs`
- Create: `~/utono/mpv-linux-lit/.gitignore`
- Create: `~/utono/mpv-linux-lit/CLAUDE.md`

- [ ] **Step 1: Create directory and initialize Cargo project**

```bash
cd ~/utono
cargo init mpv-linux-lit
cd mpv-linux-lit
```

- [ ] **Step 2: Write Cargo.toml**

Replace the generated `Cargo.toml` with:

```toml
[package]
name = "mpv-linux-lit"
version = "0.1.0"
edition = "2021"

[dependencies]
gtk4 = { version = "0.9", features = ["v4_12"] }
libadwaita = { version = "0.7", features = ["v1_4"] }
sourceview5 = { version = "0.9", features = ["gtk_v4_12"] }
pango = "0.20"
glib = "0.20"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.33", features = ["bundled"] }
regex = "1"
sha2 = "0.10"
```

- [ ] **Step 3: Write .gitignore**

```
/target
```

- [ ] **Step 4: Write CLAUDE.md**

```markdown
# mpv-linux-lit

GTK4 Rust literature reader with buffer ring, MPV audio sync, and vim-style navigation.
Fast .txt file loading with on-demand timestamp playback via Tab key.

## Debug Log

- **Dev build** (`cargo run`): `~/utono/mpv-linux-lit/mpv-linux-lit-dev.log`
- **Release build**: `~/utono/mpv-linux-lit/mpv-linux-lit-release.log`

Use `log_fmt!()` macro (from `src/logging.rs`) to add log lines.

## Build & Run

```bash
cargo build
```

Do not run `cargo run` — the user will run it themselves.

## Key Files

- `src/main.rs` — entry point, Tokio runtime, channel bridge
- `src/app.rs` — GTK4 window, AppState, buffer ring, display_work
- `src/config.rs` — ~/.config/mpv-linux-lit/config.json persistence
- `src/input/keymap.rs` — key event routing, gg state machine
- `src/input/navigation.rs` — cursor movement, scrolling, dialogue nav
- `src/input/timestamps.rs` — Tab on-demand timestamp lookup + playback
- `src/db/queries.rs` — SQLite queries (list_works, load_work, lookup_timestamp)
- `src/ui/library_picker.rs` — Ctrl+p work picker with fuzzy filter

## External Data

- Database: `~/utono/litdb/data/lit.db` (read-only)
- Themes: `~/utono/themes/.config/themes/themes-unified.json`
- Config: `~/.config/mpv-linux-lit/config.json`
```

- [ ] **Step 5: Write minimal main.rs stub**

```rust
fn main() {
    println!("mpv-linux-lit");
}
```

- [ ] **Step 6: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

Expected: successful compilation.

- [ ] **Step 7: Initialize git and create GitHub remote**

```bash
cd ~/utono/mpv-linux-lit
git init
git add Cargo.toml Cargo.lock src/main.rs .gitignore CLAUDE.md
git commit -m "Initial project skeleton"
gh repo create utono/mpv-linux-lit --private --source=. --push
```

---

### Task 2: Utility Modules (mode, logging, config)

**Files:**
- Create: `src/mode.rs`
- Create: `src/logging.rs`
- Create: `src/config.rs`
- Modify: `src/main.rs`

These are direct copies from linux-lit with minor adaptations (config path, removed excluded fields).

- [ ] **Step 1: Create src/mode.rs**

Copy verbatim from linux-lit:

```rust
pub fn is_dev_mode() -> bool {
    std::env::var("LIT_DEV").is_ok()
}
```

- [ ] **Step 2: Create src/logging.rs**

Copy verbatim from linux-lit:

```rust
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

static LOG_PATH: OnceLock<String> = OnceLock::new();

pub fn init(path: &str) {
    LOG_PATH.set(path.to_string()).ok();
}

/// Write a line to the log file.
pub fn log(msg: &str) {
    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{}", msg);
        }
    }
}

/// Log with format args, like `log_fmt!("x={} y={}", x, y)`.
#[macro_export]
macro_rules! log_fmt {
    ($($arg:tt)*) => {
        $crate::logging::log(&format!($($arg)*))
    };
}
```

- [ ] **Step 3: Create src/config.rs**

Adapted from linux-lit — removed `NavigationMode`, `TransitionStyle`, `VisualModeCommand`, `ollama_*` fields. Config path changed to `~/.config/mpv-linux-lit/`.

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default = "default_line_spacing")]
    pub line_spacing: u32,
    #[serde(default = "default_column_width")]
    pub column_width: u32,
    #[serde(default = "default_text_margins")]
    pub text_margins: u32,
    #[serde(default)]
    pub last_work: Option<String>,
    #[serde(default)]
    pub last_line: usize,
    #[serde(default)]
    pub work_positions: HashMap<String, usize>,
    #[serde(default = "default_vocab_highlight_visible")]
    pub vocab_highlight_visible: bool,
    #[serde(default = "default_dim_enabled")]
    pub dim_enabled: bool,
}

fn default_font_family() -> String {
    "Charter".to_string()
}

pub const FONT_CYCLE: &[&str] = &[
    "Charter",
    "Crimson Pro",
    "Noto Serif",
    "Source Serif 4",
    "IBM Plex Serif",
    "Cormorant Garamond",
];

pub fn default_font_size() -> u32 {
    19
}

pub const DEFAULT_LINE_SPACING: u32 = 5;
pub const DEFAULT_COLUMN_WIDTH: u32 = 1200;
pub const DEFAULT_TEXT_MARGINS: u32 = 40;
pub const EXTRA_RIGHT_MARGIN: i32 = 28;

fn default_line_spacing() -> u32 {
    DEFAULT_LINE_SPACING
}

fn default_column_width() -> u32 {
    DEFAULT_COLUMN_WIDTH
}

fn default_text_margins() -> u32 {
    DEFAULT_TEXT_MARGINS
}

fn default_vocab_highlight_visible() -> bool {
    true
}

fn default_dim_enabled() -> bool {
    false
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            line_spacing: default_line_spacing(),
            column_width: default_column_width(),
            text_margins: default_text_margins(),
            last_work: None,
            last_line: 0,
            work_positions: HashMap::new(),
            vocab_highlight_visible: default_vocab_highlight_visible(),
            dim_enabled: default_dim_enabled(),
        }
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    let filename = if crate::mode::is_dev_mode() {
        "config-dev.json"
    } else {
        "config.json"
    };
    PathBuf::from(home).join(".config/mpv-linux-lit").join(filename)
}

pub fn load() -> Config {
    let path = config_path();
    if !path.exists() {
        return Config::default();
    }
    let mut config = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    };
    config.font_size = default_font_size();
    config
}

pub fn save(config: &Config) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if let Ok(json) = serde_json::to_string_pretty(config) {
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}
```

- [ ] **Step 4: Update main.rs to declare modules and verify build**

```rust
mod config;
mod logging;
mod mode;

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let log_filename = if mode::is_dev_mode() {
        "mpv-linux-lit-dev.log"
    } else {
        "mpv-linux-lit-release.log"
    };
    let log_path = format!("{}/utono/mpv-linux-lit/{}", home, log_filename);
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);

    let _config = config::load();
    println!("mpv-linux-lit: config loaded");
}
```

- [ ] **Step 5: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 6: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/mode.rs src/logging.rs src/config.rs src/main.rs
git commit -m "Add utility modules: mode, logging, config"
```

---

### Task 3: Database Layer (models, line_types, queries, concordance)

**Files:**
- Create: `src/db/mod.rs`
- Create: `src/db/models.rs`
- Create: `src/db/line_types.rs`
- Create: `src/db/queries.rs`
- Create: `src/db/concordance.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create src/db/mod.rs**

```rust
pub mod concordance;
pub mod line_types;
pub mod models;
pub mod queries;
```

- [ ] **Step 2: Create src/db/models.rs**

Copy from linux-lit but remove `Chunk` struct (excluded feature):

```rust
#[derive(Debug, Clone)]
pub struct Work {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
    pub text_file: Option<String>,
    pub lines: Vec<Line>,
    pub timestamps: Vec<Timestamp>,
    pub media_paths: Vec<String>,
    pub media_ids: Vec<i64>,
    pub media_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub id: i64,
    pub citation: String,
    pub text: String,
    pub normalized: String,
    pub speaker: Option<String>,
    pub is_dialogue: bool,
    pub timestamp: Option<TimeRange>,
    pub div1: i64,
    pub div2: i64,
    pub line_in_div: i64,
    pub is_chapter: bool,
    pub is_spoken: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
    pub sentence_start: Option<f64>,
    pub sentence_end: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Timestamp {
    pub line_id: i64,
    pub start: f64,
    pub end: f64,
    pub media_id: i64,
    pub sentence_start: Option<f64>,
    pub sentence_end: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct WorkSummary {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work_type: String,
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub media_id: i64,
    pub path: String,
    pub display_name: Option<String>,
    pub priority: i64,
}
```

- [ ] **Step 3: Create src/db/line_types.rs**

Copy verbatim from linux-lit `src/db/line_types.rs` (158 lines). This module has no internal dependencies.

- [ ] **Step 4: Create src/db/queries.rs**

Copy from linux-lit `src/db/queries.rs`. Include:
- `open_db()` (read-only)
- `list_works()` — modify to filter `WHERE push_to_device = 1 AND text_file IS NOT NULL`
- `load_work()`
- `load_translations()`
- `load_vocab_words()`
- `load_vocab_definition()`
- `load_vocab_etymology()`
- `load_vocab_gloss()`
- `load_vocab_word_list()`
- `list_media_for_work()`

Remove all read-write functions (`open_db_rw`, `upsert_start_time`, `upsert_chapter`, `update_end_time`, `delete_timestamp`, `replace_lines`, `set_media_priority`).

Add a new function for on-demand timestamp lookup:

```rust
/// Look up the start_time for a line by normalized text match.
/// Used for Tab key on-demand playback.
pub fn lookup_timestamp_by_text(
    conn: &Connection,
    abbrev: &str,
    normalized_text: &str,
) -> Option<f64> {
    conn.query_row(
        "SELECT lt.start_time \
         FROM line_timestamps lt \
         JOIN line_mapping lm ON lt.line_mapping_id = lm.id \
         WHERE lm.work_abbrev = ?1 \
           AND lm.normalized_text = ?2 \
         ORDER BY lt.start_time ASC \
         LIMIT 1",
        [abbrev, normalized_text],
        |row| row.get(0),
    ).ok()
}
```

The key change to `list_works`: the WHERE clause must add `AND text_file IS NOT NULL` so only works with text files appear in the picker.

- [ ] **Step 5: Create src/db/concordance.rs**

Copy verbatim from linux-lit `src/db/concordance.rs` (64 lines). No internal dependencies.

- [ ] **Step 6: Update main.rs to declare db module and test DB access**

Add `mod db;` to main.rs. Update main:

```rust
mod config;
mod db;
mod logging;
mod mode;

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let log_filename = if mode::is_dev_mode() {
        "mpv-linux-lit-dev.log"
    } else {
        "mpv-linux-lit-release.log"
    };
    let log_path = format!("{}/utono/mpv-linux-lit/{}", home, log_filename);
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);

    let _config = config::load();

    let conn = db::queries::open_db().expect("Failed to open lit.db");
    let works = db::queries::list_works(&conn).expect("Failed to list works");
    logging::log(&format!("Loaded {} works with text_file", works.len()));
}
```

- [ ] **Step 7: Verify build and test**

```bash
cd ~/utono/mpv-linux-lit && cargo build && cargo test
```

- [ ] **Step 8: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/db/ src/main.rs
git commit -m "Add database layer: models, queries, line_types, concordance"
```

---

### Task 4: MPV Module (commands, discovery, client)

**Files:**
- Create: `src/mpv/mod.rs`
- Create: `src/mpv/commands.rs`
- Create: `src/mpv/discovery.rs`
- Create: `src/mpv/client.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create src/mpv/mod.rs**

```rust
pub mod client;
pub mod commands;
pub mod discovery;

pub use commands::{MpvCommand, MpvEvent};
```

- [ ] **Step 2: Create src/mpv/commands.rs**

Adapted from linux-lit — remove `SetAbLoop`, `ClearAbLoop`, `SetTimestamps`:

```rust
/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
pub enum MpvCommand {
    Seek(f64),
    SeekRelative(f64),
    VolumeAdjust(f64),
    TogglePause,
    Pause,
    ResumeAndSeek(f64),
    SetSpeed(f64),
    LoadFile(String),
    Connect(String),
    Disconnect,
    Quit,
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
pub enum MpvEvent {
    ConnectionStatus(bool),
    PlaybackState(bool),
    TimePos(f64),
    ThemeChanged,
}
```

Note: `CursorSync` and `SetTimestamps` are removed because mpv-linux-lit does not do continuous cursor sync — playback is on-demand via Tab only.

- [ ] **Step 3: Create src/mpv/discovery.rs**

Copy verbatim from linux-lit `src/mpv/discovery.rs` (162 lines including tests). The socket derivation must match `lit-prelaunch-mpv.sh` exactly.

- [ ] **Step 4: Create src/mpv/client.rs**

Simplified from linux-lit — no timestamps, no CursorSync, no AB loop:

```rust
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::commands::{MpvCommand, MpvEvent};

pub async fn run(
    mut cmd_rx: mpsc::Receiver<MpvCommand>,
    evt_tx: mpsc::Sender<MpvEvent>,
) {
    let mut reader: Option<BufReader<tokio::net::unix::OwnedReadHalf>> = None;
    let mut writer: Option<tokio::net::unix::OwnedWriteHalf> = None;

    loop {
        if let Some(ref mut r) = reader {
            let mut line_buf = String::new();
            tokio::select! {
                result = r.read_line(&mut line_buf) => {
                    match result {
                        Ok(0) | Err(_) => {
                            reader = None;
                            writer = None;
                            let _ = evt_tx.send(MpvEvent::ConnectionStatus(false)).await;
                        }
                        Ok(_) => {
                            if let Some(pos) = parse_time_pos(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::TimePos(pos)).await;
                            }
                            if let Some(paused) = parse_pause_state(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::PlaybackState(!paused)).await;
                            }
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx).await;
                }
            }
        } else {
            match cmd_rx.recv().await {
                Some(cmd) => {
                    handle_command(cmd, &mut reader, &mut writer, &evt_tx).await;
                }
                None => break,
            }
        }
    }
}

async fn handle_command(
    cmd: MpvCommand,
    reader: &mut Option<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: &mut Option<tokio::net::unix::OwnedWriteHalf>,
    evt_tx: &mpsc::Sender<MpvEvent>,
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
        MpvCommand::Pause => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["set_property","pause",true]}"#).await;
            }
        }
        MpvCommand::ResumeAndSeek(time) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","time-pos",{}]}}"#, time);
                let _ = send_command(w, &cmd).await;
                let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
            }
        }
        MpvCommand::SetSpeed(speed) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","speed",{}]}}"#, speed);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::Seek(time) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["set_property","time-pos",{}]}}"#, time);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::SeekRelative(offset) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["seek",{},"relative","exact"]}}"#, offset);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::VolumeAdjust(delta) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(r#"{{"command":["add","volume",{}]}}"#, delta);
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::LoadFile(path) => {
            if let Some(w) = writer.as_mut() {
                let cmd = format!(
                    r#"{{"command":["loadfile","{}"]}}"#,
                    path.replace('"', r#"\""#)
                );
                let _ = send_command(w, &cmd).await;
            }
        }
        MpvCommand::Quit => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["quit"]}"#).await;
            }
        }
    }
}

async fn connect_and_observe(
    path: &str,
) -> Result<
    (
        BufReader<tokio::net::unix::OwnedReadHalf>,
        tokio::net::unix::OwnedWriteHalf,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let stream = UnixStream::connect(path).await?;
    let (read_half, mut write_half) = stream.into_split();
    send_command(
        &mut write_half,
        r#"{"command":["observe_property",1,"time-pos"]}"#,
    )
    .await?;
    send_command(
        &mut write_half,
        r#"{"command":["observe_property",2,"pause"]}"#,
    )
    .await?;
    Ok((BufReader::new(read_half), write_half))
}

async fn send_command(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    cmd: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    writer.write_all(cmd.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn parse_time_pos(line: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "time-pos" {
        v.get("data")?.as_f64()
    } else {
        None
    }
}

fn parse_pause_state(line: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? == "property-change" && v.get("name")?.as_str()? == "pause" {
        v.get("data")?.as_bool()
    } else {
        None
    }
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
}
```

- [ ] **Step 5: Update main.rs to declare mpv module**

Add `mod mpv;` to the module declarations.

- [ ] **Step 6: Verify build and run tests**

```bash
cd ~/utono/mpv-linux-lit && cargo build && cargo test
```

- [ ] **Step 7: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/mpv/
git commit -m "Add MPV module: commands, discovery, client"
```

---

### Task 5: Theme Module

**Files:**
- Create: `src/theme.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create src/theme.rs**

Copy verbatim from linux-lit `src/theme.rs` (474 lines). It has only one internal dependency: `crate::logging`. The module reads from `~/utono/themes/.config/themes/themes-unified.json`.

- [ ] **Step 2: Update main.rs to declare theme module**

Add `mod theme;`.

- [ ] **Step 3: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/theme.rs src/main.rs
git commit -m "Add theme module"
```

---

### Task 6: UI Components (library_picker, search_bar, settings_overlay, keybinds_overlay, vocab_popup, concordance)

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/library_picker.rs`
- Create: `src/ui/search_bar.rs`
- Create: `src/ui/settings_overlay.rs`
- Create: `src/ui/keybinds_overlay.rs`
- Create: `src/ui/vocab_popup.rs`
- Create: `src/ui/concordance_picker.rs`
- Create: `src/ui/concordance_bar.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create src/ui/mod.rs**

```rust
pub mod concordance_bar;
pub mod concordance_picker;
pub mod keybinds_overlay;
pub mod library_picker;
pub mod search_bar;
pub mod settings_overlay;
pub mod vocab_popup;
```

- [ ] **Step 2: Copy UI modules from linux-lit**

Copy each file verbatim from linux-lit `src/ui/`:
- `library_picker.rs` (517 lines)
- `search_bar.rs` (76 lines)
- `keybinds_overlay.rs` (580 lines)
- `vocab_popup.rs` (200 lines)
- `concordance_picker.rs` (151 lines)
- `concordance_bar.rs` (68 lines)

For `settings_overlay.rs` (287 lines): copy from linux-lit but remove the `NavigationMode` and `TransitionStyle` settings rows. The settings overlay should only show: Theme, Line Spacing, Column Width, Text Margins. Remove the `Navigation` and `Transition` variants from the `SettingsChange` enum. Update the `show()` method to remove `navigation_mode` and `transition_style` parameters.

For `library_picker.rs`: the filtering logic stays the same, but the works list passed to it will already be pre-filtered to `text_file IS NOT NULL` by the query in Task 3.

- [ ] **Step 3: Handle concordance word picker and list picker**

linux-lit has `concordance_word_picker.rs` and `concordance_list_picker.rs` in addition to `concordance_picker.rs`. Check if these exist and copy them:

```bash
ls ~/utono/linux-lit/src/ui/concordance_*.rs
```

Copy all concordance UI files. Add them to `src/ui/mod.rs`.

- [ ] **Step 4: Update main.rs to declare ui module**

Add `mod ui;`.

- [ ] **Step 5: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

Fix any compilation errors. Common issues:
- `settings_overlay.rs` referencing `crate::config::NavigationMode` — remove those references
- UI modules referencing `crate::db::models::WorkSummary` — should work since db module exists
- Any references to excluded modules (visual, action_popup, correction_overlay, media_picker) — remove

- [ ] **Step 6: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/ui/ src/main.rs
git commit -m "Add UI components: picker, search, settings, keybinds, vocab, concordance"
```

---

### Task 7: Buffer Ring and AppState

**Files:**
- Create: `src/app.rs`
- Create: `src/concordance.rs`
- Modify: `src/main.rs`

This is the central module. Build AppState from scratch with buffer ring support.

- [ ] **Step 1: Create src/concordance.rs**

Copy the `ConcordanceState` struct and related logic from linux-lit `src/concordance.rs`. This tracks the current concordance word and occurrence list.

- [ ] **Step 2: Create src/app.rs with BufferRing and AppState**

```rust
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, WrapMode};
use libadwaita as adw;
use sourceview5::prelude::*;
use sourceview5::View;

use crate::config::Config;
use crate::db::models::{Work, WorkSummary};
use crate::ui::library_picker::LibraryPicker;
use crate::ui::search_bar::SearchBar;

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line_index: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone)]
pub struct VocabMatch {
    pub word: String,
    pub line_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

/// A work loaded into the buffer ring with its associated state.
pub struct LoadedWork {
    pub abbrev: String,
    pub title: String,
    pub author: String,
    pub work: Work,
    pub text: String,
    pub cursor_line: usize,
    pub vocab_words: HashSet<String>,
    pub media_socket: Option<PathBuf>,
}

/// Ring buffer of loaded works for instant switching.
pub struct BufferRing {
    pub buffers: Vec<LoadedWork>,
    pub active: usize,
}

impl BufferRing {
    pub fn new() -> Self {
        Self {
            buffers: Vec::new(),
            active: 0,
        }
    }

    /// Add a work to the ring or switch to it if already present.
    /// Returns the index of the work in the ring.
    pub fn add_or_switch(&mut self, abbrev: &str) -> Option<usize> {
        if let Some(idx) = self.buffers.iter().position(|w| w.abbrev == abbrev) {
            self.active = idx;
            Some(idx)
        } else {
            None // Caller must load and push
        }
    }

    pub fn push(&mut self, work: LoadedWork) {
        self.buffers.push(work);
        self.active = self.buffers.len() - 1;
    }

    pub fn cycle_next(&mut self) {
        if self.buffers.len() > 1 {
            self.active = (self.active + 1) % self.buffers.len();
        }
    }

    pub fn cycle_prev(&mut self) {
        if self.buffers.len() > 1 {
            self.active = if self.active == 0 {
                self.buffers.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    pub fn current(&self) -> Option<&LoadedWork> {
        self.buffers.get(self.active)
    }

    pub fn current_mut(&mut self) -> Option<&mut LoadedWork> {
        self.buffers.get_mut(self.active)
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }
}

pub struct AppState {
    pub text_view: View,
    pub buffer: sourceview5::Buffer,
    pub picker: LibraryPicker,
    pub current_line: usize,
    pub prev_highlight_line: Cell<Option<usize>>,
    pub cursor_line_tag: gtk4::TextTag,
    pub scrolled_window: ScrolledWindow,
    pub content_hbox: gtk4::Box,
    pub window: ApplicationWindow,
    pub config: Config,
    pub css_provider: CssProvider,
    pub theme: crate::theme::Theme,
    pub cmd_tx: tokio::sync::mpsc::Sender<crate::mpv::MpvCommand>,
    pub tokio_handle: tokio::runtime::Handle,
    pub playback_speed: f64,
    pub search_bar: SearchBar,
    pub search_matches: Vec<SearchMatch>,
    pub search_match_idx: usize,
    pub search_tag: gtk4::TextTag,
    pub search_current_tag: gtk4::TextTag,
    pub current_time_pos: f64,
    pub media_id: Option<i64>,
    pub settings_overlay: crate::ui::settings_overlay::SettingsOverlay,
    pub dialogue_formatting_active: bool,
    pub translations: HashMap<i64, String>,
    pub translations_visible: bool,
    pub translation_lines: Vec<bool>,
    pub translation_dim_tag: gtk4::TextTag,
    pub translation_text_tag: gtk4::TextTag,
    pub vocab_words: HashSet<String>,
    pub vocab_matches: Vec<VocabMatch>,
    pub vocab_match_idx: Option<usize>,
    pub vocab_tag: gtk4::TextTag,
    pub vocab_highlight_visible: bool,
    pub vocab_popup: crate::ui::vocab_popup::VocabPopup,
    pub vocab_popup_data: Vec<crate::ui::vocab_popup::VocabWordData>,
    pub vocab_popup_index: usize,
    pub vocab_popup_view: crate::ui::vocab_popup::VocabView,
    pub vocab_popup_auto: bool,
    pub vocab_popup_fade_gen: Rc<Cell<u64>>,
    pub concordance_picker: crate::ui::concordance_picker::ConcordancePicker,
    pub concordance_state: Option<crate::concordance::ConcordanceState>,
    pub concordance_bar: crate::ui::concordance_bar::ConcordanceBar,
    pub keybinds_overlay: crate::ui::keybinds_overlay::KeybindsOverlay,
    pub sync_enabled: bool,
    pub sync_icon: gtk4::Label,
    pub loading_work: Rc<Cell<bool>>,
    pub dim_enabled: bool,

    // Buffer ring
    pub ring: BufferRing,
}

impl AppState {
    pub fn effective_line_count(&self) -> usize {
        self.ring.current().map_or(0, |w| {
            w.text.lines().count()
        })
    }

    /// In mpv-linux-lit, buffer line index == work line index (no line_map).
    /// But we still need to map to the Work.lines vec for dialogue detection etc.
    /// The .txt file lines and Work.lines are different — .txt lines are raw file lines,
    /// Work.lines are from the database. For dialogue nav, we use the raw text.
    pub fn current_work(&self) -> Option<&Work> {
        self.ring.current().map(|lw| &lw.work)
    }
}
```

- [ ] **Step 3: Write the `build_window` function**

This is the core GTK setup function. Model it after linux-lit's `build_window` but simpler:
- No gutter setup
- No e-reader pagination (no page_turn_overlay, top_spacer, bottom_spacer, bottom_clip)
- No visual selection
- No AB repeat
- No correction overlay
- Scroll mode only

The function creates:
- `ApplicationWindow` with title "mpv-linux-lit"
- `sourceview5::View` (read-only, word wrap, no line numbers)
- `ScrolledWindow` containing the view
- CSS provider for theme
- All UI overlays (picker, search_bar, settings, keybinds, vocab_popup, concordance)
- `EventControllerKey` that dispatches to `input::keymap::handle_key`
- Returns `Rc<RefCell<AppState>>`

Reference linux-lit `src/app.rs` `build_window()` (starts around line 218) for the GTK widget setup patterns. Strip out all excluded features.

- [ ] **Step 4: Write the `display_work` function**

Called when a work is loaded from the picker or switched to from the ring:

```rust
pub fn display_work(state: &mut AppState, loaded: &LoadedWork) {
    state.loading_work.set(true);

    // Set buffer text from the raw .txt file content
    state.buffer.set_text(&loaded.text);

    // Apply dialogue formatting if applicable
    // (copy dialogue formatting logic from linux-lit's apply_dialogue_formatting)

    // Apply vocab highlighting
    // (copy vocab highlighting logic from linux-lit)

    // Restore cursor and scroll
    state.current_line = loaded.cursor_line;
    update_highlight(state);
    scroll_to_cursor(state);

    // Update window title with ring position
    let ring_pos = format!("{} [{}/{}]", loaded.title, state.ring.active + 1, state.ring.len());
    state.window.set_title(Some(&ring_pos));

    // Connect to MPV socket if available
    if let Some(ref socket) = loaded.media_socket {
        let _ = state.cmd_tx.try_send(
            crate::mpv::MpvCommand::Connect(socket.to_string_lossy().to_string())
        );
    }

    state.loading_work.set(false);
}
```

- [ ] **Step 5: Write the `switch_to_ring_entry` function**

Called when `-` or `_` is pressed:

```rust
pub fn switch_to_ring_entry(state: &mut AppState) {
    // Save current cursor position before switching
    if let Some(current) = state.ring.current_mut() {
        current.cursor_line = state.current_line;
    }

    let loaded = state.ring.current().unwrap();
    display_work(state, loaded);
}
```

- [ ] **Step 6: Update main.rs with full GTK application setup**

Model after linux-lit's `main.rs` — create GTK app, channels, Tokio runtime, call `build_window`, process MPV events. But simpler event loop (no CursorSync handling, just TimePos, ConnectionStatus, PlaybackState, ThemeChanged).

- [ ] **Step 7: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

Expect compilation errors — some UI modules may reference AppState fields that don't exist yet or reference excluded features. Fix iteratively.

- [ ] **Step 8: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/app.rs src/concordance.rs src/main.rs
git commit -m "Add AppState with buffer ring and display pipeline"
```

---

### Task 8: Input Module (keymap, navigation, timestamps, search)

**Files:**
- Create: `src/input/mod.rs`
- Create: `src/input/keymap.rs`
- Create: `src/input/navigation.rs`
- Create: `src/input/timestamps.rs`
- Create: `src/input/search.rs`

- [ ] **Step 1: Create src/input/mod.rs**

```rust
pub mod keymap;
pub mod navigation;
pub mod search;
pub mod timestamps;
```

- [ ] **Step 2: Create src/input/keymap.rs**

Adapted from linux-lit's keymap.rs. Key changes:
- Remove all `line_map` references (no text file mapping)
- Remove visual mode handling
- Remove AB repeat handling
- Remove correction overlay handling
- Remove media picker handling
- Remove gutter renderer references
- Add `-` (hyphen) → `ring.cycle_next()` + `switch_to_ring_entry()`
- Add `_` (underscore) → `ring.cycle_prev()` + `switch_to_ring_entry()`
- Change Tab → `timestamps::play_current_line_on_demand()`
- Remove `,`/`q` seek behavior — keep dialogue nav only (no `seek_to_current_line`)
- Keep: j/k, h/l, gg/G, /, n/N, Space, [/], r/R, Ctrl+p, Ctrl+Shift+p, Ctrl+Alt+p, Ctrl+/, \, Ctrl+l, Ctrl+f, f/F

The `KeyState` struct and `handle_key()` signature remain the same as linux-lit.

- [ ] **Step 3: Create src/input/navigation.rs**

Copy from linux-lit `src/input/navigation.rs` with these removals:
- Remove all `line_map` references — buffer line == work line
- Remove all e-reader / page-turn code (`set_page`, page_turn_anim, top_spacer, bottom_spacer)
- Remove `position_chunk` (AB repeat)
- Remove `pending_advance` logic
- Remove `resnap_page` (e-reader only)
- Remove dim_tag references
- Keep: `SYNC_PREROLL` constant, `move_cursor`, `jump_to_start`, `jump_to_end`, `page_forward`, `page_backward`, `jump_to_prev_dialogue`, `jump_to_next_dialogue`, `jump_to_prev_paragraph`, `jump_to_next_paragraph`, `jump_to_prev_chapter`, `jump_to_next_chapter`, `scroll_viewport`, `update_highlight_only`, `update_highlight_and_ensure_visible`, `update_highlight_and_center`, `jump_to_next_vocab`, `jump_to_prev_vocab`, `concordance_jump_to_current`, `word_cycle_copy`, `word_collect_copy`

Simplify scrolling — all navigation uses `scroll_to_cursor()` which adjusts the `ScrolledWindow` vadjustment to keep the cursor visible.

- [ ] **Step 4: Create src/input/timestamps.rs**

This is NEW — not copied from linux-lit. It handles on-demand playback via Tab:

```rust
use crate::app::AppState;

/// Tab key handler: look up the start_time for the current line's text
/// and start playback from that position.
pub fn play_current_line_on_demand(state: &mut AppState) -> bool {
    let work = match state.ring.current() {
        Some(lw) => lw,
        None => return false,
    };

    // Get the current buffer line text
    let text = {
        let start = state.buffer.iter_at_line(state.current_line as i32);
        let end = {
            let mut e = state.buffer.iter_at_line(state.current_line as i32);
            if !e.is_none() {
                e.forward_to_line_end();
            }
            e
        };
        match (start, end) {
            (Some(s), e) => state.buffer.text(&s, &e, false).to_string(),
            _ => return false,
        }
    };

    // Normalize: lowercase, strip brackets, collapse whitespace
    let normalized = normalize_text(&text);
    if normalized.is_empty() {
        return false;
    }

    // Query lit.db for start_time
    let abbrev = work.abbrev.clone();
    let cmd_tx = state.cmd_tx.clone();
    let handle = state.tokio_handle.clone();

    handle.spawn_blocking(move || {
        if let Ok(conn) = crate::db::queries::open_db() {
            if let Some(start_time) = crate::db::queries::lookup_timestamp_by_text(
                &conn, &abbrev, &normalized,
            ) {
                let _ = cmd_tx.blocking_send(
                    crate::mpv::MpvCommand::ResumeAndSeek(start_time)
                );
            }
        }
    });

    true
}

/// Normalize text for database lookup: lowercase, strip square brackets
/// and their contents, collapse whitespace.
fn normalize_text(text: &str) -> String {
    let lowered = text.to_lowercase();
    // Strip bracket contents: [stage direction] → ""
    let mut result = String::new();
    let mut in_bracket = false;
    for ch in lowered.chars() {
        match ch {
            '[' => in_bracket = true,
            ']' => in_bracket = false,
            _ if !in_bracket => result.push(ch),
            _ => {}
        }
    }
    // Collapse whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_text() {
        assert_eq!(normalize_text("Hello World"), "hello world");
        assert_eq!(normalize_text("[Exit] HAMLET."), "hamlet.");
        assert_eq!(normalize_text("  spaces   between  "), "spaces between");
        assert_eq!(normalize_text("[Aside] To be, [or not]"), "to be,");
    }
}
```

- [ ] **Step 5: Create src/input/search.rs**

Copy from linux-lit `src/input/search.rs`. Remove `line_map` references — the buffer line is the work line. Keep `execute_search`, `toggle_playback`, `next_match`, `prev_match`, `clear_search`.

- [ ] **Step 6: Update main.rs to declare input module**

Add `mod input;`.

- [ ] **Step 7: Verify build and test**

```bash
cd ~/utono/mpv-linux-lit && cargo build && cargo test
```

- [ ] **Step 8: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/input/ src/main.rs
git commit -m "Add input module: keymap, navigation, timestamps, search"
```

---

### Task 9: Integration — Wire Everything Together

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`
- Modify: `src/input/keymap.rs`

At this point all modules exist. This task wires them together into a working application.

- [ ] **Step 1: Finalize main.rs**

Write the complete `main.rs` following linux-lit's pattern:

```rust
mod app;
mod concordance;
mod config;
mod db;
mod input;
mod logging;
mod mode;
mod mpv;
mod theme;
mod ui;

use gtk4::prelude::*;
use libadwaita as adw;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let log_filename = if mode::is_dev_mode() {
        "mpv-linux-lit-dev.log"
    } else {
        "mpv-linux-lit-release.log"
    };
    let log_path = format!("{}/utono/mpv-linux-lit/{}", home, log_filename);
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);

    let app_id = if mode::is_dev_mode() {
        "com.utono.mpv-linux-lit.dev"
    } else {
        "com.utono.mpv-linux-lit"
    };

    adw::init().expect("Failed to initialize libadwaita");

    let application = gtk4::Application::builder()
        .application_id(app_id)
        .build();

    application.connect_activate(|gtk_app| {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::channel::<MpvEvent>(32);

        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let tokio_handle = rt.handle().clone();
        std::thread::spawn(move || {
            rt.block_on(async move {
                let signal_evt_tx = evt_tx.clone();
                tokio::spawn(async move {
                    let mut sig = tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::user_defined1(),
                    )
                    .expect("Failed to register SIGUSR1 handler");
                    loop {
                        sig.recv().await;
                        let _ = signal_evt_tx.send(MpvEvent::ThemeChanged).await;
                    }
                });
                crate::mpv::client::run(cmd_rx, evt_tx).await;
            });
        });

        let works = {
            let conn = db::queries::open_db().expect("Failed to open lit.db");
            db::queries::list_works(&conn).expect("Failed to list works")
        };

        let config = config::load();
        let state = app::build_window(gtk_app, works, tokio_handle, config, cmd_tx);

        // Process MPV events
        let state_for_events = std::rc::Rc::clone(&state);
        glib::spawn_future_local(async move {
            while let Some(event) = evt_rx.recv().await {
                match event {
                    MpvEvent::ConnectionStatus(connected) => {
                        crate::logging::log(&format!("MPV connection: {}", connected));
                    }
                    MpvEvent::PlaybackState(playing) => {
                        crate::logging::log(&format!(
                            "MPV playback: {}",
                            if playing { "playing" } else { "paused" }
                        ));
                    }
                    MpvEvent::TimePos(pos) => {
                        let mut s = state_for_events.borrow_mut();
                        s.current_time_pos = pos;
                    }
                    MpvEvent::ThemeChanged => {
                        let mut s = state_for_events.borrow_mut();
                        let theme_name = crate::theme::current_theme_name();
                        let theme = if theme_name.is_empty() {
                            crate::theme::load_theme("gruvbox-material")
                        } else {
                            crate::theme::load_theme(&theme_name)
                        };
                        crate::input::keymap::apply_theme_to_state(&mut s, &theme);
                    }
                }
            }
        });

        let _ = state;
    });

    application.run();
}
```

- [ ] **Step 2: Finalize build_window in app.rs**

Ensure `build_window` creates all widgets, sets up the key controller, loads the last work from config (if any), and returns the state.

- [ ] **Step 3: Finalize keymap dispatch**

Ensure all keybindings from the spec are routed:
- `-` → `state.ring.cycle_next(); switch_to_ring_entry(state);`
- `_` → `state.ring.cycle_prev(); switch_to_ring_entry(state);`
- `Tab` → `timestamps::play_current_line_on_demand(state)`
- `,` → `navigation::jump_to_prev_dialogue(state)` (no seek)
- `q` → `navigation::jump_to_next_dialogue(state)` (no seek)
- All other keys as per spec table

- [ ] **Step 4: Verify full build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

Fix all remaining compilation errors iteratively.

- [ ] **Step 5: Run all tests**

```bash
cd ~/utono/mpv-linux-lit && cargo test
```

- [ ] **Step 6: Run clippy**

```bash
cd ~/utono/mpv-linux-lit && cargo clippy
```

Fix any warnings.

- [ ] **Step 7: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add -A
git commit -m "Wire all modules together into working application"
```

---

### Task 10: Dialogue Formatting

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Port dialogue formatting from linux-lit**

Copy the `apply_dialogue_formatting` function from linux-lit `src/app.rs`. This function:
- Scans buffer lines for speakers (using `db::line_types::is_speaker`)
- Applies indentation to dialogue lines
- Applies smallcaps tags to speaker names
- Inserts gaps between speaker changes

The function operates on the GTK buffer directly and uses TextTags. No `line_map` needed — it reads the buffer text line by line.

Ensure the necessary TextTags (`speaker_tag`, `indent_tag`, etc.) are created in `build_window` and stored in AppState.

- [ ] **Step 2: Call apply_dialogue_formatting from display_work**

After `buffer.set_text()` in `display_work`, call `apply_dialogue_formatting(state)`.

- [ ] **Step 3: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/app.rs
git commit -m "Add dialogue formatting (speaker detection, indentation, smallcaps)"
```

---

### Task 11: Vocab Highlighting

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Port vocab highlighting from linux-lit**

Copy `build_vocab_matches` and `apply_vocab_highlighting` from linux-lit `src/app.rs`. These functions:
- Scan the buffer for occurrences of vocab words (from `HashSet<String>`)
- Store matches as `Vec<VocabMatch>`
- Apply `vocab_tag` to matched ranges in the buffer

- [ ] **Step 2: Load vocab words during work loading**

In the work loading flow (when picker selects a work), after `load_work`:
```rust
let vocab_words = db::queries::load_vocab_words(&conn, &abbrev)?;
```

Store in `LoadedWork.vocab_words`.

- [ ] **Step 3: Call apply_vocab_highlighting from display_work**

After dialogue formatting, call `apply_vocab_highlighting(state)`.

- [ ] **Step 4: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 5: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/app.rs
git commit -m "Add vocab word highlighting"
```

---

### Task 12: Work Loading Flow (Picker → Load → Display)

**Files:**
- Modify: `src/app.rs`
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Implement load_selected_work in keymap**

When the user presses Enter in the library picker:
1. Get the selected abbreviation from the picker
2. Check if the work is already in the buffer ring — if so, switch to it
3. Otherwise, spawn a blocking task to load from DB:
   - `load_work(&conn, abbrev)`
   - Read the `.txt` file from disk (`work.text_file`)
   - `load_vocab_words(&conn, abbrev)`
   - `find_socket_for_work(&work.media_paths)` for MPV socket
4. Create `LoadedWork` and push to ring
5. Call `display_work`

```rust
fn load_selected_work(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let abbrev = {
        let s = state.borrow();
        match s.picker.selected_abbrev() {
            Some(a) => a,
            None => return,
        }
    };

    // Check ring first
    {
        let mut s = state.borrow_mut();
        if s.ring.add_or_switch(&abbrev).is_some() {
            s.picker.hide();
            switch_to_ring_entry(&mut s);
            return;
        }
    }

    let state_clone = Rc::clone(state);
    tokio_handle.spawn_blocking(move || {
        let conn = crate::db::queries::open_db().expect("open_db");
        let work = crate::db::queries::load_work(&conn, &abbrev).expect("load_work");
        let text = work.text_file.as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_else(|| work.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n"));
        let vocab_words = crate::db::queries::load_vocab_words(&conn, &abbrev).unwrap_or_default();
        let socket = crate::mpv::discovery::find_socket_for_work(&work.media_paths);
        (work, text, vocab_words, socket)
    });
    // ... receive result via channel and call display_work
```

The exact async callback pattern needs to follow linux-lit's approach of sending the result back to the GTK thread via `glib::spawn_future_local`.

- [ ] **Step 2: Handle MRU (most recently used) work on startup**

On startup, if `config.last_work` is set and the work exists in the works list, auto-load it.

- [ ] **Step 3: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/app.rs src/input/keymap.rs
git commit -m "Implement work loading flow: picker -> DB load -> display"
```

---

### Task 13: Translation Display

**Files:**
- Modify: `src/app.rs`
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Port translation loading and display**

Copy translation handling from linux-lit:
- Load translations via `db::queries::load_translations(&conn, abbrev)` during work load
- Store in `AppState.translations`
- Toggle visibility with a keybind (check linux-lit for the key — likely `t` or `Ctrl+t`)
- When visible, insert translation lines below each translated line in the buffer
- Apply `translation_dim_tag` and `translation_text_tag` for styling

- [ ] **Step 2: Verify build**

```bash
cd ~/utono/mpv-linux-lit && cargo build
```

- [ ] **Step 3: Commit**

```bash
cd ~/utono/mpv-linux-lit
git add src/app.rs src/input/keymap.rs
git commit -m "Add translation display"
```

---

### Task 14: Final Polish and Verification

**Files:**
- Various touch-ups across all modules

- [ ] **Step 1: Full build verification**

```bash
cd ~/utono/mpv-linux-lit && cargo build 2>&1
```

Must compile clean with no errors.

- [ ] **Step 2: Run all tests**

```bash
cd ~/utono/mpv-linux-lit && cargo test
```

All tests must pass.

- [ ] **Step 3: Run clippy**

```bash
cd ~/utono/mpv-linux-lit && cargo clippy -- -W clippy::all
```

Fix any warnings.

- [ ] **Step 4: Verify keybindings match spec**

Manually verify keymap dispatch covers every keybinding from the spec:
- j/k, h/l, gg/G, /, n/N, Space, [/], Tab, -/_, ,/q, r/R
- Ctrl+p, Ctrl+Shift+p, Ctrl+Alt+p, Ctrl+/, \, Ctrl+l, Ctrl+f, f/F

- [ ] **Step 5: Push to GitHub**

```bash
cd ~/utono/mpv-linux-lit && git push
```

- [ ] **Step 6: Final commit if any polish changes**

```bash
cd ~/utono/mpv-linux-lit
git add -A
git commit -m "Final polish: clippy fixes, test cleanup"
git push
```

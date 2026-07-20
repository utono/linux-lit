# Chat-Panel Space Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `space` in the chat transcript loops audio playback of the displayed
entry's source passage on a dedicated, write-only chat-panel MPV process,
leaving the main card's player untouched (paused while the loop runs, resumed
on exit).

**Architecture:** A new `src/mpv/chat_player.rs` module owns a second mpv
process on a `chat-`-marked socket and speaks fire-and-forget JSON IPC
(connect → write one command → close; no event loop, no channel into the
app). Source resolution reads the entry's own identity
(`ChatState.gloss_ctx`), never `AppState.current_work` for glossed entries —
forward-compatible with the planned cross-work `f` finder. Loop state lives
on `AppState` (NOT `ChatState`, which is `Default`-reset on panel close/work
switch) so teardown is always an explicit call, never a silent field drop.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite, mpv JSON IPC over
`UnixStream`, `serde_json` (already a dependency).

**Spec:** `docs/superpowers/specs/2026-07-20-chat-panel-space-loop-design.md`

## Global Constraints

- The spacebar keysym is `"space"` — never `"Space"`/`"spacebar"`.
- Do NOT run the app (`cargo run`) — build/test only; the user launches it.
- Every spawned long-lived `Command` must null stdin/stdout/stderr
  (inherited stdio holds the `crll` tee pipe open on exit).
- Chat transcript keys are hardcoded in `handle_chat_transcript_key` — no
  `keymap_config.rs` / `keymap.json` change for this feature.
- Every keybind change updates the surface's own Ctrl+/ legend — here that
  is `src/ui/chat_keybinds_overlay.rs` (`GROUPS`), NOT the reader overlay.
- The main-card work-switch wipe of `ChatState` (`chat::on_work_switched`)
  is deliberate and must stay.
- Commit after each task; run `cargo build` (and the named tests) before
  each commit.

---

### Task 1: Standalone timestamp queries (`line_end_time`, `next_start_after`)

**Files:**
- Modify: `src/db/queries.rs` (append after `line_start_time`, which ends at
  line 2214; add tests inside the existing `#[cfg(test)] mod tests` at
  line 2216)

**Interfaces:**
- Consumes: existing `line_start_time(conn, line_id, media_id) -> Option<f64>`
  (`src/db/queries.rs:2205`) as the pattern to mirror.
- Produces: `pub fn line_end_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64>`
  and `pub fn next_start_after(conn: &Connection, media_id: i64, t: f64) -> Option<f64>`
  — Task 5 calls both.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/db/queries.rs` (the module already has
in-memory-Connection tests to copy the shape from, e.g.
`vocab_highlight_migration_and_writer`):

```rust
    fn timestamps_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_timestamps (
                 id INTEGER PRIMARY KEY,
                 line_mapping_id INTEGER NOT NULL,
                 media_id INTEGER,
                 start_time REAL,
                 end_time REAL
             );
             INSERT INTO line_timestamps
                 (line_mapping_id, media_id, start_time, end_time) VALUES
                 (10, 1, 100.0, 103.5),
                 (11, 1, 104.0, NULL),
                 (12, 1, 108.0, 111.0),
                 (10, 2, 500.0, 502.0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn line_end_time_reads_the_media_scoped_row() {
        let conn = timestamps_test_conn();
        assert_eq!(line_end_time(&conn, 10, 1), Some(103.5));
        assert_eq!(line_end_time(&conn, 10, 2), Some(502.0));
        // NULL end_time and missing rows are both None, mirroring
        // line_start_time's contract.
        assert_eq!(line_end_time(&conn, 11, 1), None);
        assert_eq!(line_end_time(&conn, 99, 1), None);
    }

    #[test]
    fn next_start_after_is_the_earliest_strictly_later_start() {
        let conn = timestamps_test_conn();
        // After line 11's start (104.0) the next start on media 1 is 108.0.
        assert_eq!(next_start_after(&conn, 1, 104.0), Some(108.0));
        // Strictly after: a row AT t does not count.
        assert_eq!(next_start_after(&conn, 1, 108.0), None);
        // Media-scoped: media 2 has nothing after 502.
        assert_eq!(next_start_after(&conn, 2, 502.0), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --bin linux-lit line_end_time_reads -- --nocapture
```

Expected: compile error — `line_end_time` / `next_start_after` not found.

- [ ] **Step 3: Write the implementations**

Append after `line_start_time` (before `mod tests`) in `src/db/queries.rs`:

```rust
/// Look up a single line's end time for a given media file. Mirrors
/// `line_start_time`: None when no row exists OR the row's end_time is NULL.
pub fn line_end_time(conn: &Connection, line_id: i64, media_id: i64) -> Option<f64> {
    conn.query_row(
        "SELECT end_time FROM line_timestamps \
         WHERE line_mapping_id = ?1 AND media_id = ?2",
        rusqlite::params![line_id, media_id],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}

/// Earliest start_time on `media_id` strictly after `t` — the chat loop's
/// b-point fallback when a passage's last line has no end_time. Uses times,
/// not line ids, so no assumption about id ordering within a work.
pub fn next_start_after(conn: &Connection, media_id: i64, t: f64) -> Option<f64> {
    conn.query_row(
        "SELECT MIN(start_time) FROM line_timestamps \
         WHERE media_id = ?1 AND start_time > ?2",
        rusqlite::params![media_id, t],
        |row| row.get::<_, Option<f64>>(0),
    )
    .ok()
    .flatten()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --bin linux-lit line_end_time_reads next_start_after_is -- --nocapture
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): line_end_time + next_start_after timestamp readers"
```

---

### Task 2: Chat-marked socket path + reusable launcher

**Files:**
- Modify: `src/mpv/discovery.rs` (`derive_socket_path` at line 34,
  `launch_mpv` at line 144; tests in the existing `mod tests` at line 222)

**Interfaces:**
- Produces: `pub fn derive_socket_path_marked(media_path: &str, marker: &str) -> String`
  (existing `derive_socket_path` becomes a `marker: ""` wrapper — all current
  callers and socket shapes unchanged) and
  `pub fn launch_mpv_at(socket_path: &str, media_path: &str)` (existing
  `launch_mpv` becomes a derive-then-launch wrapper returning the socket
  path as before). Tasks 3/5 call both with `marker = "chat-"`.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src/mpv/discovery.rs`:

```rust
    #[test]
    fn test_derive_socket_path_marked_chat() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{}/Music/shakespeare-william/Hamlet.m4b", home);
        let socket = derive_socket_path_marked(&path, "chat-");
        // Slot 1 in unit tests → no instance infix; the marker sits where the
        // infix would extend: /tmp/mpvsocket-{infix}{marker}{author}-{basename}.
        assert!(socket.starts_with("/tmp/mpvsocket-chat-shakespeare-william-"));
        assert!(socket.contains("Hamlet.m4b"));
        // The unmarked path is a DIFFERENT socket — main-player discovery can
        // never probe/stale-clean the chat player's socket.
        assert_ne!(socket, derive_socket_path(&path));
        // Truncation cap still applies with a marker.
        let long = format!("{}/Music/author/{}.m4b", home, "a".repeat(100));
        assert!(derive_socket_path_marked(&long, "chat-").len() <= 95);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --bin linux-lit test_derive_socket_path_marked_chat -- --nocapture
```

Expected: compile error — `derive_socket_path_marked` not found.

- [ ] **Step 3: Refactor `derive_socket_path` and `launch_mpv`**

In `src/mpv/discovery.rs`, replace the body of `derive_socket_path`
(lines 34–63) with:

```rust
pub fn derive_socket_path(media_path: &str) -> String {
    derive_socket_path_marked(media_path, "")
}

/// Socket path with an extra `marker` segment after the instance infix
/// (e.g. "chat-" for the chat panel's dedicated player). A marked socket is
/// invisible to the main player's discovery, which only ever derives
/// unmarked paths — so probe/connect/stale-clean can't cross the streams.
pub fn derive_socket_path_marked(media_path: &str, marker: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let author = extract_author(media_path, &home);
    let basename = Path::new(media_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    // Per-instance namespace: "" for slot 1 (legacy paths, reattach
    // compatibility), "i{n}-" for slot n >= 2 — so discovery can only ever
    // find/connect/stale-clean THIS instance's players.
    let infix = crate::instance::socket_infix();

    let is_ytdlp = media_path.contains("/yt-dlp-mlj/");
    let socket_path = if is_ytdlp {
        format!("/tmp/mpvsocket-{}{}ytdlp-{}-{}", infix, marker, author, basename)
    } else {
        format!("/tmp/mpvsocket-{}{}{}-{}", infix, marker, author, basename)
    };

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
```

Then split `launch_mpv` (lines 144–201): keep the doc comments, make the
existing function a wrapper and move the body into `launch_mpv_at`:

```rust
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    launch_mpv_at(&socket_path, media_path);
    socket_path
}

/// Launch mpv listening on an explicit socket path (the chat player passes a
/// `chat-`-marked one). Same args and headless guards as `launch_mpv`.
pub fn launch_mpv_at(socket_path: &str, media_path: &str) {
    // LIT_NO_MPV: diagnostic toggle to launch with no MPV at all (A/B the startup
    // flicker against the MPV window-map). Same skip as the headless test path.
    if std::env::var_os("LIT_HEADLESS_TEST").is_some()
        || std::env::var_os("LIT_NO_MPV").is_some()
    {
        crate::logging::log(&format!(
            "MPV: skipped (LIT_HEADLESS_TEST/LIT_NO_MPV) for {}",
            media_path
        ));
        return;
    }
    ...
}
```

The `...` is the untouched remainder of the original body (the `bg`/`bg_args`
block and the `std::process::Command::new("mpv")` spawn with its existing
comments and the three `Stdio::null()` calls), with the two `return
socket_path;`/trailing `socket_path` expressions deleted and `socket_path`
usages switched to the `&str` parameter.

- [ ] **Step 4: Run tests to verify they pass (including the pre-existing socket tests)**

```bash
cargo test --bin linux-lit derive_socket_path -- --nocapture
```

Expected: `test_derive_socket_path_marked_chat`, `test_derive_socket_path_music`,
`test_derive_socket_path_ytdlp`, `test_derive_socket_path_truncation`,
`test_socket_infix_splices_after_prefix` all PASS (the unmarked shapes must
be byte-identical to before).

- [ ] **Step 5: Commit**

```bash
git add src/mpv/discovery.rs
git commit -m "refactor(mpv): marker-aware socket derivation + explicit-socket launcher"
```

---

### Task 3: `chat_player.rs` — write-only player module + source resolution

**Files:**
- Create: `src/mpv/chat_player.rs`
- Modify: `src/mpv/mod.rs` (add `pub mod chat_player;` next to the existing
  `pub mod discovery;` line)

**Interfaces:**
- Consumes: `discovery::launch_mpv_at` (Task 2), `crate::db::models::MediaItem`
  (`media_id: i64, path: String, display_name: Option<String>, priority: i64`),
  `crate::gloss::GlossContext` (`work_abbrev: String`,
  `source_line_numbers: Vec<i64>`),
  `crate::input::segments::SegmentContext` (`cursor_lines: Vec<Line>`, each
  `Line` has `id: i64`).
- Produces (Tasks 4–6 call these):
  - `pub struct ChatPlayer { pub socket_path: String, pub media_path: String }`
    with `toggle_pause(&self)`, `stop_loop(&self)`, `quit(&self)`
  - `#[derive(Default)] pub struct ChatLoopState { pub armed: bool, pub paused: bool, pub main_was_playing: bool }`
  - `pub struct LoopSource { pub work_abbrev: String, pub first_line_id: i64, pub last_line_id: i64 }`
  - `pub fn loop_source_from(gloss_ctx: Option<&GlossContext>, pinned: Option<&SegmentContext>, current_abbrev: Option<&str>) -> Option<LoopSource>`
  - `pub fn pick_default_media(items: &[MediaItem]) -> Option<MediaItem>`
  - `pub fn arm_command_json(media_path: &str, a: f64, b: Option<f64>) -> String`
  - `pub fn spawn_and_arm(socket_path: String, media_path: String, a: f64, b: Option<f64>)`

- [ ] **Step 1: Write the failing tests**

Create `src/mpv/chat_player.rs` with only a `#[cfg(test)]` module for now
(the impl arrives in Step 3), and register it in `src/mpv/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::MediaItem;

    fn media(path: &str) -> MediaItem {
        MediaItem { media_id: 1, path: path.to_string(), display_name: None, priority: 0 }
    }

    #[test]
    fn pick_default_media_prefers_arkangel_else_first() {
        let plain = media("/home/x/Music/a/plain.m4b");
        let ark = media("/home/x/Music/a/aax-Arkangel/ham.m4b");
        assert_eq!(
            pick_default_media(&[plain.clone(), ark.clone()]).unwrap().path,
            ark.path
        );
        assert_eq!(pick_default_media(&[plain.clone()]).unwrap().path, plain.path);
        assert!(pick_default_media(&[]).is_none());
    }

    #[test]
    fn arm_command_is_one_loadfile_with_per_file_options() {
        let with_loop = arm_command_json("/m/a.m4b", 100.5, Some(112.25));
        assert_eq!(
            with_loop,
            r#"{"command":["loadfile","/m/a.m4b","replace",-1,"start=100.500,ab-loop-a=100.500,ab-loop-b=112.250,pause=no"]}"#
        );
        // No b-point: play once from a, no ab-loop options at all.
        let once = arm_command_json("/m/a.m4b", 100.5, None);
        assert_eq!(
            once,
            r#"{"command":["loadfile","/m/a.m4b","replace",-1,"start=100.500,pause=no"]}"#
        );
    }

    #[test]
    fn loop_source_prefers_gloss_ctx_own_work() {
        let ctx = crate::gloss::GlossContext {
            work_abbrev: "BH-Barrett".into(),
            work_title: String::new(),
            start_citation: String::new(),
            end_citation: String::new(),
            act: 0,
            scene: 0,
            speaker: String::new(),
            source_text: String::new(),
            source_line_numbers: vec![41, 42, 43],
            hash: String::new(),
            gloss_type: String::new(),
        };
        // gloss_ctx wins even when current_work says otherwise — the entry's
        // own identity, the cross-work `f` finder's contract.
        let src = loop_source_from(Some(&ctx), None, Some("TGV-Amb")).unwrap();
        assert_eq!(src.work_abbrev, "BH-Barrett");
        assert_eq!((src.first_line_id, src.last_line_id), (41, 43));
        // Empty line list → unresolvable, not a bogus 0..0 range.
        let empty = crate::gloss::GlossContext { source_line_numbers: vec![], ..ctx };
        assert!(loop_source_from(Some(&empty), None, Some("TGV-Amb")).is_none());
        // Nothing pinned at all → None.
        assert!(loop_source_from(None, None, Some("TGV-Amb")).is_none());
    }
}
```

Note: `GlossContext` derives `Clone` but not `Debug`; the struct-update
syntax above (`..ctx`) consumes `ctx` last, which is fine. `SegmentContext`'s
fallback branch is covered in Step 3's doc test-free logic and exercised at
the integration level (constructing a `Line` in a unit test drags in the
whole models module — the branch is three lines reading `first()`/`last()`
ids, same shape as the gloss branch asserted here).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --bin linux-lit chat_player -- --nocapture
```

Expected: compile error — the functions don't exist yet.

- [ ] **Step 3: Write the module**

Fill in `src/mpv/chat_player.rs` above the test module:

```rust
//! Dedicated chat-panel MPV player: a SECOND mpv process on a `chat-`-marked
//! socket, driven write-only (connect → one JSON command → close). It has no
//! event loop and no channel into the app, so it can never feed the main
//! player's cursor-sync engine. Spawned lazily by the transcript's `space`
//! loop; quit whenever focus leaves the transcript (see the design doc
//! docs/superpowers/specs/2026-07-20-chat-panel-space-loop-design.md).

use std::io::Write;
use std::os::unix::net::UnixStream;

use crate::db::models::MediaItem;
use crate::gloss::GlossContext;
use crate::input::segments::SegmentContext;

/// Handle to the chat panel's mpv process. Lives on `AppState.chat_player`
/// (NOT ChatState, which is Default-reset on panel close/work switch — the
/// process must be quit explicitly, never silently dropped).
pub struct ChatPlayer {
    pub socket_path: String,
    pub media_path: String,
}

/// State machine for the transcript `space` loop. On `AppState` for the same
/// reason as `ChatPlayer`. `main_was_playing` is captured at arm time and
/// preserved across nav-stops so a later full exit still restores correctly.
#[derive(Default)]
pub struct ChatLoopState {
    pub armed: bool,
    pub paused: bool,
    pub main_was_playing: bool,
}

/// The displayed entry's source passage, resolved from the ENTRY's own
/// identity.
pub struct LoopSource {
    pub work_abbrev: String,
    pub first_line_id: i64,
    pub last_line_id: i64,
}

/// Resolve the source passage for the loop. `gloss_ctx` (the entry's own
/// record, carrying its work) wins; a raw not-yet-glossed pin falls back to
/// `cursor_lines` + `current_abbrev` — safe because a raw pin is same-work
/// by construction (every main-card work switch wipes the panel). NEVER
/// resolve a glossed entry from current_work: the future cross-work `f`
/// finder will pin other works' entries.
pub fn loop_source_from(
    gloss_ctx: Option<&GlossContext>,
    pinned: Option<&SegmentContext>,
    current_abbrev: Option<&str>,
) -> Option<LoopSource> {
    if let Some(ctx) = gloss_ctx {
        let first = *ctx.source_line_numbers.first()?;
        let last = *ctx.source_line_numbers.last()?;
        return Some(LoopSource {
            work_abbrev: ctx.work_abbrev.clone(),
            first_line_id: first,
            last_line_id: last,
        });
    }
    let p = pinned?;
    let first = p.cursor_lines.first()?.id;
    let last = p.cursor_lines.last()?.id;
    Some(LoopSource {
        work_abbrev: current_abbrev?.to_string(),
        first_line_id: first,
        last_line_id: last,
    })
}

/// Default media for a work: prefer Arkangel, else the highest-priority row
/// (list_media_for_work orders by priority DESC). The play_selected_echo rule.
pub fn pick_default_media(items: &[MediaItem]) -> Option<MediaItem> {
    items
        .iter()
        .find(|m| m.path.contains("/aax-Arkangel/"))
        .cloned()
        .or_else(|| items.first().cloned())
}

/// The single loadfile that arms the whole loop: per-file options seek to
/// `a`, set the a-b loop, and unpause atomically on load (loadfile-replace
/// would clear ab-loop props set beforehand — see MpvCommand::LoadFileSeekAndLoop).
/// Argument order is mpv >= 0.38 (url, flags, index, options); -1 = "no index".
pub fn arm_command_json(media_path: &str, a: f64, b: Option<f64>) -> String {
    let opts = match b {
        Some(b) => format!("start={:.3},ab-loop-a={:.3},ab-loop-b={:.3},pause=no", a, a, b),
        None => format!("start={:.3},pause=no", a),
    };
    format!(
        r#"{{"command":["loadfile",{},"replace",-1,{}]}}"#,
        serde_json::to_string(media_path).unwrap_or_default(),
        serde_json::to_string(&opts).unwrap_or_default(),
    )
}

/// Fire one command at the socket and hang up. A fresh connection per
/// command means mpv never accumulates an unread event backlog for us.
fn send_json(socket_path: &str, json: &str) {
    match UnixStream::connect(socket_path) {
        Ok(mut stream) => {
            let _ = stream.write_all(json.as_bytes());
            let _ = stream.write_all(b"\n");
        }
        Err(e) => {
            crate::logging::log(&format!("CHAT-MPV: send failed ({}): {}", socket_path, e))
        }
    }
}

impl ChatPlayer {
    pub fn toggle_pause(&self) {
        send_json(&self.socket_path, r#"{"command":["cycle","pause"]}"#);
    }

    /// Disarm the a-b loop and pause; the process stays for reuse.
    pub fn stop_loop(&self) {
        send_json(&self.socket_path, r#"{"command":["set_property","ab-loop-a","no"]}"#);
        send_json(&self.socket_path, r#"{"command":["set_property","ab-loop-b","no"]}"#);
        send_json(&self.socket_path, r#"{"command":["set_property","pause",true]}"#);
    }

    pub fn quit(&self) {
        send_json(&self.socket_path, r#"{"command":["quit"]}"#);
    }
}

/// Ensure the chat mpv is running on `socket_path` and arm the loop. Runs on
/// a detached thread: a first launch waits up to ~3s for the IPC socket
/// (mirroring discover_or_launch_blocking), which must never block the GTK
/// thread. State was already updated optimistically by the caller.
pub fn spawn_and_arm(socket_path: String, media_path: String, a: f64, b: Option<f64>) {
    std::thread::spawn(move || {
        if UnixStream::connect(&socket_path).is_err() {
            let _ = std::fs::remove_file(&socket_path); // stale leftover, if any
            crate::mpv::discovery::launch_mpv_at(&socket_path, &media_path);
            for _ in 0..60 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if UnixStream::connect(&socket_path).is_ok() {
                    break;
                }
            }
        }
        send_json(&socket_path, &arm_command_json(&media_path, a, b));
        crate::logging::log(&format!(
            "CHAT-MPV: armed a={:.2} b={:?} media={}",
            a, b, media_path
        ));
    });
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --bin linux-lit chat_player -- --nocapture
```

Expected: 3 tests PASS. Also `cargo build` clean.

- [ ] **Step 5: Commit**

```bash
git add src/mpv/chat_player.rs src/mpv/mod.rs
git commit -m "feat(mpv): write-only chat-panel player module + loop-source resolution"
```

---

### Task 4: `MpvCommand::Resume` for the main player

**Files:**
- Modify: `src/mpv/commands.rs` (enum at line 6, after `Pause` at line 11)
- Modify: `src/mpv/client.rs` (command dispatch; add an arm after
  `MpvCommand::Pause`'s at lines 142–146)

**Interfaces:**
- Produces: `MpvCommand::Resume` — resume without seeking (the existing
  `ResumeAndSeek` always seeks; `TogglePause` is stateful). Task 5's exit
  path sends it.

- [ ] **Step 1: Add the variant**

In `src/mpv/commands.rs` after `Pause,`:

```rust
    /// Resume without seeking (pause=no). Counterpart of `Pause` for the
    /// chat-loop exit path, which must restore playback exactly where the
    /// arm-time `Pause` left it.
    Resume,
```

- [ ] **Step 2: Add the client arm**

In `src/mpv/client.rs` directly after the `MpvCommand::Pause` arm:

```rust
        MpvCommand::Resume => {
            if let Some(w) = writer.as_mut() {
                let _ = send_command(w, r#"{"command":["set_property","pause",false]}"#).await;
            }
        }
```

- [ ] **Step 3: Build**

```bash
cargo build
```

Expected: clean (the enum is `#[allow(dead_code)]`, so an unused variant
cannot warn; Task 5 uses it).

- [ ] **Step 4: Commit**

```bash
git add src/mpv/commands.rs src/mpv/client.rs
git commit -m "feat(mpv): Resume command (pause=no, no seek)"
```

---

### Task 5: The `space` handler — arm / pause-toggle

**Files:**
- Modify: `src/app/mod.rs` (AppState struct ~line 706 area, its init
  ~line 2148 area)
- Modify: `src/input/actions/chat.rs` (new functions at the end of the file)
- Modify: `src/input/keymap.rs` (`handle_chat_transcript_key`, add an arm
  before the final `_ => true` at line 1689)

**Interfaces:**
- Consumes: Task 1 queries, Task 2 `derive_socket_path_marked`, Task 3
  module, Task 4 `MpvCommand::{Pause, Resume}`, existing
  `crate::db::queries::{open_db, list_media_for_work, line_start_time}`,
  `crate::input::navigation::{show_chapter_toast_secs, preroll_seek_time}`,
  `AppState.mpv_playing: bool`, `AppState.cmd_tx`.
- Produces: `chat::toggle_source_loop(state: &Rc<RefCell<AppState>>)`
  (the `space` entry point), `chat::chat_loop_stop(s: &mut AppState)` and
  `chat::chat_loop_teardown(s: &mut AppState)` (Task 6 wires these into the
  exit paths), and two new `AppState` fields.

- [ ] **Step 1: Add the AppState fields**

In `src/app/mod.rs`, next to `pub mpv_playing: bool` (line 706):

```rust
    /// Transcript `space` loop state (see mpv::chat_player). On AppState, not
    /// ChatState: ChatState is Default-reset on panel close/work switch and
    /// the chat mpv process must be quit explicitly, never silently dropped.
    pub chat_loop: crate::mpv::chat_player::ChatLoopState,
    /// Handle to the chat panel's dedicated mpv process, if one was spawned.
    pub chat_player: Option<crate::mpv::chat_player::ChatPlayer>,
```

And in the AppState initializer (the struct literal containing
`mpv_playing: false` at line 2148):

```rust
        chat_loop: Default::default(),
        chat_player: None,
```

- [ ] **Step 2: Write the handlers in `chat.rs`**

Append at the end of `src/input/actions/chat.rs`:

```rust
/// `space` in the transcript: loop playback of the displayed entry's source
/// passage on the DEDICATED chat mpv (never the main player). Armed → plain
/// pause toggle. See the design doc
/// docs/superpowers/specs/2026-07-20-chat-panel-space-loop-design.md.
pub(crate) fn toggle_source_loop(state: &Rc<RefCell<AppState>>) {
    // Already armed: space is the pause toggle, nothing else.
    {
        let mut s = state.borrow_mut();
        if s.chat_loop.armed {
            if let Some(p) = &s.chat_player {
                p.toggle_pause();
            }
            s.chat_loop.paused = !s.chat_loop.paused;
            return;
        }
    }

    // Resolve the entry's OWN source work + line range (never current_work
    // for a glossed entry — the future cross-work `f` finder relies on it).
    let src = {
        let s = state.borrow();
        crate::mpv::chat_player::loop_source_from(
            s.chat.gloss_ctx.as_ref(),
            s.chat.pinned_passage.as_ref(),
            s.current_work.as_ref().map(|w| w.abbrev.as_str()),
        )
    };
    let Some(src) = src else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No source passage to play", 2);
        return;
    };

    // Default media + loop points, all standalone DB reads (the work need
    // not be loaded in the main card).
    let Ok(conn) = crate::db::queries::open_db() else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "Database unavailable", 2);
        return;
    };
    let media = crate::db::queries::list_media_for_work(&conn, &src.work_abbrev)
        .ok()
        .and_then(|items| crate::mpv::chat_player::pick_default_media(&items));
    let Some(media) = media else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            &format!("No media for {}", src.work_abbrev),
            2,
        );
        return;
    };
    let Some(start) =
        crate::db::queries::line_start_time(&conn, src.first_line_id, media.media_id)
    else {
        let s = state.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No timestamps for this passage", 2);
        return;
    };
    // b-point: last line's end_time, else the next start AFTER the last
    // line's own start, else play once (no loop).
    let last_start = crate::db::queries::line_start_time(&conn, src.last_line_id, media.media_id)
        .unwrap_or(start);
    let b = crate::db::queries::line_end_time(&conn, src.last_line_id, media.media_id)
        .or_else(|| crate::db::queries::next_start_after(&conn, media.media_id, last_start));
    // Loop from a hair before the first line (preroll), every pass.
    let a = crate::input::navigation::preroll_seek_time(start);

    let mut s = state.borrow_mut();
    if b.is_none() {
        crate::input::navigation::show_chapter_toast_secs(
            &s,
            "No end timestamp \u{2014} playing once",
            2,
        );
    }
    // Silence the main player for the duration; remember whether to restore.
    s.chat_loop.main_was_playing = s.mpv_playing;
    if s.mpv_playing {
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Pause);
    }
    // One chat mpv at a time: a different media path derives a different
    // socket, so quit any old process before pointing the handle elsewhere.
    let socket = crate::mpv::discovery::derive_socket_path_marked(&media.path, "chat-");
    if let Some(old) = &s.chat_player {
        if old.socket_path != socket {
            old.quit();
        }
    }
    s.chat_player = Some(crate::mpv::chat_player::ChatPlayer {
        socket_path: socket.clone(),
        media_path: media.path.clone(),
    });
    s.chat_loop.armed = true;
    s.chat_loop.paused = false;
    crate::logging::log(&format!(
        "CHAT-LOOP: arm {} lines {}..{} a={:.2} b={:?} media={}",
        src.work_abbrev, src.first_line_id, src.last_line_id, a, b, media.path
    ));
    crate::mpv::chat_player::spawn_and_arm(socket, media.path, a, b);
}

/// Nav-stop: disarm the loop but keep the chat mpv process AND keep the main
/// player paused — the user is likely about to `space` the next entry.
/// `main_was_playing` is preserved so a later full exit still restores.
pub(crate) fn chat_loop_stop(s: &mut AppState) {
    if !s.chat_loop.armed {
        return;
    }
    if let Some(p) = &s.chat_player {
        p.stop_loop();
    }
    s.chat_loop.armed = false;
    s.chat_loop.paused = false;
    crate::logging::log("CHAT-LOOP: stopped (nav)");
}

/// Full teardown: disarm, QUIT the chat mpv process, and resume the main
/// player iff it was playing at arm time. Every path that leaves the
/// transcript funnels here (Escape's focus_reader, panel close, work
/// switch, save-and-quit). Idempotent — safe to call with nothing armed.
pub(crate) fn chat_loop_teardown(s: &mut AppState) {
    let was_armed = s.chat_loop.armed;
    if let Some(p) = s.chat_player.take() {
        p.quit();
    }
    s.chat_loop.armed = false;
    s.chat_loop.paused = false;
    if was_armed && s.chat_loop.main_was_playing {
        let _ = s.cmd_tx.try_send(crate::mpv::MpvCommand::Resume);
    }
    s.chat_loop.main_was_playing = false;
    if was_armed {
        crate::logging::log("CHAT-LOOP: teardown");
    }
}
```

- [ ] **Step 3: Add the `space` arm to the transcript handler**

In `src/input/keymap.rs`, inside `handle_chat_transcript_key`'s `match`,
directly before the final `_ => true` arm (line 1689):

```rust
        // `space`: loop playback of the displayed entry's source passage on
        // the chat panel's DEDICATED mpv (armed → pause toggle). The global
        // reader space guard only intercepts in InputMode::Reader, so the
        // key reaches this arm untouched.
        "space" => {
            crate::input::actions::chat::toggle_source_loop(state);
            true
        }
```

- [ ] **Step 4: Build and run the full unit suite**

```bash
cargo build && cargo test --bin linux-lit
```

Expected: build clean, all tests pass (no behavior change is reachable in
tests yet — teardown wiring lands in Task 6).

- [ ] **Step 5: Commit**

```bash
git add src/app/mod.rs src/input/actions/chat.rs src/input/keymap.rs
git commit -m "feat(chat): space loops the entry's source passage on a dedicated mpv"
```

---

### Task 6: Exit wiring — nav-stop, Escape, panel close, work switch, quit

**Files:**
- Modify: `src/input/actions/chat.rs` (`close_chat_layout` line 263,
  `on_work_switched` line 308, `focus_reader` line 657, `cycle_gloss`
  line 1295, `toggle_panel_view` line 1521, `transcript_cursor_move`
  line 1889, `transcript_cursor_first` line 2013, `transcript_cursor_last`
  line 2072)
- Modify: `src/input/keymap.rs` (transcript Escape arm line 1681; the
  Shift+Ctrl+L emergency quit at lines 103–108; the `SaveAndQuit` action arm
  at lines 4116–4120)

**Interfaces:**
- Consumes: `chat_loop_stop` / `chat_loop_teardown` (Task 5).
- Produces: nothing new — this task is pure wiring.

- [ ] **Step 1: Nav-stop hooks**

Navigation that changes the DISPLAYED ENTRY disarms the loop (main stays
paused). Row-cursor moves within one entry do NOT stop it.

`cycle_gloss` (line 1295): insert after the `if n <= 1 { return; }` guard
(line 1301–1303) and before the `gloss_index` update — the two early
returns above it (Journal-view toast, nothing-to-cycle) don't change the
entry and must not stop the loop:

```rust
    chat_loop_stop(s);
```

`toggle_panel_view` (line 1521): the view flip changes what's displayed.
Insert directly after the `let mut s = state_rc.borrow_mut();` at line 1528
(NOT before the `gloss_ctx` early-return above it, which displays nothing
new):

```rust
    chat_loop_stop(&mut s);
```

`transcript_cursor_move` (line 1889): the Journal-view branch derives the
new entry at lines 1931–1933:

```rust
        if let Some(&entry) = s.chat.journal_row_owner.get(new_cursor) {
            s.chat.journal_cursor = entry;
        }
```

Capture `let prev_entry = s.chat.journal_cursor;` as the branch's first
statement (right after `if s.chat.view == PanelView::Journal {`, line 1905)
and insert after the derivation above, before `render_journal_view_inner`:

```rust
        if s.chat.journal_cursor != prev_entry {
            chat_loop_stop(s);
        }
```

The Gloss and Question branches of `transcript_cursor_move` get NO stop —
j/k there steps rows within the same displayed entry.

`transcript_cursor_first` (line 2013) and `transcript_cursor_last`
(line 2072): same rule, applied mechanically without depending on the body's
internals — capture `let prev_entry = s.chat.journal_cursor;` as the first
line of each function, and append as the last statement before every
`return`/fall-off end:

```rust
    if s.chat.view == PanelView::Journal && s.chat.journal_cursor != prev_entry {
        chat_loop_stop(s);
    }
```

- [ ] **Step 2: Escape + leave-transcript teardown**

In `src/input/keymap.rs`, the transcript `"Escape"` arm (line 1681) —
teardown BEFORE the existing visual-exit/focus_reader logic:

```rust
        "Escape" => {
            let mut s = state.borrow_mut();
            crate::input::actions::chat::chat_loop_teardown(&mut s);
            if crate::input::actions::chat::exit_transcript_visual(&mut s) {
                return true;
            }
            crate::input::actions::chat::focus_reader(&mut s);
            true
        }
```

In `chat.rs`, `focus_reader` (line 657) — first line of the body (Tab to
reader also leaves the transcript; teardown is idempotent so the doubled
call on the Escape path is harmless):

```rust
    chat_loop_teardown(s);
```

- [ ] **Step 3: Panel close + work switch**

In `close_chat_layout` (line 263), immediately after the
`if !s.chat_layout_open { return; }` guard:

```rust
    chat_loop_teardown(s);
```

In `on_work_switched` (line 308), immediately after its
`if !s.chat_layout_open { return; }` guard (the wipe below it resets
`ChatState`, so the teardown must run first):

```rust
    chat_loop_teardown(s);
```

- [ ] **Step 4: App-exit paths**

Focus can only sit in the transcript while a loop runs, and both quit paths
reachable from there must kill the chat mpv too:

Shift+Ctrl+L emergency quit (`src/input/keymap.rs:103–108`) — add the
teardown before the window closes:

```rust
    if is_shift && is_ctrl && (key_name == "L" || key_name == "l") {
        crate::app::save_position(&mut state.borrow_mut());
        crate::input::actions::chat::chat_loop_teardown(&mut state.borrow_mut());
        let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
        state.borrow().window.close();
        return true;
    }
```

`SaveAndQuit` action arm (`src/input/keymap.rs:4116–4120`) — same insertion
after `save_position`:

```rust
        SaveAndQuit => {
            crate::app::save_position(&mut state.borrow_mut());
            crate::input::actions::chat::chat_loop_teardown(&mut state.borrow_mut());
            let _ = state.borrow().cmd_tx.try_send(crate::mpv::MpvCommand::Quit);
            state.borrow().window.close();
        }
```

- [ ] **Step 5: Build, full unit suite, clippy**

```bash
cargo build && cargo test --bin linux-lit && cargo clippy
```

Expected: build + tests green; clippy introduces no NEW warnings (the
project has pre-existing warning classes).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/chat.rs src/input/keymap.rs
git commit -m "feat(chat): loop nav-stop + teardown wiring on every transcript exit"
```

---

### Task 7: Ctrl+/ legend entry + final verification

**Files:**
- Modify: `src/ui/chat_keybinds_overlay.rs` (the `GROUPS` const, line 10)

**Interfaces:**
- Consumes: the Task 5/6 behavior (legend text must match it exactly).

- [ ] **Step 1: Add the legend row**

In the `("Transcript actions", &[...])` group of `GROUPS`, after the
`("y", ...)` row:

```rust
        ("space", "loop the entry's source audio · armed: pause/resume"),
```

- [ ] **Step 2: Build + full test suite**

```bash
cargo build && cargo test --bin linux-lit && cargo clippy
```

Expected: all green, no new clippy warnings.

- [ ] **Step 3: Commit**

```bash
git add src/ui/chat_keybinds_overlay.rs
git commit -m "docs(ui): chat legend row for the transcript space loop"
```

- [ ] **Step 4: Manual acceptance hand-off (do NOT run the app yourself)**

Audio cannot be verified under the headless harness (`LIT_NO_MPV` skips mpv
entirely). Hand the user this script and ask them to run it in their live
session:

1. Open a work with media + timestamps; select a passage (`V`), `Tab` to
   pin the panel, `-` to gloss it. Start main playback, note the position.
2. `space` in the transcript: the passage loops audibly on a SECOND mpv
   (`pgrep -af mpv` shows a `/tmp/mpvsocket-chat-*` `--input-ipc-server`);
   the main player is paused, its position untouched.
3. `space` again: loop pauses. Again: resumes.
4. `Ctrl+n` (cycle gloss): loop stops, main stays paused.
5. `space`, then `Escape`: loop tears down, main RESUMES (it was playing at
   arm time) — and the chat mpv process is gone after the panel closes
   (`-` or `Ctrl+Tab`), verify with `pgrep -af "mpvsocket-chat"`.
6. Re-pin, `space`, switch works via the library picker: panel wipes (as
   always), loop tears down, chat mpv quits, main resumes.
7. Edge: a passage whose last line has no end_time (check the toast "No end
   timestamp — playing once" appears and playback does not loop).

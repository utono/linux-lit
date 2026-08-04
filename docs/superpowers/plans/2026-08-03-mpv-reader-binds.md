# MPV Reader Binds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Six keys in the linux-lit-launched MPV window (`,` `q` `[` `{` `b` `B`) reach the reader's own navigation and timestamp handlers, turning MPV into a remote control for the reader.

**Architecture:** A lit-owned `--input-conf` binds those six keys to `script-message` commands. MPV emits each as a `client-message` event on the IPC socket linux-lit already reads line-by-line. A new parser turns it into a new `MpvEvent::ReaderAction(..)` variant, and a new match arm in `main.rs` dispatches to the exact same functions `keymap.rs` calls for the reader keys. No new transport, no new socket, no polling.

**Tech Stack:** Rust, tokio (async IPC read loop), GTK4/glib (event dispatch on the UI thread), serde_json (event parsing), MPV IPC JSON protocol.

**Spec:** `docs/superpowers/specs/2026-08-03-mpv-reader-binds-design.md`

## Global Constraints

- The reader's own binds are UNCHANGED. This adds a second surface reaching existing handlers; it does not add, move, or remove any reader keybind. Therefore `src/input/keymap_config.rs`, `src/ui/keybinds_overlay.rs`, and the stowed `keymap.json` are NOT modified.
- `navigation.rs` and `timestamps.rs` are NOT modified. Parity must be structural — call the existing functions, never copy their logic.
- The sync gate for `b`/`B` lives in the `main.rs` dispatch arm, NOT in `timestamps.rs`. The reader's own `b`/`B` stay unconditional.
- Only the reading player gets the binds. Add `--input-conf` in `launch_mpv()` only, never in the shared `launch_mpv_at()` (the chat snippet player calls the latter).
- When sync is off, `b`/`B` from MPV are silently dropped: log only, no toast, no OSD, no write.
- Verify with `cargo build`; do NOT run the app with `cargo run` — the user launches it.
- All new log lines use `crate::logging::log`.

## File Structure

- **Create `assets/mpv-input.conf`** — the six-line MPV keybind override. Ships with the repo so it cannot drift from the reader.
- **Modify `src/mpv/commands.rs`** — add the `ReaderAction` enum and the `MpvEvent::ReaderAction` variant. Owns the vocabulary shared between the parser and the dispatcher.
- **Modify `src/mpv/client.rs`** — add `parse_client_message` beside the existing `parse_time_pos` / `parse_pause_state`, and wire it into the read loop. Owns socket-line parsing.
- **Modify `src/mpv/discovery.rs`** — resolve the input.conf path and pass `--input-conf` in `launch_mpv()`. Owns process launch.
- **Modify `src/main.rs`** — one new match arm dispatching to existing handlers, with the sync gate. Owns UI-thread dispatch.

Task order follows the data flow inward-out: vocabulary, then parser, then dispatch, then launch. Each task builds and tests on its own.

---

### Task 1: ReaderAction vocabulary

**Files:**
- Modify: `src/mpv/commands.rs:45-51` (the `MpvEvent` enum)

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `pub enum ReaderAction` with variants `PrevSpeaker`, `NextSpeaker`, `PrevDivision`, `NextDivision`, `SetStartTime`, `UndoTimestamp`, all deriving `Debug, Clone, Copy, PartialEq, Eq`. Also `MpvEvent::ReaderAction(ReaderAction)`. Task 2 constructs these; Task 4 matches on them.

- [ ] **Step 1: Add the ReaderAction enum and MpvEvent variant**

In `src/mpv/commands.rs`, immediately above the existing `MpvEvent` enum (which starts at line 45 with its `/// Events sent from the Tokio runtime back to the GTK UI thread.` doc comment), add:

```rust
/// A reader action requested from the MPV window's own keyboard, delivered
/// as an mpv `script-message` -> `client-message` on the IPC socket. Each
/// variant maps to the SAME handler the reader's own keybind calls, so the
/// two surfaces cannot drift. See
/// `docs/superpowers/specs/2026-08-03-mpv-reader-binds-design.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderAction {
    /// mpv `,` — previous speaker turn.
    PrevSpeaker,
    /// mpv `q` — next speaker turn.
    NextSpeaker,
    /// mpv `[` — previous division (scene/chapter boundary).
    PrevDivision,
    /// mpv `{` — next division.
    NextDivision,
    /// mpv `b` — write the current playback position to the cursor's line.
    /// Gated on `sync_enabled` at dispatch (the reader's own `b` is not).
    SetStartTime,
    /// mpv `B` — undo the last timestamp write. Same sync gate as above.
    UndoTimestamp,
}
```

Then add this variant to the existing `MpvEvent` enum, after the `ThemeChanged` line:

```rust
    /// A key pressed in the MPV window that drives the reader (Task 1).
    ReaderAction(ReaderAction),
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds successfully. The `MpvEvent` enum already carries `#[allow(dead_code)]`, so the new unused variant produces no warning.

- [ ] **Step 3: Commit**

```bash
git add src/mpv/commands.rs
git commit -m "feat(mpv): add ReaderAction vocabulary for mpv-side reader binds"
```

---

### Task 2: Parse client-message events

**Files:**
- Modify: `src/mpv/client.rs` (add `parse_client_message` next to `parse_pause_state`, which ends around line 380; add tests to the existing `mod tests`)

**Interfaces:**
- Consumes: `ReaderAction` from Task 1.
- Produces: `fn parse_client_message(line: &str) -> Option<ReaderAction>` — a private module-level function returning `Some` only for the six known message names. Task 3 calls it.

- [ ] **Step 1: Write the failing tests**

In `src/mpv/client.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block (it begins around line 460 and already has `use super::*;`), add:

```rust
    #[test]
    fn test_parse_client_message_all_six() {
        let cases = [
            ("lit-prev-speaker", ReaderAction::PrevSpeaker),
            ("lit-next-speaker", ReaderAction::NextSpeaker),
            ("lit-prev-division", ReaderAction::PrevDivision),
            ("lit-next-division", ReaderAction::NextDivision),
            ("lit-set-start-time", ReaderAction::SetStartTime),
            ("lit-undo-timestamp", ReaderAction::UndoTimestamp),
        ];
        for (name, expected) in cases {
            let line = format!(r#"{{"event":"client-message","args":["{}"]}}"#, name);
            assert_eq!(
                parse_client_message(&line),
                Some(expected),
                "failed to parse {}",
                name
            );
        }
    }

    #[test]
    fn test_parse_client_message_rejects_others() {
        // A different script's message on the same socket.
        assert_eq!(
            parse_client_message(r#"{"event":"client-message","args":["some-other-script"]}"#),
            None
        );
        // Right event, no args.
        assert_eq!(
            parse_client_message(r#"{"event":"client-message","args":[]}"#),
            None
        );
        // A different event entirely.
        assert_eq!(
            parse_client_message(r#"{"event":"property-change","name":"time-pos","data":1.0}"#),
            None
        );
        // Not JSON at all.
        assert_eq!(parse_client_message("not json"), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bins parse_client_message 2>&1 | tail -20`
Expected: FAIL to compile with `cannot find function 'parse_client_message' in this scope`.

- [ ] **Step 3: Write the implementation**

In `src/mpv/client.rs`, add the import for `ReaderAction` by changing the existing line 7:

```rust
use super::commands::{MpvCommand, MpvEvent};
```

to:

```rust
use super::commands::{MpvCommand, MpvEvent, ReaderAction};
```

Then add this function immediately after the existing `parse_pause_state` function (which ends just before `fn is_file_loaded_event`):

```rust
/// Parse an mpv `client-message` event into the reader action it requests.
/// The lit-owned input.conf (`assets/mpv-input.conf`) binds six keys to
/// `script-message lit-*`, and mpv relays each as a client-message on the
/// IPC socket we already read. Unknown names return `None` so other
/// scripts' messages on this socket are ignored.
fn parse_client_message(line: &str) -> Option<ReaderAction> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("event")?.as_str()? != "client-message" {
        return None;
    }
    match v.get("args")?.as_array()?.first()?.as_str()? {
        "lit-prev-speaker" => Some(ReaderAction::PrevSpeaker),
        "lit-next-speaker" => Some(ReaderAction::NextSpeaker),
        "lit-prev-division" => Some(ReaderAction::PrevDivision),
        "lit-next-division" => Some(ReaderAction::NextDivision),
        "lit-set-start-time" => Some(ReaderAction::SetStartTime),
        "lit-undo-timestamp" => Some(ReaderAction::UndoTimestamp),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bins parse_client_message 2>&1 | tail -20`
Expected: PASS, 2 tests. A `function is never used` warning for `parse_client_message` is expected here and disappears in Task 3.

- [ ] **Step 5: Commit**

```bash
git add src/mpv/client.rs
git commit -m "feat(mpv): parse lit-* client-message events into ReaderAction"
```

---

### Task 3: Emit ReaderAction from the read loop

**Files:**
- Modify: `src/mpv/client.rs:86-92` (the `Ok(_)` branch of the socket read loop, after the `parse_pause_state` block)

**Interfaces:**
- Consumes: `parse_client_message` from Task 2, `MpvEvent::ReaderAction` from Task 1.
- Produces: `MpvEvent::ReaderAction(..)` on the existing `evt_tx` channel. Task 4 receives it.

- [ ] **Step 1: Wire the parser into the read loop**

In `src/mpv/client.rs`, find this existing block inside the `Ok(_) => { ... }` arm of the read loop (it is the last `if let` in that arm, around line 88):

```rust
                            if let Some(paused) = parse_pause_state(&line_buf) {
                                let _ = evt_tx.send(MpvEvent::PlaybackState(!paused)).await;
                            }
```

Add immediately after it, still inside the same `Ok(_)` arm:

```rust
                            if let Some(action) = parse_client_message(&line_buf) {
                                crate::logging::log(&format!(
                                    "MPV_BIND: {:?} from mpv window",
                                    action
                                ));
                                let _ = evt_tx.send(MpvEvent::ReaderAction(action)).await;
                            }
```

- [ ] **Step 2: Verify it compiles with no dead-code warning**

Run: `cargo build 2>&1 | rg -c 'parse_client_message' ; cargo build 2>&1 | tail -10`
Expected: the `rg -c` prints `0` (no warning mentioning the function any more), and the build succeeds.

- [ ] **Step 3: Run the full unit suite**

Run: `cargo test --bins 2>&1 | tail -15`
Expected: PASS, no regressions.

- [ ] **Step 4: Commit**

```bash
git add src/mpv/client.rs
git commit -m "feat(mpv): emit ReaderAction events from the IPC read loop"
```

---

### Task 4: Dispatch to the reader handlers, with the sync gate

**Files:**
- Modify: `src/main.rs:642-666` (add a new arm after the existing `MpvEvent::ThemeChanged` arm, before the closing `}` of the `match event`)

**Interfaces:**
- Consumes: `MpvEvent::ReaderAction(ReaderAction)` from Tasks 1 and 3.
- Produces: calls into `crate::input::navigation` and `crate::input::timestamps`. Terminal task for the event path.

**Critical borrow rule:** every handler takes `&mut AppState` and does its own
`borrow_mut()`-scoped work plus its own redraw. This arm must NOT hold a
borrow across a handler call, or the app aborts on a `RefCell` double-borrow.
Read `sync_enabled` inside a short scope that ends before dispatching.

- [ ] **Step 1: Add the dispatch arm**

In `src/main.rs`, find the end of the `MpvEvent::ThemeChanged` arm — it ends with this line followed by the closing braces of the arm and the `match`:

```rust
                        crate::input::actions::settings::apply_theme_to_state(&mut s, &theme);
                    }
```

Add a new arm immediately after that arm's closing `}`:

```rust
                    MpvEvent::ReaderAction(action) => {
                        use crate::mpv::commands::ReaderAction as RA;
                        // b/B are gated on playback sync: from the MPV window
                        // the reader's cursor is invisible, and sync-on is
                        // what makes its position predictable enough to write
                        // to blind. The reader's OWN b/B stay unconditional —
                        // this gate is deliberately here, not in timestamps.rs.
                        if matches!(action, RA::SetStartTime | RA::UndoTimestamp)
                            && !state_for_events.borrow().sync_enabled
                        {
                            crate::logging::log(&format!(
                                "MPV_BIND: {:?} ignored — sync off",
                                action
                            ));
                            continue;
                        }
                        // No borrow may be held here: each handler takes
                        // &mut AppState and borrows for itself.
                        let mut s = state_for_events.borrow_mut();
                        match action {
                            RA::PrevSpeaker => {
                                crate::input::navigation::jump_to_prev_speaker(&mut s)
                            }
                            RA::NextSpeaker => {
                                crate::input::navigation::jump_to_next_speaker(&mut s)
                            }
                            RA::PrevDivision => {
                                crate::input::navigation::jump_to_prev_section(&mut s)
                            }
                            RA::NextDivision => {
                                crate::input::navigation::jump_to_next_section(&mut s)
                            }
                            RA::SetStartTime => {
                                crate::input::timestamps::set_start_time(&mut s);
                            }
                            RA::UndoTimestamp => {
                                crate::input::timestamps::undo_timestamp(&mut s);
                            }
                        }
                    }
```

Note the gate reads `sync_enabled` through a temporary `borrow()` that ends at
the end of the `if` condition, so the later `borrow_mut()` is safe.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: builds successfully.

- [ ] **Step 3: Run the full unit suite and clippy**

Run: `cargo test --bins 2>&1 | tail -15 && cargo clippy 2>&1 | tail -20`
Expected: tests PASS; clippy reports no new warnings for `src/main.rs`, `src/mpv/client.rs`, or `src/mpv/commands.rs`.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(mpv): dispatch mpv-window reader binds to the reader handlers"
```

---

### Task 5: Ship the input.conf and pass it at launch

**Files:**
- Create: `assets/mpv-input.conf`
- Modify: `src/mpv/discovery.rs:162-166` (the `launch_mpv` function) and `src/mpv/discovery.rs:201-235` (the arg chain in `launch_mpv_at`)

**Interfaces:**
- Consumes: the message names Task 2 parses (`lit-prev-speaker`, `lit-next-speaker`, `lit-prev-division`, `lit-next-division`, `lit-set-start-time`, `lit-undo-timestamp`). These strings MUST match Task 2 exactly.
- Produces: nothing consumed by later tasks. Terminal task.

**Scoping rule:** `launch_mpv_at` is shared with the chat snippet player
(`src/mpv/chat_player.rs:252`). The input.conf must reach the reading player
ONLY, so `launch_mpv_at` gains an explicit parameter rather than reading a
global.

- [ ] **Step 1: Create the input.conf**

Create `assets/mpv-input.conf` with exactly this content:

```
# linux-lit reader binds for the MPV instance the reader launches.
#
# Merged OVER the user's ~/.config/mpv/input.conf via --input-conf, so every
# other bind in that file still works here (a=pause, o/e and O/E seek,
# DEL and Ctrl+L quit, all lua scripts). Only these six lines are overridden,
# and only in the lit reading player — never the chat snippet player.
#
# Each line sends a script-message that mpv relays as a client-message on the
# IPC socket linux-lit already reads; src/mpv/client.rs parses it and
# src/main.rs dispatches to the SAME handler the reader's own key calls.
# Message names here must match parse_client_message() in src/mpv/client.rs.
#
# Note: q no longer quits in this window — use DEL or Ctrl+L.
# Note: [ and { give up their +/-5s seek here; o/e (2s) and O/E (15s) remain.
#
# Keys are RPD glyphs (the layout at ~/utono/rpd): [ and { are the unshifted
# QWERTY-2/3 caps, matching the unshifted-symbol pattern of , and q.

, script-message lit-prev-speaker
q script-message lit-next-speaker
[ script-message lit-prev-division
{ script-message lit-next-division
b script-message lit-set-start-time
B script-message lit-undo-timestamp
```

- [ ] **Step 2: Add the path resolver and thread the flag through**

In `src/mpv/discovery.rs`, add this function immediately above `pub fn launch_mpv`:

```rust
/// Absolute path to the repo's `assets/mpv-input.conf`, or `None` if it is
/// missing. Resolved next to the running binary first (release/installed
/// layout: `<dir>/assets/`, then `<dir>/../../assets/` for
/// `target/debug/linux-lit`), falling back to `CARGO_MANIFEST_DIR` for dev
/// runs. Missing file is NOT an error — mpv simply launches with the user's
/// own binds, exactly as before this feature.
fn reader_input_conf() -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("assets/mpv-input.conf"));
            candidates.push(dir.join("../../assets/mpv-input.conf"));
        }
    }
    candidates.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/mpv-input.conf"),
    );
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}
```

Change `launch_mpv` (currently at line 162) from:

```rust
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    launch_mpv_at(&socket_path, media_path);
    socket_path
}
```

to:

```rust
pub fn launch_mpv(media_path: &str) -> String {
    let socket_path = derive_socket_path(media_path);
    // `true`: the READING player gets the reader binds. The chat snippet
    // player calls launch_mpv_at directly with `false` — it has no reader
    // cursor of its own, so driving the reader from it would move the
    // cursor mid-chat and timestamp a snippet position into lit.db.
    launch_mpv_at(&socket_path, media_path, true);
    socket_path
}
```

Change the `launch_mpv_at` signature (line 170) from:

```rust
pub fn launch_mpv_at(socket_path: &str, media_path: &str) {
```

to:

```rust
pub fn launch_mpv_at(socket_path: &str, media_path: &str, reader_binds: bool) {
```

Immediately after the `bg_args` block (which ends with the closing `};` of the
`let bg_args: Vec<String> = if bg.is_empty() { ... };` statement, just before
the `match std::process::Command::new("mpv")` call), add:

```rust
    // The lit-owned input.conf: six keys that drive the reader. Reading
    // player only. Empty (no flag) when absent or for the chat player.
    let input_conf_args: Vec<String> = match reader_binds.then(reader_input_conf).flatten() {
        Some(path) => {
            crate::logging::log(&format!("MPV: reader binds via {}", path));
            vec![format!("--input-conf={}", path)]
        }
        None => Vec::new(),
    };
```

Then add the args to the command chain by inserting this line immediately
after the existing `.args(&bg_args)` line:

```rust
        .args(&input_conf_args)
```

Finally, in `src/mpv/chat_player.rs:252`, change:

```rust
            crate::mpv::discovery::launch_mpv_at(&socket_path, &media_path);
```

to:

```rust
            // `false`: no reader binds — see launch_mpv's note.
            crate::mpv::discovery::launch_mpv_at(&socket_path, &media_path, false);
```

- [ ] **Step 3: Write a test that the shipped conf matches the parser**

In `src/mpv/client.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
    /// The shipped input.conf and the parser must agree on all six message
    /// names — a typo in either would silently dead-key that bind.
    #[test]
    fn test_shipped_input_conf_matches_parser() {
        let conf = include_str!("../../assets/mpv-input.conf");
        let mut found = 0;
        for line in conf.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let msg = line
                .split("script-message ")
                .nth(1)
                .unwrap_or_else(|| panic!("non-comment line is not a script-message: {}", line))
                .trim();
            let fake = format!(r#"{{"event":"client-message","args":["{}"]}}"#, msg);
            assert!(
                parse_client_message(&fake).is_some(),
                "input.conf sends '{}' but the parser does not know it",
                msg
            );
            found += 1;
        }
        assert_eq!(found, 6, "expected exactly 6 binds in assets/mpv-input.conf");
    }
```

- [ ] **Step 4: Run the tests and build**

Run: `cargo test --bins 2>&1 | tail -15 && cargo build 2>&1 | tail -10 && cargo clippy 2>&1 | tail -20`
Expected: all tests PASS including `test_shipped_input_conf_matches_parser`; build succeeds; no new clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add assets/mpv-input.conf src/mpv/discovery.rs src/mpv/chat_player.rs src/mpv/client.rs
git commit -m "feat(mpv): ship the reader input.conf and pass it for the reading player"
```

---

### Task 6: End-to-end verification

**Files:** none modified. This task produces evidence, not code.

**Interfaces:**
- Consumes: the complete feature from Tasks 1-5.
- Produces: a verification report.

The `,`/`q`/`[`/`{` navigation and the `b`/`B` writes need a REAL mpv window
with a REAL keyboard, which the headless cage harness cannot supply (it drives
the reader's GTK surface, not mpv's). Per CLAUDE.md the on-screen check is
mandatory and cannot be waived, so this task ends in a hand-off with exact
steps rather than a headless run.

- [ ] **Step 1: Verify the launch flag is actually passed**

Run: `rg -n 'input-conf' src/mpv/discovery.rs`
Expected: shows the `--input-conf={}` format string in the `input_conf_args` block.

Run: `rg -n 'launch_mpv_at' src/ -g '*.rs'`
Expected: exactly three hits — the definition, the `true` call in `launch_mpv`, and the `false` call in `chat_player.rs:252`.

- [ ] **Step 2: Verify the conf resolves from a debug binary layout**

Run: `ls -l assets/mpv-input.conf && ls -d target/debug/linux-lit`
Expected: both exist. `target/debug/linux-lit`'s parent is `target/debug`, so the `../../assets/mpv-input.conf` candidate resolves to the repo's `assets/`.

- [ ] **Step 3: Full green check**

Run: `cargo build 2>&1 | tail -5 && cargo test --bins 2>&1 | tail -10 && cargo clippy 2>&1 | tail -10`
Expected: build OK, tests PASS, clippy clean.

- [ ] **Step 4: Hand off the manual on-screen check**

Report to the user these exact steps:

1. Launch the reader (`crll`) and open a work with audio and timestamps.
2. Focus the MPV window.
3. Press `q` then `,` — the reader's cursor should step to the next/previous
   speaker turn and MPV should seek to that line.
4. Press `{` then `[` — the cursor should step to the next/previous division.
5. With playback sync ON, position the cursor on a line, play until you hear
   it begin, and press `b` — the timestamp should be written. Press `B` to
   undo it.
6. Turn playback sync OFF and press `b` — nothing should happen. Confirm with
   `rg 'MPV_BIND.*sync off' linux-lit-dev.log`.
7. Confirm `DEL` or `Ctrl+L` still quits the MPV window (`q` no longer does).

Also give them the log check: `rg 'MPV_BIND|MPV: reader binds' linux-lit-dev.log`

- [ ] **Step 5: Final commit if anything changed**

Only if Steps 1-3 surfaced a fix. Otherwise skip — the feature was committed in Tasks 1-5.

---

## Self-Review

**Spec coverage:**
- Six binds, exact keys and meanings — Task 5 (input.conf), Task 1 (vocabulary).
- Lit-owned input.conf merged over the user's — Task 5.
- `--input-conf` in `launch_mpv` only; chat player excluded — Task 5, verified in Task 6 Step 1.
- `parse_client_message` beside the existing parsers — Task 2.
- New `MpvEvent` variant on the existing channel — Task 1, emitted Task 3.
- Dispatch to the four navigation + two timestamp handlers by name — Task 4.
- Sync gate in `main.rs`, not `timestamps.rs`; silent drop with a log — Task 4.
- Reader binds, overlay, and keymap.json unchanged — Global Constraints; no task touches them.
- Unit tests for all six messages plus a negative case — Task 2.
- Sync-gate test — see the note below.
- Live-only end-to-end check — Task 6.

**Gap found and closed:** the spec asks for a unit test that
`ReaderAction(SetStartTime)` with `sync_enabled = false` performs no write.
That gate lives inside a `glib::spawn_future_local` closure in `main.rs` and
is not callable from a unit test without extracting it. Rather than restructure
`main.rs` for testability, the gate is verified in Task 6 Step 4 item 6 via the
`MPV_BIND: ... ignored — sync off` log line. This is a deliberate deviation
from the spec's testing section, recorded here.

**Placeholder scan:** none — every code step carries complete code.

**Type consistency:** `ReaderAction` variants are spelled identically in Tasks
1, 2, and 4. Message strings are identical in Tasks 2 and 5, and Task 5 Step 3
adds a test that enforces exactly that. `launch_mpv_at`'s three-argument form
is consistent across Task 5's definition and both call sites.

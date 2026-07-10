# Multi-instance linux-lit (crll) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `crll` open two or more linux-lit instances, each with its own MPV player(s), per the approved design in `docs/plans/2026-07-09-multi-instance-crll-design.md`.

**Architecture:** A new `src/instance.rs` module flock-assigns each process a slot number (1, 2, ...) at startup. The slot namespaces MPV socket names, the app log filename, and the window title. GTK gets the `NON_UNIQUE` flag so a second process actually launches. `config::save()` becomes read-merge-write so concurrent instances stop clobbering each other's per-work state.

**Tech Stack:** Rust (edition 2021, rustc 1.89 — `std::fs::File::try_lock` is stable as of exactly 1.89), GTK4/gio, serde_json, zsh (alias file).

## Global Constraints

- **Slot 1 must be byte-identical to today's behavior**: no suffix in socket paths, log filenames, or window title. Existing tests and tooling must pass unchanged.
- **No new crate dependencies.** Slot locking uses `std::fs::File::try_lock` (stable in rustc 1.89, the installed toolchain).
- **Never run the app** (`cargo run` / launching the binary) — build and test only; the user runs it live. Headless verification via the documented cage flow is allowed.
- **Do not edit `~/.config/linux-lit/config-dev.json`** (a running instance clobbers it on exit).
- The `crll` alias lives in `~/utono/shell-config/.config/shell/alias-mlj` — a **separate git repo** (`~/utono/shell-config`), committed independently of linux-lit.
- Verification gates per task: `cargo build` must succeed; `cargo test --bins` must pass.

---

### Task 1: Instance slot module (`src/instance.rs`)

**Files:**

- Create: `src/instance.rs`
- Modify: `src/main.rs:1-20` (module list)
- Test: unit tests inside `src/instance.rs` (`cargo test --bins`)

**Interfaces:**

- Consumes: nothing (std only).
- Produces:
  - `instance::acquire() -> u32` — called once, first thing in `main()`.
  - `instance::slot() -> u32` — the held slot; returns 1 if `acquire` was never called (unit tests), so all suffix helpers default to legacy names.
  - `instance::socket_infix() -> String` — `""` for slot 1, `"i{n}-"` for slot n ≥ 2 (Task 2 splices it into socket paths).
  - `instance::socket_infix_for(slot: u32) -> String` — pure, unit-testable form.

- [ ] **Step 1: Write the failing test**

Create `src/instance.rs` containing ONLY the test module for the pure function (the implementation comes in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_one_has_no_infix() {
        assert_eq!(socket_infix_for(1), "");
    }

    #[test]
    fn higher_slots_get_i_n_infix() {
        assert_eq!(socket_infix_for(2), "i2-");
        assert_eq!(socket_infix_for(10), "i10-");
    }

    #[test]
    fn slot_defaults_to_one_without_acquire() {
        // Unit tests never call acquire(); every consumer must see slot 1
        // (legacy behavior) in that state.
        assert_eq!(slot(), 1);
        assert_eq!(socket_infix(), "");
    }
}
```

Register the module in `src/main.rs` — in the `mod` list at the top (after `mod input;`), add:

```rust
mod instance;
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins instance:: 2>&1 | tail -20
```

Expected: compile FAILURE — `cannot find function socket_infix_for in this scope` (the module has tests but no implementation yet).

- [ ] **Step 3: Write the implementation**

Prepend to `src/instance.rs` (above the test module):

```rust
//! Per-process instance slot assignment.
//!
//! Each linux-lit process auto-assigns itself a slot number (1, 2, 3, ...)
//! at startup by flock-ing a per-slot lock file under $XDG_RUNTIME_DIR.
//! The locked File is held in a OnceLock for the process lifetime, so the
//! OS releases the slot on any exit including crashes — no stale-slot
//! cleanup exists or is needed.
//!
//! Slot 1 must stay byte-identical to the pre-multi-instance app: no
//! suffix in socket paths, log filenames, or the window title.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SLOT: OnceLock<u32> = OnceLock::new();
static LOCK_FILE: OnceLock<File> = OnceLock::new();

/// Highest slot probed before degrading to slot 1.
const MAX_SLOTS: u32 = 64;

fn slot_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("linux-lit");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    PathBuf::from(format!("/tmp/linux-lit-{}", user))
}

fn try_lock_slot(dir: &Path, n: u32) -> Option<File> {
    let path = dir.join(format!("slot-{}.lock", n));
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        // WouldBlock (held by another instance) or IO error — either way
        // this slot is unavailable.
        Err(_) => None,
    }
}

/// Acquire the lowest free slot. Called once, first thing in `main()`,
/// BEFORE `logging::init` (the log filename depends on the slot), so it
/// cannot log — `main()` logs the outcome right after logging comes up.
pub fn acquire() -> u32 {
    let dir = slot_dir();
    if fs::create_dir_all(&dir).is_err() {
        // Pathological: degrade to slot 1 (today's behavior) rather than
        // refusing to start.
        SLOT.set(1).ok();
        return 1;
    }
    // LIT_INSTANCE pins a slot for deterministic test/debug runs. If the
    // requested slot is already locked, fall through to the normal scan.
    if let Ok(v) = std::env::var("LIT_INSTANCE") {
        if let Ok(n) = v.parse::<u32>() {
            if n >= 1 {
                if let Some(f) = try_lock_slot(&dir, n) {
                    LOCK_FILE.set(f).ok();
                    SLOT.set(n).ok();
                    return n;
                }
            }
        }
    }
    for n in 1..=MAX_SLOTS {
        if let Some(f) = try_lock_slot(&dir, n) {
            LOCK_FILE.set(f).ok();
            SLOT.set(n).ok();
            return n;
        }
    }
    SLOT.set(1).ok();
    1
}

/// The slot this process holds. 1 when `acquire` was never called (unit
/// tests) so every suffix helper defaults to legacy names.
pub fn slot() -> u32 {
    SLOT.get().copied().unwrap_or(1)
}

/// Socket-name infix for `derive_socket_path`: "" for slot 1 (legacy
/// paths unchanged), "i{n}-" for slot n >= 2.
pub fn socket_infix_for(slot: u32) -> String {
    if slot <= 1 {
        String::new()
    } else {
        format!("i{}-", slot)
    }
}

pub fn socket_infix() -> String {
    socket_infix_for(slot())
}
```

`acquire()` itself is deliberately NOT unit-tested: it reads process env and locks real files, and parallel test threads share both. It is covered by the headless verification in Task 6.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins instance:: 2>&1 | tail -5
```

Expected: `test result: ok. 3 passed`. Also expect a `dead_code` warning for `acquire`/`slot_dir` — that disappears in Tasks 2–3 when callers land; do not suppress it with attributes.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/instance.rs src/main.rs && git commit -m "feat: instance slot module — flock-based per-process slot assignment"
```

---

### Task 2: Per-slot MPV socket namespacing

**Files:**

- Modify: `src/mpv/discovery.rs:16-29` (`derive_socket_path`)
- Test: existing tests in `src/mpv/discovery.rs` plus one new one

**Interfaces:**

- Consumes: `crate::instance::socket_infix()` (Task 1).
- Produces: `derive_socket_path` unchanged signature; slot-1 output byte-identical to today, slot-n output `/tmp/mpvsocket-i{n}-...`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/mpv/discovery.rs` (alongside `test_derive_socket_path_music`):

```rust
#[test]
fn test_socket_infix_splices_after_prefix() {
    // derive_socket_path always uses the process slot (1 in unit tests →
    // no infix, asserted by the existing tests). The slot-n shape is
    // pinned here via the pure helper so a format drift can't slip in.
    let infix = crate::instance::socket_infix_for(2);
    let path = format!("/tmp/mpvsocket-{}shakespeare-william-Hamlet.m4b", infix);
    assert_eq!(path, "/tmp/mpvsocket-i2-shakespeare-william-Hamlet.m4b");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins test_socket_infix 2>&1 | tail -5
```

Expected: PASS already (it pins the format contract only) — the real change is Step 3; the guard tests are the EXISTING `test_derive_socket_path_*` tests, which must still pass after Step 3 proving slot-1 output is unchanged.

- [ ] **Step 3: Splice the infix into `derive_socket_path`**

In `src/mpv/discovery.rs`, replace the body of `derive_socket_path` (currently lines 16–29, the part before the 95-char truncation block):

```rust
pub fn derive_socket_path(media_path: &str) -> String {
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
        format!("/tmp/mpvsocket-{}ytdlp-{}-{}", infix, author, basename)
    } else {
        format!("/tmp/mpvsocket-{}{}-{}", infix, author, basename)
    };
```

The 95-char truncation block that follows (`if socket_path.len() > 95 { ... }`) stays exactly as is — it hashes whatever name it receives, so suffixed paths truncate deterministically per slot.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins derive_socket 2>&1 | tail -5
```

Expected: all existing `test_derive_socket_path_*` tests PASS (slot 1 in unit tests → empty infix → legacy paths byte-identical).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/mpv/discovery.rs && git commit -m "feat: namespace MPV socket paths by instance slot"
```

---

### Task 3: NON_UNIQUE application, per-slot log file, window title suffix

**Files:**

- Modify: `src/main.rs:26-42` (log path selection), `src/main.rs:62-72` (Application builder)
- Modify: `src/app/mod.rs:2809-2811` (`set_title` site)

**Interfaces:**

- Consumes: `instance::acquire()`, `instance::slot()` (Task 1).
- Produces: process-level wiring only; nothing downstream consumes it.

- [ ] **Step 1: Wire slot acquisition + per-slot log filename in `main()`**

In `src/main.rs`, the current opening of `main()` is:

```rust
fn main() {
    // Clear and set up log file
    let home = std::env::var("HOME").unwrap_or_default();
    // LIT_LOG_PATH lets an isolated run (e.g. the headless nav-fuzz) write to its
    // own log instead of clobbering a live `cargo run` session's dev log.
    let log_path = if let Ok(p) = std::env::var("LIT_LOG_PATH") {
        p
    } else {
        let log_filename = if mode::is_dev_mode() {
            "linux-lit-dev.log"
        } else {
            "linux-lit-release.log"
        };
        format!("{}/utono/linux-lit/{}", home, log_filename)
    };
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);
    crate::logging::log("STARTUP: main entry");
```

Replace with:

```rust
fn main() {
    // Acquire the per-process instance slot FIRST — the log filename and
    // all MPV socket names derive from it. Slot 1 == today's behavior.
    let slot = instance::acquire();

    // Clear and set up log file
    let home = std::env::var("HOME").unwrap_or_default();
    // LIT_LOG_PATH lets an isolated run (e.g. the headless nav-fuzz) write to its
    // own log instead of clobbering a live `cargo run` session's dev log.
    let log_path = if let Ok(p) = std::env::var("LIT_LOG_PATH") {
        p
    } else {
        let base = if mode::is_dev_mode() {
            "linux-lit-dev"
        } else {
            "linux-lit-release"
        };
        let log_filename = if slot == 1 {
            format!("{}.log", base)
        } else {
            format!("{}-{}.log", base, slot)
        };
        format!("{}/utono/linux-lit/{}", home, log_filename)
    };
    let _ = std::fs::write(&log_path, "");
    logging::init(&log_path);
    crate::logging::log("STARTUP: main entry");
    crate::logging::log_always(&format!("INSTANCE: slot {}", slot));
    if let Ok(req) = std::env::var("LIT_INSTANCE") {
        if req != slot.to_string() {
            crate::logging::log_always(&format!(
                "INSTANCE: LIT_INSTANCE={} unavailable, fell back to slot {}",
                req, slot
            ));
        }
    }
```

- [ ] **Step 2: Add the NON_UNIQUE flag**

In `src/main.rs`, the builder currently reads:

```rust
    let application = gtk4::Application::builder()
        .application_id(app_id)
        .build();
```

Replace with:

```rust
    // NON_UNIQUE: each launch gets its own GApplication. Without it a second
    // process forwards activate to the running instance over D-Bus and exits
    // — the hard blocker for multi-instance. The app id is unchanged, so the
    // dwl tag-3 window rule keeps matching.
    let application = gtk4::Application::builder()
        .application_id(app_id)
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
```

- [ ] **Step 3: Window title suffix for slot > 1**

In `src/app/mod.rs` (around line 2809), the current code is:

```rust
    state
        .window
        .set_title(Some(&format!("{} — linux-lit", work.title)));
```

Replace with:

```rust
    let slot = crate::instance::slot();
    let window_title = if slot > 1 {
        format!("{} — linux-lit [{}]", work.title, slot)
    } else {
        format!("{} — linux-lit", work.title)
    };
    state.window.set_title(Some(&window_title));
```

- [ ] **Step 4: Build and test**

Run:

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3
```

Expected: build succeeds (the Task 1 `dead_code` warning is gone — `acquire` now has a caller); all bin tests pass.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit && git add src/main.rs src/app/mod.rs && git commit -m "feat: NON_UNIQUE app, per-slot log file and window title"
```

---

### Task 4: Config merge-on-save

**Files:**

- Modify: `src/config.rs` (statics + `mark_work_dirty` + `push_recent_work` + `merge_configs` + `save`)
- Modify: `src/app/mod.rs:2782-2790` (work-switch position save), `src/app/mod.rs:~3940-3949` (current-work position save), `src/app/mod.rs:~867-877` (`record_last_gloss`)
- Modify: `docs/plans/2026-07-09-multi-instance-crll-design.md` (addendum, Step 7)
- Test: unit tests in `src/config.rs`

**Interfaces:**

- Consumes: nothing from other tasks (independent of slots — correct even single-instance).
- Produces:
  - `config::mark_work_dirty(abbrev: &str)` — call wherever a per-work config entry is written.
  - `merge_configs(ours: &Config, disk: Config, dirty: &[String], session_recent: &[String]) -> Config` (private, unit-tested).
  - `save()` — same signature, now read-merge-write.

**Design addendum (spec deviation, deliberate):** the design doc names `work_positions`; implementation reality has THREE per-work maps that a stale exit would clobber — `work_positions` (legacy line-number fallback), `work_position_ids` (line-id keyed, primary), and `last_gloss`. All three get the same dirty-key overlay. Step 7 records this in the design doc.

- [ ] **Step 1: Write the failing tests**

Add to `src/config.rs`, next to the existing `last_gloss_tests` module:

```rust
#[cfg(test)]
mod merge_tests {
    use super::*;

    fn cfg() -> Config {
        serde_json::from_str("{}").unwrap()
    }

    #[test]
    fn other_instances_positions_survive() {
        // Ours loaded Ham=5 at startup (stale) and only touched BH.
        // Disk meanwhile has Ham=99 (the other instance's newer save).
        let mut ours = cfg();
        ours.work_positions.insert("Ham".into(), 5);
        ours.work_positions.insert("BH".into(), 10);
        let mut disk = cfg();
        disk.work_positions.insert("Ham".into(), 99);
        disk.work_positions.insert("BH".into(), 1);
        let merged = merge_configs(&ours, disk, &["BH".into()], &[]);
        assert_eq!(merged.work_positions["Ham"], 99); // disk wins: not dirty
        assert_eq!(merged.work_positions["BH"], 10); // ours wins: dirty
    }

    #[test]
    fn ours_only_undirty_key_is_kept() {
        // A key the disk lost entirely (e.g. truncated file) is restored
        // from our snapshot even when not dirty.
        let mut ours = cfg();
        ours.work_position_ids.insert("Oth".into(), 42);
        let merged = merge_configs(&ours, cfg(), &[], &[]);
        assert_eq!(merged.work_position_ids["Oth"], 42);
    }

    #[test]
    fn last_gloss_merges_by_dirty_key() {
        let mut ours = cfg();
        ours.last_gloss.insert(
            "Ham".into(),
            LastGloss { start_citation: "Ham.1.1.1".into(), gloss_type: "reader-gloss".into() },
        );
        let mut disk = cfg();
        disk.last_gloss.insert(
            "Ham".into(),
            LastGloss { start_citation: "Ham.5.2.300".into(), gloss_type: "reader-gloss".into() },
        );
        let not_dirty = merge_configs(&ours, disk.clone(), &[], &[]);
        assert_eq!(not_dirty.last_gloss["Ham"].start_citation, "Ham.5.2.300");
        let dirty = merge_configs(&ours, disk, &["Ham".into()], &[]);
        assert_eq!(dirty.last_gloss["Ham"].start_citation, "Ham.1.1.1");
    }

    #[test]
    fn recent_works_session_opens_go_first() {
        let mut disk = cfg();
        disk.recent_works = vec!["Ham".into(), "Oth".into()];
        let merged = merge_configs(&cfg(), disk, &[], &["BH".into()]);
        assert_eq!(merged.recent_works, vec!["BH", "Ham", "Oth"]);
    }

    #[test]
    fn scalars_are_last_writer_wins() {
        let mut ours = cfg();
        ours.last_work = Some("BH".into());
        let mut disk = cfg();
        disk.last_work = Some("Ham".into());
        let merged = merge_configs(&ours, disk, &[], &[]);
        assert_eq!(merged.last_work.as_deref(), Some("BH"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins merge_tests 2>&1 | tail -5
```

Expected: compile FAILURE — `cannot find function merge_configs`.

- [ ] **Step 3: Implement statics, dirty tracking, and the merge**

In `src/config.rs`, add near the top (after the `use` lines; `Mutex` and `Vec::new()` are const-constructible in statics):

```rust
use std::sync::Mutex;

/// Works whose per-work config entries (position, position id, last gloss)
/// THIS instance changed. `save()` only overwrites these keys in the
/// on-disk file, so a concurrently running instance's newer entries for
/// OTHER works survive this instance's exit.
static DIRTY_WORKS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Works opened this session, most recent first — merged to the front of
/// `recent_works` on save.
static SESSION_RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Mark a work's per-work config entries as changed by this instance.
/// Call at every site that writes `work_positions`, `work_position_ids`,
/// or `last_gloss` for a work.
pub fn mark_work_dirty(abbrev: &str) {
    let mut d = DIRTY_WORKS.lock().unwrap();
    if !d.iter().any(|a| a == abbrev) {
        d.push(abbrev.to_string());
    }
}
```

Extend `push_recent_work` (in `impl Config`, currently config.rs:334) to record session order:

```rust
    pub fn push_recent_work(&mut self, abbrev: &str) {
        self.recent_works.retain(|a| a != abbrev);
        self.recent_works.insert(0, abbrev.to_string());
        self.recent_works.truncate(MAX_RECENT_WORKS);
        let mut s = SESSION_RECENT.lock().unwrap();
        s.retain(|a| a != abbrev);
        s.insert(0, abbrev.to_string());
        s.truncate(MAX_RECENT_WORKS);
    }
```

Add the merge function (private, above `save`):

```rust
/// Merge this instance's snapshot over the freshly-read on-disk config.
/// Per-work maps take the DISK value except for works this instance touched
/// (dirty) or keys the disk doesn't have; `recent_works` puts this
/// session's opens first. All scalar fields (font, theme, last_work, ...)
/// come from `ours` — last-writer-wins, the natural MRU semantic.
fn merge_configs(
    ours: &Config,
    disk: Config,
    dirty: &[String],
    session_recent: &[String],
) -> Config {
    fn overlay<V: Clone>(
        ours: &HashMap<String, V>,
        disk: HashMap<String, V>,
        dirty: &[String],
    ) -> HashMap<String, V> {
        let mut out = disk;
        for (k, v) in ours {
            if dirty.iter().any(|d| d == k) || !out.contains_key(k) {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }

    let mut merged = ours.clone();
    merged.work_positions = overlay(&ours.work_positions, disk.work_positions, dirty);
    merged.work_position_ids =
        overlay(&ours.work_position_ids, disk.work_position_ids, dirty);
    merged.last_gloss = overlay(&ours.last_gloss, disk.last_gloss, dirty);

    let mut recent: Vec<String> = session_recent.to_vec();
    for a in disk.recent_works {
        if !recent.iter().any(|r| r == &a) {
            recent.push(a);
        }
    }
    recent.truncate(MAX_RECENT_WORKS);
    merged.recent_works = recent;
    merged
}
```

Rewrite `save()` (config.rs:395). The `LIT_HEADLESS_TEST` guard, its comment, and the atomic temp+rename write are kept verbatim; only the merge step is new:

```rust
pub fn save(config: &Config) {
    // Hermetic test runs: under LIT_HEADLESS_TEST the app must NEVER write config
    // back. A headless/fuzz run starts from LIT_START_WORK/LIT_START_POS (or the
    // dev config) and would otherwise rewrite last_work/work_positions on exit —
    // the documented footgun where the next run inherits the prior run's end
    // position. Suppressing writeback makes a run fully reproducible from env
    // alone and stops it mutating state a later run depends on.
    if std::env::var_os("LIT_HEADLESS_TEST").is_some() {
        return;
    }
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Merge over the freshly-read file so a concurrently running instance's
    // per-work entries survive this instance's saves (see merge_configs).
    // Read/parse failure (missing or corrupt file) falls back to writing the
    // full snapshot, exactly the pre-merge behavior.
    let to_write = match fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
    {
        Some(disk) => {
            let dirty = DIRTY_WORKS.lock().unwrap().clone();
            let session = SESSION_RECENT.lock().unwrap().clone();
            merge_configs(config, disk, &dirty, &session)
        }
        None => config.clone(),
    };
    // Atomic write: write to temp, then rename
    let tmp = path.with_extension("tmp");
    if let Ok(json) = serde_json::to_string_pretty(&to_write) {
        if fs::write(&tmp, &json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}
```

- [ ] **Step 4: Mark the three dirty sites in `src/app/mod.rs`**

Site 1 — work switch (~line 2782):

```rust
    // Save position of the outgoing work before switching
    if let Some(ref old_work) = state.current_work {
        state.config.work_positions.insert(old_work.abbrev.clone(), state.current_line);
        if let Some(id) = state.work_line_for_buffer(state.current_line)
            .and_then(|wi| old_work.lines.get(wi)).map(|l| l.id)
        {
            state.config.work_position_ids.insert(old_work.abbrev.clone(), id);
        }
        crate::config::mark_work_dirty(&old_work.abbrev);
    }
```

Site 2 — current-work position save (~line 3940; the function whose body ends with `crate::config::save(&state.config)` right after `last_column_count`):

```rust
        state.config.last_work = Some(abbrev.clone());
        state.config.work_positions.insert(abbrev.clone(), state.current_line); // legacy fallback
        crate::config::mark_work_dirty(&abbrev);
        if let Some(id) = id {
            state.config.work_position_ids.insert(abbrev, id);
        }
        state.config.last_column_count = Some(cc);
        crate::config::save(&state.config);
```

(Note: `mark_work_dirty` is called before the `insert` that MOVES `abbrev` — placing it after would not compile.)

Site 3 — `record_last_gloss` (~line 867):

```rust
    pub fn record_last_gloss(&mut self, gloss_type: &str) {
        if let Some(ctx) = &self.gloss_context {
            let work = ctx.work_abbrev.clone();
            let entry = crate::config::LastGloss {
                start_citation: ctx.start_citation.clone(),
                gloss_type: gloss_type.to_string(),
            };
            crate::config::mark_work_dirty(&work);
            self.config.last_gloss.insert(work, entry);
            crate::config::save(&self.config);
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cd ~/utono/linux-lit && cargo test --bins 2>&1 | tail -5
```

Expected: all pass, including the 5 new `merge_tests` and the pre-existing `last_gloss_round_trips_through_json`.

- [ ] **Step 6: Clippy gate**

Run:

```bash
cd ~/utono/linux-lit && cargo clippy 2>&1 | tail -5
```

Expected: no new warnings in `config.rs` / `instance.rs` / `discovery.rs` / the touched `app/mod.rs` lines.

- [ ] **Step 7: Design-doc addendum**

In `docs/plans/2026-07-09-multi-instance-crll-design.md`, section "5. Config merge-on-save", after the first bullet, append this bullet:

```markdown
- **Implementation addendum (2026-07-09):** positions live in THREE per-work
  maps, not one — `work_positions` (legacy line-number fallback),
  `work_position_ids` (line-id keyed, primary), and `last_gloss`. All three
  get the identical dirty-key overlay in `merge_configs`.
```

- [ ] **Step 8: Commit**

```bash
cd ~/utono/linux-lit && git add src/config.rs src/app/mod.rs docs/plans/2026-07-09-multi-instance-crll-design.md && git commit -m "feat: config merge-on-save — concurrent instances stop clobbering per-work state"
```

---

### Task 5: crll alias → function with per-instance stderr tee (shell-config repo)

**Files:**

- Modify: `~/utono/shell-config/.config/shell/alias-mlj:15` (separate git repo — commit there, not in linux-lit)

**Interfaces:**

- Consumes: nothing from the Rust tasks (heuristic is process-count only).
- Produces: the `crll` command users type; behavior contract: first launch tees stderr to the canonical file, concurrent launches tee to `-2`.

- [ ] **Step 1: Replace the alias with a function**

Current line 15 of `~/utono/shell-config/.config/shell/alias-mlj`:

```zsh
alias crll='cd ~/utono/linux-lit && cargo build && LIT_DEV=1 ./target/debug/linux-lit 2>&1 | tee ~/utono/linux-lit/linux-lit-dev-stderr.log'
```

Replace with:

```zsh
# crll: dev build + run linux-lit, teeing stderr to a log. When another
# instance is already running, tee to -2 so we don't truncate the primary
# instance's stderr log. Cosmetic heuristic only — authoritative instance
# slots (sockets, app log) are assigned in-app via flock (src/instance.rs).
crll() {
    cd ~/utono/linux-lit && cargo build || return
    local stderr_log=~/utono/linux-lit/linux-lit-dev-stderr.log
    if pgrep -f 'target/debug/linux-lit' >/dev/null 2>&1; then
        stderr_log=~/utono/linux-lit/linux-lit-dev-stderr-2.log
    fi
    LIT_DEV=1 ./target/debug/linux-lit 2>&1 | tee "$stderr_log"
}
```

The file is stowed (symlinked), so the edit is live for new shells immediately; no stow step needed.

- [ ] **Step 2: Syntax-check the file**

Run:

```bash
zsh -n ~/utono/shell-config/.config/shell/alias-mlj && echo SYNTAX-OK
```

Expected: `SYNTAX-OK`.

- [ ] **Step 3: Commit (in shell-config)**

```bash
cd ~/utono/shell-config && git add .config/shell/alias-mlj && git commit -m "feat: crll teed stderr goes to -2 when an instance is already running"
```

---

### Task 6: Verification — headless two-instance run + live acceptance handoff

**Files:**

- No source changes. Uses the documented headless cage flow (CLAUDE.md "Headless Verification").

- [ ] **Step 1: Full gates**

Run:

```bash
cd ~/utono/linux-lit && cargo build 2>&1 | tail -3 && cargo test --bins 2>&1 | tail -3 && cargo clippy 2>&1 | tail -3
```

Expected: build OK, tests all pass, no new clippy warnings.

- [ ] **Step 2: Headless slot-assignment check (two concurrent cages)**

This verifies flock slot assignment and per-slot logging end-to-end. `LIT_HEADLESS_TEST=1` skips MPV and suppresses config writes, so the run is hermetic; socket namespacing itself is covered by unit tests + the live check (MPV never launches headless).

```bash
cd ~/utono/linux-lit
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_LOG_PATH=/tmp/claude-multi-a.log \
  dbus-run-session -- cage -- ./target/debug/linux-lit 2>/tmp/cage-a.log &
sleep 4
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_HEADLESS_TEST=1 \
  LIT_LOG_PATH=/tmp/claude-multi-b.log \
  dbus-run-session -- cage -- ./target/debug/linux-lit 2>/tmp/cage-b.log &
sleep 6
rg "INSTANCE: slot" /tmp/claude-multi-a.log /tmp/claude-multi-b.log
```

Expected output: `slot 1` in one log and `slot 2` in the other (order depends on which cage won the race; if the user's live instance is running it holds slot 1 and these two get 2 and 3 — any two DISTINCT slots is a pass). Both cages stay alive (two windows, NON_UNIQUE working). Then clean up — scoped pattern ONLY (a bare binary-path pkill would kill the user's live instance):

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 3: Confirm slot release**

Re-run a single headless instance the same way with `LIT_LOG_PATH=/tmp/claude-multi-c.log`, wait ~5s, and check it acquires the lowest slot that was freed in Step 2 (flock released on process exit). Clean up with the same scoped pkill.

- [ ] **Step 4: Live acceptance (hand to the user — agent must not run the app)**

Give the user exactly this checklist:

```bash
# terminal 1
crll
# terminal 2 (after instance 1 is up)
crll
```

- Second window title ends in `[2]`; first has no suffix.
- Switch instance 2 to a different work (Ctrl+p): instance 1's MPV keeps playing, untouched; instance 2 gets its own MPV on tag 10.
- `ls /tmp/mpvsocket-*` shows unsuffixed sockets for instance 1 and `mpvsocket-i2-*` for instance 2.
- Read to distinct positions in each, then close the instances in EITHER order; relaunch `crll` and confirm both works resume at their new positions (config merge).
- Stderr logs: `linux-lit-dev-stderr.log` (first) and `linux-lit-dev-stderr-2.log` (second).

- [ ] **Step 5: Commit the plan checkboxes / any doc touch-ups**

```bash
cd ~/utono/linux-lit && git add docs/plans/2026-07-09-multi-instance-crll.md && git commit -m "docs: multi-instance implementation plan"
```

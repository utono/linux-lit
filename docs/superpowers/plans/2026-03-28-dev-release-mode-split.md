# Dev/Release Mode Split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow a release binary and a dev instance (`cargo run`) to run simultaneously with separate configs, logs, and GTK application IDs.

**Architecture:** An environment variable `LIT_DEV` (set automatically by `.cargo/config.toml` for `cargo run`) determines mode. Config and logging paths branch based on this. Shell aliases provide convenient build/launch commands.

**Tech Stack:** Rust, GTK4, serde_json, zsh aliases

---

### Task 1: Add `is_dev_mode()` helper and `.cargo/config.toml`

**Files:**
- Create: `.cargo/config.toml`
- Create: `src/mode.rs`
- Modify: `src/main.rs:1` (add `mod mode;`)

- [ ] **Step 1: Create `.cargo/config.toml`**

```toml
[env]
LIT_DEV = "1"
```

- [ ] **Step 2: Create `src/mode.rs`**

```rust
pub fn is_dev_mode() -> bool {
    std::env::var("LIT_DEV").is_ok()
}
```

- [ ] **Step 3: Add `mod mode;` to `src/main.rs`**

Add after the existing `mod logging;` line:

```rust
mod mode;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add .cargo/config.toml src/mode.rs src/main.rs
git commit -m "feat: add dev/release mode detection via LIT_DEV env var"
```

---

### Task 2: Branch config path by mode

**Files:**
- Modify: `src/config.rs:117-120` (`config_path` function)

- [ ] **Step 1: Update `config_path()` to branch on mode**

Replace the `config_path` function in `src/config.rs`:

```rust
fn config_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    let filename = if crate::mode::is_dev_mode() {
        "config-dev.json"
    } else {
        "config.json"
    };
    PathBuf::from(home).join(".config/linux-lit").join(filename)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: branch config path by dev/release mode"
```

---

### Task 3: Branch log path and application ID by mode

**Files:**
- Modify: `src/main.rs:17-25` (log path and application ID setup)

- [ ] **Step 1: Update `main()` to branch log path and app ID**

Replace lines 18-25 in `src/main.rs` (the log setup and application builder):

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

    let app_id = if mode::is_dev_mode() {
        "com.utono.linux-lit.dev"
    } else {
        "com.utono.linux-lit"
    };

    let application = gtk4::Application::builder()
        .application_id(app_id)
        .build();
```

Everything after `.build();` remains unchanged.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: branch log path and GTK app ID by dev/release mode"
```

---

### Task 4: Add shell aliases

**Files:**
- Modify: `~/utono/shell-config/.config/shell/alias-mlj`

- [ ] **Step 1: Add `cbr` and `llit` aliases**

After the existing `alias cr='cd ~/utono/linux-lit && cargo run'` line, add:

```bash
alias cbr='cd ~/utono/linux-lit && cargo build --release'
alias llit='~/utono/linux-lit/target/release/linux-lit'
```

- [ ] **Step 2: Verify aliases parse**

Run: `zsh -c 'source ~/utono/shell-config/.config/shell/alias-mlj && alias cbr && alias llit'`
Expected: prints both alias definitions without errors

- [ ] **Step 3: Commit (in shell-config repo)**

```bash
cd ~/utono/shell-config
git add .config/shell/alias-mlj
git commit -m "feat: add cbr and llit aliases for linux-lit release builds"
```

---

### Task 5: Build release binary and verify isolation

- [ ] **Step 1: Build release binary**

Run: `cd ~/utono/linux-lit && cargo build --release`
Expected: compiles successfully

- [ ] **Step 2: Verify `LIT_DEV` is set for cargo run**

Run: `cd ~/utono/linux-lit && cargo run -- --help 2>&1 | head -1` (just to confirm it launches; Ctrl+C to exit)

Check the log file created:
Run: `ls -la ~/utono/linux-lit/linux-lit-dev.log`
Expected: file exists and was recently modified

- [ ] **Step 3: Verify release binary does NOT have `LIT_DEV`**

Run: `~/utono/linux-lit/target/release/linux-lit &` then check:
Run: `ls -la ~/utono/linux-lit/linux-lit-release.log`
Expected: file exists and was recently modified

Kill the release instance afterward.

- [ ] **Step 4: Verify separate config files**

After running both modes at least once:
Run: `ls ~/.config/linux-lit/`
Expected: both `config.json` and `config-dev.json` exist

- [ ] **Step 5: Commit any .gitignore updates if needed**

The `.gitignore` currently has `linux-lit.log`. Replace it with both new filenames:

```
linux-lit-dev.log
linux-lit-release.log
```

```bash
cd ~/utono/linux-lit
git add .gitignore
git commit -m "chore: update .gitignore for new dev/release log filenames"
```

# External Theme Reload via SIGUSR1 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make linux-lit reactively update its theme when external scripts change the system theme or cycle wallpaper colors.

**Architecture:** SIGUSR1 signal listener on the Tokio thread sends a `ThemeChanged` event through the existing `evt_tx` channel to the GTK main loop, which re-reads `.current_theme`, re-resolves from `themes-unified.json`, and reapplies via the existing `apply_theme_to_state()`. Shell scripts `cycle-wallpaper.sh` and `set-theme.sh` are modified to send the signal.

**Tech Stack:** Rust (tokio::signal::unix), GTK4, shell (pkill)

---

### Task 1: Add ThemeChanged variant to MpvEvent

**Files:**
- Modify: `src/mpv/commands.rs:28-33`

- [ ] **Step 1: Add the variant**

In `src/mpv/commands.rs`, add `ThemeChanged` to the `MpvEvent` enum:

```rust
/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
#[allow(dead_code)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
    TimePos(f64),
    ThemeChanged,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Success (no consumers of `ThemeChanged` yet, and the `match` in `main.rs` will get a warning about non-exhaustive patterns — that's fine, we fix it in Task 3)

- [ ] **Step 3: Commit**

```bash
git add src/mpv/commands.rs
git commit -m "add ThemeChanged variant to MpvEvent"
```

---

### Task 2: Make apply_theme_to_state public

**Files:**
- Modify: `src/input/keymap.rs:1005`

- [ ] **Step 1: Change visibility**

In `src/input/keymap.rs`, change line 1005 from:

```rust
fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
```

to:

```rust
pub(crate) fn apply_theme_to_state(state: &mut crate::app::AppState, theme: &crate::theme::Theme) {
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Success

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "make apply_theme_to_state pub(crate) for external theme reload"
```

---

### Task 3: Add SIGUSR1 listener and ThemeChanged handler

**Files:**
- Modify: `src/main.rs:37-41` (Tokio thread spawn)
- Modify: `src/main.rs:59-152` (event match block)

- [ ] **Step 1: Spawn SIGUSR1 listener on Tokio thread**

In `src/main.rs`, modify the Tokio thread spawn block. Change lines 37-41 from:

```rust
        std::thread::spawn(move || {
            rt.block_on(async move {
                crate::mpv::client::run(cmd_rx, evt_tx).await;
            });
        });
```

to:

```rust
        std::thread::spawn(move || {
            rt.block_on(async move {
                // SIGUSR1 listener for external theme changes
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
```

- [ ] **Step 2: Add ThemeChanged handler in event match block**

In `src/main.rs`, add a new match arm after the `MpvEvent::TimePos` block (after line 151, before the closing `}`). Add this arm:

```rust
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
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1`
Expected: Success, no warnings

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "handle SIGUSR1 to reload theme from external changes"
```

---

### Task 4: Add pkill to cycle-wallpaper.sh

**Files:**
- Modify: `~/.config/themes/cycle-wallpaper.sh:70` (after the dwl signal block)

- [ ] **Step 1: Add linux-lit signal**

In `~/.config/themes/cycle-wallpaper.sh`, after the dwl SIGUSR1 block (after line 70 `fi`), add:

```bash
# Signal linux-lit to reload theme
pkill -USR1 linux-lit 2>/dev/null || true
```

The result should look like:

```bash
# Signal dwl to reload
if pidof dwl > /dev/null 2>&1; then
	kill -SIGUSR1 "$(pidof dwl)" 2>/dev/null || true
fi

# Signal linux-lit to reload theme
pkill -USR1 linux-lit 2>/dev/null || true

# Find label for this color
```

- [ ] **Step 2: Commit**

The script lives in `~/utono/tty-dotfiles/` (stow-managed). Find the source file and commit there:

```bash
cd ~/utono/tty-dotfiles
git add -A && git diff --cached --stat
git commit -m "signal linux-lit on wallpaper cycle"
```

---

### Task 5: Add pkill to set-theme.sh

**Files:**
- Modify: `~/.config/themes/set-theme.sh:154` (after the success message)

- [ ] **Step 1: Add linux-lit signal**

In `~/.config/themes/set-theme.sh`, after the success echo line (line 154), add:

```bash
# Signal linux-lit to reload theme
pkill -USR1 linux-lit 2>/dev/null || true
```

The result should look like:

```bash
echo ""
echo "✓ Theme '$THEME_NAME' applied successfully"

# Signal linux-lit to reload theme
pkill -USR1 linux-lit 2>/dev/null || true
```

- [ ] **Step 2: Commit**

```bash
cd ~/utono/tty-dotfiles
git add -A && git diff --cached --stat
git commit -m "signal linux-lit on theme change"
```

---

### Task 6: Manual integration test

- [ ] **Step 1: Build and run linux-lit**

```bash
cd ~/utono/linux-lit
cargo build
cargo run
```

- [ ] **Step 2: Test manual signal**

In another terminal:

```bash
pkill -USR1 linux-lit
```

Check `linux-lit.log` for: `SETTINGS: theme changed to <current theme name>`

- [ ] **Step 3: Test wallpaper cycle**

Press Super+Shift+Backslash (cycle wallpaper). Verify linux-lit's background color updates to match the new rootcolor.

- [ ] **Step 4: Test full theme change**

Run `~/.config/themes/set-theme.sh <different-theme>`. Verify linux-lit's entire color scheme updates immediately.

- [ ] **Step 5: Test with linux-lit not running**

Close linux-lit. Run `pkill -USR1 linux-lit`. Verify no error output (pkill exits silently).

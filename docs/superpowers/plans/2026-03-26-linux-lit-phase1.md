# linux-lit Phase 1: Project Scaffold & Window

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A GTK4 window opens with a configured GtkTextView for serif reading, Tokio runtime on a background thread, and channel bridge between the two runtimes.

**Architecture:** Two-runtime design: GTK4 main loop for UI, Tokio on a spawned thread for async I/O. They communicate via `glib::MainContext::channel` (Tokio->GTK) and `tokio::sync::mpsc` (GTK->Tokio). Phase 1 sets up both runtimes and the channel bridge but only stubs the Tokio side.

**Tech Stack:** Rust, gtk4-rs, tokio, pango

---

## File Structure

```
~/utono/linux-lit/
  Cargo.toml              # Dependencies and project metadata
  src/
    main.rs               # Entry point: GTK app init, Tokio thread, channel bridge
    app.rs                # Window creation, GtkTextView setup, event controller
    mpv/
      mod.rs              # Re-exports
      commands.rs         # Command/Event enums for channel bridge
```

Phase 1 creates the minimal skeleton. Later phases add `db/`, `ui/`, `input/`, `theme/`, `config.rs`.

---

### Task 1: Initialize Cargo Project

**Files:**
- Create: `Cargo.toml`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "linux-lit"
version = "0.1.0"
edition = "2021"

[dependencies]
gtk4 = "0.9"
pango = "0.20"
glib = "0.20"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.33", features = ["bundled"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -5`
Expected: `Finished` (may take a while first time to download crates)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: initialize Cargo project with gtk4, tokio, rusqlite dependencies"
```

---

### Task 2: Define Command/Event Enums

**Files:**
- Create: `src/mpv/mod.rs`
- Create: `src/mpv/commands.rs`

These enums define the typed channel messages between GTK and Tokio. Defined early so `main.rs` can reference the types when creating channels.

- [ ] **Step 1: Create `src/mpv/commands.rs`**

```rust
/// Commands sent from the GTK UI thread to the Tokio runtime.
#[derive(Debug)]
pub enum MpvCommand {
    Seek(f64),
    TogglePause,
    LoadFile(String),
    Connect(String),
    Disconnect,
}

/// Events sent from the Tokio runtime back to the GTK UI thread.
#[derive(Debug)]
pub enum MpvEvent {
    CursorSync(usize),
    ConnectionStatus(bool),
    PlaybackState(bool),
}
```

- [ ] **Step 2: Create `src/mpv/mod.rs`**

```rust
pub mod commands;
pub use commands::{MpvCommand, MpvEvent};
```

- [ ] **Step 3: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -3`
Expected: `Finished` (with possible warnings about unused items, that's fine)

- [ ] **Step 4: Commit**

```bash
git add src/mpv/
git commit -m "feat: define MpvCommand and MpvEvent channel message types"
```

---

### Task 3: Create `app.rs` — Window and GtkTextView

**Files:**
- Create: `src/app.rs`

This module creates the application window with a properly configured GtkTextView for reading. The text view is read-only, uses serif fonts, has centered margins, appropriate line spacing, and no scrollbar.

- [ ] **Step 1: Create `src/app.rs`**

```rust
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, CssProvider, EventControllerKey, ScrolledWindow, TextBuffer, TextView,
    WrapMode,
};

/// Builds and shows the main application window.
/// Returns the TextView so the caller can hold a reference for later updates.
pub fn build_window(app: &gtk4::Application) -> TextView {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("linux-lit")
        .default_width(1000)
        .default_height(800)
        .build();

    let buffer = TextBuffer::new(None);
    let text_view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(WrapMode::Word)
        .build();

    // Apply serif font via CSS
    let css_provider = CssProvider::new();
    css_provider.load_from_string(&format!(
        "textview {{ font-family: Georgia, 'Noto Serif', 'Liberation Serif', 'DejaVu Serif'; font-size: {}pt; }}",
        18
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Line spacing: 1.6x at 18pt ≈ ~14px above + 14px below
    text_view.set_pixels_above_lines(14);
    text_view.set_pixels_below_lines(14);

    // Set initial margins for centered ~700px text column
    text_view.set_left_margin(150);
    text_view.set_right_margin(150);

    // Scrolled window — hide scrollbar
    let scrolled = ScrolledWindow::builder()
        .child(&text_view)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::External)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Recalculate margins on resize to keep text column centered
    let text_view_for_resize = text_view.clone();
    scrolled.connect_notify_local(Some("width"), move |scrolled, _| {
        let width = scrolled.width();
        let margin = ((width - 700) / 2).max(20);
        text_view_for_resize.set_left_margin(margin);
        text_view_for_resize.set_right_margin(margin);
    });

    // Key event controller — stub that logs presses
    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(|_controller, keyval, _keycode, _state| {
        if let Some(name) = keyval.name() {
            eprintln!("key: {}", name);
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.set_child(Some(&scrolled));
    window.present();

    text_view
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd ~/utono/linux-lit && cargo check 2>&1 | tail -3`
Expected: `Finished`

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: add app.rs with GTK4 window and configured read-only text view"
```

---

### Task 4: Create `main.rs` — Entry Point with Dual Runtime

**Files:**
- Create: `src/main.rs`

This wires everything together: GTK application, Tokio runtime on a background thread, and the bidirectional channel bridge.

- [ ] **Step 1: Create `src/main.rs`**

```rust
mod app;
mod mpv;

use gtk4::prelude::*;
use mpv::{MpvCommand, MpvEvent};

fn main() {
    let application = gtk4::Application::builder()
        .application_id("com.utono.linux-lit")
        .build();

    application.connect_activate(|app| {
        // Channel: GTK → Tokio (commands)
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<MpvCommand>(32);

        // Channel: Tokio → GTK (events)
        let (evt_tx, evt_rx) = glib::MainContext::channel::<MpvEvent>(glib::Priority::DEFAULT);

        // Spawn Tokio runtime on a background thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            rt.block_on(async move {
                // Stub: drain commands, log them
                while let Some(cmd) = cmd_rx.recv().await {
                    eprintln!("Tokio received command: {:?}", cmd);
                }
                // evt_tx available for sending events back to GTK
                let _ = evt_tx;
            });
        });

        // Build the window
        let _text_view = app::build_window(app);

        // Attach event receiver to GTK main loop
        evt_rx.attach(None, |event| {
            eprintln!("GTK received event: {:?}", event);
            glib::ControlFlow::Continue
        });

        // cmd_tx available for UI to send commands to Tokio
        let _ = cmd_tx;
    });

    application.run();
}
```

- [ ] **Step 2: Run the application**

Run: `cd ~/utono/linux-lit && cargo run 2>&1`
Expected: A GTK4 window appears (1000x800) with an empty text area. Key presses print to stderr. Close the window to exit.

- [ ] **Step 3: Verify key events log**

Press a few keys (j, k, q) while the window is focused. Stderr should show:
```
key: j
key: k
key: q
```

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: add main.rs with GTK4 app, Tokio background thread, and channel bridge"
```

---

### Task 5: Visual Verification with Sample Text

**Files:**
- Modify: `src/app.rs`

Temporarily add sample text to verify font rendering, margins, and line spacing are correct, then remove it.

- [ ] **Step 1: Add sample text in `build_window()`**

Temporarily add this after creating the buffer (before the CSS provider code):

```rust
    buffer.set_text(
        "Who's there?\n\
         Nay, answer me. Stand and unfold yourself.\n\
         Long live the King!\n\
         Barnardo?\n\
         He.\n\
         You come most carefully upon your hour.\n\
         'Tis now struck twelve. Get thee to bed, Francisco.\n\
         For this relief much thanks. 'Tis bitter cold,\n\
         And I am sick at heart.\n\
         Have you had quiet guard?",
    );
```

- [ ] **Step 2: Run and verify visually**

Run: `cd ~/utono/linux-lit && cargo run`
Expected:
- Window opens with Hamlet opening lines
- Text is in a serif font (Georgia if installed, else fallback)
- Text column is centered with generous margins on left and right
- Lines have comfortable spacing (1.6x)
- No scrollbar visible
- Key presses still log to stderr

- [ ] **Step 3: Verify resize behavior**

Resize the window wider and narrower. The text column should stay centered at ~700px width, with margins adjusting dynamically.

- [ ] **Step 4: Remove sample text**

Remove the `buffer.set_text(...)` call. The buffer should be empty at startup — work loading comes in Phase 2.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "chore: verify font rendering and margins (sample text removed)"
```

---

### Task 6: Clean Up Warnings

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Run `cargo clippy`**

Run: `cd ~/utono/linux-lit && cargo clippy 2>&1 | tail -20`

Fix any warnings:
- Unused imports → remove them
- Unused variables → prefix with `_` where intentional (e.g., `_cmd_tx` stored for later use)
- Missing `use` statements → add them

- [ ] **Step 2: Run `cargo fmt`**

Run: `cd ~/utono/linux-lit && cargo fmt`

- [ ] **Step 3: Final verification**

Run: `cd ~/utono/linux-lit && cargo run`
Expected: Clean compile, window opens, no warnings in build output.

- [ ] **Step 4: Commit**

```bash
git add src/
git commit -m "chore: fix clippy warnings and format code"
```

---

## Phase 1 Acceptance Criteria

After completing all tasks:

1. `cargo run` opens a 1000x800 GTK4 window titled "linux-lit"
2. Empty serif-font text area fills the window
3. Text column is centered at ~700px, margins adjust on resize
4. No scrollbar visible
5. Key presses log to stderr
6. Tokio runtime runs on a background thread (no UI blocking)
7. Channel bridge exists: `mpsc` (GTK->Tokio) and `glib channel` (Tokio->GTK)
8. `cargo clippy` produces no warnings
9. Clean git history with descriptive commits

## Notes for Phase 2

- The `cmd_tx` and `evt_rx` will be threaded through to the navigation and MPV modules
- The `text_view` reference from `build_window()` will be stored in application state for cursor management
- The empty buffer will be populated by `load_work()` from the database layer
- The stub key handler will be replaced by the full keymap state machine

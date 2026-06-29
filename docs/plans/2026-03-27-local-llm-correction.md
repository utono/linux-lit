# Local LLM Transcript Correction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the "Correct with Claude" feature (kitty terminal + file polling) with a direct Ollama HTTP API call and an inline before/after diff overlay.

**Architecture:** A new `src/ollama.rs` module handles the HTTP POST to Ollama's `/api/generate` endpoint. The correction result is displayed in a new `CorrectionOverlay` widget (following the same pattern as `KeybindsOverlay`). The async call uses the existing `tokio_handle` on `AppState` to bridge Tokio ↔ GTK via `glib::spawn_future_local`.

**Tech Stack:** Rust, GTK4, reqwest (new dep), Ollama API, existing Tokio runtime

---

## File Map

- **Create:** `src/ollama.rs` — async Ollama HTTP client
- **Create:** `src/ui/correction_overlay.rs` — before/after diff overlay widget
- **Modify:** `Cargo.toml` — add `reqwest` dependency
- **Modify:** `src/config.rs` — add `ollama_model` and `ollama_endpoint` fields
- **Modify:** `src/main.rs` — add `mod ollama`
- **Modify:** `src/ui/mod.rs` — add `pub mod correction_overlay`
- **Modify:** `src/app.rs` — add `correction_overlay` field to `AppState`, build it in `build_window`
- **Modify:** `src/input/visual.rs` — replace `action_correct_with_claude` with `action_correct_with_llm`, rename menu item
- **Modify:** `src/input/keymap.rs` — add key routing for correction overlay (`y`/`n`/`Escape`)

---

### Task 1: Add reqwest dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add reqwest to Cargo.toml**

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

Add this line after the `sha2 = "0.10"` line. Using `rustls-tls` avoids needing OpenSSL system dependency.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully (reqwest pulls in but nothing uses it yet)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "deps: add reqwest for Ollama HTTP client"
```

---

### Task 2: Add config fields for Ollama

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add default functions**

After the `default_text_margins` function (line 71-73), add:

```rust
fn default_ollama_model() -> String {
    "qwen2.5:7b".to_string()
}

fn default_ollama_endpoint() -> String {
    "http://localhost:11434".to_string()
}
```

- [ ] **Step 2: Add fields to Config struct**

After the `visual_mode_commands` field (line 39), add:

```rust
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default = "default_ollama_endpoint")]
    pub ollama_endpoint: String,
```

- [ ] **Step 3: Add fields to Default impl**

In the `Default` impl (line 75-89), add before the closing brace:

```rust
            ollama_model: default_ollama_model(),
            ollama_endpoint: default_ollama_endpoint(),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully. Existing config.json files load fine due to `serde(default)`.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "config: add ollama_model and ollama_endpoint fields"
```

---

### Task 3: Create Ollama HTTP client module

**Files:**
- Create: `src/ollama.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create src/ollama.rs**

```rust
use std::fmt;

#[derive(Debug)]
pub enum OllamaError {
    ConnectionRefused,
    Timeout,
    ModelNotFound(String),
    Other(String),
}

impl fmt::Display for OllamaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OllamaError::ConnectionRefused => write!(f, "Ollama not running — start with: systemctl start ollama"),
            OllamaError::Timeout => write!(f, "Correction timed out — try selecting fewer lines"),
            OllamaError::ModelNotFound(m) => write!(f, "Model not found — run: ollama pull {}", m),
            OllamaError::Other(msg) => write!(f, "Ollama error: {}", msg),
        }
    }
}

const SYSTEM_PROMPT: &str = "\
You are correcting mistranscribed audiobook text. \
Fix ONLY words that are obviously wrong due to speech-to-text mishearing \
(homophones, phonetically similar but wrong words). \
Do NOT rephrase, restructure, or improve the text. \
Preserve original line breaks exactly. \
Output ONLY the corrected text with no commentary.";

pub async fn correct_text(
    endpoint: &str,
    model: &str,
    text: &str,
) -> Result<String, OllamaError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| OllamaError::Other(e.to_string()))?;

    let url = format!("{}/api/generate", endpoint);

    let body = serde_json::json!({
        "model": model,
        "system": SYSTEM_PROMPT,
        "prompt": text,
        "stream": false
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                OllamaError::ConnectionRefused
            } else if e.is_timeout() {
                OllamaError::Timeout
            } else {
                OllamaError::Other(e.to_string())
            }
        })?;

    let status = response.status();
    let text = response.text().await.map_err(|e| OllamaError::Other(e.to_string()))?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(OllamaError::ModelNotFound(model.to_string()));
    }

    if !status.is_success() {
        return Err(OllamaError::Other(format!("HTTP {}: {}", status, text)));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| OllamaError::Other(e.to_string()))?;

    json.get("response")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| OllamaError::Other("No 'response' field in Ollama output".to_string()))
}
```

- [ ] **Step 2: Register module in main.rs**

In `src/main.rs`, after `mod logging;` (line 7), add:

```rust
mod ollama;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully. The module compiles but is not called yet.

- [ ] **Step 4: Commit**

```bash
git add src/ollama.rs src/main.rs
git commit -m "feat: add Ollama HTTP client module"
```

---

### Task 4: Create correction overlay widget

**Files:**
- Create: `src/ui/correction_overlay.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Check existing ui/mod.rs for the module pattern**

Read `src/ui/mod.rs` to see how other overlay modules are declared.

- [ ] **Step 2: Create src/ui/correction_overlay.rs**

This overlay shows "ORIGINAL" and "CORRECTED" sections with word-level diff highlighting. Follows the `KeybindsOverlay` pattern: a `gtk4::Overlay` wrapping a container that can be shown/hidden.

```rust
use gtk4::prelude::*;
use gtk4::{self, Align, Label, Overlay, ScrolledWindow};

pub struct CorrectionOverlay {
    pub overlay: Overlay,
    container: gtk4::Box,
    original_label: Label,
    corrected_label: Label,
}

impl CorrectionOverlay {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        container.set_halign(Align::Center);
        container.set_valign(Align::Center);
        container.set_margin_top(40);
        container.set_margin_bottom(40);
        container.set_margin_start(60);
        container.set_margin_end(60);
        container.add_css_class("correction-overlay");

        // Title
        let title = Label::new(Some("Correction Review"));
        title.add_css_class("correction-title");
        container.append(&title);

        // Original section
        let orig_header = Label::new(Some("ORIGINAL"));
        orig_header.add_css_class("correction-header");
        orig_header.set_halign(Align::Start);
        container.append(&orig_header);

        let original_label = Label::new(None);
        original_label.set_wrap(true);
        original_label.set_halign(Align::Start);
        original_label.set_selectable(false);
        original_label.add_css_class("correction-text");

        let orig_scroll = ScrolledWindow::new();
        orig_scroll.set_child(Some(&original_label));
        orig_scroll.set_max_content_height(300);
        orig_scroll.set_propagate_natural_height(true);
        container.append(&orig_scroll);

        // Corrected section
        let corr_header = Label::new(Some("CORRECTED"));
        corr_header.add_css_class("correction-header");
        corr_header.set_halign(Align::Start);
        container.append(&corr_header);

        let corrected_label = Label::new(None);
        corrected_label.set_wrap(true);
        corrected_label.set_halign(Align::Start);
        corrected_label.set_selectable(false);
        corrected_label.add_css_class("correction-text");

        let corr_scroll = ScrolledWindow::new();
        corr_scroll.set_child(Some(&corrected_label));
        corr_scroll.set_max_content_height(300);
        corr_scroll.set_propagate_natural_height(true);
        container.append(&corr_scroll);

        // Hint
        let hint = Label::new(Some("y = accept  ·  n / Esc = reject"));
        hint.add_css_class("correction-hint");
        container.append(&hint);

        container.set_visible(false);

        CorrectionOverlay {
            overlay,
            container,
            original_label,
            corrected_label,
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.container);
    }

    pub fn show(&self, original: &str, corrected: &str) {
        let orig_markup = build_diff_markup(original, corrected, true);
        let corr_markup = build_diff_markup(original, corrected, false);
        self.original_label.set_markup(&orig_markup);
        self.corrected_label.set_markup(&corr_markup);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }
}

/// Build Pango markup highlighting words that differ between original and corrected.
/// If `is_original` is true, highlights removed/changed words in the original;
/// otherwise highlights new/changed words in the corrected text.
fn build_diff_markup(original: &str, corrected: &str, is_original: bool) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let corr_lines: Vec<&str> = corrected.lines().collect();
    let max_lines = orig_lines.len().max(corr_lines.len());

    let mut result = String::new();
    for i in 0..max_lines {
        if i > 0 {
            result.push('\n');
        }
        let orig_line = orig_lines.get(i).copied().unwrap_or("");
        let corr_line = corr_lines.get(i).copied().unwrap_or("");

        let orig_words: Vec<&str> = orig_line.split_whitespace().collect();
        let corr_words: Vec<&str> = corr_line.split_whitespace().collect();

        let (source_words, other_words) = if is_original {
            (&orig_words, &corr_words)
        } else {
            (&corr_words, &orig_words)
        };

        for (j, word) in source_words.iter().enumerate() {
            if j > 0 {
                result.push(' ');
            }
            let differs = other_words.get(j).map_or(true, |other| other != word);
            let escaped = glib::markup_escape_text(word);
            if differs {
                let color = if is_original { "#cc3333" } else { "#228833" };
                result.push_str(&format!("<span foreground=\"{}\" weight=\"bold\">{}</span>", color, escaped));
            } else {
                result.push_str(&escaped);
            }
        }
    }
    result
}
```

- [ ] **Step 3: Register module in src/ui/mod.rs**

Add this line alongside the other `pub mod` declarations:

```rust
pub mod correction_overlay;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully.

- [ ] **Step 5: Commit**

```bash
git add src/ui/correction_overlay.rs src/ui/mod.rs
git commit -m "feat: add correction overlay widget with word-level diff"
```

---

### Task 5: Wire overlay into AppState and overlay chain

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add field to AppState**

In the `AppState` struct (after line 81, the `keybinds_overlay` field), add:

```rust
    pub correction_overlay: crate::ui::correction_overlay::CorrectionOverlay,
    /// Pending correction data: (start_line, end_line, db_lines, abbrev, text_file, corrected_text)
    pub pending_correction: Option<PendingCorrection>,
```

Also add this struct definition above `AppState` (or in `visual.rs` if preferred — but `app.rs` is simpler since `AppState` owns it):

```rust
pub struct PendingCorrection {
    pub start: usize,
    pub end: usize,
    pub db_lines: Vec<crate::db::models::Line>,
    pub abbrev: String,
    pub text_file: Option<String>,
    pub corrected_text: String,
}
```

- [ ] **Step 2: Build and attach overlay in build_window**

Find where `keybinds_overlay` is built and attached in `build_window`. Create the `CorrectionOverlay` similarly and insert it into the overlay chain. The correction overlay should wrap the keybinds overlay so it renders on top:

```rust
let correction_overlay = crate::ui::correction_overlay::CorrectionOverlay::new();
correction_overlay.attach(&keybinds_overlay.overlay);
```

Then use `correction_overlay.overlay` as the window's child instead of `keybinds_overlay.overlay`.

Initialize `pending_correction: None` in the `AppState` construction.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully. Overlay exists but isn't shown yet.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: wire correction overlay into AppState and overlay chain"
```

---

### Task 6: Add CSS for correction overlay

**Files:**
- Modify: the CSS setup in `src/app.rs` (or wherever the app's CSS is loaded)

- [ ] **Step 1: Find where CSS classes are defined**

Search for where `keybinds-overlay` CSS class is styled (likely in `src/app.rs` or a CSS file). Add matching styles for the correction overlay classes.

- [ ] **Step 2: Add correction overlay CSS**

Add these CSS rules alongside the existing overlay styles:

```css
.correction-overlay {
    background-color: rgba(30, 30, 30, 0.95);
    border-radius: 12px;
    padding: 24px;
    min-width: 600px;
}
.correction-title {
    font-size: 18px;
    font-weight: bold;
    color: #e0e0e0;
    margin-bottom: 8px;
}
.correction-header {
    font-size: 13px;
    font-weight: bold;
    color: #888888;
    letter-spacing: 2px;
}
.correction-text {
    font-size: 15px;
    color: #d0d0d0;
    font-family: monospace;
}
.correction-hint {
    font-size: 13px;
    color: #888888;
    margin-top: 8px;
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: add CSS styles for correction overlay"
```

---

### Task 7: Replace action_correct_with_claude with action_correct_with_llm

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Rename menu item**

Change `BUILTIN_ACTIONS` (line 128):

```rust
pub const BUILTIN_ACTIONS: &[&str] = &[
    "Copy",
    "Copy with metadata",
    "Merge lines",
    "Correct with LLM",
];
```

- [ ] **Step 2: Update dispatch in execute_action**

Change index 3 dispatch (around line 173-176) from:

```rust
3 => {
    action_correct_with_claude(state_rc);
    return;
}
```

to:

```rust
3 => {
    action_correct_with_llm(state_rc);
    return;
}
```

- [ ] **Step 3: Write action_correct_with_llm function**

Replace the entire `action_correct_with_claude` function (lines 522-714) with:

```rust
fn action_correct_with_llm(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (start, end, input_text, db_lines, abbrev, text_file, endpoint, model, tokio_handle) = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        let mut selected_lines = Vec::new();
        for buf_line in start..=end {
            if let Some(line_start) = state.buffer.iter_at_line(buf_line as i32) {
                let mut line_end = line_start;
                if !line_end.ends_line() {
                    line_end.forward_to_line_end();
                }
                selected_lines.push(state.buffer.text(&line_start, &line_end, false).to_string());
            }
        }

        let mut db_lines: Vec<crate::db::models::Line> = Vec::new();
        for buf_line in start..=end {
            if let Some(work_idx) = state.work_line_for_buffer(buf_line) {
                if let Some(line) = work.lines.get(work_idx) {
                    db_lines.push(line.clone());
                }
            }
        }

        (
            start,
            end,
            selected_lines.join("\n"),
            db_lines,
            work.abbrev.clone(),
            work.text_file.clone(),
            state.config.ollama_endpoint.clone(),
            state.config.ollama_model.clone(),
            state.tokio_handle.clone(),
        )
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    crate::logging::log("VISUAL: starting LLM correction");

    let state_for_result = std::rc::Rc::clone(state_rc);
    let file_backup = text_file.as_ref().and_then(|path| {
        std::fs::read_to_string(path).ok().map(|content| (path.clone(), content))
    });

    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::ollama::correct_text(&endpoint, &model, &input_text).await
            })
            .await;

        let mut state = state_for_result.borrow_mut();

        match result {
            Ok(Ok(corrected)) => {
                let original: String = db_lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
                state.pending_correction = Some(crate::app::PendingCorrection {
                    start,
                    end,
                    db_lines,
                    abbrev,
                    text_file,
                    corrected_text: corrected.clone(),
                });
                state.correction_overlay.show(&original, &corrected);
                crate::logging::log("VISUAL: correction overlay shown");
            }
            Ok(Err(e)) => {
                crate::logging::log(&format!("VISUAL: LLM correction error: {}", e));
                // Show error briefly in the overlay title area
                state.correction_overlay.show(&format!("Error: {}", e), "");
            }
            Err(e) => {
                crate::logging::log(&format!("VISUAL: tokio join error: {}", e));
            }
        }
    });
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully.

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs
git commit -m "feat: replace Correct with Claude with Ollama HTTP-based LLM correction"
```

---

### Task 8: Add key routing for correction overlay

**Files:**
- Modify: `src/input/keymap.rs`

- [ ] **Step 1: Add correction overlay key handling**

In `keymap.rs`, find the keybinds overlay key handling block (around line 350-372). Add a similar block **before** it to handle the correction overlay:

```rust
let correction_visible = state.borrow().correction_overlay.is_visible();
if correction_visible {
    match key_name {
        "y" => {
            accept_correction(&state);
            return true;
        }
        "n" | "Escape" => {
            state.borrow_mut().pending_correction = None;
            state.borrow().correction_overlay.hide();
            return true;
        }
        _ => return true, // consume all other keys while overlay is open
    }
}
```

- [ ] **Step 2: Write accept_correction helper**

Add this function in `keymap.rs` (or in `visual.rs` if it fits better — but since it's called from key routing, `keymap.rs` is natural):

```rust
fn accept_correction(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let pending = {
        let mut state = state_rc.borrow_mut();
        state.correction_overlay.hide();
        state.pending_correction.take()
    };

    let pending = match pending {
        Some(p) => p,
        None => return,
    };

    let new_lines: Vec<String> = pending.corrected_text.lines().map(|l| l.to_string()).collect();
    if new_lines.is_empty() {
        crate::logging::log("VISUAL: corrected text was empty");
        return;
    }

    let old_ids: Vec<i64> = pending.db_lines.iter().map(|l| l.id).collect();
    let file_backup = pending.text_file.as_ref().and_then(|path| {
        std::fs::read_to_string(path).ok().map(|content| (path.clone(), content))
    });

    let mut state = state_rc.borrow_mut();

    state.undo_stack.push(crate::input::visual::UndoEntry {
        db_lines: pending.db_lines,
        file_backup,
        cursor_line: pending.start,
    });

    match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            if let Err(e) = crate::db::queries::replace_lines(&conn, &pending.abbrev, &old_ids, &new_lines) {
                crate::logging::log(&format!("VISUAL: correction DB error: {}", e));
                state.undo_stack.pop();
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: correction open_db_rw failed: {}", e));
            state.undo_stack.pop();
            return;
        }
    }

    if let Some(ref path) = pending.text_file {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut file_lines: Vec<String> = contents.lines().map(|l| l.to_string()).collect();
            if pending.end < file_lines.len() {
                file_lines.splice(pending.start..=pending.end, new_lines.iter().cloned());
                let _ = std::fs::write(path, file_lines.join("\n"));
            }
        }
    }

    crate::logging::log(&format!("VISUAL: correction applied, {} lines", new_lines.len()));
    crate::input::visual::reload_current_work(&mut state);
}
```

- [ ] **Step 3: Ensure reload_current_work is pub**

Check that `reload_current_work` in `visual.rs` (line 717) is `pub fn` so `keymap.rs` can call it. If it's not public, change `fn reload_current_work` to `pub fn reload_current_work`.

- [ ] **Step 4: Also hide correction overlay when other overlays open**

In the Ctrl+/ handler (keymap.rs ~line 494-507) and any other overlay toggle handlers, add `s.correction_overlay.hide()` alongside the other `.hide()` calls so overlays don't stack.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Builds successfully.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy`
Expected: No warnings.

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs src/input/visual.rs
git commit -m "feat: add key routing for correction overlay (y/n/Esc)"
```

---

### Task 9: Clean up removed code

**Files:**
- Modify: `src/input/visual.rs`

- [ ] **Step 1: Remove dead imports and code**

After replacing `action_correct_with_claude`, check for any now-unused imports or helper code that was only used by the old function:
- Remove `use std::process::Command` if no longer used elsewhere in visual.rs
- Remove any references to `/tmp/linux-lit-claude/`
- Remove any unused temp file path variables

- [ ] **Step 2: Verify it compiles and passes clippy**

Run: `cargo build && cargo clippy`
Expected: Clean build, no warnings.

- [ ] **Step 3: Commit**

```bash
git add src/input/visual.rs
git commit -m "cleanup: remove dead Claude CLI correction code"
```

---

### Task 10: Manual integration testing

- [ ] **Step 1: Ensure Ollama is running**

Run: `systemctl status ollama`
If not active: `sudo systemctl start ollama`

- [ ] **Step 2: Ensure model is pulled**

Run: `ollama list`
If `qwen2.5:7b` not listed: `ollama pull qwen2.5:7b`

- [ ] **Step 3: Test happy path**

1. `cargo run`
2. Open a work, enter visual mode (`v`), select a few lines
3. Open action popup, pick "Correct with LLM"
4. Verify "Correcting..." state, then before/after overlay appears
5. Press `y` to accept — verify lines update in display
6. Undo with `u` — verify lines restore

- [ ] **Step 4: Test rejection**

1. Select lines, trigger correction
2. Press `n` or `Escape` — verify overlay dismisses, no DB changes

- [ ] **Step 5: Test error handling**

1. Stop Ollama: `sudo systemctl stop ollama`
2. Trigger correction — verify error message appears in overlay
3. Press `n` to dismiss
4. Restart Ollama: `sudo systemctl start ollama`

- [ ] **Step 6: Final commit if any fixes needed**

```bash
git add -A
git commit -m "fix: address integration testing findings"
```

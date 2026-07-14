# Chat Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tab toggles a left chat panel (card pins right, unchanged size) that holds a multi-turn passage-anchored Claude conversation, saves curated Q&As to the journal, and revises them in place.

**Architecture:** The card's position moves via an asymmetric-margin branch in `apply_card_sizing`; the panel is an `add_overlay` layer on the window's outer overlay, sized from `main_card_rect`. The input box embeds the existing `AskCard` vim host routed through the shared `ask_vim_intercept`. Multi-turn requests go through a new `send_chat` beside `send_message` and a `run_claude_chat_request` bridge. Saves/updates reuse `save_passage_page` / `update_journal_page`; block discovery reuses the structure-aware bounds from the ask-passage branch.

**Tech Stack:** Rust, GTK4/sourceview5, reqwest+tokio via the existing glib bridge, rusqlite (lit.db). Spec: `docs/superpowers/specs/2026-07-10-chat-layout-design.md`.

## Global Constraints

- Build with `cargo build`; NEVER `cargo run` (the user runs the app).
- Pre-existing failing test `db::queries::tests::test_load_work_hamlet` (asserts live lit.db state) is expected in every full-suite run — ignore it.
- All work on branch `chat-layout` off master. The stow keymap `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (separate git repo) MUST be updated with the compiled rebinds or the JSON silently shadows them.
- Journal artifacts key by `Work.canonical_abbrev` (never raw abbrev).
- Panel styling is "bare on root": no card chrome; text renders on `theme.root_color` using `text_fg` / `dim_fg` inks. All theme CSS goes through `generate_css` in `src/theme.rs`.
- Keybind end-state: `Tab` → `ToggleChatLayout`; `-` (minus) → `TogglePause`; `TogglePreviousWork` loses its binding (Action variant stays); `Ctrl+Tab` keeps `ToggleLastOverlay` but is shadowed to "close chat panel" while the panel is open; plain `a` stays `TogglePause`.
- Chat prompt context: cursor segment ±2 neighbors, title/author/chapter only; system prompt `journal_qa_prompt(work_type)`. History is session-scoped (cleared on panel close and work switch).
- Minimum panel width to open: 500px of freed left space, else toast "No room for chat panel at this layout".

---

### Task 1: `send_chat` + chat bridge

**Files:**
- Modify: `src/claude.rs` (whole file is 110 lines; `send_message` at :47)
- Modify: `src/input/actions/claude_bridge.rs` (43 lines)

**Interfaces:**
- Produces: `pub struct ChatTurn { pub role: &'static str, pub content: String }` and `pub async fn send_chat(system: &str, turns: &[ChatTurn], model: &str) -> Result<String, ClaudeError>` in `crate::claude`; `pub(crate) fn run_claude_chat_request(state_rc, system_prompt: String, turns: Vec<crate::claude::ChatTurn>, model: String, on_success, on_error)` in `claude_bridge` (same callback types as `run_claude_request`). Task 6 consumes both.

- [ ] **Step 1: Create the branch**

```bash
cd ~/utono/linux-lit && git checkout master && git pull --ff-only && git checkout -b chat-layout
```

- [ ] **Step 2: Write the failing test**

Append to `src/claude.rs`:

```rust
#[cfg(test)]
mod chat_body_tests {
    use super::{chat_body, ChatTurn};

    fn turn(role: &'static str, content: &str) -> ChatTurn {
        ChatTurn { role, content: content.to_string() }
    }

    #[test]
    fn multi_turn_body_preserves_order_and_roles() {
        let turns = [
            turn("user", "q1"),
            turn("assistant", "a1"),
            turn("user", "q2"),
        ];
        let body = chat_body("SYS", &turns, "claude-opus-4-8");
        assert_eq!(body["model"], "claude-opus-4-8");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "SYS");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "q1");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "a1");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "q2");
    }

    #[test]
    fn single_turn_body_matches_legacy_send_message_shape() {
        let body = chat_body("S", &[turn("user", "hello")], "m");
        assert_eq!(
            body["messages"],
            serde_json::json!([{"role": "user", "content": "hello"}])
        );
    }
}
```

- [ ] **Step 3: Run to verify it fails**

```bash
cargo test --bins chat_body 2>&1 | tail -3
```

Expected: compile error — `chat_body` / `ChatTurn` not found.

- [ ] **Step 4: Implement `ChatTurn`, `chat_body`, `send_chat`; delegate `send_message`**

In `src/claude.rs`, above `send_message`:

```rust
/// One conversation turn for `send_chat`. `role` is "user" or "assistant".
#[derive(Clone, Debug)]
pub struct ChatTurn {
    pub role: &'static str,
    pub content: String,
}

/// Build the /v1/messages request body for a multi-turn conversation.
/// Kept as a pure fn so the shape is unit-testable.
fn chat_body(system: &str, turns: &[ChatTurn], model: &str) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = turns
        .iter()
        .map(|t| serde_json::json!({"role": t.role, "content": t.content}))
        .collect();
    serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "system": system,
        "messages": messages,
    })
}

/// Multi-turn variant of `send_message`: `turns` carries the session's prior
/// user/assistant messages plus the new user message, in order.
pub async fn send_chat(
    system: &str,
    turns: &[ChatTurn],
    model: &str,
) -> Result<String, ClaudeError> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| ClaudeError::MissingApiKey)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    let body = chat_body(system, turns, model);

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ClaudeError::Timeout
            } else {
                ClaudeError::ApiError(e.to_string())
            }
        })?;

    let status = response.status();
    let text = response.text().await.map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ClaudeError::RateLimited);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ClaudeError::Unauthorized);
    }
    if !status.is_success() {
        return Err(ClaudeError::ApiError(extract_api_error(status, &text)));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| ClaudeError::ApiError(e.to_string()))?;

    json.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ClaudeError::ApiError("No text in response".to_string()))
}
```

Then REPLACE the entire body of `send_message` (everything from `let api_key = ...` down through the final `.ok_or_else(...)`) with a single delegation, so there is exactly ONE HTTP path:

```rust
pub async fn send_message(
    system: &str,
    user_message: &str,
    model: &str,
) -> Result<String, ClaudeError> {
    send_chat(
        system,
        &[ChatTurn { role: "user", content: user_message.to_string() }],
        model,
    )
    .await
}
```

- [ ] **Step 5: Run the tests**

```bash
cargo test --bins chat_body 2>&1 | tail -3 && cargo build 2>&1 | tail -2
```

Expected: 2 passed; build OK.

- [ ] **Step 6: Add the chat bridge**

In `src/input/actions/claude_bridge.rs`, below `run_claude_request` (mirror its exact shape — glib::spawn_future_local wrapping tokio_handle.spawn; success/error callbacks on the main thread):

```rust
/// Multi-turn variant of `run_claude_request`: sends the whole conversation
/// (`turns` = prior user/assistant messages + the new user message, in order)
/// via `crate::claude::send_chat`. Same contract: show a loading state before
/// calling; callbacks run on the GTK main loop.
pub(crate) fn run_claude_chat_request(
    state_rc: &Rc<RefCell<AppState>>,
    system_prompt: String,
    turns: Vec<crate::claude::ChatTurn>,
    model: String,
    on_success: impl Fn(&Rc<RefCell<AppState>>, String) + 'static,
    on_error: impl Fn(&Rc<RefCell<AppState>>, &str) + 'static,
) {
    let tokio_handle = state_rc.borrow().tokio_handle.clone();
    let state_for_result = Rc::clone(state_rc);
    glib::spawn_future_local(async move {
        let result = tokio_handle
            .spawn(async move {
                crate::claude::send_chat(&system_prompt, &turns, &model).await
            })
            .await;
        match result {
            Ok(Ok(reply)) => on_success(&state_for_result, reply),
            Ok(Err(e)) => {
                crate::logging::log(&format!("CLAUDE: chat API error: {}", e));
                on_error(&state_for_result, &format!("Error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("CLAUDE: tokio join error: {}", e));
                on_error(&state_for_result, "Internal error \u{2014} try again.");
            }
        }
    });
}
```

- [ ] **Step 7: Build, test, commit**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | rg "test result" 
git add src/claude.rs src/input/actions/claude_bridge.rs
git commit -m "feat: send_chat multi-turn Claude API + chat bridge"
```

Expected: build OK; suite green except `test_load_work_hamlet`.

---

### Task 2: shared block helpers + `segment_context`

**Files:**
- Modify: `src/input/visual.rs` (`enter_visual_block_mode` at :122, `block_bounds` at :39)
- Create: `src/input/segments.rs`
- Modify: `src/input/mod.rs` (add `pub mod segments;` — check the file for the existing `pub mod visual;` list and match its style)

**Interfaces:**
- Consumes: `block_bounds` (visual.rs:39), `AppState::work_line_for_buffer` (app/mod.rs:741, `&self, usize -> Option<usize>`), `AppState::is_section_start` (app/mod.rs:765, `&self, usize -> bool`), `viewport::buffer_line_text(&sourceview5::Buffer, usize) -> String`.
- Produces: in visual.rs — `pub(crate) fn block_bounds_at(state: &AppState, line: usize) -> Option<(usize, usize)>` and `pub(crate) fn cursor_block_bounds(state: &AppState) -> Option<(usize, usize)>`. In segments.rs — `pub(crate) struct SegmentContext { pub segments: Vec<String>, pub cursor_index: usize, pub cursor_lines: Vec<crate::db::models::Line>, pub div1: i64, pub div2: i64 }`, `pub(crate) fn segment_context(state: &AppState, n: usize) -> Option<SegmentContext>`, `pub(crate) fn collect_neighbor_blocks(...)` (pure), `pub(crate) fn chat_user_message(...) -> String` (pure). Task 6 consumes `segment_context` + `chat_user_message`.

- [ ] **Step 1: Factor the structure-aware bounds out of `enter_visual_block_mode`**

In `src/input/visual.rs`, add above `enter_visual_block_mode` (this is a pure extraction of its current `bounds` block, generalized from `state.current_line` to any `line`):

```rust
/// Structure-aware block bounds at an arbitrary buffer line.
/// .txt-built buffers (text_file present AND line_map built): paragraphs and
/// speeches are blank-line/separator-delimited. DB-join buffers (no
/// text_file, or unreadable text_file fallback): no blank lines exist — the
/// block is the run of buffer lines mapping to the same work row.
/// None when `line` is out of range, a boundary line, or (DB-join) unmapped.
pub(crate) fn block_bounds_at(state: &AppState, line: usize) -> Option<(usize, usize)> {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return None;
    }
    let has_text_file = state
        .current_work
        .as_ref()
        .and_then(|w| w.text_file.as_ref())
        .is_some()
        && state.line_map.is_some();
    let buffer = &state.buffer;
    let is_blank_or_sep = |idx: usize| {
        let text = crate::input::viewport::buffer_line_text(buffer, idx);
        let t = text.trim();
        t.is_empty() || crate::db::line_types::is_separator(t)
    };
    if has_text_file {
        block_bounds(line_count, line, &is_blank_or_sep)
    } else {
        match state.work_line_for_buffer(line) {
            Some(row) => block_bounds(line_count, line, |idx| {
                is_blank_or_sep(idx) || state.work_line_for_buffer(idx) != Some(row)
            }),
            None => None,
        }
    }
}

/// The cursor's paragraph/speech block (see `block_bounds_at`).
pub(crate) fn cursor_block_bounds(state: &AppState) -> Option<(usize, usize)> {
    block_bounds_at(state, state.current_line)
}
```

Then shrink `enter_visual_block_mode`: delete its `has_text_file` computation and its entire `let bounds = { ... };` block, replacing them with:

```rust
    let bounds = cursor_block_bounds(state);
```

(The doc comment on `enter_visual_block_mode` keeps its behavioral text; everything after `let bounds` — the `unwrap_or((cursor, cursor))` fallback, `SelectionState { pending_ask: true }`, highlight, log — is unchanged.)

- [ ] **Step 2: Build + run the existing block tests (refactor safety net)**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins block_bounds 2>&1 | tail -3
```

Expected: build OK, 5 passed (existing `block_bounds_tests` unchanged).

- [ ] **Step 3: Write the failing tests for the pure neighbor walk + message builder**

Create `src/input/segments.rs`:

```rust
//! Cursor-segment context for the chat panel: the cursor's paragraph/speech
//! plus up to n neighbor segments on each side, truncated at section
//! (chapter/scene) starts and buffer edges.

use crate::app::AppState;

pub(crate) struct SegmentContext {
    /// Segment texts in buffer order (cursor's segment included).
    pub segments: Vec<String>,
    /// Index of the cursor's segment within `segments`.
    pub cursor_index: usize,
    /// The work lines of the CURSOR block only — used at save time for
    /// citations (`build_context_for_type`) and `<speaker>/<verse>` markup
    /// (`build_source_header`).
    pub cursor_lines: Vec<crate::db::models::Line>,
    pub div1: i64,
    pub div2: i64,
}

/// Collect up to `n` neighbor blocks on each side of `cursor_block` (pure).
/// `block_at(line)` returns the block containing `line` (None on boundary
/// lines). Walking upward stops at the buffer edge or once the current block
/// STARTS a section (can't cross further up); walking downward stops before a
/// block whose first line starts a section (don't cross into the next one).
/// Returns all blocks in buffer order, cursor block included.
pub(crate) fn collect_neighbor_blocks(
    line_count: usize,
    cursor_block: (usize, usize),
    n: usize,
    block_at: impl Fn(usize) -> Option<(usize, usize)>,
    is_section_start: impl Fn(usize) -> bool,
) -> Vec<(usize, usize)> {
    let mut blocks = vec![cursor_block];
    // Upward.
    let mut added = 0;
    let mut cur = cursor_block;
    while added < n && cur.0 > 0 && !is_section_start(cur.0) {
        let mut l = cur.0 - 1;
        let prev = loop {
            if let Some(b) = block_at(l) {
                break Some(b);
            }
            if l == 0 {
                break None;
            }
            l -= 1;
        };
        match prev {
            Some(b) => {
                blocks.insert(0, b);
                cur = b;
                added += 1;
            }
            None => break,
        }
    }
    // Downward.
    let mut added = 0;
    let mut cur = cursor_block;
    while added < n && cur.1 + 1 < line_count {
        let mut l = cur.1 + 1;
        let next = loop {
            if let Some(b) = block_at(l) {
                break Some(b);
            }
            l += 1;
            if l >= line_count {
                break None;
            }
        };
        match next {
            Some(b) if !is_section_start(b.0) => {
                blocks.push(b);
                cur = b;
                added += 1;
            }
            _ => break,
        }
    }
    blocks
}

/// Assemble the chat user message: work header, the consecutive segments with
/// the cursor's segment marked, and the reader's question (pure).
pub(crate) fn chat_user_message(
    genre: &str,
    title: &str,
    author: &str,
    unit_label: &str,
    scene_label: &str,
    segments: &[String],
    cursor_index: usize,
    question: &str,
) -> String {
    let mut ctx = String::new();
    for (i, seg) in segments.iter().enumerate() {
        if i == cursor_index {
            ctx.push_str("[READER'S CURSOR SEGMENT]\n");
        }
        ctx.push_str(seg);
        ctx.push_str("\n\n");
    }
    format!(
        "Work type: {}\nWork: {} by {}\n{}: {}\n\nContext (consecutive segments; the reader's cursor segment is marked):\n{}Reader's question:\n{}",
        genre, title, author, unit_label, scene_label, ctx, question,
    )
}

/// The cursor's segment ±`n` neighbors, resolved against the live buffer.
/// None when there is no work or the cursor sits on a boundary/unmapped line.
pub(crate) fn segment_context(state: &AppState, n: usize) -> Option<SegmentContext> {
    let cursor_block = crate::input::visual::cursor_block_bounds(state)?;
    let line_count = state.effective_line_count();
    let blocks = collect_neighbor_blocks(
        line_count,
        cursor_block,
        n,
        |l| crate::input::visual::block_bounds_at(state, l),
        |l| state.is_section_start(l),
    );
    let cursor_index = blocks.iter().position(|b| *b == cursor_block).unwrap_or(0);
    let segments: Vec<String> = blocks
        .iter()
        .map(|&(s, e)| {
            (s..=e)
                .map(|l| crate::input::viewport::buffer_line_text(&state.buffer, l))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    let work = state.current_work.as_ref()?;
    let cursor_lines: Vec<crate::db::models::Line> = (cursor_block.0..=cursor_block.1)
        .filter_map(|bl| {
            state
                .work_line_for_buffer(bl)
                .and_then(|wi| work.lines.get(wi).cloned())
        })
        .collect();
    let (div1, div2) = cursor_lines
        .first()
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    Some(SegmentContext { segments, cursor_index, cursor_lines, div1, div2 })
}

#[cfg(test)]
mod tests {
    use super::{chat_user_message, collect_neighbor_blocks};

    /// Blocks laid out as [0..1] [3..4] [6..7] [9..10] [12..13] with blank
    /// boundary lines between; section starts at the given lines.
    fn harness(section_starts: &[usize]) -> (usize, impl Fn(usize) -> Option<(usize, usize)> + '_, impl Fn(usize) -> bool + '_) {
        let blocks = [(0usize, 1usize), (3, 4), (6, 7), (9, 10), (12, 13)];
        let block_at = move |l: usize| blocks.iter().copied().find(|&(s, e)| l >= s && l <= e);
        let is_start = move |l: usize| section_starts.contains(&l);
        (14, block_at, is_start)
    }

    #[test]
    fn collects_n_neighbors_each_side() {
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (6, 7), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn truncates_at_buffer_edges() {
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (0, 1), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7)]);
        let (count, block_at, is_start) = harness(&[]);
        let got = collect_neighbor_blocks(count, (12, 13), 2, block_at, is_start);
        assert_eq!(got, vec![(6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn does_not_cross_section_start_downward() {
        // Block (9..10) starts a new section: walking down from (3..4) may
        // include (6..7) but must stop before (9..10).
        let (count, block_at, is_start) = harness(&[9]);
        let got = collect_neighbor_blocks(count, (3, 4), 2, block_at, is_start);
        assert_eq!(got, vec![(0, 1), (3, 4), (6, 7)]);
    }

    #[test]
    fn does_not_cross_section_start_upward() {
        // Block (6..7) starts a section: walking up from (6..7) stops
        // immediately (its own start is a section start).
        let (count, block_at, is_start) = harness(&[6]);
        let got = collect_neighbor_blocks(count, (6, 7), 2, block_at, is_start);
        assert_eq!(got, vec![(6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn user_message_marks_cursor_segment() {
        let segs = vec!["before".to_string(), "here".to_string(), "after".to_string()];
        let msg = chat_user_message(
            "novel", "Bleak House", "Charles Dickens", "Chapter", "Chapter 7",
            &segs, 1, "Why the fog?",
        );
        assert!(msg.contains("Work type: novel"));
        assert!(msg.contains("Chapter: Chapter 7"));
        assert!(msg.contains("[READER'S CURSOR SEGMENT]\nhere"));
        assert!(!msg.contains("[READER'S CURSOR SEGMENT]\nbefore"));
        assert!(msg.trim_end().ends_with("Reader's question:\nWhy the fog?"));
    }
}
```

Register the module: in `src/input/mod.rs` add `pub mod segments;` alongside the existing `pub mod visual;`.

- [ ] **Step 4: Run tests**

```bash
cargo test --bins segments 2>&1 | tail -3 && cargo test --bins block_bounds 2>&1 | tail -3
```

Expected: 5 passed (segments) + 5 passed (block_bounds).

- [ ] **Step 5: Commit**

```bash
git add src/input/visual.rs src/input/segments.rs src/input/mod.rs
git commit -m "feat: shared block_bounds_at + segment_context for chat prompts"
```

---

### Task 3: layout flag, `ToggleChatLayout` action, Tab/`-` rebinds

**Files:**
- Modify: `src/app/layout.rs` (`apply_card_sizing` at :375)
- Modify: `src/app/mod.rs` (AppState struct ~:208, constructor literal ~:1627, resize-tick call sites :2034/:2051/:2157)
- Modify: `src/app/layout.rs:290`, `src/input/navigation.rs:654`, `src/input/actions/settings.rs:33,355,479` (other `apply_card_sizing` call sites)
- Create: `src/input/actions/chat.rs`; register in `src/input/actions/mod.rs` (`pub mod chat;` next to `pub mod journal;`)
- Modify: `src/input/actions/mod.rs` (Action enum), `src/input/keymap_config.rs`, `src/input/keymap.rs` (dispatch arm), `src/ui/keybinds_overlay.rs`
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` (separate repo; commit there)

**Interfaces:**
- Produces: `AppState.chat_layout_open: bool`; `apply_card_sizing(..., chat_open: bool)` (new last param); `Action::ToggleChatLayout`; `chat::toggle_chat_layout(state_rc)`, `chat::close_chat_layout(&mut AppState)`, `chat::reapply_card_margins(&AppState)` — Tasks 4/5 extend these.

- [ ] **Step 1: Asymmetric margin branch**

In `src/app/layout.rs`, change `apply_card_sizing`'s signature and margin logic:

```rust
pub(crate) fn apply_card_sizing(
    content_hbox: &gtk4::Box,
    window_width: i32,
    column_width: u32,
    column_count: u8,
    translations: bool,
    chat_open: bool,
) {
    let ww = window_width.max(0);
    let target = target_card_width(ww, column_width, column_count, translations);
    // Reserve room for margins first; if that overflows, the card itself shrinks.
    let card_w = target.min(ww.max(1));
    let slack = ww - card_w;
    if chat_open {
        // Chat layout: pin the card flush right (keep the normal right
        // margin); ALL remaining slack becomes the left margin, which is the
        // space the chat panel renders over.
        let end = (slack / 2).clamp(0, CARD_OUTER_MARGIN).min(slack.max(0));
        let start = (slack - end).max(0);
        content_hbox.set_width_request(card_w);
        content_hbox.set_margin_start(start);
        content_hbox.set_margin_end(end);
        crate::log_fmt!(
            "CARD_SIZING: chat ww={} card_w={} start={} end={}",
            ww, card_w, start, end
        );
        return;
    }
    let margin = (slack / 2).clamp(0, CARD_OUTER_MARGIN);
    content_hbox.set_width_request(card_w);
    content_hbox.set_margin_start(margin);
    content_hbox.set_margin_end(margin);
    crate::log_fmt!(
        "CARD_SIZING: ww={} col_cfg={} cols={} target={} card_w={} margin={}",
        ww, column_width as i32, column_count, target, card_w, margin
    );
}
```

Update ALL 7 call sites to pass the flag from state: at each site a `&AppState` (or borrow) is in scope — pass `s.chat_layout_open` (in `apply_column_layout` and the tick's three branches, `settings.rs`, `navigation.rs`; use the exact local state binding present at each site). Find them with:

```bash
rg -n "apply_card_sizing\(" src/
```

- [ ] **Step 2: AppState field**

In `src/app/mod.rs` struct (near `pub content_hbox: gtk4::Box,`):

```rust
    /// Chat layout (Tab): card pinned right, left chat panel visible.
    pub chat_layout_open: bool,
```

Constructor literal (~:1627 block): add `chat_layout_open: false,`.

- [ ] **Step 3: Action + chat module + dispatch**

`src/input/actions/mod.rs` — in the enum's Display group (near `ToggleDim`):

```rust
    /// Tab: toggle the left chat panel layout (card pins right).
    ToggleChatLayout,
```

Add `| Action::ToggleChatLayout` to the Display arm of `category()`, and `Action::ToggleChatLayout => "ToggleChatLayout",` to `name()`. Register the new module: `pub mod chat;` in the actions `mod.rs` module list.

Create `src/input/actions/chat.rs`:

```rust
//! Chat layout (Tab): left chat panel + right-pinned card. This task ships
//! the layout toggle only; the panel widget and conversation land in later
//! tasks of the chat-layout plan.

use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Minimum freed left space (px) required to open the chat layout.
const CHAT_MIN_PANEL_W: i32 = 500;

/// Re-apply the card margins for the current chat_layout_open value.
pub(crate) fn reapply_card_margins(s: &AppState) {
    let ww = s.window.width().max(0);
    crate::app::layout::apply_card_sizing(
        &s.content_hbox,
        ww,
        crate::app::layout::effective_column_width(s),
        s.column_count(),
        s.translations_visible,
        s.chat_layout_open,
    );
}

pub(crate) fn close_chat_layout(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat_layout_open = false;
    reapply_card_margins(s);
    s.input_mode = crate::app::InputMode::Reader;
    crate::logging::log("CHAT: layout closed");
}

pub(crate) fn toggle_chat_layout(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    if s.chat_layout_open {
        // Panel already open: Tab (from reader focus) cycles INTO the panel;
        // full cycle behavior lands with the focus task. For now close.
        close_chat_layout(&mut s);
        return;
    }
    let ww = s.window.width().max(0);
    let (card_w, _) = crate::app::layout::main_card_rect(&s);
    let free = ww - card_w - 2 * crate::app::layout::CARD_OUTER_MARGIN;
    if free < CHAT_MIN_PANEL_W {
        crate::ui::toast::show_transient(
            &s.chapter_toast,
            "No room for chat panel at this layout",
            3,
        );
        return;
    }
    s.chat_layout_open = true;
    reapply_card_margins(&s);
    crate::logging::log(&format!("CHAT: layout opened (free={}px)", free));
}
```

Dispatch arm in `src/input/keymap.rs` `dispatch_action` (Display group, near `ToggleDim`):

```rust
        ToggleChatLayout => crate::input::actions::chat::toggle_chat_layout(state),
```

- [ ] **Step 4: Rebinds (compiled + tests)**

`src/input/keymap_config.rs`:
- In `media_bindings()` (line ~281): DELETE `(KeyCombo::plain("Tab"), Action::TogglePause),` and ADD `(KeyCombo::plain("minus"), Action::TogglePause),` with the comment `// '-' is the pause toggle (Tab moved to the chat layout).`
- In `display_bindings()` (line ~329): DELETE `(KeyCombo::plain("minus"), Action::TogglePreviousWork),` and ADD `(KeyCombo::plain("Tab"), Action::ToggleChatLayout),` with the comment `// Tab toggles the chat layout; TogglePreviousWork is unbound (Ctrl+minus recent picker covers it).`
- Tests: update any assertion on `plain("Tab")` / `plain("minus")` to the new actions, and add:

```rust
        assert_eq!(m.get(&KeyCombo::plain("Tab")), Some(&Action::ToggleChatLayout));
        assert_eq!(m.get(&KeyCombo::plain("minus")), Some(&Action::TogglePause));
        assert_eq!(m.get(&KeyCombo::ctrl("minus")), Some(&Action::OpenRecentPicker));
```

- [ ] **Step 5: Stow keymap.json + keybinds overlay**

`~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`: line 107 `{"key": "Tab", "action": "TogglePause"}` → `{"key": "Tab", "action": "ToggleChatLayout"}`; line 67 `{"key": "minus", "action": "TogglePreviousWork"}` → `{"key": "minus", "action": "TogglePause"}`. Validate `jq . <file> > /dev/null && echo OK`, commit in ~/tty-dotfiles: `git commit -m "linux-lit: Tab chat layout; pause moves to minus"` (that file only; do not push).

`src/ui/keybinds_overlay.rs`:
- `TAB_KEY` (line ~66): `key("Tab", "", "chat layout", "", &[("C-Tab", "last overlay")])`
- HOME_ROW `-` (line ~79): `key("-", "_", "play/pause", "", &[("C--", "recent")])`
- `describe()`: add `"chat layout" => "Toggle the left chat panel layout: the reading card pins to the right at unchanged size and the freed left space becomes a Claude chat panel for journal Q&A. -> ToggleChatLayout arm -> chat::toggle_chat_layout — src/input/actions/chat.rs",` and DELETE the now-orphaned `"prev work"` arm (verify first that no other keycap label uses it: `rg -n '"prev work"' src/ui/keybinds_overlay.rs` must return only the describe arm before deleting). The `"play/pause"` arm already exists.
- Three-pass cross-reference per the `update-cairo-keybinds-overlay` discipline (labels vs keymap_config truth; every label has a describe arm).

- [ ] **Step 6: Build, test, commit**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins keymap 2>&1 | tail -3
git add src/app/layout.rs src/app/mod.rs src/input/navigation.rs src/input/actions/settings.rs src/input/actions/chat.rs src/input/actions/mod.rs src/input/keymap_config.rs src/input/keymap.rs src/ui/keybinds_overlay.rs
git commit -m "feat: ToggleChatLayout on Tab with right-pinned card; pause moves to minus"
```

Expected: build OK; keymap tests green.

---

### Task 4: chat panel widget + theme CSS

**Files:**
- Create: `src/ui/chat_panel.rs`; register `pub mod chat_panel;` in `src/ui/mod.rs`
- Modify: `src/theme.rs` (`generate_css`, ~:695-933)
- Modify: `src/app/mod.rs` (build panel in `build_window`, attach to `outer_overlay` before `window.set_child` at :1600, store on AppState, size in the resize tick)
- Modify: `src/input/actions/chat.rs` (show/hide/size on toggle)

**Interfaces:**
- Consumes: `AskCard` (src/ui/ask_card.rs) — read that file first; embed one exactly the way `journal_overlay.rs` embeds its `ask_host` (constructor, `open(title, hint, block_fill, block_fg)`, `take_text()`, `feed_vim_key`, `paste_text`, font application). `main_card_rect(s) -> (i32, i32)`.
- Produces: `pub struct ChatPanel` with `pub fn new(...) -> Self`, `pub container: gtk4::Box`, `pub fn set_header(&self, title: &str, author: &str, scene: &str)`, `pub fn size_to(&self, w: i32, h: i32)`, `pub fn show(&self)` / `pub fn hide(&self)`, `pub fn render_rows(&self, rows: &[TranscriptRow])` where `pub enum TranscriptRow { Question(String), Answer(String), Chip(String), Error(String), Thinking, SavedMark }`, ask-input passthroughs `open_input`, `take_input_text`, `feed_input_vim_key`, `paste_input_text`. `AppState.chat_panel: crate::ui::chat_panel::ChatPanel`. Tasks 5-8 consume all of these.

- [ ] **Step 1: Read the AskCard host API**

Read `src/ui/ask_card.rs` end-to-end and `src/ui/journal_overlay.rs`'s `ask_host` field + `open_ask_card`/`take_ask_text`/`feed_ask_vim_key`/`paste_ask_text` wrappers (journal_overlay.rs:907-949). The chat panel replicates that embedding one-for-one (same constructor args; the `return_focus` widget is the panel's transcript scroll).

- [ ] **Step 2: The widget**

Create `src/ui/chat_panel.rs`:

```rust
//! Left chat panel for the Tab chat layout. "Bare on root": no card chrome —
//! labels render directly over the themed root background using CSS classes
//! emitted by theme::generate_css (.chat-panel*, .chat-q, .chat-a, ...).

use gtk4::prelude::*;

pub enum TranscriptRow {
    Question(String),
    Answer(String),
    /// Context chip: italic excerpt of the cursor segment an exchange was
    /// asked from (shown when it differs from the previous exchange).
    Chip(String),
    Error(String),
    Thinking,
    SavedMark,
}

pub struct ChatPanel {
    pub container: gtk4::Box,
    header_title: gtk4::Label,
    header_scene: gtk4::Label,
    transcript_box: gtk4::Box,
    transcript_scroll: gtk4::ScrolledWindow,
    input: crate::ui::ask_card::AskCard,
}

impl ChatPanel {
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        container.add_css_class("chat-panel");
        container.set_margin_start(24);
        container.set_visible(false);

        let header_title = gtk4::Label::new(None);
        header_title.set_halign(gtk4::Align::Start);
        header_title.add_css_class("chat-panel-header");
        let header_scene = gtk4::Label::new(None);
        header_scene.set_halign(gtk4::Align::Start);
        header_scene.add_css_class("chat-panel-header");
        let rule = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        rule.add_css_class("chat-panel-rule");

        let transcript_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        let transcript_scroll = gtk4::ScrolledWindow::new();
        transcript_scroll.set_child(Some(&transcript_box));
        transcript_scroll.set_vexpand(true);
        transcript_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let input = crate::ui::ask_card::AskCard::new(transcript_scroll.clone().upcast());
        input.container().add_css_class("chat-input");

        container.append(&header_title);
        container.append(&header_scene);
        container.append(&rule);
        container.append(&transcript_scroll);
        container.append(input.container());

        Self { container, header_title, header_scene, transcript_box, transcript_scroll, input }
    }

    pub fn set_header(&self, title: &str, author: &str, scene: &str) {
        self.header_title.set_text(&format!("{} \u{2014} {}", title, author));
        self.header_scene.set_text(scene);
    }

    pub fn size_to(&self, w: i32, h: i32) {
        self.container.set_width_request(w.max(0));
        self.container.set_height_request(h.max(0));
    }

    pub fn show(&self) {
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Rebuild the transcript from rows, newest last, and scroll to the end.
    pub fn render_rows(&self, rows: &[TranscriptRow]) {
        while let Some(child) = self.transcript_box.first_child() {
            self.transcript_box.remove(&child);
        }
        for row in rows {
            let (text, class) = match row {
                TranscriptRow::Question(t) => (t.as_str(), "chat-q"),
                TranscriptRow::Answer(t) => (t.as_str(), "chat-a"),
                TranscriptRow::Chip(t) => (t.as_str(), "chat-chip"),
                TranscriptRow::Error(t) => (t.as_str(), "chat-error"),
                TranscriptRow::Thinking => ("thinking\u{2026}", "chat-a"),
                TranscriptRow::SavedMark => ("\u{2713} saved", "chat-saved"),
            };
            let label = gtk4::Label::new(Some(text));
            label.set_wrap(true);
            label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
            label.set_halign(gtk4::Align::Start);
            label.set_xalign(0.0);
            label.set_selectable(false);
            label.add_css_class(class);
            self.transcript_box.append(&label);
        }
        let adj = self.transcript_scroll.vadjustment();
        glib::idle_add_local_once({
            let adj = adj.clone();
            move || adj.set_value(adj.upper())
        });
    }

    // ---- ask-input passthroughs (mirror journal_overlay's ask_host wrappers)
    pub fn open_input(&self, title: &str, hint: &str, block_fill: &str, block_fg: &str) {
        self.input.open(title, hint, block_fill, block_fg);
    }
    pub fn take_input_text(&self) -> String {
        self.input.take_text()
    }
    pub fn feed_input_vim_key(&self, k: crate::input::vim::VimKey) -> crate::input::vim::EditorAction {
        self.input.feed_vim_key(k)
    }
    pub fn paste_input_text(&self, t: &str) {
        self.input.paste_text(t)
    }
}
```

(If `AskCard`'s real method names differ — e.g. `container()` accessor, `feed_vim_key` — match the ACTUAL names found in Step 1; the journal overlay wrappers name the canonical set. Do not fork AskCard.)

- [ ] **Step 3: Theme CSS**

In `src/theme.rs` `generate_css`, following the `.title-bar`/`.concordance-bar` interpolation pattern, add (using the existing `{root}` / `{fg}` / `{dim}` / `{cursor_bg}` interpolation variables of that function):

```css
.chat-panel {{ background-color: transparent; }}
.chat-panel-header {{ color: {dim}; font-variant: small-caps; font-size: 13px; }}
.chat-panel-rule {{ background-color: alpha({fg}, 0.25); }}
.chat-q {{ color: {fg}; }}
.chat-a {{ color: alpha({fg}, 0.72); }}
.chat-chip {{ color: {dim}; font-style: italic; border-left: 2px solid alpha({fg}, 0.35); padding-left: 8px; }}
.chat-error {{ color: alpha({fg}, 0.55); font-style: italic; }}
.chat-saved {{ color: {dim}; }}
.chat-input {{ border: 1px solid alpha({fg}, 0.30); border-radius: 6px; }}
```

(Adapt the interpolation to `generate_css`'s exact `format!` variable names — read the surrounding lines and follow them.)

- [ ] **Step 4: Build in build_window, attach, store, resize**

In `src/app/mod.rs` `build_window`: before the `outer_overlay` assembly (~:1586), create `let chat_panel = crate::ui::chat_panel::ChatPanel::new();`, then `chat_panel.container.set_halign(gtk4::Align::Start); chat_panel.container.set_valign(gtk4::Align::Center);` and `outer_overlay.add_overlay(&chat_panel.container);` (next to the title_bar add_overlay). Add `pub chat_panel: crate::ui::chat_panel::ChatPanel,` to AppState (near `title_bar`) and `chat_panel,` to the constructor literal.

In the resize tick's plain-resize branch (after the existing `apply_card_sizing` call at ~:2157), add:

```rust
                if s.chat_layout_open {
                    crate::input::actions::chat::size_panel(&s);
                }
```

In `src/input/actions/chat.rs` add the sizing helper and wire the toggle:

```rust
/// Size the panel to the freed left space at the card's height.
pub(crate) fn size_panel(s: &AppState) {
    let ww = s.window.width().max(0);
    let (card_w, card_h) = crate::app::layout::main_card_rect(s);
    let end = crate::app::layout::CARD_OUTER_MARGIN;
    // left outer margin (24) + gap to the card (16)
    let w = ww - card_w - end - 24 - 16;
    s.chat_panel.size_to(w, card_h);
}
```

In `toggle_chat_layout`'s open path (after `reapply_card_margins`): `size_panel(&s); set_panel_header(&s); s.chat_panel.show();` and in `close_chat_layout`: `s.chat_panel.hide();`. Add:

```rust
pub(crate) fn set_panel_header(s: &AppState) {
    let Some(w) = s.current_work.as_ref() else { return };
    let (d1, d2) = s
        .work_line_for_buffer(s.current_line)
        .and_then(|wi| w.lines.get(wi))
        .map(|l| (l.div1, l.div2))
        .unwrap_or((0, 0));
    let scene = crate::app::scene_synopsis::scene_label(d1, d2);
    s.chat_panel.set_header(&w.title, &w.author, &scene);
}
```

- [ ] **Step 5: Build, commit**

```bash
cargo build 2>&1 | tail -2
git add src/ui/chat_panel.rs src/ui/mod.rs src/theme.rs src/app/mod.rs src/input/actions/chat.rs
git commit -m "feat: chat panel widget, bare-on-root theme CSS, sizing"
```

---

### Task 5: input modes + three-way Tab focus cycle

**Files:**
- Modify: `src/app/mod.rs` (InputMode enum :88-155)
- Modify: `src/input/keymap.rs` (mode dispatch match ~:124-170; new handlers near `handle_journal_key`; `ask_vim_intercept` at :679 is reused, not changed)
- Modify: `src/input/actions/chat.rs`

**Interfaces:**
- Consumes: `ask_vim_intercept` (keymap.rs:679 — signature verbatim there), `ChatPanel` passthroughs from Task 4.
- Produces: `InputMode::ChatPrompt`, `InputMode::ChatTranscript`; `chat::focus_prompt(&mut AppState)`, `chat::focus_transcript(&mut AppState)`, `chat::focus_reader(&mut AppState)`; keymap handlers `handle_chat_prompt_key`, `handle_chat_transcript_key`. `chat::submit_chat_prompt(state_rc)` is stubbed here (toast) and implemented in Task 6. Behavior contract: Tab cycles prompt → transcript → reader → prompt; Ctrl+Tab closes from ANY of the three; reader keys fully live when reader has focus.

- [ ] **Step 1: Modes**

Add to `InputMode` (with doc comments in the enum's style):

```rust
    /// Chat layout: the panel's vim prompt owns keys (Tab cycles to the
    /// transcript; Ctrl+Tab closes the panel; Ctrl+Enter submits).
    ChatPrompt,
    /// Chat layout: the transcript owns keys (j/k exchange cursor, s saves,
    /// Tab cycles to the reader, Ctrl+Tab closes).
    ChatTranscript,
```

- [ ] **Step 2: Focus helpers + toggle wiring in chat.rs**

```rust
pub(crate) fn focus_prompt(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatPrompt;
    s.chat_panel.open_input(
        "Ask about this passage",
        "Ctrl+Enter send \u{b7} Tab cycle",
        &s.theme.cursor_bg,
        &s.theme.cursor_fg,
    );
    set_panel_header(s);
}

pub(crate) fn focus_transcript(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::ChatTranscript;
}

pub(crate) fn focus_reader(s: &mut AppState) {
    s.input_mode = crate::app::InputMode::Reader;
}
```

Rewrite `toggle_chat_layout`'s open-branch tail to end with `focus_prompt(&mut s);`, and change the already-open branch from `close_chat_layout` to `focus_prompt(&mut s);` (Tab from reader focus now cycles INTO the prompt; closing is Ctrl+Tab's job). `close_chat_layout` keeps setting `InputMode::Reader`.

- [ ] **Step 3: Key routing**

In the keymap.rs mode-dispatch match add:

```rust
            crate::app::InputMode::ChatPrompt => handle_chat_prompt_key(state, key_name, key_char, is_ctrl),
            crate::app::InputMode::ChatTranscript => handle_chat_transcript_key(state, key_name, is_ctrl),
```

New handlers (place near `handle_journal_key`):

```rust
/// Chat prompt focus: Tab cycles to the transcript BEFORE the vim editor can
/// consume it; Ctrl+Tab closes the panel; everything else feeds the embedded
/// AskCard vim editor via the shared ask_vim_intercept (Ctrl+Enter submits).
fn handle_chat_prompt_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    key_char: Option<char>,
    is_ctrl: bool,
) -> bool {
    if (key_name == "Tab" || key_name == "ISO_Left_Tab") && is_ctrl {
        crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
        return true;
    }
    if key_name == "Tab" || key_name == "ISO_Left_Tab" {
        crate::input::actions::chat::focus_transcript(&mut state.borrow_mut());
        return true;
    }
    match ask_vim_intercept(
        true,
        key_name,
        key_char,
        is_ctrl,
        state,
        |st, k| st.borrow().chat_panel.feed_input_vim_key(k),
        crate::input::actions::chat::submit_chat_prompt,
        |_st| {}, // Escape closes nothing in the chat prompt (vim Normal only)
        |st, t| st.borrow().chat_panel.paste_input_text(t),
    ) {
        AskIntercept::Consumed => true,
        AskIntercept::NotHandled => true, // prompt focus consumes everything
    }
}

/// Chat transcript focus: j/k move the exchange cursor, s saves the selected
/// exchange (Task 7), Tab cycles to the reader, Ctrl+Tab closes.
fn handle_chat_transcript_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    is_ctrl: bool,
) -> bool {
    match key_name {
        "Tab" | "ISO_Left_Tab" if is_ctrl => {
            crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
            true
        }
        "Tab" | "ISO_Left_Tab" => {
            crate::input::actions::chat::focus_reader(&mut state.borrow_mut());
            true
        }
        "j" => {
            crate::input::actions::chat::transcript_cursor_move(&mut state.borrow_mut(), 1);
            true
        }
        "k" => {
            crate::input::actions::chat::transcript_cursor_move(&mut state.borrow_mut(), -1);
            true
        }
        "s" => {
            crate::input::actions::chat::save_selected_exchange(state);
            true
        }
        "Escape" => {
            crate::input::actions::chat::focus_reader(&mut state.borrow_mut());
            true
        }
        _ => true,
    }
}
```

Stubs in chat.rs so this task compiles standalone (bodies replaced in Tasks 6-7):

```rust
pub(crate) fn submit_chat_prompt(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "Chat send lands in the next task", 2);
}

pub(crate) fn transcript_cursor_move(_s: &mut AppState, _delta: i32) {}

pub(crate) fn save_selected_exchange(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    crate::ui::toast::show_transient(&s.chapter_toast, "Save lands in a later task", 2);
}
```

- [ ] **Step 4: Ctrl+Tab shadow + reader-focus interactions**

In `dispatch_action`, change the `ToggleLastOverlay` arm to shadow while the panel is open:

```rust
        ToggleLastOverlay => {
            if state.borrow().chat_layout_open {
                crate::input::actions::chat::close_chat_layout(&mut state.borrow_mut());
            } else {
                crate::input::actions::gloss::toggle_last_overlay(state)
            }
        }
```

(Compute the flag and drop the borrow before the second borrow_mut — write it exactly as above; the `if` condition's temporary borrow ends at the `{`.)

Reader focus with panel open needs the context chip to live-follow the cursor; that lands in Task 6's render. Nothing else: reader keys already work because reader focus IS `InputMode::Reader`.

- [ ] **Step 5: Build, commit**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | rg "test result"
git add src/app/mod.rs src/input/keymap.rs src/input/actions/chat.rs
git commit -m "feat: chat focus cycle (prompt/transcript/reader) + Ctrl+Tab close shadow"
```

---

### Task 6: session state + send flow

**Files:**
- Modify: `src/input/actions/chat.rs`, `src/app/mod.rs` (AppState field + init)

**Interfaces:**
- Consumes: `segment_context` / `chat_user_message` (Task 2), `run_claude_chat_request` + `ChatTurn` (Task 1), `gloss::journal_qa_prompt(work_type)`, `gloss::genre_unit(work_type) -> (genre, unit, units)`, `gloss::build_context_for_type(work, lines, "reader-gloss") -> Option<GlossContext>`, `echoes::build_source_header(&[Line], speaker) -> String`, `scene_synopsis::scene_label(d1, d2)`, `ChatPanel::render_rows`.
- Produces: `pub(crate) struct Exchange { pub question: String, pub answer: String, pub chip: String, pub user_msg: String, pub div1: i64, pub div2: i64, pub start_citation: String, pub end_citation: String, pub source_markup: String, pub saved_id: Option<i64> }`; `pub(crate) struct ChatState { pub exchanges: Vec<Exchange>, pub cursor: usize, pub revision_of: Option<i64>, pub pending: bool }` as `AppState.chat`; real `submit_chat_prompt` + `transcript_cursor_move` + `render_transcript(&AppState)`. Task 7 consumes `Exchange`/`ChatState`; Task 8 consumes `revision_of`.

- [ ] **Step 1: State structs + AppState field**

In chat.rs add the two structs exactly as in Interfaces (with `Default` derived on `ChatState`). In `src/app/mod.rs`: `pub chat: crate::input::actions::chat::ChatState,` + constructor `chat: Default::default(),`.

- [ ] **Step 2: Send flow**

Replace the `submit_chat_prompt` stub:

```rust
pub(crate) fn submit_chat_prompt(state_rc: &Rc<RefCell<AppState>>) {
    // Revision mode: the prompt text is an instruction, not a question.
    if state_rc.borrow().chat.revision_of.is_some() {
        crate::input::actions::chat_revision::submit_revision(state_rc);
        return;
    }
    let (question, system, user_msg, turns, model, chip, meta) = {
        let s = state_rc.borrow();
        if s.chat.pending {
            crate::ui::toast::show_transient(&s.chapter_toast, "Waiting for the previous reply\u{2026}", 2);
            return;
        }
        let question = s.chat_panel.take_input_text().trim().to_string();
        if question.is_empty() {
            return;
        }
        let Some(work) = s.current_work.as_ref() else { return };
        let Some(seg) = crate::input::segments::segment_context(&s, 2) else {
            crate::ui::toast::show_transient(&s.chapter_toast, "No passage at the cursor", 2);
            return;
        };
        let Some(gctx) = crate::gloss::build_context_for_type(work, &seg.cursor_lines, "reader-gloss") else {
            crate::ui::toast::show_transient(&s.chapter_toast, "No passage at the cursor", 2);
            return;
        };
        let source_markup =
            crate::input::actions::echoes::build_source_header(&seg.cursor_lines, &gctx.speaker);
        let (genre, unit, _units) = crate::gloss::genre_unit(&work.work_type);
        let scene = crate::app::scene_synopsis::scene_label(seg.div1, seg.div2);
        let mut unit_label = unit.to_string();
        if let Some(c) = unit_label.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        let user_msg = crate::input::segments::chat_user_message(
            genre, &work.title, &work.author, &unit_label, &scene,
            &seg.segments, seg.cursor_index, &question,
        );
        // Prior turns: each exchange contributes its full user_msg (context
        // embedded, so history stays coherent as the cursor moves) + answer.
        let mut turns: Vec<crate::claude::ChatTurn> = Vec::new();
        for e in &s.chat.exchanges {
            turns.push(crate::claude::ChatTurn { role: "user", content: e.user_msg.clone() });
            turns.push(crate::claude::ChatTurn { role: "assistant", content: e.answer.clone() });
        }
        turns.push(crate::claude::ChatTurn { role: "user", content: user_msg.clone() });
        let chip: String = seg.segments[seg.cursor_index].chars().take(120).collect();
        let meta = (
            seg.div1,
            seg.div2,
            gctx.start_citation.clone(),
            gctx.end_citation.clone(),
            source_markup,
        );
        (
            question,
            crate::gloss::journal_qa_prompt(&work.work_type),
            user_msg,
            turns,
            s.config.claude_model.clone(),
            chip,
            meta,
        )
    };

    {
        let mut s = state_rc.borrow_mut();
        s.chat.pending = true;
        render_transcript_with_thinking(&s, &question, &chip);
    }

    let (div1, div2, start_citation, end_citation, source_markup) = meta;
    let question_ok = question.clone();
    let question_err = question;
    crate::input::actions::claude_bridge::run_claude_chat_request(
        state_rc,
        system,
        turns,
        model,
        move |st, answer| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            s.chat.exchanges.push(Exchange {
                question: question_ok.clone(),
                answer,
                chip: chip.clone(),
                user_msg: user_msg.clone(),
                div1,
                div2,
                start_citation: start_citation.clone(),
                end_citation: end_citation.clone(),
                source_markup: source_markup.clone(),
                saved_id: None,
            });
            s.chat.cursor = s.chat.exchanges.len() - 1;
            render_transcript(&s);
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.chat.pending = false;
            render_transcript_with_error(&s, msg);
            // Restore the failed question for retry.
            s.chat_panel.paste_input_text(&question_err);
        },
    );
}
```

- [ ] **Step 3: Rendering + cursor move**

```rust
fn transcript_rows(s: &AppState) -> Vec<crate::ui::chat_panel::TranscriptRow> {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = Vec::new();
    let mut prev_chip: Option<&str> = None;
    for (i, e) in s.chat.exchanges.iter().enumerate() {
        if prev_chip != Some(e.chip.as_str()) {
            rows.push(R::Chip(e.chip.clone()));
        }
        prev_chip = Some(e.chip.as_str());
        let marker = if i == s.chat.cursor { "\u{25b8} " } else { "" };
        rows.push(R::Question(format!("{}Q: {}", marker, e.question)));
        rows.push(R::Answer(e.answer.clone()));
        if e.saved_id.is_some() {
            rows.push(R::SavedMark);
        }
    }
    rows
}

pub(crate) fn render_transcript(s: &AppState) {
    s.chat_panel.render_rows(&transcript_rows(s));
}

fn render_transcript_with_thinking(s: &AppState, question: &str, chip: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = transcript_rows(s);
    rows.push(R::Chip(chip.to_string()));
    rows.push(R::Question(format!("Q: {}", question)));
    rows.push(R::Thinking);
    s.chat_panel.render_rows(&rows);
}

fn render_transcript_with_error(s: &AppState, msg: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    let mut rows = transcript_rows(s);
    rows.push(R::Error(msg.to_string()));
    s.chat_panel.render_rows(&rows);
}
```

Replace the `transcript_cursor_move` stub:

```rust
pub(crate) fn transcript_cursor_move(s: &mut AppState, delta: i32) {
    let n = s.chat.exchanges.len();
    if n == 0 {
        return;
    }
    let cur = s.chat.cursor as i32 + delta;
    s.chat.cursor = cur.clamp(0, n as i32 - 1) as usize;
    render_transcript(s);
}
```

- [ ] **Step 4: Build, commit**

```bash
cargo build 2>&1 | tail -2
git add src/input/actions/chat.rs src/app/mod.rs
git commit -m "feat: chat session state, multi-turn send flow, transcript render"
```

(Note: `chat_revision::submit_revision` doesn't exist yet — for THIS task's compile, add a temporary module stub at the bottom of chat.rs: `pub(crate) mod chat_revision { use super::*; pub(crate) fn submit_revision(state_rc: &Rc<RefCell<AppState>>) { let s = state_rc.borrow(); crate::ui::toast::show_transient(&s.chapter_toast, "Revision lands in a later task", 2); } }` — Task 8 replaces it.)

---

### Task 7: curated save (`s`) + saved-entry pivot

**Files:**
- Modify: `src/input/actions/chat.rs`, `src/input/actions/journal.rs:1497` (visibility only)

**Interfaces:**
- Consumes: `db::journal::save_passage_page(conn, work_abbrev, div1, div2, start_citation, end_citation, source_text, question, answer, claude_model) -> Result<i64>` (db/journal.rs:208); `db::queries::open_db_rw()`; `Work.canonical_abbrev`.
- Produces: real `save_selected_exchange`; `render_saved_entry(&AppState, q, a)` (Task 8 reuses); `journal::purge_journal_audio` becomes `pub(crate)`.

- [ ] **Step 1: Make `purge_journal_audio` pub(crate)**

`src/input/actions/journal.rs:1497`: `fn purge_journal_audio(` → `pub(crate) fn purge_journal_audio(`.

- [ ] **Step 2: Save + pivot**

Replace the `save_selected_exchange` stub:

```rust
/// `s` on the transcript: save the selected exchange as a passage journal
/// page, mark it, and pivot the panel into the revision loop on that entry.
pub(crate) fn save_selected_exchange(state_rc: &Rc<RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    let idx = s.chat.cursor;
    let Some(e) = s.chat.exchanges.get(idx) else { return };
    let Some(work) = s.current_work.as_ref() else { return };
    let abbrev = work.canonical_abbrev.clone();
    let model = s.config.claude_model.clone();
    let (q, a) = (e.question.clone(), e.answer.clone());
    let saved = crate::db::queries::open_db_rw().and_then(|conn| {
        crate::db::journal::save_passage_page(
            &conn, &abbrev, e.div1, e.div2,
            &e.start_citation, &e.end_citation, &e.source_markup,
            &e.question, &e.answer, &model,
        )
    });
    match saved {
        Ok(id) => {
            s.chat.exchanges[idx].saved_id = Some(id);
            s.chat.revision_of = Some(id);
            render_saved_entry(&s, &q, &a);
            s.chat_panel.open_input(
                "Revise this entry",
                "Ctrl+Enter send \u{b7} s update \u{b7} Tab cycle",
                &s.theme.cursor_bg,
                &s.theme.cursor_fg,
            );
            s.input_mode = crate::app::InputMode::ChatPrompt;
            crate::ui::toast::show_transient(&s.chapter_toast, "Saved", 2);
            crate::logging::log(&format!("CHAT: saved exchange as journal page {}", id));
        }
        Err(err) => {
            crate::ui::toast::show_transient(&s.chapter_toast, "Save failed", 3);
            crate::logging::log(&format!("CHAT: save failed: {}", err));
        }
    }
}

/// Revision view: the panel content IS the saved entry (Q + A), no history.
pub(crate) fn render_saved_entry(s: &AppState, question: &str, answer: &str) {
    use crate::ui::chat_panel::TranscriptRow as R;
    s.chat_panel.render_rows(&[
        R::SavedMark,
        R::Question(format!("Q: {}", question)),
        R::Answer(answer.to_string()),
    ]);
}
```

(The saved entry also appears in the existing journal overlay's passage band with no further work — `save_passage_page` writes the same rows the Ctrl+a ask card writes.)

- [ ] **Step 3: Build, commit**

```bash
cargo build 2>&1 | tail -2
git add src/input/actions/chat.rs src/input/actions/journal.rs
git commit -m "feat: chat transcript save to journal + revision pivot"
```

---

### Task 8: revision loop

**Files:**
- Modify: `src/input/actions/chat.rs` (replace the `chat_revision` stub module), `src/input/actions/journal.rs` (visibility of `rewrite_user_message` at :728)

**Interfaces:**
- Consumes: `journal::rewrite_user_message(context, question, answer, instruction) -> String` (made `pub(crate)`); `run_claude_request` (single-shot — a revision is not a chat turn); `db::journal::update_journal_page(conn, id, q, a, model)`; `journal::purge_journal_audio` (Task 7).
- Produces: `parse_revised_qa(reply, fallback_q) -> (String, String)` (pure, tested); real `submit_revision`; `s` on the revision view updates the same row.

- [ ] **Step 1: Write the failing tests for the reply parser**

The revision prompt asks for both Q and A back in a fixed format; the parser must survive a model that ignores it. In chat.rs:

```rust
/// Parse a revision reply of the form "Q: ...\nA: ..." (A may span
/// paragraphs). Falls back to (fallback_q, whole reply) when the format is
/// absent, so a format-ignoring model still yields a usable answer.
pub(crate) fn parse_revised_qa(reply: &str, fallback_q: &str) -> (String, String) {
    let trimmed = reply.trim();
    if let Some(rest) = trimmed.strip_prefix("Q:") {
        if let Some(a_pos) = rest.find("\nA:") {
            let q = rest[..a_pos].trim().to_string();
            let a = rest[a_pos + 3..].trim().to_string();
            if !q.is_empty() && !a.is_empty() {
                return (q, a);
            }
        }
    }
    (fallback_q.to_string(), trimmed.to_string())
}

#[cfg(test)]
mod revision_tests {
    use super::parse_revised_qa;

    #[test]
    fn parses_q_and_multiparagraph_a() {
        let (q, a) = parse_revised_qa(
            "Q: Sharper question?\nA: First paragraph.\n\nSecond paragraph.",
            "old q",
        );
        assert_eq!(q, "Sharper question?");
        assert_eq!(a, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn falls_back_when_format_absent() {
        let (q, a) = parse_revised_qa("Just a plain revised answer.", "old q");
        assert_eq!(q, "old q");
        assert_eq!(a, "Just a plain revised answer.");
    }

    #[test]
    fn falls_back_when_a_missing() {
        let (q, a) = parse_revised_qa("Q: only a question", "old q");
        assert_eq!(q, "old q");
        assert_eq!(a, "Q: only a question");
    }
}
```

Run `cargo test --bins revision_tests 2>&1 | tail -3` — fails (fn not defined) until you add the fn; then passes: 3 passed.

- [ ] **Step 2: Make `rewrite_user_message` reusable**

`src/input/actions/journal.rs:728`: `fn rewrite_user_message(` → `pub(crate) fn rewrite_user_message(`.

- [ ] **Step 3: Implement `submit_revision`**

Replace the Task-6 `chat_revision` stub module body:

```rust
pub(crate) mod chat_revision {
    use super::*;

    /// Ctrl+Enter in revision mode: the prompt text is an instruction to
    /// revise the saved entry. Empty instruction = no-op (hand edits are not
    /// a chat concern). Claude may rewrite both Q and A (fixed output format,
    /// parsed leniently by parse_revised_qa).
    pub(crate) fn submit_revision(state_rc: &Rc<RefCell<AppState>>) {
        let (id, q, a, context, instruction, model) = {
            let s = state_rc.borrow();
            let Some(id) = s.chat.revision_of else { return };
            let instruction = s.chat_panel.take_input_text().trim().to_string();
            if instruction.is_empty() {
                crate::ui::toast::show_transient(&s.chapter_toast, "Type a revision instruction", 2);
                return;
            }
            let Some(e) = s.chat.exchanges.iter().find(|e| e.saved_id == Some(id)) else {
                return;
            };
            let Some(work) = s.current_work.as_ref() else { return };
            let scene = crate::app::scene_synopsis::scene_label(e.div1, e.div2);
            let context = format!(
                "Work: {} by {}\nThis Q&A is filed under a PASSAGE in {}\n\nPassage:\n{}\n\nReturn the revised Q&A in exactly this format:\nQ: <revised question>\nA: <revised answer>",
                work.title, work.author, scene, e.source_markup,
            );
            (
                id,
                e.question.clone(),
                e.answer.clone(),
                context,
                instruction,
                s.config.claude_model.clone(),
            )
        };
        let user_msg =
            crate::input::actions::journal::rewrite_user_message(&context, &q, &a, &instruction);
        let work_type = state_rc
            .borrow()
            .current_work
            .as_ref()
            .map(|w| w.work_type.clone())
            .unwrap_or_default();
        {
            let s = state_rc.borrow();
            crate::ui::toast::show_persistent(&s.chapter_toast, "Rewriting\u{2026}");
        }
        let model_for_db = model.clone();
        crate::input::actions::claude_bridge::run_claude_request(
            state_rc,
            crate::gloss::journal_qa_prompt(&work_type),
            user_msg,
            model,
            move |st, reply| {
                let mut s = st.borrow_mut();
                let (new_q, new_a) = super::parse_revised_qa(&reply, &q);
                if let Some(e) = s.chat.exchanges.iter_mut().find(|e| e.saved_id == Some(id)) {
                    e.question = new_q.clone();
                    e.answer = new_a.clone();
                }
                super::render_saved_entry(&s, &new_q, &new_a);
                // Persist immediately: the revision loop's `s` re-update path
                // also exists, but the design stores exactly the model's
                // latest output, so write it now.
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if let Err(err) = crate::db::journal::update_journal_page(
                        &conn, id, &new_q, &new_a, &model_for_db,
                    ) {
                        crate::logging::log(&format!("CHAT: revision save failed: {}", err));
                    }
                    crate::input::actions::journal::purge_journal_audio(&conn, id);
                }
                crate::ui::toast::show_transient(&s.chapter_toast, "Rewritten", 2);
            },
            |st, msg| {
                let s = st.borrow();
                crate::ui::toast::show_transient(&s.chapter_toast, msg, 4);
            },
        );
    }
}
```

`s` on the transcript while in the revision view re-saves the current entry (already-updated row) — `save_selected_exchange` naturally handles it only for unsaved exchanges; add at its top:

```rust
    if let Some(id) = s.chat.revision_of {
        // Already saved: `s` re-confirms (row is persisted on every
        // successful revision); just toast.
        let _ = id;
        crate::ui::toast::show_transient(&s.chapter_toast, "Entry is saved", 2);
        return;
    }
```

- [ ] **Step 4: Build, test, commit**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins revision_tests 2>&1 | tail -3
git add src/input/actions/chat.rs src/input/actions/journal.rs
git commit -m "feat: chat revision loop updates the saved journal page"
```

---

### Task 9: resets, work-switch, header-follow

**Files:**
- Modify: `src/input/actions/chat.rs`, `src/app/mod.rs` (`display_work` — find the top of the work-load path with `rg -n "pub fn display_work" src/app/mod.rs`)

**Interfaces:**
- Consumes: everything prior.
- Produces: `chat::on_work_switched(&mut AppState)`; close clears the session; panel survives a work switch with a cleared transcript and refreshed header.

- [ ] **Step 1: Clear on close**

In `close_chat_layout`, before the log line:

```rust
    s.chat = Default::default();
    s.chat_panel.render_rows(&[]);
```

- [ ] **Step 2: Work switch**

In chat.rs:

```rust
/// Work switch with the panel open: history clears (context would be from
/// another work), the panel stays open, header refreshes.
pub(crate) fn on_work_switched(s: &mut AppState) {
    if !s.chat_layout_open {
        return;
    }
    s.chat = Default::default();
    s.chat_panel.render_rows(&[]);
    set_panel_header(s);
}
```

In `display_work` (after the new work is set on state — place next to the existing per-work state resets there):

```rust
    crate::input::actions::chat::on_work_switched(state);
```

(Match the actual receiver: if the surrounding code operates on `&mut AppState` use `on_work_switched(s)` with that binding.)

- [ ] **Step 3: Header follows the reader cursor**

The context chip already re-derives per exchange. For the header, in `focus_prompt` we already call `set_panel_header`; additionally, when the reader (panel open) turns pages the header may go stale — acceptable for v1: the header refreshes on every focus-into-panel. No code change; note this in the commit message body.

- [ ] **Step 4: Build, commit**

```bash
cargo build 2>&1 | tail -2 && cargo test --bins 2>&1 | rg "test result"
git add src/input/actions/chat.rs src/app/mod.rs
git commit -m "feat: chat session resets on close and work switch"
```

---

### Task 10: headless e2e verification + merge

**Files:** none (verification + merge).

**Interfaces:** consumes the full feature.

- [ ] **Step 1: Launch headless on Bleak House WITHOUT an API key**

Unsetting the key makes the send path fail deterministically with "Set ANTHROPIC_API_KEY environment variable" — that error line rendering in the transcript is itself a verification target. Per CLAUDE.md Headless Verification (single-column prose at 1920x1200 qualifies for the room check):

```bash
cd ~/utono/linux-lit && cargo build
env -u ANTHROPIC_API_KEY LINUX_LIT_WORK=BH LIT_HEADLESS_TEST=1 LIT_NO_MPV=1 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage-chat.log &
sleep 5 && ls /run/user/1000/wayland-*
export WAYLAND_DISPLAY=<new socket> XDG_RUNTIME_DIR=/run/user/1000
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200 && sleep 3
```

Safety: clean up ONLY with `pkill -f "cage -- ./target/debug/linux-lit"`; check for live user instances first (`pgrep -af "target/debug/linux-lit"`) and never touch them.

- [ ] **Step 2: Panel opens; card pins right at unchanged size**

`grim /tmp/chat0.png` (baseline: centered card; note the card's left/right gaps). `wtype -k Tab && sleep 2 && grim /tmp/chat1.png`. PASS: card flush right (24px right gap), same card WIDTH as baseline (measure the card's paper-colored region in both screenshots), left space shows the bare-on-root panel with header + bordered input, footer shows `-- NORMAL --`/`-- INSERT --` hint. Check log: `rg "CHAT: layout opened|CARD_SIZING: chat" <run log>`.

- [ ] **Step 3: Ask → deterministic error line**

Type a question and submit: `wtype "i" && sleep 1 && wtype "why the fog" && wtype -k Escape && sleep 1 && wtype -M ctrl -k Return -m ctrl && sleep 3 && grim /tmp/chat2.png`. PASS: transcript shows the Q row and a dim italic error line containing "ANTHROPIC_API_KEY"; the question text is restored into the input box.

- [ ] **Step 4: Focus cycle + close**

`wtype -k Tab && sleep 1 && grim /tmp/chat3.png` (transcript focus), `wtype -k Tab && sleep 1` (reader focus — `wtype "j"` moves the reader cursor; grim to confirm), `wtype -k Tab && sleep 1` (back to prompt), then `wtype -M ctrl -k Tab -m ctrl && sleep 2 && grim /tmp/chat4.png`. PASS: chat4 shows the card re-centered, panel gone. Also verify plain `a` still pauses (no MPV headless — just confirm no chat side-effect) and `-` now toasts/pauses rather than switching works.

- [ ] **Step 5: Review every PNG inline**

Read each `/tmp/chat*.png`; describe what's visible; any mismatch is a bug — stop and fix before merging.

- [ ] **Step 6: Save-path spot check (SQL-level)**

The save/revision path needs a real API reply, which headless can't get; verify the DB write shape instead with a direct call test: run the suite (`cargo test --bins 2>&1 | rg "test result"`) and confirm `parse_revised_qa` + `chat_body` + `segments` + `block_bounds` tests are green, then check `save_passage_page`'s write manually is NOT needed — it is the same function the (already-shipped, e2e-verified) Ctrl+a ask card uses. State this explicitly in the report rather than claiming live-save verification.

- [ ] **Step 7: Merge per finishing-a-branch**

```bash
cd ~/utono/linux-lit && cargo test --bins 2>&1 | rg "test result" && git status --porcelain
git checkout master && git merge --no-ff chat-layout -m "Merge branch 'chat-layout': Tab chat panel for journal Q&A"
cargo build 2>&1 | tail -2 && git push origin master && git branch -d chat-layout
```

Expected: suite green except `test_load_work_hamlet`; clean merge; push OK. tty-dotfiles commit stays local (mr flow).

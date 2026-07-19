use gtk4::prelude::*;

use crate::app::AppState;

/// Tracks the visual selection range (anchor..cursor).
pub struct SelectionState {
    pub anchor_line: usize,
    pub cursor_line: usize,
    /// True when visual mode was entered via Ctrl+a (AskPassage): Return then
    /// confirms the Journal Q&A ask directly instead of opening the Action
    /// menu. Extending the selection with j/k/G/gg keeps the flag.
    pub pending_ask: bool,
}

impl SelectionState {
    pub fn new(line: usize) -> Self {
        Self {
            anchor_line: line,
            cursor_line: line,
            pending_ask: false,
        }
    }

    /// Returns (start, end) as an inclusive range, regardless of direction.
    pub fn range(&self) -> (usize, usize) {
        let start = self.anchor_line.min(self.cursor_line);
        let end = self.anchor_line.max(self.cursor_line);
        (start, end)
    }
}

/// Inclusive `(start, end)` of the contiguous block of non-boundary lines
/// containing `cursor`. A "boundary" line (blank line or separator, decided by
/// the caller's closure) delimits the block: prose paragraphs and play
/// speeches are both blank-line-delimited in the reader buffer, so this yields
/// the paragraph (prose) or the speech including its speaker label (plays).
/// Returns `None` when `cursor` is out of range or is itself a boundary line —
/// callers fall back to a single-line selection.
pub(crate) fn block_bounds(
    line_count: usize,
    cursor: usize,
    is_boundary: impl Fn(usize) -> bool,
) -> Option<(usize, usize)> {
    if cursor >= line_count || is_boundary(cursor) {
        return None;
    }
    let mut start = cursor;
    while start > 0 && !is_boundary(start - 1) {
        start -= 1;
    }
    let mut end = cursor;
    while end + 1 < line_count && !is_boundary(end + 1) {
        end += 1;
    }
    Some((start, end))
}

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

/// Apply the selection_tag to all lines in the visual selection range.
/// Also removes dim_tag from those lines so they appear at full brightness.
pub fn apply_selection_highlight(state: &AppState) {
    let Some(selection) = state.visual_selection.as_ref() else { return };
    let (start, end) = selection.range();
    apply_selection_highlight_range(state, start, end);
}

/// Apply the selection_tag to buffer lines `[start, end]` WITHOUT requiring a
/// live `visual_selection`. Same body as `apply_selection_highlight`, which is
/// the `visual_selection`-driven wrapper over this. (The chat panel no longer
/// marks its pinned passage — the source stays unhighlighted while the panel is
/// open.)
pub fn apply_selection_highlight_range(state: &AppState, start: usize, end: usize) {
    let buffer = &state.buffer;
    for line_idx in start..=end {
        if let Some(line_start) = buffer.iter_at_line(line_idx as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            buffer.remove_tag(&state.dim_tag, &line_start, &line_end);
            buffer.apply_tag(&state.selection_tag, &line_start, &line_end);
        }
    }
}

/// Remove the selection_tag from the entire buffer.
pub fn clear_selection_highlight(state: &AppState) {
    let (buf_start, buf_end) = state.buffer.bounds();
    state.buffer.remove_tag(&state.selection_tag, &buf_start, &buf_end);
}

/// Move the visual selection cursor by delta lines.
/// Does NOT skip translation lines (visual mode selects all visible lines).
pub fn move_selection_cursor(state: &mut AppState, delta: i32) {
    if state.visual_selection.is_none() {
        return;
    }
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let current_cursor = state.visual_selection.as_ref().unwrap().cursor_line;
    let new_cursor = (current_cursor as i32 + delta)
        .max(0)
        .min(line_count as i32 - 1) as usize;
    state.visual_selection.as_mut().unwrap().cursor_line = new_cursor;
    state.current_line = new_cursor;

    crate::input::navigation::update_highlight_and_ensure_visible(state);
}

/// Enter visual mode: set anchor at current line.
pub fn enter_visual_mode(state: &mut AppState) {
    state.visual_selection = Some(SelectionState::new(state.current_line));
    state.input_mode = crate::app::InputMode::Visual;
    crate::input::navigation::update_highlight_and_ensure_visible(state);
    crate::logging::log(&format!("VISUAL: entered at line {}", state.current_line));
}

/// Ctrl+a (AskPassage): enter visual mode with the blank-line-delimited block
/// around the cursor pre-selected (prose paragraph / play speech incl. speaker
/// label) and `pending_ask` set, so a second Ctrl+a or Return opens the
/// Journal Q&A ask card directly. On a blank/separator line, falls back to a
/// single-line selection (same as V), still flagged pending-ask.
/// On DB-join buffers (works with no text_file, where one buffer line renders
/// one work row and no blank lines exist) the block is the run of lines
/// sharing the cursor's work row instead.
pub fn enter_visual_block_mode(state: &mut AppState) {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    let cursor = state.current_line;
    let bounds = cursor_block_bounds(state);
    let (start, end) = bounds.unwrap_or((cursor, cursor));
    state.visual_selection = Some(SelectionState {
        anchor_line: start,
        cursor_line: end,
        pending_ask: true,
    });
    state.current_line = end;
    state.input_mode = crate::app::InputMode::Visual;
    crate::input::navigation::update_highlight_and_ensure_visible(state);
    crate::logging::log(&format!(
        "VISUAL: ask-block entered {}..{} (cursor was {})", start, end, cursor
    ));
}

/// Yank (copy) the visual selection to clipboard, then exit visual mode.
pub fn yank_selection(state: &mut AppState) {
    action_copy(state, false);
    exit_visual_mode(state);
}

/// Exit visual mode: clear selection and highlighting.
pub fn exit_visual_mode(state: &mut AppState) {
    if state.visual_selection.is_some() {
        state.visual_selection = None;
        state.input_mode = crate::app::InputMode::Reader;
        clear_selection_highlight(state);
        crate::input::navigation::update_highlight_and_ensure_visible(state);
        crate::logging::log("VISUAL: exited");
    }
}

/// Extend visual selection to the first line (gg equivalent).
pub fn extend_to_start(state: &mut AppState) {
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = 0;
        state.current_line = 0;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}

/// Extend visual selection to the last line (G equivalent).
pub fn extend_to_end(state: &mut AppState) {
    let line_count = state.effective_line_count();
    if line_count == 0 {
        return;
    }
    if let Some(ref mut sel) = state.visual_selection {
        sel.cursor_line = line_count - 1;
        state.current_line = line_count - 1;
        crate::input::navigation::update_highlight_and_ensure_visible(state);
    }
}

/// Tracks which action is highlighted in the popup menu.
pub struct ActionPopupState {
    pub selected_index: usize,
}

/// Built-in action names, in display order. The `match index` in
/// `execute_action` maps these POSITIONALLY — reorder both together or an item
/// fires the wrong action.
pub const BUILTIN_ACTIONS: &[&str] = &["Reader Gloss", "Journal Q&A", "Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata"];

/// Determine which built-in actions are available for the current work.
pub fn available_builtin_actions(_state: &AppState) -> Vec<&'static str> {
    BUILTIN_ACTIONS.to_vec()
}

/// Open the action popup menu.
pub fn open_action_popup(state: &mut AppState) {
    let builtins = available_builtin_actions(state);
    let externals: Vec<(String, String)> = state
        .config
        .visual_mode_commands
        .iter()
        .map(|c| (c.name.clone(), c.command.clone()))
        .collect();
    state.action_popup_widget.show_actions(
        &builtins.iter().map(|s| *s).collect::<Vec<_>>(),
        &externals,
    );
    state.action_popup = Some(ActionPopupState { selected_index: 0 });
    state.input_mode = crate::app::InputMode::ActionPopup;
    crate::logging::log("VISUAL: action popup opened");
}

/// Close the action popup without executing.
pub fn close_action_popup(state: &mut AppState) {
    state.action_popup = None;
    state.action_popup_widget.hide();
    state.input_mode = crate::app::InputMode::Visual;
    crate::logging::log("VISUAL: action popup closed");
}

/// Execute the action at the given index.
/// Indices 0..N are built-in actions, N.. are external commands.
pub fn execute_action(
    state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>,
    index: usize,
    _tokio_handle: &tokio::runtime::Handle,
) {
    let builtin_count = available_builtin_actions(&state_rc.borrow()).len();

    if index < builtin_count {
        match index {
            // Index order MUST match BUILTIN_ACTIONS above.
            0 => {
                action_reader_gloss(state_rc);
                return;
            }
            1 => {
                action_journal_qa(state_rc);
                return;
            }
            2 => {
                action_gloss_with_claude(state_rc);
                return;
            }
            3 => {
                action_inner_monologue(state_rc);
                return;
            }
            4 => action_copy(&mut state_rc.borrow_mut(), false),
            5 => action_copy(&mut state_rc.borrow_mut(), true),
            _ => {}
        }
    } else {
        let ext_index = index - builtin_count;
        let command = state_rc.borrow().config.visual_mode_commands.get(ext_index).map(|c| c.command.clone());
        if let Some(cmd) = command {
            action_external_command(&mut state_rc.borrow_mut(), &cmd);
        }
    }

    // Move cursor to the first selected line, then exit visual mode
    {
        let mut s = state_rc.borrow_mut();
        if let Some(ref sel) = s.visual_selection {
            let (start, _) = sel.range();
            s.current_line = start;
        }
    }
    exit_visual_mode(&mut state_rc.borrow_mut());
}

fn action_copy(state: &mut AppState, with_metadata: bool) {
    let (start, end) = match &state.visual_selection {
        Some(s) => s.range(),
        None => return,
    };
    // Collect raw text first (borrows buffer), then apply metadata (borrows state).
    let raw_lines: Vec<(usize, String)> = {
        let buffer = &state.buffer;
        (start..=end)
            .filter_map(|line_idx| {
                buffer.iter_at_line(line_idx as i32).map(|line_start| {
                    let mut line_end = line_start;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    let text = buffer.text(&line_start, &line_end, false).to_string();
                    (line_idx, text)
                })
            })
            .collect()
    };

    let mut lines_text = Vec::new();
    for (line_idx, text) in raw_lines {
        if with_metadata {
            let meta = format_line_metadata(state, line_idx, &text);
            lines_text.push(meta);
        } else {
            lines_text.push(text);
        }
    }

    let output = lines_text.join("\n");
    // Pipe to wl-copy
    use std::process::{Command, Stdio};
    use std::io::Write;
    match Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(output.as_bytes());
            }
            let _ = child.wait();
            crate::logging::log(&format!("VISUAL: copied {} lines to clipboard", end - start + 1));
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: wl-copy failed: {}", e));
        }
    }
}

/// Format a line with metadata: [line_num] SPEAKER (start-end): text
fn format_line_metadata(state: &AppState, buffer_line: usize, text: &str) -> String {
    let work = match &state.current_work {
        Some(w) => w,
        None => return format!("[{}] {}", buffer_line + 1, text),
    };

    let work_idx = state.work_line_for_buffer(buffer_line);
    let line = work_idx.and_then(|i| work.lines.get(i));

    match line {
        Some(line) => {
            let mut parts = Vec::new();
            parts.push(format!("[{}]", buffer_line + 1));
            if let Some(ref speaker) = line.speaker {
                parts.push(speaker.clone());
            }
            if let Some(ref ts) = line.timestamp {
                parts.push(format!("({:.1}-{:.1})", ts.start, ts.end));
            }
            parts.push(format!(":{}", text));
            parts.join(" ")
        }
        None => format!("[{}] {}", buffer_line + 1, text),
    }
}


fn action_external_command(state: &mut AppState, command: &str) {
    // Extract selection range
    let (start, end) = match &state.visual_selection {
        Some(s) => s.range(),
        None => return,
    };

    let work = match &state.current_work {
        Some(w) => w,
        None => return,
    };

    // Collect buffer text for selection
    let mut selected_text = Vec::new();
    for buf_line in start..=end {
        if let Some(line_start) = state.buffer.iter_at_line(buf_line as i32) {
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            selected_text.push(state.buffer.text(&line_start, &line_end, false).to_string());
        }
    }
    let input = selected_text.join("\n");

    // Collect DB lines for undo
    let mut db_lines: Vec<crate::db::models::Line> = Vec::new();
    for buf_line in start..=end {
        if let Some(work_idx) = state.work_line_for_buffer(buf_line) {
            if let Some(line) = work.lines.get(work_idx) {
                db_lines.push(line.clone());
            }
        }
    }

    // Run external command
    use std::process::{Command, Stdio};
    use std::io::Write;
    let result = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(input.as_bytes())?;
            }
            drop(child.stdin.take());
            child.wait_with_output()
        });

    let output = match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                crate::logging::log(&format!(
                    "VISUAL: command '{}' failed ({}): {}",
                    command, output.status, stderr
                ));
                return;
            }
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: command '{}' spawn failed: {}", command, e));
            return;
        }
    };

    let new_lines: Vec<String> = output.lines().map(|l| l.to_string()).collect();
    if new_lines.is_empty() {
        crate::logging::log("VISUAL: command returned empty output, skipping");
        return;
    }

    let abbrev = work.abbrev.clone();
    let text_file = work.text_file.clone();
    let old_ids: Vec<i64> = db_lines.iter().map(|l| l.id).collect();

    // Write to DB
    match crate::db::queries::open_db_rw() {
        Ok(conn) => {
            if let Err(e) = crate::db::queries::replace_lines(&conn, &abbrev, &old_ids, &new_lines) {
                crate::logging::log(&format!("VISUAL: replace DB error: {}", e));
                return;
            }
        }
        Err(e) => {
            crate::logging::log(&format!("VISUAL: open_db_rw failed: {}", e));
            return;
        }
    }

    // Update text file if it exists
    if let Some(ref path) = text_file {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let mut file_lines: Vec<String> = contents.lines().map(|l| l.to_string()).collect();
            if end < file_lines.len() {
                file_lines.splice(start..=end, new_lines.iter().cloned());
                let _ = std::fs::write(path, file_lines.join("\n"));
            }
        }
    }

    crate::logging::log(&format!(
        "VISUAL: command '{}' replaced {} lines with {} lines",
        command, old_ids.len(), new_lines.len(),
    ));

    reload_current_work(state);
}


pub(crate) fn action_journal_qa(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    // Phase 1 — build context while holding borrow.
    let (div1, div2, start, end, source_text) = {
        let state = state_rc.borrow();
        let (start_buf, end_buf) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        let selected_lines: Vec<crate::db::models::Line> = (start_buf..=end_buf)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        // Reuse build_context_for_type just to get citations, div1/div2, speaker.
        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(c) => c,
            None => return,
        };

        // <speaker>/<verse> markup for the passage.
        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx.act, ctx.scene, ctx.start_citation, ctx.end_citation, passage_doc)
    };

    // Phase 2 — exit visual mode, then open the passage ask via the shared fn.
    exit_visual_mode(&mut state_rc.borrow_mut());
    crate::input::actions::journal::begin_passage_ask(state_rc, div1, div2, start, end, source_text);
    crate::logging::log("JOURNAL-QA: opened ask card for visual passage");
}

fn action_reader_gloss(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (ctx, model, tokio_handle, all_glosses, passage_doc) = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(c) => c,
            None => return,
        };

        let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        // `<speaker>`/`<verse>` markup for the passage being glossed, shared with
        // the echoes source header so the "Glossing…" loading card formats it the
        // same single-column way as the original passage in the gloss result.
        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), all_glosses, passage_doc)
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    let own_idx = all_glosses.iter().position(|g| g.gloss_type == "reader-gloss");
    if let Some(idx) = own_idx {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, card_width, card_height, Some(&s.theme.root_color), &pairs);
        s.gloss_overlay.set_position(idx, all_glosses.len());
        s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
        s.gloss_list = all_glosses;
        s.gloss_index = idx;
        s.gloss_context = Some(ctx);
        s.record_last_gloss("reader-gloss");
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("READER-GLOSS: showing cached gloss");
        return;
    }

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        // Show the passage being glossed on the loading card, formatted the same
        // way (single-column `<speaker>`/`<verse>`) as the original passage in
        // the gloss result, so it looks identical before and after the gloss.
        let cw = s.content_hbox.width();
        let h = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }

    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    let state_for_result = std::rc::Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        // Keep a copy for the DB stamp; `model` itself is moved into the spawn.
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(&crate::gloss::READER_GLOSS_PROMPT, &user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &gloss_text, "reader-gloss", &model_for_db,
                    "READER-GLOSS: generated and saved new gloss",
                );
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("READER-GLOSS: API error: {}", e));
            }
            Err(e) => {
                // Recover the UI so the overlay isn't stuck on the loading card.
                let s = state_for_result.borrow();
                s.gloss_overlay.show("Internal error \u{2014} try again.", "");
                crate::logging::log(&format!("READER-GLOSS: tokio join error: {}", e));
            }
        }
    });
}

/// `-` in visual mode: open the chat panel pinned to the selection and gloss
/// the passage immediately — no ask input. Sibling to `Ctrl+a` (Journal Q&A
/// ask card) and `Tab` (chat pinned, empty input) on the same select-then-act
/// flow.
///
/// On a cache hit the stored gloss is shown and NO API call is made, so
/// pressing `-` twice on a passage is cheap. `r`/`R` in the panel is the way
/// to force a fresh gloss.
pub(crate) fn action_reader_gloss_chat(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    // Build the gloss context BEFORE opening the panel: open_chat_pinned_to_selection
    // exits visual mode, which clears the selection this reads.
    let prepared = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state
                    .work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();
        match crate::gloss::build_context_for_type(work, &selected_lines, "reader-gloss") {
            Some(ctx) => Some((ctx, state.config.claude_model.clone())),
            None => None,
        }
    };
    let Some((ctx, model)) = prepared else { return };

    // Pins the passage, exits visual mode, opens and places the panel. Bails
    // with its own toast (returning false) when the selection has no
    // passage, or when a single-column layout has no room for the panel.
    //
    // MUST branch on the return value, not on `chat_layout_open`: if a panel
    // was already open from a PREVIOUS passage, `chat_layout_open` stays
    // true even when THIS call fails to pin (e.g. this selection has no
    // passage) — that would gloss the new (empty) selection into a panel
    // still pinned to the old one.
    if !crate::input::actions::chat::open_chat_pinned_to_selection(state_rc) {
        return; // callee already toasted why
    }
    // open_chat_pinned_to_selection -> toggle_chat_layout opens the panel via
    // focus_prompt (correct for Tab, whose whole purpose is to land the user
    // ready to ask). '-' auto-glosses instead — no question was typed — so
    // immediately retire that input and hand focus to the transcript, same
    // shape as submit_chat_prompt's answer-arrived path. Without this the
    // user lands in a focused, empty "Ask about this passage" input while the
    // gloss is still generating.
    {
        let mut s = state_rc.borrow_mut();
        s.chat_panel.close_input();
        crate::input::actions::chat::focus_transcript(&mut s);
    }

    let cached = crate::input::actions::chat::reload_gloss_list(&ctx.work_abbrev, &ctx.start_citation);
    {
        let mut s = state_rc.borrow_mut();
        s.chat.gloss_ctx = Some(ctx.clone());
        s.chat.gloss_list = cached;
        s.chat.gloss_index = 0;
    }

    // Cache hit: show the newest stored gloss, spend no API call.
    let hit = {
        let s = state_rc.borrow();
        s.chat.gloss_list.first().map(|g| g.gloss_text.clone())
    };
    if let Some(text) = hit {
        let mut s = state_rc.borrow_mut();
        crate::input::actions::chat::push_gloss_exchange(&mut s, &ctx, &text);
        crate::input::actions::chat::focus_transcript(&mut s);
        crate::logging::log("CHAT-GLOSS: showing cached gloss");
        return;
    }

    crate::input::actions::chat::request_reader_gloss(state_rc, ctx, model);
}

/// Reader-mode `-`: a toggle. When the chat panel is already open, close it
/// (the reader-side close path, since the Tab caps are unbound). Otherwise open
/// the panel pinned to the reader-gloss covering the cursor line and show the
/// stored gloss — the same end state as visual-mode `-`, WITHOUT the `V`-select
/// step. No-op (toast) when no reader-gloss covers the cursor line.
///
/// Reuses `action_reader_gloss_chat` verbatim by staging a transient
/// `SelectionState` over the gloss's authored passage span (the
/// `enter_visual_block_mode` pattern). `action_reader_gloss_chat` ->
/// `open_chat_pinned_to_selection` reads that selection, pins, then
/// `exit_visual_mode` clears it — so the transient selection never outlives
/// this call.
pub(crate) fn reader_gloss_chat_at_cursor(
    state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>,
) {
    // Toggle: `-` closes an already-open panel (the reader-focus close path
    // that Ctrl+Tab used to provide). Only then does it fall through to gloss.
    if state_rc.borrow().chat_layout_open {
        crate::input::actions::chat::close_chat_layout(&mut state_rc.borrow_mut());
        return;
    }
    let span = crate::input::actions::gloss::reader_gloss_passage_at_cursor(&state_rc.borrow());
    let Some((start, end)) = span else {
        let s = state_rc.borrow();
        crate::input::navigation::show_chapter_toast_secs(&s, "No gloss on this line", 2);
        return;
    };
    {
        let mut s = state_rc.borrow_mut();
        s.visual_selection = Some(SelectionState {
            anchor_line: start,
            cursor_line: end,
            pending_ask: false,
        });
        s.input_mode = crate::app::InputMode::Visual;
    }
    // Reads the staged selection, builds the reader-gloss context, pins the
    // panel, exits visual mode (clearing the selection), shows the cached gloss.
    action_reader_gloss_chat(state_rc);
}

fn action_gloss_with_claude(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (ctx, model, tokio_handle, all_glosses, passage_doc) = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        let ctx = match crate::gloss::build_context(work, &selected_lines) {
            Some(c) => c,
            None => return,
        };

        let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        // `<speaker>`/`<verse>` markup for the passage being glossed, shared with
        // the echoes source header so the "Glossing…" loading card formats it the
        // same single-column way as the original passage in the gloss result.
        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx, state.config.claude_model.clone(), state.tokio_handle.clone(), all_glosses, passage_doc)
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    let own_idx = all_glosses.iter().position(|g| g.gloss_type == "teacher-generic");
    if let Some(idx) = own_idx {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, card_width, card_height, Some(&s.theme.root_color), &pairs);
        s.gloss_overlay.set_position(idx, all_glosses.len());
        s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
        s.gloss_list = all_glosses;
        s.gloss_index = idx;
        s.gloss_context = Some(ctx);
        s.record_last_gloss("teacher-generic");
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("GLOSS: showing cached gloss");
        return;
    }

    {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        // Show the passage being glossed on the loading card, formatted the same
        // way (single-column `<speaker>`/`<verse>`) as the original passage in
        // the gloss result, so it looks identical before and after the gloss.
        let cw = s.content_hbox.width();
        let h = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
    }

    let user_msg = crate::gloss::build_user_message(&ctx, None, None);
    let state_for_result = std::rc::Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        // Keep a copy for the DB stamp; `model` itself is moved into the spawn.
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude(&user_msg, &model).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &gloss_text, "teacher-generic", &model_for_db,
                    "GLOSS: generated and saved new gloss",
                );
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: API error: {}", e));
            }
            Err(e) => {
                // Recover the UI so the overlay isn't stuck on the loading card.
                let s = state_for_result.borrow();
                s.gloss_overlay.show("Internal error \u{2014} try again.", "");
                crate::logging::log(&format!("GLOSS: tokio join error: {}", e));
            }
        }
    });
}

fn action_inner_monologue(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (ctx, scene_lines, tokio_handle, all_glosses, passage_doc) = {
        let state = state_rc.borrow();
        let (start, end) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };

        let selected_lines: Vec<crate::db::models::Line> = (start..=end)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();

        let ctx = match crate::gloss::build_context_for_type(work, &selected_lines, "inner-monologue") {
            Some(c) => c,
            None => return,
        };

        let scene_lines: Vec<crate::db::models::Line> = work.lines.iter()
            .filter(|l| l.div1 == ctx.act && l.div2 == ctx.scene)
            .cloned()
            .collect();

        let all_glosses: Vec<crate::db::queries::SavedGloss> = match crate::db::queries::open_db() {
            Ok(conn) => crate::db::queries::find_glosses_by_start(
                &conn, &ctx.work_abbrev, &ctx.start_citation,
                &["teacher-generic", "inner-monologue", "reader-gloss"],
            ).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        // `<speaker>`/`<verse>` markup for the passage being glossed, shared with
        // the echoes source header so the "Glossing…" loading card formats it the
        // same single-column way as the reader-gloss loading card + gloss result.
        let passage_doc = crate::input::actions::echoes::build_source_header(&selected_lines, &ctx.speaker);

        (ctx, scene_lines, state.tokio_handle.clone(), all_glosses, passage_doc)
    };

    exit_visual_mode(&mut state_rc.borrow_mut());

    let own_idx = all_glosses.iter().position(|g| g.gloss_type == "inner-monologue");
    if let Some(idx) = own_idx {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        let pairs = ctx.source_line_pairs();
        let gloss_text = &all_glosses[idx].gloss_text;
        let card_width = s.content_hbox.width();
        let card_height = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_gloss_with_color(&ctx.source_text, gloss_text, card_width, card_height, Some(&s.theme.root_color), &pairs);
        s.gloss_overlay.set_position(idx, all_glosses.len());
        s.gloss_overlay.set_citation(&ctx.start_citation, &ctx.end_citation);
        s.gloss_list = all_glosses;
        s.gloss_index = idx;
        s.gloss_context = Some(ctx);
        s.record_last_gloss("inner-monologue");
        s.input_mode = crate::app::InputMode::GlossOverlay;
        crate::logging::log("GLOSS: showing cached inner monologue");
        return;
    }

    // Stash context for the deferred gloss call, then run the semantic
    // echo search and show the picker. The picker selection (or skip)
    // resumes via run_pending_inner_monologue.
    {
        let mut s = state_rc.borrow_mut();
        s.gloss_original_text = Some(ctx.source_text.clone());
        // Show the passage being glossed on the loading card (like reader-gloss)
        // rather than a bare "Glossing…" label.
        let cw = s.content_hbox.width();
        let h = crate::app::layout::overlay_card_height(&s);
        s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        s.input_mode = crate::app::InputMode::GlossOverlay;
        s.pending_echo_context = Some(ctx.clone());
        s.pending_echo_scene_lines = scene_lines.clone();
        s.pending_echo_passage_doc = passage_doc;
    }

    // Build the enriched query: "{SPEAKER} to {ADDRESSEE}: {text}".
    let query_text = build_echo_query(&ctx, &scene_lines);
    // Raw selected text (not the enriched query) for the affect axis.
    let affect_text = ctx.source_text.clone();
    let affect_weight = state_rc.borrow().config.echo_affect_weight;
    let source_work = ctx.work_abbrev.clone();
    let state_for_echo = std::rc::Rc::clone(state_rc);
    let echo_handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        let embed_result = echo_handle
            .spawn(async move { crate::voyage::embed_query(&query_text).await })
            .await;

        let candidates = match embed_result {
            Ok(Ok(embedding)) => crate::db::queries::open_db()
                .ok()
                .and_then(|conn| {
                    crate::db::echoes::find_similar_passages(
                        &conn, &embedding, &affect_text, &source_work, 10, affect_weight,
                    )
                    .ok()
                })
                .unwrap_or_default(),
            Ok(Err(e)) => {
                crate::logging::log(&format!("ECHO: embed error: {}", e));
                Vec::new()
            }
            Err(e) => {
                crate::logging::log(&format!("ECHO: embed join error: {}", e));
                Vec::new()
            }
        };

        if candidates.is_empty() {
            // No candidates — fall through to Claude finding its own echo.
            crate::logging::log("ECHO: no candidates, skipping picker");
            run_pending_inner_monologue_blocking(&state_for_echo, &echo_handle, None);
            return;
        }

        let titles = crate::db::queries::load_work_titles_or_default();

        let mut s = state_for_echo.borrow_mut();
        s.gloss_overlay.hide();
        s.echo_picker.set_titles(titles);
        s.echo_picker.set_items(candidates);
        s.echo_picker.show();
        s.input_mode = crate::app::InputMode::EchoPicker;
        crate::logging::log("ECHO: showing picker");
    });
}

/// Build the enriched query string for semantic echo search, matching the
/// "{SPEAKER} to {ADDRESSEE}: {text}" format used during pre-computation.
fn build_echo_query(ctx: &crate::gloss::GlossContext, scene_lines: &[crate::db::models::Line]) -> String {
    // Addressee: first speaker in the scene different from ctx.speaker.
    let primary_speaker = ctx.speaker.split(',').next().unwrap_or("").trim();
    let addressee = scene_lines
        .iter()
        .filter_map(|l| l.speaker.as_deref())
        .find(|sp| *sp != primary_speaker)
        .unwrap_or("?");
    format!("{} to {}: {}", primary_speaker, addressee, ctx.source_text)
}

/// Resume the inner-monologue gloss call after the echo picker. If an echo
/// was selected it is injected into the prompt as a suggested cross-work echo.
pub fn run_pending_inner_monologue(
    state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    selected: Option<crate::db::echoes::EchoCandidate>,
) {
    run_pending_inner_monologue_blocking(state_rc, tokio_handle, selected);
}

/// Cancel a pending inner-monologue gloss: dismiss the echo picker and the
/// gloss overlay, drop the stashed context, and return to the reader.
pub fn cancel_pending_inner_monologue(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let mut s = state_rc.borrow_mut();
    s.echo_picker.hide();
    s.gloss_overlay.hide();
    s.pending_echo_context = None;
    s.pending_echo_scene_lines = Vec::new();
    s.pending_echo_passage_doc = String::new();
    s.input_mode = crate::app::InputMode::Reader;
    crate::logging::log("ECHO: cancelled gloss from picker");
}

fn run_pending_inner_monologue_blocking(
    state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
    selected: Option<crate::db::echoes::EchoCandidate>,
) {
    let (ctx, scene_lines, model, titles) = {
        let mut s = state_rc.borrow_mut();
        let ctx = match s.pending_echo_context.take() {
            Some(c) => c,
            None => return,
        };
        let scene_lines = std::mem::take(&mut s.pending_echo_scene_lines);
        // Re-show the passage on the loading card (the echo picker hid the
        // overlay, or we skipped it) so generation reads as the reader-gloss
        // loading card, not a bare "Glossing…" label.
        let passage_doc = std::mem::take(&mut s.pending_echo_passage_doc);
        let cw = s.content_hbox.width();
        let h = crate::app::layout::overlay_card_height(&s);
        if passage_doc.is_empty() {
            s.gloss_overlay.show_loading();
        } else {
            s.gloss_overlay.show_glossing(&passage_doc, cw, h, Some(&s.theme.root_color));
        }
        s.input_mode = crate::app::InputMode::GlossOverlay;
        let titles = crate::db::queries::load_work_titles_or_default();
        (ctx, scene_lines, s.config.claude_model.clone(), titles)
    };

    let mut user_msg = crate::gloss::build_inner_monologue_message(&ctx, &scene_lines);
    if let Some(ref echo) = selected {
        let title = titles.get(&echo.work_abbrev).cloned().unwrap_or_else(|| echo.work_abbrev.clone());
        user_msg.push_str(&format!(
            "\n\n--- SUGGESTED ECHO (from semantic search) ---\nSpeaker: {}\nWork: {} {}.{}\nText: {}",
            echo.speaker, title, echo.div1, echo.div2, echo.passage_text.lines().next().unwrap_or("")
        ));
    }

    let state_for_result = std::rc::Rc::clone(state_rc);
    let handle = tokio_handle.clone();

    glib::spawn_future_local(async move {
        // Keep a copy for the DB stamp; `model` itself is moved into the spawn.
        let model_for_db = model.clone();
        let result = handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    crate::gloss::INNER_MONOLOGUE_PROMPT.as_str(), &user_msg, &model,
                ).await
            })
            .await;

        match result {
            Ok(Ok(gloss_text)) => {
                let verified_text = crate::gloss::verify_echo_citations(
                    &gloss_text, &ctx.work_abbrev, ctx.act, ctx.scene,
                );
                let mut s = state_for_result.borrow_mut();
                crate::input::actions::gloss::persist_render_install_gloss(
                    &mut s, ctx, &verified_text, "inner-monologue", &model_for_db,
                    "GLOSS: generated and saved inner monologue",
                );
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.gloss_overlay.show(&format!("Error: {}", e), "");
                crate::logging::log(&format!("GLOSS: inner monologue API error: {}", e));
            }
            Err(e) => {
                // Recover the UI so the overlay isn't stuck on the loading card.
                let s = state_for_result.borrow();
                s.gloss_overlay.show("Internal error \u{2014} try again.", "");
                crate::logging::log(&format!("GLOSS: inner monologue tokio join error: {}", e));
            }
        }
    });
}

/// Reload the current work from DB and refresh the display.
pub fn reload_current_work(state: &mut AppState) {
    let abbrev = match &state.current_work {
        Some(w) => w.abbrev.clone(),
        None => return,
    };
    let saved_line = state.current_line;

    match crate::db::queries::open_db() {
        Ok(conn) => {
            match crate::db::queries::load_work(&conn, &abbrev) {
                Ok(work) => {
                    crate::app::display_work(state, work);
                    let new_count = state.effective_line_count();
                    state.current_line = saved_line.min(new_count.saturating_sub(1));
                }
                Err(e) => crate::logging::log(&format!("VISUAL: reload work failed: {}", e)),
            }
        }
        Err(e) => crate::logging::log(&format!("VISUAL: open_db failed: {}", e)),
    }
}

#[cfg(test)]
mod block_bounds_tests {
    use super::block_bounds;

    /// Test harness: boundary = blank or separator line, same rule
    /// enter_visual_block_mode uses.
    fn bounds(lines: &[&str], cursor: usize) -> Option<(usize, usize)> {
        let texts: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        let is_boundary = |idx: usize| {
            let t = texts[idx].trim();
            t.is_empty() || crate::db::line_types::is_separator(t)
        };
        block_bounds(lines.len(), cursor, is_boundary)
    }

    #[test]
    fn paragraph_mid_buffer() {
        let lines = ["First para.", "", "Second para line 1.", "line 2.", "line 3.", "", "Third."];
        assert_eq!(bounds(&lines, 3), Some((2, 4)));
        // Every line of the block maps to the same bounds.
        assert_eq!(bounds(&lines, 2), Some((2, 4)));
        assert_eq!(bounds(&lines, 4), Some((2, 4)));
    }

    #[test]
    fn speech_includes_speaker_label() {
        // A play speech: speaker label + verse lines form one contiguous block.
        let lines = ["", "HAMLET", "To be, or not to be: that is the question:", "Whether 'tis nobler in the mind to suffer", ""];
        assert_eq!(bounds(&lines, 2), Some((1, 3)));
    }

    #[test]
    fn cursor_on_blank_line_is_none() {
        let lines = ["First.", "", "Second."];
        assert_eq!(bounds(&lines, 1), None);
    }

    #[test]
    fn block_at_buffer_start_and_end() {
        let lines = ["Line a.", "Line b.", "", "Tail line 1.", "Tail line 2."];
        assert_eq!(bounds(&lines, 0), Some((0, 1)));
        assert_eq!(bounds(&lines, 4), Some((3, 4)));
    }

    #[test]
    fn cursor_out_of_range_is_none() {
        let lines = ["Only line."];
        assert_eq!(bounds(&lines, 5), None);
        assert_eq!(bounds(&[], 0), None);
    }
}

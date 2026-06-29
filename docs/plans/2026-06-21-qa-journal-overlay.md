# Q&A Journal Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-work Q&A "journal" overlay to linux-lit: `Ctrl+j` opens a card for the scene under the reading cursor; `a` asks Claude a question about the scene (Claude draws on whole-play knowledge); each Q&A pair is a stored "page" navigable with `Ctrl+n/Ctrl+p` (within scene) and `Alt+n/Alt+p` (across scenes that have pages).

**Architecture:** Clone the gloss-overlay machinery but simpler — no TTS, no voice cycling, no block-cursor model. A new `JournalOverlay` widget (own `gtk4::TextView` + `ScrolledWindow` + `bottom_clip`, reusing the gloss row-snap scroll/clip path); a new `InputMode::JournalOverlay` with `handle_journal_key`; a new `journal_entries` table in `lit.db`; the Claude call uses the exact `glib::spawn_future_local` + `tokio_handle.spawn(claude::send_message(...))` bridge the gloss `add_gloss` uses. Whole-play awareness comes from Claude's training knowledge; only the current scene text is sent.

**Tech Stack:** Rust, GTK4 (`gtk4` crate), `rusqlite` (SQLite at `~/utono/litdb/data/lit.db`), Tokio runtime + `glib` main loop, `reqwest` (via existing `src/claude.rs`).

**Spec:** `docs/superpowers/specs/2026-06-21-qa-journal-overlay-design.md`

**Reference (clone these verbatim where noted):**
- Gloss Claude bridge: `src/input/actions/gloss.rs:700-808` (`add_gloss`).
- Gloss key handler: `src/input/keymap.rs:639-873` (`handle_gloss_key`).
- Gloss overlay widget: `src/ui/gloss_overlay.rs`.
- DB query patterns: `src/db/queries.rs` (`save_gloss`, `find_glosses_by_start`, `ensure_gloss_audio_table`).
- Scene helpers: `src/app.rs` (`current_scene_divs:5556`, `scene_label:6125`, `synopsis_label:5545`).

---

## File Structure

**New files:**
- `src/db/journal.rs` — `JournalPage` struct + `ensure_journal_table`, `save_journal_page`, `find_journal_pages`, `find_journal_scenes`, `update_journal_page`, `delete_journal_page`. (One responsibility: journal persistence.)
- `src/ui/journal_overlay.rs` — `JournalOverlay` widget (render one page; scroll/clip; ask-card). (One responsibility: journal rendering/input widget.)
- `src/input/actions/journal.rs` — open/close/navigation/ask/edit/delete + the Claude bridge. (One responsibility: journal behavior/state wiring.)

**Modified files:**
- `src/db/mod.rs` — add `pub mod journal;`.
- `src/db/queries.rs` — nothing (journal queries live in `db::journal`); the `ensure_journal_table` call is wired in `app.rs`.
- `src/gloss.rs` — add `JOURNAL_QA_PROMPT` (`LazyLock<String>`).
- `src/app.rs` — `InputMode::JournalOverlay` variant; AppState `journal_*` fields + initializers; `build_window` overlay attach; `ensure_journal_table` startup call; (`scene_text_for` helper added here).
- `src/input/actions/mod.rs` — `Action::ToggleJournalOverlay` (declaration + reader-consume guard list + `name()` arm).
- `src/input/actions/journal.rs` — declared in `src/input/actions/mod.rs` via `pub mod journal;`.
- `src/input/keymap_config.rs` — `ctrl("j")` → `ToggleJournalOverlay`.
- `src/input/keymap.rs` — top-level mode route to `handle_journal_key`; `dispatch_action` arm; the new `handle_journal_key` fn.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — `Ctrl+j` binding (stow source).
- `src/ui/keybinds_overlay.rs` — `Ctrl+j` cap + `describe()` arm (via the `update-cairo-keybinds-overlay` skill).

---

## Task 1: Journal DB layer

**Files:**
- Create: `src/db/journal.rs`
- Modify: `src/db/mod.rs` (add module declaration)
- Test: inline `#[cfg(test)]` module in `src/db/journal.rs`

- [ ] **Step 1: Declare the module**

In `src/db/mod.rs`, add alongside the existing `pub mod queries;` line:

```rust
pub mod journal;
```

- [ ] **Step 2: Write the failing test**

Create `src/db/journal.rs` with the struct, signatures, and a test (implementations stubbed with `unimplemented!()` so it compiles-then-fails):

```rust
use rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct JournalPage {
    pub id: i64,
    pub div1: i64,
    pub div2: i64,
    pub question: String,
    pub answer: String,
    pub claude_model: String,
    pub timestamp: String,
}

pub fn ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    unimplemented!()
}

pub fn save_journal_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    unimplemented!()
}

pub fn find_journal_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    unimplemented!()
}

pub fn find_journal_scenes(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(i64, i64)>, rusqlite::Error> {
    unimplemented!()
}

pub fn update_journal_page(
    conn: &Connection,
    id: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    unimplemented!()
}

pub fn delete_journal_page(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_journal_table(&conn).unwrap();
        conn
    }

    #[test]
    fn save_find_update_delete_roundtrip() {
        let conn = mem();
        // empty work has no scenes, no pages
        assert!(find_journal_scenes(&conn, "Ham").unwrap().is_empty());
        assert!(find_journal_pages(&conn, "Ham", 1, 2).unwrap().is_empty());

        // two pages in scene (1,2), one in (3,1)
        let id1 = save_journal_page(&conn, "Ham", 1, 2, "Q1?", "A1.", "claude-opus-4-8").unwrap();
        let _id2 = save_journal_page(&conn, "Ham", 1, 2, "Q2?", "A2.", "claude-opus-4-8").unwrap();
        let _id3 = save_journal_page(&conn, "Ham", 3, 1, "Q3?", "A3.", "claude-opus-4-8").unwrap();

        // pages for (1,2): two, chronological (oldest first)
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].question, "Q1?");
        assert_eq!(pages[0].answer, "A1.");
        assert_eq!(pages[1].question, "Q2?");

        // scenes-with-pages: (1,2) and (3,1), in scene order
        let scenes = find_journal_scenes(&conn, "Ham").unwrap();
        assert_eq!(scenes, vec![(1, 2), (3, 1)]);

        // update page 1
        update_journal_page(&conn, id1, "Q1b?", "A1b.", "claude-opus-4-8").unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages[0].question, "Q1b?");
        assert_eq!(pages[0].answer, "A1b.");

        // delete page 1 -> one page left in (1,2)
        delete_journal_page(&conn, id1).unwrap();
        let pages = find_journal_pages(&conn, "Ham", 1, 2).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].question, "Q2?");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --bins journal::tests::save_find_update_delete_roundtrip -- --nocapture`
Expected: PASS-to-compile, then FAIL/panic at `unimplemented!()` ("not implemented").

- [ ] **Step 4: Implement the six functions**

Replace the six `unimplemented!()` bodies in `src/db/journal.rs`:

```rust
pub fn ensure_journal_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS journal_entries (
            id          INTEGER PRIMARY KEY,
            work_abbrev TEXT    NOT NULL,
            div1        INTEGER NOT NULL,
            div2        INTEGER NOT NULL,
            question    TEXT    NOT NULL,
            answer      TEXT    NOT NULL,
            claude_model TEXT,
            timestamp   TEXT    NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journal_work_scene
            ON journal_entries(work_abbrev, div1, div2, timestamp);",
    )
}

pub fn save_journal_page(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO journal_entries
            (work_abbrev, div1, div2, question, answer, claude_model, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        rusqlite::params![work_abbrev, div1, div2, question, answer, claude_model],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_journal_pages(
    conn: &Connection,
    work_abbrev: &str,
    div1: i64,
    div2: i64,
) -> Result<Vec<JournalPage>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, div1, div2, question, answer, COALESCE(claude_model, ''), timestamp
         FROM journal_entries
         WHERE work_abbrev = ?1 AND div1 = ?2 AND div2 = ?3
         ORDER BY timestamp ASC, id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![work_abbrev, div1, div2], |row| {
        Ok(JournalPage {
            id: row.get(0)?,
            div1: row.get(1)?,
            div2: row.get(2)?,
            question: row.get(3)?,
            answer: row.get(4)?,
            claude_model: row.get(5)?,
            timestamp: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn find_journal_scenes(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<(i64, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT div1, div2 FROM journal_entries
         WHERE work_abbrev = ?1
         ORDER BY div1 ASC, div2 ASC",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn update_journal_page(
    conn: &Connection,
    id: i64,
    question: &str,
    answer: &str,
    claude_model: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET question = ?1, answer = ?2, claude_model = ?3, timestamp = datetime('now')
         WHERE id = ?4",
        rusqlite::params![question, answer, claude_model, id],
    )?;
    Ok(())
}

pub fn delete_journal_page(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM journal_entries WHERE id = ?1", [id])?;
    Ok(())
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --bins journal::tests::save_find_update_delete_roundtrip -- --nocapture`
Expected: PASS (1 passed).

- [ ] **Step 6: Commit**

```bash
git add src/db/mod.rs src/db/journal.rs
git commit -m "feat(journal): journal_entries table + CRUD queries"
```

---

## Task 2: Journal Claude prompt

**Files:**
- Modify: `src/gloss.rs` (add a `LazyLock<String>` near `USER_QUESTION_PROMPT` ~line 116)

- [ ] **Step 1: Add the prompt constant**

In `src/gloss.rs`, add this `pub static` near the other prompt statics (e.g. just after `USER_QUESTION_PROMPT`). It uses the existing `template_or` helper (`src/gloss.rs:12`) so the prompt can later be overridden from the lit.db `prompts` table, with a compiled fallback:

```rust
pub static JOURNAL_QA_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a literary interlocutor in conversation with a reader who is working through a play, one scene at a time. The reader has asked a question while reading a specific scene. The verbatim text of that scene is provided.

Answer the question substantively and in plain prose. Ground your answer in the scene text provided, but DO situate the scene within the whole play: trace how this moment echoes earlier scenes and foreshadows or is answered by later ones, and how it participates in the work's larger arcs of character, theme, and image. Drawing such connections across the full play is encouraged — this is a study companion for a reader engaging the entire work, not a spoiler-free first-read assistant, so do not withhold connections to later scenes.

Write for a thoughtful reader: clear, specific, and concrete. Quote sparingly from the scene where it helps. No markdown, no bullet lists, no numbered lists, no headers — flowing prose paragraphs only. Do not use the = sign; write paraphrases as prose. Be substantive but not padded.";
    template_or("journal.qa", FALLBACK)
});
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds with no error (a dead-code warning for the unused static is acceptable at this stage — it is used in Task 6).

- [ ] **Step 3: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(journal): add JOURNAL_QA_PROMPT system prompt"
```

---

## Task 3: Scene-text helper

**Files:**
- Modify: `src/app.rs` (add a free function near `current_scene_divs` ~line 5556)
- Test: inline `#[cfg(test)]` is not practical here (needs a `Work`); covered indirectly. Add a small unit test against a hand-built `Work` if `Work`/`Line` are constructible in tests — otherwise verify via build only.

- [ ] **Step 1: Add `scene_text_for`**

In `src/app.rs`, near `current_scene_divs` (line 5556), add a helper that gathers all lines for a `(div1, div2)` into a speaker-prefixed string in reading order. `work.lines` is already sorted by `(div1, div2, line_in_div)`, so iteration order is reading order:

```rust
/// Assemble the verbatim text of one scene `(div1, div2)` for the current work,
/// with speaker attributions, in reading order. Empty string if no current work
/// or no matching lines.
pub fn scene_text_for(state: &AppState, div1: i64, div2: i64) -> String {
    let work = match state.current_work.as_ref() {
        Some(w) => w,
        None => return String::new(),
    };
    let mut out = String::new();
    let mut last_speaker: Option<&str> = None;
    for line in work.lines.iter().filter(|l| l.div1 == div1 && l.div2 == div2) {
        match line.speaker.as_deref() {
            Some(sp) if last_speaker != Some(sp) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(sp);
                out.push('\n');
                last_speaker = Some(sp);
            }
            _ => {}
        }
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds (dead-code warning acceptable; used in Task 6).

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat(journal): scene_text_for helper to assemble scene text"
```

---

## Task 4: JournalOverlay widget

**Files:**
- Create: `src/ui/journal_overlay.rs`
- Modify: `src/ui/mod.rs` (add `pub mod journal_overlay;`)

This is a trimmed clone of `GlossOverlay`: one scrolling `TextView` for the page body (question shown as a header line, then the answer), a `bottom_clip` box, a header label (scene + page position), and the stacked ask card. **No bar-drawing, no blocks, no TTS, no echo views, no voice cycling.**

- [ ] **Step 1: Declare the module**

In `src/ui/mod.rs`, alongside `pub mod gloss_overlay;`:

```rust
pub mod journal_overlay;
```

- [ ] **Step 2: Create the widget file**

Create `src/ui/journal_overlay.rs`. Mirror the gloss overlay's CSS classes (`card`, `card-title`, etc.) so theming matches; reuse the row-snap scroll pattern from `gloss_overlay.rs:1780-1826` and the bottom-clip from `recompute_bottom_clip`.

```rust
use gtk4::prelude::*;
use gtk4::{glib, Label, Overlay};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AskFocus {
    Page,
    Ask,
}

pub struct JournalOverlay {
    pub overlay: Overlay,
    scrim: gtk4::Box,
    container: gtk4::Box,
    title: Label,         // scene label, e.g. "Hamlet — Act 1, Scene 2"
    position_label: Label, // "page 2 of 3 in this scene"
    scrolled: gtk4::ScrolledWindow,
    view: gtk4::TextView,
    bottom_clip: gtk4::Box,
    text_margins: i32,
    column_width: i32,
    font_family: RefCell<String>,
    font_size: Cell<i32>,
    last_card_size: Cell<(i32, i32)>,
    // stacked ask card (clone of gloss ask card)
    ask_container: gtk4::Box,
    ask_input: gtk4::TextView,
    ask_title: Label,
    ask_hint: Label,
    ask_focus: Cell<AskFocus>,
}

impl JournalOverlay {
    pub fn new(column_width: u32, text_margins: u32) -> Self {
        let overlay = Overlay::new();

        let scrim = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        scrim.add_css_class("overlay-scrim");
        scrim.set_visible(false);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("card");
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_visible(false);

        let title = Label::new(Some(""));
        title.add_css_class("card-title");
        title.set_halign(gtk4::Align::Start);
        container.append(&title);

        let position_label = Label::new(Some(""));
        position_label.add_css_class("card-citation");
        position_label.set_halign(gtk4::Align::Start);
        container.append(&position_label);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::External);
        scrolled.set_propagate_natural_height(false);
        scrolled.set_vexpand(true);

        let view = gtk4::TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_wrap_mode(gtk4::WrapMode::Word);
        view.add_css_class("card-body");

        // The view sits inside an Overlay so the bottom_clip box can float over
        // the partial last row (same technique as the gloss card).
        let scroll_overlay = Overlay::new();
        scroll_overlay.set_child(Some(&view));
        let bottom_clip = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        bottom_clip.add_css_class("card");
        bottom_clip.set_valign(gtk4::Align::End);
        bottom_clip.set_vexpand(false);
        bottom_clip.set_can_target(false);
        scroll_overlay.add_overlay(&bottom_clip);
        scroll_overlay.set_measure_overlay(&bottom_clip, false);
        scrolled.set_child(Some(&scroll_overlay));
        container.append(&scrolled);

        // ---- stacked ask card (clone of gloss) ----
        let ask_container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        ask_container.add_css_class("card");
        ask_container.set_visible(false);
        let ask_title = Label::new(Some("Ask a question"));
        ask_title.add_css_class("card-title");
        ask_title.set_halign(gtk4::Align::Start);
        ask_container.append(&ask_title);
        let ask_input = gtk4::TextView::new();
        ask_input.set_editable(true);
        ask_input.set_wrap_mode(gtk4::WrapMode::Word);
        ask_input.add_css_class("card-body");
        ask_input.set_vexpand(true);
        ask_container.append(&ask_input);
        let ask_hint = Label::new(Some("Ctrl+Enter to ask · Esc to cancel"));
        ask_hint.add_css_class("card-citation");
        ask_hint.set_halign(gtk4::Align::Start);
        ask_container.append(&ask_hint);
        container.append(&ask_container);

        Self {
            overlay,
            scrim,
            container,
            title,
            position_label,
            scrolled,
            view,
            bottom_clip,
            text_margins: text_margins as i32,
            column_width: column_width as i32,
            font_family: RefCell::new(String::new()),
            font_size: Cell::new(16),
            last_card_size: Cell::new((0, 0)),
            ask_container,
            ask_input,
            ask_title,
            ask_hint,
            ask_focus: Cell::new(AskFocus::Page),
        }
    }

    pub fn attach(&self, child: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(child));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.container);
        self.overlay.set_measure_overlay(&self.scrim, false);
        self.overlay.set_measure_overlay(&self.container, false);
        self.overlay.set_clip_overlay(&self.scrim, true);
        self.overlay.set_clip_overlay(&self.container, true);
    }

    fn size_card(&self, card_width: i32, card_height: i32) {
        let w = (card_width as f64 * 0.8) as i32;
        let h = (card_height as f64 * 0.8) as i32;
        self.container.set_size_request(w, h);
        self.last_card_size.set((w, h));
        self.view.set_left_margin(self.text_margins);
        self.view.set_right_margin(self.text_margins);
        let _ = self.column_width;
    }

    /// Render one page (question header + answer body) plus the scene title and
    /// the "page N of M" position. `page_index`/`page_count` are 0-based index /
    /// total count for the current scene (0/0 == empty scene).
    pub fn show_page(
        &self,
        scene_title: &str,
        page_index: usize,
        page_count: usize,
        question: &str,
        answer: &str,
        card_width: i32,
        card_height: i32,
    ) {
        self.size_card(card_width, card_height);
        self.title.set_text(scene_title);
        if page_count == 0 {
            self.position_label.set_text("page 0 of 0 in this scene");
        } else {
            self.position_label.set_text(&format!(
                "page {} of {} in this scene",
                page_index + 1,
                page_count
            ));
        }
        let body = if page_count == 0 {
            "No pages yet — press a to ask.".to_string()
        } else {
            format!("{}\n\n{}", question, answer)
        };
        self.view.buffer().set_text(&body);
        self.apply_font();
        self.ask_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
    }

    pub fn show_loading(&self) {
        let (w, h) = self.last_card_size.get();
        if w > 0 {
            self.container.set_size_request(w, h);
        }
        self.position_label.set_text("");
        self.view.buffer().set_text("Asking…");
        self.ask_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn show_message(&self, text: &str) {
        self.view.buffer().set_text(text);
        self.ask_container.set_visible(false);
        self.scrim.set_visible(true);
        self.container.set_visible(true);
    }

    pub fn hide(&self) {
        self.container.set_visible(false);
        self.scrim.set_visible(false);
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Page);
    }

    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    // ---- scroll (row-snapped, clip-aware) — clone of gloss scroll path ----

    fn row_step(&self) -> f64 {
        let (_, h) = self.view.line_yrange(&self.view.buffer().start_iter());
        if h > 0 {
            h as f64
        } else {
            (self.font_size.get() as f64) * 1.4
        }
    }

    fn snap_value_to_line(&self, value: f64) -> f64 {
        let step = self.row_step();
        if step <= 0.0 {
            return value;
        }
        (value / step).round() * step
    }

    pub fn scroll(&self, delta: i32) {
        let adj = self.scrolled.vadjustment();
        let step = self.row_step();
        let raw = adj.value() + step * 3.0 * delta as f64;
        adj.set_value(self.snap_value_to_line(raw));
        self.update_bottom_clip();
    }

    pub fn scroll_to_top(&self) {
        let adj = self.scrolled.vadjustment();
        adj.set_value(adj.lower());
        self.update_bottom_clip();
    }

    pub fn scroll_to_bottom(&self) {
        let adj = self.scrolled.vadjustment();
        let bottom = (adj.upper() - adj.page_size()).max(adj.lower());
        adj.set_value(self.snap_value_to_line(bottom));
        self.update_bottom_clip();
    }

    fn update_bottom_clip(&self) {
        // Size the clip box to mask the partial bottom row, matching the gloss
        // card. Height = the fractional remainder of the viewport not filled by
        // whole rows.
        let adj = self.scrolled.vadjustment();
        let step = self.row_step();
        if step <= 0.0 {
            self.bottom_clip.set_size_request(-1, 0);
            return;
        }
        let page = adj.page_size();
        let remainder = page - (page / step).floor() * step;
        let clip_h = remainder.round().max(0.0) as i32;
        self.bottom_clip.set_size_request(-1, clip_h);
    }

    // ---- font ----

    pub fn set_font(&self, family: &str, size: i32) {
        *self.font_family.borrow_mut() = family.to_string();
        self.font_size.set(size);
        self.apply_font();
    }

    fn apply_font(&self) {
        let family = self.font_family.borrow().clone();
        if family.is_empty() {
            return;
        }
        let css = format!(
            "textview.card-body, textview.card-body text {{ font-family: \"{}\"; font-size: {}px; }}",
            family,
            self.font_size.get()
        );
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        for w in [&self.view, &self.ask_input] {
            w.style_context()
                .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
    }

    // ---- ask card (clone of gloss ask card) ----

    pub fn ask_is_open(&self) -> bool {
        self.ask_container.is_visible()
    }

    pub fn ask_focus(&self) -> AskFocus {
        self.ask_focus.get()
    }

    pub fn open_ask_card(&self, title: &str, hint: &str) {
        self.ask_title.set_text(title);
        self.ask_hint.set_text(hint);
        self.ask_input.buffer().set_text("");
        self.ask_container.set_visible(true);
        self.apply_font();
        self.ask_focus.set(AskFocus::Ask);
        self.ask_input.grab_focus();
    }

    pub fn close_ask_card(&self) {
        self.ask_container.set_visible(false);
        self.ask_focus.set(AskFocus::Page);
    }

    pub fn toggle_ask_focus(&self) {
        let next = match self.ask_focus.get() {
            AskFocus::Page => AskFocus::Ask,
            AskFocus::Ask => AskFocus::Page,
        };
        self.ask_focus.set(next);
        if next == AskFocus::Ask {
            self.ask_input.grab_focus();
        }
    }

    pub fn take_ask_text(&self) -> String {
        let buffer = self.ask_input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        buffer.set_text("");
        text
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds. Fix any GTK API drift the compiler reports (e.g. if `line_yrange` or `load_from_data` signatures differ in this gtk4 version, the error names the exact symbol — match the gloss overlay's usage in `src/ui/gloss_overlay.rs`). Dead-code warnings for not-yet-called methods are acceptable.

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs src/ui/journal_overlay.rs
git commit -m "feat(journal): JournalOverlay widget (page render + scroll + ask card)"
```

---

## Task 5: AppState wiring, InputMode, build_window attach, startup ensure

**Files:**
- Modify: `src/app.rs` (InputMode variant; AppState fields + initializers; build_window attach; startup ensure)

- [ ] **Step 1: Add the InputMode variant**

In `src/app.rs`, in the `InputMode` enum (after `GlossVisual`, near line 57):

```rust
    JournalOverlay,
```

- [ ] **Step 2: Add AppState fields**

In the `AppState` struct (model on the `gloss_*` fields, ~line 266-372), add:

```rust
    pub journal_overlay: crate::ui::journal_overlay::JournalOverlay,
    pub journal_scene: (i64, i64),
    pub journal_pages: Vec<crate::db::journal::JournalPage>,
    pub journal_page_index: usize,
    pub journal_return_pos: Option<(usize, usize)>,
    pub journal_prompt_mode: JournalPromptMode,
```

And add the prompt-mode enum near `GlossPromptMode` (`src/app.rs:84`):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalPromptMode {
    Ask,
    Edit,
}
```

- [ ] **Step 3: Construct the widget in build_window and attach into the chain**

In `build_window`, find the gloss/translation attach block (`src/app.rs:1452-1461`). Splice the journal overlay **between** the gloss overlay and the translation overlay so it shares the same full-card scrim layer:

```rust
    // Correction overlay wraps the gamepad overlay
    let gloss_overlay = crate::ui::gloss_overlay::GlossOverlay::new(config.column_width, config.text_margins);
    gloss_overlay.attach(&gamepad_overlay.overlay);
    gloss_overlay.overlay.set_vexpand(true);

    // Journal overlay wraps the gloss overlay
    let journal_overlay = crate::ui::journal_overlay::JournalOverlay::new(config.column_width, config.text_margins);
    journal_overlay.attach(&gloss_overlay.overlay);
    journal_overlay.overlay.set_vexpand(true);

    // Translation overlay wraps the journal overlay (above journal, below pickers)
    let translation_overlay = crate::ui::translation_overlay::TranslationOverlay::new();
    translation_overlay.attach(&journal_overlay.overlay);
    translation_overlay.overlay.set_vexpand(true);
```

- [ ] **Step 4: Initialize the AppState fields**

In the `AppState { ... }` constructor (the big struct literal near lines 1730-1840, where `gloss_return_pos: None,` and `synopsis_overlay_scene: (0, 0),` live), add:

```rust
        journal_overlay,
        journal_scene: (0, 0),
        journal_pages: Vec::new(),
        journal_page_index: 0,
        journal_return_pos: None,
        journal_prompt_mode: JournalPromptMode::Ask,
```

(Note: `journal_overlay` must be moved into the struct; ensure the `let journal_overlay = ...` from Step 3 is in scope at the constructor. Follow exactly how `gloss_overlay` flows from its `let` into the struct literal.)

- [ ] **Step 5: Wire the startup table creation**

In the `BOOKMARKS_INIT.call_once` block (`src/app.rs:2668-2678`), add after the other `ensure_*` calls:

```rust
            let _ = crate::db::journal::ensure_journal_table(&conn);
```

- [ ] **Step 6: Set the overlay font where the gloss overlay's font is set**

Find where `gloss_overlay.set_font` / font application happens for the gloss overlay (grep `gloss_overlay` + `font` in `src/app.rs`) and add the parallel call for `journal_overlay.set_font(<same family>, <same size>)` so the journal card matches the reader font. If the gloss overlay's font is applied inside `display_work` or a theme-apply path, mirror it there.

Run: `rg -n 'gloss_overlay\.(set_font|apply_font|set_font_family)' src/app.rs`
Then add the matching `journal_overlay` call at each site.

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: builds. If the compiler complains the `journal_overlay` binding is used after move or not found in the constructor, reorder so the `let journal_overlay` precedes the `AppState { ... }` literal, exactly as `gloss_overlay` does.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(journal): AppState fields, InputMode, overlay attach, startup ensure"
```

---

## Task 6: Journal actions module (open/close/nav/ask/edit/delete + Claude bridge)

**Files:**
- Create: `src/input/actions/journal.rs`
- Modify: `src/input/actions/mod.rs` (add `pub mod journal;` and the `Action::ToggleJournalOverlay` variant + guard list + `name()`)

- [ ] **Step 1: Declare the module and Action variant**

In `src/input/actions/mod.rs`:

(a) Add the module declaration alongside the others (e.g. near `pub mod gloss;`):

```rust
pub mod journal;
```

(b) Add the enum variant near `ToggleGlossOverlay` (line 98):

```rust
    ToggleJournalOverlay,
```

(c) If there is a reader-consume guard list (`src/input/actions/mod.rs:213-245`) that lists `Action::ToggleGlossOverlay | Action::ToggleSynopsis | ...`, add `| Action::ToggleJournalOverlay` to it (same group as the gloss/synopsis toggles).

(d) Add the `name()` arm (`src/input/actions/mod.rs:337-352`):

```rust
            Action::ToggleJournalOverlay => "ToggleJournalOverlay",
```

- [ ] **Step 2: Create the actions module — open/close/render/nav**

Create `src/input/actions/journal.rs`. `render_current` is the single re-render entry; `toggle_overlay`, `nav_page`, `nav_scene` all funnel through it.

```rust
use crate::app::{AppState, InputMode, JournalPromptMode};
use std::cell::RefCell;
use std::rc::Rc;

/// Load the pages for `state.journal_scene` from the DB into `journal_pages`,
/// clamp the index, and render the current page (or the empty-scene card).
fn render_current(s: &mut AppState) {
    let (d1, d2) = s.journal_scene;
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();

    let pages = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_journal_pages(&conn, &work_abbrev, d1, d2).ok())
        .unwrap_or_default();

    let count = pages.len();
    if s.journal_page_index >= count {
        s.journal_page_index = count.saturating_sub(1);
    }
    let scene_title = format!(
        "{} — {}",
        s.current_work.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
        crate::app::synopsis_label(s, d1, d2),
    );

    let cw = s.content_hbox.width();
    let h = s.content_hbox.height();
    let (q, a) = if count == 0 {
        (String::new(), String::new())
    } else {
        let p = &pages[s.journal_page_index];
        (p.question.clone(), p.answer.clone())
    };
    s.journal_overlay
        .show_page(&scene_title, s.journal_page_index, count, &q, &a, cw, h);
    s.journal_pages = pages;
}

pub(crate) fn toggle_overlay(state: &Rc<RefCell<AppState>>) {
    // Close if open.
    if state.borrow().input_mode == InputMode::JournalOverlay {
        let mut s = state.borrow_mut();
        s.journal_overlay.hide();
        s.input_mode = InputMode::Reader;
        if let Some((line, top)) = s.journal_return_pos.take() {
            s.current_line = line;
            s.page_top_line = top;
            crate::input::scroll::resnap_page(&mut s);
            crate::input::highlight::update_highlight(&mut s);
        }
        return;
    }

    // Open on the scene under the cursor.
    let mut s = state.borrow_mut();
    if s.current_work.is_none() {
        return;
    }
    s.journal_return_pos = Some((s.current_line, s.page_top_line));
    s.journal_scene = crate::app::current_scene_divs(&s);
    s.journal_page_index = 0;
    s.input_mode = InputMode::JournalOverlay;
    render_current(&mut s);
}

pub(crate) fn close_overlay(state: &Rc<RefCell<AppState>>) {
    if state.borrow().input_mode == InputMode::JournalOverlay {
        toggle_overlay(state);
    }
}

/// Flip pages within the current scene (clamped, no wrap).
pub(crate) fn nav_page(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let count = s.journal_pages.len();
    if count == 0 {
        return;
    }
    let cur = s.journal_page_index as i64;
    let next = (cur + delta as i64).clamp(0, count as i64 - 1) as usize;
    if next != s.journal_page_index {
        s.journal_page_index = next;
        render_current(&mut s);
    }
}

/// Jump to the next/prev scene that has pages (skips empty scenes). Lands on
/// that scene's first page.
pub(crate) fn nav_scene(state: &Rc<RefCell<AppState>>, delta: i32) {
    let mut s = state.borrow_mut();
    let work_abbrev = s
        .current_work
        .as_ref()
        .map(|w| crate::app::base_work_abbrev(&w.abbrev).to_string())
        .unwrap_or_default();
    let scenes = crate::db::queries::open_db()
        .ok()
        .and_then(|conn| crate::db::journal::find_journal_scenes(&conn, &work_abbrev).ok())
        .unwrap_or_default();
    if scenes.is_empty() {
        return;
    }
    // Find the current scene's position among scenes-with-pages, or the nearest.
    let cur = s.journal_scene;
    let cur_pos = scenes.iter().position(|&sc| sc == cur);
    let target_idx: i64 = match cur_pos {
        Some(i) => (i as i64 + delta as i64).clamp(0, scenes.len() as i64 - 1),
        None => {
            // Current scene has no pages: step to the first/last with-pages scene.
            if delta > 0 { 0 } else { scenes.len() as i64 - 1 }
        }
    };
    let target = scenes[target_idx as usize];
    if target != s.journal_scene || cur_pos.is_none() {
        s.journal_scene = target;
        s.journal_page_index = 0;
        render_current(&mut s);
    }
}
```

- [ ] **Step 3: Add the ask/edit prompt flow + the Claude bridge**

Append to `src/input/actions/journal.rs`. The bridge is the exact `glib::spawn_future_local` + `tokio_handle.spawn` + nested `Ok(Ok(_))` shape from `gloss::add_gloss` (`src/input/actions/gloss.rs:700-808`), trimmed for the journal.

```rust
use gtk4::glib;

pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let s = state.borrow();
    s.journal_prompt_mode_set(JournalPromptMode::Ask);
    s.journal_overlay
        .open_ask_card("Ask a question about this scene", "Ctrl+Enter to ask · Esc to cancel");
}

pub(crate) fn begin_edit(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let count = s.journal_pages.len();
    if count == 0 {
        return;
    }
    s.journal_prompt_mode = JournalPromptMode::Edit;
    // Prefill nothing (Edit re-asks a new question that overwrites the page).
    s.journal_overlay
        .open_ask_card("Edit: ask a new question for this page", "Ctrl+Enter · Esc");
}

pub(crate) fn close_prompt(state: &Rc<RefCell<AppState>>) {
    state.borrow().journal_overlay.close_ask_card();
}

pub(crate) fn submit_prompt(state: &Rc<RefCell<AppState>>) {
    let (question, mode) = {
        let s = state.borrow();
        (s.journal_overlay.take_ask_text(), s.journal_prompt_mode)
    };
    close_prompt(state);
    if question.trim().is_empty() {
        return;
    }
    ask_claude(state, &question, mode);
}

fn ask_claude(state_rc: &Rc<RefCell<AppState>>, question: &str, mode: JournalPromptMode) {
    // Gather everything needed off a single borrow.
    let (work_title, work_author, work_abbrev, scene, scene_text, model, tokio_handle) = {
        let s = state_rc.borrow();
        let (d1, d2) = s.journal_scene;
        let (title, author, abbrev) = match s.current_work.as_ref() {
            Some(w) => (
                w.title.clone(),
                w.author.clone(),
                crate::app::base_work_abbrev(&w.abbrev).to_string(),
            ),
            None => return,
        };
        let scene_text = crate::app::scene_text_for(&s, d1, d2);
        (
            title,
            author,
            abbrev,
            (d1, d2),
            scene_text,
            s.config.claude_model.clone(),
            s.tokio_handle.clone(),
        )
    };

    state_rc.borrow().journal_overlay.show_loading();

    // The page id being edited (Edit mode overwrites it); -1 for Ask.
    let edit_id: i64 = if mode == JournalPromptMode::Edit {
        let s = state_rc.borrow();
        s.journal_pages
            .get(s.journal_page_index)
            .map(|p| p.id)
            .unwrap_or(-1)
    } else {
        -1
    };

    let user_msg = format!(
        "Work: {} by {}\nScene: {}\n\nScene text:\n{}\n\nReader's question:\n{}",
        work_title,
        work_author,
        crate::app::scene_label(scene.0, scene.1),
        scene_text,
        question,
    );
    let question_owned = question.to_string();
    let state_for_result = Rc::clone(state_rc);

    glib::spawn_future_local(async move {
        let model_for_db = model.clone();
        let result = tokio_handle
            .spawn(async move {
                crate::gloss::call_claude_with_prompt(
                    &crate::gloss::JOURNAL_QA_PROMPT,
                    &user_msg,
                    &model,
                )
                .await
            })
            .await;

        match result {
            Ok(Ok(answer)) => {
                if let Ok(conn) = crate::db::queries::open_db_rw() {
                    if mode == JournalPromptMode::Edit && edit_id >= 0 {
                        let _ = crate::db::journal::update_journal_page(
                            &conn,
                            edit_id,
                            &question_owned,
                            &answer,
                            &model_for_db,
                        );
                    } else {
                        let _ = crate::db::journal::save_journal_page(
                            &conn,
                            &work_abbrev,
                            scene.0,
                            scene.1,
                            &question_owned,
                            &answer,
                            &model_for_db,
                        );
                    }
                }
                // Re-borrow after the await; reload + land on the relevant page.
                let mut s = state_for_result.borrow_mut();
                // Reload pages for the scene to compute the new index.
                let pages = crate::db::queries::open_db()
                    .ok()
                    .and_then(|conn| {
                        crate::db::journal::find_journal_pages(
                            &conn,
                            &work_abbrev,
                            scene.0,
                            scene.1,
                        )
                        .ok()
                    })
                    .unwrap_or_default();
                let new_index = if mode == JournalPromptMode::Edit && edit_id >= 0 {
                    pages.iter().position(|p| p.id == edit_id).unwrap_or(0)
                } else {
                    // new page appended at end (chronological)
                    pages.len().saturating_sub(1)
                };
                s.journal_scene = scene;
                s.journal_page_index = new_index;
                render_current(&mut s);
                crate::logging::log("JOURNAL: saved page");
            }
            Ok(Err(e)) => {
                let s = state_for_result.borrow();
                s.journal_overlay.show_message(&format!("Error: {}", e));
                crate::logging::log(&format!("JOURNAL: claude error: {}", e));
            }
            Err(e) => {
                crate::logging::log(&format!("JOURNAL: tokio join error: {}", e));
            }
        }
    });
}

pub(crate) fn delete_current(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    let count = s.journal_pages.len();
    if count == 0 {
        return;
    }
    let id = s.journal_pages[s.journal_page_index].id;
    if let Ok(conn) = crate::db::queries::open_db_rw() {
        let _ = crate::db::journal::delete_journal_page(&conn, id);
    }
    // Land on the previous page, or the empty-scene card.
    if s.journal_page_index > 0 {
        s.journal_page_index -= 1;
    }
    render_current(&mut s);
}
```

Note: `render_current`, `nav_page`, `nav_scene` reload from the DB rather than mutating `journal_pages` in place — simplest correct approach and matches the gloss reload-after-write pattern.

**Delete-confirm — deliberate simplification vs. the spec.** The spec suggested reusing the gloss `delete_confirm_*` sub-overlay. This plan instead deletes immediately on `d` (no confirm), because the gloss delete-confirm machinery (`delete_confirm_container`/`delete_confirm_overlay` + `InputMode::DeleteConfirm` + `handle_delete_confirm_key`) is a sizable addition for a single page that is trivially re-askable. If a confirm step is wanted, it is a self-contained follow-up: add a `JournalDeleteConfirm` input mode, a confirm box in `JournalOverlay`, and route `y`/`n`/`Escape` to it — mirroring the gloss pattern. Flagged here so the omission is a decision, not an oversight.

- [ ] **Step 4: Add the `journal_prompt_mode_set` shim used in `begin_ask`**

`begin_ask` takes a shared `&self` borrow but needs to set a field; add this small helper method on `AppState` (in `src/app.rs`, in the `impl AppState` block) OR change `begin_ask` to take `borrow_mut`. Simplest: change `begin_ask` to use `borrow_mut` and set the field directly, dropping the shim. Replace the `begin_ask` body with:

```rust
pub(crate) fn begin_ask(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    s.journal_prompt_mode = JournalPromptMode::Ask;
    s.journal_overlay
        .open_ask_card("Ask a question about this scene", "Ctrl+Enter to ask · Esc to cancel");
}
```

(Remove the `journal_prompt_mode_set` call entirely — no new `AppState` method needed.)

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: builds. Resolve any borrow-checker complaints by matching the gloss patterns: extract values under a short borrow before `glib::spawn_future_local`, re-borrow inside the future after `.await`. Confirm `open_db`, `open_db_rw`, `base_work_abbrev`, `content_hbox`, `config.claude_model`, `tokio_handle` all exist (they are used identically in `gloss.rs`).

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/mod.rs src/input/actions/journal.rs
git commit -m "feat(journal): actions — open/close/nav/ask/edit/delete + Claude bridge"
```

---

## Task 7: Key routing — keymap_config, dispatch, handle_journal_key

**Files:**
- Modify: `src/input/keymap_config.rs` (add `ctrl("j")` binding)
- Modify: `src/input/keymap.rs` (top-level mode route; `dispatch_action` arm; new `handle_journal_key`)

- [ ] **Step 1: Bind Ctrl+j**

In `src/input/keymap_config.rs`, in the same binding group as the gloss bindings (near line 275, after the `bracketright`/`alt("g")` lines), add:

```rust
        (KeyCombo::ctrl("j"), Action::ToggleJournalOverlay),
```

- [ ] **Step 2: Dispatch the action**

In `src/input/keymap.rs`, in `dispatch_action` (near line 1971, beside `ToggleGlossOverlay =>`), add:

```rust
        ToggleJournalOverlay => crate::input::actions::journal::toggle_overlay(state),
```

- [ ] **Step 3: Route keys while the overlay is open**

In `src/input/keymap.rs`, in the top-level `match s.input_mode` (near line 115, beside the `GlossOverlay =>` arm), add:

```rust
            crate::app::InputMode::JournalOverlay => handle_journal_key(state, key_state, key_name, is_ctrl, is_shift, is_alt),
```

- [ ] **Step 4: Write `handle_journal_key`**

In `src/input/keymap.rs`, add this function (model on `handle_gloss_key` at line 639, but trimmed — no TTS, voice, blocks, font-size, or amend). It implements your chosen bindings: `j/k` scroll, `gg`/`G` top/bottom, `Ctrl+n/Ctrl+p` page-in-scene, `Alt+n/Alt+p` scene jump, `a` ask, `e` edit, `d` delete, `Escape`/`Ctrl+j` close. The ask-card guard at the top mirrors the gloss handler so typed text reaches the input:

```rust
fn handle_journal_key(
    state: &Rc<RefCell<AppState>>,
    key_state: &Rc<RefCell<KeyState>>,
    key_name: &str,
    is_ctrl: bool,
    _is_shift: bool,
    is_alt: bool,
) -> bool {
    use crate::ui::journal_overlay::AskFocus;

    // ---- Ask/edit input card intercepts Tab / Ctrl+Enter / Escape first ----
    let (ask_open, ask_focus) = {
        let s = state.borrow();
        (s.journal_overlay.ask_is_open(), s.journal_overlay.ask_focus())
    };
    if ask_open {
        if key_name == "Tab" || key_name == "ISO_Left_Tab" {
            state.borrow().journal_overlay.toggle_ask_focus();
            return true;
        }
        if is_ctrl && key_name == "Return" {
            crate::input::actions::journal::submit_prompt(state);
            return true;
        }
        if key_name == "Escape" {
            crate::input::actions::journal::close_prompt(state);
            return true;
        }
        if ask_focus == AskFocus::Ask {
            // let the keystroke reach the editable input TextView
            return false;
        }
    }

    // gg chord -> top
    if key_state.borrow().chord == ChordState::PendingG {
        key_state.borrow_mut().chord = ChordState::None;
        if key_name == "g" {
            state.borrow().journal_overlay.scroll_to_top();
        }
        return true;
    }

    if is_alt {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_scene(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_scene(state, -1);
                return true;
            }
            _ => {}
        }
    }

    if is_ctrl {
        match key_name {
            "n" => {
                crate::input::actions::journal::nav_page(state, 1);
                return true;
            }
            "p" => {
                crate::input::actions::journal::nav_page(state, -1);
                return true;
            }
            "j" => {
                // Ctrl+j toggles the overlay closed (same as the open bind).
                crate::input::actions::journal::close_overlay(state);
                return true;
            }
            _ => {}
        }
    }

    match key_name {
        "a" => {
            crate::input::actions::journal::begin_ask(state);
            true
        }
        "e" => {
            crate::input::actions::journal::begin_edit(state);
            true
        }
        "d" => {
            crate::input::actions::journal::delete_current(state);
            true
        }
        "g" => {
            KeyState::start_chord(key_state, ChordState::PendingG);
            true
        }
        "G" => {
            state.borrow().journal_overlay.scroll_to_bottom();
            true
        }
        "j" => {
            state.borrow().journal_overlay.scroll(1);
            true
        }
        "k" => {
            state.borrow().journal_overlay.scroll(-1);
            true
        }
        "Escape" => {
            crate::input::actions::journal::close_overlay(state);
            true
        }
        _ => false,
    }
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: builds. If `ChordState`/`KeyState::start_chord` paths differ, copy the exact usage from `handle_gloss_key` (`src/input/keymap.rs:639+`). Confirm the top-level `match` passes `is_shift`/`is_alt` in the right order (the journal handler ignores shift).

- [ ] **Step 6: Run the full pure-logic suite**

Run: `cargo test --bins`
Expected: PASS, including `journal::tests::save_find_update_delete_roundtrip` from Task 1 and the existing keymap tests (the keymap default-binding tests in `keymap_config.rs` should still pass; the new `ctrl("j")` bind doesn't conflict with any reader bind).

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap_config.rs src/input/keymap.rs
git commit -m "feat(journal): Ctrl+j open + handle_journal_key (nav/ask/edit/delete)"
```

---

## Task 8: Keymap.json stow source + Ctrl+/ overlay

**Files:**
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
- Modify: `src/ui/keybinds_overlay.rs`

- [ ] **Step 1: Add the binding to the stow source**

`keymap.json` overrides the compiled defaults, so the binding must exist in both or the JSON silently wins. Add an entry to `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` matching the file's existing object shape. Inspect an existing ctrl binding first:

Run: `rg -n '"ctrl"|ToggleGloss|ToggleSynopsis' ~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

Then add (matching that shape exactly — keys are `key`/`action` plus modifier flags):

```json
{ "key": "j", "ctrl": true, "action": "ToggleJournalOverlay" }
```

- [ ] **Step 2: Update the Ctrl+/ keybinds overlay**

Use the `update-cairo-keybinds-overlay` skill (per the project's mandatory cross-reference rules in CLAUDE.md). It will:
- Add `Ctrl+j` to the appropriate row table (`HOME_ROW` for `j`, ctrl modifier) in `src/ui/keybinds_overlay.rs` with the action label `ToggleJournalOverlay`.
- Add a `describe()` arm for the label: e.g. `"ToggleJournalOverlay" => "Open/close the Q&A journal for the current scene -> journal::toggle_overlay — src/input/actions/journal.rs"`.

Run the skill: invoke `update-cairo-keybinds-overlay` and follow its three-pass cross-reference (no blank slot hides a real binding; no label names the wrong action; every label has a `describe()` arm).

- [ ] **Step 3: Deploy the stow package**

Run:
```bash
cd ~/tty-dotfiles && stow linux-lit
```
Expected: no conflict output (symlink already in place; the edited file is the same target).

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds (the keybinds overlay is compiled Cairo code; a missing `describe()` arm renders blank rather than failing to compile, so visually confirm in Step 5 of Task 9).

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git -C ~/tty-dotfiles add linux-lit/.config/linux-lit/keymap.json
git commit -m "feat(journal): keymap.json + Ctrl+/ overlay entry for Ctrl+j"
git -C ~/tty-dotfiles commit -m "linux-lit: add Ctrl+j journal binding"
```

(Two repos: the linux-lit commit and the tty-dotfiles commit are separate. If `~/tty-dotfiles` has its own commit conventions, follow them.)

---

## Task 9: Build, pure tests, and hand off runtime verification

**Files:** none (verification only)

- [ ] **Step 1: Full build + clippy**

Run:
```bash
cargo build
cargo clippy
```
Expected: builds clean; address any clippy warnings introduced by the new code (e.g. needless clones) by matching surrounding style.

- [ ] **Step 2: Full pure-logic test suite**

Run: `cargo test --bins`
Expected: PASS (includes Task 1's journal DB roundtrip + all existing tests).

- [ ] **Step 3: State that runtime verification is user-gated**

Per CLAUDE.md ("When to ASK THE USER to run e2e-env.sh"): this change's acceptance is **visual** (overlay renders, scrolls without clipping, ask-card geometry, reveal timing) and an agent generally **cannot** launch `cage` on the live dwl session. Do not claim the overlay is verified from a build alone.

- [ ] **Step 4: Give the user the exact verification commands**

Ask the user to run the headless harness and a manual single-work launch:

```bash
./scripts/e2e-env.sh cargo test --test line_clipping -- --ignored --nocapture
```

And to eyeball the journal specifically (manual cage launch from CLAUDE.md "Headless Verification"), then in the reader: press `Ctrl+j` to open the journal on the current scene, `a` to ask a question (requires `ANTHROPIC_API_KEY` in the env), confirm the answer renders and scrolls with `j`/`k`, `Ctrl+n`/`Ctrl+p` flip pages within the scene, `Alt+n`/`Alt+p` jump scenes, `e` edits, `d` deletes, `Escape` closes and returns the cursor. Capture `grim` screenshots of: an empty-scene page, a populated page, and a long answer scrolled to the bottom (to confirm no bottom-clip).

- [ ] **Step 5: Open every screenshot and report inline**

Per the "UI review protocol" in CLAUDE.md: after the user's e2e run, open every PNG in `target/ui/` (and any `_clip.png`), quote the on-screen text, and call out any clipping/layout problem by eye.

- [ ] **Step 6: Final commit (if any review fixes were made)**

```bash
git add -A
git commit -m "fix(journal): address runtime/clipping review findings"
```

(Skip if no fixes were needed.)

---

## Notes for the implementer

- **Borrow discipline (the #1 hazard):** every Claude-bridge function borrows `AppState` *once* to extract owned values (`ctx`/scene/model/`tokio_handle`), drops the borrow, calls `show_loading()` under a fresh short borrow, then re-borrows inside the `glib::spawn_future_local` future *after* `.await`. Never hold a `borrow()`/`borrow_mut()` across an `.await`. Copy the shape from `gloss::add_gloss` exactly.
- **`base_work_abbrev` / `-Amb` normalization:** always key journal rows by `base_work_abbrev(&work.abbrev)` so `-Amb` editions share a journal (matches the gloss behavior). It's applied in `render_current`, `nav_scene`, and `ask_claude`.
- **`content_hbox.width()/height()`** are the card-sizing source used by the gloss overlay; reuse them verbatim.
- **GTK API drift:** if any `gtk4` call in `journal_overlay.rs` doesn't compile (CSS provider, `line_yrange`, scroll policy enums), the gloss overlay (`src/ui/gloss_overlay.rs`) is the authoritative reference for the exact call shape in this crate version — match it.
- **No new dependencies** — everything uses crates already in `Cargo.toml` (`gtk4`, `glib`, `rusqlite`, `tokio`, and the existing `claude`/`gloss` modules).

# Echo Turns Picker (Ctrl+Shift+G) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Ctrl+Shift+G` picker that lists every turn in the current work that has echoes, in reading order; selecting a turn jumps the cursor there and opens that turn's echoes overlay.

**Architecture:** A new DB query returns the work's echo turns. A new GTK overlay picker (modeled on `echo_picker.rs`, attached as an `add_overlay` panel — never into the reader's size-bearing chain) displays them. A new `InputMode`, `Action`, and keybind route to open/confirm handlers in `echoes.rs`. Confirm jumps the cursor to the turn's line, then reuses the existing `show_echoes_for_cursor_line` so the overlay opens from cache.

**Tech Stack:** Rust, GTK4 (gtk4-rs), rusqlite (SQLite), glib.

---

## File Structure

- `src/db/queries.rs` — add `EchoTurnSummary` struct + `list_echo_turns_for_work` query + unit test.
- `src/ui/echo_turns_picker.rs` — **new** picker widget (modeled on `echo_picker.rs`).
- `src/ui/mod.rs` — register the new module.
- `src/app.rs` — add `InputMode::EchoTurnsPicker`, `echo_turns_picker` field, construct + attach the picker, include it in the `AppState` initializer.
- `src/input/actions/mod.rs` — add `Action::ShowEchoTurns` (enum variant, `Category` arm, `name()` arm).
- `src/input/keymap_config.rs` — bind `Ctrl+Shift+G` → `ShowEchoTurns` in `app_bindings()`.
- `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json` — same binding (JSON override).
- `src/input/keymap.rs` — route the new mode to a handler; dispatch the action.
- `src/input/actions/echoes.rs` — `open_echo_turns_picker` + `confirm_echo_turns_pick`.

---

## Task 1: DB query `list_echo_turns_for_work`

**Files:**
- Modify: `src/db/queries.rs` (add struct + fn near the other echo queries ~line 1130; add test in the `#[cfg(test)] mod tests` block ~line 1359)

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `src/db/queries.rs` (after `add_curated_echo_link_inserts_at_top_shifting_curated`):

```rust
    #[test]
    fn list_echo_turns_for_work_returns_only_linked_turns_in_reading_order() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE echo_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT, work_abbrev TEXT NOT NULL,
                div1 INTEGER, div2 INTEGER, start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL, speaker TEXT, turn_text TEXT NOT NULL
             );
             CREATE TABLE echo_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT, turn_id INTEGER NOT NULL,
                echo_work_abbrev TEXT, echo_div1 INTEGER, echo_div2 INTEGER,
                echo_text TEXT, similarity REAL, curated INTEGER, rank INTEGER,
                echo_start_line INTEGER
             );
             -- Two Hamlet turns with links, one without; one turn in another work.
             INSERT INTO echo_turns (id, work_abbrev, div1, div2, start_line, end_line, speaker, turn_text)
                VALUES
                (1, 'Ham', 3, 1, 56, 60, 'HAMLET', 'To be or not to be'),
                (2, 'Ham', 1, 2, 10, 12, 'HAMLET', 'O that this too too'),
                (3, 'Ham', 5, 1, 1, 2, 'GHOST', 'no links here'),
                (4, 'Mac', 1, 1, 1, 2, 'MACBETH', 'is this a dagger');
             INSERT INTO echo_links (turn_id, echo_work_abbrev, echo_text, curated, rank)
                VALUES
                (1, 'Mac', 'echo a', 0, 0),
                (1, 'Lr', 'echo b', 1, 1),
                (2, 'Mac', 'echo c', 0, 0),
                (4, 'Ham', 'echo d', 0, 0);",
        ).unwrap();

        let rows = list_echo_turns_for_work(&conn, "Ham").unwrap();
        // Turn 3 (no links) and turn 4 (other work) excluded.
        let ids: Vec<i64> = rows.iter().map(|r| r.turn_id).collect();
        // Reading order: (1,2,10) before (3,1,56) -> turn 2 first, then turn 1.
        assert_eq!(ids, vec![2, 1]);
        assert_eq!(rows[0].speaker, "HAMLET");
        assert_eq!(rows[0].div1, 1);
        assert_eq!(rows[0].div2, 2);
        assert_eq!(rows[0].start_line, 10);
        assert_eq!(rows[1].turn_text, "To be or not to be");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test list_echo_turns_for_work_returns_only_linked_turns_in_reading_order`
Expected: FAIL to compile — `cannot find function list_echo_turns_for_work` and `EchoTurnSummary` not found.

- [ ] **Step 3: Write the struct + query**

Add to `src/db/queries.rs` immediately after the `StoredEchoLink` struct (around line 1063, before `ensure_echo_tables`):

```rust
/// A turn in a work that has at least one echo link. Used by the
/// echo-turns picker (Ctrl+Shift+G) to list all annotated turns.
#[derive(Debug, Clone)]
pub struct EchoTurnSummary {
    pub turn_id: i64,
    pub div1: i64,
    pub div2: i64,
    pub start_line: i64, // line_in_div of the turn's first line
    pub speaker: String,
    pub turn_text: String,
}

/// List every turn in `work_abbrev` that has >= 1 echo link, in reading
/// order (div1, div2, start_line). The JOIN + GROUP BY guarantees only
/// turns with links appear.
pub fn list_echo_turns_for_work(
    conn: &Connection,
    work_abbrev: &str,
) -> Result<Vec<EchoTurnSummary>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.div1, t.div2, t.start_line, t.speaker, t.turn_text \
         FROM echo_turns t \
         JOIN echo_links l ON l.turn_id = t.id \
         WHERE t.work_abbrev = ?1 \
         GROUP BY t.id \
         ORDER BY t.div1, t.div2, t.start_line",
    )?;
    let rows = stmt.query_map([work_abbrev], |row| {
        Ok(EchoTurnSummary {
            turn_id: row.get(0)?,
            div1: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            div2: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            start_line: row.get(3)?,
            speaker: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            turn_text: row.get(5)?,
        })
    })?;
    rows.collect()
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test list_echo_turns_for_work_returns_only_linked_turns_in_reading_order`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "Add list_echo_turns_for_work query for echo-turns picker"
```

---

## Task 2: Picker widget `EchoTurnsPicker`

**Files:**
- Create: `src/ui/echo_turns_picker.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the picker module**

Create `src/ui/echo_turns_picker.rs` with this complete content:

```rust
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, Overlay, ScrolledWindow};

use crate::db::queries::EchoTurnSummary;

/// Picker listing every turn in the current work that has echoes
/// (Ctrl+Shift+G). Selecting a turn jumps the cursor there and opens the
/// echoes overlay. Matches the library-picker look-and-feel.
pub struct EchoTurnsPicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    scrim: GtkBox,
    list_box: ListBox,
    pub items: Vec<EchoTurnSummary>,
    titles: std::collections::HashMap<String, String>,
    work_abbrev: String,
}

impl EchoTurnsPicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();

        let scrim = GtkBox::builder().hexpand(true).vexpand(true).build();
        scrim.add_css_class("library-picker-scrim");
        scrim.set_visible(false);

        let picker_box = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .halign(Align::Center)
            .valign(Align::Center)
            .width_request(640)
            .height_request(520)
            .build();
        picker_box.add_css_class("library-picker");

        let header_box = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .build();
        header_box.add_css_class("library-picker-header");

        let header_title = Label::builder()
            .label("ECHOES IN THIS WORK")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        header_title.add_css_class("library-picker-title");
        header_box.append(&header_title);

        let list_box = ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .build();

        let footer_label = Label::builder()
            .label("j/k navigate  ·  Enter select  ·  Esc cancel")
            .halign(Align::Start)
            .hexpand(true)
            .build();
        footer_label.add_css_class("library-picker-footer");

        picker_box.append(&header_box);
        picker_box.append(&scrolled);
        picker_box.append(&footer_label);

        Self {
            overlay,
            picker_box,
            scrim,
            list_box,
            items: Vec::new(),
            titles: std::collections::HashMap::new(),
            work_abbrev: String::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        self.overlay.set_child(Some(base));
        self.overlay.add_overlay(&self.scrim);
        self.overlay.add_overlay(&self.picker_box);
        self.picker_box.set_visible(false);
    }

    pub fn set_titles(&mut self, titles: std::collections::HashMap<String, String>) {
        self.titles = titles;
    }

    pub fn set_items(&mut self, items: Vec<EchoTurnSummary>, work_abbrev: String) {
        self.items = items;
        self.work_abbrev = work_abbrev;
    }

    pub fn show(&self) {
        self.populate_list();
        self.scrim.set_visible(true);
        self.picker_box.set_visible(true);
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
        self.scrim.set_visible(false);
    }

    fn populate_list(&self) {
        while let Some(row) = self.list_box.first_child() {
            self.list_box.remove(&row);
        }

        let title = self
            .titles
            .get(&self.work_abbrev)
            .cloned()
            .unwrap_or_else(|| self.work_abbrev.clone());

        for (idx, item) in self.items.iter().enumerate() {
            let row_box = GtkBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(2)
                .build();

            let meta = format!("{}  ·  {} {}.{}", item.speaker, title, item.div1, item.div2);
            let meta_label = Label::builder()
                .label(&meta)
                .halign(Align::Start)
                .build();
            meta_label.add_css_class("picker-item-detail");
            row_box.append(&meta_label);

            let first_line = item.turn_text.lines().next().unwrap_or("").trim();
            let text_label = Label::builder()
                .label(first_line)
                .halign(Align::Start)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .build();
            row_box.append(&text_label);

            let row = ListBoxRow::builder().child(&row_box).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        if let Some(row) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&row));
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.list_box
            .selected_row()
            .and_then(|row| row.widget_name().parse::<usize>().ok())
    }

    pub fn move_selection(&self, delta: i32) {
        let current = self.list_box.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (current + delta).max(0);
        if let Some(row) = self.list_box.row_at_index(next) {
            self.list_box.select_row(Some(&row));
        }
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, add (alphabetically among the `pub mod` lines, next to `echo_picker`):

```rust
pub mod echo_turns_picker;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds (warnings about unused `EchoTurnsPicker` are fine at this stage).

- [ ] **Step 4: Commit**

```bash
git add src/ui/echo_turns_picker.rs src/ui/mod.rs
git commit -m "Add EchoTurnsPicker widget"
```

---

## Task 3: Action + InputMode + AppState wiring

**Files:**
- Modify: `src/input/actions/mod.rs` (enum variant ~line 94; `Category` arm ~line 195; `name()` arm ~line 305)
- Modify: `src/app.rs` (InputMode enum ~line 48; AppState field ~line 186; construct+attach ~line 791; initializer ~line 1006)

- [ ] **Step 1: Add the Action variant**

In `src/input/actions/mod.rs`, add `ShowEchoTurns` right after `ReopenEchoes` in the enum (line ~95):

```rust
    ShowEchoes,
    ReopenEchoes,
    ShowEchoTurns,
```

Add it to the `Category::Vocab` match group (after `Action::ReopenEchoes` at line ~196):

```rust
            | Action::ShowEchoes
            | Action::ReopenEchoes
            | Action::ShowEchoTurns
```

Add the `name()` arm (after `Action::ReopenEchoes => "ReopenEchoes",` at line ~306):

```rust
            Action::ShowEchoes => "ShowEchoes",
            Action::ReopenEchoes => "ReopenEchoes",
            Action::ShowEchoTurns => "ShowEchoTurns",
```

- [ ] **Step 2: Add the InputMode variant**

In `src/app.rs`, add `EchoTurnsPicker` to the `InputMode` enum after `EchoPicker` (line ~48):

```rust
    EchoPicker,
    EchoTurnsPicker,
    EchoesOverlay,
```

- [ ] **Step 3: Add the AppState field**

In `src/app.rs`, add the field after `echo_picker` (line ~186):

```rust
    pub echo_picker: crate::ui::echo_picker::EchoPicker,
    pub echo_turns_picker: crate::ui::echo_turns_picker::EchoTurnsPicker,
```

- [ ] **Step 4: Construct and attach the picker**

In `src/app.rs`, find the `echo_line_picker` attach block (around line 791, the `add_overlay` comment about "overlay panel, NOT a chain link"). Add immediately **before** that block:

```rust
    // Echo turns picker (Ctrl+Shift+G: list all turns in this work that have
    // echoes). add_overlay panel onto the outer overlay, NOT wrapped into the
    // reader's size-bearing chain (wrapping collapses the reader layout).
    let echo_turns_picker = crate::ui::echo_turns_picker::EchoTurnsPicker::new();
    authorship_picker.overlay.add_overlay(&echo_turns_picker.picker_box());
```

This needs a `picker_box()` accessor since the field is private. Add it to `EchoTurnsPicker` in `src/ui/echo_turns_picker.rs` (after `attach`):

```rust
    pub fn picker_box(&self) -> &GtkBox {
        &self.picker_box
    }
```

- [ ] **Step 5: Add to the AppState initializer**

In `src/app.rs`, find where `echo_picker,` appears in the `AppState { ... }` struct initializer (line ~1006) and add the new field after it:

```rust
        echo_picker,
        echo_turns_picker,
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: builds. If the compiler reports a non-exhaustive match on `InputMode` somewhere, that is Task 4 — proceed.

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/app.rs src/ui/echo_turns_picker.rs
git commit -m "Wire ShowEchoTurns action, EchoTurnsPicker InputMode and AppState field"
```

---

## Task 4: Open + confirm handlers

**Files:**
- Modify: `src/input/actions/echoes.rs` (add two functions near the other public echo handlers, e.g. after `show_echoes_for_cursor_line` ends ~line 300)

- [ ] **Step 1: Add `open_echo_turns_picker`**

Add to `src/input/actions/echoes.rs`:

```rust
/// Open the echo-turns picker: list every turn in the current work that has
/// echoes (Ctrl+Shift+G). Empty work -> toast and stay in Reader.
pub(crate) fn open_echo_turns_picker(state_rc: &Rc<RefCell<AppState>>) {
    let work_abbrev = match state_rc.borrow().current_work.as_ref() {
        Some(w) => w.abbrev.clone(),
        None => return,
    };

    let (turns, titles) = {
        let conn = match crate::db::queries::open_db() {
            Ok(c) => c,
            Err(e) => {
                crate::logging::log(&format!("ECHO-TURNS: open_db failed: {e}"));
                show_no_echo_turns_toast(state_rc);
                return;
            }
        };
        let turns = crate::db::queries::list_echo_turns_for_work(&conn, &work_abbrev)
            .unwrap_or_default();
        let titles = crate::db::queries::load_work_titles(&conn).unwrap_or_default();
        (turns, titles)
    };

    if turns.is_empty() {
        crate::logging::log("ECHO-TURNS: no echo turns in this work");
        show_no_echo_turns_toast(state_rc);
        return;
    }

    let mut s = state_rc.borrow_mut();
    s.echo_turns_picker.set_titles(titles);
    s.echo_turns_picker.set_items(turns, work_abbrev);
    s.echo_turns_picker.show();
    s.input_mode = crate::app::InputMode::EchoTurnsPicker;
}

fn show_no_echo_turns_toast(state_rc: &Rc<RefCell<AppState>>) {
    let s = state_rc.borrow();
    s.chapter_toast.set_text("No echoes in this work");
    s.chapter_toast.set_visible(true);
    let toast = s.chapter_toast.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        toast.set_visible(false);
    });
}
```

- [ ] **Step 2: Add `confirm_echo_turns_pick`**

Add to `src/input/actions/echoes.rs`:

```rust
/// Confirm the echo-turns picker selection: jump the cursor to the turn's
/// first line, then open its echoes overlay via the normal cursor path
/// (cache hit, no API call).
pub(crate) fn confirm_echo_turns_pick(
    state_rc: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let picked = {
        let s = state_rc.borrow();
        s.echo_turns_picker
            .selected_index()
            .and_then(|idx| s.echo_turns_picker.items.get(idx).cloned())
    };
    let picked = match picked {
        Some(p) => p,
        None => {
            let s = state_rc.borrow();
            s.echo_turns_picker.hide();
            return;
        }
    };

    // Resolve (div1, div2, start_line) -> buffer line index and jump.
    let jumped = {
        let mut s = state_rc.borrow_mut();
        s.echo_turns_picker.hide();
        s.input_mode = crate::app::InputMode::Reader;

        let work_idx = s.current_work.as_ref().and_then(|w| {
            w.lines.iter().position(|l| {
                l.div1 == picked.div1
                    && l.div2 == picked.div2
                    && l.line_in_div == picked.start_line
            })
        });
        match work_idx {
            Some(wi) => {
                let buf_idx = match s.line_map {
                    Some(ref lm) => lm.work_to_buffer[wi],
                    None => wi,
                };
                s.current_line = buf_idx;
                crate::input::highlight::update_highlight_and_center(&mut s);
                true
            }
            None => {
                crate::logging::log(&format!(
                    "ECHO-TURNS: turn line {}.{}.{} not found in loaded work",
                    picked.div1, picked.div2, picked.start_line
                ));
                false
            }
        }
    };

    if jumped {
        show_echoes_for_cursor_line(state_rc, tokio_handle);
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds. Confirm `chapter_toast`, `line_map`, `work_to_buffer`, and `current_work.lines` field names match (they are used identically in `concordance.rs` and `pickers.rs`). If a name differs, fix to match the existing usage in `src/input/actions/pickers.rs:181-195`.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/echoes.rs
git commit -m "Add open/confirm handlers for echo-turns picker"
```

---

## Task 5: Key routing + dispatch

**Files:**
- Modify: `src/input/keymap.rs` (mode routing ~line 97; dispatch ~line 1226; add handler fn)

- [ ] **Step 1: Route the new InputMode**

In `src/input/keymap.rs`, add a routing arm right after the `EchoPicker` arm (line ~97):

```rust
            crate::app::InputMode::EchoPicker => handle_echo_picker_key(state, key_name, tokio_handle),
            crate::app::InputMode::EchoTurnsPicker => handle_echo_turns_picker_key(state, key_name, tokio_handle),
```

- [ ] **Step 2: Add the handler function**

In `src/input/keymap.rs`, add this function next to `handle_echo_picker_key` (~line 747):

```rust
fn handle_echo_turns_picker_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
    tokio_handle: &tokio::runtime::Handle,
) {
    match key_name {
        "j" | "Down" => state.borrow().echo_turns_picker.move_selection(1),
        "k" | "Up" => state.borrow().echo_turns_picker.move_selection(-1),
        "Return" | "KP_Enter" => {
            crate::input::actions::echoes::confirm_echo_turns_pick(state, tokio_handle);
        }
        "Escape" => {
            let s = state.borrow();
            s.echo_turns_picker.hide();
            drop(s);
            state.borrow_mut().input_mode = crate::app::InputMode::Reader;
        }
        _ => {}
    }
}
```

Note: confirm the Enter key name(s) match the codebase. Check with
`rg -n '"Return"|"KP_Enter"' src/input/keymap.rs` and mirror what
`handle_echo_picker_key` / the other picker handlers use.

- [ ] **Step 3: Dispatch the action**

In `src/input/keymap.rs`, add a dispatch arm after `ReopenEchoes` (~line 1227):

```rust
        ShowEchoes => crate::input::actions::echoes::show_echoes_for_cursor_line(state, tokio_handle),
        ReopenEchoes => crate::input::actions::echoes::reopen_echoes(state, tokio_handle),
        ShowEchoTurns => crate::input::actions::echoes::open_echo_turns_picker(state),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds with no non-exhaustive-match errors for `InputMode` or `Action`.

- [ ] **Step 5: Commit**

```bash
git add src/input/keymap.rs
git commit -m "Route and dispatch echo-turns picker keys"
```

---

## Task 6: Keybindings (compiled + JSON)

**Files:**
- Modify: `src/input/keymap_config.rs` (`app_bindings()` ~line 325)
- Modify: `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`

- [ ] **Step 1: Add the compiled-in binding**

In `src/input/keymap_config.rs`, inside `app_bindings()` (after the `ToggleNavTest` line ~329):

```rust
        (KeyCombo::ctrl_shift("T"), Action::ToggleNavTest),
        (KeyCombo::ctrl_shift("G"), Action::ShowEchoTurns),
```

- [ ] **Step 2: Add the JSON binding**

In `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`, add an entry in the `"reader"` array (alphabetical neighborhood near the other `ctrl+shift` capital-letter entries, e.g. after the `"L"` SaveAndQuit line):

```json
    {"key": "G", "ctrl": true, "shift": true, "action": "ShowEchoTurns"},
```

- [ ] **Step 3: Deploy the stow package**

Run:

```bash
cd ~/tty-dotfiles && stow linux-lit
```

Expected: no output (symlink already in place; file content updated in source).

- [ ] **Step 4: Verify JSON is valid**

Run: `jq -e '.reader[] | select(.action == "ShowEchoTurns")' ~/.config/linux-lit/keymap.json`
Expected: prints the JSON object (non-zero exit only if missing/invalid).

- [ ] **Step 5: Build + clippy + test**

Run: `cargo build && cargo clippy && cargo test`
Expected: build clean, clippy clean, all tests pass (including `list_echo_turns_for_work_returns_only_linked_turns_in_reading_order`).

- [ ] **Step 6: Commit (both repos)**

```bash
git add src/input/keymap_config.rs
git commit -m "Bind Ctrl+Shift+G to ShowEchoTurns"
cd ~/tty-dotfiles && git add linux-lit/.config/linux-lit/keymap.json && \
  git commit -m "linux-lit: bind Ctrl+Shift+G to ShowEchoTurns"
```

---

## Task 7: Manual verification (user runs the app)

**Files:** none.

- [ ] **Step 1: User launches and verifies**

Tell the user to run `cargo run` and confirm:
- On a work with curated/cached echoes, `Ctrl+Shift+G` opens a picker titled
  "ECHOES IN THIS WORK" listing turns in reading order.
- `j`/`k` move the selection; `Esc` closes back to the reader.
- `Enter` jumps the cursor to the selected turn and opens its echoes overlay
  (same as pressing `i` on that line).
- On a work with no echoes, `Ctrl+Shift+G` shows the "No echoes in this work"
  toast and stays in the reader.

Do NOT run `cargo run` yourself (project rule: user runs the app).

---

## Notes for the implementer

- **Overlay, not chain link:** the picker is added via `add_overlay`, never
  wrapped into the reader's size-bearing widget chain. Wrapping has previously
  collapsed the reader layout (sw_h stuck at 0). Match the `echo_line_picker`
  pattern at `app.rs:792`.
- **Both keybind files:** the JSON in `~/.config/linux-lit/keymap.json`
  overrides compiled defaults. If you only edit `keymap_config.rs`, the JSON
  silently shadows it. Edit both.
- **Reuse, don't duplicate:** confirm reuses `show_echoes_for_cursor_line` so
  the overlay-population logic lives in exactly one place.
- **Don't restart a running instance:** a running linux-lit rewrites config on
  exit; only change config/keymap when no instance is running (project memory).
```

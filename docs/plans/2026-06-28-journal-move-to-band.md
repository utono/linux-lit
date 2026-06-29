# Move a journal Q&A to a different band — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Ctrl+Shift+J` keybind in the journal overlay that moves the current Q&A entry to a different band — any scene/chapter in the work, or the whole-work band — via a reused picker overlay, updating `scope` + `(div1,div2)` in `lit.db` and following the entry to its new band.

**Architecture:** A new `move_journal_page` DB function updates one row by `id`. A new `JournalMovePicker` (a near-clone of `JournalQaPicker`) lists every target band. Two new action handlers (`open_move_picker`, `confirm_move_picker`) and a new `InputMode::JournalMovePicker` wire it into the existing journal-overlay key flow and the shared `handle_picker_key` dispatch.

**Tech Stack:** Rust, GTK4 (gtk4-rs), SQLite (rusqlite), sourceview5.

**Spec:** `docs/superpowers/specs/2026-06-28-journal-move-to-band-design.md`

## Global Constraints

- Do NOT run `cargo run` — the user runs the app. The agent verifies with `cargo build` and `cargo test --bins`; GUI behavior is user-verified.
- A chapter is `scope='scene'` with `(div1,div2) = (chapter, 0)` — there is NO `'chapter'` scope.
- Whole-work entries use `(div1,div2) = (-1,-1)` (`crate::app::JOURNAL_WORK_DIV`).
- Passage entries are citation-anchored and are NOT movable; the target list never offers a passage target.
- The existing `update_journal_page` is untouched (it edits only Q&A text).
- Pagination/boundary facts come from authoritative `(div1,div2)` metadata, never re-inferred from buffer text.
- End commit messages with the standard Co-Authored-By / Claude-Session trailer used in this repo.

---

## File Structure

- `src/db/journal.rs` — add `move_journal_page` + unit test (Task 1).
- `src/ui/journal_move_picker.rs` — NEW: the `JournalMovePicker` widget + `MoveTargetRow` (Task 2).
- `src/ui/mod.rs` — register the new module (Task 2).
- `src/input/picker_dispatch.rs` — register the picker for its mode (Task 4).
- `src/app/mod.rs` — `InputMode::JournalMovePicker` variant; construct + attach the picker; wire its search-entry filter (Tasks 3, 4).
- `src/input/actions/journal.rs` — `move_target_rows`, `open_move_picker`, `confirm_move_picker` + unit test (Tasks 3, 5).
- `src/input/keymap.rs` — `Ctrl+Shift+J` arm; add mode to the picker-key group, Hide arm, Confirm arm (Task 6).
- `src/ui/keybinds_overlay.rs` — document the bind in the Ctrl+/ overlay (Task 7).

---

## Task 1: DB layer — `move_journal_page`

**Files:**
- Modify: `src/db/journal.rs` (add function after `update_journal_page`, ~line 220; add test in the `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub fn move_journal_page(conn: &Connection, id: i64, scope: &str, div1: i64, div2: i64) -> Result<(), rusqlite::Error>`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/db/journal.rs`:

```rust
#[test]
fn move_page_changes_band_scene_to_work_and_back() {
    let conn = mem();
    let id = save_journal_page(&conn, "Ham", 1, 2, "Q?", "A.", "m", "scene").unwrap();

    // Move scene -> work.
    move_journal_page(&conn, id, "work", -1, -1).unwrap();
    assert!(find_journal_pages(&conn, "Ham", 1, 2).unwrap().is_empty());
    let work = find_work_pages(&conn, "Ham").unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].id, id);
    assert_eq!(work[0].div1, -1);
    assert_eq!(work[0].div2, -1);

    // Move work -> a different scene.
    move_journal_page(&conn, id, "scene", 3, 1).unwrap();
    assert!(find_work_pages(&conn, "Ham").unwrap().is_empty());
    let scene = find_journal_pages(&conn, "Ham", 3, 1).unwrap();
    assert_eq!(scene.len(), 1);
    assert_eq!(scene[0].id, id);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins move_page_changes_band -- --nocapture`
Expected: FAIL — `cannot find function move_journal_page in this scope`.

- [ ] **Step 3: Write minimal implementation**

Add after `update_journal_page` (after line ~220) in `src/db/journal.rs`:

```rust
/// Re-target an existing journal entry to a different band by updating its
/// `scope` + `(div1, div2)` in place. Used by the journal overlay's
/// "move to band" action (Ctrl+Shift+J). Does NOT touch question/answer.
/// For the whole-work band pass `scope = "work"` and `div1 = div2 = -1`;
/// for a scene/chapter pass `scope = "scene"` and the scene's `(div1, div2)`.
pub fn move_journal_page(
    conn: &Connection,
    id: i64,
    scope: &str,
    div1: i64,
    div2: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE journal_entries
         SET scope = ?1, div1 = ?2, div2 = ?3
         WHERE id = ?4",
        rusqlite::params![scope, div1, div2, id],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bins move_page_changes_band -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/journal.rs
git commit -m "feat(journal): move_journal_page DB function to re-target an entry's band

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 2: The `JournalMovePicker` widget

**Files:**
- Create: `src/ui/journal_move_picker.rs`
- Modify: `src/ui/mod.rs` (add `pub mod journal_move_picker;` alongside `pub mod journal_picker;` at line ~18)

**Interfaces:**
- Produces:
  - `pub struct MoveTargetRow { pub band: crate::app::JournalBand, pub label: String }` (derives `Clone`)
  - `pub struct JournalMovePicker { pub overlay: Overlay, /* private */ }`
  - `impl JournalMovePicker`: `pub fn new() -> Self`, `pub fn attach(&self, base: &impl IsA<gtk4::Widget>)`, `pub fn set_items(&mut self, items: Vec<MoveTargetRow>)`, `pub fn show(&self)`, `pub fn hide(&self)`, `pub fn is_visible(&self) -> bool`, `pub fn search_entry(&self) -> &Entry`, `pub fn has_items(&self) -> bool`, `pub fn populate_list(&self, filter: &str)`, `pub fn move_selection(&self, delta: i32)`, `pub fn selected_index(&self) -> Option<usize>`, `pub items: Vec<MoveTargetRow>`
- Consumes: `crate::ui::picker_nav` helpers, `crate::ui::picker_filter::subsequence_match`, `crate::ui::picker_attach::attach_panel`, `crate::app::JournalBand`.

This is a near-clone of `src/ui/journal_picker.rs`. The only differences: rows hold a `band` + a single `label` (rendered with `two_label_row(&label, "")` so it visually matches the other pickers), and the placeholder text reads "Move Q&A to...".

- [ ] **Step 1: Create the file**

Create `src/ui/journal_move_picker.rs`:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Entry, ListBox, ListBoxRow, Overlay};

use crate::app::JournalBand;

/// One selectable target band in the "move Q&A to band" picker.
/// `band` is the destination (Work or Scene(d1,d2)); `label` is its display
/// text ("whole work" / "Act 3, Scene 2" / "Chapter 5").
#[derive(Clone)]
pub struct MoveTargetRow {
    pub band: JournalBand,
    pub label: String,
}

pub struct JournalMovePicker {
    pub overlay: Overlay,
    picker_box: GtkBox,
    search_entry: Entry,
    list_box: ListBox,
    pub items: Vec<MoveTargetRow>,
}

impl JournalMovePicker {
    pub fn new() -> Self {
        let overlay = Overlay::new();
        let picker_box = crate::ui::picker_nav::build_picker_card();

        let search_entry = Entry::builder()
            .placeholder_text("Move Q&A to...")
            .build();

        let (list_box, scrolled) = crate::ui::picker_nav::new_picker_list();

        picker_box.append(&search_entry);
        picker_box.append(&scrolled);

        JournalMovePicker {
            overlay,
            picker_box,
            search_entry,
            list_box,
            items: Vec::new(),
        }
    }

    pub fn attach(&self, base: &impl IsA<gtk4::Widget>) {
        crate::ui::picker_attach::attach_panel(&self.overlay, base, None, &self.picker_box);
    }

    pub fn set_items(&mut self, items: Vec<MoveTargetRow>) {
        self.items = items;
        self.populate_list("");
    }

    pub fn show(&self) {
        self.picker_box.set_visible(true);
        self.search_entry.set_text("");
        self.search_entry.grab_focus();
        self.populate_list("");
    }

    pub fn hide(&self) {
        self.picker_box.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.picker_box.is_visible()
    }

    pub fn search_entry(&self) -> &Entry {
        &self.search_entry
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }

    pub fn populate_list(&self, filter: &str) {
        crate::ui::picker_nav::clear_list(&self.list_box);
        let filter_lower = filter.to_lowercase();

        for (idx, item) in self.items.iter().enumerate() {
            if !filter.is_empty() {
                let target = item.label.to_lowercase();
                if !crate::ui::picker_filter::subsequence_match(&filter_lower, &target) {
                    continue;
                }
            }

            let hbox = crate::ui::picker_nav::two_label_row(&item.label, "");
            let row = ListBoxRow::builder().child(&hbox).build();
            row.set_widget_name(&idx.to_string());
            self.list_box.append(&row);
        }

        crate::ui::picker_nav::select_first_row(&self.list_box);
    }

    pub fn move_selection(&self, delta: i32) {
        crate::ui::picker_nav::move_selection_clamped(&self.list_box, delta);
    }

    /// Index into `items` of the selected row (the row's widget_name).
    pub fn selected_index(&self) -> Option<usize> {
        crate::ui::picker_nav::selected_index(&self.list_box)
    }
}
```

- [ ] **Step 2: Register the module**

In `src/ui/mod.rs`, add next to `pub mod journal_picker;` (line ~18):

```rust
pub mod journal_move_picker;
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build`
Expected: builds (warnings about unused `JournalMovePicker` are fine — it's wired up in later tasks).

- [ ] **Step 4: Commit**

```bash
git add src/ui/journal_move_picker.rs src/ui/mod.rs
git commit -m "feat(journal): JournalMovePicker widget + MoveTargetRow

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 3: Target-list builder + `InputMode` variant

**Files:**
- Modify: `src/app/mod.rs` (add `JournalMovePicker` to the `InputMode` enum after `JournalPicker`, line ~102)
- Modify: `src/input/actions/journal.rs` (add `move_target_rows` near the top, after the `footer_left_text` helper ~line 39; add the `use` for `MoveTargetRow`)

**Interfaces:**
- Produces: `fn move_target_rows(s: &AppState, current: &JournalBand) -> Vec<crate::ui::journal_move_picker::MoveTargetRow>` (private to the module)
- Consumes: `crate::app::scene_synopsis::synopsis_label`, `crate::app::JournalBand`.

The builder lists every band the entry could move to: a whole-work row first, then one row per unique `(div1,div2)` in `work.lines` (reading order), excluding the entry's current band. Labels via `synopsis_label` (handles play scenes, chapters, and "Preface").

- [ ] **Step 1: Add the `InputMode` variant**

In `src/app/mod.rs`, in the `InputMode` enum, add immediately after `JournalPicker,` (line ~102):

```rust
    JournalMovePicker,
```

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/input/actions/journal.rs` (the same block holding `footer_left_scene_shows_abbrev_act_scene`). This test does NOT build an `AppState` (heavy GTK); instead it tests the pure ordering/exclusion logic via a small extracted helper. First, define the pure core helper and test it:

```rust
#[test]
fn target_bands_exclude_current_and_lead_with_work() {
    // Pure core: given the unique (div1,div2) scene keys in reading order and
    // the current band, produce the ordered destination bands (work first,
    // current band omitted). Labels are applied separately by the caller.
    let scenes = vec![(1, 1), (1, 2), (3, 1)];

    // Current = Scene(1,2): work row first, then 1.1 and 3.1 (1.2 omitted).
    let bands = target_bands(&scenes, &JournalBand::Scene(1, 2));
    assert_eq!(
        bands,
        vec![JournalBand::Work, JournalBand::Scene(1, 1), JournalBand::Scene(3, 1)]
    );

    // Current = Work: work row omitted, all scenes listed.
    let bands = target_bands(&scenes, &JournalBand::Work);
    assert_eq!(
        bands,
        vec![JournalBand::Scene(1, 1), JournalBand::Scene(1, 2), JournalBand::Scene(3, 1)]
    );
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --bins target_bands_exclude_current -- --nocapture`
Expected: FAIL — `cannot find function target_bands in this scope`.

- [ ] **Step 4: Write the implementation**

Add to `src/input/actions/journal.rs`. First the import near the top `use` block:

```rust
use crate::ui::journal_move_picker::MoveTargetRow;
```

Then, after `footer_left_text` (~line 39), add the pure core and the row builder:

```rust
/// Pure core of `move_target_rows`: given the work's unique scene keys in
/// reading order and the entry's current band, return the ordered list of
/// destination bands — whole work first, then each scene — with the current
/// band omitted. Labels are applied by `move_target_rows`.
fn target_bands(scenes: &[(i64, i64)], current: &JournalBand) -> Vec<JournalBand> {
    let mut out = Vec::with_capacity(scenes.len() + 1);
    if *current != JournalBand::Work {
        out.push(JournalBand::Work);
    }
    for &(d1, d2) in scenes {
        let band = JournalBand::Scene(d1, d2);
        if band != *current {
            out.push(band);
        }
    }
    out
}

/// Build the list of move targets for the current entry: every band it could be
/// moved to (whole work + every scene/chapter in the work), excluding its
/// current band. Scene keys come from `work.lines` (unique (div1,div2) in
/// reading order — the same source the synopsis picker uses), unfiltered, so
/// every scene is offered even if it has no Q&A yet. Labels via `synopsis_label`.
fn move_target_rows(s: &AppState, current: &JournalBand) -> Vec<MoveTargetRow> {
    let scenes: Vec<(i64, i64)> = match s.current_work.as_ref() {
        Some(work) => {
            let mut seen = std::collections::HashSet::new();
            let mut keys = Vec::new();
            for line in &work.lines {
                let k = (line.div1, line.div2);
                if seen.insert(k) {
                    keys.push(k);
                }
            }
            keys
        }
        None => Vec::new(),
    };

    target_bands(&scenes, current)
        .into_iter()
        .map(|band| {
            let label = match band {
                JournalBand::Work => "whole work".to_string(),
                JournalBand::Scene(d1, d2) => crate::app::scene_synopsis::synopsis_label(s, d1, d2),
                // target_bands never yields Passage; map defensively.
                JournalBand::Passage { div1, div2, .. } => format!("{}.{} passage", div1, div2),
            };
            MoveTargetRow { band, label }
        })
        .collect()
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --bins target_bands_exclude_current -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Build (verify the InputMode variant didn't break exhaustive matches)**

Run: `cargo build`
Expected: may FAIL with non-exhaustive `match` errors on `InputMode` in `keymap.rs` (the new variant). That is EXPECTED and fixed in Task 6. If it fails ONLY with `JournalMovePicker` non-exhaustive-match / unreachable-pattern errors in `keymap.rs`, proceed; any OTHER error must be fixed now.

- [ ] **Step 7: Commit**

```bash
git add src/app/mod.rs src/input/actions/journal.rs
git commit -m "feat(journal): move-target list builder + JournalMovePicker input mode

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 4: Construct, attach, register, and wire the picker filter

**Files:**
- Modify: `src/app/mod.rs` (import; struct field; construct + attach in the overlay chain ~line 1147; field in the struct literal ~line 1476; search-entry filter ~line 1945)
- Modify: `src/input/picker_dispatch.rs` (`impl_picker!` + `picker_for_mode` arm)

**Interfaces:**
- Consumes: `JournalMovePicker::new/attach`, and the `Picker` trait (`move_selection`, `hide`).
- Produces: `AppState.journal_move_picker: JournalMovePicker` (a public field, like `journal_picker`).

- [ ] **Step 1: Import + struct field**

In `src/app/mod.rs`, near the `use crate::ui::journal_picker::JournalQaPicker;` (line ~31) add:

```rust
use crate::ui::journal_move_picker::JournalMovePicker;
```

In the `AppState` struct, after `pub journal_picker: JournalQaPicker,` (line ~321) add:

```rust
    pub journal_move_picker: JournalMovePicker,
```

- [ ] **Step 2: Construct + attach in the overlay chain**

In `src/app/mod.rs`, the overlay chain currently reads (lines ~1144–1152):

```rust
    // Journal picker overlays the journal overlay (above journal, below translation)
    let journal_picker = JournalQaPicker::new();
    journal_picker.attach(&journal_overlay.overlay);
    journal_picker.overlay.set_vexpand(true);

    // Translation overlay wraps the journal picker overlay
    let translation_overlay = crate::ui::translation_overlay::TranslationOverlay::new();
    translation_overlay.attach(&journal_picker.overlay);
    translation_overlay.overlay.set_vexpand(true);
```

Insert the move picker BETWEEN the journal picker and the translation overlay, and re-point the translation overlay at it:

```rust
    // Journal picker overlays the journal overlay (above journal, below translation)
    let journal_picker = JournalQaPicker::new();
    journal_picker.attach(&journal_overlay.overlay);
    journal_picker.overlay.set_vexpand(true);

    // Journal move-to-band picker overlays the journal Q&A picker
    let journal_move_picker = JournalMovePicker::new();
    journal_move_picker.attach(&journal_picker.overlay);
    journal_move_picker.overlay.set_vexpand(true);

    // Translation overlay wraps the journal move picker overlay
    let translation_overlay = crate::ui::translation_overlay::TranslationOverlay::new();
    translation_overlay.attach(&journal_move_picker.overlay);
    translation_overlay.overlay.set_vexpand(true);
```

- [ ] **Step 3: Add the field to the struct literal**

In `src/app/mod.rs`, in the `AppState { ... }` construction literal, after `journal_picker,` (line ~1476) add:

```rust
        journal_move_picker,
```

- [ ] **Step 4: Wire the search-entry filter**

In `src/app/mod.rs`, after the journal Q&A picker filter block (lines ~1937–1945), add:

```rust
    // Connect journal move-picker search entry filter
    let state_for_journal_move_filter = Rc::clone(&state);
    {
        let s = state.borrow();
        s.journal_move_picker.search_entry().connect_changed(move |entry| {
            let filter = entry.text().to_string();
            state_for_journal_move_filter.borrow().journal_move_picker.populate_list(&filter);
        });
    }
```

- [ ] **Step 5: Register in picker_dispatch**

In `src/input/picker_dispatch.rs`, after `impl_picker!(crate::ui::journal_picker::JournalQaPicker);` (line ~32) add:

```rust
impl_picker!(crate::ui::journal_move_picker::JournalMovePicker);
```

And in `picker_for_mode`, after the `InputMode::JournalPicker => Some(&s.journal_picker),` arm (line ~47) add:

```rust
        InputMode::JournalMovePicker => Some(&s.journal_move_picker),
```

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: same EXPECTED non-exhaustive-match failures in `keymap.rs` for `JournalMovePicker` (fixed in Task 6), but NO errors in `app/mod.rs` or `picker_dispatch.rs`. If `app/mod.rs`/`picker_dispatch.rs` are clean, proceed.

- [ ] **Step 7: Commit**

```bash
git add src/app/mod.rs src/input/picker_dispatch.rs
git commit -m "feat(journal): construct/attach/register JournalMovePicker

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 5: Action handlers — `open_move_picker`, `confirm_move_picker`

**Files:**
- Modify: `src/input/actions/journal.rs` (add the two handlers; reuse `render_current`, `move_target_rows`)

**Interfaces:**
- Consumes: `crate::db::journal::move_journal_page`, `move_target_rows`, `render_current`, `crate::ui::toast::show_transient`, `crate::db::queries::open_db_rw`, `crate::app::JOURNAL_WORK_DIV`.
- Produces: `pub(crate) fn open_move_picker(state: &Rc<RefCell<AppState>>)`, `pub(crate) fn confirm_move_picker(state: &Rc<RefCell<AppState>>)`.

These are GUI-driven (no pure unit test); correctness is verified by `cargo build` + the user's rendered run. Mirror `open_picker`/`confirm_picker` patterns exactly (already in this file).

- [ ] **Step 1: Add `open_move_picker`**

Add to `src/input/actions/journal.rs` (after `confirm_picker`, ~line 547):

```rust
/// Open the "move this Q&A to another band" picker over the journal overlay.
/// Lists every band the current entry could move to (whole work + every
/// scene/chapter), excluding its current band. No-op with a toast if there is no
/// current page, or if the current band is a passage (passages are
/// citation-anchored and not movable).
pub(crate) fn open_move_picker(state: &Rc<RefCell<AppState>>) {
    let mut s = state.borrow_mut();
    if s.journal.pages.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No page to move", 2);
        return;
    }
    if matches!(s.journal_band, JournalBand::Passage { .. }) {
        crate::ui::toast::show_transient(&s.chapter_toast, "Can't move a passage page", 2);
        return;
    }
    let rows = move_target_rows(&s, &s.journal_band.clone());
    if rows.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No other band to move to", 2);
        return;
    }
    s.journal_move_picker.set_items(rows);
    s.journal_move_picker.show();
    s.input_mode = InputMode::JournalMovePicker;
}
```

- [ ] **Step 2: Add `confirm_move_picker`**

Add immediately after `open_move_picker`:

```rust
/// Confirm the move-picker selection: re-target the current entry to the chosen
/// band in lit.db, then follow it — switch the overlay to the destination band
/// and land on the moved entry (matched by id). Hides the picker and returns to
/// the journal overlay.
pub(crate) fn confirm_move_picker(state: &Rc<RefCell<AppState>>) {
    let selected = state.borrow().journal_move_picker.selected_index();
    let mut s = state.borrow_mut();
    s.journal_move_picker.hide();
    s.input_mode = InputMode::JournalOverlay;

    let Some(idx) = selected else {
        render_current(&mut s);
        return;
    };

    // The destination band + label, and the current entry's id.
    let (dest_band, label) = {
        let row = &s.journal_move_picker.items[idx];
        (row.band.clone(), row.label.clone())
    };
    let Some(entry_id) = s.journal.pages.get(s.journal.page_index).map(|p| p.id) else {
        render_current(&mut s);
        return;
    };

    // Map the destination band to (scope, div1, div2).
    let (scope, d1, d2) = match &dest_band {
        JournalBand::Work => ("work", crate::app::JOURNAL_WORK_DIV.0, crate::app::JOURNAL_WORK_DIV.1),
        JournalBand::Scene(a, b) => ("scene", *a, *b),
        // open_move_picker excludes the passage band from targets; unreachable
        // in practice, but re-render-and-bail defensively rather than panic.
        JournalBand::Passage { .. } => {
            render_current(&mut s);
            return;
        }
    };

    if let Ok(conn) = crate::db::queries::open_db_rw() {
        if let Err(e) = crate::db::journal::move_journal_page(&conn, entry_id, scope, d1, d2) {
            crate::logging::log(&format!("JOURNAL: move failed: {}", e));
            render_current(&mut s);
            return;
        }
    }

    // Follow the entry: switch to the destination band and land on it.
    s.journal_band = dest_band;
    s.journal.page_index = 0;
    render_current(&mut s); // loads the destination band's pages
    if let Some(pos) = s.journal.pages.iter().position(|p| p.id == entry_id) {
        s.journal.page_index = pos;
        render_current(&mut s);
    }
    crate::ui::toast::show_transient(&s.chapter_toast, &format!("Moved to {}", label), 2);
    crate::logging::log("JOURNAL: moved page to new band");
}
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: still only the EXPECTED `keymap.rs` non-exhaustive `JournalMovePicker` failures (Task 6 fixes them). `actions/journal.rs` itself must be clean.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "feat(journal): open/confirm move-picker handlers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 6: Keymap wiring — Ctrl+Shift+J + picker dispatch

**Files:**
- Modify: `src/input/keymap.rs` (the picker-key mode group ~line 110; the `Hide` arm ~line 288; the `Confirm` arm ~line 427; the `is_ctrl` block of `handle_journal_key` ~line 730)

**Interfaces:**
- Consumes: `open_move_picker`, `confirm_move_picker`.

`Ctrl+Shift+J` arrives as `key_name = "J"` (uppercase) with `is_ctrl = true`. The existing close-overlay arm matches lowercase `"j"`, so there is no collision.

- [ ] **Step 1: Add the open bind in `handle_journal_key`**

In `src/input/keymap.rs`, inside the `if is_ctrl { match key_name { ... } }` block of `handle_journal_key` (the arms `"n"`, `"p"`, `"j"`, `"backslash"`, `"g"`, ~lines 730–757), add a new arm (place it after the `"j"` close-overlay arm, ~line 743):

```rust
            // Ctrl+Shift+J: open the "move this Q&A to another band" picker.
            // Arrives as key_name "J" (shifted), distinct from Ctrl+j (close).
            "J" => {
                crate::input::actions::journal::open_move_picker(state);
                return true;
            }
```

- [ ] **Step 2: Add the mode to the picker-key dispatch group**

In `src/input/keymap.rs`, in the big `match mode` at line ~100, add `JournalMovePicker` to the group that routes to `handle_picker_key` (after `| crate::app::InputMode::JournalPicker`, line ~110):

```rust
            | crate::app::InputMode::JournalMovePicker
```

- [ ] **Step 3: Add the Hide arm**

In `handle_picker_key`'s `PickerAction::Hide` match (line ~288), after the `InputMode::JournalPicker => { ... }` arm add:

```rust
                InputMode::JournalMovePicker => { s.journal_move_picker.hide(); s.input_mode = InputMode::JournalOverlay; }
```

- [ ] **Step 4: Add the Confirm arm**

In `handle_picker_key`'s `PickerAction::Confirm` match (line ~427), after the `InputMode::JournalPicker => { ... confirm_picker ... }` arm add:

```rust
                InputMode::JournalMovePicker => {
                    crate::input::actions::journal::confirm_move_picker(state);
                    true
                }
```

- [ ] **Step 5: Build the whole crate (now exhaustive)**

Run: `cargo build`
Expected: PASS, no errors. The new `InputMode` variant is now handled everywhere it must be.

- [ ] **Step 6: Run the full pure-logic suite**

Run: `cargo test --bins`
Expected: PASS (the pre-existing count plus the 2 new tests from Tasks 1 and 3).

- [ ] **Step 7: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(journal): bind Ctrl+Shift+J to open move-picker; wire picker dispatch

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 7: Document the bind in the Ctrl+/ overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs` (the `j`/`J` key's overlay-context bind list at line ~86 and its `describe()` arms ~line 355)

Per the project rule, every keybind change updates the Ctrl+/ overlay. The overlay's `describe()` panel already documents journal-overlay-internal binds keyed to the `j`/`J` physical key (e.g. `journal tog`, `view jrnl`). Add the move bind there.

- [ ] **Step 1: Add the overlay-context label to the `j`/`J` key**

In `src/ui/keybinds_overlay.rs`, line ~86, the `key(...)` for `j`/`J` has a trailing slice of overlay-context binds:

```rust
    key("j", "J", "cursor ↓", "J: next speaker", &[("C-j", "journal tog"), ("J", "jrnl from gloss"), ("C-j", "view jrnl")]),
```

Add a `("C-S-j", "move jrnl band")` entry to that slice:

```rust
    key("j", "J", "cursor ↓", "J: next speaker", &[("C-j", "journal tog"), ("J", "jrnl from gloss"), ("C-j", "view jrnl"), ("C-S-j", "move jrnl band")]),
```

- [ ] **Step 2: Add the `describe()` arm**

In `src/ui/keybinds_overlay.rs`, in the `describe()` match (near the other journal arms, ~line 392–399), add an arm for the new label:

```rust
        "move jrnl band" => "While the journal overlay is open (Ctrl+Shift+J): move the \
current Q&A page to a different band. Opens a picker listing every scene/chapter in \
the work plus a \u{201c}whole work\u{201d} row (the current band omitted); Enter re-targets \
the entry\u{2019}s scope + (div1,div2) in lit.db and follows it to its new band. Passage \
pages can\u{2019}t be moved. \
-> journal::open_move_picker / confirm_move_picker \u{2014} src/input/actions/journal.rs",
```

- [ ] **Step 3: Cross-reference pass with the skill**

Invoke the `update-cairo-keybinds-overlay` skill and run its mandatory three-pass exhaustive cross-reference for the `j`/`J` key:
1. No blank detail slot hides a real binding.
2. No label names the wrong action.
3. Every label (including `"move jrnl band"`) has a `describe()` arm.

Fix anything the skill surfaces.

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(keybinds): document Ctrl+Shift+J move-Q&A-to-band in Ctrl+/ overlay

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

---

## Task 8: Final verification + user runtime check

**Files:** none (verification only)

- [ ] **Step 1: Full build + tests + clippy**

Run:
```bash
cargo build && cargo test --bins && cargo clippy
```
Expected: build clean, all `--bins` tests pass, no new clippy warnings on the changed files.

- [ ] **Step 2: Update `ac`**

Update `CLAUDE-activeContext.md`: record the feature (Ctrl+Shift+J move-Q&A-to-band), the new files, that logic tests pass, and that the rendered run is pending user verification. Commit it.

```bash
git add CLAUDE-activeContext.md
git commit -m "docs(ac): journal move-to-band feature; runtime verification pending

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Y2fVY74VaQBkezv9cgAdY6"
```

- [ ] **Step 3: Ask the user to verify on a rendered run**

The acceptance criterion is visual/runtime ("the picker lists the right scenes; Enter moves the entry; the overlay follows it"). Per the no-`cargo run` rule, ask the user to launch the app, open a work's journal (Ctrl+j), put a Q&A on a scene, press `Ctrl+Shift+J`, pick a different scene or "whole work", and confirm the entry moves and the overlay follows. Report the result; if it fails, debug from the dev log.

---

## Self-Review

**Spec coverage:**
- DB band re-target → Task 1 (`move_journal_page`). ✓
- Picker overlay (reused pattern) → Task 2 (`JournalMovePicker`). ✓
- Target list = every scene + whole work, current omitted → Task 3 (`move_target_rows`/`target_bands`). ✓
- Ctrl+Shift+J open → Task 6. ✓
- Follow-the-entry after move → Task 5 (`confirm_move_picker`). ✓
- Passage entries not movable / not offered → guard in Task 5; `target_bands` only emits Work/Scene (Task 3). ✓
- Keybinds overlay updated → Task 7. ✓
- Unit tests (DB round-trip, target ordering/exclusion) → Tasks 1 and 3. ✓
- Runtime user verification → Task 8. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; every command has expected output. ✓

**Type consistency:**
- `move_journal_page(conn, id, scope: &str, div1, div2)` — defined Task 1, called Task 5. ✓
- `MoveTargetRow { band, label }` — defined Task 2, used Tasks 3 and 5. ✓
- `JournalMovePicker` methods (`set_items`/`show`/`hide`/`selected_index`/`items`/`search_entry`/`populate_list`/`move_selection`) — defined Task 2, used Tasks 4, 5. ✓
- `target_bands(&[(i64,i64)], &JournalBand) -> Vec<JournalBand>` — defined + tested Task 3. ✓
- `InputMode::JournalMovePicker` — added Task 3, matched in Tasks 4 and 6. ✓
- `crate::app::JOURNAL_WORK_DIV` (a `(i64,i64)`) — used Task 5, exists at `src/app/mod.rs:3803`. ✓

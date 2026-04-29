# Async Media Picker DB Write Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the synchronous database write in the media picker's `"p"` key handler off the GTK main thread into an async task, eliminating a potential UI freeze.

**Architecture:** Extract the DB work into `actions::pickers::set_media_default()`, following the established async pattern used by `delete_bookmark` and `open_bookmark_picker`. The key handler in `keymap.rs` becomes a single call to the new verb.

**Tech Stack:** Rust, GTK4, tokio, rusqlite, glib

---

### Task 1: Extract set_media_default verb

**Files:**
- Modify: `src/input/actions/pickers.rs` (add `set_media_default` function)
- Modify: `src/input/keymap.rs:270-318` (replace inline DB code with verb call)

- [ ] **Step 1: Add `set_media_default` to `pickers.rs`**

In `src/input/actions/pickers.rs`, add after the `delete_bookmark` function (after line 311):

```rust
/// Set the selected media as the default (highest priority) for the current
/// work. Spawns an async task to write to the DB, then updates the picker
/// widget on completion. Called from the media picker's `p` key.
pub(crate) fn set_media_default(
    state: &Rc<RefCell<AppState>>,
    tokio_handle: &tokio::runtime::Handle,
) {
    let selected_id = state.borrow().media_picker.selected_media_id();
    let abbrev = state
        .borrow()
        .current_work
        .as_ref()
        .map(|w| w.abbrev.clone());
    if let (Some(media_id), Some(abbrev)) = (selected_id, abbrev) {
        let state_clone = Rc::clone(state);
        let handle = tokio_handle.clone();
        glib::spawn_future_local(async move {
            let result = handle
                .spawn_blocking(move || {
                    let conn = crate::db::queries::open_db_rw()?;
                    crate::db::queries::set_media_priority(&conn, &abbrev, media_id)?;
                    let max_pri: i64 = conn
                        .query_row(
                            "SELECT priority FROM work_media_associations \
                             WHERE work_abbrev = ?1 AND media_id = ?2",
                            rusqlite::params![&abbrev, media_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(20);
                    crate::logging::log(&format!(
                        "MEDIA: set default media_id={} for {} (pri={})",
                        media_id, abbrev, max_pri
                    ));
                    Ok::<_, rusqlite::Error>((media_id, max_pri))
                })
                .await;
            match result {
                Ok(Ok((media_id, max_pri))) => {
                    state_clone
                        .borrow_mut()
                        .media_picker
                        .set_default(media_id, max_pri);
                }
                Ok(Err(e)) => {
                    crate::logging::log(&format!(
                        "MEDIA: set_media_default DB error: {}",
                        e
                    ));
                }
                Err(e) => {
                    crate::logging::log(&format!(
                        "MEDIA: set_media_default join error: {}",
                        e
                    ));
                }
            }
        });
    }
}
```

- [ ] **Step 2: Replace the inline DB code in keymap.rs**

In `src/input/keymap.rs`, find the media picker's `"p"` arm (lines 270-318). Replace the entire `"p" =>` arm with:

```rust
            "p" => {
                let is_search_focused = state.borrow().media_picker.search_entry().has_focus();
                if !is_search_focused {
                    crate::input::actions::pickers::set_media_default(state, tokio_handle);
                    return true;
                }
            }
```

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/pickers.rs src/input/keymap.rs
git commit -m "Move media picker DB write to async task

The media picker's 'p' key handler previously opened a read-write
database connection synchronously on the GTK main thread. Extracted
into actions::pickers::set_media_default which spawns the DB work
via tokio spawn_blocking, matching the pattern of delete_bookmark
and open_bookmark_picker. Eliminates potential UI freeze."
```

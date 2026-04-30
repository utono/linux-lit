# Bookmarks / Annotations Review vs Reference Codebases

**Date:** 2026-04-29
**Linux-lit files reviewed:** `src/db/queries.rs:428-522` (95 lines), `src/input/actions/bookmarks.rs` (95 lines), `src/ui/bookmark_picker.rs` (247 lines), `src/db/models.rs:86-91` (6 lines)
**References consulted:** `foliate/src/annotations.js` (722 lines), `lue/lue/progress_manager.py` (232 lines)

## Summary

Linux-lit's bookmark system is a simple toggle-on-line model (one table, keyed by `work_abbrev + line_mapping_id`, no metadata beyond `created_at`). Foliate's is a two-tier system: lightweight bookmarks (CFI value + label) plus rich annotations (CFI + color + text + note + timestamps), each with its own GObject model, list view, and export pipeline. The headline alignment win is separating the bookmark *model* from the *persistence layer* so the foliate pattern of "model manages sorted insert / delete / export; persistence is a separate concern" translates directly — enabling future annotation features to drop in without restructuring.

## Findings

### F1. Bookmark model lives in the DB layer, not a domain object [pattern-alignment]

**Reference shape:** `foliate/src/annotations.js:10-13` — `Bookmark` is a GObject data class with `value` (CFI string) and `label` (display text). `BookmarkModel` (lines 88-111) is a `Gio.ListStore` with `add`, `delete`, `export` methods. The model owns the sorted-insert invariant and the in-memory collection; persistence is handled externally by the book-viewer.

**Linux-lit shape:** `src/db/queries.rs:453-475` — `toggle_bookmark` is a DB function that does both the in-memory decision (exists? → delete : insert) and the SQLite write in one call. `src/app.rs:104` — `is_bookmarked: Rc<RefCell<Vec<bool>>>` is a parallel boolean array synced manually after every toggle. There's no domain model — the DB *is* the model.

**Refactor toward reference:** Extract a `BookmarkSet` struct (analogous to `BookmarkModel`) that holds the in-memory sorted list of bookmarked `line_mapping_id`s, with `add(id)`, `remove(id)`, `contains(id)`, `toggle(id) -> bool`, `export() -> Vec<i64>` methods. `AppState` replaces `is_bookmarked: Vec<bool>` with `bookmarks: BookmarkSet`. The `toggle_bookmark` action verb calls `bookmarks.toggle()` for the in-memory change, then spawns the DB write separately. The `is_bookmarked` boolean array is derived from `BookmarkSet` on demand or cached with invalidation.

**Leverage unlocked:** Foliate's `BookmarkModel` pattern translates directly: `add` → `add`, `delete` → `remove`, `export` → `export`. Future features (bookmark notes, bookmark colors, cross-work bookmark lists) extend `BookmarkSet` without touching the DB layer or the action verbs.

**Risk if ignored:** Every new bookmark feature requires threading through both the DB function and the manual `is_bookmarked` vec sync — two places to forget, two places that can diverge.

**Effort:** M

---

### F2. No undo for bookmark deletion [missing-edge-case]

**Reference shape:** `foliate/src/annotations.js:136-141` — when a bookmark is deleted, foliate shows an `Adw.Toast` with an "Undo" button. The undo callback calls `model.add(row.value, row.label)` to re-insert. Same pattern at line 171-175 for bulk delete. Undo is trivial because the model has `add` — re-inserting a deleted bookmark is just another `add` call.

**Linux-lit shape:** `src/input/actions/pickers.rs:261-311` — `delete_bookmark` spawns a DB delete and removes the row from the picker widget. No undo path. The deleted bookmark's `line_mapping_id` and `created_at` are lost after the DB write.

**Refactor toward reference:** With `BookmarkSet` from F1, undo becomes: (1) capture the removed id before `remove()`, (2) on undo, call `add(id)` + spawn a DB insert. Show a transient status label or use the existing `word_status_label` for "Bookmark deleted — press z to undo" with a 3-second timeout. Mirrors foliate's Toast pattern adapted to linux-lit's keyboard-driven UI.

**Leverage unlocked:** Foliate's undo-via-Toast pattern maps to linux-lit's undo-via-status-label. The `BookmarkSet.add()` method makes undo a one-liner.

**Risk if ignored:** Accidental bookmark deletion is permanent. In foliate this is the most common undo action.

**Effort:** S (given F1 is done first)

---

### F3. Bookmarks have no label/note metadata [schema-gap]

**Reference shape:** `foliate/src/annotations.js:10-13` — `Bookmark` has `value` (location) and `label` (display text, typically the TOC section name). `Annotation` (lines 15-22) extends this with `color`, `text` (highlighted text), `note` (user-written note), `created`, `modified`.

**Linux-lit shape:** `src/db/queries.rs:431-438` — bookmarks table has `work_abbrev`, `line_mapping_id`, `created_at`. No label, no note, no color. The picker shows the line's `canonical_text` from `line_mapping` as the display text, not a user-provided label.

**Refactor toward reference:** Add an optional `note TEXT` column to the bookmarks table. The `BookmarkItem` model gains a `note: Option<String>` field. The bookmark picker shows the note (if any) below the line text, matching foliate's annotation row layout (text + note). No color system needed — linux-lit's bookmarks are line-anchored, not range-anchored, so the highlight/underline/strikethrough annotation types don't translate.

**Leverage unlocked:** Foliate's `Annotation` model's `note` field maps directly. Future "annotate this line" feature drops into the existing note column without schema migration.

**Risk if ignored:** Users who want to annotate why they bookmarked a line have no mechanism. The bookmark picker shows only raw line text with no context.

**Effort:** S

---

### F4. Bookmark sort order is creation-time only [pattern-alignment]

**Reference shape:** `foliate/src/annotations.js:91-99` — `BookmarkModel.add()` inserts in CFI sort order (document position), using `CFI.compare`. The model is always sorted by location, not by creation time. This means the bookmark list mirrors the reading order.

**Linux-lit shape:** `src/db/queries.rs:494-499` — `load_bookmarks_with_details` orders by `b.created_at DESC`. The picker shows most-recent-first. `next_bookmark`/`prev_bookmark` in `navigation.rs:915-946` iterate the `is_bookmarked` array by buffer position (document order). So navigation is position-ordered but the picker is time-ordered — two different sort orders for the same data.

**Refactor toward reference:** Add a sort toggle to the bookmark picker: document order (default, matching foliate) and creation order. Or simply default to document order in the picker, matching the navigation order. With `BookmarkSet` from F1, the sorted-by-position invariant is maintained by the model, and the picker iterates it directly.

**Leverage unlocked:** Foliate's `BookmarkModel` is always position-sorted — linux-lit's `BookmarkSet` matches, making the picker and navigation code use the same iteration order.

**Risk if ignored:** Users see bookmarks in a different order in the picker vs when pressing `;`/`:` to navigate — mild but confusing.

**Effort:** S

---

### F5. No bookmark export [missing-edge-case]

**Reference shape:** `foliate/src/annotations.js:632-722` — `exportAnnotations` supports JSON, HTML, Markdown, and Org-mode export. Each format has a dedicated formatter. `BookmarkModel.export()` (line 108-110) serializes to a value array. Import is also supported (lines 589-630).

**Linux-lit shape:** No export mechanism. Bookmarks exist only in `lit.db`.

**Refactor toward reference:** Add a `BookmarkSet.export() -> Vec<BookmarkExport>` method (where `BookmarkExport` includes `line_mapping_id`, `canonical_text`, `citation`, `note`). Wire it to a keybind or menu action that writes JSON to a file. HTML/Markdown formatters can follow later. Mirrors foliate's `export()` → formatter pipeline.

**Leverage unlocked:** Foliate's export pipeline shape (model.export → format → file) translates directly. Each new format is a formatter function, not a DB query.

**Risk if ignored:** Users cannot share or back up bookmarks outside of lit.db.

**Effort:** M

---

### F6. Bookmark toggle is async but could be sync [pattern-alignment]

**Reference shape:** `foliate/src/annotations.js:165-178` — `BookmarkView.toggle()` is synchronous: `model.delete(value)` or `model.add(value, label)`. No async, no DB round-trip in the toggle path. Persistence is a separate concern.

**Linux-lit shape:** `src/input/actions/bookmarks.rs:11-53` — `toggle_bookmark` spawns a `tokio::spawn_blocking` for the DB write, then updates `is_bookmarked` in the completion callback. The gutter redraw happens asynchronously. This means there's a brief moment where the bookmark state is out of sync (toggled in DB but not yet reflected in `is_bookmarked`).

**Refactor toward reference:** With `BookmarkSet` from F1, the toggle becomes synchronous on the GTK thread (flip the in-memory set), with the DB write spawned fire-and-forget afterward. The gutter redraws immediately from the in-memory state. Mirrors foliate's sync-model + async-persistence split.

**Leverage unlocked:** Foliate's synchronous model operations translate directly. The brief out-of-sync window between DB write and UI update disappears.

**Risk if ignored:** Rapid toggle (press `m` twice quickly) could race — the second toggle reads stale `is_bookmarked` state before the first toggle's completion callback fires.

**Effort:** S (given F1 is done first)

## Out of scope

- **Annotation colors/highlights** — foliate's rich annotation system (underline, squiggly, strikethrough, named colors) is range-based and WebView-rendered. linux-lit's line-based model doesn't support text ranges. Noted for a future selection-tools review.
- **Import annotations** — foliate supports importing from JSON files. Useful but lower priority than export.
- **Bookmark-in-view detection** — foliate's `BookmarkView.update()` tracks which bookmarks are in the current viewport. linux-lit's `is_bookmarked` array + gutter renderer already provides this visually. Not a structural gap.
- **Progress persistence** — lue's `progress_manager.py` is about last-read-position (chapter/paragraph/sentence), not bookmarks. linux-lit handles this via `save_position`/`restore_position` in `app.rs`. Different subsystem.

## Suggested next step

Implement F1 (BookmarkSet domain model) first — it's the foundation that F2, F4, and F6 depend on. F3 (note column) is independent and can be done in parallel. F5 (export) is lower priority and depends on F1.

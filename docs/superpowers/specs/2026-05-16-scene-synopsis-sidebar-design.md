# Scene Synopsis Sidebar

Show a scene synopsis in the right sidebar area (same position as vocab popup) when the cursor enters a new scene. Toggle between vocab and synopsis with H.

## Database Schema

New table in `~/utono/litdb/data/lit.db`:

```sql
CREATE TABLE scene_synopses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_abbrev TEXT NOT NULL,
    div1 INTEGER NOT NULL,
    div2 INTEGER NOT NULL,
    synopsis TEXT NOT NULL,
    UNIQUE(work_abbrev, div1, div2)
);
```

Query: `SELECT synopsis FROM scene_synopses WHERE work_abbrev = ? AND div1 = ? AND div2 = ?`

## Bulk Data

A SQL script `scripts/insert_synopses.sql` populates synopses for all 37 Shakespeare plays in lit.db (~756 scenes total). Synopses are beginner-friendly and complete — typically 3-6 sentences depending on scene complexity.

Shakespeare plays to cover (primary editions only, not -Amb/-BBC variants):

1H4, 1H6, 2H4, 2H6, 3H6, AWW, AYL, Ado, Ant, Cor, Cym, Err, H5, H8, Ham, JC, Jn, LLL, Lr, MM, MND, MV, Mac, Oth, Per, R2, R3, Rom, Shr, TGV, TN, TNK, Tim, Tit, Tmp, Tro, WT, Wiv

The -Amb and -BBC variants share the same div1/div2 structure as their parent work. They can reuse the same synopses via the same (div1, div2) lookup against the base work_abbrev, or have their own rows inserted. Decision: insert rows for the base abbreviation only. The app maps variant abbreviations to base when querying (strip `-Amb`, `-BBC`, `-Ep-N` suffixes).

## App State Changes

In `src/app.rs` AppState:

- `sidebar_mode: SidebarMode` — enum `Vocab | Synopsis`, default `Vocab`
- `synopsis_cache: HashMap<(i64, i64), String>` — populated on `display_work`, keyed by (div1, div2)
- `synopsis_visible: bool` — whether the sidebar is currently showing synopsis content

## Sidebar Mode Enum

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum SidebarMode {
    Vocab,
    Synopsis,
}
```

## Runtime Flow

### On display_work

After loading the work, query all synopses for this work_abbrev (or its base abbreviation) and store in `synopsis_cache`. If the query returns no rows, the cache is empty and synopsis features are inert for this work.

### On cursor movement

In the existing `auto_show_vocab_popup` path (or adjacent), after updating the highlight:

1. Determine current line's (div1, div2) from `state.work.lines[current_line]`
2. Detect "first line of scene": current line's div2 differs from previous line's div2, or div1 differs, or current_line == 0
3. If first-line-of-scene AND synopsis exists in cache AND sidebar_mode is not already Synopsis:
   - Set `sidebar_mode = Synopsis`
   - Set `synopsis_visible = true`
   - Render synopsis in the popup widget

### H keybind

- If `synopsis_cache` is empty for current work: no-op
- If `sidebar_mode == Synopsis` and `synopsis_visible`:
  - Set `sidebar_mode = Vocab`
  - Set `synopsis_visible = false`
  - Re-render vocab popup for current line (or hide if no vocab on this line)
- Else (sidebar_mode == Vocab or synopsis not visible):
  - Look up current line's (div1, div2) in `synopsis_cache`
  - If found: set `sidebar_mode = Synopsis`, `synopsis_visible = true`, render synopsis
  - If not found: no-op

## VocabPopup Extension

Add method to existing `VocabPopup`:

```rust
pub fn update_synopsis(&self, scene_label: &str, synopsis: &str) {
    // Clear content_box
    // Set header to scene_label (e.g., "Act 1, Scene 1")
    // Set counter_label invisible
    // Add wrapped Label with synopsis text
    // Hide footer
}
```

This reuses the same container, positioning, margins, and CSS classes. The synopsis label uses `definition-text` class for consistent styling with word wrapping enabled.

## Keybind Configuration

In `src/input/keymap_config.rs`, add default binding:

```json
{"key": "H", "shift": true, "action": "ToggleSynopsis"}
```

New action variant in `src/input/actions/mod.rs`:

```rust
ToggleSynopsis
```

Handler in keymap dispatch: calls the toggle logic described above.

## DB Query Function

In `src/db/queries.rs`, add:

```rust
pub fn load_synopses(conn: &Connection, work_abbrev: &str) -> HashMap<(i64, i64), String> {
    // SELECT div1, div2, synopsis FROM scene_synopses WHERE work_abbrev = ?
    // Returns HashMap keyed by (div1, div2)
}
```

## Base Abbreviation Mapping

For variant works like `Ham-Amb` or `MND-BBC`, strip the suffix to get the base abbreviation for synopsis lookup:

```rust
fn base_work_abbrev(abbrev: &str) -> &str {
    // Strip -Amb, -BBC, -Ep-N suffixes
    if let Some(pos) = abbrev.find('-') {
        &abbrev[..pos]
    } else {
        abbrev
    }
}
```

Exception: works like `1H4`, `2H4` where the hyphen is not a variant suffix. These don't contain `-` so they pass through unchanged. Works like `3H6` similarly have no hyphen.

Wait — `1H4` has no hyphen. But `Mac-Ep-1` does. The rule: strip from first `-` onward. `Mac-Ep-1` → `Mac`. `Err-Amb` → `Err`. `MND-BBC` → `MND`. Works without hyphens pass through. This covers all variants in the database.

## First-Line Detection Logic

```rust
fn is_first_line_of_scene(state: &AppState) -> bool {
    let current = state.current_line;
    if current == 0 {
        return true;
    }
    let lines = &state.work.lines;
    if current >= lines.len() {
        return false;
    }
    let cur = &lines[current];
    let prev = &lines[current - 1];
    cur.div1 != prev.div1 || cur.div2 != prev.div2
}
```

Note: this fires on the first line of the scene content (the line after the "ACT X, SCENE Y" marker), which is what the user lands on after scene navigation. The marker line itself also triggers it since div2 changes there too.

## Files Modified

- `src/app.rs` — add `sidebar_mode`, `synopsis_cache`, `synopsis_visible` to AppState; load synopses in `display_work`; synopsis rendering path
- `src/ui/vocab_popup.rs` — add `update_synopsis()` method
- `src/input/actions/mod.rs` — add `ToggleSynopsis` variant
- `src/input/keymap_config.rs` — add H default binding
- `src/input/keymap.rs` — route `ToggleSynopsis` to handler
- `src/db/queries.rs` — add `load_synopses()` function
- `src/input/highlight.rs` — add first-line-of-scene check in cursor movement path

## Files Created

- `scripts/insert_synopses.sql` — bulk INSERT for ~756 Shakespeare scene synopses

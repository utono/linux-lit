# Play Page Citation Label

## Problem

The page-label overlay at the bottom of the reader currently shows `line_mapping.id` — a raw integer — for every work, including plays. For plays, the traditional citation (act.scene.line) is far more useful as a "page number" than an opaque DB id.

## Goal

When the currently displayed work is a play (`work.work_type == "play"`), render the page label as a traditional act/scene/line citation in roman-numeral form, e.g. `I.i.15`. All other work types remain unchanged (raw `line_mapping.id`).

## Data

Each `LineRow` already carries a `citation: String` built in `src/db/queries.rs:61` as `"{abbrev}.{div1}.{div2}.{line_in_div}"` — e.g. `Tro.1.1.15`.

Plays always have four dot-separated components: `abbrev`, `act`, `scene`, `line`.

## Formatting Rules

Input: `Tro.1.1.15` → Output: `I.i.15`

- Drop the work abbreviation (component 0).
- Act (component 1) → uppercase roman numeral.
- Scene (component 2) → lowercase roman numeral.
- Line (component 3) → arabic numeral, unchanged.
- Expected range: acts 1–5, scenes 1–20, lines 1–999. Roman conversion must cover at least 1..=99 correctly.

## Parsing Strategy (Strict)

Exactly four dot-separated components, components 1 and 2 parseable as `u32` in range `1..=3999` (roman conversion limit). If any condition fails, fall back to the raw `citation` string unchanged. No panics.

## Implementation

### New module: `src/ui/page_label.rs`

Two pure functions, fully unit-tested:

```rust
/// Convert an arabic number to a roman numeral. Supports 1..=3999.
/// Returns None if out of range.
pub fn to_roman(n: u32) -> Option<String>;

/// Format a play line's citation as "I.i.15".
/// Returns None if the citation is not a strict 4-component play citation.
pub fn format_play_citation(citation: &str) -> Option<String>;
```

`format_play_citation` uppercases the act roman and lowercases the scene roman.

### AppState helper

Add a method on `AppState` in `src/app.rs`:

```rust
/// Text to display in the page-label overlay for the given buffer line.
/// Plays → formatted citation (e.g. "I.i.15"); other works → line_mapping.id.
pub fn page_label_text_for_buffer(&self, buffer_line: usize) -> Option<String>;
```

Logic:
1. Look up the `LineRow` for `buffer_line` (same path used by `line_mapping_id_for_buffer`).
2. If `current_work.work_type == "play"`, try `format_play_citation(&line.citation)`; on `None`, fall back to the raw `line.citation`.
3. Otherwise, return `format!("{}", line.id)` (the existing `line_mapping.id` behavior).

### Call-site updates

Replace all three existing call sites that format `lm_id`:

- `src/app.rs:1216-1219`
- `src/input/navigation.rs:642-645`
- `src/input/navigation.rs:1025-1028`

Each becomes:

```rust
if let Some(text) = state.page_label_text_for_buffer(buffer_line) {
    state.page_line_label.set_text(&text);
    state.page_line_label.set_visible(true);
}
```

## Testing

Unit tests in `src/ui/page_label.rs`:

- `to_roman`: 1→I, 4→IV, 9→IX, 40→XL, 99→XCIX, 0→None, 4000→None.
- `format_play_citation`:
  - `Tro.1.1.15` → `Some("I.i.15")`
  - `Tro.3.2.187` → `Some("III.ii.187")`
  - `Ham.5.1.1` → `Some("V.i.1")`
  - `Tro.1.1` → `None` (too few components)
  - `Tro.1.1.1.x` → `None` (too many)
  - `Tro.a.b.c` → `None` (non-numeric)
  - empty string → `None`

No integration test — this is a label formatter, and the three call sites each call the helper identically.

## Non-Goals

- No changes to the citation format stored in the DB.
- No changes to non-play work types (novels, poems, prose).
- No changes to keybind behavior, scroll, or pagination.
- No new config option to toggle the format; plays always use the citation.

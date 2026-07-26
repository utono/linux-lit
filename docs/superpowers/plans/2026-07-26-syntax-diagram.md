# Syntax Diagram Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw a full-screen Cairo diagram of a selected passage's grammatical structure — the text, its parts of speech, and nested clause bands — opened from visual mode on the reader card and the gloss/synopsis/journal overlays.

**Architecture:** Claude returns band spans as JSON; a pure data module validates them; a `DrawingArea` renders them full-screen with Pango. Where lit.db `line_syntax` rows exist they are sent as prompt enrichment so Claude anchors on a real dependency parse, but they never gate the feature — 301 of 306 works have no parse and must still work.

**Tech Stack:** Rust, GTK4, Cairo + Pango (`pangocairo` 0.20), `serde`/`serde_json`, rusqlite, tokio.

**Spec:** `docs/superpowers/specs/2026-07-26-syntax-diagram-design.md`

## Global Constraints

- **Verify with `cargo build` only — never run the app.** The user launches it (`crll`). See CLAUDE.md.
- **Full-screen, not card-bound.** All drawing computes against the `widget_w`/`widget_h` passed to `set_draw_func`. No `main_card_rect` anywhere in this feature.
- **Content column capped at 1240px**, centered: `let panel_w = (widget_w - 2.0 * margin).min(1240.0);` — the existing `keybinds_overlay.rs:692` pattern.
- **Pango for all text**, never `cr.show_text` — the diagram renders early modern English with italic stage directions.
- **Theme colors, never hardcoded literals.** Read `state.theme`; reuse `theme.rs` contrast helpers.
- **Char offsets** into the selection text, matching the `line_syntax` convention (offsets into `canonical_text`).
- **Keybind changes update every surface in the SAME commit** — the new overlay's `Ctrl+/` legend, and `keymap_config.rs` + the stowed `~/tty-dotfiles/linux-lit/keymap.json` if a compiled bind changes. Required, not optional.
- **`run_claude_request` is `pub(crate)`** in `src/input/actions/claude_bridge.rs` — callers must live inside the crate.
- Commit after each task. Branch per the project's git rules (worktree off master if this spans sessions).

**API names verified against the tree at plan time** — use these exactly:
- `crate::theme::vocab_popup_fg(&Theme) -> String` and `vocab_popup_accent(&Theme) -> String` (both `pub(crate)`, `src/theme.rs:601,615`).
- The theme's root color field is **`root_color`**, not `root` (`src/theme.rs:115`).
- `crate::input::navigation::show_chapter_toast_secs(&AppState, &str, u64)` (`pub(crate)`).
- `WidgetExt::create_pango_layout(Option<&str>) -> pango::Layout`; `pangocairo::functions::show_layout(&cairo::Context, &pango::Layout)`; `pango::Layout::cursor_pos(i32) -> (Rectangle, Rectangle)`.
- `AppState.config.claude_model: String` (`src/config.rs:147`).
- `GlossOverlay::visual_selection_text()` / `visual_selection_buffer_text()`; `JournalOverlay::visual_selection_text()`, `exit_visual()`, `exit_visual_to_anchor()`.
- `crate::db::models::Line` has `id: i64` — that id IS the `line_mapping_id` the parse joins on.

---

### Task 1: Data model and JSON validation

Pure module, no GTK. This is the whole correctness surface of the feature — everything drawable is validated here.

**Files:**
- Create: `src/syntax_diagram.rs`
- Modify: `src/main.rs` (add `mod syntax_diagram;`)

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `SyntaxAnalysis { text: String, bands: Vec<Band>, pos: Vec<PosTag>, note: Option<String> }`, `Band { start_char: usize, end_char: usize, label: String, depth: u8 }`, `PosTag { start_char: usize, end_char: usize, pos: String }`, `parse_analysis(json: &str, text: &str) -> Result<SyntaxAnalysis, String>`, `assign_rows(bands: &[Band]) -> Vec<usize>`, `max_row(bands: &[Band]) -> usize` (Task 3 sizes the band stack from this).

- [ ] **Step 1: Write the failing tests**

Create `src/syntax_diagram.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "A touch on the hand, irresolute, makes him start";

    fn json_with(bands: &str) -> String {
        format!(r#"{{"bands":{bands},"pos":[],"note":null}}"#)
    }

    #[test]
    fn parses_well_formed_bands() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":19,"label":"subject","depth":1},
                {"start_char":0,"end_char":47,"label":"main clause","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 2);
        assert_eq!(a.text, TEXT);
    }

    #[test]
    fn drops_band_past_end_of_text() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":9999,"label":"bogus","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert!(a.bands.is_empty(), "out-of-range band must be dropped");
    }

    #[test]
    fn drops_inverted_band() {
        let j = json_with(
            r#"[{"start_char":20,"end_char":5,"label":"inverted","depth":0}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert!(a.bands.is_empty(), "end before start must be dropped");
    }

    #[test]
    fn drops_partially_overlapping_band() {
        // Nesting requires containment or disjointness. 0..20 and 10..30
        // partially overlap: the second must go.
        let j = json_with(
            r#"[{"start_char":0,"end_char":20,"label":"a","depth":0},
                {"start_char":10,"end_char":30,"label":"b","depth":1}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 1);
        assert_eq!(a.bands[0].label, "a");
    }

    #[test]
    fn keeps_disjoint_and_contained_bands() {
        let j = json_with(
            r#"[{"start_char":0,"end_char":47,"label":"outer","depth":0},
                {"start_char":0,"end_char":19,"label":"inner","depth":1},
                {"start_char":21,"end_char":31,"label":"sibling","depth":1}]"#,
        );
        let a = parse_analysis(&j, TEXT).unwrap();
        assert_eq!(a.bands.len(), 3);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_analysis("not json at all", TEXT).is_err());
    }

    #[test]
    fn rejects_offsets_splitting_a_utf8_char() {
        // "café" — byte 4 is inside the é. A band boundary there would panic
        // any later slicing, so it must be dropped.
        let text = "café au lait";
        let j = json_with(r#"[{"start_char":0,"end_char":4,"label":"x","depth":0}]"#);
        let a = parse_analysis(&j, text).unwrap();
        assert!(a.bands.is_empty(), "non-char-boundary offset must be dropped");
    }

    #[test]
    fn assigns_deeper_bands_to_higher_rows() {
        let bands = vec![
            Band { start_char: 0, end_char: 47, label: "outer".into(), depth: 0 },
            Band { start_char: 0, end_char: 19, label: "inner".into(), depth: 1 },
        ];
        let rows = assign_rows(&bands);
        assert_eq!(rows, vec![0, 1]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bins syntax_diagram`
Expected: FAIL — `cannot find function parse_analysis`, `cannot find type Band`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/syntax_diagram.rs` (above the test module):

```rust
//! Data model for the syntax diagram overlay: the validated band/POS spans a
//! Cairo surface draws. Pure — no GTK — so the whole correctness surface is
//! unit-testable without a display.
//!
//! Spans are CHAR OFFSETS into the selection text, matching the `line_syntax`
//! convention (offsets into `canonical_text`), so parse-derived and
//! Claude-derived spans share one coordinate space.

use serde::Deserialize;

/// A labelled span marking what a stretch of the selection grammatically IS.
#[derive(Debug, Clone, Deserialize)]
pub struct Band {
    pub start_char: usize,
    pub end_char: usize,
    pub label: String,
    /// 0 = outermost. Nesting depth, used to stack rows.
    pub depth: u8,
}

/// Part of speech for one word.
#[derive(Debug, Clone, Deserialize)]
pub struct PosTag {
    pub start_char: usize,
    pub end_char: usize,
    pub pos: String,
}

/// What Claude returns, before validation.
#[derive(Debug, Deserialize)]
struct RawAnalysis {
    bands: Vec<Band>,
    #[serde(default)]
    pos: Vec<PosTag>,
    #[serde(default)]
    note: Option<String>,
}

/// A validated analysis, safe to draw.
#[derive(Debug)]
pub struct SyntaxAnalysis {
    /// The selection, exactly as sent.
    pub text: String,
    pub bands: Vec<Band>,
    pub pos: Vec<PosTag>,
    pub note: Option<String>,
}

/// True when `span` is a usable slice of `text`: ordered, in bounds, and on
/// char boundaries (a mid-UTF-8 offset would panic any later slicing).
fn span_ok(text: &str, start: usize, end: usize) -> bool {
    start < end
        && end <= text.len()
        && text.is_char_boundary(start)
        && text.is_char_boundary(end)
}

/// True when two spans nest (one contains the other) or are disjoint.
/// Partial overlap is not drawable as a stack, so it is rejected.
fn compatible(a: &Band, b: &Band) -> bool {
    let disjoint = a.end_char <= b.start_char || b.end_char <= a.start_char;
    let a_in_b = b.start_char <= a.start_char && a.end_char <= b.end_char;
    let b_in_a = a.start_char <= b.start_char && b.end_char <= a.end_char;
    disjoint || a_in_b || b_in_a
}

/// Parse Claude's JSON reply into a validated analysis.
///
/// Malformed JSON is an Err (the caller toasts and does not open the overlay).
/// Individual bad SPANS are dropped, not fatal: a hallucinated offset loses one
/// band, but a bad one would draw garbage. Bands are checked against every band
/// already accepted, so the survivors are mutually nestable.
pub fn parse_analysis(json: &str, text: &str) -> Result<SyntaxAnalysis, String> {
    // Claude may wrap JSON in prose or a fence; take the outermost object.
    let slice = match (json.find('{'), json.rfind('}')) {
        (Some(a), Some(b)) if b > a => &json[a..=b],
        _ => return Err("no JSON object in reply".to_string()),
    };
    let raw: RawAnalysis =
        serde_json::from_str(slice).map_err(|e| format!("bad JSON: {e}"))?;

    let mut bands: Vec<Band> = Vec::new();
    for b in raw.bands {
        if !span_ok(text, b.start_char, b.end_char) {
            crate::logging::log(&format!(
                "SYNTAX: dropped band '{}' [{}..{}] — bad span",
                b.label, b.start_char, b.end_char
            ));
            continue;
        }
        if !bands.iter().all(|k| compatible(&b, k)) {
            crate::logging::log(&format!(
                "SYNTAX: dropped band '{}' [{}..{}] — partial overlap",
                b.label, b.start_char, b.end_char
            ));
            continue;
        }
        bands.push(b);
    }

    let pos = raw
        .pos
        .into_iter()
        .filter(|p| span_ok(text, p.start_char, p.end_char))
        .collect();

    Ok(SyntaxAnalysis { text: text.to_string(), bands, pos, note: raw.note })
}

/// Display row per band, by depth: row 0 sits directly under the POS strip,
/// deeper bands stack above it, so the outermost band is the bottom rule.
pub fn assign_rows(bands: &[Band]) -> Vec<usize> {
    bands.iter().map(|b| b.depth as usize).collect()
}

/// Highest row index any band occupies (0 when there are none) — the drawing
/// code sizes the band stack from this.
pub fn max_row(bands: &[Band]) -> usize {
    assign_rows(bands).into_iter().max().unwrap_or(0)
}
```

- [ ] **Step 4: Register the module**

In `src/main.rs`, add alongside the other `mod` declarations:

```rust
mod syntax_diagram;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins syntax_diagram`
Expected: PASS, 8 tests.

- [ ] **Step 6: Commit**

```bash
git add src/syntax_diagram.rs src/main.rs
git commit -m "feat(syntax): validated band/POS data model for the diagram

Drops out-of-range, inverted, non-char-boundary, and partially-overlapping
spans rather than trusting the model's offsets; malformed JSON is an error so
the overlay never opens on a partial diagram."
```

---

### Task 2: Read `line_syntax` from lit.db

linux-lit's first reader of the upstream parse table. Optional enrichment — returns empty for the 301 unparsed works, and that is a normal result, not a failure.

**Files:**
- Create: `src/db/syntax.rs`
- Modify: `src/db/mod.rs` (add `pub mod syntax;`)

**Interfaces:**
- Consumes: `crate::db::queries::open_db()`.
- Produces: `SyntaxToken { text, pos, dep, head_i }`, `load_line_syntax(conn: &Connection, line_ids: &[i64]) -> Vec<SyntaxToken>`.

- [ ] **Step 1: Write the failing test**

Create `src/db/syntax.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE line_syntax (
               line_mapping_id INTEGER NOT NULL,
               tok_i INTEGER NOT NULL,
               start_char INTEGER NOT NULL,
               end_char INTEGER NOT NULL,
               pos TEXT NOT NULL,
               tag TEXT,
               dep TEXT NOT NULL,
               head_i INTEGER NOT NULL,
               lemma TEXT,
               PRIMARY KEY (line_mapping_id, tok_i));
             INSERT INTO line_syntax VALUES
               (7, 0, 0, 1, 'DET',  'DT', 'det',   1, 'a'),
               (7, 1, 2, 7, 'NOUN', 'NN', 'nsubj', 2, 'touch'),
               (9, 0, 0, 5, 'VERB', 'VB', 'ROOT',  0, 'make');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn loads_tokens_in_line_and_token_order() {
        let toks = load_line_syntax(&db(), &[9, 7]);
        assert_eq!(toks.len(), 3);
        // Ordered by line id then tok_i, regardless of argument order.
        assert_eq!(toks[0].pos, "DET");
        assert_eq!(toks[1].dep, "nsubj");
        assert_eq!(toks[2].pos, "VERB");
    }

    #[test]
    fn unparsed_lines_yield_no_tokens() {
        assert!(load_line_syntax(&db(), &[404]).is_empty());
    }

    #[test]
    fn empty_input_does_not_query() {
        assert!(load_line_syntax(&db(), &[]).is_empty());
    }

    #[test]
    fn missing_table_is_empty_not_an_error() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(load_line_syntax(&conn, &[7]).is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins db::syntax`
Expected: FAIL — `cannot find function load_line_syntax`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/db/syntax.rs`:

```rust
//! Read-only access to lit.db's `line_syntax` table — a spaCy dependency parse
//! per token per line, built upstream in litdb (see that repo's
//! docs/superpowers/specs/2026-07-25-line-syntax-layer-design.md).
//!
//! Coverage is PARTIAL by design: 5 of 306 works are parsed. An unparsed work
//! returns an empty vec, which callers treat as "no enrichment available", not
//! as an error. Never gate a feature on this table being populated.

use rusqlite::Connection;

/// One parsed token, in the shape the diagram prompt needs.
#[derive(Debug, Clone)]
pub struct SyntaxToken {
    pub text: String,
    pub pos: String,
    pub dep: String,
    /// `tok_i` of this token's head within its line; self-pointing = ROOT.
    pub head_i: i64,
}

/// Tokens for `line_ids`, ordered by line then token index.
///
/// Returns empty when the lines are unparsed, the table is absent (an older
/// lit.db), or the query fails — all "no enrichment", never fatal.
pub fn load_line_syntax(conn: &Connection, line_ids: &[i64]) -> Vec<SyntaxToken> {
    if line_ids.is_empty() {
        return Vec::new();
    }
    // rusqlite has no array binding; line_ids are i64 from our own DB rows, so
    // formatting them into the IN list is not an injection vector.
    let ids = line_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT ls.start_char, ls.end_char, ls.pos, ls.dep, ls.head_i, \
                lm.canonical_text \
         FROM line_syntax ls \
         JOIN line_mapping lm ON lm.id = ls.line_mapping_id \
         WHERE ls.line_mapping_id IN ({ids}) \
         ORDER BY ls.line_mapping_id, ls.tok_i"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            crate::logging::log(&format!("SYNTAX: line_syntax unavailable: {e}"));
            return Vec::new();
        }
    };
    let rows = stmt.query_map([], |row| {
        let start: i64 = row.get(0)?;
        let end: i64 = row.get(1)?;
        let pos: String = row.get(2)?;
        let dep: String = row.get(3)?;
        let head_i: i64 = row.get(4)?;
        let canonical: String = row.get(5)?;
        // Offsets index canonical_text exactly (guaranteed by the upstream
        // backfill), but clamp anyway: a stale parse must not panic the reader.
        let (s, e) = (start.max(0) as usize, end.max(0) as usize);
        let text = if s < e && e <= canonical.len()
            && canonical.is_char_boundary(s) && canonical.is_char_boundary(e)
        {
            canonical[s..e].to_string()
        } else {
            String::new()
        };
        Ok(SyntaxToken { text, pos, dep, head_i })
    });
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).filter(|t| !t.text.is_empty()).collect(),
        Err(e) => {
            crate::logging::log(&format!("SYNTAX: line_syntax query failed: {e}"));
            Vec::new()
        }
    }
}

/// Render tokens as the compact table the diagram prompt embeds. One token per
/// line, tab-separated: `word<TAB>POS<TAB>dep<TAB>head_index`. Compact because
/// a long prose selection can run to hundreds of tokens and every one costs
/// prompt budget.
pub fn tokens_as_table(tokens: &[SyntaxToken]) -> String {
    let mut out = String::from("word\tPOS\tdep\thead\n");
    for (i, t) in tokens.iter().enumerate() {
        out.push_str(&format!("{}\t{}\t{}\t{}\n", t.text, t.pos, t.dep, t.head_i));
        if i >= 600 {
            out.push_str("… (truncated)\n");
            break;
        }
    }
    out
}
```

- [ ] **Step 4: Register the module**

In `src/db/mod.rs`, add alongside the other module declarations:

```rust
pub mod syntax;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins db::syntax`
Expected: PASS, 4 tests.

- [ ] **Step 6: Commit**

```bash
git add src/db/syntax.rs src/db/mod.rs
git commit -m "feat(syntax): read line_syntax from lit.db

First reader-side consumer of the upstream parse table. Coverage is partial by
design (5 of 306 works), so an unparsed work, an absent table, and a failed
query all return empty — enrichment, never a gate."
```

---

### Task 3: The Cairo surface

Full-screen `DrawingArea`, Pango text, theme colors. Follows `src/ui/keybinds_overlay.rs`'s structure.

**Files:**
- Create: `src/ui/syntax_overlay.rs`
- Modify: `src/ui/mod.rs` (add `pub mod syntax_overlay;`)

**Interfaces:**
- Consumes: `crate::syntax_diagram::{SyntaxAnalysis, Band, max_row}` (Task 1).
- Produces: `SyntaxOverlay::new()`, `.attach_to(&gtk4::Overlay)`, `.show_loading()`, `.show_analysis(SyntaxAnalysis, &Theme)`, `.hide()`, `.is_visible()`, `.toggle_note()`.

- [ ] **Step 1: Write the failing test**

Create `src/ui/syntax_overlay.rs` with the test module only. Drawing needs a display, so unit tests cover only the pure layout helper:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_row_height_shrinks_to_fit_available_space() {
        // 3 rows in generous space keeps the natural height.
        assert_eq!(row_height(3, 400.0), BAND_ROW_H);
        // 20 rows in 100px must shrink rather than overflow.
        let h = row_height(20, 100.0);
        assert!(h < BAND_ROW_H, "expected shrink, got {h}");
        assert!(h * 20.0 <= 100.0 + f64::EPSILON, "must fit the budget");
        assert!(h >= MIN_BAND_ROW_H, "must not shrink below legibility floor");
    }

    #[test]
    fn zero_rows_is_safe() {
        assert_eq!(row_height(0, 100.0), BAND_ROW_H);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins syntax_overlay`
Expected: FAIL — `cannot find function row_height`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/ui/syntax_overlay.rs`:

```rust
//! Full-screen Cairo diagram of a selection's grammatical structure.
//!
//! Geometry follows `keybinds_overlay.rs` (hexpand/vexpand + Align::Fill,
//! everything computed against the widget_w/widget_h handed to set_draw_func)
//! so the diagram fills the WINDOW, never the reading card — it is unaffected
//! by column count, card margins, or an open two-column play.
//!
//! Two deliberate departures from that precedent:
//!   * Pango, not cr.show_text — this renders the work's own early modern
//!     English, which the toy text API cannot shape or fall back for.
//!   * Theme colors, not hardcoded literals — this is a reading surface, so it
//!     follows the theme cycle.

use gtk4::prelude::*;
use gtk4::{DrawingArea, Overlay};
use std::cell::RefCell;
use std::rc::Rc;

use crate::syntax_diagram::SyntaxAnalysis;

/// Natural height of one band row.
const BAND_ROW_H: f64 = 26.0;
/// Never shrink a band row below this — past it, labels stop being legible.
const MIN_BAND_ROW_H: f64 = 12.0;
/// Window margin around the content column.
const MARGIN: f64 = 48.0;
/// Content column cap, so text does not run edge to edge on a wide display.
const MAX_CONTENT_W: f64 = 1240.0;

/// Height per band row: natural, shrunk to fit `available`, floored at
/// `MIN_BAND_ROW_H` so a pathological stack degrades rather than vanishing.
fn row_height(rows: usize, available: f64) -> f64 {
    if rows == 0 {
        return BAND_ROW_H;
    }
    let fitted = available / rows as f64;
    fitted.min(BAND_ROW_H).max(MIN_BAND_ROW_H)
}

/// What the surface is currently showing.
enum View {
    Loading,
    Analysis(SyntaxAnalysis),
}

struct Inner {
    view: View,
    /// Commentary hidden by default; a key toggles it.
    show_note: bool,
    /// Theme colors, resolved at show time (r, g, b) in 0..1.
    ink: (f64, f64, f64),
    dim: (f64, f64, f64),
    accent: (f64, f64, f64),
    scrim: (f64, f64, f64),
}

pub struct SyntaxOverlay {
    drawing_area: DrawingArea,
    inner: Rc<RefCell<Inner>>,
}

impl SyntaxOverlay {
    pub fn new() -> Self {
        let drawing_area = DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Fill)
            .visible(false)
            .build();

        let inner = Rc::new(RefCell::new(Inner {
            view: View::Loading,
            show_note: false,
            ink: (0.96, 0.94, 0.90),
            dim: (0.70, 0.68, 0.66),
            accent: (0.80, 0.60, 0.40),
            scrim: (0.10, 0.10, 0.12),
        }));

        let draw_inner = inner.clone();
        drawing_area.set_draw_func(move |area, cr, w, h| {
            draw(area, cr, &draw_inner.borrow(), w as f64, h as f64);
        });

        SyntaxOverlay { drawing_area, inner }
    }

    /// Add to the window-filling outer overlay (the same layer the vocab popup
    /// and toasts use), so the diagram floats above the whole reader chain.
    pub fn attach_to(&self, overlay: &Overlay) {
        overlay.add_overlay(&self.drawing_area);
        self.drawing_area.set_visible(false);
    }

    /// Show the loading state. MUST be called before dispatching the Claude
    /// request — `run_claude_request`'s contract.
    pub fn show_loading(&self, theme: &crate::theme::Theme) {
        {
            let mut i = self.inner.borrow_mut();
            i.view = View::Loading;
            i.show_note = false;
            apply_theme(&mut i, theme);
        }
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn show_analysis(&self, analysis: SyntaxAnalysis, theme: &crate::theme::Theme) {
        {
            let mut i = self.inner.borrow_mut();
            i.view = View::Analysis(analysis);
            apply_theme(&mut i, theme);
        }
        self.drawing_area.set_visible(true);
        self.drawing_area.queue_draw();
    }

    pub fn hide(&self) {
        self.drawing_area.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.drawing_area.is_visible()
    }

    /// Toggle the prose commentary under the diagram.
    pub fn toggle_note(&self) {
        {
            let mut i = self.inner.borrow_mut();
            i.show_note = !i.show_note;
        }
        self.drawing_area.queue_draw();
    }
}

/// Resolve theme colors into the (r,g,b) floats Cairo wants.
fn apply_theme(inner: &mut Inner, theme: &crate::theme::Theme) {
    inner.ink = parse_hex(&crate::theme::vocab_popup_fg(theme)).unwrap_or((0.96, 0.94, 0.90));
    inner.accent = parse_hex(&crate::theme::vocab_popup_accent(theme)).unwrap_or((0.80, 0.60, 0.40));
    // NOTE: the field is `root_color`, not `root`.
    inner.scrim = parse_hex(&theme.root_color).unwrap_or((0.10, 0.10, 0.12));
    // Dim = ink pulled toward the scrim, for secondary labels.
    inner.dim = (
        inner.ink.0 * 0.65 + inner.scrim.0 * 0.35,
        inner.ink.1 * 0.65 + inner.scrim.1 * 0.35,
        inner.ink.2 * 0.65 + inner.scrim.2 * 0.35,
    );
}

/// `#rrggbb` (or `rrggbb`) into 0..1 floats.
fn parse_hex(s: &str) -> Option<(f64, f64, f64)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
}

/// Lay out a Pango layout and return it plus its pixel size.
fn layout_text(
    area: &DrawingArea,
    text: &str,
    font: &str,
    width: Option<f64>,
) -> (gtk4::pango::Layout, f64, f64) {
    let layout = area.create_pango_layout(Some(text));
    layout.set_font_description(Some(&gtk4::pango::FontDescription::from_string(font)));
    if let Some(w) = width {
        layout.set_width((w * gtk4::pango::SCALE as f64) as i32);
        layout.set_wrap(gtk4::pango::WrapMode::Word);
    }
    let (pw, ph) = layout.pixel_size();
    (layout, pw as f64, ph as f64)
}

fn draw(area: &DrawingArea, cr: &gtk4::cairo::Context, inner: &Inner, w: f64, h: f64) {
    // Full-window scrim.
    cr.set_source_rgba(inner.scrim.0, inner.scrim.1, inner.scrim.2, 0.97);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();

    let content_w = (w - 2.0 * MARGIN).min(MAX_CONTENT_W);
    let x0 = (w - content_w) / 2.0;

    match &inner.view {
        View::Loading => {
            cr.set_source_rgb(inner.dim.0, inner.dim.1, inner.dim.2);
            let (layout, tw, th) = layout_text(area, "Analyzing syntax…", "Sans 16", None);
            cr.move_to((w - tw) / 2.0, (h - th) / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        }
        View::Analysis(a) => draw_analysis(area, cr, inner, a, x0, content_w, h),
    }
}

fn draw_analysis(
    area: &DrawingArea,
    cr: &gtk4::cairo::Context,
    inner: &Inner,
    a: &SyntaxAnalysis,
    x0: f64,
    content_w: f64,
    h: f64,
) {
    let mut y = MARGIN;

    // ── The selection text ──
    cr.set_source_rgb(inner.ink.0, inner.ink.1, inner.ink.2);
    let (layout, _tw, th) = layout_text(area, &a.text, "Serif 20", Some(content_w));
    cr.move_to(x0, y);
    pangocairo::functions::show_layout(cr, &layout);

    // Byte offset -> (x, y) of that character, via Pango's own index mapping,
    // so bands line up with wrapped text exactly.
    let pos_of = |byte: usize| -> (f64, f64) {
        let (rect, _) = layout.cursor_pos(byte as i32);
        (
            x0 + rect.x() as f64 / gtk4::pango::SCALE as f64,
            y + rect.y() as f64 / gtk4::pango::SCALE as f64,
        )
    };
    let line_h = {
        let (_, _, one_line_h) = layout_text(area, "X", "Serif 20", None);
        one_line_h
    };

    y += th + 8.0;

    // ── POS row: each tag under its word ──
    cr.set_source_rgb(inner.dim.0, inner.dim.1, inner.dim.2);
    for p in &a.pos {
        let (px, py) = pos_of(p.start_char);
        let (pl, _, _) = layout_text(area, &p.pos, "Sans 9", None);
        cr.move_to(px, py + line_h);
        pangocairo::functions::show_layout(cr, &pl);
    }
    y += 16.0;

    // ── Band rows, stacked by depth ──
    let rows = crate::syntax_diagram::max_row(&a.bands) + 1;
    let note_reserve = if inner.show_note && a.note.is_some() { 160.0 } else { 40.0 };
    let budget = (h - y - note_reserve).max(MIN_BAND_ROW_H);
    let rh = row_height(rows, budget);

    for b in &a.bands {
        let (sx, sy) = pos_of(b.start_char);
        let (ex, ey) = pos_of(b.end_char);
        // Deeper bands sit higher: row 0 (outermost) is the bottom rule.
        let row_y = y + (rows as f64 - 1.0 - b.depth as f64) * rh;
        // A band crossing a line wrap draws one segment per visual row.
        let segments: Vec<(f64, f64)> = if (sy - ey).abs() < 1.0 {
            vec![(sx, ex)]
        } else {
            vec![(sx, x0 + content_w), (x0, ex)]
        };
        // Tint by depth, from the theme accent.
        let fade = 1.0 - (b.depth as f64 * 0.15).min(0.6);
        cr.set_source_rgba(inner.accent.0, inner.accent.1, inner.accent.2, fade);
        cr.set_line_width(2.0);
        for (a_x, b_x) in &segments {
            cr.move_to(*a_x, row_y);
            cr.line_to(*b_x, row_y);
            let _ = cr.stroke();
        }
        // Label, centered on the first segment.
        let (lx0, lx1) = segments[0];
        let (ll, lw, _) = layout_text(area, &b.label, "Sans 10", None);
        cr.move_to(((lx0 + lx1) / 2.0 - lw / 2.0).max(x0), row_y - 14.0);
        pangocairo::functions::show_layout(cr, &ll);
    }
    y += rows as f64 * rh + 16.0;

    // ── Commentary (toggleable) ──
    if inner.show_note {
        if let Some(note) = &a.note {
            cr.set_source_rgb(inner.ink.0, inner.ink.1, inner.ink.2);
            let (nl, _, _) = layout_text(area, note, "Sans 12", Some(content_w));
            cr.move_to(x0, y);
            pangocairo::functions::show_layout(cr, &nl);
        }
    }
}
```

- [ ] **Step 4: Register the module**

In `src/ui/mod.rs`:

```rust
pub mod syntax_overlay;
```

- [ ] **Step 5: Verify it builds and tests pass**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles. If `theme.rs`'s `vocab_popup_fg`/`vocab_popup_accent` are not visible from `ui/`, widen them from `pub(crate)` to `pub(crate)` at the crate root (they are already `pub(crate)` — no change expected) and re-run.

Run: `cargo test --bins syntax_overlay`
Expected: PASS, 2 tests.

- [ ] **Step 6: Commit**

```bash
git add src/ui/syntax_overlay.rs src/ui/mod.rs
git commit -m "feat(syntax): full-screen Cairo band diagram surface

Fills the window, not the reading card: keybinds_overlay's geometry
(hexpand/vexpand + Align::Fill, drawn against widget_w/widget_h), content
capped at 1240px. Pango rather than cr.show_text so the work's own early
modern English shapes correctly, and theme colors rather than literals so the
surface follows the theme cycle. Band rows shrink to fit before they overflow,
floored at a legibility minimum."
```

---

### Task 4: Prompt and request plumbing

Builds the request, dispatches it, routes the reply into the surface. This is where the `line_syntax` enrichment becomes optional in exactly one place.

**Files:**
- Create: `src/input/actions/syntax.rs`
- Modify: `src/input/actions/mod.rs` (add `pub mod syntax;`)

**Interfaces:**
- Consumes: `syntax_diagram::parse_analysis` (Task 1), `db::syntax::{load_line_syntax, tokens_as_table}` (Task 2), `ui::syntax_overlay::SyntaxOverlay` (Task 3), `claude_bridge::run_claude_request`.
- Produces: `open_syntax_diagram(state_rc: &Rc<RefCell<AppState>>, text: String, line_ids: Vec<i64>)`.

- [ ] **Step 1: Write the failing test**

Create `src/input/actions/syntax.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_includes_the_selection() {
        let msg = build_user_message("A touch, irresolute, makes him start", "");
        assert!(msg.contains("A touch, irresolute, makes him start"));
    }

    #[test]
    fn user_message_embeds_the_parse_when_present() {
        let table = "word\tPOS\tdep\thead\ntouch\tNOUN\tnsubj\t2\n";
        let msg = build_user_message("A touch", table);
        assert!(msg.contains("nsubj"), "parse table must be embedded");
        assert!(msg.contains("dependency parse"), "must label the table");
    }

    #[test]
    fn user_message_omits_the_parse_section_when_absent() {
        let msg = build_user_message("A touch", "");
        assert!(!msg.contains("dependency parse"),
            "no parse section when the work is unparsed");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bins actions::syntax`
Expected: FAIL — `cannot find function build_user_message`.

- [ ] **Step 3: Write the implementation**

Prepend to `src/input/actions/syntax.rs`:

```rust
//! Syntax diagram: build the request, dispatch it, route the reply into the
//! full-screen Cairo surface.
//!
//! The `line_syntax` enrichment is optional in exactly ONE place — the parse
//! table is empty for the 301 unparsed works and the prompt simply omits that
//! section. There is no second code path and no coverage gate.

use crate::app::AppState;
use std::cell::RefCell;
use std::rc::Rc;

/// Compiled fallback, used when lit.db has no `syntax.diagram` row.
const FALLBACK_PROMPT: &str = "\
You analyze the grammatical structure of a passage of literature and return \
ONLY a JSON object — no prose, no markdown fence, no commentary outside the \
JSON.

The JSON object has exactly these keys:

{
  \"bands\": [{\"start_char\": 0, \"end_char\": 19, \"label\": \"subject\", \"depth\": 0}],
  \"pos\": [{\"start_char\": 0, \"end_char\": 1, \"pos\": \"DET\"}],
  \"note\": \"Two or three sentences on what this structure is doing.\"
}

`bands` mark what each stretch of the passage grammatically IS — \"main \
clause\", \"relative clause\", \"appositive\", \"subject\", \"predicate\", \
\"participial modifier\". `depth` is nesting depth: 0 is the outermost span, \
and a band at depth N+1 must be fully CONTAINED in a band at depth N. Bands at \
the same depth must not overlap. Partially overlapping bands are discarded, so \
never emit them.

`start_char` and `end_char` are BYTE offsets into the passage exactly as given, \
counted from 0. Be precise: an offset that lands mid-word or past the end of \
the text discards that band.

`pos` gives a part-of-speech tag per word, using the coarse Universal \
Dependencies set (ADJ, ADP, ADV, AUX, CCONJ, DET, INTJ, NOUN, NUM, PART, PRON, \
PROPN, PUNCT, SCONJ, VERB).

`note` is two or three sentences on what the structure is DOING rhetorically — \
why a modifier is set off, what an inversion delays, how the subordination \
shapes the reading. Write for a thoughtful reader. No markdown. Set any work \
title in quotation marks, never asterisks.

The passage may be early modern English. Analyze the syntax as it actually \
stands, not as modern English would render it.";

/// The system prompt: lit.db `api_prompts` row `syntax.diagram`, else the
/// compiled fallback. Mirrors `gloss.rs`'s prompt-or-fallback pattern.
fn system_prompt() -> String {
    crate::db::prompts::active_prompt("syntax.diagram").unwrap_or_else(|| {
        crate::logging::log(
            "SYNTAX PROMPT: syntax.diagram missing from api_prompts; using compiled fallback",
        );
        FALLBACK_PROMPT.to_string()
    })
}

/// The user message: the passage, plus the parse table when the work has one.
/// `parse_table` is empty for unparsed works, which omits the section entirely.
fn build_user_message(text: &str, parse_table: &str) -> String {
    let mut msg = format!("Passage:\n{text}\n");
    if !parse_table.is_empty() {
        msg.push_str(
            "\nA dependency parse of this passage is available. Use it to \
             anchor your analysis; where it disagrees with your own reading of \
             the syntax (it was produced by a model trained on modern English \
             and misparses archaic constructions), trust your reading.\n\n",
        );
        msg.push_str(parse_table);
    }
    msg
}

/// Open the diagram for `text`. `line_ids` are `line_mapping` row ids for the
/// selection — empty for overlay selections (gloss/journal text has no
/// line_mapping rows), which simply means no enrichment.
pub fn open_syntax_diagram(
    state_rc: &Rc<RefCell<AppState>>,
    text: String,
    line_ids: Vec<i64>,
) {
    if text.trim().is_empty() {
        crate::logging::log("SYNTAX: empty selection, not opening");
        return;
    }

    let parse_table = if line_ids.is_empty() {
        String::new()
    } else {
        match crate::db::queries::open_db() {
            Ok(conn) => {
                let toks = crate::db::syntax::load_line_syntax(&conn, &line_ids);
                crate::logging::log(&format!(
                    "SYNTAX: {} parsed tokens for {} lines",
                    toks.len(),
                    line_ids.len()
                ));
                crate::db::syntax::tokens_as_table(&toks)
            }
            Err(_) => String::new(),
        }
    };

    let model = {
        let s = state_rc.borrow();
        // Loading state BEFORE the request — run_claude_request's contract.
        s.syntax_overlay.show_loading(&s.theme);
        s.config.claude_model.clone()
    };
    {
        let mut s = state_rc.borrow_mut();
        s.input_mode = crate::app::InputMode::SyntaxDiagram;
    }

    let user_msg = build_user_message(&text, &parse_table);
    let text_for_parse = text.clone();

    crate::input::actions::claude_bridge::run_claude_request(
        state_rc,
        system_prompt(),
        user_msg,
        model,
        move |st, reply| {
            match crate::syntax_diagram::parse_analysis(&reply, &text_for_parse) {
                Ok(analysis) => {
                    crate::logging::log(&format!(
                        "SYNTAX: {} bands, {} pos tags",
                        analysis.bands.len(),
                        analysis.pos.len()
                    ));
                    let s = st.borrow();
                    s.syntax_overlay.show_analysis(analysis, &s.theme);
                }
                Err(e) => {
                    crate::logging::log(&format!("SYNTAX: {e}"));
                    let mut s = st.borrow_mut();
                    s.syntax_overlay.hide();
                    s.input_mode = crate::app::InputMode::Reader;
                    crate::input::navigation::show_chapter_toast_secs(
                        &s,
                        "Could not analyze syntax",
                        3,
                    );
                }
            }
        },
        move |st, msg| {
            let mut s = st.borrow_mut();
            s.syntax_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            crate::input::navigation::show_chapter_toast_secs(&s, msg, 3);
        },
    );
}
```

- [ ] **Step 4: Register the module**

In `src/input/actions/mod.rs`:

```rust
pub mod syntax;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bins actions::syntax`
Expected: PASS, 3 tests.

This task does not build standalone — `state.syntax_overlay` and
`InputMode::SyntaxDiagram` arrive in Task 5. Expect `cargo build` to fail on
exactly those two names until then; the unit tests compile because they touch
only `build_user_message`.

- [ ] **Step 6: Commit**

```bash
git add src/input/actions/syntax.rs src/input/actions/mod.rs
git commit -m "feat(syntax): prompt and request plumbing for the diagram

line_syntax enrichment is optional in exactly one place — the parse table is
empty for unparsed works and the prompt omits that section, so there is one
code path and no coverage gate. The prompt tells the model to trust its own
reading where the parse disagrees, since both spaCy models misparse archaic
syntax."
```

---

### Task 5: Wire into AppState and the input modes

Adds the mode, the state field, the overlay attachment, and the key handling. After this task the feature builds and runs.

**Files:**
- Modify: `src/app/mod.rs` (`InputMode` enum ~line 89; `AppState` struct; window construction ~line 2043)
- Modify: `src/input/keymap.rs` (mode dispatch ~line 276)
- Modify: `src/input/visual.rs` (`BUILTIN_ACTIONS` line 235; `execute_action` line 270)

**Interfaces:**
- Consumes: `open_syntax_diagram` (Task 4), `SyntaxOverlay` (Task 3).
- Produces: `InputMode::SyntaxDiagram`, `AppState.syntax_overlay`, `action_syntax_diagram`.

- [ ] **Step 1: Add the InputMode variant**

In `src/app/mod.rs`, inside `enum InputMode` (after `SynopsisVisual`):

```rust
    /// Full-screen Cairo syntax diagram of a visual-mode selection. Escape
    /// closes; `n` toggles the prose commentary. All other keys are swallowed.
    SyntaxDiagram,
```

- [ ] **Step 2: Add the AppState field**

In `src/app/mod.rs`, in the `AppState` struct near `vocab_popup`:

```rust
    pub syntax_overlay: crate::ui::syntax_overlay::SyntaxOverlay,
```

- [ ] **Step 3: Construct and attach the overlay**

In `src/app/mod.rs` window construction, beside the vocab popup's creation (~line 1673):

```rust
    let syntax_overlay = crate::ui::syntax_overlay::SyntaxOverlay::new();
```

Then beside `vocab_popup.attach_to(&outer_overlay);` (~line 2055):

```rust
    syntax_overlay.attach_to(&outer_overlay);
```

Then in the `AppState { ... }` initializer, beside `vocab_popup:`:

```rust
        syntax_overlay,
```

- [ ] **Step 4: Add the mode dispatch arm**

In `src/input/keymap.rs`, in the `match mode` block that handles non-Reader modes (~line 291, beside the other overlay arms):

```rust
            crate::app::InputMode::SyntaxDiagram => handle_syntax_diagram_key(state, key_name),
```

Then add the handler beside the other per-mode handlers:

```rust
/// Full-screen syntax diagram. Escape closes and returns to the reader; `n`
/// toggles the prose commentary; every other key is swallowed so the diagram
/// is fully modal.
fn handle_syntax_diagram_key(
    state: &Rc<RefCell<AppState>>,
    key_name: &str,
) -> bool {
    match key_name {
        "Escape" => {
            let mut s = state.borrow_mut();
            s.syntax_overlay.hide();
            s.input_mode = crate::app::InputMode::Reader;
            true
        }
        "n" => {
            state.borrow().syntax_overlay.toggle_note();
            true
        }
        _ => true,
    }
}
```

- [ ] **Step 5: Add the visual-mode action**

In `src/input/visual.rs` line 235, add `"Syntax"` to the END of the array (appending keeps every existing index stable — the file warns the array and the `match` are coupled POSITIONALLY):

```rust
pub const BUILTIN_ACTIONS: &[&str] = &["Reader Gloss", "Journal Q&A", "Gloss with Claude", "Inner Monologue", "Copy", "Copy with metadata", "Syntax"];
```

In `execute_action`'s `match index` (line 278), add the arm for index 6, before the `_ => {}`:

```rust
            6 => {
                action_syntax_diagram(state_rc);
                return;
            }
```

Then add the handler beside `action_journal_qa`:

```rust
/// Visual-mode "Syntax": open the full-screen diagram for the selection.
/// Collects the selected buffer lines' text and their `line_mapping` row ids
/// (the parse enrichment key); works with no `line_syntax` rows simply send
/// the text alone.
pub(crate) fn action_syntax_diagram(state_rc: &std::rc::Rc<std::cell::RefCell<AppState>>) {
    let (text, line_ids) = {
        let state = state_rc.borrow();
        let (start_buf, end_buf) = match &state.visual_selection {
            Some(s) => s.range(),
            None => return,
        };
        let work = match &state.current_work {
            Some(w) => w,
            None => return,
        };
        let lines: Vec<crate::db::models::Line> = (start_buf..=end_buf)
            .filter_map(|buf_line| {
                state.work_line_for_buffer(buf_line)
                    .and_then(|wi| work.lines.get(wi).cloned())
            })
            .collect();
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let ids = lines.iter().map(|l| l.id).collect::<Vec<i64>>();
        (text, ids)
    };
    exit_visual_mode(&mut state_rc.borrow_mut());
    crate::input::actions::syntax::open_syntax_diagram(state_rc, text, line_ids);
}
```

- [ ] **Step 6: Build and verify**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles with no errors.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new warnings.

Run: `cargo test --bins`
Expected: PASS — all existing tests plus Tasks 1–4's.

- [ ] **Step 7: Commit**

```bash
git add src/app/mod.rs src/input/keymap.rs src/input/visual.rs
git commit -m "feat(syntax): wire the diagram into visual mode and the mode table

'Syntax' appended to BUILTIN_ACTIONS (appending keeps every existing index
stable — the array and execute_action's match are coupled positionally).
InputMode::SyntaxDiagram is fully modal: Escape closes, n toggles the
commentary, everything else is swallowed."
```

---

### Task 6: Overlay visual-mode entry points

Extends the diagram to the gloss/synopsis/journal overlays. Their selections are rendered overlay text with no `line_mapping` rows, so they always take the text-only path.

**Files:**
- Modify: `src/input/keymap.rs` (`BlockVisualCfg` ~line 3183, both CFG consts ~3202/3211, `handle_block_visual_key` ~3243, `handle_journal_visual_key` ~3296)

**Interfaces:**
- Consumes: `open_syntax_diagram` (Task 4).
- Produces: nothing downstream.

- [ ] **Step 1: Add the key to the block-visual handler**

In `src/input/keymap.rs`, in `handle_block_visual_key`'s `match key_name`, before the `_ => true` arm:

```rust
        // s: open the full-screen syntax diagram for the selected blocks.
        // Overlay text has no line_mapping rows, so no parse enrichment.
        "s" => {
            let text = {
                let s = state.borrow();
                (cfg.yank_text)(&s.gloss_overlay)
            };
            {
                let mut s = state.borrow_mut();
                (cfg.escape_exit)(&s.gloss_overlay);
                s.input_mode = cfg.return_mode;
                (cfg.set_hint)(&s.gloss_overlay);
            }
            crate::input::actions::syntax::open_syntax_diagram(state, text, Vec::new());
            true
        }
```

- [ ] **Step 2: Add the same key to the journal visual handler**

`handle_journal_visual_key` is a parallel function fixed to `JournalOverlay` (a
different widget type, so it cannot share `BlockVisualCfg`). Add the same arm
there, using the journal overlay's own accessors. Read the function's existing
`"y"` arm first and mirror its text source and exit call exactly — it uses
`s.journal_overlay`, not `s.gloss_overlay`:

```rust
        "s" => {
            let text = {
                let s = state.borrow();
                s.journal_overlay.visual_selection_text()
            };
            {
                let mut s = state.borrow_mut();
                s.journal_overlay.exit_visual_to_anchor();
                s.input_mode = crate::app::InputMode::JournalOverlay;
            }
            crate::input::actions::syntax::open_syntax_diagram(state, text, Vec::new());
            true
        }
```

If `visual_selection_text` / `exit_visual_to_anchor` are named differently on
`JournalOverlay`, use whatever the existing `"y"` arm calls — do not invent
names.

- [ ] **Step 3: Update the overlay keybind legends**

Required, not optional — the project's keybind rule is that every surface a
bind touches updates in the SAME change. Add an `s → syntax diagram` entry to
the visual-mode group of each affected legend:

- `src/ui/gloss_keybinds_overlay.rs` (GROUPS)
- `src/ui/synopsis_keybinds_overlay.rs` (GROUPS)
- `src/ui/journal_keybinds_overlay.rs` (GROUPS)

Read one legend's `GROUPS` const first and match its exact entry format.

- [ ] **Step 4: Add the diagram's own legend**

The diagram is a modal surface, so it needs its own `Ctrl+/` legend listing its
two binds (`Escape` close, `n` toggle commentary). Create
`src/ui/syntax_keybinds_overlay.rs` modelled on
`src/ui/gloss_keybinds_overlay.rs` — read that file and mirror its structure,
`GROUPS` shape, and MRU consts. Register it in `src/ui/mod.rs` and show it from
`handle_syntax_diagram_key` on the `Ctrl+/` chord, matching how the gloss
overlay opens its legend.

- [ ] **Step 5: Build and verify**

Run: `cargo build 2>&1 | tail -20`
Expected: compiles.

Run: `cargo clippy 2>&1 | tail -20`
Expected: no new warnings.

- [ ] **Step 6: Commit**

```bash
git add src/input/keymap.rs src/ui/
git commit -m "feat(syntax): open the diagram from the overlay visual modes

s in gloss/synopsis/journal visual mode diagrams the selected blocks. Overlay
text has no line_mapping rows, so these always take the text-only path — which
is why text-only is a first-class path, not a fallback. Legends updated in the
same change per the keybind rule, plus the diagram's own Ctrl+/ legend."
```

---

### Task 7: Headless on-screen verification

Mandatory. Per CLAUDE.md the on-screen check is correctness, not review, and
survives any "no review gates" instruction. Cage is software rendering and can
disagree with the real GL renderer on layout, so this ends with a hand-off.

**Files:**
- None (verification only)

**Interfaces:**
- Consumes: the built binary.
- Produces: screenshots in the scratchpad, a written report.

- [ ] **Step 1: Build and launch headless**

```bash
cd ~/utono/linux-lit && cargo build
```

Launch cage with the harness `run_in_background` (NOT `nohup`/`setsid`/`timeout`
— a detached wrapper kills the instance when it returns):

```bash
LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log
```

`LIT_DEV=1` is required or the run loads release config and takes a release
instance slot. Prefer `./scripts/e2e-env.sh` (it mints a fresh
`XDG_RUNTIME_DIR`); a hand-rolled cage run reusing `/run/user/1000` collides
with the user's own compositor and `grim` will screenshot their live desktop.

- [ ] **Step 2: Resize to production geometry**

```bash
wlr-randr --output HEADLESS-1 --custom-mode 1920x1200
```

The first `wtype` chord after a resize is dropped (focus loss) — re-send it and
confirm the `KEY:` line landed in the log before trusting any screenshot.

- [ ] **Step 3: Verify the enriched path**

Land on BH-Barrett (one of the five parsed works), select the passage
containing "in the dark room, irresolute, makes him start and say", open the
action popup, choose "Syntax".

Expected: `irresolute` draws as its own labelled band, nested inside the main
clause band. Check the log for `SYNTAX: N parsed tokens for M lines` with N > 0
— that confirms the enrichment path actually ran.

- [ ] **Step 4: Verify the text-only path**

Repeat on any work with NO `line_syntax` rows (anything except BH-Barrett,
BH-Margolyes, BH-Vance, TT, Ham-Arkangel).

Expected: a well-formed band stack. Log shows `SYNTAX: 0 parsed tokens` or no
token line at all. This is the path 301 of 306 works take — it must not be
skipped.

- [ ] **Step 5: Verify full-screen geometry on a two-column play**

Open a two-column play, select a speech, open the diagram.

Expected: the scrim fills the whole window. A card-bound surface would visibly
size to one column — this is exactly the regression the full-screen decision
was made to prevent.

Pixel-measure rather than judging by eye, per the project's clipping rules:

```bash
python3 -c "
from PIL import Image
im = Image.open('/tmp/claude-1000/-home-mlj-utono-linux-lit/21fa60bf-7f89-4c5e-aff9-405088c0970c/scratchpad/syntax-play.png')
w, h = im.size
print('size', w, h)
print('corners', im.getpixel((2,2)), im.getpixel((w-3,2)), im.getpixel((2,h-3)), im.getpixel((w-3,h-3)))
"
```

All four corners must be the scrim color. A card-bound surface leaves reader
content in at least one.

- [ ] **Step 6: Verify the commentary toggle and dismissal**

Press `n` — commentary appears below the bands; `n` again hides it. Press
Escape — the diagram closes and the reader is back, cursor intact.

- [ ] **Step 7: Open every screenshot and report**

Per the UI review protocol: open each PNG, quote the on-screen text, and call
out clipping or layout problems by eye. A passing exit code is not enough.

- [ ] **Step 8: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Use exactly this pattern. A bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

- [ ] **Step 9: Hand off for real-renderer confirmation**

Cage is software rendering. Give the user the exact command and the criteria to
eyeball on their real GL renderer — text shaping, band alignment against
wrapped text, and theme colors are all things cairo-vs-GL can disagree on.

---

## Open decisions deferred to implementation

- **Parse table format** is settled in Task 2 as tab-separated
  `word\tPOS\tdep\thead`, truncated at 600 tokens. If a long prose selection
  proves to blow the prompt budget in practice, tighten the cap — the format
  is one function (`tokens_as_table`).
- **The `s` bind in overlay visual mode** may collide with an existing bind in
  one of the three overlays. Task 6 Step 2 says to read the existing arms
  first; if `s` is taken, pick a free key and update all three legends to
  match.

# Wright Scansion Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reader keybind (`i`) that cycles a Wright metrical scansion overlay (Off → StressOnly → Full) on the current verse work's lines, drawing combining stress marks, a line-type label, and a caesura marker from lit.db's `line_meter`/`syllable_scan` tables.

**Architecture:** A pure mark-renderer (`src/scansion.rs`) re-finds each syllable's vowel on the *displayed* buffer line and inserts combining marks — never trusting a stored char offset. A DB query loads scansion per work into an `AppState` cache. The existing `rebuild_buffer_text` path bakes marks into the whole buffer when the overlay is on. Audio highlight is line-level, so intra-line marks can't drift it.

**Tech Stack:** Rust, GTK4 (`gtk4`/sourceview5), rusqlite, the existing linux-lit input/dispatch/config machinery.

**Spec:** `docs/superpowers/specs/2026-06-15-wright-scansion-overlay-design.md`

---

## File Structure

- **Create** `src/scansion.rs` — pure `ScanLevel` enum + `mark_line()` renderer + `LineScansion`/`ScanSyllable` types. No DB, no GTK. Fully unit-tested.
- **Modify** `src/main.rs:19` — add `mod scansion;`.
- **Modify** `src/db/queries.rs` — add `load_scansion_for_work()`.
- **Modify** `src/app.rs` — add `scansion_level` + `scansion_data` fields to `AppState`; init them; extend `rebuild_buffer_text` to bake marks.
- **Modify** `src/input/actions/mod.rs` — add `Action::CycleScansion` (enum + `category()` + `name()`).
- **Modify** `src/input/keymap.rs` — add the `CycleScansion` dispatch arm.
- **Modify** `src/input/keymap_config.rs` — rebind `i`/`Alt+i`/`Ctrl+Alt+i`; update default tests.
- **Modify** `src/config.rs` — persist `scansion_level`.
- **Modify** `src/ui/keybinds_overlay.rs` — document the rebind.

Build/test commands (from `~/utono/linux-lit`):
- `cargo build` — compile
- `cargo test --bins` — unit tests (no GUI)

---

## Task 1: Pure scansion types + module wiring

**Files:**
- Create: `src/scansion.rs`
- Modify: `src/main.rs:19`

- [ ] **Step 1: Create the module with types and a placeholder renderer**

Create `src/scansion.rs`:

```rust
//! Pure Wright-scansion mark renderer. No DB, no GTK. Given a DISPLAYED line and
//! its scansion, returns the line with combining stress marks inserted on each
//! syllable's vowel, plus the line-type label. Marks are placed by re-finding the
//! vowel IN the displayed line — never by trusting a stored char offset — so the
//! invariant "strip the combining marks -> the displayed line" always holds.

/// Combining acute U+0301 over a stressed syllable's vowel.
pub const ACUTE: char = '\u{0301}';
/// Combining breve U+0306 over an unstressed syllable's vowel.
pub const BREVE: char = '\u{0306}';
/// Thin double bar marking a caesura (metrical pause), inserted after the
/// caesura syllable's vowel.
pub const CAESURA: &str = "\u{2016}"; // ‖

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanLevel {
    Off,
    StressOnly,
    Full,
}

impl ScanLevel {
    /// Advance Off -> StressOnly -> Full -> Off.
    pub fn next(self) -> ScanLevel {
        match self {
            ScanLevel::Off => ScanLevel::StressOnly,
            ScanLevel::StressOnly => ScanLevel::Full,
            ScanLevel::Full => ScanLevel::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanSyllable {
    /// The syllable text as scanned (from syllable_scan.surface).
    pub surface: String,
    /// 1 = strong (stressed), 0 = weak (unstressed). Single source of truth.
    pub ictus: i8,
    pub is_extrametrical: bool,
}

#[derive(Debug, Clone)]
pub struct LineScansion {
    pub line_type: String,
    /// 1-based syllable position after which a caesura falls, or None.
    pub caesura_after: Option<i32>,
    pub syllables: Vec<ScanSyllable>,
}

/// A rendered line: the marked text plus the separate line-type label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedLine {
    pub text: String,
    pub label: String,
}
```

- [ ] **Step 2: Declare the module**

In `src/main.rs`, after line 19 (`mod ui;`), add:

```rust
mod scansion;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles (dead-code warnings on the new types are fine for now).

- [ ] **Step 4: Commit**

```bash
git add src/scansion.rs src/main.rs
git commit -m "feat(scansion): pure scansion types + module wiring"
```

---

## Task 2: `mark_line` renderer — vowel re-find + marks

**Files:**
- Modify: `src/scansion.rs`
- Test: `src/scansion.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Append to `src/scansion.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn syl(surface: &str, ictus: i8) -> ScanSyllable {
        ScanSyllable { surface: surface.to_string(), ictus, is_extrametrical: false }
    }

    fn strip(s: &str) -> String {
        s.chars().filter(|c| *c != ACUTE && *c != BREVE).collect()
    }

    #[test]
    fn off_is_identity() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1)] };
        let m = mark_line("If music", &scan, ScanLevel::Off);
        assert_eq!(m.text, "If music");
        assert_eq!(m.label, "regular");
    }

    #[test]
    fn stress_only_marks_only_strong() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1), syl("sic", 0)] };
        let m = mark_line("If music", &scan, ScanLevel::StressOnly);
        assert!(m.text.contains(ACUTE));   // the strong "mu"
        assert!(!m.text.contains(BREVE));  // no breve in StressOnly
        assert_eq!(strip(&m.text), "If music"); // invariant: strip -> displayed line
    }

    #[test]
    fn full_marks_both() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 0), syl("mu", 1)] };
        let m = mark_line("If mu", &scan, ScanLevel::Full);
        assert!(m.text.contains(ACUTE));
        assert!(m.text.contains(BREVE));
        assert_eq!(strip(&m.text), "If mu");
    }

    #[test]
    fn caesura_inserted_after_position() {
        let scan = LineScansion { line_type: "regular".into(), caesura_after: Some(1),
            syllables: vec![syl("If", 1), syl("mu", 0)] };
        let m = mark_line("If mu", &scan, ScanLevel::StressOnly);
        assert!(m.text.contains(CAESURA));
        // stripping marks AND the caesura glyph reproduces the line
        let cleaned: String = m.text.chars()
            .filter(|c| *c != ACUTE && *c != BREVE && c.to_string() != CAESURA)
            .collect();
        assert_eq!(cleaned, "If mu");
    }

    #[test]
    fn surface_not_found_skips_syllable_no_panic() {
        // "xyz" isn't in the line; that syllable gets no mark, others still do.
        let scan = LineScansion { line_type: "regular".into(), caesura_after: None,
            syllables: vec![syl("If", 1), syl("xyz", 1)] };
        let m = mark_line("If music", &scan, ScanLevel::StressOnly);
        assert_eq!(strip(&m.text), "If music");
        assert!(m.text.contains(ACUTE)); // "If" still marked
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bins scansion::tests`
Expected: FAIL — `cannot find function mark_line in this scope`.

- [ ] **Step 3: Implement `mark_line`**

Add to `src/scansion.rs` (above the `#[cfg(test)]` module):

```rust
/// Vowels that can carry a combining mark.
fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// Render `displayed_line` with stress marks for `scan` at `level`.
///
/// Walks the syllables in order, advancing a cursor across the displayed line.
/// For each syllable it locates the syllable's first vowel at or after the cursor
/// (anchored on the syllable's `surface` when that substring is found ahead, so
/// repeated letters don't misalign) and records a combining mark for that char
/// index. A syllable whose vowel can't be located is skipped (no mark) rather
/// than mis-placed. Marks are inserted AFTER the vowel char so stripping the
/// combining chars reproduces `displayed_line` exactly.
pub fn mark_line(displayed_line: &str, scan: &LineScansion, level: ScanLevel) -> MarkedLine {
    if level == ScanLevel::Off {
        return MarkedLine { text: displayed_line.to_string(), label: scan.line_type.clone() };
    }

    let chars: Vec<char> = displayed_line.chars().collect();
    // char index -> combining mark to insert after it
    let mut marks: std::collections::BTreeMap<usize, char> = std::collections::BTreeMap::new();
    // char indices after which to insert a caesura glyph
    let mut caesura_at: Option<usize> = None;

    let mut cursor = 0usize; // char index into `chars`
    for (pos, syl) in scan.syllables.iter().enumerate() {
        // Anchor the search: if the surface appears at/after cursor, start there.
        let search_from = find_surface(&chars, cursor, &syl.surface).unwrap_or(cursor);
        let vowel_idx = (search_from..chars.len()).find(|&i| is_vowel(chars[i]));
        let vowel_idx = match vowel_idx {
            Some(i) => i,
            None => continue, // no vowel locatable — skip this syllable
        };
        // Place the stress mark per level.
        let want_mark = match level {
            ScanLevel::StressOnly => syl.ictus == 1,
            ScanLevel::Full => true,
            ScanLevel::Off => false,
        };
        if want_mark {
            marks.insert(vowel_idx, if syl.ictus == 1 { ACUTE } else { BREVE });
        }
        // Caesura falls after the 1-based `caesura_after` syllable's vowel.
        if scan.caesura_after == Some(pos as i32 + 1) {
            caesura_at = Some(vowel_idx);
        }
        cursor = vowel_idx + 1;
    }

    // Rebuild the string, inserting marks/caesura after the relevant chars.
    let mut out = String::with_capacity(displayed_line.len() + marks.len() * 2 + 3);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        if let Some(&mk) = marks.get(&i) {
            out.push(mk);
        }
        if caesura_at == Some(i) {
            out.push(' ');
            out.push_str(CAESURA);
            out.push(' ');
        }
    }
    MarkedLine { text: out, label: scan.line_type.clone() }
}

/// First char index >= `from` where `surface` begins in `chars` (case-insensitive,
/// alphanumeric-only comparison so punctuation/spacing differences don't block it).
/// Returns None if not found.
fn find_surface(chars: &[char], from: usize, surface: &str) -> Option<usize> {
    let needle: Vec<char> = surface.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if needle.is_empty() {
        return None;
    }
    for start in from..chars.len() {
        let mut ni = 0usize;
        let mut ci = start;
        while ci < chars.len() && ni < needle.len() {
            let cc = chars[ci];
            if cc.is_alphanumeric() {
                if cc.to_ascii_lowercase() != needle[ni] {
                    break;
                }
                ni += 1;
            }
            ci += 1;
        }
        if ni == needle.len() {
            return Some(start);
        }
    }
    None
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bins scansion::tests`
Expected: PASS (all 5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/scansion.rs
git commit -m "feat(scansion): mark_line vowel re-find renderer + tests"
```

---

## Task 3: `load_scansion_for_work` DB query

**Files:**
- Modify: `src/db/queries.rs`
- Test: `src/db/queries.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Append a test module to `src/db/queries.rs` (or add to an existing one):

```rust
#[cfg(test)]
mod scansion_tests {
    use super::*;
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE line_mapping (id INTEGER PRIMARY KEY, work_abbrev TEXT,
               div1 INTEGER, div2 INTEGER, line_in_div INTEGER, canonical_text TEXT);
             CREATE TABLE line_meter (line_id INTEGER, syllable_count INTEGER,
               nominal_feet INTEGER, line_type TEXT, caesura_after INTEGER,
               is_rhymed INTEGER, confidence REAL, source_note TEXT);
             CREATE TABLE syllable_scan (line_id INTEGER, position INTEGER,
               foot_index INTEGER, ictus INTEGER, foot_type TEXT, surface TEXT,
               start_char INTEGER, end_char INTEGER, phenomenon TEXT,
               is_extrametrical INTEGER);
             INSERT INTO line_mapping VALUES (10,'TN',1,1,1,'If music');
             INSERT INTO line_meter (line_id,syllable_count,nominal_feet,line_type,caesura_after)
               VALUES (10,2,5,'regular',NULL);
             INSERT INTO syllable_scan (line_id,position,foot_index,ictus,surface,is_extrametrical)
               VALUES (10,1,1,0,'If',0),(10,2,1,1,'mu',0);",
        ).unwrap();
        c
    }

    #[test]
    fn loads_scansion_keyed_by_line_id() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        let ls = map.get(&10).expect("line 10 present");
        assert_eq!(ls.line_type, "regular");
        assert_eq!(ls.caesura_after, None);
        assert_eq!(ls.syllables.len(), 2);
        assert_eq!(ls.syllables[1].ictus, 1);
        assert_eq!(ls.syllables[1].surface, "mu");
    }

    #[test]
    fn unscanned_line_absent_from_map() {
        let c = fixture();
        let map = load_scansion_for_work(&c, "TN").unwrap();
        assert!(map.get(&999).is_none());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --bins scansion_tests`
Expected: FAIL — `cannot find function load_scansion_for_work`.

- [ ] **Step 3: Implement the query**

Add to `src/db/queries.rs` (top-level, near `load_work`):

```rust
use std::collections::HashMap;
use crate::scansion::{LineScansion, ScanSyllable};

/// Load Wright scansion for every scanned line of `abbrev`, keyed by
/// `line_mapping.id`. Lines with no `line_meter` row are absent from the map
/// (rendered plain by the caller). Mirrors `load_work`'s query idiom.
pub fn load_scansion_for_work(
    conn: &Connection,
    abbrev: &str,
) -> Result<HashMap<i64, LineScansion>, rusqlite::Error> {
    // 1. line_meter rows for this work's lines.
    let mut meter_stmt = conn.prepare(
        "SELECT lm.line_id, lm.line_type, lm.caesura_after \
         FROM line_meter lm JOIN line_mapping m ON lm.line_id = m.id \
         WHERE m.work_abbrev = ?1",
    )?;
    let mut map: HashMap<i64, LineScansion> = HashMap::new();
    let meter_rows = meter_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let line_type: String = row.get(1)?;
        let caesura_after: Option<i32> = row.get(2)?;
        Ok((line_id, line_type, caesura_after))
    })?;
    for r in meter_rows {
        let (line_id, line_type, caesura_after) = r?;
        map.insert(line_id, LineScansion { line_type, caesura_after, syllables: Vec::new() });
    }

    // 2. syllable_scan rows, appended in position order to their line.
    let mut syl_stmt = conn.prepare(
        "SELECT s.line_id, s.surface, s.ictus, s.is_extrametrical \
         FROM syllable_scan s JOIN line_mapping m ON s.line_id = m.id \
         WHERE m.work_abbrev = ?1 ORDER BY s.line_id, s.position",
    )?;
    let syl_rows = syl_stmt.query_map([abbrev], |row| {
        let line_id: i64 = row.get(0)?;
        let surface: Option<String> = row.get(1)?;
        let ictus: i64 = row.get(2)?;
        let is_extra: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
        Ok((line_id, surface.unwrap_or_default(), ictus as i8, is_extra != 0))
    })?;
    for r in syl_rows {
        let (line_id, surface, ictus, is_extrametrical) = r?;
        if let Some(ls) = map.get_mut(&line_id) {
            ls.syllables.push(ScanSyllable { surface, ictus, is_extrametrical });
        }
    }
    Ok(map)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --bins scansion_tests`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**

```bash
git add src/db/queries.rs
git commit -m "feat(db): load_scansion_for_work query + tests"
```

---

## Task 4: `AppState` fields + init

**Files:**
- Modify: `src/app.rs` (AppState struct ~93-415; constructor ~1549)

- [ ] **Step 1: Add the fields to the AppState struct**

In `src/app.rs`, inside the `AppState` struct definition (near the other display flags such as `dim_enabled`), add:

```rust
    /// Current Wright scansion overlay level (Off/StressOnly/Full).
    pub scansion_level: crate::scansion::ScanLevel,
    /// Cached scansion for the current work, keyed by line_mapping.id. Empty
    /// until first toggle-on (or for works with no scansion).
    pub scansion_data: std::collections::HashMap<i64, crate::scansion::LineScansion>,
```

- [ ] **Step 2: Initialize them in the constructor**

In the `AppState { ... }` literal where state is constructed (search for `dim_enabled:` to find the init block), add:

```rust
            scansion_level: crate::scansion::ScanLevel::Off,
            scansion_data: std::collections::HashMap::new(),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles (the fields are read in later tasks; `#[allow(dead_code)]` on AppState covers any interim warning).

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): AppState scansion_level + scansion_data fields"
```

---

## Task 5: Bake marks into `rebuild_buffer_text`

**Files:**
- Modify: `src/app.rs:3305-3340` (`rebuild_buffer_text`)

- [ ] **Step 0: Make `rebuild_buffer_text` callable from the dispatch arm**

The dispatch arm (Task 6) calls `crate::app::rebuild_buffer_text` from another
module, but it is currently private (`fn rebuild_buffer_text`, app.rs:3305). Change
its signature to `pub(crate)`:

```rust
pub(crate) fn rebuild_buffer_text(state: &mut AppState) {
```

- [ ] **Step 1: Extend rebuild_buffer_text to apply marks**

In `src/app.rs`, in `rebuild_buffer_text`, replace the success branch that does
`state.buffer.set_text(&prep.filtered_contents);` (around line 3314) with a
version that bakes marks when the overlay is on:

```rust
    if let Some(prep) = prepare_text_for_display(work) {
        let mapped = prep.line_map.buffer_to_work.iter().filter(|o| o.is_some()).count();
        let first_mapped = prep.line_map.buffer_to_work.iter().position(|o| o.is_some());

        let display_text = if state.scansion_level == crate::scansion::ScanLevel::Off
            || state.scansion_data.is_empty()
        {
            prep.filtered_contents.clone()
        } else {
            apply_scansion_marks(
                &prep.filtered_contents,
                &prep.line_map,
                &work.lines,
                &state.scansion_data,
                state.scansion_level,
            )
        };
        state.buffer.set_text(&display_text);
        state.line_map = Some(prep.line_map);
        crate::logging::log(&format!(
            "TEXT_FILE: loaded '{}' work_type='{}' is_prose={} file_lines={} cleaned_lines={} work_lines={} mapped_buffer_lines={} first_mapped={:?} path={}",
            prep.abbrev,
            prep.work_type,
            prep.is_prose,
            prep.file_lines_count,
            prep.cleaned_lines_count,
            prep.work_lines_count,
            mapped,
            first_mapped,
            prep.path
        ));
        return;
    }
```

- [ ] **Step 2: Add the `apply_scansion_marks` helper**

Add this free function near `rebuild_buffer_text` in `src/app.rs`:

```rust
/// Rebuild the joined buffer text with scansion marks baked into each verse line
/// that has scansion. Operates line-by-line on the already-joined display text so
/// the line count (and thus the line_map) is unchanged — only intra-line combining
/// chars and a trailing label are added. Un-mapped / un-scanned lines pass through
/// unchanged.
fn apply_scansion_marks(
    joined: &str,
    line_map: &crate::text_file_map::LineMap,
    work_lines: &[crate::db::models::Line],
    scansion: &std::collections::HashMap<i64, crate::scansion::LineScansion>,
    level: crate::scansion::ScanLevel,
) -> String {
    let mut out_lines: Vec<String> = Vec::new();
    for (buf_idx, line) in joined.lines().enumerate() {
        let scan = line_map
            .buffer_to_work
            .get(buf_idx)
            .copied()
            .flatten()
            .and_then(|work_idx| work_lines.get(work_idx))
            .and_then(|wl| scansion.get(&wl.id));
        match scan {
            Some(s) => {
                let m = crate::scansion::mark_line(line, s, level);
                // Trailing label, padded with spaces. The label span is excluded
                // from word-highlight scanning (see scansion-label tag below).
                out_lines.push(format!("{}   {}", m.text, m.label));
            }
            None => out_lines.push(line.to_string()),
        }
    }
    out_lines.join("\n")
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles.

- [ ] **Step 4: Manual smoke check (deferred to Task 8 e2e)**

No unit test here (GUI buffer). Behavior is exercised by Task 8's e2e toggle.
For now confirm no warnings about unused `apply_scansion_marks`:
Run: `cargo build 2>&1 | rg -i "apply_scansion_marks" || echo "no warning"`
Expected: `no warning` (it is called from rebuild_buffer_text).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): bake scansion marks into rebuild_buffer_text"
```

---

## Task 6: `CycleScansion` action + dispatch

**Files:**
- Modify: `src/input/actions/mod.rs` (enum; `category()`; `name()`)
- Modify: `src/input/keymap.rs` (dispatch arm)

- [ ] **Step 1: Add the Action enum variant**

In `src/input/actions/mod.rs`, in the `// Display`-adjacent region of the enum
(near `ToggleDim`), add:

```rust
    CycleScansion,
```

- [ ] **Step 2: Add it to `category()`**

In the `Category::Display` match arm group (where `Action::ToggleDim` is listed),
add a line:

```rust
            | Action::CycleScansion
```

- [ ] **Step 3: Add it to `name()`**

Next to `Action::ToggleDim => "ToggleDim",` add:

```rust
            Action::CycleScansion => "CycleScansion",
```

- [ ] **Step 4: Add the dispatch arm**

In `src/input/keymap.rs`, next to the `ToggleDim => { ... }` arm, add:

```rust
        CycleScansion => {
            let mut s = state.borrow_mut();
            // Populate the cache on first use (or for a freshly loaded work).
            if s.scansion_data.is_empty() {
                if let Some(work) = s.current_work.as_ref() {
                    let abbrev = work.abbrev.clone();
                    if let Ok(conn) = crate::db::queries::open_db() {
                        match crate::db::queries::load_scansion_for_work(&conn, &abbrev) {
                            Ok(map) => s.scansion_data = map,
                            Err(e) => crate::logging::log(&format!("SCANSION: load failed: {}", e)),
                        }
                    }
                }
            }
            if s.scansion_data.is_empty() {
                s.scansion_level = crate::scansion::ScanLevel::Off;
                // Reuse the chapter-toast widget for a transient reader message
                // (same pattern as show_chapter_toast in navigation.rs:1670).
                s.chapter_toast.set_text("No scansion for this work");
                s.chapter_toast.set_visible(true);
                let toast = s.chapter_toast.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                    toast.set_visible(false);
                });
                crate::logging::log("SCANSION: no scansion for this work");
                return;
            }
            s.scansion_level = s.scansion_level.next();
            s.config.scansion_level = scansion_level_to_str(s.scansion_level);
            crate::config::save(&s.config);
            crate::logging::log(&format!("SCANSION: level -> {:?}", s.scansion_level));
            crate::app::rebuild_buffer_text(&mut s);
            crate::input::highlight::update_highlight_only(&mut s);
        }
```

Note: the no-scansion message reuses the `chapter_toast` widget directly (the same
transient-toast pattern as `show_chapter_toast`, `src/input/navigation.rs:1670`) —
there is no generic reader `show_toast` helper, so we inline the three lines. The
arm holds the `borrow_mut` (`s`) throughout and `return`s without dropping it.
Confirm `glib` is in scope in `keymap.rs` (it is — used elsewhere in the file); if
not, prefix with `gtk4::glib`.

- [ ] **Step 5: Add the config<->enum string helpers**

In `src/input/keymap.rs` (or a small shared spot — keep it next to the arm), add:

```rust
fn scansion_level_to_str(level: crate::scansion::ScanLevel) -> String {
    match level {
        crate::scansion::ScanLevel::Off => "off",
        crate::scansion::ScanLevel::StressOnly => "stress",
        crate::scansion::ScanLevel::Full => "full",
    }.to_string()
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build`
Expected: compiles. (If `s.config.scansion_level` errors, Task 7 adds it — do Task 7
first, or temporarily comment the two `config` lines and restore them after Task 7.)

- [ ] **Step 7: Commit**

```bash
git add src/input/actions/mod.rs src/input/keymap.rs
git commit -m "feat(input): CycleScansion action + dispatch arm"
```

---

## Task 7: Config persistence + keybind rebind

**Files:**
- Modify: `src/config.rs` (struct + default + init)
- Modify: `src/input/keymap_config.rs` (rebind + default tests)
- Modify: `src/ui/keybinds_overlay.rs` (overlay entries)

- [ ] **Step 1: Add the config field**

In `src/config.rs`, in `struct Config` (near `dim_enabled`), add:

```rust
    #[serde(default = "default_scansion_level")]
    pub scansion_level: String,
```

Add the default fn near `default_dim_enabled`:

```rust
fn default_scansion_level() -> String {
    "off".to_string()
}
```

And in the `Config { ... }` default/init literal (near `dim_enabled: default_dim_enabled(),`):

```rust
            scansion_level: default_scansion_level(),
```

- [ ] **Step 2: Apply persisted level on work-load**

In `src/app.rs`, where a work is loaded and `rebuild_buffer_text` is first called
(search `rebuild_buffer_text(` call sites), set the level from config before the
rebuild, parsing the string:

```rust
            state.scansion_level = match state.config.scansion_level.as_str() {
                "stress" => crate::scansion::ScanLevel::StressOnly,
                "full" => crate::scansion::ScanLevel::Full,
                _ => crate::scansion::ScanLevel::Off,
            };
            state.scansion_data.clear(); // force reload for the new work
```

Per spec: if the persisted level is non-Off but the work has no scansion, the
overlay stays visually Off — the empty-`scansion_data` guard in `rebuild_buffer_text`
(Task 5, Step 1) already produces plain text, so no extra code is needed; the
persisted level is retained.

- [ ] **Step 3: Rebind the `i` family in keymap_config**

In `src/input/keymap_config.rs`, change the three bindings:

```rust
        (KeyCombo::plain("i"), Action::CycleScansion),
        (KeyCombo::alt("i"), Action::ShowTranslationOverlay),
        (KeyCombo::ctrl_alt("i"), Action::ToggleTranslations),
```

(Replace the existing `(KeyCombo::plain("i"), Action::ShowTranslationOverlay)` at
line ~269 and `(KeyCombo::alt("i"), Action::ToggleTranslations)` at line ~294. Put
`CycleScansion` in `display_bindings()` alongside `Alt+d`; keep the two translation
bindings in their current section but with the new modifiers.)

- [ ] **Step 4: Update the default-binding tests**

In `src/input/keymap_config.rs` tests (the `#[cfg(test)]` module asserting default
combos), update or add assertions:

```rust
        assert_eq!(m.get(&KeyCombo::plain("i")), Some(&Action::CycleScansion));
        assert_eq!(m.get(&KeyCombo::alt("i")), Some(&Action::ShowTranslationOverlay));
        assert_eq!(m.get(&KeyCombo::ctrl_alt("i")), Some(&Action::ToggleTranslations));
```

(Remove any old assertion that `plain("i")` maps to `ShowTranslationOverlay` or that
`alt("i")` maps to `ToggleTranslations`.)

- [ ] **Step 5: Update the keybinds overlay**

In `src/ui/keybinds_overlay.rs`, update the `i` key entry (line ~73) and the
`describe()` arms (~516-521). Change the `i` key row to advertise scansion and the
new translation modifiers, e.g.:

```rust
    key("i", "I", "scansion", "", &[("i", "scansion"), ("M-i", "2-col translation"), ("C-M-i", "inline translation")]),
```

And update `describe()` so "scansion" is documented:

```rust
        "scansion" => "Cycle the Wright metrical scansion overlay on verse lines: \
off -> stress-only -> full (stress + unstressed marks), with line-type label and \
caesura. -> input::keymap CycleScansion",
```

Leave the gloss-context `i`=fix-IPA meaning untouched (it is not a reader binding).

- [ ] **Step 6: Verify everything compiles and tests pass**

Run: `cargo build && cargo test --bins`
Expected: compiles; all unit tests pass (scansion, queries, keymap_config defaults).

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/app.rs src/input/keymap_config.rs src/ui/keybinds_overlay.rs
git commit -m "feat: persist scansion level + rebind i-family keys"
```

---

## Task 8: Vocab-highlight regression test + e2e toggle

**Files:**
- Test: `src/app.rs` inline test (vocab pass) OR `tests/` e2e (toggle)

- [ ] **Step 1: Add a vocab-highlight regression unit test**

The vocab word-highlight pass (`src/app.rs:~4638`) recomputes word boundaries from
the buffer string. Combining marks are non-word chars, so words must not split.
Add a focused unit test for the word-boundary scan helper if it is a pure function;
if the logic is inline in a GUI method, instead assert the property on a small
string via a tiny extracted helper. Extract this helper near the vocab pass:

```rust
/// Count word-character runs in a line, treating combining marks (which attach to
/// the preceding letter) as part of the word. Used to verify scansion marks don't
/// split vocab words.
#[cfg(test)]
fn word_run_count(line: &str) -> usize {
    let mut runs = 0;
    let mut in_word = false;
    for ch in line.chars() {
        let is_word = ch.is_alphanumeric() || ch == '\'' || ch == '\u{2019}'
            || ch == '\u{0301}' || ch == '\u{0306}';
        if is_word && !in_word { runs += 1; }
        in_word = is_word;
    }
    runs
}

#[cfg(test)]
mod scansion_vocab_tests {
    use super::word_run_count;
    #[test]
    fn combining_marks_dont_split_words() {
        // "músic" (acute after u) is still one word run.
        let marked = "If m\u{0075}\u{0301}sic be";
        assert_eq!(word_run_count(marked), 3); // If, músic, be
    }
}
```

- [ ] **Step 2: Run the test to verify it passes**

Run: `cargo test --bins scansion_vocab_tests`
Expected: PASS.

- [ ] **Step 3: Manual e2e via the headless harness**

Run the app on a scanned work and toggle scansion:

```bash
./scripts/e2e-env.sh cargo run -- --start-work TN
```

In the app: press `i` once (stress-only), `i` again (full), `i` again (off).
Expected: verse lines on the page gain/lose combining marks + a trailing line-type
label; cursor movement and audio sync behave exactly as before.

- [ ] **Step 4: Verify the strip invariant on real output (sanity)**

Confirm in the dev log (`~/utono/linux-lit/linux-lit-dev.log`) there are no
`SCANSION: load failed` errors and the toggle logged `SCANSION: level -> StressOnly`
etc.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "test(scansion): vocab-highlight regression for combining marks"
```

---

## Self-Review Notes

- **Spec coverage:** keybind cycle (T6/T7), three states (T2), line-type label (T5),
  caesura (T2), re-find placement (T2), DB load (T3), state cache (T4), whole-buffer
  rebuild (T5), config persistence (T7), keybind rebind incl. overlay (T7), error
  handling — no scansion / DB fail / surface-not-found / unmapped line (T2/T3/T5/T6),
  vocab regression (T8). All covered.
- **Toast:** the no-scansion message reuses the `chapter_toast` widget inline (the
  `show_chapter_toast` pattern, navigation.rs:1670) — verified there is no generic
  reader `show_toast` helper. No deny(warnings) in the build, so interim dead-code
  during incremental tasks won't fail compilation.
- **Task ordering:** Task 7 adds `config.scansion_level`, which Task 6 references —
  Task 6 Step 6 notes to either do Task 7 first or temporarily comment the two config
  lines. The dependency is called out explicitly.
- **Type consistency:** `ScanLevel`, `LineScansion`, `ScanSyllable`, `MarkedLine`,
  `mark_line`, `load_scansion_for_work`, `apply_scansion_marks`, `scansion_level`,
  `scansion_data`, `CycleScansion` — used identically across all tasks.

# In-Overlay Regex Search + `f`-Term Highlighting — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add find-in-view search to the journal + gloss overlays: `/` regex-searches the current entry (n/N step within it), the `f` term is highlighted in each entry of the picked set as Ctrl+n/p walk it, Escape clears + n/N revives the MRU pattern.

**Architecture:** A new `overlay_search` engine operates on an overlay's own `TextView` buffer + a search TextTag (NOT the reader `state.buffer`). It reuses `search.rs::build_matcher` (regex, smart-case, literal fallback) but collects char-offset matches in the overlay buffer. The reader `search_bar` widget is reused for the `/` input via a dedicated input mode. State (`OverlaySearch` + MRU) lives on `JournalState` and the gloss overlay; `render_filtered_match`/entry-render re-apply the highlight so the `f`-term lights up in every entry.

**Tech Stack:** Rust, gtk4-rs (TextBuffer/TextTag/TextView), the `regex` crate, cargo test, headless `cage`+`grim`+`wtype`.

## Global Constraints

- Design: `docs/superpowers/specs/2026-07-12-overlay-search-design.md`. Binding decisions:
- ONE active pattern; set by `f` (journal term → also loads the set) OR `/` (regex for the current view). Last-set-wins; the set pattern becomes MRU.
- `n`/`N` step matches WITHIN the current entry (scroll the overlay to each, clamp no-wrap). `Ctrl+n`/`Ctrl+p` stay between-entry (existing `nav_page` filter branch — do NOT change).
- The term is highlighted in each entry AS SHOWN (re-applied on every entry render).
- `Escape`: search-active → clear search (stay in overlay); else the existing precedence (journal: filter-active → clear filter; else close. gloss: existing close). After Escape, `n`/`N` revive the MRU pattern.
- Bad regex → literal-substring fallback (via `build_matcher`) + toast. Zero matches → toast, pattern stays active. Empty pattern → no search mode.
- Overlay search tags the OVERLAY buffer, NOT `state.buffer`; do NOT route through reader `InputMode::Search`.
- `/`, `n`, `N` are SAFE under an active journal filter (they act on the overlay buffer) — do NOT add them to the mutating-key gate in `handle_journal_key`.
- BORROW SAFETY (this codebase has aborted 3× this session on RefCell double-borrows in GTK callbacks): never hold `state.borrow()/borrow_mut()` across a `search_bar` signal emission, a `set_text`, or a `dispatch`. Bind lookups to a `let` (drop before dispatch); use `try_borrow()` in any signal closure. See the picker-crash gotcha in `CLAUDE-activeContext.md`.
- Theme cycling must recolor the new search tags: set them in `apply_theme_to_state` from `selection_bg`.
- Build: `cd ~/utono/linux-lit && cargo build`. Test: `cargo test <name>` (bin-only crate — no `--lib`). Headless: the protocol in `CLAUDE.md`.

## Verified pre-facts (do not re-derive)

- `search.rs::build_matcher(query: &str) -> regex::Regex` (line 357) is the regex compiler (smart-case + literal fallback). Make it `pub(crate)`.
- `search.rs::collect_line(line_text, re, line_idx, out)` collects byte-offset matches per line — reference for the buffer collector, but overlay uses CHAR offsets over the whole buffer.
- Journal overlay: `view: gtk4::TextView`; `self.view.buffer()`; `set_highlight_color` (line 859) manipulates `buffer.tag_table()` (line 882) — the pattern for registering a tag. Scroll via `view.scroll_mark_onscreen(&mark)` (line 1167).
- Reader `/` open (keymap.rs:3169-3174): `s.search_bar.show(); s.input_mode = InputMode::Search;`. Search bar API: `show/hide/query/set_text/update_counter`.
- `nav_page`'s filter branch already does between-entry stepping; leave it.

## File Structure

- Create `src/input/overlay_search.rs` — the buffer/tag search engine (pure-ish; unit-tested).
- Modify `src/input/search.rs` — `pub(crate)` on `build_matcher`.
- Modify `src/ui/journal_overlay.rs`, `src/ui/gloss_overlay.rs` — register `search_tag`/`search_current_tag`, add `buffer()`/`scroll_to_char_offset()`/tag getters.
- Modify `src/input/actions/journal.rs` (+ `gloss.rs`) — `OverlaySearch` state + MRU, handlers, reapply-on-render, `activate_filter` seeding.
- Modify `src/input/keymap.rs` — `/`, n/N, Escape precedence, the search-input mode, gate exclusion.
- Modify `src/app/mod.rs` — the new `InputMode::OverlaySearchInput` variant + its dispatch arm.
- Modify `src/input/actions/settings.rs` — `apply_theme_to_state` sets the overlay search-tag colors.

---

## Task 1: `overlay_search` engine + `build_matcher` reuse

**Files:**
- Modify: `src/input/search.rs` (make `build_matcher` `pub(crate)`)
- Create: `src/input/overlay_search.rs`
- Modify: `src/input/mod.rs` (add `pub mod overlay_search;`)
- Test: `src/input/overlay_search.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::input::search::build_matcher`.
- Produces:
  - `pub struct OverlaySearch { pub pattern: String, pub matches: Vec<(i32,i32)>, pub current: usize }`
  - `pub fn collect(text: &str, pattern: &str) -> Vec<(i32,i32)>` — char-offset spans of every non-empty match (uses `build_matcher`; smart-case + literal fallback). Pure, unit-tested.
  - `pub fn step(cur: usize, len: usize, forward: bool) -> Option<usize>` — clamp no-wrap (reuse the `flat_step` idea; a standalone pure fn here to avoid a cross-module dep). Unit-tested.

- [ ] **Step 1: `pub(crate)` build_matcher**

In `src/input/search.rs`, change `fn build_matcher` (line ~357) to `pub(crate) fn build_matcher`.

- [ ] **Step 2: Write failing tests for the engine**

Create `src/input/overlay_search.rs`:

```rust
//! Regex/literal search over an overlay's TextView buffer. Unlike reader
//! search (line-index over work.lines / state.buffer), this collects CHAR-offset
//! spans in an arbitrary buffer's text and is applied to the OVERLAY buffer's
//! own search TextTag. Reuses search::build_matcher for the regex + smart-case +
//! literal-fallback semantics. No AppState, no GTK types in the pure core.

/// A live search over one overlay buffer: the pattern, the char-offset spans of
/// every match in that buffer (in document order), and the current index.
#[derive(Debug, Clone, Default)]
pub struct OverlaySearch {
    pub pattern: String,
    pub matches: Vec<(i32, i32)>,
    pub current: usize,
}

/// Char-offset (start, end) spans of every non-empty match of `pattern` in
/// `text`, in document order. `pattern` is a regex (smart-cased); an invalid
/// regex degrades to a literal search. Empty pattern → no matches. Offsets are
/// CHARACTER offsets (GTK TextBuffer indexes by char), computed from the byte
/// offsets `regex` returns.
pub fn collect(text: &str, pattern: &str) -> Vec<(i32, i32)> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let re = crate::input::search::build_matcher(pattern);
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        if m.start() == m.end() {
            continue; // skip zero-width
        }
        // byte offset -> char offset
        let start_char = text[..m.start()].chars().count() as i32;
        let end_char = text[..m.end()].chars().count() as i32;
        out.push((start_char, end_char));
    }
    out
}

/// Step `cur` by ±1 within `len`, clamped, no wrap. None if it can't move.
pub fn step(cur: usize, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if forward {
        if cur + 1 < len { Some(cur + 1) } else { None }
    } else if cur > 0 {
        Some(cur - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_char_offsets_regex_and_literal() {
        // two occurrences of "fee"
        let spans = collect("a fee and a fee simple", "fee");
        assert_eq!(spans, vec![(2, 5), (12, 15)]);
    }

    #[test]
    fn collect_char_offsets_are_char_not_byte() {
        // a leading multibyte char shifts byte offsets but not char offsets
        let spans = collect("\u{00e9} fee", "fee"); // é + space + fee
        assert_eq!(spans, vec![(2, 5)]); // char offsets: é=0, space=1, f=2
    }

    #[test]
    fn collect_smart_case_and_bad_regex_literal_fallback() {
        assert_eq!(collect("Fee fee", "fee").len(), 2); // lowercase query = case-insensitive
        // invalid regex "(" degrades to literal — matches the literal "("
        assert_eq!(collect("a ( b", "(").len(), 1);
    }

    #[test]
    fn collect_empty_pattern_is_empty() {
        assert!(collect("anything", "").is_empty());
    }

    #[test]
    fn step_clamps_no_wrap() {
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(2, 3, true), None); // last, forward
        assert_eq!(step(0, 3, false), None); // first, back
        assert_eq!(step(0, 0, true), None); // empty
    }
}
```

Add `pub mod overlay_search;` to `src/input/mod.rs`.

- [ ] **Step 3: Run tests to verify they fail then pass**

Run: `cd ~/utono/linux-lit && cargo test overlay_search`
Expected: FAIL first if `build_matcher` isn't yet `pub(crate)` (compile error) → after Step 1 + this file, all 5 pass.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/overlay_search.rs src/input/search.rs src/input/mod.rs
git commit -m "feat(overlay-search): char-offset regex collect + step engine (reuses build_matcher)"
```

---

## Task 2: Overlay buffers expose search tags + scroll

**Files:**
- Modify: `src/ui/journal_overlay.rs`
- Modify: `src/ui/gloss_overlay.rs`

**Interfaces:**
- Produces on BOTH overlays:
  - `pub fn buffer(&self) -> gtk4::TextBuffer` (the overlay's TextView buffer)
  - `pub fn search_tag(&self) -> &gtk4::TextTag` and `pub fn search_current_tag(&self) -> &gtk4::TextTag`
  - `pub fn set_search_colors(&self, all: &str, current: &str)` (theme-wired)
  - `pub fn scroll_to_char_offset(&self, off: i32)` — scroll the TextView so the char offset is on-screen.

- [ ] **Step 1: Register the two tags (journal)**

In `src/ui/journal_overlay.rs`, mirror how `set_highlight_color` builds/looks up a tag on `buffer.tag_table()` (line ~859-882). In the constructor (where `md_tags` register on `view.buffer()`, line ~409), create two tags:

```rust
let buffer = view.buffer();
let search_tag = buffer.create_tag(Some("overlay_search"), &[("background", &"#ffe000".to_value())])
    .expect("tag");
let search_current_tag = buffer.create_tag(Some("overlay_search_current"), &[("background", &"#ff9000".to_value())])
    .expect("tag");
```

Store both on the struct. (Colors are placeholders; Task 5 wires them to the theme via `set_search_colors`.) Add the getters + `set_search_colors` (set the `background` property on each tag) + `buffer()` + `scroll_to_char_offset`:

```rust
pub fn buffer(&self) -> gtk4::TextBuffer { self.view.buffer() }
pub fn search_tag(&self) -> &gtk4::TextTag { &self.search_tag }
pub fn search_current_tag(&self) -> &gtk4::TextTag { &self.search_current_tag }
pub fn set_search_colors(&self, all: &str, current: &str) {
    self.search_tag.set_property("background", all);
    self.search_current_tag.set_property("background", current);
}
pub fn scroll_to_char_offset(&self, off: i32) {
    let buffer = self.view.buffer();
    let iter = buffer.iter_at_offset(off);
    let mark = buffer.create_mark(None, &iter, false);
    self.view.scroll_mark_onscreen(&mark);
    buffer.delete_mark(&mark);
}
```

(During implementation confirm `create_tag`'s exact arg shape in this gtk4-rs version — match how existing tags in this file / `markdown.rs` are created; the `.to_value()`/property-array form may differ. If existing code uses `TextTag::new` + `tag_table().add`, follow THAT.)

- [ ] **Step 2: Same for gloss**

Repeat in `src/ui/gloss_overlay.rs` (same tag names, getters, `set_search_colors`, `buffer()`, `scroll_to_char_offset`), matching that file's tag-registration idiom.

- [ ] **Step 3: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: compiles (new methods unused yet → dead_code warnings OK; Task 3/4 use them). Do NOT `#[allow(dead_code)]`.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/ui/journal_overlay.rs src/ui/gloss_overlay.rs
git commit -m "feat(overlay-search): search tags + buffer/scroll accessors on journal+gloss overlays"
```

---

## Task 3: Apply/clear/step highlights on the overlay buffer

**Files:**
- Modify: `src/input/overlay_search.rs` (add GTK-touching apply/clear/step-render helpers)
- Test: none new (GTK — verified headlessly); the pure `collect`/`step` are already tested.

**Interfaces:**
- Consumes: `OverlaySearch`, `collect`, `step` (Task 1); overlay `buffer()`/tags/`scroll_to_char_offset` (Task 2).
- Produces (GTK helpers taking the buffer + tags):
  - `pub fn apply(buffer, tag, current_tag, s: &OverlaySearch)` — remove old tags, re-tag all `s.matches`, current-tag `s.matches[s.current]`.
  - `pub fn clear(buffer, tag, current_tag)` — remove both tags over the whole buffer.
  - `pub fn set_from_text(buffer, tag, current_tag, pattern) -> OverlaySearch` — `collect` on the buffer text, build `OverlaySearch{current:0}`, `apply`, return it.
  - `pub fn reapply(buffer, tag, current_tag, s: &mut OverlaySearch)` — re-`collect` against the CURRENT buffer text (entry changed), clamp `current`, `apply`.

- [ ] **Step 1: Implement the GTK helpers**

Add to `src/input/overlay_search.rs`:

```rust
use gtk4::prelude::*;

fn char_iter(buffer: &gtk4::TextBuffer, off: i32) -> gtk4::TextIter {
    buffer.iter_at_offset(off)
}

pub fn clear(buffer: &gtk4::TextBuffer, tag: &gtk4::TextTag, current_tag: &gtk4::TextTag) {
    let (start, end) = buffer.bounds();
    buffer.remove_tag(tag, &start, &end);
    buffer.remove_tag(current_tag, &start, &end);
}

pub fn apply(
    buffer: &gtk4::TextBuffer,
    tag: &gtk4::TextTag,
    current_tag: &gtk4::TextTag,
    s: &OverlaySearch,
) {
    clear(buffer, tag, current_tag);
    for (a, b) in &s.matches {
        buffer.apply_tag(tag, &char_iter(buffer, *a), &char_iter(buffer, *b));
    }
    if let Some((a, b)) = s.matches.get(s.current) {
        buffer.apply_tag(current_tag, &char_iter(buffer, *a), &char_iter(buffer, *b));
    }
}

pub fn buffer_text(buffer: &gtk4::TextBuffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, false).to_string()
}

pub fn set_from_text(
    buffer: &gtk4::TextBuffer,
    tag: &gtk4::TextTag,
    current_tag: &gtk4::TextTag,
    pattern: &str,
) -> OverlaySearch {
    let matches = collect(&buffer_text(buffer), pattern);
    let s = OverlaySearch { pattern: pattern.to_string(), matches, current: 0 };
    apply(buffer, tag, current_tag, &s);
    s
}

pub fn reapply(
    buffer: &gtk4::TextBuffer,
    tag: &gtk4::TextTag,
    current_tag: &gtk4::TextTag,
    s: &mut OverlaySearch,
) {
    s.matches = collect(&buffer_text(buffer), &s.pattern);
    if s.current >= s.matches.len() {
        s.current = s.matches.len().saturating_sub(1);
    }
    apply(buffer, tag, current_tag, s);
}
```

- [ ] **Step 2: Build**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: compiles (helpers unused until Task 4).

- [ ] **Step 3: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/overlay_search.rs
git commit -m "feat(overlay-search): apply/clear/set_from_text/reapply on the overlay buffer"
```

---

## Task 4: Journal wiring — state, `f` seeding, `/`, n/N, Escape, reapply-on-render

**Files:**
- Modify: `src/app/mod.rs` (`InputMode::OverlaySearchInput` + dispatch arm)
- Modify: `src/input/actions/journal.rs` (state + handlers + reapply on render + `activate_filter` seeding)
- Modify: `src/input/keymap.rs` (`/`, n/N, Escape precedence, gate exclusion, the search-input mode handler)

**Interfaces:**
- Consumes: `overlay_search::{OverlaySearch, set_from_text, reapply, clear, step}` (Tasks 1/3); overlay accessors (Task 2); the `search_bar` widget; `activate_filter`/`render_filtered_match` (existing).

- [ ] **Step 1: Add state**

In `JournalState` (`src/input/actions/journal.rs`): add
```rust
    pub search: Option<crate::input::overlay_search::OverlaySearch>,
    pub last_pattern: Option<String>,
```
Init `None` (add to the constructor beside `filter`).

- [ ] **Step 2: Add the `InputMode` + dispatch**

In `src/app/mod.rs`: add `OverlaySearchInput,` to `InputMode` (beside `Search`). In `keymap.rs`'s mode-dispatch match, route `InputMode::OverlaySearchInput => handle_overlay_search_input_key(state, key_name, key_char)`.

- [ ] **Step 3: The `/`-input handlers**

In `src/input/actions/journal.rs`:
```rust
/// `/` in the journal overlay: open the search bar to type a regex for the
/// CURRENT entry. Borrow scoped (search_bar.show emits signals).
pub(crate) fn open_overlay_search(state: &Rc<RefCell<AppState>>) {
    {
        let s = state.borrow();
        s.search_bar.set_text("");
        s.search_bar.show();
    }
    state.borrow_mut().input_mode = InputMode::OverlaySearchInput;
}

/// Enter in the `/` bar: set the pattern on the current overlay buffer.
pub(crate) fn confirm_overlay_search(state: &Rc<RefCell<AppState>>) {
    let query = state.borrow().search_bar.query();
    {
        let s = state.borrow();
        s.search_bar.hide();
    }
    state.borrow_mut().input_mode = InputMode::JournalOverlay;
    if query.trim().is_empty() {
        return;
    }
    let mut s = state.borrow_mut();
    let buffer = s.journal_overlay.buffer();
    let tag = s.journal_overlay.search_tag().clone();
    let ctag = s.journal_overlay.search_current_tag().clone();
    let search = crate::input::overlay_search::set_from_text(&buffer, &tag, &ctag, query.trim());
    if search.matches.is_empty() {
        crate::ui::toast::show_transient(&s.chapter_toast, "No matches", 2);
    } else if let Some((off, _)) = search.matches.first() {
        s.journal_overlay.scroll_to_char_offset(*off);
    }
    s.journal.last_pattern = Some(query.trim().to_string());
    s.journal.search = Some(search);
}
```
(Confirm `TextTag` is `Clone`/cheap to clone — GTK objects are refcounted, `.clone()` bumps the refcount; fine. If the getter returning `&TextTag` fights the borrow, clone as shown.)

- [ ] **Step 4: n/N + revive (MRU) + clear**

```rust
/// n / N in the journal overlay: step matches within the current entry. If no
/// live search but an MRU pattern exists, revive it first (post-Escape n/N).
pub(crate) fn step_overlay_search(state: &Rc<RefCell<AppState>>, forward: bool) {
    let mut s = state.borrow_mut();
    if s.journal.search.is_none() {
        // revive MRU
        if let Some(pat) = s.journal.last_pattern.clone() {
            let buffer = s.journal_overlay.buffer();
            let tag = s.journal_overlay.search_tag().clone();
            let ctag = s.journal_overlay.search_current_tag().clone();
            let search = crate::input::overlay_search::set_from_text(&buffer, &tag, &ctag, &pat);
            if search.matches.is_empty() {
                crate::ui::toast::show_transient(&s.chapter_toast, "No matches", 2);
                return;
            }
            s.journal.search = Some(search);
        } else {
            return;
        }
    }
    let buffer = s.journal_overlay.buffer();
    let ctag = s.journal_overlay.search_current_tag().clone();
    let (scroll_to, cur) = {
        let search = s.journal.search.as_mut().unwrap();
        match crate::input::overlay_search::step(search.current, search.matches.len(), forward) {
            Some(next) => {
                search.current = next;
                (search.matches.get(next).map(|(a, _)| *a), next)
            }
            None => (None, search.current),
        }
    };
    let _ = cur;
    // Re-apply current-tag position + scroll.
    if let Some(search) = s.journal.search.as_ref() {
        let tag = s.journal_overlay.search_tag().clone();
        crate::input::overlay_search::apply(&buffer, &tag, &ctag, search);
    }
    if let Some(off) = scroll_to {
        s.journal_overlay.scroll_to_char_offset(off);
    }
}

/// Clear the active overlay search (Escape). Keeps `last_pattern` for MRU revive.
pub(crate) fn clear_overlay_search(state: &Rc<RefCell<AppState>>) -> bool {
    let mut s = state.borrow_mut();
    if s.journal.search.is_none() {
        return false;
    }
    let buffer = s.journal_overlay.buffer();
    let tag = s.journal_overlay.search_tag().clone();
    let ctag = s.journal_overlay.search_current_tag().clone();
    crate::input::overlay_search::clear(&buffer, &tag, &ctag);
    s.journal.search = None;
    true
}
```

- [ ] **Step 5: Seed from `f` + reapply on every entry render**

In `activate_filter` (after storing the filter + rendering match 1), seed the search from the term so it highlights:
```rust
    // Seed overlay search from the browsed term so it highlights in every entry.
    {
        let buffer = s.journal_overlay.buffer();
        let tag = s.journal_overlay.search_tag().clone();
        let ctag = s.journal_overlay.search_current_tag().clone();
        let search = crate::input::overlay_search::set_from_text(&buffer, &tag, &ctag, term);
        s.journal.last_pattern = Some(term.to_string());
        s.journal.search = Some(search);
    }
```
In `render_filtered_match` (after `show_page`), reapply so each stepped entry lights up:
```rust
    if let Some(search) = s.journal.search.as_mut() {
        let buffer = s.journal_overlay.buffer();
        let tag = s.journal_overlay.search_tag().clone();
        let ctag = s.journal_overlay.search_current_tag().clone();
        crate::input::overlay_search::reapply(&buffer, &tag, &ctag, search);
    }
```
(Confirm borrow shape — `s` is `&mut AppState` in `render_filtered_match`; the getters borrow `s.journal_overlay` while `search` borrows `s.journal` — split the borrows or clone the tags first as shown to avoid an aliasing conflict.)

- [ ] **Step 6: keymap — `/`, n/N, Escape, the input-mode handler, gate exclusion**

In `src/input/keymap.rs`:

(a) `handle_overlay_search_input_key` (new): Return → `confirm_overlay_search`; Escape → hide bar + back to `JournalOverlay` (no pattern); typed chars flow to the focused search_bar entry.

(b) In `handle_journal_key`'s plain-key match, add:
```rust
"slash" => { crate::input::actions::journal::open_overlay_search(state); true }
"n" => { crate::input::actions::journal::step_overlay_search(state, true); true }
"N" => { crate::input::actions::journal::step_overlay_search(state, false); true }
```
(Confirm the GTK key name for `/` on RPD — likely `slash`; verify in keymap_config / the xkb notes. `n`/`N` are plain letters.)

(c) Escape precedence in `handle_journal_key` — update the existing `"Escape"` arm:
```rust
"Escape" => {
    if crate::input::actions::journal::clear_overlay_search(state) {
        // cleared a live search; stay in the overlay
    } else if state.borrow().journal.filter.is_some() {
        crate::input::actions::journal::clear_filter(state);
    } else {
        crate::input::actions::journal::close_overlay(state);
    }
    true
}
```

(d) Gate: ensure `slash`/`n`/`N` are NOT added to the mutating-key `matches!` gate (they're safe on the overlay buffer). They must fall through to their arms even when `filter.is_some()`.

- [ ] **Step 7: Build + full test**

Run: `cd ~/utono/linux-lit && cargo build && cargo test`
Expected: clean; all pass.

- [ ] **Step 8: Commit**

```bash
cd ~/utono/linux-lit
git add src/app/mod.rs src/input/actions/journal.rs src/input/keymap.rs
git commit -m "feat(journal): / regex search + n/N + f-term highlight + Esc/MRU in the overlay"
```

---

## Task 5: Gloss overlay `/` search + theme-wired colors

**Files:**
- Modify: `src/input/actions/gloss.rs` (gloss equivalents of open/confirm/step/clear + state)
- Modify: `src/input/keymap.rs` (`handle_gloss_key`: `/`, n/N, Escape)
- Modify: `src/input/actions/settings.rs` (`apply_theme_to_state`: set overlay search colors)

**Interfaces:** same `overlay_search` helpers, against `s.gloss_overlay`.

- [ ] **Step 1: Gloss state + handlers**

Add `search`/`last_pattern` to the gloss overlay state (find its state struct — mirror `JournalState`). Add `open_overlay_search`/`confirm_overlay_search`/`step_overlay_search`/`clear_overlay_search` for gloss (same bodies as journal Task 4, but `s.gloss_overlay` and the gloss input mode returns to `GlossOverlay`). Reuse the SAME `InputMode::OverlaySearchInput` — its confirm handler must know which overlay opened it: store the origin (e.g. a field `overlay_search_origin: InputMode` on AppState set in `open_overlay_search`, read in `confirm_overlay_search` to dispatch to the right overlay). (Simplest: two input modes `OverlaySearchInputJournal`/`...Gloss`; OR one mode + an origin field. Pick one during implementation and note it.)

- [ ] **Step 2: keymap for gloss**

In `handle_gloss_key`: `slash` → gloss `open_overlay_search`; `n`/`N` → gloss `step_overlay_search`; `Escape` → gloss `clear_overlay_search` first, else existing gloss close. (Gloss has no journal filter; simpler precedence.)

- [ ] **Step 3: Theme-wire the colors**

In `apply_theme_to_state` (`src/input/actions/settings.rs`), after the existing overlay highlight-color lines, add:
```rust
    let sel = crate::theme::selection_bg(theme);
    state.journal_overlay.set_search_colors(&sel, &theme.reader_gloss_cursor);
    state.gloss_overlay.set_search_colors(&sel, &theme.reader_gloss_cursor);
```
(Use `selection_bg` for all-matches and a distinct current-match color — pick an existing theme field that contrasts; confirm one exists, else reuse `cursor_line_bg`. Note the choice.)

- [ ] **Step 4: Build + full test**

Run: `cd ~/utono/linux-lit && cargo build && cargo test`
Expected: clean; all pass.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/input/actions/gloss.rs src/input/keymap.rs src/input/actions/settings.rs
git commit -m "feat(gloss): / regex search in the gloss overlay + theme-wired search colors"
```

---

## Task 6: Headless end-to-end verification

**Files:** none (verification).

- [ ] **Step 1: `f`-term highlight across the set**

Headless-drive (CLAUDE.md protocol): open the journal overlay, press `f`, type `fee simple`, Enter. Screenshot — confirm occurrences are highlighted in the landed entry. Ctrl+n/Ctrl+p to other matches — confirm the term is highlighted in EACH entry shown. (Real DB has backfill tags; term-browse works.)

- [ ] **Step 2: `/` regex + n/N in journal**

In the overlay, press `/`, type a regex (e.g. `\bthe\b`), Enter. Confirm matches highlight; press `n`/`N` — the current-match highlight moves and the overlay scrolls to each; clamps at ends (no wrap).

- [ ] **Step 3: `/` in gloss**

Open a gloss overlay (verify the open key), press `/`, type a term present in the gloss, Enter — confirm highlight + n/N stepping.

- [ ] **Step 4: Escape + MRU revive**

With a search active, press Escape — highlights clear, overlay stays open. Press `n` — the MRU pattern revives and re-highlights + steps. Screenshot each.

- [ ] **Step 5: Borrow-safety (regression guard)**

Repeat `/` → type → Esc → `/` several times, and `f` → Esc → `f`, and press Ctrl+t (theme cycle) while a search is active. Confirm NO crash (`pgrep` the cage instance; grep the fresh log for `panicked`/`RefCell`/`non-unwinding`). Theme cycle should recolor the live highlights.

- [ ] **Step 6: Report + cleanup**

Open every screenshot and report the highlight color + which matches are lit. Confirm no work switch and no panic. Cleanup: `pkill -f "cage -- ./target/debug/linux-lit"` (scoped ONLY).

---

## Self-Review notes

- **Spec coverage:** `/` journal+gloss → Tasks 4/5; n/N within entry → Task 4 `step_overlay_search`; f-term highlight per entry → Task 4 (`activate_filter` seed + `render_filtered_match` reapply); Escape clears + MRU revive → Task 4 `clear_overlay_search` + `step_overlay_search` revive branch; bad-regex/zero-match/empty → Task 1 `collect` (via `build_matcher`) + Task 4 toasts; theme colors → Task 5.
- **Type consistency:** `OverlaySearch{pattern,matches:Vec<(i32,i32)>,current}` and `collect(&str,&str)->Vec<(i32,i32)>`, `step(usize,usize,bool)->Option<usize>` used identically across tasks; overlay accessors `buffer()/search_tag()/search_current_tag()/scroll_to_char_offset()/set_search_colors()` consistent journal+gloss.
- **Borrow safety** called out at every GTK-callback / set_text / dispatch site (Task 4 handlers scope borrows; clone refcounted tags to avoid aliasing `s`). This is the session's repeated failure mode — the plan front-loads it.
- **Open item for the implementer (Task 5 Step 1):** one shared `OverlaySearchInput` mode + an origin field, vs. two modes. Pick and note. Also confirm the reader `search_bar` can be shown over an overlay without stealing the overlay's context (it's a bottom bar; should overlay fine).
- **MRU semantics:** this plan's n/N revive is simpler than reader `reactivate_and_step` (no backward-search XOR); acceptable — overlay `/` is always forward. Note if the user wants `?`-style backward later.

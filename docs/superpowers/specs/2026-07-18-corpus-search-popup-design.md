# Cross-corpus regex search popup — design

**Date:** 2026-07-18
**Status:** Approved design, ready for implementation plan
**Scope:** One implementation plan.

## Problem

There is no way to search *across* journal Q&A entries or *across* reader-gloss
glosses. Today's search facilities are all single-entry or wrong-shaped:

- **`/` (in-entry find)** — regex over the ONE open journal/gloss entry's text,
  `n`/`N` step matches within it. Real regex via `search::build_matcher`
  (smart-case, literal fallback). Scoped to the current entry only.
- **`f` (journal term filter)** — steps entry-to-entry across works, but matches
  `journal_tags` + FTS5 phrase-query (NOT regex) and has no results-list UI.
- **Gloss bodies** — no free-text search of any kind exists over gloss text.

The user wants a single **fzf-like popup** that searches the whole journal Q&A
corpus OR the whole reader-gloss corpus (toggle-able), with **regex**, a results
list, and a jump-to-entry that highlights the match.

## Goals

- A modal popup opened with **`Ctrl+f`** from the reader, the journal overlay,
  and the gloss overlay.
- Searches **cross-work, whole corpus** — every journal Q&A / every reader-gloss
  gloss in lit.db, regardless of the open work.
- Toggle corpus **journal ↔ gloss** with `Tab`; remembers last-used.
- **Regex + smart-case**, identical semantics to `/` (reuses
  `search::build_matcher`); invalid regex falls back to literal search.
- One **result row per matching entry**, labeled by work + citation + question
  (journal) or speaker (gloss).
- On select: load the work if needed, open that entry's overlay, and **seed the
  overlay's `/` search** with the popup pattern so the match highlights and
  `n`/`N` step within it.

## Non-goals (YAGNI)

- No fuzzy/subsequence mode — regex only (a fuzzy pattern could not seed the
  `/` highlight, which is regex).
- No merged journal+gloss result list — one corpus at a time, toggled.
- No new FTS index / no use of `journal_fts` — regex runs in Rust over loaded
  rows (FTS5 is phrase-query, cross-work-only, cannot do regex).
- No per-match rows — one row per entry (dedup multiple hits in an entry).
- No reader-line jump mode — select always opens the entry overlay.
- The existing `/`, `f`, and journal term-filter behaviors are untouched.

## Architecture

Five components. Only the pure core holds real logic; everything else reuses
existing regex, work-load, overlay-open, and `/`-highlight code.

### 1. Pure search core — `src/input/corpus_search.rs` (new, GTK-free)

Mirrors the pure/GTK split of `overlay_search.rs`. Unit-tested with no GTK.

```rust
#[derive(Clone, Copy, PartialEq)]
pub enum Corpus { Journal, Gloss }

pub struct JournalRow { pub id: i64, pub work_abbrev: String,
    pub div1: i32, pub div2: i32, pub question: String, pub answer: String }

pub struct GlossRow { pub gloss_id: i64, pub work_abbrev: String,
    pub start_citation: String, pub speaker: String, pub gloss_text: String }

pub struct CorpusHit {
    pub corpus: Corpus,
    pub entry_id: i64,        // journal_entries.id  OR glosses.id
    pub work_abbrev: String,
    pub label: String,        // display row text (see Display)
    pub sort_key: (String, i32, i32),  // (work_abbrev, div1/act, div2/scene)
}

/// One hit per row whose (question OR answer) matches. Preserves input order,
/// then callers sort by sort_key.
pub fn filter_journal(rows: &[JournalRow], re: &regex::Regex) -> Vec<CorpusHit>;

/// One hit per row whose gloss_text matches.
pub fn filter_gloss(rows: &[GlossRow], re: &regex::Regex) -> Vec<CorpusHit>;
```

The caller compiles the pattern once with `search::build_matcher(query)` (which
returns a ready `regex::Regex` with smart-case applied) and passes it in. An
empty query yields ALL rows as hits (label-only browse).

### 2. Corpus loaders — `src/db/journal.rs` + `src/db/queries.rs`

- **Journal** (`journal.rs`): `list_all_journal_rows() -> Vec<JournalRow>` —
  `SELECT id, work_abbrev, div1, div2, question, answer FROM journal_entries`
  (all works). Body text already present; no schema change.
- **Gloss** (`queries.rs`): `list_all_gloss_rows() -> Vec<GlossRow>` — the one
  genuine gap. Today `find_glossed_passages` selects citation metadata only and
  never loads `gloss_text`. New query joins `glosses` → `passages`:
  ```sql
  SELECT g.id, p.work_abbrev, p.start_citation, p.character, g.gloss_text
  FROM glosses g JOIN passages p ON p.id = g.passage_id
  WHERE g.gloss_type = 'reader-gloss';
  ```
  (`reader-gloss` is the reader gloss type per `find_glossed_passages`'s filter.)

### 3. Popup widget — `src/ui/corpus_search_popup.rs` (new)

Clone of `src/ui/gloss_picker.rs` (closest minimal template): `Entry` +
`ListBox` + scrim, mounted via `picker_attach::attach_overlay_panel` (an
`add_overlay` layer — NOT in the size-bearing chain, per the picker rule).

Struct holds:
- `corpus: Corpus` and both cached corpora (`Vec<JournalRow>`, `Vec<GlossRow>`)
  loaded once per open so `Tab` re-filters without re-querying.
- current `Vec<CorpusHit>`, selected index.
- header `Label` showing `[journal|GLOSS]` + `(regex)`.

`populate_list(query)` compiles the matcher, filters the active corpus, sorts by
`sort_key`, rebuilds the `ListBox`, auto-selects row 0.

### 4. Input mode + keymap — `InputMode::CorpusSearch`

New modal `InputMode`. `Ctrl+f` dispatches open from three contexts (reader,
journal overlay, gloss overlay). Inside the mode:
- typing → re-filter (via `connect_changed`, see hazard below)
- `Up`/`Down` → move selection (`picker_nav::move_selection_clamped`)
- `Tab` → toggle corpus + re-filter, query text retained
- `Return` → select (§5)
- `Escape` → close, return to opening context

Routed in `keymap.rs` beside the other picker/overlay arms. `Ctrl+f` is
confirmed free in reader defaults AND in the journal/gloss overlay handlers
(only plain `f` is bound, to the term filter; gated on `is_ctrl`).

### 5. Select handler — `src/input/actions/corpus_search.rs` (new)

On `Return` with a selected `CorpusHit`:
1. If `hit.work_abbrev` != current work → load it (existing work-load path).
   Normalize via `Work::canonical_abbrev` (the `-Amb` strip the gloss picker
   load path already applies) before comparing/opening.
2. Open the matching overlay on `hit.entry_id`:
   - Journal → the journal-overlay-open path, positioned to that entry.
   - Gloss → the gloss-overlay-open path, positioned to that gloss.
3. Seed the opened overlay's `/` search: `overlay_search::collect(entry_text,
   pattern)` → the overlay's `set_search_matches` / `reapply_search` (the exact
   calls the `/` find uses). Match highlights; `n`/`N` step within the entry.

## Data flow

**Open (`Ctrl+f`):** record opening context (for Esc return). Corpus =
context's own kind if opened from an overlay, else last-used (persisted on
`AppState`, initial Journal). Load both corpora once. Reset entry to "" (through
the guarded `connect_changed`). Empty query shows all rows.

**Keystroke:** `connect_changed` (guarded) reads query →
`build_matcher(query)` → `filter_*` → sort → `populate_list`, row 0 selected.

**Tab:** flip `corpus`, re-filter the other cached `Vec`, update header, keep
query.

**Return:** close popup → select handler (§5).

**Escape:** close popup, return to opening context. No work-load, no selection.

## Error / edge handling

- **Empty corpus** (no journal entries / no glosses): popup opens with empty
  list + "no entries" hint; `Tab` still switches.
- **Invalid regex** mid-type: `build_matcher` falls back to escaped-literal —
  never an error state.
- **No matches:** empty list; `Return` is a no-op.
- **Work fails to load** on select: toast + stay in reader (mirrors concordance
  cross-work-load failure handling).

## The one mandatory hazard

The popup's `connect_changed` wiring goes in `src/app/mod.rs` (matching the
existing picker convention — signal wiring lives there, not in the picker file)
and MUST use the **`try_borrow()`-guarded** form. `show()`/reset calls
`entry.set_text("")`, which synchronously emits `changed` while the open path
holds `borrow_mut()`; a plain `state.borrow()` there double-borrows → a
non-unwinding abort. This is the documented picker-signal crash class; a naive
clone of `gloss_picker` would reintroduce it.

## Testing

- **Unit (`corpus_search.rs`):** `filter_journal`/`filter_gloss` — regex match
  in question vs answer vs gloss_text; one-hit-per-entry dedup; empty query =
  all rows; smart-case (lower = insensitive, any-upper = sensitive); invalid
  regex → literal via `build_matcher`; sort order by (work, act, scene).
- **DB:** `list_all_journal_rows` / `list_all_gloss_rows` load body text and the
  right columns; gloss query returns `gloss_text` (regression guard against the
  citation-only gap) and filters `gloss_type = 'reader-gloss'`.
- **Headless e2e (cage/grim/wtype):** `Ctrl+f` opens the popup from reader and
  overlays; typing filters; `Tab` toggles the header journal↔gloss; `Return`
  loads the work + opens the entry overlay with the match highlighted; `Escape`
  returns to context. Screenshot-verified per the UI review protocol.

## Key files

- New: `src/input/corpus_search.rs` (pure core),
  `src/ui/corpus_search_popup.rs` (widget),
  `src/input/actions/corpus_search.rs` (open + select).
- Edited: `src/db/journal.rs` (`list_all_journal_rows`),
  `src/db/queries.rs` (`list_all_gloss_rows`),
  `src/input/keymap.rs` (`Ctrl+f` routing + `CorpusSearch` mode arm),
  `src/input/keymap_config.rs` (`Ctrl+f` default bind),
  `src/app/mod.rs` (guarded `connect_changed`, mode plumbing, last-used corpus),
  the `InputMode` enum.
- Reused unchanged: `src/input/search.rs` (`build_matcher`),
  `src/input/overlay_search.rs` (`collect`), `src/ui/picker_nav.rs`,
  `src/ui/picker_attach.rs`.

## Follow-ups (out of scope)

- The Ctrl+/ overlay legends must gain a `Ctrl+f` entry once the bind ships
  (keybinds-overlay + journal/gloss overlay legends) — part of the
  implementation plan's keybind-change checklist, not this design.
- `keymap.json` stowed override must add the same bind (compiled default +
  JSON, or the JSON shadows it).

# Term-filter → Arkangel-source Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reader `f` term filter: Escape returns to the opener (reader vs journal overlay); choosing a term shows the filtered subset; Escape from a filtered *passage* entry jumps to its `<work>-Arkangel` source first line with the Arkangel media in MPV (non-passage entries just close to the reader).

**Architecture:** Three focused changes reusing existing machinery — a `term_input_from_reader` opener flag (mirrors `picker_from_reader`), an opener-aware term-input Escape, and a new `escape_filtered_entry_to_source` that reuses `preferred_arkangel_abbrev` + the corpus-search cross-work load + a citation→buffer-index helper factored out of `jump_to_journal_source_start`.

**Tech Stack:** Rust, GTK4/gtk4-rs, rusqlite, the cage/grim/wtype headless harness.

**Design doc:** `docs/superpowers/specs/2026-07-18-term-filter-arkangel-workflow-design.md`

## Global Constraints

- Bin-only crate: build with `cargo build`, test with `cargo test --bins <name>` (NO `--lib`). Do NOT run the app (`cargo run`); the user launches it. Headless verification uses the cage harness in CLAUDE.md.
- Do NOT touch any user keybind surfaces beyond what a task names; `f` (`OpenJournalTermInput`), `Ctrl+f`, and `Ctrl+c` bindings are already shipped and must stay as-is.
- Borrow discipline: gather entry data under a short borrow, drop it, then load/mutate under fresh borrows; NEVER hold a `state.borrow()`/`borrow_mut()` across an `.await` (mirror `corpus_search::select`).
- Arkangel resolution is `db::queries::preferred_arkangel_abbrev(conn, abbrev)` → `{abbrev}-Arkangel` if the row exists, else `abbrev`. Only ~38 Shakespeare works have an Arkangel edition — the base-work fallback is mandatory.
- The cross-work load must set `s.skip_mpv_discovery = false` before `display_work_at_with_prepared` so MPV discovery loads the target edition's media (the Arkangel `.m4b`), exactly like `corpus_search::select`.
- Existing fns to reuse (do not reimplement): `journal::displayed_journal_page(&AppState) -> Option<JournalPage>`, `journal::clear_filter(&Rc<RefCell<AppState>>)`, `journal::close_overlay(&Rc<RefCell<AppState>>)`, `app::return_to_reader_mode(&mut AppState)`, `app::parse_citation(&str) -> Option<(i64,i64,i64)>`, `app::display_work_at_with_prepared`, `navigation::jump_to_line(&mut AppState, usize)`.
- `JournalPage` has `div1, div2, start_citation: Option<String>, source_text: Option<String>` but NO `work_abbrev`; the filtered match's abbrev is on `TermMatch.work_abbrev` (`s.journal.filter.matches[pos].work_abbrev`).
- Known pre-existing test failure may appear in a broad run (`theme_cycle_defaults_to_reading_themes` from unrelated uncommitted work) — ignore it.
- keymap.json / overlay legends are NOT touched by this plan (no keybind changes).

---

### Task 1: Extract the citation→buffer-index resolver

Factor the source-line resolution out of `jump_to_journal_source_start` so a new caller can resolve an arbitrary entry against a freshly-loaded edition. Pure over a `Work` + citation; unit-tested.

**Files:**
- Modify: `src/input/actions/journal.rs` (add `source_first_buffer_line`, rewrite `jump_to_journal_source_start` to call it)

**Interfaces:**
- Produces:
  `pub(crate) fn source_first_buffer_line(work: &crate::db::models::Work, line_map: Option<&crate::app::LineMap>, start_citation: &str, source_text: &str) -> Option<usize>`
  — resolves the buffer index of the first dialogue line of the passage at `start_citation` (falling back to matching `first_plain_source_line(source_text)` text), or `None`.
- Consumes: `app::parse_citation`, the existing `first_plain_source_line`.

- [ ] **Step 1: Write the failing test**

Confirm the `LineMap` type path first (grep `pub struct LineMap` / `pub type LineMap`); use whatever the crate exposes. In `journal.rs` tests, add a test that builds a tiny `Work` with a couple of `Line`s and asserts resolution. Model the `Work`/`Line` construction on an existing journal.rs or queries.rs test that builds `Work`/`Line` fixtures (grep `Line {` in tests). If no in-module `Work` fixture helper exists, keep the test minimal: assert that a citation matching a line's `(div1,div2,line_in_div)` returns that line's mapped index, and an unresolvable citation returns `None`.

```rust
#[test]
fn source_first_buffer_line_resolves_citation() {
    // Build a Work with lines at (div1,div2,line_in_div); a LineMap that maps
    // work index -> buffer index 1:1. Citation "Cym.5.5.1" resolves to the first
    // dialogue line's buffer index; a bogus citation resolves to None.
    // (Fill in using the crate's Work/Line/LineMap constructors — see existing
    //  fixtures in this file / queries.rs tests.)
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins source_first_buffer_line 2>&1 | tail -15`
Expected: compile error (`source_first_buffer_line` undefined).

- [ ] **Step 3: Implement the resolver + rewire the caller**

Add the pure resolver (the body lifted from `jump_to_journal_source_start:865-902`, generalized to take `work`, `line_map`, `start_citation`, `source_text` as parameters):

```rust
/// Buffer index of the first dialogue line of the passage at `start_citation`
/// within `work`. Primary match is the citation tuple `(div1,div2,line_in_div)`;
/// the fallback matches the first plain source line of `source_text` (which
/// carries `<speaker>/<verse>` markup) against line text. Advances to the first
/// `is_dialogue` line, then maps through `line_map.work_to_buffer`. `None` when
/// the citation/text doesn't resolve.
pub(crate) fn source_first_buffer_line(
    work: &crate::db::models::Work,
    line_map: Option<&crate::app::LineMap>,
    start_citation: &str,
    source_text: &str,
) -> Option<usize> {
    let target = crate::app::parse_citation(start_citation);
    let by_citation = target
        .and_then(|t| work.lines.iter().position(|l| (l.div1, l.div2, l.line_in_div) == t));
    let first_src = first_plain_source_line(source_text);
    let start_idx = by_citation.or_else(|| {
        if first_src.is_empty() {
            None
        } else {
            work.lines.iter().position(|l| l.text.trim() == first_src)
        }
    })?;
    let work_idx = work.lines[start_idx..]
        .iter()
        .position(|l| l.is_dialogue)
        .map(|off| start_idx + off)
        .unwrap_or(start_idx);
    match line_map {
        Some(lm) => lm.work_to_buffer.get(work_idx).copied(),
        None => Some(work_idx),
    }
}
```

Rewrite `jump_to_journal_source_start` to delegate (preserving its exact current behavior — read the current page's citation/source, resolve against `current_work`, jump):

```rust
pub(crate) fn jump_to_journal_source_start(s: &mut AppState) -> bool {
    let (start_citation, source_text) = match s.journal.pages.get(s.journal.page_index) {
        Some(p) => match &p.start_citation {
            Some(c) => (c.clone(), p.source_text.clone().unwrap_or_default()),
            None => return false,
        },
        None => return false,
    };
    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return false,
    };
    let buf_idx = match source_first_buffer_line(
        work, s.line_map.as_ref(), &start_citation, &source_text,
    ) {
        Some(i) => i,
        None => return false,
    };
    crate::input::navigation::jump_to_line(s, buf_idx);
    true
}
```

(Confirm the `Work`, `LineMap`, and `line_map` field types by reading the current `jump_to_journal_source_start` and `AppState` — use the exact paths the file already uses; adjust the signature's type paths to match.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins source_first_buffer_line 2>&1 | tail -6`
Expected: PASS. Then `cargo build 2>&1 | tail -1` — clean (no behavior change to existing callers).

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs
git commit -m "refactor(journal): extract source_first_buffer_line resolver"
```

---

### Task 2: Opener-aware term-input escape

Record whether the term input was opened from the reader; Escape returns to the opener.

**Files:**
- Modify: `src/input/actions/journal.rs` (`JournalState` field + `open_term_input`)
- Modify: `src/input/keymap.rs` (the `JournalTermInput` Escape arm, ~line 526)

**Interfaces:**
- Consumes: `app::return_to_reader_mode`.
- Produces: `JournalState.term_input_from_reader: bool`; `open_term_input` sets it from the current mode.

- [ ] **Step 1: Write the failing test**

`open_term_input` is GTK-touching (it calls `.show()`), so a pure unit test isn't clean. Instead assert the FLAG-DERIVATION rule with a tiny pure helper. Add to `journal.rs`:

```rust
/// True when the term input, opened while in `mode`, should return to the reader
/// on cancel (opened from the reading card) rather than the journal overlay.
pub(crate) fn term_input_opened_from_reader(mode: crate::app::InputMode) -> bool {
    matches!(mode, crate::app::InputMode::Reader)
}
```

Test:

```rust
#[test]
fn term_input_from_reader_only_in_reader_mode() {
    use crate::app::InputMode;
    assert!(term_input_opened_from_reader(InputMode::Reader));
    assert!(!term_input_opened_from_reader(InputMode::JournalOverlay));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bins term_input_from_reader_only_in_reader_mode 2>&1 | tail -12`
Expected: compile error (`term_input_opened_from_reader` undefined).

- [ ] **Step 3: Implement**

Add the helper above. Add the field to `JournalState` (near `picker_from_reader`, ~journal.rs:291):

```rust
/// True when the term input was opened from the READING CARD (reader `f`)
/// rather than inside the journal overlay. Consumed by the term-input Escape
/// path so cancel returns to the reader, not the overlay. Mirrors
/// `picker_from_reader`.
pub term_input_from_reader: bool,
```

Initialize it `false` wherever `JournalState` is constructed (grep the struct literal / `Default`). In `open_term_input` (journal.rs:646), set it as the FIRST statement, from the current mode, before mutating `input_mode`:

```rust
pub(crate) fn open_term_input(state: &Rc<RefCell<AppState>>) {
    let from_reader = term_input_opened_from_reader(state.borrow().input_mode);
    state.borrow_mut().journal.term_input_from_reader = from_reader;
    // ... existing body (load terms, show, set input_mode = JournalTermInput) ...
}
```

In `keymap.rs`, the `JournalTermInput` Escape arm (currently `~line 526`, inside the `PickerAction::Hide` match):

```rust
InputMode::JournalTermInput => {
    s.journal_term_input.hide();
    if s.journal.term_input_from_reader {
        crate::app::return_to_reader_mode(&mut s);
    } else {
        s.input_mode = InputMode::JournalOverlay;
    }
}
```

(Verify `s` is a `borrow_mut()` in that arm — `return_to_reader_mode(&mut AppState)` needs `&mut`. If the arm holds a non-mut borrow, adjust to match how the sibling `JournalMovePicker` arm mutates; read the surrounding code.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bins term_input_from_reader_only_in_reader_mode 2>&1 | tail -6`; `cargo build 2>&1 | tail -1`.
Expected: test PASS, build clean.

- [ ] **Step 5: Commit**

```bash
git add src/input/actions/journal.rs src/input/keymap.rs
git commit -m "feat(journal): term-input Escape returns to opener (reader vs overlay)"
```

---

### Task 3: Escape from a filtered passage entry → Arkangel source

The core behavior. A new async function; wired into the journal-overlay Escape cascade before the clear-filter branch.

**Files:**
- Create/modify: `src/input/actions/journal.rs` (add `escape_filtered_entry_to_source`)
- Modify: `src/input/keymap.rs` (journal-overlay Escape cascade, ~line 1878)
- Read for template: `src/input/actions/corpus_search.rs` (`select`'s spawn_blocking + preferred_arkangel + display + jump), Task 1's `source_first_buffer_line`.

**Interfaces:**
- Consumes: Task 1's `source_first_buffer_line`, `preferred_arkangel_abbrev`, `displayed_journal_page`, `clear_filter`, `close_overlay`, `return_to_reader_mode`, `display_work_at_with_prepared`, `navigation::jump_to_line`.
- Produces: `pub(crate) fn escape_filtered_entry_to_source(state: &Rc<RefCell<AppState>>) -> bool` — returns `true` when it handled a passage entry (started the jump), `false` for a non-passage entry (no citation) so the caller falls back.

- [ ] **Step 1: Implement `escape_filtered_entry_to_source`**

No pure unit test fits (async + GTK + DB); verification is the headless e2e in Task 4. Implement:

```rust
/// Escape from a journal entry shown under an active term filter: if it is a
/// PASSAGE entry (has a source citation), close the overlay + clear the filter,
/// load its `<work>-Arkangel` edition (base if none) with the Arkangel media,
/// and land the cursor on the entry's source first line. Returns `true` when it
/// handled a passage entry; `false` for a non-passage note (no citation) — the
/// caller then falls back to clear-filter + close-to-reader.
pub(crate) fn escape_filtered_entry_to_source(state: &Rc<RefCell<AppState>>) -> bool {
    // Gather the filtered entry's abbrev + citation + source under a short borrow.
    let (base_abbrev, start_citation, source_text, current_abbrev) = {
        let s = state.borrow();
        let Some(filter) = s.journal.filter.as_ref() else { return false };
        let Some(m) = filter.matches.get(filter.pos) else { return false };
        let Some(cite) = m.page.start_citation.clone() else {
            return false; // non-passage note: caller falls back
        };
        (
            m.work_abbrev.clone(),
            cite,
            m.page.source_text.clone().unwrap_or_default(),
            s.current_work.as_ref().map(|w| w.abbrev.clone()),
        )
    };

    // Leave the overlay: clear the filter + close to the reader BEFORE loading.
    crate::input::actions::journal::clear_filter(state);
    crate::input::actions::journal::close_overlay(state);

    let state_clone = std::rc::Rc::clone(state);
    let handle = state.borrow().tokio_handle.clone();
    glib::spawn_future_local(async move {
        let base_for_load = base_abbrev.clone();
        let current_for_load = current_abbrev.clone();
        let result = handle
            .spawn_blocking(move || {
                let conn = crate::db::queries::open_db()
                    .expect(crate::db::queries::OPEN_DB_PANIC_MSG);
                let target = crate::db::queries::preferred_arkangel_abbrev(&conn, &base_for_load);
                if current_for_load.as_deref() == Some(target.as_str()) {
                    return Ok::<_, rusqlite::Error>((target, None));
                }
                let work = crate::db::queries::load_work(&conn, &target)?;
                let prepared = crate::app::text_prep::prepare_text_for_display(&work);
                Ok::<_, rusqlite::Error>((target, Some((work, prepared))))
            })
            .await;
        match result {
            Ok(Ok((_target, None))) => {
                // Already on the target edition: just move the cursor.
                let mut s = state_clone.borrow_mut();
                if let Some(work) = s.current_work.as_ref() {
                    if let Some(buf) = crate::input::actions::journal::source_first_buffer_line(
                        work, s.line_map.as_ref(), &start_citation, &source_text,
                    ) {
                        crate::input::navigation::jump_to_line(&mut s, buf);
                    }
                }
            }
            Ok(Ok((_target, Some((work, prepared))))) => {
                let mut s = state_clone.borrow_mut();
                s.skip_mpv_discovery = false; // let discovery load the Arkangel media
                crate::app::clear_display(&mut s);
                crate::app::display_work_at_with_prepared(&mut s, work, None, prepared);
                // Resolve the source line against the freshly-loaded edition.
                if let Some(w) = s.current_work.as_ref() {
                    // Clone the small pieces needed to drop the immutable borrow
                    // before the mutable jump.
                    let buf = crate::input::actions::journal::source_first_buffer_line(
                        w, s.line_map.as_ref(), &start_citation, &source_text,
                    );
                    if let Some(bi) = buf {
                        crate::input::navigation::jump_to_line(&mut s, bi);
                    }
                }
            }
            _ => {
                let s = state_clone.borrow();
                crate::input::navigation::show_chapter_toast_secs(
                    &s, &format!("Could not load {}", base_abbrev), 3,
                );
            }
        }
    });
    true
}
```

Adjust type paths / borrow scoping to satisfy the borrow checker (the `current_work` immutable borrow must not overlap the `jump_to_line(&mut s)` — restructure with an intermediate `let buf = ...;` computed under the immutable borrow, then drop it before the mutable jump, as written). Match `corpus_search::select`'s exact `tokio_handle`/`open_db`/`display_work_at_with_prepared` usage.

- [ ] **Step 2: Wire into the journal-overlay Escape cascade**

In `keymap.rs`, the `JournalOverlay` Escape handler (~line 1871-1884), replace the `filter.is_some()` branch:

```rust
} else if state.borrow().journal.filter.is_some() {
    // Filtered passage entry -> jump to its Arkangel source; non-passage note
    // (no citation) -> fall back to clearing the filter + returning to reader.
    if !crate::input::actions::journal::escape_filtered_entry_to_source(state) {
        crate::input::actions::journal::clear_filter(state);
        crate::input::actions::journal::close_overlay(state);
    }
}
```

(Read the current cascade to place this correctly relative to the rewrite-browse / diff / search branches above it — those must still run first.)

- [ ] **Step 3: Verify build**

Run: `cargo build 2>&1 | rg -i "error\[" | head` (empty = clean); then `cargo build 2>&1 | tail -1`.
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/journal.rs src/input/keymap.rs
git commit -m "feat(journal): filtered-entry Escape jumps to Arkangel source"
```

---

### Task 4: Headless end-to-end verification

**Files:**
- Read: `CLAUDE.md` "Headless Verification"; confirm current key names in `keymap_config.rs`.

- [ ] **Step 1: Build + launch cage**

```bash
cd ~/utono/linux-lit && cargo build
XDG_RUNTIME_DIR=/run/user/1000 LIT_DEV=1 LIT_NO_MPV=1 GSK_RENDERER=cairo \
  WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit >/tmp/cage-tf.log 2>&1 &
disown
sleep 6
```
Find the cage socket (`find /run/user/1000 -maxdepth 1 -name 'wayland-[0-9]' | sort | tail -1`), export `WAYLAND_DISPLAY`, `wlr-randr --output HEADLESS-1 --custom-mode 1920x1200`. Give the window ~3s to map; check `stat -c%s` before Read-ing a PNG; use scoped cleanup `pkill -f "cage -- ./target/debug/linux-lit"`.

- [ ] **Step 2: Drive + screenshot each acceptance criterion**

Confirm key names in `keymap_config.rs` first. Then, with `wtype`:
1. Reader `f` → `Escape` → back in the reader (current work), no journal overlay open. (Screenshot: reading card, no term box.)
2. Open the journal overlay (`Ctrl+j`), press `f` → `Escape` → back in the journal overlay (not the reader).
3. Reader `f` → type a term that hits a Shakespeare passage Q&A (pick one from the DB, e.g. a term whose match is a `Cym`/`2H6` passage entry) → `Enter` → journal overlay shows the filtered subset (footer "… match n of m").
4. `Escape` from that filtered passage entry → reader loads `<work>-Arkangel` (title shows "(Arkangel)"), cursor on the entry's source first line, and the log shows the Arkangel `.m4b` resolved (`rg -i "arkangel|display_work|generated" <fresh dev log>`).
5. Repeat 3-4 with a non-passage entry (a corpus/scene note with no `start_citation`, e.g. a `TT`/author-scope note) → `Escape` → reader, current work, cursor unchanged, filter cleared (no edition switch in the log).
6. After step 4, `Ctrl+c` → returns to the pre-jump work at its exact prior line (log: `resumed saved position current_line=…`, prior work's pages).

Open every PNG and report what you see inline (UI review protocol). A green exit code is not enough.

- [ ] **Step 3: Cleanup**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 4: Commit (only if a harness tweak was needed)**

```bash
git add -A && git commit -m "test(journal): headless e2e for term-filter Arkangel workflow"
```

---

## Finishing

Per project convention: once tests pass and the tree is clean, merge `feat/term-filter-arkangel-workflow` to `master` from the main checkout (`git checkout master && git merge --no-ff`), re-verify `cargo build` + `cargo test --bins`, `git push origin master`, `git branch -d`. No keymap.json / tty-dotfiles changes in this feature.

## Self-Review notes

- **Spec coverage:** opener flag (T2), opener-aware term-input escape (T2), filtered subset on confirm (unchanged — spec §"confirm unchanged", verified in T4 step 3), filtered-passage Escape → Arkangel source + media (T3), non-passage fallback (T3 returns false → caller clears filter/closes, T4 step 5), citation→line resolver reuse (T1), Ctrl+c composition (T4 step 6). All covered.
- **Type-path caveat:** T1/T3 name `crate::db::models::Work`, `crate::app::LineMap`, and `AppState.line_map` — the implementer MUST confirm these exact paths/field names against the current source (the plan says so at each step); they are how `jump_to_journal_source_start` already refers to them, so lifting that function's body keeps them correct.
- **Borrow-checker caveat (T3):** the `current_work` immutable borrow must be dropped before `jump_to_line(&mut s)`; the plan computes `let buf = …;` under the immutable borrow first. The implementer resolves any residual borrow conflict by mirroring `corpus_search::select`'s scoping.

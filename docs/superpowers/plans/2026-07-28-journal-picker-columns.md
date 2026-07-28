# Q&A Picker Columns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Q&A picker's author scope a global list, render it as five aligned columns, widen the journal pickers to fit, and hint Alt+t in the empty filter box.

**Architecture:** One new DB query (global, replacing the per-author one), a pure `author_surname` helper, two new fields on `JournalRow`, a five-column row builder using GTK `SizeGroup` for alignment, and a width override at the two journal picker construction sites.

**Tech Stack:** Rust, GTK4 (`gtk4::SizeGroup`, `Label`, `Box`), rusqlite.

**Spec:** `docs/superpowers/specs/2026-07-28-journal-picker-columns-design.md`

## Global Constraints

- Branch off `master` (currently `72ee1d7a`), in a worktree under
  `~/utono/linux-lit-wt/<branch>` per CLAUDE.md.
- **Never write to the shared lit.db** at `~/utono/litdb/data/lit.db`. Tests
  use `Connection::open_in_memory()`. Read-only `sqlite3` inspection is fine.
- Do NOT run `cargo run` — the user launches the app.
- **linux-lit is a BIN-ONLY crate.** `cargo test --lib` fails. Use
  `cargo test --bins`; `cargo test --bins A B` is invalid (ONE filter only).
  Baseline is **1235 passed / 0 failed / 3 ignored**.
- A PRE-EXISTING deny-level clippy error at `src/db/queries.rs:2456`
  (unrelated) makes `cargo clippy --all-targets` fail. Plain `cargo clippy`
  is the gate.
- **SCENE and WORK scope rendering must stay byte-identical.** The five-column
  form is AUTHOR scope only. Any change visible in the other two scopes is a
  defect.
- `two_label_row` (`src/ui/picker_nav.rs:176`) has eight other callers. Do NOT
  modify it — add a new builder beside it.
- The picker still always opens on WORK; scope is not persisted.

## File structure

- `src/db/journal.rs` — replace `find_author_all_pages` with a global query.
- `src/input/actions/pickers.rs` — `author_surname` pure helper (beside the
  other picker helpers, so it is unit-testable without GTK).
- `src/ui/picker_nav.rs` — new `five_column_row` builder + a shared
  `SizeGroup` set. `two_label_row` untouched.
- `src/ui/journal_picker.rs` — `JournalRow` gains `author_label` and
  `type_label`; `populate_list` picks the row builder; width override;
  placeholder text.
- `src/ui/journal_move_picker.rs` — width override only.
- `src/input/actions/journal.rs` — populate the new fields.

---

### Task 1: Global author query + `author_surname`

Data layer only, no UI. Fully unit-testable.

**Files:**
- Modify: `src/db/journal.rs` (replace `find_author_all_pages`, ~line 342-367)
- Modify: `src/db/journal.rs` (`mod tests`)
- Modify: `src/input/actions/pickers.rs` (add `author_surname` + tests)

**Interfaces:**
- Consumes: `JOURNAL_PAGE_COLUMNS_J` (the `j.`-qualified 11-column const at
  `src/db/journal.rs:338`) and `map_journal_page_row`.
- Produces, used by Task 3:
  - `pub fn find_all_journal_pages(conn: &Connection) -> Result<Vec<(JournalPage, String, String, String)>, rusqlite::Error>`
    returning `(page, work_abbrev, work_title, author)` per row.
  - `pub(crate) fn author_surname(author: &str) -> &str`

- [ ] **Step 1: Write the failing tests**

In `src/db/journal.rs`'s `mod tests`. NOTE the existing helper `insert_cited`
and the `works` table shape used by
`author_scope_spans_works_and_corpus_notes` — that test creates
`works (id INTEGER PRIMARY KEY, abbrev TEXT, title TEXT, author TEXT)`.
Reuse that shape; the `id` column matters (a bare `id` in a JOIN is ambiguous
and was a live bug — see `JOURNAL_PAGE_COLUMNS_J`).

```rust
    /// Author scope is now a GLOBAL list: every entry, every author, plus
    /// corpus notes (which key by AUTHOR NAME in work_abbrev, not an abbrev).
    #[test]
    fn all_journal_pages_spans_every_author_and_corpus_notes() {
        let conn = mem();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                 id INTEGER PRIMARY KEY, abbrev TEXT, title TEXT, author TEXT
             );
             INSERT INTO works (abbrev, title, author) VALUES
                 ('Ham', 'Hamlet', 'Shakespeare'),
                 ('BH',  'Bleak House', 'Charles Dickens');",
        )
        .unwrap();

        save_journal_page(&conn, "Ham", 1, 2, "HamQ?", "A.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "BH", 1, 0, "DickensQ?", "A.", "m", "scene", "qa").unwrap();
        save_author_page(&conn, "Shakespeare", "CorpusQ?", "A.", "m", "note").unwrap();

        let rows = find_all_journal_pages(&conn).unwrap();
        let qs: Vec<&str> = rows.iter().map(|(p, _, _, _)| p.question.as_str()).collect();

        assert_eq!(rows.len(), 3, "every author's entries plus the corpus note");
        assert!(qs.contains(&"HamQ?"));
        assert!(qs.contains(&"DickensQ?"), "another author must NOT be excluded now");
        assert!(qs.contains(&"CorpusQ?"));

        let ham = rows.iter().find(|(p, _, _, _)| p.question == "HamQ?").unwrap();
        assert_eq!(ham.1, "Ham", "work abbrev");
        assert_eq!(ham.2, "Hamlet", "work title");
        assert_eq!(ham.3, "Shakespeare", "author");
    }

    /// Shakespeare pins to the top; other authors follow alphabetically.
    #[test]
    fn all_journal_pages_sorts_shakespeare_first() {
        let conn = mem();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS works (
                 id INTEGER PRIMARY KEY, abbrev TEXT, title TEXT, author TEXT
             );
             INSERT INTO works (abbrev, title, author) VALUES
                 ('BH',  'Bleak House', 'Charles Dickens'),
                 ('GT',  'Gullivers Travels', 'Jonathan Swift'),
                 ('Ham', 'Hamlet', 'Shakespeare');",
        )
        .unwrap();
        save_journal_page(&conn, "GT", 1, 0, "SwiftQ?", "A.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "BH", 1, 0, "DickensQ?", "A.", "m", "scene", "qa").unwrap();
        save_journal_page(&conn, "Ham", 1, 2, "ShakeQ?", "A.", "m", "scene", "qa").unwrap();

        let authors: Vec<String> = find_all_journal_pages(&conn)
            .unwrap()
            .into_iter()
            .map(|(_, _, _, a)| a)
            .collect();

        assert_eq!(
            authors,
            vec!["Shakespeare", "Charles Dickens", "Jonathan Swift"],
            "Shakespeare first, then alphabetical by author"
        );
    }
```

In `src/input/actions/pickers.rs`'s `mod tests`:

```rust
    #[test]
    fn author_surname_takes_the_last_word() {
        use super::author_surname;
        assert_eq!(author_surname("Charles Dickens"), "Dickens");
        assert_eq!(author_surname("Diarmaid MacCulloch"), "MacCulloch");
        // A mononym is its own surname — Shakespeare is the dominant corpus.
        assert_eq!(author_surname("Shakespeare"), "Shakespeare");
        assert_eq!(author_surname(""), "");
        assert_eq!(author_surname("  Jonathan  Swift  "), "Swift");
    }
```

- [ ] **Step 2: Run them and confirm they fail**

```bash
cargo test --bins all_journal_pages 2>&1 | tail -15
cargo test --bins author_surname 2>&1 | tail -15
```

Expected: FAIL TO COMPILE — `cannot find function find_all_journal_pages`
and `cannot find function author_surname`.

- [ ] **Step 3: Write the global query**

Replace `find_author_all_pages` in `src/db/journal.rs` with:

```rust
/// EVERY journal entry, each paired with `(work_abbrev, work_title, author)`.
///
/// The picker's AUTHOR scope is a global everything-view (2026-07-28), so this
/// takes no author parameter. Two keying schemes are unioned: ordinary entries
/// store a WORK ABBREV in `work_abbrev` and join to `works`; corpus notes
/// (`scope='author'`, see `save_author_page`) store the AUTHOR NAME there and
/// have no work row, so they are selected separately and carry their author as
/// both title and author.
///
/// Columns are `j.`-qualified: the first half JOINs `works`, and a bare `id`
/// is ambiguous across the two tables — SQLite refuses to prepare such a
/// statement, and the caller's error handling would surface it as an EMPTY
/// LIST rather than an error.
pub fn find_all_journal_pages(
    conn: &Connection,
) -> Result<Vec<(JournalPage, String, String, String)>, rusqlite::Error> {
    let sql = format!(
        "SELECT {JOURNAL_PAGE_COLUMNS_J}, j.work_abbrev, \
                COALESCE(w.title, j.work_abbrev), COALESCE(w.author, j.work_abbrev) \
           FROM journal_entries j \
           JOIN works w ON w.abbrev = j.work_abbrev \
          WHERE j.scope != 'author' \
         UNION ALL \
         SELECT {JOURNAL_PAGE_COLUMNS_J}, j.work_abbrev, j.work_abbrev, j.work_abbrev \
           FROM journal_entries j \
          WHERE j.scope = 'author' \
         -- Ordinals, not names: ORDER BY on a compound SELECT can be ambiguous.
         -- 14 = author, 13 = title, 7 = timestamp, 1 = id.
         ORDER BY (14) = 'Shakespeare' DESC, (14) ASC, (13) ASC, (7) ASC, (1) ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let page = map_journal_page_row(row)?;
        Ok((page, row.get(11)?, row.get(12)?, row.get(13)?))
    })?;
    rows.collect()
}
```

`JOURNAL_PAGE_COLUMNS_J` is 11 columns (indices 0-10) AT THIS TASK, so
`work_abbrev` is 11, title 12, author 13 — and SQL ordinals are 1-based,
hence 12/13/14 in the ORDER BY. **Verify against the const, not this
comment**; an off-by-one is a silent wrong-field read, not a compile error.

**Task 3 appends a 12th column (`scope`) to that const**, which shifts these
by one — 12/13/14 for `row.get`, ordinals 13/14/15. Expect to revisit this
query then; Task 3's step spells it out.

If SQLite rejects the parenthesised ordinal form, fall back to naming the
appended columns with `AS` aliases in BOTH union halves and ordering by those
alias names. Confirm whichever form you use actually runs — the tests will
catch it.

- [ ] **Step 4: Write `author_surname`**

In `src/input/actions/pickers.rs`:

```rust
/// The author column's display form: the LAST WORD of the stored author name.
///
/// `"Charles Dickens"` → `"Dickens"`; a mononym like `"Shakespeare"` is its
/// own surname. Deliberately simple — correct for every author in lit.db, and
/// the picker needs the horizontal space for the entry's identifying line. It
/// would mis-split a particle surname ("van Gogh"), which does not occur.
pub(crate) fn author_surname(author: &str) -> &str {
    author.split_whitespace().last().unwrap_or("")
}
```

- [ ] **Step 5: Update the caller so the tree compiles**

`repopulate_picker_for_scope` in `src/input/actions/journal.rs` calls
`find_author_all_pages`. Point it at `find_all_journal_pages` and adapt the
tuple destructuring. Do NOT yet add the new columns to the rows — that is
Task 3. The minimum here is: it compiles, and author scope still lists rows
(now global). Keep `work_label` populated as today.

- [ ] **Step 6: Verify**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

Expected: clean build; **1238 passed** (1235 + 3 new); 0 clippy errors.

- [ ] **Step 7: Commit**

```bash
git add src/db/journal.rs src/input/actions/pickers.rs src/input/actions/journal.rs
git commit -m "feat(journal): author scope lists every entry, Shakespeare first

The picker's author scope was limited to the current work's author. It is now
a global everything-view: every entry, every author, plus corpus notes.
Shakespeare pins to the top (the dominant corpus), then authors sort
alphabetically.

Adds author_surname, the column's display form: the last word of the stored
name, so a mononym is its own surname.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 2: The five-column row builder

**Files:**
- Modify: `src/ui/picker_nav.rs` (add `five_column_row` + a `ColumnGroups` struct)

**Interfaces:**
- Consumes: nothing new.
- Produces, used by Task 3:
  - `pub(crate) struct PickerColumnGroups { author: SizeGroup, work: SizeGroup, div: SizeGroup, kind: SizeGroup }`
    with `PickerColumnGroups::new()`.
  - `pub(crate) fn five_column_row(groups: &PickerColumnGroups, author: &str, work: &str, tag: &str, div: &str, kind: &str) -> GtkBox`

- [ ] **Step 1: Add the builder**

There is NO existing `SizeGroup` use in this codebase — this is the first, so
follow GTK's documented pattern rather than a local precedent. A
`SizeGroup` with `SizeGroupMode::Horizontal` makes every label added to it
adopt the width of the widest member, which is exactly what column alignment
needs and what a plain `Box` cannot do.

```rust
use gtk4::{SizeGroup, SizeGroupMode};

/// Column width groups for the five-column picker rows. One set per picker
/// instance, shared across all its rows — that is what makes the columns line
/// up: every label in a group takes the width of the widest member.
pub(crate) struct PickerColumnGroups {
    author: SizeGroup,
    work: SizeGroup,
    div: SizeGroup,
    kind: SizeGroup,
}

impl PickerColumnGroups {
    pub(crate) fn new() -> Self {
        let mk = || SizeGroup::new(SizeGroupMode::Horizontal);
        Self { author: mk(), work: mk(), div: mk(), kind: mk() }
    }
}

/// `author · work · tag … div kind` with the four fixed columns width-matched
/// across every row via `groups`. The TAG column is the only elastic one: it
/// hexpands and ellipsizes, so a long identifying line shortens rather than
/// pushing the division and type off the right edge.
pub(crate) fn five_column_row(
    groups: &PickerColumnGroups,
    author: &str,
    work: &str,
    tag: &str,
    div: &str,
    kind: &str,
) -> GtkBox {
    let fixed = |text: &str, group: &SizeGroup, css: &str| {
        let l = Label::builder()
            .label(text)
            .halign(Align::Start)
            .xalign(0.0)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        l.add_css_class(css);
        group.add_widget(&l);
        l
    };

    let author_l = fixed(author, &groups.author, "picker-item-detail");
    let work_l = fixed(work, &groups.work, "picker-item-detail");

    let tag_l = Label::builder()
        .label(tag)
        .halign(Align::Start)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .build();

    let div_l = fixed(div, &groups.div, "picker-item-detail");
    let kind_l = fixed(kind, &groups.kind, "picker-item-detail");

    let hbox = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();
    hbox.append(&author_l);
    hbox.append(&work_l);
    hbox.append(&tag_l);
    hbox.append(&div_l);
    hbox.append(&kind_l);
    hbox
}
```

**Do NOT modify `two_label_row`** — it has eight other callers whose
rendering must not change.

- [ ] **Step 2: Verify it compiles**

```bash
cargo build 2>&1 | tail -3
```

Expected: clean, with a dead-code warning on the new items until Task 3
consumes them. Do NOT silence that with `#[allow(dead_code)]`.

- [ ] **Step 3: Commit**

```bash
git add src/ui/picker_nav.rs
git commit -m "feat(ui): five-column picker row with SizeGroup alignment

The Q&A picker's author scope is now cross-work and needs author, work,
identifying line, division, and type on one row. two_label_row cannot express
that and has eight other callers, so this is a new builder beside it.

SizeGroup (first use in this codebase) width-matches the four fixed columns
across rows so they line up vertically; the tag column is the only elastic
one, ellipsizing rather than pushing div/type off the edge.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 3: Wire the columns into author scope

**Files:**
- Modify: `src/ui/journal_picker.rs` (`JournalRow` fields, `populate_list`, own the `PickerColumnGroups`)
- Modify: `src/input/actions/journal.rs` (`repopulate_picker_for_scope` populates the new fields)

**Interfaces:**
- Consumes: `find_all_journal_pages` + `author_surname` (Task 1),
  `five_column_row` + `PickerColumnGroups` (Task 2).
- Produces: no new public surface.

- [ ] **Step 1: Extend `JournalRow`**

Add to the struct in `src/ui/journal_picker.rs`:

```rust
    /// `Some(surname)` in AUTHOR scope only — the five-column form. `None`
    /// in scene/work scope, which keep the two-column rendering because
    /// author and work are constant there and would be noise.
    pub author_label: Option<String>,
    /// The entry's OWN scope (`passage`/`scene`/`work`/`author`). Shown as
    /// the type column so the header's BROWSING scope is never mistaken for
    /// each row's own scope — the confusion that prompted this change.
    pub type_label: String,
```

- [ ] **Step 2: Branch the row rendering**

In `populate_list`, choose the builder by whether `author_label` is set:

```rust
            let hbox = match (&item.author_label, &item.work_label) {
                (Some(author), Some(work)) => crate::ui::picker_nav::five_column_row(
                    &self.column_groups,
                    author,
                    work,
                    &item.question_prefix,
                    &item.scene_label,
                    &item.type_label,
                ),
                // Scene/work scope: byte-identical to before this change.
                _ => crate::ui::picker_nav::two_label_row(&primary, &item.scene_label),
            };
```

Add `column_groups: crate::ui::picker_nav::PickerColumnGroups` to
`JournalQaPicker` and build it in `new()`. **Keep the existing `primary`
computation for the two-label branch exactly as it is** — including the
`work_label` prefix logic — so scene/work rows do not change.

Note the empty-state row (`self.items.is_empty()`) also uses
`two_label_row`; leave it alone.

- [ ] **Step 3: Populate the fields**

In `repopulate_picker_for_scope` (`src/input/actions/journal.rs`):

- Scene and Work scopes: `author_label: None`, and `type_label` from the
  page's own `scope` column.
- Author scope: `author_label: Some(author_surname(&author).to_string())`,
  `work_label: Some(title)`, `type_label` from the page's own scope.

**Getting the entry's own `scope` — do this the APPEND way, not the insert
way.** `JournalPage` does not expose `scope`;
`JOURNAL_PAGE_COLUMNS` (`src/db/journal.rs:29`) selects 11 columns ending at
`kind`, and `map_journal_page_row` reads indices 0-10.

I checked the blast radius: **ten** call sites feed `map_journal_page_row`,
and `map_term_match_row` (`src/db/journal.rs:~486`) additionally hardcodes
`row.get(11)` with the comment "the column AFTER the
JOURNAL_PAGE_COLUMNS list". Inserting `scope` mid-list would shift every
appended index and silently break that reader — a wrong-field read, not a
compile error.

So do NOT insert. Instead:

1. APPEND `scope` to the END of both `JOURNAL_PAGE_COLUMNS` and
   `JOURNAL_PAGE_COLUMNS_J`, making them 12 columns (indices 0-11).
2. Add `pub scope: String` as the LAST field of `JournalPage` and read
   `row.get(11)?` for it in `map_journal_page_row`.
3. Every appended-column index in the file shifts by exactly one:
   `map_term_match_row`'s `row.get(11)` becomes `row.get(12)`, and
   `find_all_journal_pages` uses 12/13/14 with ORDER BY ordinals 13/14/15.
   **Grep for every `row.get(1[12])` in the file and fix each**; also update
   the two comments that name index 11.

Appending keeps all ten `map_journal_page_row` callers working untouched —
only the handful that read PAST the shared list need the +1.

If the ripple still looks larger than this, STOP and report rather than
half-applying it.

- [ ] **Step 4: Verify**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
cargo clippy 2>&1 | rg -c "^error" || echo "0 clippy errors"
```

Expected: clean build, 1238 passing, 0 clippy errors, and the Task 2
dead-code warnings gone.

- [ ] **Step 5: Commit**

```bash
git add src/ui/journal_picker.rs src/input/actions/journal.rs src/db/journal.rs
git commit -m "feat(journal): five aligned columns in author scope

Author scope rows now show surname, work title, identifying line, division,
and the entry's OWN type. Scene and work scopes keep the two-column form,
where author and work would be constant noise.

Surfacing each row's own scope is what stops the header's browsing-scope from
being read as a claim about the rows.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 4: Fixed width + Alt+t placeholder hint

Two small independent UI changes, batched because both touch
`journal_picker.rs`'s constructor.

**Files:**
- Modify: `src/ui/picker_nav.rs` (an optional-width variant of `build_picker_card`)
- Modify: `src/ui/journal_picker.rs` (width + placeholder)
- Modify: `src/ui/journal_move_picker.rs` (width)

**Interfaces:**
- Produces: `pub(crate) const JOURNAL_PICKER_WIDTH: i32` and
  `pub(crate) fn build_picker_card_wide(width: i32) -> GtkBox`.

- [ ] **Step 1: Add the width variant**

`build_picker_card` (`src/ui/picker_nav.rs:156`) hardcodes
`width_request(900)` and is called by NINE pickers. Do NOT change that
default — the media, bookmark, and gloss pickers have short rows and would
sit in dead space.

```rust
/// Fixed width for the JOURNAL pickers, which carry five columns. One
/// constant so the Q&A picker and the journal-move picker cannot drift apart.
/// Other pickers keep `build_picker_card`'s 900.
pub(crate) const JOURNAL_PICKER_WIDTH: i32 = 1180;

/// `build_picker_card` at an explicit width.
pub(crate) fn build_picker_card_wide(width: i32) -> GtkBox {
    let picker_box = build_picker_card();
    picker_box.set_width_request(width);
    picker_box
}
```

1180 is a starting value: it must be CONFIRMED on screen in Task 5 and
adjusted if the columns do not fit or the card overflows the reading card.
The main card is 1920-wide in production geometry, so 1180 leaves margin —
but verify rather than assume.

- [ ] **Step 2: Use it in both journal pickers**

In `src/ui/journal_picker.rs` and `src/ui/journal_move_picker.rs`, replace
`build_picker_card()` with
`build_picker_card_wide(crate::ui::picker_nav::JOURNAL_PICKER_WIDTH)`.

- [ ] **Step 3: Add the Alt+t hint**

In `src/ui/journal_picker.rs`, the search entry (~line 45) reads
`.placeholder_text("Filter Q&A pages...")`. Change it to:

```rust
            .placeholder_text("Filter Q&A pages…   (Alt+t cycles scope: scene · work · author)")
```

GTK shows a placeholder ONLY while the entry is empty, which is exactly the
requested condition ("when the user has entered no text") — so no signal
handler is needed. Confirm on screen in Task 5 that the hint reappears after
Alt+t clears the filter, since the scope cycle calls `set_text("")`.

Do NOT add the hint to `journal_move_picker` — Alt+t does nothing there.

- [ ] **Step 4: Verify**

```bash
cargo build 2>&1 | tail -2
cargo test --bins 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
git add src/ui/picker_nav.rs src/ui/journal_picker.rs src/ui/journal_move_picker.rs
git commit -m "feat(ui): fixed journal-picker width + Alt+t placeholder hint

The five-column author scope needs more room. build_picker_card is shared by
nine pickers, so the wider width is applied at the two journal construction
sites via one constant rather than to the shared default, which would stretch
the media/bookmark/gloss pickers into dead space.

The empty filter box now hints that Alt+t cycles the scope; GTK shows a
placeholder only while the entry is empty, which is exactly the asked-for
condition.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_0181pfwpd4fM8AoSXwh627oE"
```

---

### Task 5: On-screen verification

Non-waivable. Every criterion here is visual — alignment, width, and
ellipsization cannot be asserted from logs.

**Files:** none. Produces evidence.

- [ ] **Step 1: Launch headless on a Shakespeare work**

Use a DB COPY so the run cannot touch the user's database:

```bash
SCRATCH=/tmp/claude-1000/-home-mlj-utono-linux-lit/a9328b70-9801-4496-bb7e-d3bfe4cbf974/scratchpad
sqlite3 ~/utono/litdb/data/lit.db ".backup '$SCRATCH/cols.db'"
export XDG_RUNTIME_DIR=$(mktemp -d)
cd ~/utono/linux-lit-wt/<branch>
LIT_DEV=1 LIT_NO_MPV=1 LIT_HEADLESS_TEST=1 \
  LIT_DB_PATH="$SCRATCH/cols.db" LIT_LOG_PATH="$SCRATCH/cols.log" \
  LIT_START_WORK=AWW LIT_START_SCENE=1.1 \
  GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman \
  cage -- ./target/debug/linux-lit 2>"$SCRATCH/cols-cage.err"
```

Launch with the harness `run_in_background` — a `nohup`/`setsid`/`timeout`
wrapper kills the instance when it returns. Poll for `TEST_VIEWPORT_RECT`
with an until-loop; do not chain sleeps.

- [ ] **Step 2: Capture all three scopes**

```bash
export WAYLAND_DISPLAY=wayland-0
wtype -M ctrl -k j -m ctrl   # opens the picker (WORK)
sleep 3 && grim "$SCRATCH/col-work.png"
wtype -M alt -k t -m alt     # -> AUTHOR
sleep 3 && grim "$SCRATCH/col-author.png"
wtype -M alt -k t -m alt     # -> SCENE
sleep 3 && grim "$SCRATCH/col-scene.png"
```

**The first chord after mapping is routinely DROPPED** (it lands as
`mode=Reader`). Confirm `ACTION: OpenJournalPicker` in the log before
sending Alt+t, and confirm each `KEY: name=t … mode=JournalPicker` before
trusting the next screenshot.

- [ ] **Step 3: Open all three PNGs and report what you see**

Per the UI review protocol, report inline. Check against the spec:
- AUTHOR: five columns, with author and work VISUALLY LINING UP down the
  list (this is the SizeGroup's whole job — if they are ragged, it is not
  working).
- AUTHOR: Shakespeare rows appear FIRST; other authors follow.
- AUTHOR: the type column shows each row's own scope (`passage`/`scene`), so
  a `passage` row is visibly not an "author" row.
- SCENE and WORK: TWO columns, unchanged from before this branch. Compare
  against `target/ui/picker-p-author.png` and `picker-p-scene.png` from the
  previous branch — the scene capture should look the same.
- No clipping; long tag text ellipsizes rather than pushing div/type off.
- The picker is visibly wider than before.

- [ ] **Step 4: Verify the placeholder hint**

With the picker open and the filter EMPTY, confirm the placeholder reads the
Alt+t hint. Then type a character, confirm the hint disappears, then press
Alt+t and confirm the hint REAPPEARS (the scope cycle clears the entry).

- [ ] **Step 5: Confirm the media picker is unchanged**

The width override must not have leaked into the shared builder. Open the
media picker (confirm its bind in `keymap_config.rs`) and screenshot; it
must still be 900 wide.

- [ ] **Step 6: Clean up**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Exactly that pattern — a bare `pkill -f target/debug/linux-lit` kills the
user's live instance.

- [ ] **Step 7: Report**

State what was observed with the screenshots. If the width is wrong or the
columns are ragged, adjust `JOURNAL_PICKER_WIDTH` / the SizeGroup wiring and
re-verify rather than reporting it as acceptable.

---

## Finishing

Per CLAUDE.md: merge to master locally, then push.

1. `cargo build`, `cargo clippy`, `cargo test --bins` green; tree clean.
2. `git checkout master && git merge --no-ff <branch>`
3. Re-verify the build on master.
4. `git push origin master`
5. `git worktree remove …`, then `git branch -d <branch>`.

**Use `git commit -F <file>` for any message containing backticks** — a merge
earlier in this session had `` `works` `` shell-executed via `-m`.

This branch MEETS the spec threshold (a new axis in the picker, multiple
surfaces, a schema-ish change to `JournalPage`), so
`superpowers:requesting-code-review` runs before merge unless the user
waives it. The Task 5 on-screen run is NOT waivable either way.

## Follow-ups (NOT this branch)

- DC entry id 44 renders blank (empty question, untagged prose
  `source_text`) — pre-existing data issue, visible in any scope.
- `RecentQaPicker` has the corpus-note cross-work dead-end shape that was
  fixed in the Q&A picker.
- A log breadcrumb on `repopulate_picker_for_scope`'s `.ok()` arms, so a
  future query failure is not silently an empty list.

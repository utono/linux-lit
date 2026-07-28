# Q&A picker: global author scope, aligned columns, fixed width

_2026-07-28 (US Central). Status: approved, ready for a plan._

Follow-on to `2026-07-28-journal-picker-scope-cycling-design.md` (merged
`2293a3d1`), which added the scene/work/author scope cycle.

## Problem

Three things, all surfaced from using the shipped picker:

1. **Author scope is too narrow.** It lists only the current work's author.
   The user wants it to be a genuine everything-view: every journal entry in
   the database.
2. **Rows do not identify themselves.** A row shows a work prefix and the
   question/first line. Reading a cross-work list, there is no author, no
   division, and no entry type — so two similar questions from different
   works are hard to tell apart, and the header's scope label
   (`Q&A PAGES — AUTHOR`) was misread as describing each row's own scope
   rather than the scope being browsed.
3. **The picker is too narrow for that information**, and picker widths are
   not deliberately consistent.

## Change

### 1. Author scope lists EVERY entry

`find_author_all_pages(conn, author)` is replaced by a query that returns all
journal entries, regardless of author or work, each paired with its author
and work title.

This supersedes an earlier remark that "the picker should only show entries
for the current work" — that constraint continues to hold for the SCENE and
WORK scopes, which are unchanged. The author scope is deliberately the
widest ring of the cycle.

Corpus notes (`scope='author'`) still appear; they key by author name in
`work_abbrev` rather than by a work abbrev, so the query must union them in
exactly as the current one does.

### 2. Ordering: Shakespeare first, then by author

```sql
ORDER BY (author = 'Shakespeare') DESC, author ASC, work_title ASC, timestamp ASC, id ASC
```

Shakespeare is pinned to the top of the list because he is the dominant
corpus; remaining authors sort alphabetically. Within an author, group by
work, then by creation time.

The literal `'Shakespeare'` matches `works.author` as stored (verified: the
four authors with entries are `Shakespeare`, `Charles Dickens`,
`Diarmaid MacCulloch`, `Jonathan Swift`).

### 3. Five aligned columns

Rows render as fixed-width columns so author and work line up vertically
down the list:

```
Dickens      Bleak House     How Alexander wept whe…  2.0  passage
Dickens      D. Copperfield  I was born with a caul…  1.0  passage
Shakespeare  Cymbeline       Believe it, sir, I hav…  1.4  passage
```

- **Author** — the LAST WORD of `works.author`: `Dickens`, `Swift`,
  `MacCulloch`, and `Shakespeare` (a mononym, which falls out as itself).
  Chosen over the full name to leave room for the identifying line. The rule
  is deliberately simple and is correct for every author in lit.db; it would
  mis-split a name like "van Gogh", which does not occur.
- **Work title** — `works.title`, ellipsized to the column.
- **Tag** — the identifying line: the same text the picker shows today
  (`first_passage_line` of `source_text` for a passage entry, else the
  question). This column takes the remaining space and ellipsizes.
- **Division** — `div1.div2`, as today.
- **Type** — the entry's OWN scope (`passage` / `scene` / `work` / `author`),
  which is what makes the header's browsing-scope unambiguous.

`two_label_row` (`src/ui/picker_nav.rs:176`) produces a two-label row and
cannot express this. Aligned columns need per-column size groups or a
`Grid`; a new row builder is required rather than an extension of that
helper, whose existing callers must keep their current rendering
byte-identical.

In SCENE and WORK scope the author and work columns are constant and
therefore noise — they are OMITTED in those scopes, which keep today's
two-column rendering. Only author scope gets the five-column form.

### 4. Fixed width for the journal pickers

`build_picker_card` (`src/ui/picker_nav.rs:156-162`) sets
`width_request(900)` and is shared by NINE pickers: journal, recent-Q&A,
gloss, journal-move, media, bookmark, corpus-search, journal-term-input, and
the picker_nav default.

Widening it there would stretch the media/bookmark/gloss pickers, whose rows
are short and would sit in dead space. So the widened width applies to the
JOURNAL pickers only — the Q&A picker and its sibling journal-move picker —
via an explicit width override at those construction sites, leaving the
shared default at 900 for everyone else.

The exact width is chosen to fit the five columns at the current picker font
and then confirmed on screen; it is a single named constant so the two
journal pickers cannot drift apart.

## Consequences

- A global author scope makes the cross-work confirm path (already built and
  verified) load work far more often. That path is unchanged and already
  handles the same-work skip.
- Corpus notes have no work title. Their work column shows the author's
  corpus label rather than a title, matching the `"<Author> corpus"` detail
  they already carry.
- The list is 56 rows today and grows with use; the existing filter entry
  remains the narrowing tool. No pagination.

## Explicitly unchanged

- The scope does NOT persist across opens. The picker always opens on WORK.
  (Re-confirmed with the user 2026-07-28.)
- SCENE and WORK scope contents and ordering.
- Alt+t as the cycle key, and the cycle order scene → work → author.
- The empty-state row, the filter entry, Escape behavior, and confirm
  semantics.
- `two_label_row` and every non-journal picker's rendering.

## Testing

1. The global query returns entries from MULTIPLE authors, including corpus
   notes, and excludes nothing.
2. Shakespeare rows sort before every other author; remaining authors are
   alphabetical.
3. `author_surname()` is a pure function with unit tests:
   `"Charles Dickens" → "Dickens"`, `"Shakespeare" → "Shakespeare"`,
   `"Diarmaid MacCulloch" → "MacCulloch"`, `"" → ""`.
4. SCENE and WORK scope rows are unchanged — assert their row text does not
   gain author/work columns.
5. **On screen (non-waivable):** author scope shows five aligned columns with
   author and work visually lining up; the division and type columns are
   right-aligned and legible; long tag text ellipsizes rather than pushing
   the type column off; and the journal picker is visibly wider than the
   media picker (which must be unchanged at 900).

## Acceptance

- All three scopes render correctly on screen, captured.
- `cargo build`, `cargo clippy`, `cargo test --bins` green
  (baseline 1235 passed / 0 failed / 3 ignored).
- The shared lit.db is never written during development or testing.

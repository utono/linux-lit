# Q&A picker: cycle between author, work, and scene scope

_2026-07-28 (US Central). Status: approved, ready for a plan._

## Problem

The Q&A picker (Ctrl+j from the reader, Ctrl+\ from the journal overlay)
lists exactly one thing: every entry for the current work, via
`find_all_pages_ordered` (`src/input/actions/journal.rs:3018`). There is no
way to narrow to the chapter being read, and no way to widen to everything
by the author. Author-scope corpus notes are not listed at all.

## Change

Three scopes, cycled in place with **Alt+t** while the picker is open:

- **Scene** — entries anchored to the cursor's `(div1, div2)` band: scene
  and passage scopes, i.e. `find_scene_band_pages`.
- **Work** — every entry for the current work. Exactly today's behavior.
- **Author** — every entry from every work by the current work's author,
  PLUS that author's corpus notes (`scope='author'`).

Cycle order is scene → work → author → scene: tightest to widest, wrapping.
The picker OPENS on **Work**, so existing muscle memory is untouched and the
other scopes are opt-in.

### Why Alt+t

The gloss picker already cycles its type filter with Alt+t
(`src/input/keymap.rs:1027-1034`, `toggle_gloss_picker_type`). Same key, same
mode-scoped placement, same reason it is safe: Alt combos do not type into
the picker's search entry, so no focus guard is needed. The new arm sits
beside the existing `InputMode::GlossPicker` arm as
`InputMode::JournalPicker`.

### Showing the active scope

`build_picker_header` (`src/ui/picker_nav.rs:29`) already returns its
`Label`; `journal_picker.rs:37` currently discards it as `_header_title`.
Retain it and retitle on each cycle:

- `Q&A PAGES — SCENE`
- `Q&A PAGES — WORK`
- `Q&A PAGES — AUTHOR`

The scope must be visible: three list contents behind one unlabeled title
is unreadable.

### Author rows need a work label

Author scope is the only cross-work list, so its rows must say which work
each entry belongs to — otherwise two identically-worded questions from
different plays are indistinguishable. `JournalRow` gains a
`work_label: Option<String>`: `None` in scene/work scope (unchanged
rendering), `Some(title)` in author scope, rendered as a prefix on the
primary label.

`RecentQaPicker` (`src/ui/recent_qa_picker.rs`) already solves exactly this
for its cross-work list — follow its `work_label` shape rather than
inventing a second convention.

## Data

A new query is needed for author scope. Author-scope corpus notes store the
AUTHOR NAME in `work_abbrev` (see `save_author_page`, `AUTHOR_DIV`), while
every other entry stores a work abbrev, so it is a UNION of two selects:
entries joined to `works` on the author, plus `scope='author'` rows keyed by
the author string.

Volumes are small — the largest author in lit.db is Shakespeare with 26
entries across 7 works plus 2 corpus notes. No pagination or caching needed;
the existing filter entry handles narrowing.

Scene scope reuses `find_scene_band_pages`; work scope reuses
`find_all_pages_ordered`. Only the author query is new.

## Behavior details

- **Cycling rebuilds the list and resets the selection to the first row.**
  Preserving selection across a scope change is meaningless — the row sets
  differ.
- **The filter text is CLEARED on cycle.** A filter typed for one scope
  rarely makes sense in another, and a stale filter silently hiding a
  scope's contents reads as "this scope is empty."
- **An empty scope shows a non-selectable empty-state row**, never a
  dismissed picker. `RecentQaPicker::populate_list` already does this
  (`recent_qa_picker.rs:91-101`) — follow it. Scene scope is legitimately
  empty on most chapters, so this path is common, not exceptional.
- **Confirm (Enter) is unchanged in every scope**: land the journal overlay
  on the selected entry by id. In author scope the entry may belong to
  ANOTHER WORK, so confirm must load that work first — `confirm_picker`
  (`journal.rs:3110`) currently assumes the current work.
  `confirm_recent_qa_picker` (`journal.rs:3187`) already handles the
  cross-work load; reuse that path rather than duplicating it.
- **Escape is unchanged**: reader-initiated opens return to the reader,
  overlay-initiated opens return to the overlay.
- The scope does NOT persist across opens — the picker always opens on Work.

## Testing

1. Cycle order wraps scene → work → author → scene (pure unit test on the
   enum, mirroring `GlossPickerFilter`'s test at
   `src/input/actions/pickers.rs:1069`).
2. The author query returns entries from multiple works plus corpus notes,
   and excludes other authors' entries. In-memory rusqlite.
3. Scene scope returns only the cursor band's entries.
4. An empty scope yields the empty-state row, not a crash or a dismissal.
5. **On screen (non-waivable):** headless — open the picker, press Alt+t
   twice, screenshot each scope. Confirm the header label changes, the row
   set changes, and author rows carry a work label.
6. Cross-work confirm from author scope loads the other work and lands on
   the right entry.

## Keybind surfaces (required, same change)

Per CLAUDE.md, a bind change updates every surface it touches:

- `src/ui/journal_keybinds_overlay.rs` — the journal overlay's own Ctrl+/
  legend, since Ctrl+\ opens this picker from there.
- The main-card Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`) is NOT
  touched: it lists main-card binds only, and Alt+t here is picker-modal.
- `~/.config/linux-lit/keymap.json` is NOT touched: this bind is handled in
  the picker's modal arm in `keymap.rs`, not in `keymap_config.rs`, exactly
  like the gloss picker's Alt+t.

Run the `update-cairo-keybinds-overlay` three-pass cross-reference as the
self-check.

## Consistency note

`docs/guides/keybind-consistency-guide.md` records the app's key→concept
map. Alt+t = "cycle this picker's filter/scope" is now used by two pickers
with the same meaning, which strengthens rather than muddies the map. Record
the decision in that guide's change log.

## Not in scope

- Persisting the scope across opens or across launches.
- Scope cycling in any other picker.
- Changing what Ctrl+j / Ctrl+\ / Alt+j open — only what the picker lists
  once open.

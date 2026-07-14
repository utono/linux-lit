# Journal in-place modal vim editing — design

Replace the journal overlay's `e` edit card (`JournalEditCard`) with an **in-place
modal vim editor** on the journal page. `e` turns the page into one editable
buffer holding the whole Q&A; a full vim verb set (normal/insert/visual) edits it;
`:w` saves the hand-edits straight to `lit.db`; `R` opens a small LLM-rewrite
prompt; `:q` / Esc×2 cancels.

## Scope

- **Journal only.** Gloss & synopsis keep their current "ask Claude to rewrite"
  edit flow — their stored form is markup (`<p>`/`<gloss>`/`<speaker>`/`<verse>`)
  rendered into different display text, so editing the displayed text does not
  round-trip. The vim engine is built reusable so gloss/synopsis can adopt it
  later once markup round-trip is solved, but that is OUT of scope here.
- **Full vim verb set in one project** (not staged): see §4.
- The `JournalEditCard` (3-field Question/Answer/Instruction card) is **removed**.
- The shared `AskCard`/`AskCardHost` machinery stays (the `r` create-Q&A flow and
  the `R` rewrite prompt reuse it).

## Why approach A (pure engine, GTK as a thin adapter)

A new pure `src/input/vim/` module is the editor's **source of truth** (buffer
text, cursor, mode, pending count/operator, registers, undo). The journal
`TextView` is a **mirror** of the engine, not an editor (`editable(false)`; the
adapter drives it). Rationale:

- The entire verb set (operators × motions × counts × registers × `.` repeat) is
  **pure logic, unit-testable with `cargo test --bins`** — no GTK, no display.
  Decisive because the full set is a lot of behavior and the headless GUI harness
  is flaky / SIGTERM-killed in agent environments, so pure tests are the only
  reliable verification.
- Fits the existing `InputMode` + keymap dispatch (one handler per mode).
- Isolates a clean, reusable unit (each motion/text-object is one tested fn).

Rejected: (B) making the `TextView` the editable source of truth and manipulating
`TextBuffer`/`TextIter` per verb — entangles every verb with GTK, not
unit-testable, fights GTK's own editing; (C) vendoring a vim crate — violates the
"patterns not code" reference convention and needs heavy adaptation anyway.

## 1. Module architecture (`src/input/vim/`, zero GTK deps)

- `vim/mod.rs` — re-exports; module docs.
- `vim/key.rs` — `enum VimKey { Char(char), Esc, Enter, Backspace, Tab, CtrlR }`
  — the engine's GTK-independent input alphabet. (Counts and command chars arrive
  as `Char`.)
- `vim/mode.rs` — `enum Mode { Normal, Insert, Visual, VisualLine }`;
  `enum Op { Delete, Change, Yank, IndentR, IndentL }`;
  `enum Pending { None, Count(usize), Operator { op: Op, count: usize },
  FindChar { op_ctx, kind: f|t|F|T }, Replace, GPrefix, RegisterSelect, Command }`.
- `vim/motion.rs` — pure motions `(buf: &str, cursor: usize, count) -> usize` for
  `h j k l w b e ge 0 ^ $ gg G f t F T %`. Char-indexed (grapheme-naive v1; ASCII
  + BMP correct, combining-mark edge cases acceptable for prose Q&A text).
- `vim/textobject.rs` — `(buf, cursor, kind, inner|around) -> Option<Range>` for
  `iw aw i" a" i( a( i{ a{ i[ a[ i< a< ip ap`. Used by `ci(`/`di"`/`ca{`/etc.
- `vim/edit.rs` — buffer mutations: insert char, delete range, change range,
  yank range→register, put (`p`/`P`), `x`, `dd`, `D`, `J` (join), `r` (replace
  char), `>>`/`<<` (indent line). All `(buffer, cursor, …) -> (buffer, cursor)`.
- `vim/register.rs` — `Registers { unnamed: String, named: [String; 26],
  linewise: bool }`; yank/delete write unnamed (and a named reg when `"a` was
  pending); `p` reads them.
- `vim/repeat.rs` — `RepeatableChange` records the last buffer-mutating command
  (op + count + inserted text) so `.` replays it.
- `vim/undo.rs` — `UndoStack` of buffer+cursor snapshots; `u` pops, `Ctrl+R`
  redoes. One snapshot per completed change (insert session coalesces to one).
- `vim/command.rs` — the `:` ex-line: accumulate chars after `:`, parse on Enter
  into `Cmd { Write, Quit, WriteQuit, Unknown }`.
- `vim/engine.rs` — `VimEngine { buffer, cursor, mode, pending, registers,
  last_change, undo, cmdline }` and the single entry point
  `fn handle_key(&mut self, k: VimKey) -> Outcome`. Routes by `mode`+`pending`,
  delegates to motion/textobject/edit, updates state.

`Outcome { buffer_changed: bool, cursor: usize, mode: Mode,
selection: Option<Range>, action: EditorAction }` where
`enum EditorAction { Nop, Save, SaveQuit, Cancel, OpenRewrite }`. The adapter acts
on `action` and mirrors `buffer`/`cursor`/`selection` to GTK.

## 2. Buffer model & parse-back (journal framing, OUT of the engine)

A thin `journal_doc` helper (in `actions/journal.rs` or `vim/journal_doc.rs`),
unit-tested:

- **Enter (`e`):** build the buffer as `format!("Q: {q}\n\n{a}")` (the same `Q: `
  prefix the page shows). `VimEngine::new(buffer)`, cursor 0, Normal.
- **Save (`:w`/`:wq`):** split back — first line with a leading `Q:` / `Q: `
  stripped → `question`; text after the first blank line → `answer` (trimmed).
  Reuse `db::journal::update_journal_page(conn, id, question, answer, model)` (the
  save-as-is path already in `submit_edit_rewrite`), then `purge_journal_audio`,
  re-render, `land_on_current_band_id`, and snapshot `journal_undo` for the
  reading-mode `u`.
- **Parse-back guard:** no blank-line split → first line = question, rest =
  answer; never drop text. Toast if the structure was ambiguous.

Keeping framing out of the engine keeps the engine a generic text editor.

## 3. GTK adapter & key translation (the thin glue)

- New `InputMode::JournalEdit`. The dispatcher routes it to a new
  `handle_journal_edit_key`. `e` in `JournalOverlay` mode enters it (replacing
  `begin_edit` + the edit card), seeding the engine from the current page's Q&A.
- The journal page `TextView` mirrors the engine: stays `editable(false)`; after
  each key the adapter writes `engine.buffer` into the `TextBuffer`, places the
  GTK cursor at `engine.cursor`, and paints `selection` (Visual) as a GTK
  selection or the existing accent-bar range. **Pagination is suspended** while
  editing — the whole buffer shows in the scroll viewport and scrolls normally
  (the engine has no page concept; on save we re-paginate the read view).
- **Key translation** (real detail): GTK delivers `key_name` + modifiers, not
  characters. The controller (`app/mod.rs`) currently passes only `key_name`; we
  also thread the keyval's `to_unicode()` so the edit handler can build
  `VimKey::Char(c)` for printable insert-mode input. Named keys map explicitly:
  `Escape`→Esc, `Return`→Enter, `BackSpace`→Backspace, `Tab`→Tab; `Ctrl+R`→
  `VimKey::CtrlR` (vim redo — distinct from the `R` rewrite key, capital R no
  ctrl, handled before reaching the engine as an `EditorAction::OpenRewrite`
  trigger in Normal mode).
- **Mode indicator** `-- NORMAL -- / -- INSERT -- / -- VISUAL --` and the `:`
  command line render in the journal footer (the overlay already owns a footer
  label area; while editing it shows the mode line instead of the nav footer).

## 4. Modes & verb set (the full set)

**Motions (Normal & Visual, count-aware):** `h j k l`, `w b e ge`, `0 ^ $`,
`gg G` (with counts: `5G`), `f t F T` + `;`/`,` repeat, `%` (match pair).

**Insert entry:** `i a I A o O` (and `gi`? — no, v1 set: the six listed). `Esc`
returns to Normal (cursor moves left one, vim-style).

**Edits (Normal):** `x` (delete char ×count), `r<char>` (replace char),
`dd` (delete line), `D` (delete to EOL), `C` (change to EOL), `cc` (change line),
`J` (join line below), `>>` `<<` (indent/dedent line), `~` (toggle case of char
under cursor, advance). All included (no "optional" — full set).

**Operator + motion / text-object:** `d`/`c`/`y` + a motion (`dw d$ dgg dG df)`)
or a text-object (`diw ciw di( ci" ca{ dip`). `dd`/`cc`/`yy` are the doubled-line
forms.

**Yank/put & registers:** `y{motion}`, `yy`, `p`/`P` (charwise & linewise),
unnamed register + named `"a`–`"z` (prefix `"a` selects the register for the next
y/d/p).

**Counts:** a leading digit string accumulates into `Pending::Count` and
multiplies the next motion/operator (`3dd`, `5j`, `2cw`).

**Repeat:** `.` replays the last buffer-mutating change (op+count+inserted text).

**Undo:** `u` undo, `Ctrl+R` redo — the engine's own per-session stack.

**Visual:** `v` (charwise), `V` (linewise); motions extend the selection; `y`/`d`/
`c` operate on it; `>`/`<` indent; Esc leaves Visual.

**Ex / control:** `:w` save-as-is, `:wq` save+quit, `:q` quit (confirm if dirty),
`:q!` quit discarding. **Esc in Normal** (i.e. Esc when already Normal) = `:q`
behavior (Esc×2 from anywhere cancels). `R` (Normal) = open the rewrite prompt.

## 5. `:w` / `R` rewrite / cancel flows

- **`:w` / `:wq`:** parse-back (§2) → `update_journal_page` save-as-is → toast
  "Saved". `:wq` then exits edit mode back to `JournalOverlay`. `:w` stays in the
  editor (buffer marked clean).
- **`R` (rewrite):** opens the existing `AskCard` ("Rewrite instruction") via the
  journal `ask_host`, but in a new rewrite sub-mode. On Ctrl+Enter it sends the
  **current edited buffer's answer** + the instruction to Claude (so hand-edits
  compose with the instruction) through the existing rewrite path in
  `submit_edit_rewrite`'s Claude branch (the question comes from the current
  buffer too). On success the returned answer replaces the buffer's answer, the
  row is saved, and the editor **closes to the read view showing the revision**
  (matching today's edit-then-save UX). Empty instruction → behaves like `:w`
  (save-as-is, stays in the editor).
- **Cancel (`:q` / Esc×2):** if the buffer differs from the seed (dirty), show a
  confirm card ("Discard edits? y / Esc") reusing the `UndoConfirm` pattern; `y`
  discards and returns to `JournalOverlay`, Esc returns to the editor. If clean,
  exit immediately.

## 6. Coexistence with existing undo

- Vim `u` / `Ctrl+R` act **within** the edit session (engine stack) only.
- The reading-mode single-level `journal_undo` (`u` in `JournalOverlay`, behind
  the `UndoConfirm` card) is unchanged and applies **after** a `:w` (the save
  snapshots the pre-edit Q&A, exactly as `submit_edit_rewrite` does today).
- No key conflict: `u` means engine-undo only while in `JournalEdit` mode.

## 7. Removals & keybind/legend updates

- Remove `JournalEditCard` (`src/ui/journal_edit_card.rs`), its field in
  `JournalOverlay`, `open_edit_card`/`close_edit_card`/`take_edit_fields`/
  `toggle_edit_focus`/`edit_is_open`, the edit-card intercept block at the top of
  `handle_journal_key`, and the now-unused `journal-edit-card-sizing.md` guidance
  (replace with a note that the card was superseded by vim editing).
- `begin_edit` now enters `InputMode::JournalEdit` instead of opening the card.
- Update the journal Ctrl+/ legend (`journal_keybinds_overlay.rs` GROUPS) — `e`
  now "edit (vim)"; document the vim mode's keys in a dedicated edit-mode legend
  (a new `JournalEdit` legend, or a section). Update the reader-card Ctrl+/ cross
  refs if any name the journal edit card.
- keymap.json / keymap_config.rs are NOT involved (journal overlay keys are
  handled directly in `handle_journal_*`, not via the configurable table).

## 8. Testing

- **Pure engine (`cargo test --bins`)** — the bulk: a table-driven harness
  `(initial_buffer, cursor, keys[]) -> (expected_buffer, expected_cursor,
  expected_mode)`. Cover every motion, operator×motion, text-object, count,
  register, `p`/`P`, `.` repeat, `u`/`Ctrl+R`, visual ops, and the `:` commands
  (asserting the emitted `EditorAction`). This is where correctness lives.
- **Parse-back** — unit tests for build-buffer / split-back round-trips incl. the
  ambiguous-structure guard.
- **e2e (ask the user to run)** — `InputMode` routing, the TextView mirror, the
  footer mode indicator, `R` rewrite, and cancel-confirm only settle in a mapped
  surface; verify via `scripts/e2e-env.sh` + a screenshot. The headless cage
  harness is SIGTERM-killed in the agent env, so runtime verification is the
  user's to run; the engine tests carry correctness.

## 9. Out of scope (YAGNI)

Macros (`q`/`@`), marks (`m`/`` ` ``), search-in-editor (`/` within the edit
buffer), `:s` substitution, multiple windows/splits, jumplist, the gloss/synopsis
adoption. Multi-page editing (the buffer is one page; pagination is suspended
during edit by design).

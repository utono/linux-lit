# Chat-panel Journal-view `r` / `R` — design

_2026-07-18 (US Central)._

## Goal

In the chat panel's **Journal view** (`PanelView::Journal`), make the two keys
behave like the main journal overlay:

- **`r`** — ask a new question about the pinned passage.
- **`R`** — open the rewrite-target popup (`q` / `a` / `b`) on the **selected**
  saved Q&A and run the existing journal rewrite pipeline.

Journal view **only**. In the Gloss and Question views, `r` / `R` keep their
current meaning (`regloss_pinned`), unchanged.

## Background / current state

- The chat panel is a distinct surface from the journal overlay: its own widget
  (`s.chat_panel`), its own data (`s.chat.journal_list: Vec<JournalPage>`), and
  its own input mode (`InputMode::ChatTranscript`). Journal view renders a flat,
  cursor-less list via `render_journal_view` → `render_rows_to_top`.
- The main journal overlay's `begin_ask` and `open_rewrite_target` — and the
  whole rewrite pipeline (`rewrite_question_path` → `improve_question` →
  `rewrite_with_claude` / `begin_rewrite_with`) — are wired to the
  `journal_overlay` widget, `s.journal.pages` / `s.journal.page_index`,
  `displayed_journal_page`, and `InputMode::JournalOverlay`, at roughly twenty
  `render_current` / mode-restore sites.
- The chat panel already has a native ask flow: `a` → `focus_prompt_insert` →
  `submit_chat_prompt`, which sends the pinned-passage question and auto-persists
  the first follow-up to the journal.

So `r` is a small reuse of the panel's own ask; `R` needs a bridge into the
overlay's rewrite pipeline without opening the overlay.

## Decisions (settled)

- **Scope:** Journal view only.
- **`R` target:** the Journal view gains a **row cursor**; `R` rewrites the
  highlighted entry.
- **Bridge approach (Approach A):** reuse the overlay rewrite pipeline by
  pre-seeding its page state from the selected chat journal entry; the action
  stays visually in the chat panel.
- **`r` flow:** reuse the panel's own ask input (same as `a`).
- **Post-rewrite:** re-sync `chat.journal_list` from lit.db, re-render Journal
  view, keep the row cursor on the rewritten entry.

## Components

### 1. Journal-view row cursor

Add a selectable cursor so `R` has an unambiguous target and `j` / `k` step it.

- New field `ChatState.journal_cursor: usize` — index into `journal_list`.
- `render_journal_view` highlights the cursor's entry. Each `journal_list` entry
  renders as a `Q:` row + an `Answer` row (see `journal_view_rows`); the accent
  bar (`.chat-cursor-row`) lands on the entry's **`Q:` widget row**. Render via
  `render_rows_focused_cursor` with that widget index (no visual-selection
  range — Journal view has no `V`), replacing the current `render_rows_to_top`.
  The scroll-to-top behavior just shipped is preserved by seeding the cursor at
  entry 0 on every toggle into Journal view (its `Q:` widget row is row 0, and
  `render_rows_focused_cursor` scrolls the cursor row to the top).
- `toggle_panel_view`'s `PanelView::Journal` arm resets `journal_cursor = 0`
  after reloading `journal_list`.
- `j` / `k` in Journal view move `journal_cursor` (they currently viewport-
  scroll). Pure step + clamp to `[0, journal_list.len() - 1]`; empty list keeps
  cursor 0 and paints no bar (the one placeholder row is not landable). This
  lives in `transcript_cursor_move`'s existing Journal guard.
- An entry-index → `Q:`-widget-row map: entry `i` owns widget rows `2*i`
  (question) and `2*i + 1` (answer) when the list is non-empty (each entry is
  exactly two rows in `journal_view_rows`). A small pure helper returns the
  cursor's `Q:` widget row; unit-tested.

### 2. `r` — ask a new question

In Journal view, `r` calls `focus_prompt_insert` — the panel's existing ask
input, landing in vim INSERT (same as `a`). No new ask card. `submit_chat_prompt`
already sends the pinned passage question and auto-saves the first follow-up. The
new Q&A appears in the transcript (Question view); toggling back to Journal view
re-syncs `journal_list` from lit.db and shows it.

### 3. `R` — rewrite the selected entry (Approach A bridge)

New `chat::rewrite_journal_entry(state_rc)`:

1. Require `PanelView::Journal`, a non-empty `journal_list`, and a `gloss_ctx`
   (the pinned passage's citations + work). Missing any → toast, no-op (mirrors
   `regloss_pinned` / `toggle_panel_view`'s "no passage" toasts).
2. Read the selected entry `journal_list[journal_cursor]`.
3. **Pre-seed overlay state** so `displayed_journal_page(&s)` resolves the right
   row: set `s.journal.filter = None`, `s.journal.pages = journal_list.clone()`,
   `s.journal.page_index = journal_cursor`, and `s.journal_band` to the passage
   band derived from `gloss_ctx` (`JournalBand::Passage { div1, div2, start, end }`).
4. Set `s.chat.rewrite_return = true` (a new `bool` on `ChatState`) — marks that
   this rewrite was launched from the panel and must return to it.
5. Call the existing `open_rewrite_target(state)`. The popup, the `q` / `a` / `b`
   dispatch (`InputMode::RewriteTargetChoice`), and the Claude pipeline run
   unchanged.

**Return bridge.** The rewrite pipeline's success closures end at
`rewrite_with_claude` (question-only / answer paths: `render_current` +
`land_on_current_band_id`, ~line 2100) and the answer-instruction submit reached
via `begin_rewrite_with` (the `both` path). At each of those overlay-render sites,
guard on `s.chat.rewrite_return`:

- When **set**: skip the overlay render; instead re-sync
  `s.chat.journal_list = reload_journal_list(ctx…)`, clamp `journal_cursor` to the
  (possibly reordered) list — re-find the rewritten entry by `id` so a timestamp
  bump can't strand the cursor — re-render Journal view via
  `render_journal_view`, set `s.input_mode = InputMode::ChatTranscript`, and clear
  the flag. The panel shows the rewritten answer in place.
- When **unset**: the existing overlay behavior runs byte-for-byte.

`close_rewrite_target` currently forces `InputMode::JournalOverlay`; when
`rewrite_return` is set (the reader pressed `Esc` at the popup, cancelling), it
must instead restore `ChatTranscript` and clear the flag — the cancel path never
reaches the success closures, so it owns its own restore + flag clear.

The `rewrite_return` flag is scoped to exactly this: it is set only by
`rewrite_journal_entry`, read only at the overlay-render / mode-restore sites, and
always cleared on the terminal outcome (success re-render or cancel). It defaults
`false` (`#[derive(Default)]`) and resets with the rest of `ChatState` on panel
close.

### 4. Keymap

In `handle_chat_transcript_key`, split the `"r" | "R"` arm:

- `PanelView::Journal`: `r` → `focus_prompt_insert`; `R` → `rewrite_journal_entry`.
- Otherwise (Gloss / Question): keep `regloss_pinned` for both.

### 5. Legend

Update the chat-panel Ctrl+/ legend (`src/ui/chat_keybinds_overlay.rs` GROUPS) to
note the Journal-view meanings of `r` (ask) and `R` (rewrite this Q&A). Same
change as the handler, per the overlay-legend rule.

## Data flow

```
Journal view, cursor on entry i
  r ─▶ focus_prompt_insert ─▶ submit_chat_prompt ─▶ (auto-save) ─▶ transcript
  R ─▶ rewrite_journal_entry
         ├─ seed s.journal.pages / page_index / band from journal_list[i]
         ├─ s.chat.rewrite_return = true
         └─ open_rewrite_target ─▶ q/a/b ─▶ improve_question ─▶ rewrite_with_claude
                                                                    │ (success)
                                    rewrite_return? ── yes ─────────┤─▶ reload journal_list,
                                                                    │    re-render Journal view,
                                                                    │    ChatTranscript, clear flag
                                                     ── no  ────────┴─▶ render_current (overlay)
              Esc at popup ─▶ close_rewrite_target ─▶ rewrite_return? yes → restore ChatTranscript
```

## Error handling / edge cases

- **No `gloss_ctx`** (panel opened by `Tab`, never glossed) → `R` toasts
  "No passage to rewrite" and no-ops, matching `toggle_panel_view`.
- **Empty `journal_list`** → `R` toasts "No journal entry to rewrite".
- **Cursor out of range after reload** (entry re-sorted) → re-find by `id`;
  fall back to clamp to `len - 1`.
- **Rewrite failure** (Claude error) → existing error toast; the panel must still
  restore `ChatTranscript` and clear `rewrite_return` so the reader isn't stranded
  in `RewriteTargetChoice` / a stale flag. The error closure guards on the flag
  the same way the success closure does.
- **Overlay rewrites unaffected**: with the flag `false`, every guarded site runs
  its original overlay path.

## Testing

**Unit (`cargo test --bins`):**
- `journal_cursor` step + clamp (including empty list, single entry, bounds).
- Entry-index → `Q:`-widget-row mapping (`2*i`), and the inverse used to re-find
  the cursor after reload.

**Headless (cage, `test-headless-navigation` env):**
- Open the panel floating → `\` to Journal view → `j` / `k` moves the accent bar
  across entries (screenshot the bar position).
- `R` shows the rewrite-target popup; `q` rewrites; the list re-renders in place
  with the cursor still on the entry and the panel back in transcript focus.
- `r` opens the ask input in INSERT.

**Manual (`crll`) final eyeball:** rewrite an entry with several saved Q&As on one
passage; confirm the correct one changed and the cursor stayed on it.

## Risk

The one non-trivial risk is the hidden-overlay bridge (step 3 + return). It is
contained by the `rewrite_return` flag: every overlay path is untouched when the
flag is `false`, and the flag is cleared on all three terminal outcomes (rewrite
success, rewrite error, popup cancel). No pipeline signature changes; the ~20
overlay sites are untouched except the two success closures and
`close_rewrite_target`, each behind the flag guard.

## Out of scope (YAGNI)

- No `e` (in-place edit), `D` (delete), `u` (undo), or `c` (copy-id) parity in the
  panel's Journal view — only `r` / `R` were requested.
- No cross-work term filter, no rewrite-history browse, in the panel.
- No visual-selection (`V`) in Journal view.

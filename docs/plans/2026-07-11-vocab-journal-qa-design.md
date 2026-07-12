# Vocab Journal Q&A — Design

**Date:** 2026-07-11
**Status:** Approved design, pre-implementation
**Mockup:** https://claude.ai/code/artifact/fa95b6fe-36b3-460d-902b-fcb0614fdc74

## Goal

A main-card keybind that, with the vocab popup visible and the cursor on a
segment containing one or more vocab words, immediately asks Claude to discuss
the popup's current vocab word — its use in this segment and elsewhere in the
author's corpus — and stores the exchange as a new *vocab* type of journal
Q&A. The answer renders inside the vocab popup panel itself, paginated when it
exceeds the panel, with the word and its definition pinned at the bottom of
every page.

## Decisions (settled during brainstorming)

- **Fire immediately** on the keybind: no ask card, no editing step, no
  separate save step. Mirrors the journal overlay's `ask_claude` shape
  (prompt → API → DB insert → repaint), not the chat panel's
  save-on-`s` shape.
- **One word per Q&A**: the popup's *current* word (the one `r` has cycled
  to). Press again after cycling for another word.
- **Answer displays in the vocab popup** (new Journal view), and is saved as
  a journal entry.
- **Length**: the prompt targets **10–15 sentences**. No panel-fitting cap —
  long answers paginate in place.
- **Pagination**: new main-card binds **Ctrl+n / Ctrl+p** page the answer
  when it spans more than one panel-height. Both are unbound in the main
  card today (their only uses are inside the library-picker and
  synopsis-overlay modal handlers), and they match the app's existing
  Ctrl+n/p next/prev idiom.
- **Pinned anchor**: the word and its definition are always visible at the
  bottom of each page; only the Q&A body above them pages.
- **Reuse**: revisiting the same word + segment shows the stored Q&A; no
  duplicate API call.
- **Typing**: new `kind='vocab'` in `journal_entries`, `scope='passage'`,
  with a new nullable `word` column for exact reuse lookup.
- **Trigger key**: plain `R` (Shift+r) — unbound today, and sits on top of
  the popup's own family (`r` tap/cycle, `Ctrl+r` hide).

## UX

### Trigger and guard

`R` in the main card dispatches a new `Action::VocabJournalAsk`. The handler
is a silent no-op unless:

1. the vocab popup is visible (`state.vocab_popup.popup.is_visible()`), and
2. the cursor line has at least one `VocabMatch` (the same
   `state.vocab_matches` filter `refresh_vocab_popup` uses).

The target word is the popup's currently-cycled word
(`VocabPopupState.data[index].word`).

### Popup states (Journal view)

The popup gains a third view alongside `Definition` and `Gloss`:
`VocabView::Journal`. Its layout, top to bottom:

1. counter (`1 / 2`) top-right, as today;
2. `JOURNAL Q&A` header (11px letter-spaced caps);
3. a dim one-line question label (`Q · "franklin" in this segment, and
   across Shakespeare`);
4. the **answer body** — the paging region;
5. a rule, then the **pinned block**: the word (16px) and its definition
   (small, dimmed) — fixed children, present on every page including the
   pending state;
6. the hint-rule footer: left `saved · <model>`, right
   `page N / M · Ctrl+n ▸` (page indicator only when M > 1; the whole
   footer is hidden while a request is pending — nothing is saved yet).

State sequence on `R`:

- **No stored entry**: body shows a pending row (`asking <model>…`), the
  request fires, and on success the answer replaces the pending row and the
  entry is already saved. On failure the body shows a brief error line and
  nothing is inserted.
- **Stored entry exists** (matched by work + div1/div2 + kind + word): the
  stored answer renders immediately; no API call.

Leaving the segment or cycling words with `r` returns the popup to its
normal Definition/Gloss behavior; the Journal view is not sticky across
cursor movement.

### Pagination

The answer body is a height-capped container: available height = window
height minus the popup's bottom anchor margins minus the fixed chrome
(counter, headers, pinned block, footer). Implementation: the body in a
`GtkScrolledWindow` (never user-scrolled) capped to that height; `Ctrl+n` /
`Ctrl+p` move the vadjustment by whole viewport heights. Page count
`M = ceil(content_height / viewport_height)` and position `N` derive from
the adjustment, feeding the footer indicator. The pinned block sits outside
the scrolled container, so its height never affects page arithmetic.

`Ctrl+n`/`Ctrl+p` dispatch new actions (`VocabJournalPageNext` /
`VocabJournalPagePrev`) that no-op unless the popup is visible in Journal
view with more than one page.

## Data model

`journal_entries` (src/db/journal.rs) gains one nullable column:

```sql
ALTER TABLE journal_entries ADD COLUMN word TEXT;
```

applied with the app-side auto-migration pattern already used for
`claude_model` (check `pragma_table_info`, add if missing).

The new entry is written via a variant of `save_passage_page`:

- `kind = 'vocab'`, `scope = 'passage'`
- `work_abbrev` = `Work.canonical_abbrev` (per the existing normalization
  rule for all journal/gloss paths)
- `div1`/`div2` = the cursor segment's division
- `start_citation`/`end_citation` from the segment's `cursor_lines`
- `source_text` = the cursor segment text
- `word` = the vocab word (lowercased/normalized the same way
  `vocab_matches` keys it)
- `question` = the rendered question label; `answer`, `claude_model`,
  `timestamp` as usual.

**Reuse lookup**: `SELECT ... WHERE work_abbrev=? AND div1=? AND div2=? AND
kind='vocab' AND word=?` (most recent wins). Exact and stable — no matching
on question text.

**Journal overlay**: `kind='vocab'` entries render through the existing
non-note Q&A branch (the overlay only special-cases `kind='note'`), so they
appear in the journal with no required overlay changes. An optional small
kind marker can come later.

## Prompt

New `api_prompts` key **`journal.vocab`** (new row inserted in lit.db, plus
a compiled fallback in `src/gloss.rs` beside `journal_qa_prompt`, fetched
through the same `template_or` wrapper). Placeholders, substituted with the
existing ad-hoc `.replace` chain style:

- `{word}` — the vocab word
- `{author}`, `{title}`, `{scene_label}` — from `AppState.current_work` +
  the segment's division (same assembly the chat prompt uses)
- `{segment}` — the cursor segment text (`segments::segment_context`)
- `{corpus_hits}` — real evidence: up to ~10 lines from
  `db::concordance::find_word_occurrences(conn, word, author)` where
  `work_abbrev != current`, deduped by line text, grouped under work titles.
  If no other-work hits exist, the block says so and the prompt asks the
  model to note that the usage is (in this corpus) unique to this work.

The prompt instructs: discuss the word's use in this specific segment, then
its use elsewhere in the author's corpus, grounding claims in the supplied
lines; **10–15 sentences**. The request goes through the existing
`claude_bridge` non-streaming path (`send_message`, `max_tokens: 4096`,
model from `config.claude_model`).

## Keybind bookkeeping

Three new binds — `R` → `VocabJournalAsk`, `Ctrl+n` → `VocabJournalPageNext`,
`Ctrl+p` → `VocabJournalPagePrev` — each added in **all three** places:

1. `src/input/keymap_config.rs` (compiled defaults, in `vocab_bindings`),
2. the stowed `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`
   (deploy with stow, or the JSON silently shadows the compiled change),
3. the Ctrl+/ overlay (`src/ui/keybinds_overlay.rs`, keycap strip AND
   describe() arm) via the `update-cairo-keybinds-overlay` skill's
   three-pass cross-reference.

The `keymap_config.rs` test assertions that currently document `R` as free
must be updated in the same change.

## Flow summary

1. `R` → `dispatch_action` → `VocabJournalAsk` handler (new
   `src/input/actions/vocab_journal.rs` or a section of the existing vocab
   popup module).
2. Guard (popup visible + vocab word at cursor); resolve current word,
   segment context, division, citations.
3. Reuse lookup in `journal_entries`; hit → render stored answer in
   Journal view, done.
4. Miss → popup shows pending row; build `{corpus_hits}` via
   `find_word_occurrences`; render `journal.vocab` prompt; fire
   `claude_bridge` request.
5. On success: insert the `kind='vocab'` row, then render the answer in the
   popup (page 1) — insert first, render from the stored row, so a repaint
   or crash never loses a paid answer.
6. On failure: error line in the popup body, no insert.

## Error handling

- Guard failures: silent no-op (consistent with other gated binds).
- API error / timeout: dim error line in the popup body; popup returns to
  Definition view on next cursor move; nothing stored.
- Missing `journal.vocab` row in lit.db: compiled fallback prompt keeps the
  feature working.
- No corpus hits: prompt still fires with the "unique in corpus" framing.

## Testing

- **Unit** (`cargo test --bins`): prompt builder (placeholder substitution,
  corpus-hit capping/grouping/exclusion of current work, empty-hits
  framing); reuse lookup (hit/miss, word normalization, most-recent wins);
  the `word` column auto-migration on a legacy schema.
- **Headless e2e** (cage/grim per CLAUDE.md, `LIT_NO_MPV=1`): drive `r` to
  open the popup, `R` with a pre-seeded stored entry (so no live API call is
  needed), screenshot Journal view; assert the pinned word/definition and
  footer are present and unclipped; `Ctrl+n`/`Ctrl+p` on a long stored
  answer flips the page indicator. UI review protocol applies: open the
  PNGs and report what's on them.
- **Live**: one real `R` ask by the user with an API key (the app's
  standard manual check for API paths).

## Out of scope

- Editing the vocab Q&A from the popup (the journal overlay's vim editor
  already covers editing).
- A journal-overlay filter/marker for `kind='vocab'` (renders fine via the
  existing Q&A branch; marker is a possible follow-up).
- Vocab Q&A for words outside the vocab list, or popup-hidden operation.
- Any change to the vocab-sentence loop mode (`Ctrl+-`) or concordance
  navigation.

## Geometry assumption

Panel capacity math assumes production geometry (1920×1200). Pagination
makes the feature safe at any size — smaller windows simply mean more,
shorter pages.

# Corpus-level (author-scope) journal notes + Markdown rendering

**Date:** 2026-07-02
**Status:** Design approved — proceed to plan + implementation (no per-section review gate).

## Problem

The journal Q&A feature scopes every entry to a single work (`scope ∈
{scene, work, passage}`, all keyed to a NOT-NULL `work_abbrev`). There is no way
to file a journal entry against an author's whole corpus rather than one title or
scene.

Two concrete needs drove this:

1. **Corpus-level entries.** A finding-aid like `~/Downloads/loading-the-cry.md`
   is *about Shakespeare's corpus*, not any one play or scene. It should appear
   for every Shakespeare work, not be pinned to one abbrev.
2. **Non-`Q:` entries + Markdown.** Such documents are authored outside the app
   (by claude.ai) and are not questions, so they must render **without** the
   `Q:` prefix. And the journal overlay should render **Markdown** (headings,
   emphasis, lists, blockquotes, rules, tables) rather than plain text.

## Current system (as-is)

- **Table `journal_entries`** (`~/utono/litdb/data/lit.db`):
  `id, work_abbrev (NOT NULL), div1, div2, question, answer, claude_model,
  scope (DEFAULT 'scene'), start_citation, end_citation, source_text, timestamp`.
  `scope ∈ {scene, work, passage}`. `work` scope uses sentinel `div1=div2=-1`
  (`JOURNAL_WORK_DIV` in `src/app/mod.rs:3945`). `work_abbrev` is always the
  `Work.canonical_abbrev` (never a `-Amb`/`-BBC` variant).
- **`JournalBand` enum** (`src/app/mod.rs`): `Work`, `Scene(d1,d2)`,
  `Passage{div1,div2,start,end}`. Overlay band nav walks work ↔ scenes.
- **`Q:` prefix is display-only** — `prefix_question()`
  (`src/ui/journal_overlay.rs:113`) prepends `Q: ` at render time; the stored
  `question` column never contains it. The vim editor buffer
  (`src/input/vim/journal_doc.rs` `build_buffer`/`parse_back`) seeds/strips `Q:`.
- **Bodies render as plain text** — `format!("{}\n\n{}", prefix_question(q), a)`.
- **DB functions** (`src/db/journal.rs`): `save_journal_page`,
  `save_passage_page`, `find_journal_pages`, `find_scene_band_pages`,
  `find_work_pages`, `find_all_pages_ordered`, `move_journal_page`.

## Decisions (from brainstorming)

- **Scope:** new `scope='author'`, keyed by the **author string**; appears for
  every work by that author. Broadest band, above `work`.
- **Two creation paths:** (1) in-app "ask Claude about {author}'s corpus" → a
  `qa` row; (2) a Claude Code skill imports an `.md` (from claude.ai) as a
  `note` row.
- **Entry kind:** explicit `kind` flag (`qa` | `note`) — drives the `Q:` prefix,
  not inference from an empty question.
- **Storage:** the `answer` column holds **raw Markdown**. Import is a one-line
  insert; `e` edits raw Markdown; `:w` saves raw; display re-parses. (Chosen over
  storing rendered TextTags: raw round-trips losslessly, keeps import trivial,
  and matches the existing "vim editor edits raw stored text" grain. Render cost
  is a caching problem, not a correctness one.)
- **Rendering:** parse full CommonMark with `pulldown-cmark`, map to `TextView`
  `TextTag`s. Rich for headings/emphasis/lists/blockquotes/rules; monospace
  preformatted block for tables/code (a `TextBuffer` cannot render true grids).
  Applies to **note** bodies (imported `.md`, `kind='note'`). **Scope revised
  during implementation (2026-07-02): NOTES ONLY, not Q&A answers.** A Q&A answer
  can carry `<hi>` highlight spans and drives the block-cursor, both computed
  against the raw-text character offsets; Markdown rendering changes that text
  (consuming `##`/`**`, adding `• `/`─` markers) and would misalign the highlight
  ranges and block navigation. Notes have no `<hi>` spans and no such offset
  dependency, so only they render as Markdown. Q&A answers stay plain text. (The
  original wording "all bodies" was narrowed to notes-only by user decision; a
  future task could render Q&A answers by recomputing offsets against the rendered
  buffer, but that is out of scope here.)
- **Target styling — replicate the claude.ai artifact view** (the reference
  screenshot of `loading-the-cry.md`): a **serif** body in linux-lit's existing
  reading-card family (Charter), generous leading, and a comfortable left
  measure (not edge-to-edge). Specifically:
  - **Title (`#`)**: large bold serif, ~2× body size, space below.
  - **Subtitle/`###`**: smaller bold serif directly under the title.
  - **Section heading (`##`)**: bold serif, ~1.3× body, clear space above.
  - **`---` rule**: a thin, full-width light-grey line with real vertical margin
    above/below — NOT a row of dashes/box-drawing characters. (Rendered as a
    dedicated paragraph carrying a bottom-border style via the tag, or a
    single light `─`-run styled to span the measure; pick the cleaner of the two
    in the TextView, but it must read as a hairline rule, not literal dashes.)
  - ***Italic*** and **bold** inline runs render as true italic/bold serif.
  - **Ordered/bulleted lists**: real `1.`/`•` markers with a **hanging indent**,
    the bold lead-in phrase then flowing into body text.
  - Blank line between paragraphs; paragraph leading matches the reading card.
  The intent is that an imported `.md` looks like the claude.ai render, reusing
  the reader's serif look rather than inventing a new typographic scale.
- **Schema encoding (Approach 1):** reuse `work_abbrev` to hold the author string
  for author rows, with a new sentinel `div1=div2=-2`. No NOT-NULL drop, no key
  column added — mirrors how `work` scope already overloads sentinels. The
  `scope`/`kind` columns keep the meaning unambiguous.
- **Nav:** a **dedicated keybind** jumps to the author band (not part of the
  sequential band walk). Author is a jump target only.
- **Note editing:** `e` on a `note` edits the **raw Markdown** (no `Q:` seed
  line); `qa` entries keep the `Q:`/answer buffer.
- **Import skill:** lives at `~/utono/linux-lit/.claude/skills/import-corpus-note/`,
  takes an `.md` path + **explicit author arg**.

## Design

### 1. Schema (`journal_entries`)

Additive, backward-compatible:

- **`kind TEXT NOT NULL DEFAULT 'qa'`** — values `qa` | `note`. Existing rows
  default correctly to `qa`. Added via startup migration
  (`ALTER TABLE journal_entries ADD COLUMN kind …` if absent), mirroring the
  model-provenance migration.
- **New `scope` value `'author'`** (no DDL — `scope` is free text). An author row:
  - `scope='author'`, `work_abbrev = <author>` (e.g. `'Shakespeare'`),
    `div1=div2=-2` (new `JOURNAL_AUTHOR_DIV`).
  - `note`: `question=''`, `kind='note'`, `answer=<raw markdown>`.
  - `qa`: `question=<q>`, `kind='qa'`, `answer=<raw markdown answer>`.
  - `claude_model` set as usual (import may leave null).

No column dropped; `work_abbrev` stays NOT NULL (author string satisfies it).

### 2. `JournalBand` + state (`src/app/mod.rs`)

- Add `JournalBand::Author(String)` (author name).
- `JOURNAL_AUTHOR_DIV: (i64,i64) = (-2,-2)` beside `JOURNAL_WORK_DIV`.
- `footer_left_text`: `Author(name) => format!("{} · corpus", name)`.
- The author band is a **jump target only** — `target_bands` and scene-nav are
  unchanged.

### 3. DB layer (`src/db/journal.rs`)

- `save_author_page(conn, author, question, answer, model, kind)` → inserts
  `scope='author'`, `work_abbrev=author`, `div1=div2=-2`.
- `find_author_pages(conn, author)` → `WHERE scope='author' AND work_abbrev=?
  ORDER BY timestamp`.
- Add `kind` to every INSERT and to the `JournalPage` row struct
  (`kind: String`); existing inserts pass `'qa'`.
- `move_journal_page` gains an `Author` target arm (writes the `-2` sentinels).
- `band_for_page`/`band_for_rewrite`: `scope='author'` →
  `JournalBand::Author(work_abbrev.clone())`.

### 4. Ask + import (`src/input/actions/journal.rs`)

- **In-app author ask:** `begin_author_ask(state)` (dispatched from the new
  keybind while the journal overlay is open) sets
  `journal_band = Author(current_work.author)`. `ask_claude`'s band match gains
  an `Author` arm → `save_author_page(..., kind='qa')`. Prompt: "Ask a question
  about {author}'s corpus" (reuse the parameterized genre/unit prompt with a
  "corpus" unit).
- **Import** is a direct DB insert by the skill (§6); app code only displays and
  edits note rows.

### 5. Rendering + editing (Markdown)

- **New module `src/ui/markdown.rs`:** `render_markdown_into(buffer, text, …)`
  parses with `pulldown-cmark` and applies `TextTag`s: headings (scaled
  weight/size), bold, italic, bullet + numbered lists (indent + marker),
  blockquotes (indent + color), horizontal rules (full-width `─`), paragraphs;
  tables + fenced code → monospace preformatted (JetBrainsMono tag), best-effort
  column spacing for tables. A parsed-render cache keyed by `(entry id, text
  hash)` avoids re-parsing on scroll (snapshot-cache pattern).
- **Prefix / body build** driven by `kind`, not empty-question inference:
  `qa` → `Q: <question>\n\n` + rendered answer; `note` → rendered answer only.
- **Vim editor (`journal_doc.rs`):** `note` → `build_buffer` returns raw Markdown
  (no `Q:` seed), `parse_back` returns it verbatim (saved raw). `qa` keeps the
  existing buffer. `:w` re-renders through `markdown.rs`.
- **Keybind overlays** (mandated by CLAUDE.md): update the journal overlay
  `GROUPS` legend (`src/ui/journal_keybinds_overlay.rs`) and the Ctrl+/ reader
  overlay (`src/ui/keybinds_overlay.rs` via the `update-cairo-keybinds-overlay`
  skill) for the new author-jump key. Key chosen from `~/utono/rpd` at plan time
  and reflected in `keymap_config.rs`, `keymap.json`, and the stow source
  `~/tty-dotfiles/linux-lit/.config/linux-lit/keymap.json`.

### 6. Import skill (`~/utono/linux-lit/.claude/skills/import-corpus-note/`)

- Args: `.md` path + author name (explicit). Inserts one row: `scope='author'`,
  `kind='note'`, `work_abbrev=<author>`, `div1=div2=-2`, `question=''`,
  `answer=<raw file contents>`, `claude_model` optional. Warns if an identical
  `(author, answer)` note already exists (dedup).
- `SKILL.md` documents the exact `sqlite3` insert and the `loading-the-cry.md`
  example.

### 7. Testing / verification

- **Pure unit (`cargo test --bins`):** `kind`-driven prefix; `save_author_page`
  / `find_author_pages` round-trip; `journal_doc` note round-trip (raw in = raw
  out); `band_for_page` author mapping; `markdown.rs` tag-range parse tests
  (heading/bold/list/blockquote).
- **Visual (user runs e2e):** rendered Markdown in the overlay and author-jump
  nav are screenshot-level acceptance. Build + `cargo test --bins`, then give the
  user the `e2e-env.sh` / cage command to eyeball rendered `loading-the-cry.md`.
- **Snapshot version:** no `LineMap` shape change → `SNAPSHOT_VERSION` untouched.

## Non-goals (YAGNI)

- No global cross-author corpus pool (`scope='corpus'`) — author scope only.
- No NOT-NULL drop on `work_abbrev` / no separate `author` column (Approach 2).
- No true tabular grid rendering in the TextView (monospace preformatted only).
- Author band is not inserted into the sequential band walk — jump-key only.

## Key files touched

- `~/utono/litdb/data/lit.db` — `kind` column (startup migration).
- `src/app/mod.rs` — `JournalBand::Author`, `JOURNAL_AUTHOR_DIV`, footer text.
- `src/db/journal.rs` — `save_author_page`, `find_author_pages`, `kind` column,
  `JournalPage.kind`, `move_journal_page` author arm.
- `src/input/actions/journal.rs` — `begin_author_ask`, `ask_claude` author arm,
  `band_for_page`/`band_for_rewrite` author mapping.
- `src/ui/markdown.rs` — new CommonMark → TextTag renderer + cache.
- `src/ui/journal_overlay.rs` — `kind`-driven prefix, render bodies via markdown.
- `src/input/vim/journal_doc.rs` — note raw-Markdown buffer round-trip.
- `src/input/keymap.rs`, `keymap_config.rs`, `keymap.json` (+ stow source) —
  author-jump key.
- `src/ui/journal_keybinds_overlay.rs`, `src/ui/keybinds_overlay.rs` — legends.
- `~/utono/linux-lit/.claude/skills/import-corpus-note/SKILL.md` — import skill.

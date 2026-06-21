# Vocabulary Words: Catalog and Coloring

How `lit.db` catalogs vocabulary words and their definitions, and how
linux-lit colors those words in the main reading card.

This guide is in two halves. The first half ("What you see") is for anyone
reading in the app. The second half ("How it works") is a developer reference
with exact tables, queries, and `file:line` pointers.

## Overview

`lit.db` holds a **global catalog** of roughly 2,500 vocabulary words — the
kind found on SAT / GRE / PSAT lists — each paired with a definition. When you
open a work, linux-lit loads that whole catalog, scans the on-screen text, and
tints every word that appears in the catalog a single gold/amber color in the
main reading card.

Two things are worth knowing up front, because they surprise people:

- **Highlighting is global, not curated per work.** A word is colored because
  it exists in the catalog, not because someone tagged it for *this* book. The
  same word lights up in every work.
- **The color carries no state.** There is no "known / learning / new"
  distinction and no part-of-speech coloring. Every matched word gets the same
  single foreground color. (Per-word detail — definition, etymology, a
  work-specific gloss — lives in a popup, not in the card color.)

## What you see (reader)

- **Gold words in the text.** Catalog words are drawn in a gold/amber
  foreground over the normal text color. Defaults:
  - Dark themes: `#d8a657` (gold).
  - Light themes: `#8a6534` (brown), further adjusted for contrast (see
    *Color* below).
- **One uniform color.** Every vocab word looks the same. The highlight only
  tells you "this word is in the catalog" — matched vs. not matched.
- **Toggle with `Alt+\`.** This shows or hides the vocab coloring and the
  choice is remembered across sessions.
- **Look up a word for detail.** The vocab popup shows the word's definition,
  its test sources (sat/gre/psat), an etymology breakdown (prefix / root /
  suffix), and — if one exists for this exact passage — a work-specific gloss.
  The popup uses its own derived colors and is independent of the in-card
  gold.
- **Structural lines are skipped.** Act/scene headings and separator lines are
  not colored, so words like "prologue" or "epilogue" in a heading stay plain.

## How it works (developer reference)

### How lit.db catalogs vocabulary words

All vocabulary tables are prefixed `vocab_`. The schema is created by migration
scripts under `~/utono/litdb/scripts/`, not a standalone schema file.

**`vocab_words`** — the core word + definition table. This is what drives card
highlighting.

```sql
CREATE TABLE vocab_words (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    word TEXT NOT NULL UNIQUE,
    definition TEXT NOT NULL,
    difficulty_level INTEGER,        -- 1-5
    created_at TEXT DEFAULT (datetime('now')),
    source TEXT                      -- sat / gre / psat / NULL
);
CREATE INDEX idx_vocab_words_word ON vocab_words(word);
```

`word` is stored lowercased; `definition` is auto-fetched (wordnet / gcide /
online) or entered manually.

**`vocab_word_variants`** — alternate surface forms that should match the same
base word (e.g. an inflected form). Matched the same way as base words during
highlighting.

```sql
CREATE TABLE vocab_word_variants (
    word_id INTEGER NOT NULL REFERENCES vocab_words(id),
    variant TEXT NOT NULL UNIQUE
);
CREATE INDEX idx_vocab_variant ON vocab_word_variants(variant);
```

**`vocab_word_sources`** — many-to-many test-source tags (shown in the popup).

```sql
CREATE TABLE vocab_word_sources (
    word_id INTEGER NOT NULL REFERENCES vocab_words(id),
    source TEXT NOT NULL CHECK(source IN ('sat', 'gre', 'psat')),
    PRIMARY KEY (word_id, source)
);
```

**`vocab_rhetoric`** — one row per word, holding the morphological /
etymological breakdown shown in the popup. There is **no plain part-of-speech
column** anywhere in the vocab schema; the closest data is here.

```sql
CREATE TABLE vocab_rhetoric (
    id INTEGER PRIMARY KEY,
    word_id INTEGER NOT NULL UNIQUE REFERENCES vocab_words(id),
    prefix TEXT, prefix_gloss TEXT,
    root TEXT, root_base TEXT, root_gloss TEXT,
    suffix TEXT, suffix_gloss TEXT,
    rhetorical_function TEXT,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**`vocab_word_families`** — groups related words (`family_id`, `word_id`).

**`vocab_lookup_log`** — defined to record a word lookup against a work + line,
but currently unused (no writer in the Rust source or the litdb scripts).

> Note: `vocab_iconic_uses` is documented in
> `~/utono/litdb/docs/vocab-word-glosses.md` but is **not present** in the live
> `lit.db`. The doc is ahead of the database.

Schema lives in:

- `~/utono/litdb/scripts/migrate_vocabulary.py` — `vocab_words`,
  `vocab_word_sources`, `vocab_word_variants`
- `~/utono/litdb/scripts/migrations/add_vocab_rhetoric.sql` — `vocab_rhetoric`
- `~/utono/litdb/scripts/migrations/add_vocab_word_families.sql` — `vocab_word_families`
- `~/utono/litdb/scripts/add_vocab.py` — main CLI that writes words, definitions,
  glosses, and rhetoric

### How a word is tied to a specific work and line

There is no direct word-to-line table in normal use. The catalog match itself
is global. Two mechanisms attach per-work context:

1. **Per-work vocab glosses** live in the shared `passages` + `glosses` tables
   (not a vocab-specific table). A vocab gloss is a `glosses` row with
   `gloss_type = 'vocab-word'` and `word_id` referencing `vocab_words.id`,
   joined to a `passages` row that carries `work_abbrev`, `start_citation`,
   `end_citation` (e.g. `PL.4.736`, `Ham.1.2.93`), and `source_text`.
2. **Citation-to-line resolution** uses `line_mapping`
   (`work_abbrev, div1, div2, line_in_div, canonical_text`). When a gloss is
   authored, `add_vocab.py` scans a window of `line_mapping` rows to build the
   surrounding `source_text` context.

See `~/utono/litdb/docs/vocab-word-glosses.md` for the gloss authoring detail.

### How linux-lit loads and colors the words

The full pipeline lives in `src/db/queries.rs` (loading) and `src/app.rs`
(matching + tagging).

**1. Load the catalog.** `load_vocab_words()` —
`src/db/queries.rs:453`. Despite taking a `_work_abbrev` parameter, it
**ignores the work** and loads the entire global set:

```sql
SELECT LOWER(word) FROM vocab_words;
SELECT LOWER(v.variant) FROM vocab_word_variants v;
```

Both go into a single `HashSet<String>` stored on `state.vocab_words`. This is
the surprising "highlighting is global" behavior in code form.

**2. Create the tag.** `src/app.rs:1186` builds one GTK4 `TextTag` named
`"vocab-word"` with **only a foreground color** (`.foreground(&theme.vocab_fg)`)
and adds it to the buffer's tag table. It is stored on state as `vocab_tag`
(`src/app.rs:340`).

**3. Compute matches.** `build_vocab_matches()` — `src/app.rs:5242`. Reads the
whole buffer, iterates line by line, tokenizes manually (word characters are
alphanumeric plus `'` and `\u{2019}`), lowercases each token, and if
`state.vocab_words.contains(&lower)` records a `VocabMatch { word, line_index,
char_start, char_end }` using **character offsets**. It skips act/scene markers
and separator lines, and trims any scansion-label region so those are never
colored.

**4. Apply the tag by range.** `apply_vocab_highlighting()` — `src/app.rs:5325`.
For each match it gets `iter_at_line(line_index)`, advances `forward_chars` to
the start and end offsets, and calls `buffer.apply_tag(&vocab_tag, &start,
&end)`. The tag covers only the matched word's range, so surrounding text keeps
the default foreground — that range-limited tag is what visually sets a vocab
word apart.

**5. Remove / toggle.** `remove_vocab_highlighting()` — `src/app.rs:5339` —
strips the tag across the whole buffer. The `Alt+\` keybind
(`Action::ToggleVocabHighlight`) flips `state.vocab_highlight_visible`, calls
apply or remove, and persists the choice to config
(`src/input/keymap.rs:1949`, bound in `src/input/keymap_config.rs:267`).

On opening a work, `src/app.rs:2992` runs load -> build matches -> apply (when
visible) in sequence.

### Color

The vocab foreground is `theme.vocab_fg`, defined in `src/theme.rs`.

- It is sourced from the active color scheme's `VocabWord` highlight `guifg`.
- If absent, it falls back to `#d8a657` (dark themes) or `#8a6534` (light
  themes) — `src/theme.rs:150`.
- **Light themes additionally run `choose_vocab_fg()`**
  (`src/theme.rs:350`) to guarantee hue contrast against the body text color:
  it prefers whichever candidate (the scheme's vocab color or the cursor color)
  has the greater hue distance, and otherwise derives a color by rotating the
  text hue +150° with clamped saturation and lightness. Dark themes use the
  scheme color unchanged.

The color depends on **nothing about the word** — not its state, source, or
part of speech. There is exactly one tag and one color.

### The popup (separate from card color)

The vocab definition popup (`src/ui/vocab_popup.rs`) is distinct from the
in-card coloring. It pulls:

- `load_vocab_definition()` — `src/db/queries.rs:475` — definition plus
  concatenated test sources.
- `load_vocab_etymology()` — `src/db/queries.rs:513` — prefix / root / suffix
  glosses from `vocab_rhetoric`.
- `load_vocab_gloss()` — `src/db/queries.rs:536` — the per-work gloss for the
  exact citation, selected from `glosses` joined to `passages` by
  `work_abbrev` and a `start_citation <= … <= end_citation` range.

The popup styles itself with derived blend colors
(`vocab_popup_fg/dim/border`, `src/theme.rs:540`), **not** the card's
`vocab_fg`. A related query, `load_vocab_word_list()` (`src/db/queries.rs:562`),
counts catalog occurrences per line for the vocab word sidebar/picker — it does
not drive card coloring.

## Key files

Loading and queries:

- `src/db/queries.rs:453` — `load_vocab_words` (global catalog into a HashSet)
- `src/db/queries.rs:475` — `load_vocab_definition` (popup)
- `src/db/queries.rs:513` — `load_vocab_etymology` (popup)
- `src/db/queries.rs:536` — `load_vocab_gloss` (per-work gloss, popup)
- `src/db/queries.rs:562` — `load_vocab_word_list` (sidebar/picker)

Matching and coloring:

- `src/app.rs:1186` — create the `"vocab-word"` `TextTag`
- `src/app.rs:5242` — `build_vocab_matches` (tokenize + match)
- `src/app.rs:5325` — `apply_vocab_highlighting` (tag by char range)
- `src/app.rs:5339` — `remove_vocab_highlighting`
- `src/app.rs:2992` — load/build/apply on work open
- `src/input/keymap.rs:1949` — `Alt+\` toggle
- `src/theme.rs:150`, `src/theme.rs:350` — `vocab_fg` and `choose_vocab_fg`

Schema (litdb):

- `~/utono/litdb/scripts/migrate_vocabulary.py` — core vocab tables
- `~/utono/litdb/scripts/migrations/add_vocab_rhetoric.sql` — `vocab_rhetoric`
- `~/utono/litdb/scripts/migrations/add_vocab_word_families.sql` — `vocab_word_families`
- `~/utono/litdb/docs/vocab-word-glosses.md` — per-work gloss authoring

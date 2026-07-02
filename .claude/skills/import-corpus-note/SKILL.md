---
name: import-corpus-note
description: Use when importing a .md file (e.g. from claude.ai) into linux-lit's journal as an author/corpus-scope note entry (scope='author', kind='note'), keyed by author name, so it renders for every work by that author with no Q: prefix
argument-hint: <path.md> <author>
---

## What This Does

Imports a Markdown file into `lit.db` as an **author/corpus-scope journal note**. The note appears in linux-lit when you open ANY work by the specified author and press `Alt+a` (jump to the author/corpus band), with no `Q:` prefix (it's a note, not a Q&A entry).

The row inserted has this shape:

| Column | Value |
|--------|-------|
| `scope` | `'author'` |
| `kind` | `'note'` |
| `work_abbrev` | The author string (e.g. `'Shakespeare'`) |
| `div1`, `div2` | `-2`, `-2` (sentinel for author band) |
| `question` | `''` (empty, notes don't have questions) |
| `answer` | Raw contents of the .md file |
| `claude_model` | `'claude.ai'` |
| `timestamp` | Current time (`datetime('now')`) |

## Import Command

The skill uses a Python/sqlite3 approach to safely handle multi-line Markdown with proper quoting:

```bash
# args: MD_PATH (a .md file), AUTHOR (e.g. "Shakespeare")
MD_PATH="$1"; AUTHOR="$2"
DB=~/utono/litdb/data/lit.db

# Dedup: warn if an identical (author, answer) note already exists.
python3 - "$DB" "$AUTHOR" "$MD_PATH" <<'PY'
import sqlite3, sys
db, author, path = sys.argv[1], sys.argv[2], sys.argv[3]
answer = open(path, encoding="utf-8").read()
c = sqlite3.connect(db)

# Ensure the kind column exists (mirrors linux-lit's ensure_journal_table
# migration; a DB not yet opened by the migrated build won't have it).
has_kind = c.execute(
  "SELECT 1 FROM pragma_table_info('journal_entries') WHERE name='kind'"
).fetchone()
if not has_kind:
  c.execute("ALTER TABLE journal_entries ADD COLUMN kind TEXT NOT NULL DEFAULT 'qa'")
  c.commit()

# Check if author exists in the works table
author_count = c.execute(
  "SELECT COUNT(*) FROM works WHERE author = ?",
  (author,)).fetchone()[0]
if author_count == 0:
  print(f"WARNING: No works found for author '{author}'.")
  print("The imported note will not appear in linux-lit.")
  print("Valid authors in the database:")
  for (a,) in c.execute("SELECT DISTINCT author FROM works ORDER BY author"):
    print(f"  - {a}")
  sys.exit(1)

# Check for duplicates
dup = c.execute(
  "SELECT id FROM journal_entries WHERE scope='author' AND work_abbrev=? AND answer=?",
  (author, answer)).fetchone()

if dup:
  print(f"Already imported as id {dup[0]}; skipping.")
else:
  c.execute(
    "INSERT INTO journal_entries "
    "(work_abbrev, div1, div2, question, answer, claude_model, scope, kind, timestamp) "
    "VALUES (?, -2, -2, '', ?, 'claude.ai', 'author', 'note', datetime('now'))",
    (author, answer))
  c.commit()
  print(f"Imported note id {c.execute('SELECT last_insert_rowid()').fetchone()[0]} for {author}.")

c.close()
PY
```

## Worked Example

Import the "Loading the Cry" finding-aid as a Shakespeare corpus note:

```bash
import-corpus-note ~/Downloads/loading-the-cry.md Shakespeare
```

Then in linux-lit:

1. Open any Shakespeare work (e.g. *Hamlet*).
2. Press `Alt+a` to jump to the author/corpus band.
3. The imported note appears with no `Q:` prefix.

## Important Notes

**Author string must match exactly.** The `<author>` argument must be an exact value from the `works.author` column in lit.db. For Shakespeare, the exact string is:

```
Shakespeare
```

(NOT "William Shakespeare" or any other variant). Before importing, check valid authors:

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT DISTINCT author FROM works ORDER BY author;"
```

**Deduplication.** The skill warns if an identical note for the same author already exists (same `answer` text) and skips the insert.

**Author validation.** The skill checks that at least one work has the given author. If none is found, it prints a warning with a list of valid authors and exits without importing.

**Linux-lit must be closed.** lit.db has no hot reload. The imported note only appears in linux-lit after it is restarted — a running instance will not see the new row until it is reopened.

**`kind` column auto-migration.** The skill adds the `kind` column if it is absent (mirroring linux-lit's own idempotent `ensure_journal_table` migration), so import works correctly even before the migrated build has been launched for the first time against this DB.

## Row Semantics

- **`div1 = -2, div2 = -2`** — Sentinel value marking an author/corpus-scope entry (not tied to a specific scene or section within a work).
- **`scope = 'author'`** — The note is keyed by author and appears for all works by that author.
- **`kind = 'note'`** — A plain note, not a Q&A pair (no `question` field; `question` is always empty).
- **`work_abbrev = <author>`** — For author-scope entries, this column holds the author string itself (a reuse of the column for cross-scope keying).

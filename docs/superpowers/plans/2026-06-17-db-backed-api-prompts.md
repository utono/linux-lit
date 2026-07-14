# DB-backed Claude-API Prompts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move linux-lit's and litdb's Claude-API prompts (gloss explications, synopsis generation) into a versioned `api_prompts` table in `lit.db` managed by a new `~/utono/claude-api-prompts` repo, then update the gloss/synopsis prompts to discuss rhetorical devices (per gloss 21741) and complement the Eleanor voice.

**Architecture:** `lit.db` holds every prompt version (`api_prompts`); the active row is what apps send. The new repo holds plaintext masters + Python sync/restore/render/list scripts (git history mirrors DB versions) and the editing skills. linux-lit (Rust) and litdb (Python) read the active prompt at runtime, each with a compiled/in-file fallback. Gloss templates keep `{}` placeholders filled by shared `ipa.*` fragment rows.

**Tech Stack:** SQLite (`lit.db`), Rust (rusqlite, linux-lit), Python 3 (sqlite3, litdb scripts), git + `gh` (private remote `utono/claude-api-prompts`).

**Spec:** `docs/superpowers/specs/2026-06-17-db-backed-api-prompts-design.md`

---

## File Structure

**New repo `~/utono/claude-api-prompts/`:**
- `prompts/*.md` — 12 master files (one per prompt_key), YAML frontmatter + body.
- `scripts/db.py` — shared: resolve `lit.db` path, connect, schema-ensure.
- `scripts/sync-to-db.py` — upsert master(s) → new active version.
- `scripts/restore-version.py` — old version → active + rewrite master.
- `scripts/list-versions.py` — print version history.
- `scripts/render-prompt.py` — assemble a gloss template + fragments.
- `.claude/skills/{update-gloss-prompt,update-synopsis-prompt,restore-prompt-version,sync-prompts}/SKILL.md`
- `README.md`, `CLAUDE.md`.

**linux-lit (branch `db-backed-api-prompts`, off master — already created):**
- Create `src/db/prompts.rs` — `active_prompt(key) -> Option<String>`; `ensure_api_prompts_table`.
- Modify `src/db/mod.rs` — register `pub mod prompts;`.
- Modify `src/gloss.rs` — DB-first load for the 7 gloss prompts + 2 IPA fragments; consts → `*_FALLBACK`.
- Modify `src/input/actions/synopsis.rs` — DB-first `synopsis.amend`.

**litdb (own branch off its master):**
- Modify `scripts/improve_synopses.py` — DB-first `synopsis.batch`.

---

## Phase 0 — Create the claude-api-prompts repo

### Task 0.1: Scaffold repo + shared db helper

**Files:**
- Create: `~/utono/claude-api-prompts/.gitignore`
- Create: `~/utono/claude-api-prompts/README.md`
- Create: `~/utono/claude-api-prompts/scripts/db.py`

- [ ] **Step 1: Make the directory and git init**

```bash
mkdir -p ~/utono/claude-api-prompts/{prompts,scripts,.claude/skills}
cd ~/utono/claude-api-prompts && git init -q && git branch -M master
```

- [ ] **Step 2: Write `.gitignore`**

```
__pycache__/
*.pyc
```

- [ ] **Step 3: Write `scripts/db.py`** (shared connection + schema)

```python
"""Shared helpers for the claude-api-prompts sync scripts."""
import os
import sqlite3
from pathlib import Path

DB_PATH = Path(os.path.expanduser("~/utono/litdb/data/lit.db"))

SCHEMA = """
CREATE TABLE IF NOT EXISTS api_prompts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    prompt_key  TEXT NOT NULL,
    version     INTEGER NOT NULL,
    text        TEXT NOT NULL,
    is_active   INTEGER NOT NULL DEFAULT 0,
    note        TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(prompt_key, version)
);
CREATE INDEX IF NOT EXISTS idx_api_prompts_active
    ON api_prompts(prompt_key, is_active);
"""


def connect():
    if not DB_PATH.exists():
        raise SystemExit(f"lit.db not found at {DB_PATH}")
    conn = sqlite3.connect(DB_PATH)
    conn.executescript(SCHEMA)
    return conn


def active_version(conn, key):
    row = conn.execute(
        "SELECT version FROM api_prompts WHERE prompt_key=? AND is_active=1",
        (key,),
    ).fetchone()
    return row[0] if row else None


def max_version(conn, key):
    row = conn.execute(
        "SELECT MAX(version) FROM api_prompts WHERE prompt_key=?", (key,)
    ).fetchone()
    return row[0] if row and row[0] is not None else 0
```

- [ ] **Step 4: Write `README.md`**

```markdown
# claude-api-prompts

Versioned Claude-API prompts for linux-lit and litdb, stored in lit.db
(`api_prompts` table). Each `prompts/*.md` master is the editable source;
`scripts/sync-to-db.py` upserts a master as a new ACTIVE version, demoting the
prior active. Git history mirrors the DB version history.

- Edit a master, then `python scripts/sync-to-db.py <key>` (or `--all`).
- `python scripts/list-versions.py [key]` shows history (active marked `*`).
- `python scripts/restore-version.py <key> <version>` re-activates an old
  version AND rewrites the master file so git and DB stay in sync.
- `python scripts/render-prompt.py <gloss-key>` prints the assembled final
  string (template + ipa fragments) as the app composes it.

Consumers read the active prompt at runtime and fall back to a compiled/in-file
copy if the DB row is missing.
```

- [ ] **Step 5: Commit**

```bash
cd ~/utono/claude-api-prompts
git add -A && git commit -q -m "chore: scaffold claude-api-prompts repo + db helper"
```

---

### Task 0.2: Seed master files from current prompt text

The masters MUST be byte-for-byte the current prompt text (v1 = no behavior
change). Capture each from source. Frontmatter records `prompt_key`, `consumer`,
`has_placeholders`.

**Files:** Create all 12 under `~/utono/claude-api-prompts/prompts/`.

- [ ] **Step 1: Extract the current gloss prompt bodies from linux-lit source**

Run (read-only, to copy text into the masters accurately):

```bash
sed -n '99,339p' ~/utono/linux-lit/src/gloss.rs
```

Expected: prints `USER_QUESTION_PROMPT` … `TEACHER_GENERIC_PROMPT` source. Use
these as the master bodies. For the gloss templates, the body is the format
string WITH the literal `{ipa_rules}` placeholder where `{}` / `*IPA_VERSE_RULES`
is interpolated (rename the positional `{}` to the named token `{ipa_rules}` in
the master; the Rust side will substitute by name).

- [ ] **Step 2: Write `prompts/ipa.op-conventions.md`**

Frontmatter then the exact string from the `op_ipa_conventions!` macro body
(`src/gloss.rs:38-64`), de-escaped (join the `\`-continued lines into one
paragraph as the macro produces at compile time).

```markdown
---
prompt_key: ipa.op-conventions
consumer: linux-lit
has_placeholders: false
---
Use Crystal's Shakespearean Original Pronunciation (OP), NOT modern values, for the /IPA/. OP is rhotic — sound every written r and let it colour the vowel (letter /ˈlɛtɚ/, art /ɑrt/). Pin these lexical-set vowels to the OP value, never the modern one: FACE (daily, gave, day) = OP monophthong /eː/ NOT /eɪ/; GOAT (go, so) = /oː/ NOT /əʊ/; PRICE (wise, time, I) = /əɪ/ NOT /aɪ/; CHOICE (boy) = /əɪ/; MOUTH (house, now) = /əʊ/ NOT /aʊ/; happY (city, money) = /əɪ/ NOT /i/; STRUT (love, blood, cut) = /ɤ/; TRAP (bath, path, man) = /a/ (no broad-a); LOT/THOUGHT (lot, call) = /ɑ/; DRESS (bed) = /ɛ/ or /ɛː/; FLEECE (meet) = /eː/~/iː/; GOOSE (food) = /uː/; KIT (sit) = /ɪ/. MEAT–MEET split: great/break/steak keep /ɛː/ (great /ɡrɛːt/, not /ɡriːt/). So daily is /ˈdeːli/ (or /ˈdeɪli/), gave /ɡeːv/, wise /wəɪz/ — never modern diphthongs. Consonants & connected speech (still OP, applied only to a word you are already tagging — never a reason to tag MORE words): suffix -ing → /ɪn/ (calling /ˈkɑlɪn/, singing /ˈsɪŋɪn/). Aspirated wh- → /ʍ/ in which /ʍɪtʃ/, when /ʍɛn/, why /ʍəɪ/, whither — but who, whom, whole keep /h/. Fuller -sion/-tion → /sɪən/ (not /ʃən/) ONLY when the metre admits the extra syllable; otherwise /ʃən/. In casual delivery, drop initial /h/ on unstressed his, her, him, he (who's her best friend → /huːz ə bɛst/), and elide medial /v/ and /ð/ in common words (heaven /ˈhɛən/, even /ˈiːən/, devil /ˈdiːl/, seven /ˈsɛən/, hither /ˈhɪər/). Reduce unstressed function words to their weakest form — and /ən/, of /ə/, to /tə/, for /fər/, my /mɪ/, thou /ðə/, the /ðə/ — but this tells you HOW to render a function word IF you have chosen to tag it for a connected-speech effect; it is NOT licence to tag every function word. The operative / accent-bearing word rule still governs WHAT gets tagged. Include stress markers on multi-syllable tags: primary /ˈ/ and secondary /ˌ/ before the stressed syllable (/ˈdeːli/, /əˈpoːzɪn/). But let line structure, not IPA, govern syllable count — leave -ed and -ion syllabicity to the metre.
```

- [ ] **Step 3: Write `prompts/ipa.verse-rules.md`**

This master holds the `APPEND_IPA=false` branch text (current runtime state).
A `{op_conventions}` placeholder is NOT used here because the false branch has
no OP block. Body = the false-branch string from `src/gloss.rs:80`.

```markdown
---
prompt_key: ipa.verse-rules
consumer: linux-lit
has_placeholders: false
---
Do NOT add /IPA/ pronunciation tags to verse lines. Quote the source words exactly as written, with no phonetic markup of any kind.
```

- [ ] **Step 4: Write `prompts/ipa.verse-rules-sparse.md`** (identical false-branch text)

```markdown
---
prompt_key: ipa.verse-rules-sparse
consumer: linux-lit
has_placeholders: false
---
Do NOT add /IPA/ pronunciation tags to verse lines. Quote the source words exactly as written, with no phonetic markup of any kind.
```

- [ ] **Step 5: Write the 7 gloss template masters**

For each, frontmatter (`consumer: linux-lit`, `has_placeholders: true`) then the
prompt body copied from Step 1's output, with the positional `{}` replaced by the
literal token `{ipa_rules}`. Files:
- `prompts/gloss.user-question.md` (from `USER_QUESTION_PROMPT`, `gloss.rs:99-119`)
- `prompts/gloss.inner-monologue.md` (from `INNER_MONOLOGUE_PROMPT`, `:121-205`)
- `prompts/gloss.inner-monologue-add.md` (from `INNER_MONOLOGUE_ADD_PROMPT`, `:207-238`)
- `prompts/gloss.inner-monologue-edit.md` (from `INNER_MONOLOGUE_EDIT_PROMPT`, `:240-271`)
- `prompts/gloss.edit.md` (from `EDIT_GLOSS_PROMPT`, `:273-294`)
- `prompts/gloss.fix-ipa.md` (from `FIX_IPA_PROMPT` false branch, `:307-309`; `has_placeholders: false`)
- `prompts/gloss.teacher-generic.md` (from `TEACHER_GENERIC_PROMPT`, `:313-339`; its placeholder is filled by `ipa.verse-rules-sparse`, so frontmatter adds `ipa_fragment: ipa.verse-rules-sparse`)

For the templates that use `*IPA_VERSE_RULES` (all but teacher-generic and
fix-ipa), frontmatter adds `ipa_fragment: ipa.verse-rules`.

- [ ] **Step 6: Write `prompts/synopsis.amend.md`**

Frontmatter (`consumer: linux-lit`, `has_placeholders: false`) + the exact
`SYNOPSIS_AMEND_PROMPT` body from `src/input/actions/synopsis.rs:17-33`
(de-escaped: `\n` become real newlines, `\` line-continuations joined).

- [ ] **Step 7: Write `prompts/synopsis.batch.md`**

Frontmatter (`consumer: litdb`, `has_placeholders: false`) + the full
`SYSTEM_PROMPT` triple-quoted body from
`~/utono/litdb/scripts/improve_synopses.py` (capture with
`sed -n '25,/^"""/p' ~/utono/litdb/scripts/improve_synopses.py` — copy from the
line after `SYSTEM_PROMPT = """\` up to the closing `"""`).

- [ ] **Step 8: Commit the seed masters**

```bash
cd ~/utono/claude-api-prompts
git add prompts/ && git commit -q -m "feat: seed prompt masters (v1, verbatim from source)"
```

---

### Task 0.3: sync-to-db.py

**Files:** Create `~/utono/claude-api-prompts/scripts/sync-to-db.py`

- [ ] **Step 1: Write the script**

```python
#!/usr/bin/env python3
"""Upsert prompt master(s) into lit.db api_prompts as a new ACTIVE version.

Usage:
  sync-to-db.py <prompt_key> [<prompt_key> ...]
  sync-to-db.py --all
  sync-to-db.py --all --dry-run
"""
import argparse
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import db  # noqa: E402

PROMPTS_DIR = Path(__file__).resolve().parent.parent / "prompts"


def parse_master(path):
    raw = path.read_text()
    if raw.startswith("---\n"):
        _, fm, body = raw.split("---\n", 2)
        return body.lstrip("\n")
    return raw


def git_subject():
    try:
        return subprocess.check_output(
            ["git", "log", "-1", "--format=%s"],
            cwd=PROMPTS_DIR.parent, text=True,
        ).strip()
    except Exception:
        return None


def keys_from_args(args):
    if args.all:
        return sorted(p.stem for p in PROMPTS_DIR.glob("*.md"))
    return args.keys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("keys", nargs="*")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    keys = keys_from_args(args)
    if not keys:
        ap.error("give one or more prompt_keys or --all")

    conn = db.connect()
    note = git_subject()
    try:
        for key in keys:
            path = PROMPTS_DIR / f"{key}.md"
            if not path.exists():
                raise SystemExit(f"no master for key '{key}' at {path}")
            text = parse_master(path)
            new_version = db.max_version(conn, key) + 1
            current = conn.execute(
                "SELECT text FROM api_prompts WHERE prompt_key=? AND is_active=1",
                (key,),
            ).fetchone()
            if current and current[0] == text:
                print(f"  {key}: unchanged (active v{db.active_version(conn, key)})")
                continue
            print(f"  {key}: -> v{new_version} (active)"
                  + (" [dry-run]" if args.dry_run else ""))
            if args.dry_run:
                continue
            conn.execute(
                "UPDATE api_prompts SET is_active=0 WHERE prompt_key=?", (key,)
            )
            conn.execute(
                "INSERT INTO api_prompts(prompt_key, version, text, is_active, note)"
                " VALUES(?,?,?,1,?)",
                (key, new_version, text, note),
            )
        if not args.dry_run:
            conn.commit()
    finally:
        conn.close()


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Make executable + dry-run all**

```bash
cd ~/utono/claude-api-prompts
chmod +x scripts/sync-to-db.py
python scripts/sync-to-db.py --all --dry-run
```

Expected: lists all 12 keys `-> v1 (active) [dry-run]`, no DB write.

- [ ] **Step 3: Real sync of all 12 (seed v1)**

```bash
python scripts/sync-to-db.py --all
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT prompt_key, version, is_active FROM api_prompts ORDER BY prompt_key;"
```

Expected: 12 rows, all `version=1`, `is_active=1`.

- [ ] **Step 4: Commit**

```bash
git add scripts/sync-to-db.py && git commit -q -m "feat: sync-to-db.py (upsert master -> active version)"
```

---

### Task 0.4: list / restore / render scripts

**Files:** Create `list-versions.py`, `restore-version.py`, `render-prompt.py` in `scripts/`.

- [ ] **Step 1: Write `scripts/list-versions.py`**

```python
#!/usr/bin/env python3
"""Show prompt version history (active marked '*'). Usage: list-versions.py [key]"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import db  # noqa: E402

def main():
    key = sys.argv[1] if len(sys.argv) > 1 else None
    conn = db.connect()
    q = ("SELECT prompt_key, version, is_active, substr(note,1,50), created_at "
         "FROM api_prompts")
    params = ()
    if key:
        q += " WHERE prompt_key=?"
        params = (key,)
    q += " ORDER BY prompt_key, version"
    for k, v, act, note, ts in conn.execute(q, params):
        mark = "*" if act else " "
        print(f"{mark} {k:28} v{v:<3} {ts}  {note or ''}")
    conn.close()

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Write `scripts/restore-version.py`**

```python
#!/usr/bin/env python3
"""Re-activate an old prompt version AND rewrite its master file.

Usage: restore-version.py <prompt_key> <version> [--dry-run]
"""
import argparse
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import db  # noqa: E402

PROMPTS_DIR = Path(__file__).resolve().parent.parent / "prompts"


def rewrite_master(key, text):
    path = PROMPTS_DIR / f"{key}.md"
    raw = path.read_text()
    if raw.startswith("---\n"):
        _, fm, _ = raw.split("---\n", 2)
        path.write_text(f"---\n{fm}---\n{text}")
    else:
        path.write_text(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("key")
    ap.add_argument("version", type=int)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    conn = db.connect()
    row = conn.execute(
        "SELECT text FROM api_prompts WHERE prompt_key=? AND version=?",
        (args.key, args.version),
    ).fetchone()
    if not row:
        raise SystemExit(f"no {args.key} v{args.version}")
    print(f"restore {args.key} -> v{args.version} active"
          + (" [dry-run]" if args.dry_run else ""))
    if args.dry_run:
        return
    conn.execute("UPDATE api_prompts SET is_active=0 WHERE prompt_key=?", (args.key,))
    conn.execute(
        "UPDATE api_prompts SET is_active=1 WHERE prompt_key=? AND version=?",
        (args.key, args.version),
    )
    conn.commit()
    conn.close()
    rewrite_master(args.key, row[0])
    print(f"  master rewritten: prompts/{args.key}.md (commit it)")


if __name__ == "__main__":
    main()
```

- [ ] **Step 3: Write `scripts/render-prompt.py`**

```python
#!/usr/bin/env python3
"""Print the assembled final prompt (gloss template + ipa fragment).

Usage: render-prompt.py <gloss_key>
Reads the active template; substitutes {ipa_rules} with the active text of the
fragment named in the master's `ipa_fragment` frontmatter (default
ipa.verse-rules). Mirrors how linux-lit composes the prompt at runtime.
"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import db  # noqa: E402

PROMPTS_DIR = Path(__file__).resolve().parent.parent / "prompts"


def frontmatter(key):
    path = PROMPTS_DIR / f"{key}.md"
    raw = path.read_text()
    meta = {}
    if raw.startswith("---\n"):
        _, fm, _ = raw.split("---\n", 2)
        for line in fm.splitlines():
            if ":" in line:
                k, v = line.split(":", 1)
                meta[k.strip()] = v.strip()
    return meta


def active_text(conn, key):
    row = conn.execute(
        "SELECT text FROM api_prompts WHERE prompt_key=? AND is_active=1", (key,)
    ).fetchone()
    return row[0] if row else None


def main():
    key = sys.argv[1]
    conn = db.connect()
    template = active_text(conn, key)
    if template is None:
        raise SystemExit(f"no active {key}")
    if "{ipa_rules}" in template:
        frag_key = frontmatter(key).get("ipa_fragment", "ipa.verse-rules")
        frag = active_text(conn, frag_key) or ""
        template = template.replace("{ipa_rules}", frag)
    print(template)
    conn.close()


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Verify round-trip (sync → list → restore dry-run → render)**

```bash
cd ~/utono/claude-api-prompts
chmod +x scripts/*.py
python scripts/list-versions.py gloss.teacher-generic
python scripts/render-prompt.py gloss.teacher-generic | head -5
python scripts/restore-version.py gloss.teacher-generic 1 --dry-run
```

Expected: list shows `* gloss.teacher-generic v1`; render prints the assembled
prompt with the no-IPA verse rule inlined; restore dry-run prints the plan.

- [ ] **Step 5: Commit**

```bash
git add scripts/list-versions.py scripts/restore-version.py scripts/render-prompt.py
git commit -q -m "feat: list/restore/render prompt scripts"
```

---

## Phase 1 — linux-lit reads prompts from the DB

### Task 1.1: `src/db/prompts.rs` with tests

**Files:**
- Create: `src/db/prompts.rs`
- Modify: `src/db/mod.rs`
- Test: inline `#[cfg(test)]` in `src/db/prompts.rs`

- [ ] **Step 1: Write the failing test**

Create `src/db/prompts.rs` with ONLY the test module first:

```rust
//! Read the active Claude-API prompt for a given key from lit.db `api_prompts`.

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    fn seed(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE api_prompts (id INTEGER PRIMARY KEY, prompt_key TEXT, \
             version INTEGER, text TEXT, is_active INTEGER, note TEXT, \
             created_at TEXT);",
        )
        .unwrap();
    }

    #[test]
    fn returns_active_text() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        conn.execute(
            "INSERT INTO api_prompts(prompt_key,version,text,is_active) \
             VALUES('k',1,'old',0),('k',2,'new',1)",
            [],
        )
        .unwrap();
        assert_eq!(super::active_prompt_in(&conn, "k"), Some("new".to_string()));
    }

    #[test]
    fn returns_none_when_absent() {
        let conn = Connection::open_in_memory().unwrap();
        seed(&conn);
        assert_eq!(super::active_prompt_in(&conn, "missing"), None);
    }
}
```

- [ ] **Step 2: Run the test, expect failure**

Run: `cargo test --bins -p linux-lit db::prompts 2>&1 | tail -20`
Expected: FAIL — `cannot find function active_prompt_in`.

- [ ] **Step 3: Implement the module body** (prepend above the test module)

```rust
use rusqlite::{Connection, OptionalExtension};

/// Read the active prompt text for `key` from an explicit connection.
/// Returns `None` if no active row exists or on any query error.
pub fn active_prompt_in(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT text FROM api_prompts WHERE prompt_key = ?1 AND is_active = 1 \
         ORDER BY version DESC LIMIT 1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// Open lit.db read-only and return the active prompt for `key`, or `None`
/// (missing row, missing table, or DB unavailable — caller falls back).
pub fn active_prompt(key: &str) -> Option<String> {
    let conn = crate::db::queries::open_db().ok()?;
    active_prompt_in(&conn, key)
}
```

- [ ] **Step 4: Register the module** in `src/db/mod.rs`

Add alongside the other `pub mod` lines:

```rust
pub mod prompts;
```

- [ ] **Step 5: Run the tests, expect pass**

Run: `cargo test --bins db::prompts 2>&1 | tail -20`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add src/db/prompts.rs src/db/mod.rs
git commit -m "feat(db): active_prompt reader for api_prompts (DB-first, None on miss)"
```

---

### Task 1.2: Wire gloss.rs prompts to DB-first

**Files:** Modify `src/gloss.rs`

- [ ] **Step 1: Add a DB-or-fallback template helper** near the top of `src/gloss.rs` (after the `use` lines)

```rust
/// Active template for `key` from lit.db, or the compiled `fallback` verbatim.
fn template_or(key: &str, fallback: &str) -> String {
    crate::db::prompts::active_prompt(key).unwrap_or_else(|| fallback.to_string())
}
```

- [ ] **Step 2: Convert the IPA fragments to DB-first**

Replace the `IPA_VERSE_RULES` initializer body so the `APPEND_IPA == false`
branch reads the DB first:

```rust
static IPA_VERSE_RULES: LazyLock<String> = LazyLock::new(|| {
    if APPEND_IPA {
        concat!(
            "On each <verse> line, APPEND inline Original-Pronunciation IPA in forward slashes IMMEDIATELY AFTER the operative / accent-bearing / metrically stressed words (e.g. take /tɛːk/), leaving the original words unchanged; per word never per phrase; let line structure govern syllable count.\n- ",
            op_ipa_conventions!()
        )
        .to_string()
    } else {
        template_or(
            "ipa.verse-rules",
            "Do NOT add /IPA/ pronunciation tags to verse lines. Quote the source words exactly as written, with no phonetic markup of any kind.",
        )
    }
});
```

Apply the same change to `IPA_VERSE_RULES_SPARSE`'s `else` branch with key
`"ipa.verse-rules-sparse"` and the identical fallback string.

- [ ] **Step 3: Convert each gloss prompt `LazyLock` to DB-first**

For each of the 6 templated prompts (`USER_QUESTION_PROMPT`,
`INNER_MONOLOGUE_PROMPT`, `INNER_MONOLOGUE_ADD_PROMPT`,
`INNER_MONOLOGUE_EDIT_PROMPT`, `EDIT_GLOSS_PROMPT`, `TEACHER_GENERIC_PROMPT`),
keep the existing literal as the fallback and wrap with `template_or`. Pattern
(shown for `TEACHER_GENERIC_PROMPT`; the DB master uses `{ipa_rules}` so
substitute by name, not positional `{}`):

```rust
static TEACHER_GENERIC_PROMPT: LazyLock<String> = LazyLock::new(|| {
    const FALLBACK: &str = "\
You are a performance-focused teacher helping a reader understand a passage from a literary text.
... (the existing literal body, with the {} kept as {}) ...
- No markdown, no bullets, no numbered lists, no headers";
    let template = template_or("gloss.teacher-generic", FALLBACK);
    // DB master uses the named token {ipa_rules}; the compiled fallback uses {}.
    if template.contains("{ipa_rules}") {
        template.replace("{ipa_rules}", &IPA_VERSE_RULES_SPARSE)
    } else {
        format!("{}", template).replacen("{}", &IPA_VERSE_RULES_SPARSE, 1)
    }
});
```

For the 5 prompts that use `*IPA_VERSE_RULES` (not sparse), substitute
`&IPA_VERSE_RULES` instead. Keys: `gloss.user-question`, `gloss.inner-monologue`,
`gloss.inner-monologue-add`, `gloss.inner-monologue-edit`, `gloss.edit`.

For `FIX_IPA_PROMPT` (no placeholder), wrap only the active branch:

```rust
pub static FIX_IPA_PROMPT: LazyLock<String> = LazyLock::new(|| {
    if APPEND_IPA {
        "... existing true-branch literal ...".to_string()
    } else {
        template_or(
            "gloss.fix-ipa",
            "IPA pronunciation tagging is currently disabled. Return ONLY two forward slashes with nothing between them (//) and no other text.",
        )
    }
});
```

- [ ] **Step 4: Build**

Run: `cargo build 2>&1 | tail -15`
Expected: compiles clean (warnings ok).

- [ ] **Step 5: Add a fallback unit test** to `src/gloss.rs` `#[cfg(test)]`

```rust
#[test]
fn teacher_generic_has_no_unfilled_placeholder() {
    // Whether sourced from DB or fallback, the {ipa_rules}/{} slot must be filled.
    let p = &*TEACHER_GENERIC_PROMPT;
    assert!(!p.contains("{ipa_rules}"), "ipa_rules token left unfilled");
    assert!(!p.contains("{}"), "positional placeholder left unfilled");
    assert!(p.contains("Defines literary terminology"));
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --bins gloss 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/gloss.rs
git commit -m "feat(gloss): load gloss prompts + ipa fragments from lit.db (compiled fallback)"
```

---

### Task 1.3: Wire synopsis amend to DB-first

**Files:** Modify `src/input/actions/synopsis.rs`

- [ ] **Step 1: Replace the const usage at the call site**

The const `SYNOPSIS_AMEND_PROMPT` is used at `synopsis.rs:104`
(`send_message(SYNOPSIS_AMEND_PROMPT, ...)`). Keep the const as the fallback;
resolve DB-first just before the call. Find the block that builds `user_msg` and
calls `send_message`, and change the system-prompt argument:

```rust
let system_prompt = crate::db::prompts::active_prompt("synopsis.amend")
    .unwrap_or_else(|| SYNOPSIS_AMEND_PROMPT.to_string());
// ... inside the async/tokio call:
crate::claude::send_message(&system_prompt, &user_msg, &model).await
```

(`system_prompt` must be computed where it can be moved into the async task;
clone if the closure requires `'static`.)

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -15`
Expected: compiles clean.

- [ ] **Step 3: Run the full bins test suite**

Run: `cargo test --bins 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/input/actions/synopsis.rs
git commit -m "feat(synopsis): load amend prompt from lit.db (compiled fallback)"
```

---

## Phase 2 — litdb reads the batch synopsis prompt from the DB

### Task 2.1: improve_synopses.py DB-first

**Files:** Modify `~/utono/litdb/scripts/improve_synopses.py` (own branch off litdb master)

- [ ] **Step 1: Create the litdb branch**

```bash
cd ~/utono/litdb
git checkout master && git pull --ff-only 2>/dev/null; git checkout -b db-backed-api-prompts
```

(If `master` has uncommitted work, commit it on its branch first, as in the
linux-lit setup.)

- [ ] **Step 2: Add a DB-first resolver** just after the `SYSTEM_PROMPT = """..."""`
definition in `improve_synopses.py` (keep the literal as `SYSTEM_PROMPT_FALLBACK`
by renaming, OR keep the name and add a getter — use a getter to minimize diff):

```python
def resolve_system_prompt():
    """Active synopsis.batch from lit.db, or the in-file SYSTEM_PROMPT fallback."""
    try:
        conn = sqlite3.connect(DB_PATH)
        row = conn.execute(
            "SELECT text FROM api_prompts WHERE prompt_key='synopsis.batch' "
            "AND is_active=1 ORDER BY version DESC LIMIT 1"
        ).fetchone()
        conn.close()
        if row and row[0]:
            return row[0]
    except sqlite3.Error:
        pass
    return SYSTEM_PROMPT
```

- [ ] **Step 3: Use it at the submission site**

At the call that passes `SYSTEM_PROMPT` (line ~292,
`chunks, SYSTEM_PROMPT,`), replace with the resolved value:

```python
        chunks, resolve_system_prompt(),
```

- [ ] **Step 4: Dry-run to confirm it reads the DB prompt**

```bash
cd ~/utono/litdb
python scripts/improve_synopses.py --dry-run Ham 2>&1 | tail -20
```

Expected: runs, reports request count, no submission. (DB prompt is active v1,
identical to the literal, so behavior is unchanged.)

- [ ] **Step 5: Commit (litdb repo)**

```bash
cd ~/utono/litdb
git add scripts/improve_synopses.py
git commit -m "feat(synopses): read batch system prompt from lit.db api_prompts (fallback to literal)"
```

---

## Phase 3 — Content update (version 2) + skills

### Task 3.1: Update gloss.teacher-generic master (rhetorical + Eleanor)

**Files:** Modify `~/utono/claude-api-prompts/prompts/gloss.teacher-generic.md`

- [ ] **Step 1: Edit the master body**

Strengthen the existing rule list. Keep the XML format rules and the
`{ipa_rules}` token. Replace the three delivery/device bullets with sharper
instructions modeled on gloss 21741 and the Eleanor voice. Concretely, the
`<gloss>` analysis section gains these rules (insert into the existing bullet
list, do not remove the format rules):

```
- When a rhetorical device shapes the line, NAME it (anaphora, caesura, enjambment, antithesis, epistrophe, chiasmus), DEFINE it inline on first use in plain words (e.g. "this is anaphora, the repetition of a word at the start of successive clauses"), and tie it to the operative words and the actor's breath/drive — how the device should move the voice forward, gather energy, or pause. Follow the model of explaining the device AND its delivery effect in the same breath.
- Frame delivery for a slow, deliberate, coaxing, low-register voice: where the line invites it, cue a sultry, insinuating, half-coaxing/half-commanding delivery — words lingered over, intention velveted under warmth. Let the analysis suggest pace and weight (where to slow, where to press, where the breath extends outward) so the explication complements an unhurried, seductive chest-register reading.
- Reference voice/verse coaches by name where apt (Rodenburg, Berry, Barton, Hall, Linklater) for the breath and drive of a line, as a working actor would.
```

- [ ] **Step 2: Sync as v2**

```bash
cd ~/utono/claude-api-prompts
git add prompts/gloss.teacher-generic.md
git commit -q -m "feat: gloss.teacher-generic v2 — rhetorical devices + Eleanor voice"
python scripts/sync-to-db.py gloss.teacher-generic
python scripts/list-versions.py gloss.teacher-generic
```

Expected: list shows `v1` and `* v2` (v2 active, note = the commit subject).

- [ ] **Step 3: Verify the assembled render**

```bash
python scripts/render-prompt.py gloss.teacher-generic | tail -20
```

Expected: prints the new rules, `{ipa_rules}` replaced by the no-IPA line.

---

### Task 3.2: Update the synopsis masters (v2)

**Files:** Modify `prompts/synopsis.amend.md`, `prompts/synopsis.batch.md`

- [ ] **Step 1: Edit `synopsis.amend.md`**

Append to the existing instruction (before the FORMAT block) one rule:

```
Where a scene turns on a rhetorical or rhetorical-performance moment (a speech built on anaphora, a sharp antithesis, a sustained metaphor), you may note it briefly in plain words, naming the device, so the synopsis primes a reader who will hear the scene performed in a slow, deliberate, persuasive register.
```

- [ ] **Step 2: Edit `synopsis.batch.md`**

Add one bullet to the numbered "improved synopsis that:" list:

```
8. **Flags a defining rhetorical or performance moment** when one anchors the scene — name the device (anaphora, antithesis, a sustained image) in plain words — so the synopsis complements a slow, deliberate, persuasive narration. Keep this to a single clause; do not turn the synopsis into analysis.
```

(Leave the `Do NOT add interpretation or thematic analysis` line; the single
device-clause is descriptive, not interpretive — keep it minimal.)

- [ ] **Step 3: Sync both as v2**

```bash
cd ~/utono/claude-api-prompts
git add prompts/synopsis.amend.md prompts/synopsis.batch.md
git commit -q -m "feat: synopsis amend+batch v2 — rhetorical-moment clause + voice complement"
python scripts/sync-to-db.py synopsis.amend synopsis.batch
python scripts/list-versions.py | grep synopsis
```

Expected: each synopsis key shows `v1` and `* v2`.

---

### Task 3.3: Write the four skills

**Files:** Create the four `SKILL.md` files under `~/utono/claude-api-prompts/.claude/skills/`.

Before writing, READ `superpowers:writing-skills` (per the global CLAUDE.md
convention). Each skill is <500 words, frontmatter `name` / `description`
(starting "Use when…") / `argument-hint`.

- [ ] **Step 1: `update-gloss-prompt/SKILL.md`**

Frontmatter then body. Description: "Use when editing any gloss explication
prompt linux-lit sends to Claude (teacher-generic, user-question, edit,
inner-monologue variants, fix-ipa, or the shared OP-IPA fragments)…".
Body: edit `prompts/<key>.md`, then
`python scripts/sync-to-db.py <key>`, verify with `render-prompt.py` and
`list-versions.py`; note that linux-lit reads it on next launch; keys list.

```yaml
---
name: update-gloss-prompt
description: Use when editing a gloss explication prompt linux-lit sends to Claude — teacher-generic, user-question, edit, inner-monologue, fix-ipa, or the shared OP-IPA fragments stored in lit.db api_prompts
argument-hint: <prompt-key>
---
```

- [ ] **Step 2: `update-synopsis-prompt/SKILL.md`**

Description covers `synopsis.amend` (linux-lit) and `synopsis.batch` (litdb).
Body mirrors update-gloss-prompt; notes litdb's `improve_synopses.py` reads
`synopsis.batch`.

- [ ] **Step 3: `restore-prompt-version/SKILL.md`**

Description: "Use when reverting a Claude-API prompt to an earlier version…".
Body: `list-versions.py <key>` to find the version, then
`restore-version.py <key> <version>`, then commit the rewritten master.

- [ ] **Step 4: `sync-prompts/SKILL.md`**

Description: "Use when pushing edited prompt masters into lit.db…". Body:
`sync-to-db.py --all` / per-key, `--dry-run` first.

- [ ] **Step 5: Write `CLAUDE.md` for the repo**

Document: masters in `prompts/`, the `api_prompts` table is the runtime source,
consumers (linux-lit `src/db/prompts.rs`, litdb `improve_synopses.py`) fall back
to compiled/in-file copies, sync after every edit, restore = activate + rewrite
master + commit.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/claude-api-prompts
git add .claude CLAUDE.md && git commit -q -m "feat: prompt-editing skills + CLAUDE.md"
```

---

### Task 3.4: Create the private GitHub remote + push

**Files:** none (git/remote ops)

- [ ] **Step 1: Create the private repo and push**

```bash
cd ~/utono/claude-api-prompts
gh repo create utono/claude-api-prompts --private --source=. --remote=origin --push
```

Expected: repo created on github.com (account `utono`), `origin` set, branch
`master` pushed.

- [ ] **Step 2: Verify**

```bash
git remote -v && git log --oneline | head
gh repo view utono/claude-api-prompts --json visibility --jq .visibility
```

Expected: remote `origin` present; `visibility = PRIVATE`.

---

## Phase 4 — Verification

### Task 4.1: Build, test, and request runtime verification

- [ ] **Step 1: linux-lit build + full bins tests**

```bash
cd ~/utono/linux-lit
cargo build 2>&1 | tail -5
cargo test --bins 2>&1 | tail -20
```

Expected: build clean; all bins tests pass.

- [ ] **Step 2: Confirm DB has v2 active for the three content prompts**

```bash
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT prompt_key, version FROM api_prompts WHERE is_active=1 \
   AND prompt_key IN ('gloss.teacher-generic','synopsis.amend','synopsis.batch') \
   ORDER BY prompt_key;"
```

Expected: all three at `version=2`.

- [ ] **Step 3: Ask the user to verify at runtime** (per CLAUDE.md "do not run the app")

The acceptance criterion is visual/behavioral (a real gloss renders with the new
rhetorical phrasing). State that bins tests pass but runtime is user-verified,
and ask the user to:

```bash
cd ~/utono/linux-lit && cargo run
```

then open a gloss (e.g. via `h` then `Ctrl+g`, or regenerate a gloss) and
confirm the explication names/defines rhetorical devices and reads in the
Eleanor register. Also confirm fallback by temporarily nothing-needed (DB is
seeded). Paste the gloss text back.

- [ ] **Step 4: Final linux-lit commit (if any uncommitted)**

```bash
cd ~/utono/linux-lit && git status --porcelain
# commit anything outstanding with an appropriate message
```

---

## Self-Review notes

- **Spec coverage:** table (Task 0.1), repo masters+scripts (0.2–0.4), private
  remote (3.4), linux-lit DB-read+fallback (1.1–1.3), litdb DB-read+fallback
  (2.1), placeholders/fragments preserved (0.2 step 5, 1.2 step 3), content v2
  rhetorical+Eleanor (3.1–3.2), skills (3.3), testing/verification (4.1). All
  spec sections mapped.
- **Fallback names:** the Rust fallbacks reuse the existing literals (kept inline
  as `FALLBACK`/branch text), so no missing symbols. `active_prompt` /
  `active_prompt_in` names are consistent across Tasks 1.1–1.3.
- **No behavior change at v1:** masters are verbatim copies; only Phase 3 changes
  content.
- **Branching:** linux-lit `db-backed-api-prompts` (off master, created); litdb
  gets its own same-named branch (2.1); claude-api-prompts is a fresh repo.
```

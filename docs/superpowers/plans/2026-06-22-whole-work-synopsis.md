# Whole-Work Synopsis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface a whole-work synopsis as the first position in the synopsis overlay (stored at `(div1,div2)=(0,0)`, reachable via `p`/`n` and new `Ctrl+p`/`Ctrl+n`, editable via the existing `A`/`E`/`U`), and add a batch script that generates one per Shakespeare play + the narrative poems.

**Architecture:** Reuse the existing scene-synopsis machinery end to end. `(0,0)` is an unused `scene_synopses` key (real scenes are 1-indexed), so the whole-work synopsis is just another cached synopsis: `load_synopses` already loads it, `ordered_synopsis_scenes` prepends it, `cycle_synopsis`/`synopsis_label`/amend/edit/undo all key on `(div1,div2)` and need no special-casing beyond ordering + label. A new Python batch script (modeled on `chapter_synopses.py`) generates the `(0,0)` rows from Claude's training knowledge.

**Tech Stack:** Rust + GTK4 (app side); Python 3 + the repo's `common.claude_api` Batch API helpers + `rusqlite`-compatible SQLite at `~/utono/litdb/data/lit.db` (script side).

## Global Constraints

- Build check only: `cargo build`. **Do NOT run the app** (`cargo run`) — the user runs it. (CLAUDE.md)
- Pure-logic verification: `cargo test --bins`, `cargo clippy`. Visual/runtime acceptance (the overlay rendering the whole-work synopsis, editing it) is user-gated via `cage`.
- The `(0,0)` key denotes the whole-work synopsis. Real scenes are `div1 >= 1`; chapter works use `(n,0)` with `n >= 1`. `(0,0)` is unused for every work.
- All synopsis load/save keys on `crate::app::base_work_abbrev(&w.abbrev)` (so `-Amb`/`-BBC`/etc. editions share one synopsis). This is the established convention.
- The generation script writes to `scene_synopses` as `(base_abbrev, 0, 0, synopsis, 'claude-opus-4-8')`, upsert on `(work_abbrev, div1, div2)`.
- Script targets (base abbrevs only): the 37 plays + TNK + Ven, Luc, LC, PhT. EXCLUDE Son and BenCrystalOP. (Full list in Task 5.)
- Do NOT run the generation script as part of implementation (it spends API tokens) — the deliverable is the script + a `--dry-run` that works. The user runs the real generation.
- Commit message footer:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs
  ```
- Branch: `feat/qa-journal-overlay` (continue on it).

---

### Task 1: `ordered_synopsis_scenes` prepends `(0,0)`

**Files:**
- Modify: `src/app.rs:6236-6265` (`ordered_synopsis_scenes`)
- Test: inline `#[cfg(test)]` in `src/app.rs` (or extend an existing one) — but see note: this fn reads `AppState`, which is heavy to construct in a unit test. Use a thin pure helper instead (below).

**Interfaces:**
- Consumes: `AppState.synopsis_cache: HashMap<(i64,i64), String>`.
- Produces: `ordered_synopsis_scenes(&AppState) -> Vec<(i64,i64)>` with `(0,0)` first when cached. A pure helper `prepend_whole_work(has_whole_work: bool, rest: Vec<(i64,i64)>) -> Vec<(i64,i64)>` is the unit-tested seam.

The current function has two return paths (chapter-work branch returns early; scene branch falls through). To keep the change small and testable, factor the "put `(0,0)` first" decision into a pure helper and call it from both paths.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/app.rs` (create one at end of file if none exists for these helpers):

```rust
#[test]
fn prepend_whole_work_puts_zero_zero_first() {
    let rest = vec![(1, 1), (1, 2), (2, 1)];
    assert_eq!(
        super::prepend_whole_work(true, rest.clone()),
        vec![(0, 0), (1, 1), (1, 2), (2, 1)]
    );
}

#[test]
fn prepend_whole_work_absent_is_unchanged() {
    let rest = vec![(1, 1), (1, 2)];
    assert_eq!(super::prepend_whole_work(false, rest.clone()), rest);
}

#[test]
fn prepend_whole_work_empty_rest() {
    assert_eq!(super::prepend_whole_work(true, vec![]), vec![(0, 0)]);
    assert_eq!(super::prepend_whole_work(false, vec![]), Vec::<(i64, i64)>::new());
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test --bins prepend_whole_work`
Expected: FAIL to compile — `prepend_whole_work` not defined.

- [ ] **Step 3: Add the pure helper**

Insert immediately above `fn ordered_synopsis_scenes` in `src/app.rs`:

```rust
/// Put the whole-work synopsis key `(0,0)` first when it exists, otherwise
/// return `rest` unchanged. Pure seam for `ordered_synopsis_scenes`.
fn prepend_whole_work(has_whole_work: bool, rest: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    if has_whole_work {
        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push((0, 0));
        out.extend(rest);
        out
    } else {
        rest
    }
}
```

- [ ] **Step 4: Wire it into `ordered_synopsis_scenes`**

Replace `ordered_synopsis_scenes` (`src/app.rs:6236-6265`) with the version that
builds the existing list, then prepends `(0,0)`. The body is unchanged except the
two `return`/fall-through points now route through `prepend_whole_work`:

```rust
fn ordered_synopsis_scenes(s: &AppState) -> Vec<(i64, i64)> {
    let has_whole_work = s.synopsis_cache.contains_key(&(0, 0));

    if is_chapter_work(s) {
        let work = match s.current_work.as_ref() {
            Some(w) => w,
            None => return Vec::new(),
        };
        let chapter_count = work.lines.iter().filter(|l| l.is_chapter).count();
        let mut keys = Vec::new();
        for n in 1..=chapter_count {
            let k = (n as i64, 0);
            if s.synopsis_cache.contains_key(&k) {
                keys.push(k);
            }
        }
        return prepend_whole_work(has_whole_work, keys);
    }

    let work = match s.current_work.as_ref() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();
    for line in &work.lines {
        let k = (line.div1, line.div2);
        // Never list (0,0) as a scene from the line loop — it's the whole-work
        // key, prepended separately. (Lines always have div1 >= 1, but guard
        // anyway so a stray (0,0) line can't double it.)
        if k != (0, 0) && seen.insert(k) && s.synopsis_cache.contains_key(&k) {
            keys.push(k);
        }
    }
    prepend_whole_work(has_whole_work, keys)
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --bins prepend_whole_work`
Expected: all three PASS.

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(synopsis): order whole-work (0,0) synopsis first

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 2: `synopsis_label(0,0)` → "Whole work"

**Files:**
- Modify: `src/app.rs:5597-5603` (`synopsis_label`)
- Test: inline `#[cfg(test)]` via a pure helper.

**Interfaces:**
- Consumes: nothing new.
- Produces: `synopsis_label(&AppState, 0, 0)` returns `"Whole work"`. A pure helper `whole_work_label(div1, div2) -> Option<&'static str>` is the tested seam.

`synopsis_label` reads `AppState` (via `is_chapter_work`/`scene_label_for`), so test the `(0,0)` decision through a pure helper.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/app.rs`:

```rust
#[test]
fn whole_work_label_only_for_zero_zero() {
    assert_eq!(super::whole_work_label(0, 0), Some("Whole work"));
    assert_eq!(super::whole_work_label(1, 1), None);
    assert_eq!(super::whole_work_label(2, 0), None);
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test --bins whole_work_label`
Expected: FAIL to compile — `whole_work_label` not defined.

- [ ] **Step 3: Add the pure helper**

Insert immediately above `fn synopsis_label` in `src/app.rs`:

```rust
/// The fixed label for the whole-work synopsis position `(0,0)`, or `None` for
/// any real scene/chapter key. Pure seam for `synopsis_label`.
fn whole_work_label(div1: i64, div2: i64) -> Option<&'static str> {
    if (div1, div2) == (0, 0) {
        Some("Whole work")
    } else {
        None
    }
}
```

- [ ] **Step 4: Use it in `synopsis_label`**

Replace `synopsis_label` (`src/app.rs:5597-5603`) with:

```rust
pub fn synopsis_label(state: &AppState, div1: i64, div2: i64) -> String {
    if let Some(label) = whole_work_label(div1, div2) {
        return label.to_string();
    }
    if is_chapter_work(state) {
        format!("Chapter {}", div1)
    } else {
        scene_label_for(state, div1, div2)
    }
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test --bins whole_work_label`
Expected: PASS.

- [ ] **Step 6: Build**

Run: `cargo build`
Expected: compiles clean.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(synopsis): label the (0,0) position 'Whole work'

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 3: `Ctrl+p` / `Ctrl+n` aliases in the synopsis overlay

**Files:**
- Modify: `src/input/keymap.rs:1248-1255` (the `n`/`p` arms in `handle_synopsis_overlay_key`)

**Interfaces:**
- Consumes: `crate::app::cycle_synopsis(state, delta)` (existing).
- Produces: `Ctrl+n`/`Ctrl+p` handled identically to `n`/`p` in the synopsis overlay.

Plain `p`/`n` already cycle synopses (and reach `(0,0)` by wraparound after Task 1). This adds the Ctrl aliases the user asked for. The handler already receives `is_ctrl`.

- [ ] **Step 1: Add the Ctrl-guarded arms**

In `handle_synopsis_overlay_key` (`src/input/keymap.rs`), replace the `n`/`p`
arms (`:1248-1255`):

```rust
        "n" => {
            crate::app::cycle_synopsis(state, 1);
            true
        }
        "p" => {
            crate::app::cycle_synopsis(state, -1);
            true
        }
```

with (guarded Ctrl arms first, so the modifier matches before the plain arm):

```rust
        "n" if is_ctrl => {
            crate::app::cycle_synopsis(state, 1);
            true
        }
        "p" if is_ctrl => {
            crate::app::cycle_synopsis(state, -1);
            true
        }
        "n" => {
            crate::app::cycle_synopsis(state, 1);
            true
        }
        "p" => {
            crate::app::cycle_synopsis(state, -1);
            true
        }
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles clean. (If the compiler warns the `if is_ctrl` arm is unreachable because an earlier arm in the same `match` already matches `"n"`/`"p"` unconditionally, move the guarded arms ABOVE any such arm — there is none above `:1248` in this match, so this is fine.)

- [ ] **Step 3: Commit**

```bash
git add src/input/keymap.rs
git commit -m "feat(synopsis): Ctrl+p/Ctrl+n cycle synopses (alias of p/n)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 4: Document cycling + whole-work in the Ctrl+/ overlay

**Files:**
- Modify: `src/ui/keybinds_overlay.rs:536-544` (the `"synopsis"` describe arm)

**Interfaces:**
- Consumes: nothing (descriptive text only).
- Produces: the synopsis detail panel documents `p`/`n` + `Ctrl+p`/`Ctrl+n` cycling and the whole-work synopsis.

The current `"synopsis"` describe arm does not mention scene cycling at all. Add a
sentence covering it and the whole-work position.

- [ ] **Step 1: Update the `"synopsis"` describe arm**

Replace the `"synopsis"` arm (`src/ui/keybinds_overlay.rs:536-544`) with (adds the
cycling sentence; the rest is unchanged):

```rust
        "synopsis" => "Show the synopsis overlay for the current scene. Inside \
it, j/k move the cursor block (left accent bar) and gg/G jump first/last, like \
the gloss overlay; Space (or Tab) plays/stops the cursor paragraph's TTS \
(synthesizing on a cache miss); Shift+Space batch-synthesizes every synopsis paragraph to \
cached ElevenLabs MP3s (cache only, no playback), showing a \u{201c}Synthesizing\u{2026}\u{201d} \
toast. p / n (or Ctrl+p / Ctrl+n) cycle to the previous / next scene's synopsis, \
wrapping through a whole-work synopsis that sorts first (before Act 1) \u{2014} so \
from Act 1 Scene 1, p shows the whole-work overview. A amends and E edits the \
current synopsis (including the whole-work one); U undoes the last amend. \
-> app::show_synopsis_overlay — src/app.rs; \
gloss::read_current_synopsis_block, gloss::synth_all_synopsis_blocks \
— src/input/actions/gloss.rs (Ctrl+h toggles \
the side panel via app::toggle_synopsis).",
```

- [ ] **Step 2: Run the overlay cross-reference skill**

Invoke the `update-cairo-keybinds-overlay` skill and run its three-pass
cross-reference for the synopsis key (`h` cap → `"synopsis"` describe arm).
Confirm the describe arm now mentions `p`/`n` + `Ctrl+p`/`Ctrl+n` cycling and the
whole-work synopsis, and that no cap slot or label was left inconsistent.

- [ ] **Step 3: Build + full check**

Run: `cargo build && cargo test --bins && cargo clippy 2>&1 | rg -i "synopsis" | head`
Expected: build clean, all tests pass, no synopsis-specific clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add src/ui/keybinds_overlay.rs
git commit -m "docs(synopsis): document p/n cycling + whole-work synopsis in Ctrl+/ overlay

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

---

### Task 5: `whole_work_synopses.py` generation script

**Files:**
- Create: `~/utono/litdb/scripts/whole_work_synopses.py`

**Interfaces:**
- Consumes: `common.claude_api.submit_and_wait`, `poll_batch` (same as `chapter_synopses.py`); SQLite at `~/utono/litdb/data/lit.db`.
- Produces: a CLI that upserts `(base_abbrev, 0, 0, synopsis, 'claude-opus-4-8')` rows into `scene_synopses`. NOT run during implementation.

Model the script on `~/utono/litdb/scripts/chapter_synopses.py` (read it first).
Key differences: no DB text is read (Claude uses training knowledge), the
`custom_id` is the abbrev alone, and the row key is `(abbrev, 0, 0)`. The model
defaults to opus.

- [ ] **Step 1: Write the script**

Create `~/utono/litdb/scripts/whole_work_synopses.py`:

```python
#!/usr/bin/env python3
"""Generate a whole-work synopsis for each Shakespeare play + narrative poem.

The synopsis is written from Claude's training knowledge of the work (title +
author only — no text is sent). Stored in scene_synopses keyed
(work_abbrev, div1=0, div2=0), which the linux-lit reader surfaces as the first
synopsis-overlay position (before Act 1). Keyed by BASE abbrev so -Amb/-BBC/etc.
editions share one whole-work synopsis.

Usage:
    python scripts/whole_work_synopses.py --all [--dry-run]
    python scripts/whole_work_synopses.py --work Ham [--dry-run]
    python scripts/whole_work_synopses.py --resume BATCH_ID
"""

import argparse
import sqlite3
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
DB_PATH = SCRIPT_DIR.parent / 'data' / 'lit.db'
MODEL = 'claude-opus-4-8'

sys.path.insert(0, str(SCRIPT_DIR))
from common.claude_api import submit_and_wait, poll_batch  # noqa: E402

# Base abbrevs: 37 plays + TNK + the narrative poems. Excludes Son (sonnet
# collection) and BenCrystalOP (OP anthology). -Amb/-BBC/-KPR/-DC editions are
# NOT listed — they share the base row via base_work_abbrev in the reader.
TARGETS = [
    "1H4", "1H6", "2H4", "2H6", "3H6", "AWW", "AYL", "Ado", "Ant", "Cor",
    "Cym", "Err", "H5", "H8", "Ham", "JC", "Jn", "LLL", "Lr", "MM", "MND",
    "MV", "Mac", "Oth", "Per", "R2", "R3", "Rom", "Shr", "TGV", "TN", "TNK",
    "Tim", "Tit", "Tmp", "Tro", "WT", "Wiv",
    # narrative poems
    "Ven", "Luc", "LC", "PhT",
]

SYSTEM_PROMPT = """\
You are a literary scholar writing a whole-work synopsis for a Shakespeare
reading companion app. The synopsis is the overview a reader sees BEFORE Act 1.

Given a work's title and author, write a synopsis that:

1. Names the central characters and their relationships.
2. Lays out the through-line of the plot from beginning to end, in order.
3. Covers the major turns, the climax, and the resolution — do NOT avoid the
   ending; a reader wants the whole arc.
4. For a narrative poem with no acts, describes the poem's argument and
   movement instead of a scene plot.
5. Maintains 6-10 sentences — comprehensive but not a scene-by-scene retelling.
6. Uses third-person present tense consistently.

Do NOT:
- Add thematic essay-style interpretation beyond what the plot implies.
- Use quotation marks around character names.
- Mention act or scene numbers.

Return ONLY the synopsis text, no commentary or explanation."""


def work_title(db_path: Path, abbrev: str) -> str | None:
    conn = sqlite3.connect(db_path)
    row = conn.execute(
        "SELECT title, author FROM works WHERE abbrev = ?", (abbrev,)
    ).fetchone()
    conn.close()
    if row is None:
        return None
    return f'{row[0]} by {row[1]}'


def backup_synopses(db_path: Path):
    conn = sqlite3.connect(db_path)
    conn.execute('DROP TABLE IF EXISTS scene_synopses_backup')
    conn.execute('CREATE TABLE scene_synopses_backup AS SELECT * FROM scene_synopses')
    conn.commit()
    count = conn.execute('SELECT COUNT(*) FROM scene_synopses_backup').fetchone()[0]
    conn.close()
    print(f'Backed up {count} synopses to scene_synopses_backup')


def apply_results(db_path: Path, results: list[tuple[str, str]]) -> tuple[int, int, int]:
    """Upsert batch results into scene_synopses keyed (abbrev, 0, 0)."""
    conn = sqlite3.connect(db_path)
    inserted = updated = unchanged = 0
    for abbrev, synopsis in results:
        synopsis = synopsis.strip()
        current = conn.execute(
            "SELECT synopsis FROM scene_synopses "
            "WHERE work_abbrev = ? AND div1 = 0 AND div2 = 0",
            (abbrev,),
        ).fetchone()
        if current is None:
            conn.execute(
                "INSERT INTO scene_synopses (work_abbrev, div1, div2, synopsis, claude_model) "
                "VALUES (?, 0, 0, ?, ?)",
                (abbrev, synopsis, MODEL),
            )
            inserted += 1
        elif current[0].strip() == synopsis:
            unchanged += 1
        else:
            conn.execute(
                "UPDATE scene_synopses SET synopsis = ?, claude_model = ? "
                "WHERE work_abbrev = ? AND div1 = 0 AND div2 = 0",
                (synopsis, MODEL, abbrev),
            )
            updated += 1
    conn.commit()
    conn.close()
    print(f'Inserted {inserted}, updated {updated}, unchanged {unchanged}')
    return inserted, updated, unchanged


def build_chunks(db_path: Path, abbrevs: list[str]) -> list[tuple[str, str]]:
    """Build (custom_id=abbrev, user_msg) pairs for the batch."""
    chunks = []
    for abbrev in abbrevs:
        title = work_title(db_path, abbrev)
        if title is None:
            print(f'  skip {abbrev}: not in works table')
            continue
        user_msg = f'Work: {title}\n\nWrite the whole-work synopsis.'
        chunks.append((abbrev, user_msg))
    return chunks


def main():
    parser = argparse.ArgumentParser(
        description='Generate whole-work synopses for Shakespeare works via Batch API')
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--work', help='Single work abbreviation, e.g. Ham')
    group.add_argument('--all', action='store_true', help='All target works')
    parser.add_argument('--dry-run', action='store_true',
                        help='Print targets and sample requests without submitting')
    parser.add_argument('--resume', type=str, help='Resume polling a batch ID')
    args = parser.parse_args()

    db_path = DB_PATH

    if args.resume:
        results = poll_batch(args.resume)
        apply_results(db_path, results)
        return

    if args.work:
        abbrevs = [args.work]
    elif args.all:
        abbrevs = TARGETS
    else:
        parser.error('one of --work, --all, or --resume is required')

    chunks = build_chunks(db_path, abbrevs)
    print(f'Prepared {len(chunks)} batch requests (model {MODEL})')

    if args.dry_run:
        for cid, msg in chunks[:5]:
            print(f'\n--- {cid} ---\n{msg}')
        if len(chunks) > 5:
            print(f'\n... and {len(chunks) - 5} more')
        return

    if not chunks:
        print('Nothing to do.')
        return

    backup_synopses(db_path)
    chunks_dir = SCRIPT_DIR / '.whole-work-synopsis-batch'
    chunks_dir.mkdir(exist_ok=True)
    results = submit_and_wait(chunks, SYSTEM_PROMPT, client=None,
                              chunks_dir=chunks_dir, model=MODEL)
    apply_results(db_path, results)


if __name__ == '__main__':
    main()
```

- [ ] **Step 2: Syntax-check the script**

Run: `python3 -m py_compile ~/utono/litdb/scripts/whole_work_synopses.py`
Expected: no output (compiles clean).

- [ ] **Step 3: Verify the dry run lists targets without API calls**

Run: `python3 ~/utono/litdb/scripts/whole_work_synopses.py --all --dry-run`
Expected: prints `Prepared 42 batch requests (model claude-opus-4-8)` (38 plays incl. TNK + 4 poems = 42; minus any not found in the `works` table), then the first 5 `--- ABBREV ---` request previews and `... and N more`. NO API calls, NO DB writes, NO backup.

- [ ] **Step 4: Verify a single-work dry run**

Run: `python3 ~/utono/litdb/scripts/whole_work_synopses.py --work Ham --dry-run`
Expected: `Prepared 1 batch requests`, then the `--- Ham ---` preview showing `Work: Hamlet by Shakespeare`.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/litdb && git add scripts/whole_work_synopses.py
git commit -m "feat(synopses): whole-work synopsis batch generator

Generates a whole-work synopsis per Shakespeare play + narrative poem from
Claude's training knowledge (title+author only), stored in scene_synopses as
(abbrev, 0, 0) — the reader's pre-Act-1 synopsis position. Keyed by base abbrev
so -Amb/etc. editions share. --dry-run / --resume supported; not run here.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014UYTmcaAHC2SDypMpKJvNs"
```

(Note: this script lives in the `~/utono/litdb` repo, a SEPARATE git repo from
linux-lit. Commit it there.)

- [ ] **Step 6: Hand off generation + visual verification to the user**

The script is NOT run during implementation (it spends API tokens). Tell the user
the exact commands to generate and then verify:

```bash
# dry run (no cost):
python3 ~/utono/litdb/scripts/whole_work_synopses.py --all --dry-run
# real generation (spends opus Batch API tokens):
python3 ~/utono/litdb/scripts/whole_work_synopses.py --all
```

Then, in linux-lit (user-run via cage; an agent can't drive the live dwl seat):
- Open a play, press `h` for the synopsis overlay.
- Press `p` (or `Ctrl+p`) from Act 1 Scene 1 → the "Whole work" synopsis shows first.
- `n` (or `Ctrl+n`) from it → Act 1 Scene 1.
- On the whole-work synopsis, press `A` (amend) or `E` (edit) → confirm it revises and persists the `(0,0)` row, exactly like a scene synopsis.

---

## Self-Review

**Spec coverage:**
- A1 (`ordered_synopsis_scenes` prepends `(0,0)`) → Task 1. ✓
- A2 (`synopsis_label(0,0)` → "Whole work") → Task 2. ✓
- A3 (`Ctrl+p`/`Ctrl+n` aliases) → Task 3. ✓
- A4 (editing works for free) → no code; verified in Task 5 Step 6 hand-off + covered by the existing upsert path. ✓ (No task needed — the spec states it's a verification requirement, and the user-run check exercises it.)
- A5 (keybinds overlay) → Task 4. ✓
- B (generation script: targets, training-knowledge method, `(0,0)` upsert, opus, backup, dry-run, resume) → Task 5. ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; commands have expected output. ✓

**Type consistency:**
- `prepend_whole_work(bool, Vec<(i64,i64)>) -> Vec<(i64,i64)>` defined Task 1, used in `ordered_synopsis_scenes` same task. ✓
- `whole_work_label(i64,i64) -> Option<&'static str>` defined Task 2, used in `synopsis_label` same task. ✓
- `cycle_synopsis(state, delta)` (existing) used in Task 3 — signature matches the current `n`/`p` arms. ✓
- Script: `apply_results` upserts `(abbrev,0,0,...)`; `build_chunks` custom_id == abbrev, consumed by `apply_results` as the abbrev — consistent. `submit_and_wait(..., model=MODEL)` matches the helper's `model` kwarg (`claude_api.py:152`). ✓

**Note on the A4 "no code" decision:** the spec is explicit that editing the
whole-work synopsis requires no new code (the amend/edit/undo path keys on
`synopsis_overlay_scene`, and `save_synopsis` upserts on `(work_abbrev,0,0)`).
Confirmed against `queries.rs:421` (upsert) and `synopsis.rs` (keys on
`synopsis_overlay_scene`). No task is needed; the user-run check in Task 5 Step 6
verifies it end to end.

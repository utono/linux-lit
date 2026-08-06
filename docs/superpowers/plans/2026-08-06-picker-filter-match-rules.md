# Picker Filter Match Rules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the journal Q&A picker from returning four unrelated rows when
searching `simile`, by matching short label fields fuzzily and long text
literally.

**Architecture:** Add one pure predicate `row_matches` to `src/ui/picker_filter.rs`
that applies fuzzy `subsequence_match` to a short target and contiguous
`contains` to a long target and a body haystack. Rewrite the two pickers that
currently fuzzy-match long prose (`journal_picker.rs`, `recent_qa_picker.rs`) to
call it. Every other picker is untouched.

**Tech Stack:** Rust, GTK4 (gtk4-rs). Tests are plain `#[cfg(test)]` unit tests —
the predicate is pure, so no GTK, no cage, no headless harness.

**Spec:** `docs/superpowers/specs/2026-08-06-picker-filter-match-rules-design.md`

## Global Constraints

- Match rule follows field LENGTH, not field identity: short labels (author,
  work, division, type) stay fuzzy; long text (80-char row label, full Q&A body)
  requires a contiguous substring.
- `row_matches` has a **non-empty-filter precondition**. Both call sites already
  sit inside `if !filter.is_empty()`. Do NOT remove those guards and do NOT
  re-check emptiness inside `row_matches` — `"".contains("")` is true, so an
  empty filter would otherwise match every target.
- All arguments to `row_matches` are **already lowercased by the caller**.
  `subsequence_match` is case-sensitive by contract; keep it that way.
- Do NOT modify `bookmark_picker.rs`, `gloss_picker.rs`, `media_picker.rs`,
  `journal_move_picker.rs`, `library_picker.rs`, or `journal_term_input.rs`.
  They fuzzy-match genuinely short labels where fuzzy is the intended feature.
- Verify with `cargo build` and `cargo test --bins`. Do NOT run the app —
  the user launches it.

---

### Task 1: Add the `row_matches` predicate

**Files:**
- Modify: `src/ui/picker_filter.rs` (add function after `subsequence_match`,
  which ends at line 90; add tests inside the existing `mod tests` block that
  starts at line 92)

**Interfaces:**
- Consumes: `subsequence_match(filter: &str, target: &str) -> bool` — already
  exists at `src/ui/picker_filter.rs:88`.
- Produces: `pub(crate) fn row_matches(filter: &str, short_target: &str,
  long_target: &str, haystack: &str) -> bool` — used by Tasks 2 and 3.

- [ ] **Step 1: Write the failing tests**

Add these five tests inside the existing `mod tests` block in
`src/ui/picker_filter.rs`, immediately after the
`subsequence_match_preserves_boolean` test (which ends at line 133):

```rust
    /// The four rows that "simile" wrongly matched in the journal Q&A picker.
    /// Each is a real `scope='passage'` label from BH-Barrett. None contains
    /// the substring "simile"; all four matched fuzzily because the letters
    /// s-i-m-i-l-e occur scattered and in order (the leading `s` came from the
    /// word "passage" in the type column, which no longer shares a target with
    /// the prose).
    const SIMILE_FALSE_POSITIVES: [&str; 4] = [
        "i was brought up, from my earliest remembrance—like some of the princesses in th",
        "i opened it softly and found miss jellyby shivering there with a broken candle i",
        "among the ladies who were most distinguished for this rapacious benevolence (if",
        "“these, young ladies,” said mrs. pardiggle with great volubility after the first",
    ];

    #[test]
    fn long_target_rejects_scattered_subsequence() {
        for label in SIMILE_FALSE_POSITIVES {
            assert!(
                !row_matches("simile", "", label, ""),
                "long target must not fuzzy-match: {label}"
            );
        }
    }

    #[test]
    fn body_hit_survives_with_unrelated_label() {
        // Division 9.0: the ONE true hit. Its visible label is about breakfast,
        // not similes — the term appears only in the answer body. Body search
        // must still reach it.
        let label = "we were going on in this way, when one morning at breakfast mr. jarndyce receive";
        let body = "there's no simile for his lungs, said mr. jarndyce.";
        assert!(row_matches("simile", "", label, body));
    }

    #[test]
    fn long_target_accepts_contiguous_substring() {
        let label = "i opened it softly and found miss jellyby shivering there with a broken candle i";
        assert!(row_matches("jellyby", "", label, ""));
        // Typo tolerance is deliberately gone on the long target.
        assert!(!row_matches("jelby", "", label, ""));
    }

    #[test]
    fn short_target_stays_fuzzy() {
        // Division/type queries still narrow on the short fields.
        assert!(row_matches("3.0", "3.0 passage", "", ""));
        assert!(row_matches("passage", "3.0 passage", "", ""));
        // Abbreviation-style fuzzy matching survives where labels are short.
        assert!(row_matches("psg", "3.0 passage", "", ""));
    }

    #[test]
    fn absent_long_target_loses_nothing() {
        // A caller with no haystack (RecentQaPicker) passes "" and still
        // matches on its short target.
        assert!(row_matches("bh", "bh", "some unrelated label", ""));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test --bins picker_filter 2>&1 | tail -20
```

Expected: FAIL to compile, with `cannot find function 'row_matches' in this scope`
(five errors, one per test).

- [ ] **Step 3: Write the implementation**

Add this function to `src/ui/picker_filter.rs` immediately after
`subsequence_match` (after line 90, before the `#[cfg(test)]` block):

```rust
/// True when `filter` matches a picker row, with the match rule scaled to each
/// field's LENGTH rather than its identity.
///
/// - `short_target` — the short label fields joined (author, work, division,
///   type). Fuzzy subsequence matching, so `ch. 2` and `dickens` still narrow.
/// - `long_target` — the ~80-char row label. Contiguous substring only.
/// - `haystack` — the entry's full question + answer. Contiguous substring only.
///
/// A subsequence over long prose degenerates: six common letters match almost
/// any passage. Searching `simile` returned four unrelated rows because
/// s-i-m-i-l-e occurs scattered and in order in ordinary Dickens prose, so the
/// filter stopped filtering. Only the short fields may be fuzzy.
///
/// PRECONDITION: `filter` is non-empty (callers guard with
/// `if !filter.is_empty()`), because `"".contains("")` is true and would
/// otherwise accept every row including absent targets. Pass `""` for a target
/// the caller does not have. All arguments must already be lowercased —
/// `subsequence_match` is case-sensitive by contract.
pub(crate) fn row_matches(
    filter: &str,
    short_target: &str,
    long_target: &str,
    haystack: &str,
) -> bool {
    subsequence_match(filter, short_target)
        || long_target.contains(filter)
        || haystack.contains(filter)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
cargo test --bins picker_filter 2>&1 | tail -20
```

Expected: PASS — `test result: ok.` with 11 tests (the 6 pre-existing plus the 5 new).

- [ ] **Step 5: Commit**

```bash
git add src/ui/picker_filter.rs
git commit -m "feat(picker): row_matches — fuzzy short labels, literal long text

Searching 'simile' in the journal Q&A picker returned five rows; only one
had anything to do with similes. Six common letters occur scattered and in
order in ordinary prose, so a subsequence over an 80-char passage label
matches almost anything and the filter stops filtering.

Add the shared predicate that scales the match rule to field LENGTH: fuzzy
subsequence for the short label fields, contiguous substring for long text
and the Q&A body. Tests pin the four real false-positive labels as reject
cases, plus the one true hit that is reachable only via the body.

Callers wire up next."
```

---

### Task 2: Wire up `journal_picker`

**Files:**
- Modify: `src/ui/journal_picker.rs:162-194` (inside `populate_list`)

**Interfaces:**
- Consumes: `crate::ui::picker_filter::row_matches(filter, short_target,
  long_target, haystack) -> bool` from Task 1.
- Produces: nothing consumed by later tasks.

Field reference for `JournalRow` (defined at the top of the same file):
`author_label: Option<String>`, `work_label: Option<String>`,
`synopsis_division_label: String`, `div_label: String`, `type_label: String`,
`question_prefix: String`, `search_haystack: String` (already lowercased at
build time).

- [ ] **Step 1: Replace the match block**

In `src/ui/journal_picker.rs`, replace the whole `if !filter.is_empty() { … }`
block (lines 162-194 — it starts with `if !filter.is_empty() {` and ends with
the `}` after `continue;`) with this:

```rust
            if !filter.is_empty() {
                // Match rule scales to field LENGTH (see picker_filter::row_matches).
                //
                // SHORT fields stay fuzzy so a surname ("dickens") or a division
                // ("ch. 2") still narrows — the natural gesture on a global
                // cross-work list.
                //
                // The 80-char row label and the multi-thousand-character body
                // are CONTIGUOUS-substring only. A scattered subsequence over
                // prose that long matches almost any short filter, so every row
                // survives and the filter stops filtering.
                //
                // `type_label` belongs with the SHORT fields, deliberately away
                // from the passage prose: while it shared one concatenated
                // target with the label, the "s" in "passage" supplied the
                // leading letter for every "simile" false positive.
                let short_target = format!(
                    "{} {} {} {}",
                    item.author_label.as_deref().unwrap_or(""),
                    item.synopsis_division_label,
                    item.div_label,
                    item.type_label,
                )
                .to_lowercase();
                // `primary` already embeds `work_label`, so the work title still
                // matches literally here (fuzzy work-finding lives in the
                // library picker).
                let long_target = primary.to_lowercase();
                let hit = crate::ui::picker_filter::row_matches(
                    &filter_lower,
                    &short_target,
                    &long_target,
                    &item.search_haystack,
                );
                if !hit {
                    continue;
                }
            }
```

- [ ] **Step 2: Verify it compiles**

Run:

```bash
cargo build 2>&1 | rg "^error" -A 5 | head -20; echo "exit=$?"
```

Expected: no `error` lines (exit=1 from rg finding nothing is correct).

- [ ] **Step 3: Confirm the old fuzzy target is gone**

Run:

```bash
rg -n "display_target" src/ui/journal_picker.rs
```

Expected: NO output. The old concatenated target must be fully replaced —
a leftover would silently keep the bug alive.

- [ ] **Step 4: Run the full unit suite**

Run:

```bash
cargo test --bins 2>&1 | tail -5
```

Expected: `test result: ok.` with 0 failed.

- [ ] **Step 5: Commit**

```bash
git add src/ui/journal_picker.rs
git commit -m "fix(journal): picker filter stops fuzzy-matching passage prose

Split the single concatenated display target in two: the short label fields
(author, division, type) keep fuzzy matching; the 80-char row label joins the
body as contiguous-substring only.

Moving type_label away from the passage prose is the load-bearing part — the
's' in 'passage' supplied the leading letter for all four 'simile' false
positives."
```

---

### Task 3: Wire up `recent_qa_picker`

**Files:**
- Modify: `src/ui/recent_qa_picker.rs:106-112` (inside `populate_list`)

**Interfaces:**
- Consumes: `crate::ui::picker_filter::row_matches` from Task 1.
- Produces: nothing consumed by later tasks.

Note: `RecentQaRow` (defined at `src/ui/recent_qa_picker.rs:7-12`) has only
`id`, `work_abbrev`, `work_label`, `question_prefix` — there is **no**
`search_haystack` field. Pass `""` for the haystack; do NOT add a haystack
field to this struct (out of scope).

- [ ] **Step 1: Replace the match block**

In `src/ui/recent_qa_picker.rs`, replace lines 106-112 — the block that starts
`if !filter.is_empty() {` and ends with the `}` after `continue;` — with:

```rust
            if !filter.is_empty() {
                // Same length-scaled rule as the journal Q&A picker: the short
                // work label stays fuzzy, the 80-char question label is
                // contiguous-substring only. This picker carries no body
                // haystack, so it passes "".
                let short_target = item.work_label.to_lowercase();
                let long_target = item.question_prefix.to_lowercase();
                if !crate::ui::picker_filter::row_matches(
                    &filter_lower,
                    &short_target,
                    &long_target,
                    "",
                ) {
                    continue;
                }
            }
```

- [ ] **Step 2: Verify it compiles**

Run:

```bash
cargo build 2>&1 | rg "^error" -A 5 | head -20; echo "exit=$?"
```

Expected: no `error` lines.

- [ ] **Step 3: Confirm no picker still fuzzy-matches a question prefix**

Run:

```bash
rg -n "question_prefix" src/ui/recent_qa_picker.rs src/ui/journal_picker.rs
```

Expected: `question_prefix` appears only in struct definitions, row-label
construction, and the `long_target` bindings — never as an argument to
`subsequence_match`.

- [ ] **Step 4: Run build, tests, and clippy**

Run:

```bash
cargo test --bins 2>&1 | tail -5
cargo clippy 2>&1 | rg -c "^warning|^error"
```

Expected: tests `ok.` with 0 failed. Clippy count must be **186** — the
pre-change baseline on master. A higher number means this change added a
warning; investigate before committing.

- [ ] **Step 5: Commit**

```bash
git add src/ui/recent_qa_picker.rs
git commit -m "fix(journal): recent-Q&A picker adopts the length-scaled filter

Same defect as the journal Q&A picker: work_label and question_prefix were
concatenated into one fuzzy target, so a short common-letter query scavenged
letters across the question text. Work label stays fuzzy; the question label
is contiguous-substring only. No body haystack on this picker, so it passes
an empty one."
```

---

### Task 4: Verify on screen and record the outcome

The predicate is pure and unit-tested, but the acceptance criterion is what the
real picker shows. Cage is software rendering and this is a text-filter change,
so a headless capture is sufficient evidence here.

**Files:**
- Modify: `docs/superpowers/specs/2026-08-06-picker-filter-match-rules-design.md`
  (append the verification outcome)

- [ ] **Step 1: Launch headless, landed in the reader**

Run from the repo root (foreground-alive; the harness owns the lifecycle):

```bash
./scripts/land-on.sh BH-Barrett 9.0
```

Wait for the `[land-on] XDG_RUNTIME_DIR=… WAYLAND_DISPLAY=…` line and export
both variables. If it reports an error instead, re-run once before treating it
as a real failure.

- [ ] **Step 2: Open the journal Q&A picker and search**

`Ctrl+j` opens the picker. The FIRST chord after launch is dropped (focus is
still settling) — send it, confirm a `KEY:` line appeared in `/tmp/land-on.log`,
and re-send if it did not.

```bash
wtype -M ctrl -k j -m ctrl; sleep 2
wtype "simile"; sleep 1
grim /tmp/simile-after.png; stat -c%s /tmp/simile-after.png
```

- [ ] **Step 3: Read the screenshot and count the rows**

Open `/tmp/simile-after.png` with the Read tool and count the result rows.

Expected: exactly ONE row — division `9.0`, labelled "We were going on in this
way, when one morning at breakfast Mr. Jarndyce receive". The four rows at
3.0, 4.0, 8.0, and 8.0 must be gone.

If more than one row appears, STOP and diagnose — do not proceed to Step 5.

- [ ] **Step 4: Clean up the test instance**

```bash
pkill -f "cage -- ./target/debug/linux-lit"
```

Use exactly this scoped pattern. A bare `pkill -f target/debug/linux-lit`
would kill the user's own running instance.

- [ ] **Step 5: Record the outcome in the spec**

Replace the `## Acceptance` section of
`docs/superpowers/specs/2026-08-06-picker-filter-match-rules-design.md` with
the confirmed result, substituting the real observed row count:

```markdown
## Acceptance

Searching `simile` in the journal Q&A picker returns exactly one row —
division 9.0, the Jarndyce "no simile for his lungs" entry.

**Verified 2026-08-06** headlessly (`land-on.sh BH-Barrett 9.0`, Ctrl+j,
`simile`): one row returned, down from five. The four false positives at
3.0, 4.0, 8.0 and 8.0 are gone; the true hit survives via the body haystack
despite a visible label about breakfast.
```

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-08-06-picker-filter-match-rules-design.md
git commit -m "docs(spec): record picker filter verification outcome"
```

---

## Finishing the branch

Per the project convention, merge back to master locally and push — no PR:

```bash
git checkout master
git merge --no-ff <branch> -m "Merge branch '<branch>'"
cargo build 2>&1 | rg "^error|Finished" | tail -3
git push origin master
git branch -d <branch>
```

If the work was done in a worktree, `git worktree remove` it before deleting
the branch.

# BCP Sentence-Per-Line Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render BCP works from their TEI `.txt` so each sentence is one short
physical line (paginates with no pager change) while every sentence line still
maps to its containing lit.db paragraph row, so timestamps / gutter signs / sync
keep working.

**Architecture:** Two coordinated changes. (1) The generator
`ws-book-of-common-prayer-references/scripts/tei_to_text.py` emits each lit.db
content block (`<p>`, `<sp>`) split **one sentence per physical line** (heads,
rubrics, `<l>` stay single-line). (2) The reader's `build_line_map`
(`linux-lit/src/text_file_map.rs`) gains a **paragraph-accumulation** matcher:
for a sentence-split prose work it accumulates consecutive buffer lines until
their concatenated normalized text equals a DB row's `normalized_text`, then maps
that whole run of buffer lines to that one row (first line canonical for
timestamp / chapter / section). This mirrors the proven `build_line_map_bcp`
`source_index` identity mapping but derives the grouping by text instead of by
construction, so the `.txt` (not the DB) drives rendering.

**Tech Stack:** Python 3 + `textwrap`/`xml.etree` (generator); Rust + GTK4 +
`sourceview5` + `unicode_normalization` (reader); SQLite (`lit.db`).

**Why this and not sub-line pixel paging:** linux-lit's pager is whole-buffer-line
granular (`visible_range`/`column_split` sum whole `line_yrange` heights and
cannot start a column mid-line). A BCP paragraph is one DB row up to 4119 chars;
as a single soft-wrapped buffer line it is taller than a column, so
`next_page_top` cannot advance and `x`/`G` stall. Splitting paragraphs into
sentences yields short lines the existing pager handles unchanged. Verified
offline: splitting all 677 BCP1662 paragraph rows into sentences and
concatenating the normalized sentences reproduces the normalized row for
**677/677 rows** (1146 sentence lines total).

---

## Background facts the implementer needs

- **DB granularity.** `lit.db` stores BCP at **paragraph** granularity: one
  `line_mapping` row per paragraph (`canonical_text` is the whole unwrapped
  paragraph; `normalized_text` is its normalized form). BCP1662 has 677 rows,
  677 timestamps for `media_id=274`, 13 chapter rows.
- **Reader load path (already correct).** A BCP work whose `works.text_file` is
  set renders through the generic prose `text_file` path: `rebuild_buffer_text`
  (`src/app.rs`) and the off-thread startup path both call
  `prepare_text_for_display` / `prepare_text_only`, which key on
  `work.text_file` and call `build_line_map`. The BCP DB sentence-split branch
  (`src/app.rs`, `is_bcp_work` + `split_bcp_sentences` + `build_line_map_bcp`)
  only runs when `text_file` is NULL — leave it as the fallback for
  BCP1549/1559/1559M.
- **The matcher today.** `build_line_map(file_lines, work_lines, is_prose)`
  (`src/text_file_map.rs:233`) matches a buffer line to a DB row by **whole-line
  normalized equality**. That is why a sentence line (a fragment of a paragraph
  row) currently maps to nothing. The new code path adds accumulation.
- **`normalize()`** (`src/text_file_map.rs:47`) lowercases, strips
  `[...]` brackets and diacritics, keeps alphanumerics, collapses whitespace.
  Leading-space indent / centering / the speaker `.` punctuation all normalize
  away, so chrome whitespace does not affect matching.
- **`split_bcp_sentences`** (`src/db/line_types.rs:195`) is the reader's existing
  sentence splitter: breaks on `. ` before an uppercase letter; does NOT break
  on abbreviations (`&c.`, single-letter initials, roman numerals,
  footnote-markers); a trailing `Amen.` attaches to the preceding sentence. The
  Python generator must mirror these rules so the `.txt` sentence boundaries
  line up with what the reader expects.
- **Gutter / chapters / sections.** `build_line_map` derives `chapter_breaks`
  from `Line.is_chapter` (mapped through `work_to_buffer`) and `section_starts`
  from `(div1,div2)` via `build_section_starts`. As long as
  `work_to_buffer[wi]` points at the FIRST buffer line of a row's run, the
  gutter sign and chapter nav land on that first sentence line — correct.

## File structure

- **Modify** `~/utono/ws-book-of-common-prayer-references/scripts/tei_to_text.py`
  — add a Python sentence splitter mirroring `split_bcp_sentences`; emit `<p>`
  and `<sp>` bodies one sentence per physical line.
- **Modify** `~/utono/ws-book-of-common-prayer-references/tests/test_tei_to_text.py`
  — tests for the splitter + sentence-per-line emission.
- **Regenerate (artifact, not hand-edited)**
  `~/utono/literature/BCP/1662/TEI/bcp-1662-spoken.txt` via the generator.
- **Modify** `~/utono/literature/BCP/1662/TEI/README.md` — describe the
  sentence-per-line model.
- **Modify** `~/utono/linux-lit/src/text_file_map.rs` — add the
  paragraph-accumulation matcher behind a new `ParagraphAccumulate` matching
  mode; add unit tests.
- **Modify** `~/utono/linux-lit/src/app.rs` — pass the new mode for BCP works
  with a `text_file` into `prepare_text_for_display` / `prepare_text_only` /
  `build_line_map_for_prepared` so the off-thread and synchronous paths agree.
- **Modify** `~/utono/linux-lit/src/snapshot.rs` — bump `SNAPSHOT_VERSION`
  (the serialized `LineMap` shape is unchanged, but the cached `buffer_to_work`
  values change because the `.txt` and matcher change, so stale snapshots must
  be invalidated).

---

## Task 1: Python sentence splitter mirroring `split_bcp_sentences`

**Files:**
- Modify: `~/utono/ws-book-of-common-prayer-references/scripts/tei_to_text.py`
- Test: `~/utono/ws-book-of-common-prayer-references/tests/test_tei_to_text.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_tei_to_text.py` (import `split_sentences` at the top with the
other imports):

```python
from scripts.tei_to_text import split_sentences


def test_split_sentences_basic():
    assert split_sentences("Lord have mercy. Christ have mercy.") == [
        "Lord have mercy.", "Christ have mercy."]


def test_split_sentences_no_break_on_abbreviation():
    # "&c." and single-letter initials are not sentence ends.
    assert split_sentences("Our Father. &c. And lead us.") == [
        "Our Father. &c. And lead us."]


def test_split_sentences_amen_attaches():
    assert split_sentences("Deliver us from evil. Amen. Now this.") == [
        "Deliver us from evil. Amen.", "Now this."]


def test_split_sentences_no_break_point():
    assert split_sentences("Lord have mercy upon us.") == [
        "Lord have mercy upon us."]


def test_split_sentences_no_break_lowercase_after():
    # period followed by a lowercase word is mid-sentence (e.g. an ellipsis-ish
    # or a number list), do not split.
    assert split_sentences("seen at the font. and again later.") == [
        "seen at the font. and again later."]


def test_split_sentences_two_char_initial_no_break():
    # Faithful-to-Rust: a token with exactly one alpha char and length <= 2 is an
    # initial, so "x'." does NOT end a sentence (regression guard for the
    # narrower len==1 port).
    assert split_sentences("named x'. Then prayed.") == [
        "named x'. Then prayed."]
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/ws-book-of-common-prayer-references && python -m pytest tests/test_tei_to_text.py -k split_sentences -q`
Expected: FAIL with `ImportError: cannot import name 'split_sentences'`.

- [ ] **Step 3: Implement `split_sentences`**

Add to `scripts/tei_to_text.py` (after the `render_inline` helper, before
`wrap_block`). This is a direct port of `split_bcp_sentences`
(`linux-lit/src/db/line_types.rs:195`) and its helper `is_sentence_break_at`:

```python
def _is_sentence_break_at(chars, dot):
    """True if the '.' at index `dot` ends a sentence: followed by whitespace
    then an uppercase letter, and not an abbreviation/initial/numeral/footnote.
    Port of is_sentence_break_at in linux-lit/src/db/line_types.rs."""
    n = len(chars)
    j = dot + 1
    if j >= n or not chars[j].isspace():
        return False
    while j < n and chars[j].isspace():
        j += 1
    if j >= n or not chars[j].isupper():
        return False
    if dot > 0 and chars[dot - 1] == "*":   # footnote marker
        return False
    # token immediately before the period
    k = dot
    while k > 0 and not chars[k - 1].isspace():
        k -= 1
    token = "".join(chars[k:dot])
    low = token.lower()
    if low == "&c" or low.endswith("&c"):
        return False
    if any(c.isdigit() for c in token):
        return False
    if k > 0 and chars[k] == ".":           # ".roman." numeral group
        return False
    # Single-letter token + period: an initial/abbreviation (e.g. "S." "x.").
    # Faithful to Rust: EXACTLY one alphabetic char AND total token length <= 2
    # (so a 2-char form like "x'" also suppresses the break).
    if sum(1 for c in token if c.isalpha()) == 1 and len(token) <= 2:
        return False
    return True


def _next_word_after(chars, i):
    """The next whitespace-delimited word at/after index i, stripped of trailing
    .,!? — used to detect a following 'Amen.' sentence. Port of next_word_after
    (Rust strips trailing '.' ',' '!' '?')."""
    n = len(chars)
    while i < n and chars[i].isspace():
        i += 1
    start = i
    while i < n and not chars[i].isspace():
        i += 1
    return "".join(chars[start:i]).rstrip(".,!?")


def split_sentences(line):
    """Split a paragraph into sentences, one per returned string. A trailing
    'Amen.' attaches to the sentence it follows. Mirror of split_bcp_sentences
    in linux-lit so the .txt sentence boundaries match the reader's DB-path
    expectations."""
    chars = list(line)
    n = len(chars)
    sentences = []
    start = 0
    i = 0
    while i < n:
        if chars[i] == "." and _is_sentence_break_at(chars, i):
            after = _next_word_after(chars, i + 1)  # already trailing-stripped
            if after.lower() == "amen":
                i += 1
                continue
            sentences.append("".join(chars[start:i + 1]).strip())
            j = i + 1
            while j < n and chars[j].isspace():
                j += 1
            start = j
            i = j
            continue
        i += 1
    if start < n:
        rest = "".join(chars[start:]).strip()
        if rest:
            sentences.append(rest)
    if not sentences:
        sentences.append(line.strip())
    return sentences
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd ~/utono/ws-book-of-common-prayer-references && python -m pytest tests/test_tei_to_text.py -k split_sentences -q`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/ws-book-of-common-prayer-references
git add scripts/tei_to_text.py tests/test_tei_to_text.py
git commit -m "feat(tei): port split_bcp_sentences to the .txt renderer

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Emit `<p>` and `<sp>` one sentence per physical line

**Files:**
- Modify: `~/utono/ws-book-of-common-prayer-references/scripts/tei_to_text.py:render_rite`
- Test: `~/utono/ws-book-of-common-prayer-references/tests/test_tei_to_text.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_tei_to_text.py`:

```python
def test_convert_paragraph_one_sentence_per_line(tmp_path):
    body = "Lord have mercy upon us. Christ have mercy upon us. Lord have mercy upon us."
    tei = f'''<?xml version="1.0"?>
<TEI xmlns="{NS}"><text><body>
<div type="rite" n="t"><head>H</head><p n="1">{body}</p></div>
</body></text></TEI>'''
    f = tmp_path / "p.xml"; f.write_text(tei)
    out = convert(f).splitlines()
    assert "Lord have mercy upon us." in out
    assert "Christ have mercy upon us." in out
    # three sentences -> three separate physical lines, none containing two.
    body_lines = [l for l in out if "mercy" in l]
    assert len(body_lines) == 3, body_lines


def test_convert_speaker_sentence_split(tmp_path):
    tei = f'''<?xml version="1.0"?>
<TEI xmlns="{NS}"><text><body>
<div type="rite" n="t"><head>H</head>
<sp who="#p" n="1"><speaker>Priest</speaker><p>O Lord, save us. Help us now.</p></sp>
</div></body></text></TEI>'''
    f = tmp_path / "sp.xml"; f.write_text(tei)
    out = convert(f).splitlines()
    # speaker label stays on the FIRST sentence line only.
    assert "Priest.  O Lord, save us." in out
    assert "Help us now." in out
    assert not any("Priest" in l and "Help us now" in l for l in out)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd ~/utono/ws-book-of-common-prayer-references && python -m pytest tests/test_tei_to_text.py -k "one_sentence_per_line or speaker_sentence_split" -q`
Expected: FAIL — current code emits the whole paragraph as one line, so
`len(body_lines) == 1`.

- [ ] **Step 3: Implement sentence-per-line emission**

In `scripts/tei_to_text.py`, replace the `<sp>` and `<p>` arms of `render_rite`
(the versions added in the prior turn that used `oneline_block`) with sentence
splitting. Add a helper just above `render_rite`:

```python
def sentence_lines(text, indent=0, first_prefix=""):
    """Emit `text` as one physical line per sentence. `first_prefix` (e.g. a
    speaker label) is prepended to the FIRST sentence only; `indent` left-pads
    every line. Each line is whole (never hard-wrapped) so the reader can
    accumulate sentences back into the DB paragraph row."""
    out = []
    sents = split_sentences(text)
    for idx, s in enumerate(sents):
        prefix = first_prefix if idx == 0 else ""
        out.append((" " * indent) + prefix + s)
    return out
```

Then change the arms:

```python
        elif tag == "sp":
            speaker = ""
            body = ""
            for sub in child:
                if _local(sub.tag) == "speaker":
                    speaker = render_inline(sub)
                elif _local(sub.tag) == "p":
                    body = render_inline(sub)
            # Speaker label on the first sentence line; remaining sentences are
            # their own lines. The lit.db row stores the speaker inline in
            # canonical_text, so the reader's accumulation rejoins them.
            blocks.append(sentence_lines(body, first_prefix=f"{speaker}.  "))
        elif tag == "p":
            blocks.append(sentence_lines(render_inline(child)))
```

Leave the `<head>`, `<rubric>`, `<lg>`/`<l>`, standalone `<l>`, `<note>`, and
unhandled-element arms exactly as they are (heads centered single line, rubrics
indented single line, verse `<l>` single line via `oneline_block`). `<l>` lines
are already short and are one DB row each, so they need no sentence split.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd ~/utono/ws-book-of-common-prayer-references && python -m pytest tests/test_tei_to_text.py -q`
Expected: PASS (all tests, including the Task 1 splitter tests and the existing
suite). The earlier one-physical-line `<p>` tests
(`test_convert_does_not_hard_wrap_long_paragraph`) still pass because each
SENTENCE is still one physical line (just possibly several sentences per
paragraph); if a test asserted a multi-sentence paragraph is exactly one line,
update it to assert per-sentence lines instead.

- [ ] **Step 5: Commit**

```bash
cd ~/utono/ws-book-of-common-prayer-references
git add scripts/tei_to_text.py tests/test_tei_to_text.py
git commit -m "feat(tei): emit BCP body/speaker one sentence per line

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Regenerate the BCP1662 .txt and update its README

**Files:**
- Regenerate: `~/utono/literature/BCP/1662/TEI/bcp-1662-spoken.txt`
- Modify: `~/utono/literature/BCP/1662/TEI/README.md`

- [ ] **Step 1: Back up and regenerate**

```bash
cd ~/utono/ws-book-of-common-prayer-references
TXT=/home/mlj/utono/literature/BCP/1662/TEI/bcp-1662-spoken.txt
\cp -f "$TXT" /tmp/bcp-1662-spoken.txt.bak
python scripts/tei_to_text.py /home/mlj/utono/literature/BCP/1662/TEI --out "$TXT"
wc -l "$TXT"
```

Expected: exit 0, no `warning:` lines, line count larger than the
paragraph-per-line version (sentences split paragraphs) but each line short.

- [ ] **Step 2: Verify offline that sentence lines accumulate to DB rows**

Run this check (it mirrors the Task 4 reader matcher); it must report 100%
paragraph reconstruction:

```bash
cd ~/utono/linux-lit && python3 - <<'PY'
import sqlite3, unicodedata
def norm(s):
    out=[]; depth=0; last=True
    for ch in unicodedata.normalize('NFD', s):
        if unicodedata.combining(ch): continue
        if ch=='[': depth+=1; continue
        if ch==']' and depth>0: depth-=1; continue
        if depth>0: continue
        if ch.isalnum(): out.append(ch.lower()); last=False
        elif ch.isspace():
            if not last: out.append(' '); last=True
    return ''.join(out).strip()
db=sqlite3.connect('/home/mlj/utono/litdb/data/lit.db')
rows=[norm(r[0]) for r in db.execute(
    "SELECT canonical_text FROM line_mapping WHERE work_abbrev='BCP1662' "
    "ORDER BY div1,div2,line_in_div").fetchall()]
rows=[r for r in rows if r]
txt=[norm(l) for l in open('/home/mlj/utono/literature/BCP/1662/TEI/bcp-1662-spoken.txt')
     .read().splitlines()]
txt=[l for l in txt if l]
# greedy accumulation: walk txt lines, accumulate until concat == next row
ri=0; acc=''; matched=0
for l in txt:
    if ri>=len(rows): break
    acc=(acc+' '+l).strip() if acc else l
    if acc==rows[ri]:
        matched+=1; ri+=1; acc=''
    elif not rows[ri].startswith(acc):
        acc=''   # resync: this line is chrome (rubric/head), skip it
print(f"DB rows reconstructed by greedy accumulation: {matched}/{len(rows)} "
      f"({100*matched/len(rows):.1f}%)")
PY
```

Expected: **≥ 97%** of rows reconstructed (the residue is split-title heads and
a couple of DB encoding quirks — acceptable; they still get section/chapter
treatment).

- [ ] **Step 3: Update the README**

In `~/utono/literature/BCP/1662/TEI/README.md`, replace the description of the
`.txt` line model (the paragraph-per-line wording added in the prior turn) with:

```markdown
- `bcp-1662-spoken.txt` — a **downstream rendering**: a standalone plain-text
  version generated from all 13 TEI files concatenated in rite order. Each
  prayer/response is split **one sentence per physical line** (a trailing
  `Amen.` stays attached); heads are centered single lines and rubrics indented
  single lines. linux-lit accumulates consecutive sentence lines back into the
  one lit.db paragraph row they came from (text-match) to attach the audiobook
  timestamps, and paginates the short lines without the giant-paragraph overflow
  that one-line-per-paragraph caused. `--width` only bounds head/separator
  centering and editorial-note wrapping.
```

- [ ] **Step 4: Commit**

```bash
cd ~/utono/literature
git add BCP/1662/TEI/bcp-1662-spoken.txt BCP/1662/TEI/README.md
git commit -m "data(bcp): regenerate 1662 .txt one sentence per line

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Add the paragraph-accumulation matcher to `build_line_map`

**Files:**
- Modify: `~/utono/linux-lit/src/text_file_map.rs`
- Test: `~/utono/linux-lit/src/text_file_map.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/text_file_map.rs`. Use the
existing test helper pattern (look at `test_build_line_map_bcp_*` for how `Line`
values are constructed; reuse that constructor or build `Line` literally with
the same fields).

```rust
#[test]
fn test_build_line_map_accumulates_sentences_into_paragraph_row() {
    // One DB row = a two-sentence paragraph. The .txt has the two sentences on
    // two physical lines. Both buffer lines must map to work line 0, and
    // work_to_buffer[0] must be the FIRST buffer line (canonical for timestamp).
    let work_lines = vec![
        make_line(10, "Lord have mercy upon us. Christ have mercy upon us.", 0, 0, 1),
    ];
    let file_lines = vec![
        "Lord have mercy upon us.".to_string(),
        "Christ have mercy upon us.".to_string(),
    ];
    let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
    assert_eq!(map.buffer_to_work, vec![Some(0), Some(0)]);
    assert_eq!(map.work_to_buffer[0], 0);
}

#[test]
fn test_build_line_map_accumulate_merged_head_covers_two_rows() {
    // lit.db stores a split title as TWO rows ("The Order for Morning Prayer,"
    // and "Daily Throughout the Year.") but the TEI <head> merges them into ONE
    // .txt line. The merged line must map to the FIRST of the two rows, and the
    // matcher must then advance PAST the second row (consumed by the same line)
    // so the prayer that follows still maps. work_to_buffer for BOTH rows points
    // at the merged buffer line.
    let work_lines = vec![
        make_line(10, "The Order for Morning Prayer,", 1, 0, 1),
        make_line(11, "Daily Throughout the Year.", 1, 0, 2),
        make_line(12, "O Lord, open thou our lips.", 1, 0, 3),
    ];
    let file_lines = vec![
        "      The Order for Morning Prayer, Daily Throughout the Year".to_string(),
        "O Lord, open thou our lips.".to_string(),
    ];
    let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
    // Merged head line maps to the first head row; prayer maps to row 2.
    assert_eq!(map.buffer_to_work, vec![Some(0), Some(2)]);
    // Both merged rows resolve to the merged buffer line (canonical for any
    // chapter/section sign on either).
    assert_eq!(map.work_to_buffer[0], 0);
    assert_eq!(map.work_to_buffer[1], 0);
    assert_eq!(map.work_to_buffer[2], 1);
}

#[test]
fn test_build_line_map_accumulate_skips_chrome_between_rows() {
    // A centered head (no DB row) sits between two prayer rows. The head line
    // maps to None; the two prayer sentence lines map to rows 0 and 1.
    let work_lines = vec![
        make_line(10, "First prayer here.", 0, 0, 1),
        make_line(11, "Second prayer here.", 0, 0, 2),
    ];
    let file_lines = vec![
        "First prayer here.".to_string(),
        "        A Centered Head".to_string(),  // chrome, no row
        "Second prayer here.".to_string(),
    ];
    let map = build_line_map_mode(&file_lines, &work_lines, false, MatchMode::ParagraphAccumulate);
    assert_eq!(map.buffer_to_work, vec![Some(0), None, Some(1)]);
}
```

If `make_line` does not already exist as a test helper, add it near the top of
the test module:

```rust
#[cfg(test)]
fn make_line(id: i64, text: &str, div1: i64, div2: i64, line_in_div: i64) -> Line {
    Line {
        id,
        citation: format!("T.{}.{}.{}", div1, div2, line_in_div),
        is_dialogue: true,
        text: text.to_string(),
        normalized: normalize(text),
        speaker: None,
        timestamp: None,
        div1, div2, line_in_div,
        is_chapter: false,
        is_spoken: None,
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd ~/utono/linux-lit && cargo test --bins build_line_map_accumulate -- --nocapture`
Expected: FAIL to COMPILE — `MatchMode` and `build_line_map_mode` don't exist
yet.

- [ ] **Step 3: Implement the matcher**

In `src/text_file_map.rs`:

(a) Add the mode enum near the top (after the `LineMap` struct):

```rust
/// How `build_line_map` matches buffer lines to DB rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Whole-line normalized equality: one buffer line == one DB row. Plays /
    /// verse, where each physical line is one DB row.
    WholeLine,
    /// Accumulate consecutive buffer lines until their concatenated normalized
    /// text equals a DB row's normalized text, then map the whole run to that
    /// row (first line canonical). Sentence-split prose (BCP from text_file),
    /// where one paragraph row is rendered as several sentence lines.
    ParagraphAccumulate,
}
```

(b) Rename the existing `pub fn build_line_map(file_lines, work_lines, is_prose)`
body into `pub fn build_line_map_mode(file_lines, work_lines, is_prose, mode)`
— keeping the `is_prose` param (it still feeds `is_dialogue` classification
inside the body) and ADDING `mode` — then keep `build_line_map` as a thin
wrapper so existing callers are unaffected. Final signatures:

```rust
pub fn build_line_map_mode(
    file_lines: &[String],
    work_lines: &[Line],
    is_prose: bool,
    mode: MatchMode,
) -> LineMap {
    // ... former build_line_map body, with the matching loop branched on `mode` ...
}

pub fn build_line_map(file_lines: &[String], work_lines: &[Line], is_prose: bool) -> LineMap {
    build_line_map_mode(file_lines, work_lines, is_prose, MatchMode::WholeLine)
}
```

(Test calls become `build_line_map_mode(&file_lines, &work_lines, true, MatchMode::ParagraphAccumulate)`.)

(c) In `build_line_map_mode`, branch the matching loop on `mode`. Keep the
existing whole-line loop for `WholeLine`. For `ParagraphAccumulate`, replace the
per-line search with accumulation:

```rust
    // Build buffer_to_work according to the match mode.
    let mut buffer_to_work: Vec<Option<usize>> = vec![None; n_buf];
    let mut work_to_buffer: Vec<usize> = vec![0; n_work];
    let mut matched = 0usize;

    match mode {
        MatchMode::WholeLine => {
            // ... existing windowed whole-line matcher, unchanged ...
            // (leave the current body here; it already fills buffer_to_work,
            //  work_to_buffer, matched)
        }
        MatchMode::ParagraphAccumulate => {
            // Walk buffer lines, accumulating consecutive non-empty lines until
            // the running normalized concat equals the current DB row. Three
            // wrinkles beyond plain equality:
            //   (1) a paragraph DB row spans several sentence lines  -> accumulate;
            //   (2) chrome lines (heads / rubrics with no DB row)    -> leave None,
            //       resync the cursor without consuming a row;
            //   (3) a MERGED-TITLE buffer line covers two+ DB rows
            //       ("The Order for Morning Prayer, Daily Throughout the Year"
            //        == rows "The Order for Morning Prayer," + "Daily Throughout
            //        the Year.") -> the row is a PREFIX of the line: consume the
            //       row against this line and keep matching further rows against
            //       the SAME line until it is exhausted.
            let mut wi = 0usize;                 // current DB row cursor
            let mut run_start: Option<usize> = None; // first buffer line of the run
            let mut acc = String::new();         // normalized accumulation
            for (bi, nf) in norm_file.iter().enumerate() {
                if nf.is_empty() {
                    continue; // blank / chrome that normalizes to empty
                }
                if wi >= n_work {
                    break;
                }
                let candidate = if acc.is_empty() {
                    nf.clone()
                } else {
                    format!("{} {}", acc, nf)
                };
                if candidate == norm_db[wi] {
                    // (1) Run complete: map every line in the run to wi.
                    let start = run_start.unwrap_or(bi);
                    for b in start..=bi {
                        if !norm_file[b].is_empty() {
                            buffer_to_work[b] = Some(wi);
                        }
                    }
                    work_to_buffer[wi] = start;
                    matched += 1;
                    wi += 1;
                    run_start = None;
                    acc.clear();
                } else if norm_db[wi].starts_with(&candidate) {
                    // Still inside this paragraph row: keep accumulating.
                    if run_start.is_none() {
                        run_start = Some(bi);
                    }
                    acc = candidate;
                } else if run_start.is_none() && consume_merged_rows(
                    nf, &norm_db, &mut wi, &mut work_to_buffer, &mut matched, bi,
                ) {
                    // (3) Merged-title line: `consume_merged_rows` greedily peeled
                    // one or more whole rows that are successive prefixes of `nf`
                    // (each followed by the next, covering the whole line). It set
                    // buffer_to_work for `bi` and advanced `wi`. Nothing else to do.
                    buffer_to_work[bi] = buffer_to_work[bi].or(Some(wi.saturating_sub(1)));
                } else {
                    // (2) Resync. Abandon any partial run, then RETRY this line as
                    // a fresh single-line run against the (possibly advanced) row.
                    run_start = None;
                    acc.clear();
                    if wi < n_work {
                        if norm_db[wi] == *nf {
                            buffer_to_work[bi] = Some(wi);
                            work_to_buffer[wi] = bi;
                            matched += 1;
                            wi += 1;
                        } else if norm_db[wi].starts_with(nf.as_str()) {
                            run_start = Some(bi);
                            acc = nf.clone();
                        }
                        // else: genuine chrome line — leave None, stay on wi.
                    }
                }
            }
        }
    }
```

The `consume_merged_rows(...)` call above refers to a free helper you must add
near `build_line_map_mode`. **Implement it to satisfy the
`test_build_line_map_accumulate_merged_head_covers_two_rows` test (Step 1)** —
that test is the contract. Required behavior:

- Input: one normalized buffer line `nf`, the `norm_db` rows, the current cursor
  `wi`, and `bi`.
- Greedily peel consecutive DB rows that are successive leading prefixes of `nf`
  (row 0 is a prefix of `nf`; after stripping it + one space, row 1 equals the
  remainder, etc.), until `nf` is fully consumed.
- Accept ONLY when **≥ 2** rows were consumed AND they exactly account for the
  whole line (no leftover) — a single-row exact line is already handled by the
  plain-equality branch, so requiring ≥ 2 avoids hijacking ordinary lines.
- On accept: set `work_to_buffer[w] = bi` for each consumed row `w`, advance
  `*wi` past them, add the count to `*matched`, and return `true`. The caller
  sets `buffer_to_work[bi] = Some(first consumed row)`.
- On reject: **fully roll back** any mutation to `work_to_buffer` and `*wi`
  (mutate local copies first, commit only on accept) and return `false`.

Write it test-first: the Step-1 merged-head test must pass, and a one-row line
must NOT be consumed by it (covered by the plain accumulate test). Keep it pure
(no `state`), so it is unit-testable.

Everything after the match (the `dialogue_buffer_lines` collection, the
`LINEMAP: matched` log, `sentence_groups`, `chapter_breaks`, `section_starts`,
and the `LineMap { .. }` return) stays exactly as it is — it already reads
`buffer_to_work` / `work_to_buffer`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd ~/utono/linux-lit && cargo test --bins build_line_map_accumulate -- --nocapture`
Expected: PASS (both new tests).

- [ ] **Step 5: Run the whole text_file_map suite to verify no regression**

Run: `cd ~/utono/linux-lit && cargo test --bins text_file_map -- --nocapture`
Expected: PASS — all existing `build_line_map` / `build_line_map_bcp` tests
still green (the `WholeLine` path is unchanged).

- [ ] **Step 5b: Authoritative integration test against the real BCP1662 data**

The brittle offline Python gate from Task 3 is retired; THIS is the oracle. Add
an `#[ignore]`d test (it needs the real `lit.db` + `.txt` on disk, so it must not
run in the default suite) that loads BCP1662 through the real code path and
asserts a high match rate via the actual matcher:

```rust
#[test]
#[ignore] // needs ~/utono/litdb/data/lit.db + the regenerated .txt on disk
fn bcp1662_accumulate_maps_most_rows() {
    let conn = crate::db::queries::open_db().expect("open lit.db");
    let work = crate::db::queries::load_work(&conn, "BCP1662").expect("load BCP1662");
    let path = work.text_file.clone().expect("BCP1662 has a text_file");
    let contents = std::fs::read_to_string(&path).expect("read .txt");
    let file_lines: Vec<String> = contents.lines().map(String::from).collect();
    let map = build_line_map_mode(
        &file_lines, &work.lines, false, MatchMode::ParagraphAccumulate);
    let matched = map.work_to_buffer.iter().enumerate()
        // a row counts as matched if some buffer line maps to it
        .filter(|(wi, _)| map.buffer_to_work.iter().any(|o| *o == Some(*wi)))
        .count();
    let pct = 100.0 * matched as f64 / work.lines.len() as f64;
    eprintln!("BCP1662 accumulate: {}/{} rows matched ({:.1}%)",
              matched, work.lines.len(), pct);
    assert!(pct >= 95.0, "only {:.1}% of rows matched (want >= 95%)", pct);
}
```

Run it explicitly: `cargo test --bins bcp1662_accumulate -- --ignored --nocapture`
Expected: prints the percentage and PASSES at ≥ 95%. **If it is below 95%, do
NOT commit — report the percentage and the first few unmatched rows as a
DONE_WITH_CONCERNS / BLOCKED so the controller can decide** (the residue may be
a handful of split-title rows that are acceptable, or a real matcher bug). The
controller has already established that 538/677 rows are verbatim single lines
and ~131 are legitimately sentence-split paragraphs, so a correct matcher should
land well above 95%; a much lower number means the accumulation/merge logic is
wrong.

- [ ] **Step 6: Commit**

```bash
cd ~/utono/linux-lit
git add src/text_file_map.rs
git commit -m "feat(linemap): paragraph-accumulation matcher for sentence-split prose

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Route BCP-with-text_file through the accumulate matcher

**Files:**
- Modify: `~/utono/linux-lit/src/app.rs` (`prepare_text_for_display`,
  `prepare_text_only`, `build_line_map_for_prepared`, and the off-thread phase-2
  `build_line_map` call near line 2300)
- Modify: `~/utono/linux-lit/src/text_file_map.rs` (expose a helper to pick the
  mode from a work, to keep the choice in one place)

- [ ] **Step 1: Write the failing test**

Add to `src/text_file_map.rs` tests:

```rust
#[test]
fn test_match_mode_for_work_picks_accumulate_for_bcp_textfile() {
    assert_eq!(match_mode_for_work("BCP1662", true), MatchMode::ParagraphAccumulate);
    // a normal play with a text_file stays whole-line
    assert_eq!(match_mode_for_work("Ham", true), MatchMode::WholeLine);
    // a BCP work without a text_file never reaches build_line_map, but the
    // helper still answers WholeLine defensively.
    assert_eq!(match_mode_for_work("BCP1662", false), MatchMode::WholeLine);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd ~/utono/linux-lit && cargo test --bins match_mode_for_work -- --nocapture`
Expected: FAIL to compile — `match_mode_for_work` does not exist.

- [ ] **Step 3: Implement the mode selector and thread it through**

(a) In `src/text_file_map.rs`, add:

```rust
/// Choose the matcher for a work. A BCP work rendered from its sentence-split
/// `.txt` needs paragraph accumulation; everything else (plays, future prose
/// with a 1:1 .txt) uses whole-line matching.
pub fn match_mode_for_work(abbrev: &str, has_text_file: bool) -> MatchMode {
    if has_text_file && crate::db::line_types::is_bcp_work(abbrev) {
        MatchMode::ParagraphAccumulate
    } else {
        MatchMode::WholeLine
    }
}
```

and make `build_line_map_mode` public (`pub fn`) so `app.rs` can call it.

(b) In `src/app.rs`, in `prepare_text_for_display` (the synchronous path,
around line 3357), replace:

```rust
    let line_map = crate::text_file_map::build_line_map(&cleaned_lines, &work.lines, is_prose);
```

with:

```rust
    let mode = crate::text_file_map::match_mode_for_work(&work.abbrev, work.text_file.is_some());
    let line_map = crate::text_file_map::build_line_map_mode(&cleaned_lines, &work.lines, is_prose, mode);
```

(c) In `src/app.rs`, `build_line_map_for_prepared` (around line 3333) currently
takes `(cleaned_lines, work_lines, is_prose)`. Add the work's abbrev +
has_text_file so it can pick the mode. Change its signature to also accept
`abbrev: &str, has_text_file: bool` and use `match_mode_for_work` +
`build_line_map_mode` inside. Update its one caller accordingly.

(d) In `src/app.rs`, the off-thread phase-2 call (around line 2300-2303 inside
`SnapshotOrPrep::Prep(Some(text_only))`) calls
`crate::text_file_map::build_line_map(&cleaned, &work_lines, is_prose)`. Replace
with the mode-aware form, deriving the mode from the work captured in that scope:

```rust
    let mode = crate::text_file_map::match_mode_for_work(&work.abbrev, work.text_file.is_some());
    let lm = crate::text_file_map::build_line_map_mode(&cleaned, &work_lines, is_prose, mode);
```

(`work` is in scope there — it is the `(work, ...)` from phase 1. If the borrow
checker objects because `work_lines` was moved out of `work`, capture `let mode`
BEFORE the `spawn_blocking` closure and move `mode` into it.)

- [ ] **Step 4: Run the test + build**

Run: `cd ~/utono/linux-lit && cargo test --bins match_mode_for_work -- --nocapture && cargo build`
Expected: PASS, then a clean build (only pre-existing dead-code warnings).

- [ ] **Step 5: Commit**

```bash
cd ~/utono/linux-lit
git add src/app.rs src/text_file_map.rs
git commit -m "feat(app): route BCP text_file works through accumulate matcher

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Bump SNAPSHOT_VERSION and invalidate stale BCP snapshot

**Files:**
- Modify: `~/utono/linux-lit/src/snapshot.rs`

- [ ] **Step 1: Bump the version + update the comment**

In `src/snapshot.rs`, change `pub const SNAPSHOT_VERSION: u32 = 5;` to `= 6;`
and add a comment line above it:

```rust
// Bumped to 6: BCP text_file works now render one sentence per line and map via
// MatchMode::ParagraphAccumulate, so the cached buffer_to_work values differ
// from any v5 snapshot. The serialized shape is unchanged; the bump forces a
// rebuild of stale BCP snapshots.
```

- [ ] **Step 2: Remove the on-disk stale snapshot (so a dev run rebuilds)**

```bash
command rm -f ~/.cache/linux-lit/snapshots/BCP1662.text.bin
```

- [ ] **Step 3: Build to confirm it compiles**

Run: `cd ~/utono/linux-lit && cargo build`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
cd ~/utono/linux-lit
git add src/snapshot.rs
git commit -m "chore(snapshot): bump version for BCP sentence-per-line linemap

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Verify end-to-end (build, unit tests, headless render)

**Files:** none (verification only).

- [ ] **Step 1: Full unit suites**

Run:
```bash
cd ~/utono/linux-lit && cargo test --bins -- --nocapture
cd ~/utono/ws-book-of-common-prayer-references && python -m pytest -q
```
Expected: both green. linux-lit pure-logic suite passes; generator suite passes.

- [ ] **Step 2: Confirm the load maps almost every row (dev log)**

After the user runs `cargo run` on BCP1662 (dev `last_work` is already
BCP1662), inspect the log:

```bash
rg -n "TEXT_FILE: loaded 'BCP1662'|LINEMAP: matched" ~/utono/linux-lit/linux-lit-dev.log | tail -3
```
Expected: `LINEMAP: matched` ≥ 97% and `mapped_buffer_lines` close to the
sentence-line count (was 266 before this work).

- [ ] **Step 3: Headless render check (ask the user — agent cannot launch cage)**

The agent shell cannot launch the headless `cage` harness (the live dwl session
owns the seat; cage dies with SIGTERM/exit 144). Ask the user to run the
single-work headless launch from `linux-lit/CLAUDE.md` → *Headless Verification*
and confirm in the screenshot:
- BCP1662 shows short, indented prayer lines (one sentence each), centered
  heads, inline `Priest.`/`Answer.` speakers — resembling the cummings-brian
  page images.
- `x` (page forward) advances; `G` jumps to the end; neither stalls.
- Timestamp gutter signs (`•` / `◐` / `◑`) appear on prayer lines.

Provide the exact command:
```bash
cd ~/utono/linux-lit && cargo build
GSK_RENDERER=cairo WLR_BACKENDS=headless WLR_RENDERER=pixman XDG_RUNTIME_DIR=/run/user/1000 \
  cage -- ./target/debug/linux-lit 2>/tmp/cage.log &
WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 grim /tmp/shot.png
WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 wtype "x"   # page forward
WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 grim /tmp/shot2.png
pkill -f "cage -- ./target/debug/linux-lit"
```

- [ ] **Step 4: Nav-fuzz on BCP1662 (ask the user to run; agent cannot)**

Per `linux-lit/CLAUDE.md` → *When to ASK THE USER*, pagination changes need the
nav-fuzz. Ask the user to run:
```bash
cd ~/utono/linux-lit
./scripts/e2e-env.sh .claude/skills/test-headless-navigation/run-fuzz.sh --start-work BCP1662 --secs 120
```
Expected: no `UNBALANCED` / stuck-page / gap FAILs attributable to BCP. The full
log lands at `/tmp/fuzz-nav.log`.

---

## Self-review notes

- **Spec coverage:** generator sentence split (Tasks 1–2), regenerate artifact +
  README (Task 3), reader accumulation matcher (Task 4), wiring for both the
  sync and off-thread load paths (Task 5), snapshot invalidation (Task 6),
  verification incl. headless + nav-fuzz (Task 7). The DB fallback for
  text_file-less BCP works is untouched (Task 4 keeps `WholeLine` default;
  `app.rs`'s BCP DB branch is unchanged).
- **Type consistency:** `MatchMode` (`WholeLine` | `ParagraphAccumulate`),
  `build_line_map_mode(file_lines, work_lines, is_prose, mode)`,
  `match_mode_for_work(abbrev, has_text_file)`, and the `make_line` test helper
  are used identically across Tasks 4–5.
- **Open risk to watch during execution:** the accumulation matcher's resync
  branch (chrome between rows) is the subtle part — Task 4 Step 1's
  `test_build_line_map_accumulate_skips_chrome_between_rows` guards it; if the
  real .txt has a row whose text also appears as a prefix of an adjacent row,
  add a targeted test from the actual data. Confirm against the Task 3 Step 2
  offline reconstruction percentage before trusting the live run.

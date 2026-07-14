# Folger XML to Cleaned TXT Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract clean play/poem text from Folger TEI XML files into a stripped-down format that linux-lit can load without runtime filtering.

**Architecture:** A single Python 3 script (`folger-xml-to-cleaned.py`) parses each XML file, walks the TEI structure, reconstructs text from `<w>`/`<c>`/`<pc>` elements between `<milestone>` boundaries, and writes cleaned `.txt` files. A separate SQL script updates the DB paths. One minor Rust change makes `is_act_scene_marker` recognize `## ` headers.

**Tech Stack:** Python 3 stdlib (`xml.etree.ElementTree`), SQLite3, Rust (linux-lit)

---

### Task 1: Create the Python extraction script for plays

**Files:**
- Create: `~/utono/literature/shakespeare-william/folger-xml-to-cleaned.py`

This is the core script. It handles the 38 play XML files (those with `<sp>` elements).

- [ ] **Step 1: Create the script with filename mapping and skeleton**

```python
#!/usr/bin/env python3
"""Extract cleaned text from Folger Shakespeare TEI XML files."""

import os
import sys
import xml.etree.ElementTree as ET

NS = "{http://www.tei-c.org/ns/1.0}"

# XML filename stem -> cleaned output filename stem
# LC (A Lover's Complaint) is from Gutenberg, not Folger XML — skip it.
FILENAME_MAP = {
    "AWW": "alls-well-that-ends-well",
    "AYL": "as-you-like-it",
    "Err": "the-comedy-of-errors",
    "Cor": "coriolanus",
    "Cym": "cymbeline",
    "Ham": "hamlet",
    "1H4": "henry-iv-part-1",
    "2H4": "henry-iv-part-2",
    "H5": "henry-v",
    "1H6": "henry-vi-part-1",
    "2H6": "henry-vi-part-2",
    "3H6": "henry-vi-part-3",
    "H8": "henry-viii",
    "JC": "julius-caesar",
    "Jn": "king-john",
    "Lr": "king-lear",
    "LLL": "loves-labors-lost",
    "Mac": "macbeth",
    "MM": "measure-for-measure",
    "MV": "the-merchant-of-venice",
    "Wiv": "the-merry-wives-of-windsor",
    "MND": "a-midsummer-nights-dream",
    "Ado": "much-ado-about-nothing",
    "Oth": "othello",
    "Per": "pericles",
    "R2": "richard-ii",
    "R3": "richard-iii",
    "Rom": "romeo-and-juliet",
    "Shr": "the-taming-of-the-shrew",
    "Tmp": "the-tempest",
    "Tim": "timon-of-athens",
    "Tit": "titus-andronicus",
    "Tro": "troilus-and-cressida",
    "TN": "twelfth-night",
    "TGV": "the-two-gentlemen-of-verona",
    "TNK": "the-two-noble-kinsmen",
    "WT": "the-winters-tale",
    "Ant": "antony-and-cleopatra",
    "Son": "shakespeares-sonnets",
    "Ven": "venus-and-adonis",
    "Luc": "lucrece",
    "PhT": "the-phoenix-and-turtle",
}

# div1 types to skip entirely
SKIP_DIV1_TYPES = {"preface", "dedication", "argument"}

# Poems: XML files with no <sp> elements — use poem extraction
POEM_STEMS = {"Son", "Ven", "Luc", "PhT"}


def extract_text(el):
    """Recursively extract text from w, c, pc elements within el.

    Walks all descendants. Skips <speaker>, <milestone>, <lb>, <stage> elements
    and their children. Collects text from <w>, <c>, <pc> elements.
    """
    skip_tags = {f"{NS}speaker", f"{NS}milestone", f"{NS}lb", f"{NS}stage"}
    text_tags = {f"{NS}w", f"{NS}c", f"{NS}pc"}
    parts = []

    def walk(node):
        if node.tag in skip_tags:
            return
        if node.tag in text_tags:
            if node.text:
                parts.append(node.text)
            return
        # For container elements (ab, sp, q, seg, foreign, etc.), recurse
        for child in node:
            walk(child)

    walk(el)
    return "".join(parts)


def extract_stage_text(stage_el):
    """Extract text from a <stage> element's w/c/pc children."""
    text_tags = {f"{NS}w", f"{NS}c", f"{NS}pc"}
    parts = []
    for el in stage_el.iter():
        if el.tag in text_tags and el.text:
            parts.append(el.text)
    return "".join(parts).strip()


def process_ab(ab_el, lines):
    """Process an <ab> (anonymous block) element containing milestones and text.

    Each <milestone unit="ftln"> starts a new line. Inline <stage> elements
    are emitted as [text] on their own line.
    """
    current_line_parts = []

    for child in ab_el:
        tag = child.tag

        if tag == f"{NS}milestone" and child.get("unit") == "ftln":
            # Flush current line
            line = "".join(current_line_parts).strip()
            if line:
                lines.append(line)
            current_line_parts = []
            continue

        if tag == f"{NS}stage":
            # Flush any accumulated text first
            line = "".join(current_line_parts).strip()
            if line:
                lines.append(line)
            current_line_parts = []
            # Emit stage direction
            stage_text = extract_stage_text(child)
            if stage_text:
                lines.append(f"[{stage_text}]")
            continue

        if tag in {f"{NS}w", f"{NS}c", f"{NS}pc"}:
            if child.text:
                current_line_parts.append(child.text)
            continue

        # Container elements (q, seg, foreign, etc.) — recurse for w/c/pc
        if tag in {f"{NS}lb", f"{NS}milestone"}:
            continue

        # Recurse into other container elements
        for desc in child.iter():
            if desc.tag in {f"{NS}w", f"{NS}c", f"{NS}pc"} and desc.text:
                current_line_parts.append(desc.text)

    # Flush trailing line
    line = "".join(current_line_parts).strip()
    if line:
        lines.append(line)


def process_play(tree):
    """Process a play XML tree, return list of output lines."""
    root = tree.getroot()
    body = root.find(f".//{NS}body")
    if body is None:
        return []

    output = []

    for div1 in body.findall(f"{NS}div1"):
        div1_type = div1.get("type", "")
        div1_n = div1.get("n", "")

        if div1_type in SKIP_DIV1_TYPES:
            continue

        # Emit div1 header for non-act sections
        if div1_type == "prologue":
            output.append(f"## Prologue")
            output.append("")
        elif div1_type == "epilogue":
            output.append(f"## Epilogue")
            output.append("")
        elif div1_type == "induction":
            output.append(f"## Induction")
            output.append("")
        # For type="act", the header is emitted per-scene as "## Act N, Scene M"

        # Process div2 (scenes) within this div1
        div2s = div1.findall(f"{NS}div2")
        if div2s:
            for div2 in div2s:
                div2_n = div2.get("n", "")
                div2_type = div2.get("type", "")
                # Scene header
                if div1_type == "act":
                    output.append(f"## Act {div1_n}, Scene {div2_n}")
                elif div2_type == "scene" and div1_type in ("prologue", "epilogue", "induction"):
                    # Scene within a prologue/epilogue — rare but handle it
                    output.append(f"## {div1_type.title()}, Scene {div2_n}")
                else:
                    output.append(f"## {div2_type.title()} {div2_n}")
                output.append("")
                process_div_children(div2, output)
        else:
            # No div2 — process children directly (e.g., prologues without scenes)
            process_div_children(div1, output)

    return output


def process_div_children(div_el, output):
    """Process the children of a div1 or div2: stage directions and speeches."""
    for child in div_el:
        tag = child.tag

        if tag == f"{NS}stage":
            stage_text = extract_stage_text(child)
            if stage_text:
                output.append(f"[{stage_text}]")
                output.append("")

        elif tag == f"{NS}sp":
            # Speaker name
            speaker_el = child.find(f"{NS}speaker")
            if speaker_el is not None:
                speaker_name = extract_text(speaker_el) or ""
                # The speaker element contains <w> elements, extract their text
                name_parts = []
                for w in speaker_el.iter(f"{NS}w"):
                    if w.text:
                        name_parts.append(w.text)
                speaker_name = " ".join(name_parts).strip()
                if speaker_name:
                    output.append(speaker_name)

            # Process each <ab> in the speech
            for ab in child.findall(f"{NS}ab"):
                lines = []
                process_ab(ab, lines)
                output.extend(lines)

            output.append("")  # blank line after speech

        # Skip head, lb, milestone, fw, pb, anchor, and other structural elements


def process_poem(tree):
    """Process a poem XML tree (sonnets, Venus, Lucrece, PhT).

    Poems use <milestone unit="line"> for lines and either <div2> (sonnets)
    or <milestone unit="stanza"> (Venus, Lucrece, PhT) for sections.
    """
    root = tree.getroot()
    body = root.find(f".//{NS}body")
    if body is None:
        return []

    output = []

    # Check structure: sonnets use div2, others use stanza milestones
    div1s = body.findall(f"{NS}div1")

    for div1 in div1s:
        div1_type = div1.get("type", "")
        if div1_type in SKIP_DIV1_TYPES:
            continue

        div2s = div1.findall(f"{NS}div2")
        if div2s:
            # Sonnet-style: each div2 is a numbered poem
            for div2 in div2s:
                n = div2.get("n", "")
                output.append(n)
                output.append("")
                lines = extract_poem_lines(div2)
                output.extend(lines)
                output.append("")
        else:
            # Stanza-style: Venus, Lucrece, PhT
            # Walk children looking for stanza milestones and line milestones
            lines = extract_stanza_poem(div1)
            output.extend(lines)

    return output


def extract_poem_lines(container):
    """Extract lines from a container with <milestone unit="line"> markers.

    Returns list of text lines.
    """
    lines = []
    current_parts = []
    text_tags = {f"{NS}w", f"{NS}c", f"{NS}pc"}

    for el in container.iter():
        if el.tag == f"{NS}milestone" and el.get("unit") == "line":
            line = "".join(current_parts).strip()
            if line:
                lines.append(line)
            current_parts = []
            continue

        if el.tag in text_tags and el.text:
            current_parts.append(el.text)

    # Flush trailing line
    line = "".join(current_parts).strip()
    if line:
        lines.append(line)

    return lines


def extract_stanza_poem(div1):
    """Extract lines from a stanza-based poem (Venus, Lucrece, PhT).

    Stanza milestones produce blank-line separators. Line milestones
    produce line breaks.
    """
    output = []
    current_parts = []
    stanza_count = 0
    text_tags = {f"{NS}w", f"{NS}c", f"{NS}pc"}

    for el in div1.iter():
        if el.tag == f"{NS}milestone":
            unit = el.get("unit", "")
            if unit == "stanza":
                # Flush current line
                line = "".join(current_parts).strip()
                if line:
                    output.append(line)
                current_parts = []
                # Add stanza separator (blank line) between stanzas
                if stanza_count > 0:
                    output.append("")
                stanza_count += 1
                continue
            elif unit == "line":
                line = "".join(current_parts).strip()
                if line:
                    output.append(line)
                current_parts = []
                continue

        if el.tag in text_tags and el.text:
            current_parts.append(el.text)

    # Flush trailing
    line = "".join(current_parts).strip()
    if line:
        output.append(line)
    output.append("")

    return output


def main():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    xml_dir = os.path.join(script_dir, "folger-xml")
    out_dir = os.path.join(script_dir, "folger-cleaned")
    os.makedirs(out_dir, exist_ok=True)

    if not os.path.isdir(xml_dir):
        print(f"ERROR: XML directory not found: {xml_dir}", file=sys.stderr)
        sys.exit(1)

    processed = 0
    errors = []

    for xml_stem, out_stem in sorted(FILENAME_MAP.items()):
        xml_path = os.path.join(xml_dir, f"{xml_stem}.xml")
        out_path = os.path.join(out_dir, f"{out_stem}.txt")

        if not os.path.isfile(xml_path):
            errors.append(f"SKIP: {xml_path} not found")
            continue

        try:
            tree = ET.parse(xml_path)
        except ET.ParseError as e:
            errors.append(f"ERROR: {xml_path}: {e}")
            continue

        if xml_stem in POEM_STEMS:
            lines = process_poem(tree)
        else:
            lines = process_play(tree)

        # Clean up: collapse 3+ consecutive blank lines to 2
        cleaned = []
        blank_count = 0
        for line in lines:
            if line == "":
                blank_count += 1
                if blank_count <= 1:
                    cleaned.append(line)
            else:
                blank_count = 0
                cleaned.append(line)

        # Strip trailing blank lines
        while cleaned and cleaned[-1] == "":
            cleaned.pop()

        with open(out_path, "w", encoding="utf-8") as f:
            f.write("\n".join(cleaned) + "\n")

        line_count = len([l for l in cleaned if l])
        print(f"  {xml_stem:>4} -> {out_stem}.txt ({line_count} non-blank lines)")
        processed += 1

    print(f"\nProcessed {processed} files -> {out_dir}")
    if errors:
        print("\nIssues:")
        for e in errors:
            print(f"  {e}")

    # Emit SQL update statements
    sql_path = os.path.join(out_dir, "update-db-paths.sql")
    with open(sql_path, "w") as f:
        for xml_stem, out_stem in sorted(FILENAME_MAP.items()):
            out_path = os.path.join(out_dir, f"{out_stem}.txt")
            f.write(f"UPDATE works SET text_file = '{out_path}' WHERE abbrev = '{xml_stem}';\n")
    print(f"SQL update script: {sql_path}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Make the script executable and run it**

```bash
chmod +x ~/utono/literature/shakespeare-william/folger-xml-to-cleaned.py
cd ~/utono/literature/shakespeare-william && python3 folger-xml-to-cleaned.py
```

Expected: 42 files created in `folger-cleaned/`, summary printed showing each file with its line count. `update-db-paths.sql` created.

- [ ] **Step 3: Spot-check a play — compare Troilus first speech**

```bash
head -20 ~/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt
```

Expected output (approximately):
```
## Prologue

[Enter the Prologue in armor.]

PROLOGUE
In Troy there lies the scene. From isles of Greece
The princes orgulous, their high blood chafed,
```

Also check that the preface ("A never writer to an ever reader...") and character list are NOT present:

```bash
grep -c "never writer" ~/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt
```

Expected: `0`

- [ ] **Step 4: Spot-check a poem — verify Sonnet 1 format**

```bash
head -20 ~/utono/literature/shakespeare-william/folger-cleaned/shakespeares-sonnets.txt
```

Expected output (approximately):
```
1

From fairest creatures we desire increase,
That thereby beauty's rose might never die,
But as the riper should by time decease,
```

- [ ] **Step 5: Spot-check edge cases**

Check Henry V (has a prologue as div1):
```bash
head -5 ~/utono/literature/shakespeare-william/folger-cleaned/henry-v.txt
```
Expected: starts with `## Prologue`

Check 2 Henry IV (has induction and epilogue):
```bash
grep "## Induction\|## Epilogue" ~/utono/literature/shakespeare-william/folger-cleaned/henry-iv-part-2.txt
```
Expected: both present

Check Taming of the Shrew (has induction):
```bash
grep "## Induction" ~/utono/literature/shakespeare-william/folger-cleaned/the-taming-of-the-shrew.txt
```
Expected: present

- [ ] **Step 6: Fix any issues found in spot checks**

If the output doesn't match expected, debug the script. Common issues:
- Milestones in `<ab>` may have `unit="line"` instead of `unit="ftln"` in some contexts (check and handle both)
- Inline `<q>` elements wrapping `<w>` elements — the recursive `process_ab` should handle these
- `<seg>` elements (songs/letters) containing milestones — test with a play that has songs

- [ ] **Step 7: Commit the script and generated files**

```bash
cd ~/utono/literature/shakespeare-william
git add folger-xml-to-cleaned.py folger-cleaned/
git commit -m "feat: add Folger XML to cleaned txt extraction script and output"
```

---

### Task 2: Update the database to point to cleaned files

**Files:**
- Modify: `~/utono/litdb/data/lit.db` (via SQL)

- [ ] **Step 1: Review the generated SQL**

```bash
cat ~/utono/literature/shakespeare-william/folger-cleaned/update-db-paths.sql | head -5
```

Expected: lines like:
```sql
UPDATE works SET text_file = '/home/mlj/utono/literature/shakespeare-william/folger-cleaned/alls-well-that-ends-well.txt' WHERE abbrev = 'AWW';
```

- [ ] **Step 2: Back up the database**

```bash
cp ~/utono/litdb/data/lit.db ~/utono/litdb/data/lit.db.bak-$(date +%Y%m%d)
```

- [ ] **Step 3: Run the SQL update**

```bash
sqlite3 ~/utono/litdb/data/lit.db < ~/utono/literature/shakespeare-william/folger-cleaned/update-db-paths.sql
```

- [ ] **Step 4: Verify the update**

```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT abbrev, text_file FROM works WHERE text_file LIKE '%folger-cleaned%' LIMIT 5"
```

Expected: paths now point to `folger-cleaned/` directory.

Also verify LC (A Lover's Complaint) was NOT changed:
```bash
sqlite3 ~/utono/litdb/data/lit.db "SELECT abbrev, text_file FROM works WHERE abbrev = 'LC'"
```

Expected: still points to `folger-txt/a-lovers-complaint-gutenberg.txt`

---

### Task 3: Update `is_act_scene_marker` to recognize `## ` headers

**Files:**
- Modify: `~/utono/linux-lit/src/db/line_types.rs:43-49`
- Modify: `~/utono/linux-lit/src/db/line_types.rs` (tests)

- [ ] **Step 1: Write the failing test**

Add to the `test_act_scene_marker` test in `src/db/line_types.rs`:

```rust
    #[test]
    fn test_act_scene_marker() {
        assert!(is_act_scene_marker("ACT 1"));
        assert!(is_act_scene_marker("SCENE 2"));
        assert!(is_act_scene_marker("Scene 3"));
        assert!(is_act_scene_marker("Act 1"));
        assert!(is_act_scene_marker("PROLOGUE"));
        assert!(is_act_scene_marker("Prologue"));
        assert!(is_act_scene_marker("EPILOGUE"));
        assert!(is_act_scene_marker("Epilogue"));
        assert!(!is_act_scene_marker("Action"));
        // New: ## headers from cleaned format
        assert!(is_act_scene_marker("## Act 1, Scene 1"));
        assert!(is_act_scene_marker("## Prologue"));
        assert!(is_act_scene_marker("## Epilogue"));
        assert!(is_act_scene_marker("## Induction"));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test test_act_scene_marker -- --nocapture
```

Expected: FAIL — the `## Act 1, Scene 1` assertions fail because `is_act_scene_marker` doesn't strip `## `.

- [ ] **Step 3: Update `is_act_scene_marker` to handle `## ` prefix**

In `src/db/line_types.rs`, change `is_act_scene_marker`:

```rust
pub fn is_act_scene_marker(text: &str) -> bool {
    let trimmed = text.trim();
    let stripped = trimmed.strip_prefix("## ").unwrap_or(trimmed);
    let upper = stripped.to_uppercase();
    upper.starts_with("ACT ")
        || upper.starts_with("SCENE ")
        || upper.starts_with("PROLOGUE")
        || upper.starts_with("EPILOGUE")
        || upper.starts_with("INDUCTION")
}
```

Note: also adds "INDUCTION" which was missing (needed for 2H4 and Shr cleaned files).

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test test_act_scene_marker -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Run all tests to check for regressions**

```bash
cargo test
```

Expected: all tests pass

- [ ] **Step 6: Build to verify compilation**

```bash
cargo build
```

Expected: compiles with no new errors

- [ ] **Step 7: Commit**

```bash
cd ~/utono/linux-lit
git add src/db/line_types.rs
git commit -m "feat: recognize ## headers and Induction in act/scene markers"
```

---

### Task 4: Validate cleaned files with linux-lit line map matching

**Files:** (no modifications — validation only)

- [ ] **Step 1: Run linux-lit with a cleaned file and check the log**

The user runs `cargo run` and opens Troilus and Cressida. Then check the log:

```bash
grep "LINEMAP" ~/utono/linux-lit/linux-lit-dev.log
```

Expected: `LINEMAP: matched NNNN/3576 work lines (>=96.0%)`

The match percentage should be at least 96%. If significantly lower than the previous 96.4% match rate, the cleaned file has issues.

- [ ] **Step 2: Check that the preamble is gone from the display**

The user opens Troilus — the text should start at `## Prologue` or the first stage direction, not with "Troilus and Cressida by William Shakespeare..."

- [ ] **Step 3: Check dialogue formatting**

Speaker names should appear correctly formatted (small caps, indented). Stage directions should be italic. Act/scene headers should be bold.

- [ ] **Step 4: If match percentage dropped, debug**

Compare a problematic section:
```bash
diff <(head -50 ~/utono/literature/shakespeare-william/folger-cleaned/troilus-and-cressida.txt) <(head -50 ~/utono/literature/shakespeare-william/folger-txt/troilus-and-cressida_TXT_FolgerShakespeare.txt)
```

Common causes of lower matching:
- Quotation marks: XML uses `<q>` elements which may render differently than the txt version
- Hyphens: XML may join hyphenated words differently
- Apostrophes: unicode vs ASCII differences

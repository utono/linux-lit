# Authorship Attribution System

linux-lit displays collaborator-attributed lines in italics for Shakespeare's co-authored works. This document describes the data model, scholarly sources, and how to add or modify attributions.

## Data Model

Two tables in `~/utono/litdb/data/lit.db`:

**`attribution_sets`** — one row per scholarly attribution hypothesis per work.

- `id` — primary key
- `work_abbrev` — FK to `works.abbrev` (e.g., `H8`, `TNK`, `Tim`, `Per`)
- `name` — short identifier (e.g., `spedding-hoy`)
- `display_name` — shown in the picker (e.g., `Spedding-Hoy (1850)`)
- `primary_author` — always `shakespeare`
- `secondary_author` — the collaborator (`fletcher`, `middleton`, `wilkins`)
- `description`, `source_citation` — scholarly reference
- `created_at` — timestamp
- Unique on `(work_abbrev, name)`

**`line_authorship`** — one row per collaborator-attributed line.

- `id` — primary key
- `attribution_set_id` — FK to `attribution_sets.id`
- `citation` — Folger citation format: `{work_abbrev}.{div1}.{div2}.{line_in_div}` (e.g., `H8.1.3.15`)
- `author` — the collaborator name (e.g., `fletcher`)
- `confidence` — unused (NULL)
- `notes` — unused (NULL)
- Unique on `(attribution_set_id, citation)`

Only lines attributed to the **secondary author** (the collaborator) are stored. Shakespeare lines are implied by absence.

## Citation Format

Citations match the `line_mapping` table's structure: `{work_abbrev}.{div1}.{div2}.{line_in_div}`.

- `div1` = act number (0 = Prologue, 6 = Epilogue)
- `div2` = scene number within act (0 = Chorus/Prologue/Epilogue)
- `line_in_div` = line number within the scene

Examples: `H8.0.0.1` (Prologue line 1), `TNK.3.6.100` (Act 3 Scene 6 line 100), `Per.2.0.5` (Act 2 Chorus line 5).

## Current Attribution Sets

### Set 1: Henry VIII (H8) — Spedding-Hoy (1850)

**Source:** James Spedding, "Who Wrote Shakespeare's Henry VIII?" (1850); Cyrus Hoy confirmation (1962).

**Collaborator:** John Fletcher

**Fletcher scenes:**
- Prologue (0.0)
- Act 1 Scene 3 (1.3)
- Act 1 Scene 4 (1.4)
- Act 2 Scene 1 (2.1)
- Act 2 Scene 2 (2.2)
- Act 3 Scene 1 (3.1)
- Act 4 Scene 1 (4.1)
- Act 4 Scene 2 (4.2)
- Act 5 Scene 2 (5.2) — the christening scene
- Act 5 Scene 4 (5.4) — christening pageant
- Epilogue (6.0)

**Shakespeare scenes:** 1.1, 1.2, 2.3, 2.4, 3.2, 5.1, 5.3

**Note:** Act 3 Scene 2 is traditionally considered mixed — Shakespeare wrote the first portion (Wolsey's fall) and Fletcher the latter, but the Folger edition treats it as a single scene (div1=3, div2=2, 540 lines). The current attribution assigns the entire scene to Shakespeare (omits it from `line_authorship`). A future enhancement could split this scene at the transition point.

### Set 2: Two Noble Kinsmen (TNK) — Riverside-Smith (1974)

**Source:** Hallet Smith, The Riverside Shakespeare (1974); orthodox scholarly consensus.

**Collaborator:** John Fletcher

**Fletcher scenes:**
- Act 2: all scenes (2.1 through 2.6)
- Act 3: Scenes 3-6 (3.3, 3.4, 3.5, 3.6)
- Act 4: all scenes (4.1, 4.2, 4.3)
- Act 5 Scene 2 (5.2)

**Shakespeare scenes:** Prologue (0.0), 1.1, 1.2, 1.3, 1.4, 1.5, 3.1, 3.2, 5.1, 5.3, 5.4, Epilogue (6.0)

### Set 3: Timon of Athens (Tim) — Taylor-Loughnane (2016)

**Source:** Gary Taylor & Rory Loughnane, New Oxford Shakespeare (2016); based on Jowett's Oxford edition (2004).

**Collaborator:** Thomas Middleton

**Middleton scenes:**
- Act 1 Scene 2 (1.2)
- Act 3: all scenes (3.1 through 3.6)
- Act 4 Scene 1 (4.1)
- Act 4 Scene 2 (4.2)

**Shakespeare scenes:** 1.1, 2.1, 2.2, 4.3, 5.1, 5.2, 5.3, 5.4

### Set 4: Pericles (Per) — Traditional Consensus

**Source:** Traditional consensus; George Wilkins attributed Acts I-II (c. 1607).

**Collaborator:** George Wilkins

**Wilkins scenes:**
- Act 1: Chorus and all scenes (1.0 through 1.4)
- Act 2: Chorus and all scenes (2.0 through 2.5)

**Shakespeare scenes:** Acts 3-5 and Epilogue (3.0 through 6.0)

## How Attribution Displays in linux-lit

- On work load, linux-lit queries `attribution_sets` for the work's `work_abbrev`
- If data exists, the first set is auto-selected and collaborator lines render in **italics**
- `Ctrl+a` toggles authorship display on/off (shows toast)
- `Ctrl+Shift+A` opens a picker if multiple attribution sets exist for a work
- Works without attribution data are unaffected

## How to Add or Modify Attributions

### Adding a new attribution set for an existing work

If a different scholarly attribution exists (e.g., a new edition with different scene assignments):

```sql
-- 1. Create the attribution set
INSERT INTO attribution_sets
  (work_abbrev, name, display_name, primary_author, secondary_author, description, source_citation)
VALUES
  ('H8', 'hope-2022', 'Hope (2022)', 'shakespeare', 'fletcher',
   'Alternative attribution based on function word analysis',
   'Jonathan Hope, Shakespeare and Authorship (2022)');

-- 2. Get the new set ID
SELECT id FROM attribution_sets WHERE work_abbrev = 'H8' AND name = 'hope-2022';
-- Suppose it returns 5

-- 3. Insert line attributions for each Fletcher scene
INSERT INTO line_authorship (attribution_set_id, citation, author)
SELECT 5, 'H8.' || div1 || '.' || div2 || '.' || line_in_div, 'fletcher'
FROM line_mapping
WHERE work_abbrev = 'H8'
  AND (div1, div2) IN (
    (0, 0),   -- Prologue
    (1, 3),   -- etc.
    (1, 4)
  );
```

### Adding a new co-authored work

```sql
-- 1. Create the attribution set
INSERT INTO attribution_sets
  (work_abbrev, name, display_name, primary_author, secondary_author, description, source_citation)
VALUES
  ('Mac', 'taylor-2016', 'Taylor (2016)', 'shakespeare', 'middleton',
   'Middleton interpolations in Macbeth',
   'Gary Taylor, New Oxford Shakespeare (2016)');

-- 2. Get the set ID
SELECT id FROM attribution_sets WHERE work_abbrev = 'Mac' AND name = 'taylor-2016';

-- 3. Insert line attributions for Middleton scenes/interpolations
INSERT INTO line_authorship (attribution_set_id, citation, author)
SELECT <set_id>, 'Mac.' || div1 || '.' || div2 || '.' || line_in_div, 'middleton'
FROM line_mapping
WHERE work_abbrev = 'Mac'
  AND (div1, div2) IN (...);
```

### Modifying an existing attribution

To reassign scenes within an existing set:

```sql
-- Remove lines for a scene
DELETE FROM line_authorship
WHERE attribution_set_id = 1
  AND citation LIKE 'H8.5.4.%';

-- Add lines for a different scene
INSERT INTO line_authorship (attribution_set_id, citation, author)
SELECT 1, 'H8.' || div1 || '.' || div2 || '.' || line_in_div, 'fletcher'
FROM line_mapping
WHERE work_abbrev = 'H8' AND div1 = 5 AND div2 = 3;
```

### Verifying attribution data

```sql
-- Check coverage: attributed lines vs total for a work
SELECT
  as2.work_abbrev,
  as2.display_name,
  (SELECT COUNT(*) FROM line_authorship la WHERE la.attribution_set_id = as2.id) as attributed,
  (SELECT COUNT(*) FROM line_mapping lm WHERE lm.work_abbrev = as2.work_abbrev) as total
FROM attribution_sets as2;

-- Check per-scene attribution
SELECT div1, div2, COUNT(*) as total_lines,
  (SELECT COUNT(*) FROM line_authorship la
   WHERE la.attribution_set_id = 1
     AND la.citation = 'H8.' || lm.div1 || '.' || lm.div2 || '.' || la.citation
  ) as dummy
FROM line_mapping lm
WHERE work_abbrev = 'H8'
GROUP BY div1, div2
ORDER BY div1, div2;

-- Simpler: list scenes with their attribution count
SELECT
  lm.div1, lm.div2,
  COUNT(*) as total,
  COUNT(la.id) as attributed
FROM line_mapping lm
LEFT JOIN line_authorship la
  ON la.attribution_set_id = 1
  AND la.citation = lm.work_abbrev || '.' || lm.div1 || '.' || lm.div2 || '.' || lm.line_in_div
WHERE lm.work_abbrev = 'H8'
GROUP BY lm.div1, lm.div2
ORDER BY lm.div1, lm.div2;
```

## Data Provenance

The original per-line attributions were hand-entered into `~/utono/literature/gloss.db` (January 2026), keyed by Gutenberg edition line numbers. A migration script (`~/utono/litdb/scripts/migrate_authorship.py`) converted them to Folger citation format in `lit.db` by matching normalized line text between editions. This migration had ~56% success rate due to text differences between Gutenberg and Folger editions.

The current data was rebuilt from scratch using scene-level attributions (May 2026), which is more complete and accurate than the per-line migration approach. All lines in a collaborator-assigned scene are attributed to the collaborator.

## Scholarly References

- **Spedding (1850):** James Spedding, "Who Wrote Shakespeare's Henry VIII?" *The Gentleman's Magazine*, August 1850.
- **Hoy (1962):** Cyrus Hoy, "The Shares of Fletcher and his Collaborators in the Beaumont and Fletcher Canon (VII)," *Studies in Bibliography* 15 (1962): 71-90.
- **Riverside-Smith (1974):** Hallet Smith, ed., *The Riverside Shakespeare* (Boston: Houghton Mifflin, 1974).
- **Taylor & Loughnane (2016):** Gary Taylor & Rory Loughnane, "The Canon and Chronology of Shakespeare's Works," in *The New Oxford Shakespeare: Authorship Companion* (Oxford University Press, 2016).
- **Jowett (2004):** John Jowett, ed., *The Life of Timon of Athens*, Oxford Shakespeare (Oxford University Press, 2004).
- **Traditional Pericles consensus:** The division of Pericles into Wilkins (Acts I-II) and Shakespeare (Acts III-V) is the standard editorial position since Delius (1868), supported by stylometric work including Craig & Kinney (2009).

## Known Limitations

- **H8 Act 3 Scene 2** is traditionally considered a mixed scene (Shakespeare/Fletcher transition mid-scene). The current attribution assigns the entire scene to Shakespeare. A sub-scene split would require identifying the transition line.
- **TNK Act 3 Scene 1** is sometimes considered partially Fletcher. The current attribution assigns it entirely to Shakespeare.
- Only one attribution set per work exists. The picker supports multiple sets if scholars disagree on attribution boundaries.

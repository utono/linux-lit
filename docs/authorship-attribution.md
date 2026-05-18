# Authorship Attribution System

linux-lit displays collaborator-attributed lines in italics for Shakespeare's co-authored works. This document describes the data model, scholarly sources, and how to add or modify attributions.

## Shakespeare's Co-Authored Works

Shakespeare collaborated with other playwrights throughout his career. Early modern drama was frequently collaborative — perhaps a third of all plays from the period involved multiple authors. Identifying Shakespeare's collaborators has been a scholarly project since the eighteenth century, accelerated in recent decades by computational stylometry (statistical analysis of vocabulary, function words, metrical patterns, and other linguistic features).

The landmark publication in this area is the New Oxford Shakespeare (NOS, 2016), edited by Gary Taylor, John Jowett, Terri Bourus, and Gabriel Egan. Its *Authorship Companion* volume, with the chapter "The Canon and Chronology of Shakespeare's Works" by Taylor and Rory Loughnane, provides the most comprehensive modern assessment of Shakespeare's collaborative works. The NOS identifies co-authorship in roughly a third of the plays in the canon.

### Plays with strong co-authorship consensus

These plays are widely accepted as collaborative by mainstream Shakespeare scholarship. The evidence comes from converging lines of inquiry: linguistic stylometry, verse tests (metrical habits, feminine endings, rhyme patterns), bibliographic analysis, and historical documentation.

**Titus Andronicus** (c. 1593) — with **George Peele**. Peele wrote Act 1 and parts of Acts 2 and 4 (specifically 2.1, 2.2, and 4.1). The case rests on Peele's distinctive stylistic markers: classicizing rhetoric, alliterative patterns, and vocabulary clusters shared with his solo plays. Brian Vickers established the modern consensus in *Shakespeare, Co-Author* (2002), building on earlier work by MacDonald P. Jackson. The NOS (2016) confirmed Peele's participation. Shakespeare wrote the more dramatically intense middle and final acts, including the mutilation scene (2.3-2.4) and the revenge plot (Acts 3 and 5).

**1 Henry VI** (c. 1591) — with **Thomas Nashe** and possibly others. The play has long been suspected of multiple authorship. The NOS attributes Act 1 to Nashe, based on vocabulary analysis (Craig & Kinney 2009) and Nashe's characteristic prose rhythms. Shakespeare is credited with the Talbot scenes (2.4, 4.2-4.7) and possibly other passages. The remaining scenes — the Temple Garden, the Joan of Arc sequences, much of Acts 2-3 and 5 — may involve one or more additional collaborators whose identity remains disputed. Christopher Marlowe has been proposed (by Craig & Kinney and by Santiago Segarra et al. 2016), but this attribution is contested. The play may represent Shakespeare's earliest work for the stage, joining an existing collaborative project.

**Henry VIII / All Is True** (1613) — with **John Fletcher**. The most extensively studied of all Shakespeare collaborations. James Spedding proposed the division in 1850 on the basis of metrical tests, and his scene-by-scene attribution has been substantially confirmed by every subsequent study, including Cyrus Hoy's comprehensive analysis of Fletcher's linguistic preferences (1962) and modern computational work. Fletcher wrote roughly half the play, including the Prologue, Epilogue, and scenes centered on ceremony and spectacle (1.3-1.4, 2.1-2.2, 3.1, 4.1-4.2). Shakespeare wrote the dramatic core: Buckingham's arrest (1.1-1.2), the trial of Queen Katherine (2.3-2.4), and Wolsey's fall (3.2). Act 3 Scene 2 is the most contested — Spedding placed the transition mid-scene at Wolsey's "Farewell, a long farewell to all my greatness," with Shakespeare writing the fall and Fletcher the aftermath.

**The Two Noble Kinsmen** (1613-14) — with **John Fletcher**. Published in 1634 with both names on the title page — rare documentary evidence of collaboration. The division follows the play's tonal shifts: Shakespeare wrote the framing narrative (Prologue, Act 1, parts of Act 3, the final scenes) while Fletcher wrote the romantic subplot, the Jailer's Daughter scenes, and the comic episodes (Act 2, parts of Acts 3-4, 5.2). Hallet Smith's division for the Riverside Shakespeare (1974) represents the orthodox consensus, confirmed by subsequent stylometric work.

**Timon of Athens** (c. 1606) — with **Thomas Middleton**. The play survives only in the First Folio and shows signs of incomplete revision. John Jowett's Oxford edition (2004) established Middleton's contribution, later confirmed by Taylor and Loughnane (NOS 2016). Middleton wrote the satirical banquet scene (1.2), most of Act 3 (the creditor scenes), and parts of Act 4 (4.1-4.2). Shakespeare wrote the opening scene with the artist and poet (1.1), the Senate scenes (2.1-2.2), and the great misanthropic soliloquies of the wilderness acts (4.3, Act 5). The collaboration may have been sequential rather than simultaneous — Middleton possibly completing or revising a play Shakespeare left unfinished.

**Pericles** (c. 1607-08) — with **George Wilkins**. Not included in the First Folio, possibly because the editors recognized it as partly non-Shakespearean. The traditional division — Wilkins wrote Acts 1-2, Shakespeare wrote Acts 3-5 — has been the standard editorial position since Delius (1868) and is supported by stylometric analysis (Craig & Kinney 2009). The first two acts are dramatically weaker, with wooden verse and episodic plotting. Shakespeare's Acts 3-5 contain the storm scene, the Marina sequences, and the great recognition scene (5.1), widely considered among his finest late writing. Wilkins published a prose version, *The Painful Adventures of Pericles Prince of Tyre* (1608), which appears to draw on his own contributions to the play.

### Plays with posthumous adaptation

These plays were revised by another playwright after Shakespeare's death (1616), before publication in the First Folio (1623) or later.

**Macbeth** (c. 1606, adapted c. 1616) — adapted by **Thomas Middleton**. The Folio text of Macbeth is unusually short and shows signs of theatrical revision. Middleton added the Hecate scene (3.5) and Hecate's speeches in 4.1, incorporating two songs ("Come away, come away" and "Black spirits") from his own play *The Witch* (c. 1615). John Jowett's analysis (2013) and the NOS (2016) both identify Middleton's hand. The adaptation is small in scope — perhaps 50 lines — but the Hecate material has a noticeably different style and dramatic function from the surrounding Shakespeare text. Some scholars (notably Brooke 1990) dispute any non-Shakespearean presence.

**Measure for Measure** (c. 1604, adapted c. 1621) — adapted by **Thomas Middleton**. The NOS (2016) identifies Middleton as having substantially revised Act 1 Scene 2 and contributed scattered interpolations elsewhere, including the song at the opening of 4.1 ("Take, O take those lips away"). The evidence includes Middleton's distinctive oaths and colloquialisms, and the scene's structural anomalies. This is the most controversial of the NOS co-authorship attributions — John Jowett (2007) makes the case, but many scholars remain skeptical, and the Arden and Cambridge editions do not accept Middleton's involvement.

### Plays with debated or partial co-authorship claims

These plays have been proposed as collaborative but lack the broad consensus of the cases above.

**Edward III** (c. 1593) — **collaborator unknown**. The play was attributed to Shakespeare on external grounds and included in the NOS (2016). Shakespeare is generally credited with the Countess of Salisbury scenes (Acts 1-2), based on stylistic parallels with the Sonnets and other early plays. The remaining acts involve military campaigns in France and are stylistically distinct, but the collaborator has not been convincingly identified. Edward III is not currently in the lit.db `works` table.

**The Spanish Tragedy additions** (c. 1602) — possibly **Shakespeare**. Ben Jonson was paid for additions to Thomas Kyd's play, but some scholars have attributed the 1602 additions to Shakespeare instead. The evidence is inconclusive. Not in lit.db.

**Sir Thomas More** (c. 1600-04) — **Hand D** is widely accepted as Shakespeare's autograph. The play was a collaboration among Anthony Munday, Henry Chettle, Thomas Heywood, Thomas Dekker, and Shakespeare, who contributed a scene of approximately 147 lines. The manuscript survives (British Library MS Harley 7368). Not in lit.db as a complete work.

**Cardenio / Double Falsehood** (1612-13) — with **John Fletcher**. A lost play. Lewis Theobald's *Double Falsehood* (1727) claims to be based on Shakespeare and Fletcher's manuscript. Brean Hammond's Arden edition (2010) accepts the attribution; the NOS includes it cautiously. Not in lit.db.

### Plays sometimes proposed but generally rejected

**2 Henry VI** and **3 Henry VI** — Earlier scholars (Malone, Fleay) proposed collaborative authorship, but modern stylometric analysis consistently attributes both plays entirely to Shakespeare. The NOS agrees.

**The Taming of the Shrew** — Occasionally proposed as collaborative (with an unknown hand for the Induction or the subplot), but the evidence is weak. The NOS attributes it solely to Shakespeare.

**All's Well That Ends Well** — Middleton's involvement has been suggested based on some lexical evidence, but the case has not achieved consensus. The NOS does not identify a collaborator.

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

### Set 5: Titus Andronicus (Tit) — Vickers (2002)

**Source:** Brian Vickers, *Shakespeare, Co-Author* (2002); confirmed by New Oxford Shakespeare (2016).

**Collaborator:** George Peele

**Peele scenes:**
- Act 1 (1.1) — the entire first act
- Act 2 Scene 1 (2.1)
- Act 2 Scene 2 (2.2)
- Act 4 Scene 1 (4.1)

**Shakespeare scenes:** 2.3, 2.4, 3.1, 3.2, 4.2, 4.3, 4.4, 5.1, 5.2, 5.3

### Set 6: Macbeth (Mac) — Taylor (2016)

**Source:** Gary Taylor & Rory Loughnane, New Oxford Shakespeare (2016); John Jowett, "Middleton and Macbeth" (2013).

**Collaborator:** Thomas Middleton (posthumous adaptation)

**Middleton scenes:**
- Act 3 Scene 5 (3.5) — the Hecate scene, entirely Middleton

**Note:** Middleton also inserted Hecate speeches into Act 4 Scene 1 (lines ~39-43, ~125-132) and added songs from his play *The Witch*. These are line-level interpolations within a Shakespeare scene and cannot be cleanly attributed at scene granularity. Only the wholly Middleton scene (3.5) is attributed.

**Shakespeare scenes:** all others

### Set 7: 1 Henry VI (1H6) — NOS (2016)

**Source:** Gary Taylor & Rory Loughnane, New Oxford Shakespeare (2016); Craig & Kinney (2009).

**Collaborator:** Thomas Nashe

**Nashe scenes:**
- Act 1: all scenes (1.1 through 1.6)

**Shakespeare scenes:** 2.4, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7

**Note:** The remaining scenes (2.1-2.3, 2.5, 3.1-3.4, 4.1, 5.1-5.5) are attributed to anonymous collaborators by the NOS — possibly Marlowe, Peele, or other hands. Only the Nashe attribution has strong consensus and is included here.

### Set 8: Measure for Measure (MM) — Taylor (2016)

**Source:** Gary Taylor & Rory Loughnane, New Oxford Shakespeare (2016); John Jowett, "Middleton and Measure for Measure" (2007).

**Collaborator:** Thomas Middleton (posthumous adaptation)

**Middleton scenes:**
- Act 1 Scene 2 (1.2) — substantial rewriting

**Note:** Middleton also contributed the song "Take, O take those lips away" at the opening of 4.1 and scattered interpolations throughout the play. These are line-level insertions within Shakespeare scenes. Only 1.2 (the most substantially rewritten scene) is attributed at scene granularity. This is the most controversial of the NOS co-authorship claims.

**Shakespeare scenes:** all others

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
  ('E3', 'nos-2016', 'NOS (2016)', 'shakespeare', 'unknown',
   'Shakespeare attributed Acts 1-2 (Countess scenes)',
   'Gary Taylor, New Oxford Shakespeare (2016)');

-- 2. Get the set ID
SELECT id FROM attribution_sets WHERE work_abbrev = 'E3' AND name = 'nos-2016';

-- 3. Insert line attributions for non-Shakespeare scenes
INSERT INTO line_authorship (attribution_set_id, citation, author)
SELECT <set_id>, 'E3.' || div1 || '.' || div2 || '.' || line_in_div, 'unknown'
FROM line_mapping
WHERE work_abbrev = 'E3'
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
- **Delius (1868):** Nikolaus Delius, "Über Shakespeare's Pericles, Prince of Tyre," *Jahrbuch der deutschen Shakespeare-Gesellschaft* 3 (1868).
- **Hoy (1962):** Cyrus Hoy, "The Shares of Fletcher and his Collaborators in the Beaumont and Fletcher Canon (VII)," *Studies in Bibliography* 15 (1962): 71-90.
- **Riverside-Smith (1974):** Hallet Smith, ed., *The Riverside Shakespeare* (Boston: Houghton Mifflin, 1974).
- **Brooke (1990):** Nicholas Brooke, ed., *Macbeth*, Oxford Shakespeare (Oxford University Press, 1990).
- **Vickers (2002):** Brian Vickers, *Shakespeare, Co-Author: A Historical Study of Five Collaborative Plays* (Oxford University Press, 2002).
- **Jowett (2004):** John Jowett, ed., *The Life of Timon of Athens*, Oxford Shakespeare (Oxford University Press, 2004).
- **Jowett (2007):** John Jowett, "Middleton and Measure for Measure," in *Thomas Middleton and Early Modern Textual Culture* (Oxford University Press, 2007).
- **Craig & Kinney (2009):** Hugh Craig & Arthur F. Kinney, *Shakespeare, Computers, and the Mystery of Authorship* (Cambridge University Press, 2009).
- **Hammond (2010):** Brean Hammond, ed., *Double Falsehood*, Arden Shakespeare (Methuen Drama, 2010).
- **Jowett (2013):** John Jowett, "Middleton and Macbeth," in *Thomas Middleton in Context* (Cambridge University Press, 2013).
- **Segarra et al. (2016):** Santiago Segarra et al., "Attributing the Authorship of the Henry VI Plays by Word Adjacency," *Shakespeare Quarterly* 67.2 (2016): 232-56.
- **Taylor & Loughnane (2016):** Gary Taylor & Rory Loughnane, "The Canon and Chronology of Shakespeare's Works," in *The New Oxford Shakespeare: Authorship Companion* (Oxford University Press, 2016).

## Known Limitations

- **H8 Act 3 Scene 2** is traditionally considered a mixed scene (Shakespeare/Fletcher transition mid-scene). The current attribution assigns the entire scene to Shakespeare. A sub-scene split would require identifying the transition line.
- **TNK Act 3 Scene 1** is sometimes considered partially Fletcher. The current attribution assigns it entirely to Shakespeare.
- **Mac Act 4 Scene 1** contains Middleton interpolations (Hecate speeches, ~10 lines) within a Shakespeare scene. Only scene 3.5 is attributed at scene granularity.
- **MM** attribution is the most controversial of the NOS claims. Only 1.2 is attributed; other Middleton contributions are scattered line-level insertions.
- **1H6** scenes not attributed to Nashe or Shakespeare may have additional anonymous collaborators (possibly Marlowe or Peele). Only the Nashe attribution (Act 1) has strong consensus.
- Only one attribution set per work exists. The picker supports multiple sets if scholars disagree on attribution boundaries.

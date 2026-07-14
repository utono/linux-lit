# Fold-aware stage-direction line mapping

**Date:** 2026-06-25
**Status:** Design approved

## Problem

A multi-line stage direction inside a reader-gloss passage renders UNCOLORED on
the main reading card (reported: 2H6-Amb 1.4 `[The Guard arrest Margery
Jourdain... / accomplices and seize their papers.]`), while surrounding glossed
dialogue and *single-line* stage directions (`[To Jourdain.]`) are tinted.

Root cause (confirmed by a runtime `RGLOSS_DIAG` log: the SD's buffer line is
`buffer_to_work = None`):

- `clean_file_lines` (`src/app/text_prep.rs:79-109`) FOLDS a multi-line stage
  direction — one that opens with `[` and whose first source line has no closing
  `]` — into a SINGLE buffer line, space-joined, so GTK soft-wraps it. Its comment
  claims "stage directions normalize to empty so folding doesn't disturb mapping."
- That comment is now FALSE. lit.db gained `line_mapping.sub_line` stage-direction
  rows, so this SD is TWO DB rows: `43.1` = `[The Guard arrest Margery Jourdain
  and her`, `43.2` = `accomplices and seize their papers.]`.
- `build_line_map`'s stage matcher (`src/text_file_map.rs:294-308`, `WholeLine`
  mode only — plays/verse) matches a stage buffer line to a DB row by RAW TRIMMED
  TEXT equality (`work_lines[wi].text.trim() == want`). The folded line equals
  NEITHER row, so it never matches → `buffer_to_work[buf] = None` →
  `apply_reader_gloss_highlighting`'s `None => continue` skips it (and so would
  every other id-based feature: `u`/`.` binds, bookmarks, concordance).

Single-line SDs end with `]` on their own line, are NOT folded, and match 1:1 —
hence they color. That is the discriminator.

Verified: the folded buffer text is BYTE-IDENTICAL to the DB rows 43.1+43.2
space-joined (`group_concat(canonical_text, ' ')`), because `clean_file_lines`
joins continuation lines with a single space (`joined.push(' ')`).

Scope note: the stage matcher with the `sub_line > 0` raw-text path exists ONLY in
`MatchMode::WholeLine` (plays/verse); prose (`ParagraphAccumulate`) has no such
path, so this bug is play-specific — matching the report.

## Goal

A folded multi-line stage direction maps to its DB stage row(s), so it carries a
`(div1, div2, line_in_div)` citation and is colored / addressable like any other
line. Single-line SDs and all other mapping behavior are unchanged.

## Fix — concatenate consecutive DB stage rows to match a folded SD

In the stage-direction branch of `build_line_map_mode`'s `WholeLine` arm
(`src/text_file_map.rs:294-308`), when no single DB row equals `want`:

1. Starting at the first unconsumed `sub_line > 0` DB row at/after `db_cursor`
   within the window, **accumulate consecutive `sub_line > 0` rows**, space-joining
   their `text.trim()` (mirroring `clean_file_lines`' single-space join), and
   compare the running join to `want` after each row.
2. On an exact match, map the folded buffer line to the **first** row of the run
   (`buffer_to_work[buf_idx] = Some(first_wi)`), set `work_to_buffer[wi] = buf_idx`
   for **every** row in the run (so a reverse lookup from any consumed DB row
   lands on the folded buffer line), advance `db_cursor` past the last consumed
   row, and `matched += 1`.
3. The existing single-row exact match stays as the FIRST attempt (fast path,
   covers single-line SDs and unfolded directions). The multi-row accumulation is
   the fallback only when the single-row match fails.

The accumulation must:
- only join CONSECUTIVE `sub_line > 0` rows (stop at the first `sub_line == 0` /
  spoken row, or when the join length exceeds `want`),
- bound itself to the existing `WINDOW` so a pathological work can't scan the
  whole DB,
- leave `db_cursor` exactly past the consumed run on success, and unchanged
  (falling through to the existing `continue`) on failure, so an unmatched folded
  line stays `None` exactly as today (no new mis-mapping).

### Why this is the right layer

`build_line_map` is the one place that owns the `.txt`/buffer ↔ DB binding. Fixing
it here makes EVERY folded multi-row SD in EVERY play map correctly, keeps the
nice soft-wrapped folded display, and needs no litdb reimport. The alternatives
(stop folding — display regression; reimport — heavy, fragile to the next split)
were rejected.

### Correct the stale comment

Update the `clean_file_lines` comment (`text_prep.rs:83`) — folding no longer
"doesn't disturb mapping"; it now requires the fold-aware matcher. And the stage
matcher's comment (`text_file_map.rs:291-293`) — the "byte-identical 1:1" claim is
only true for UNfolded SDs; document the multi-row join.

## Snapshot invalidation

A snapshot built by the OLD `build_line_map` cached the SD as unmapped. Its
`db_fingerprint` (hashes per-line id/div/line_in_div — unchanged) and `.txt` mtime
(unchanged) won't trigger invalidation, so stale `~/.cache/linux-lit/snapshots/
<abbrev>.text.bin` would keep the bug. **Bump `SNAPSHOT_VERSION`**
(`src/snapshot.rs:39`, currently 9 → 10) with a one-line comment noting the
fold-aware stage mapping, so `validate` rejects every old snapshot once and
rebuilds. The serialized SHAPE is unchanged (same `buffer_to_work`/`work_to_buffer`
Vec types), so only the version bump is needed — no serde format change.

## Testing

### Unit (pure, `cargo test --bins`)

Add to `src/text_file_map.rs` tests, using synthetic `Line`s (the existing
`make_line_div` helper) — no DB needed:

- **Folded multi-row SD maps to its rows.** Build a `WholeLine` map from a buffer
  containing one folded SD line (`"[A B C and D E.]"`) against work lines with two
  `sub_line > 0` rows (`"[A B C and"`, `"D E.]"`) plus surrounding dialogue.
  Assert `buffer_to_work[sd_buf] == Some(first_row_idx)` and that both DB rows'
  `work_to_buffer` point to `sd_buf`.
- **Single-line SD still maps 1:1** (regression: the fast path is unaffected).
- **Non-matching folded line stays None** (a folded `[ ... ]` whose join matches no
  DB run leaves `buffer_to_work = None`, `db_cursor` unmoved).
- **The real data path** (gated on lit.db, skip if absent): build the map through
  `clean_file_lines` + `build_line_map` for 2H6-Amb and assert the buffer line
  containing `"Guard arrest"` maps to `(1,4,43)` — the exact failure, reproduced
  through the SAME prep path the app uses (NOT raw `.txt`; that was the misleading
  earlier repro).

### Visual (user-run, per CLAUDE.md)

After the fix + version bump, reopen 2H6-Amb 1.4 (no manual cache clear needed —
the bump rebuilds it): `[The Guard arrest...]` should now be rose-tinted like the
surrounding glossed lines. Also confirm `u`/`.`/bookmark on that SD now work
(it has a citation).

## Files

- `src/text_file_map.rs` — fold-aware multi-row stage match in the `WholeLine`
  stage branch; comment fix; unit tests.
- `src/app/text_prep.rs` — correct the `clean_file_lines` fold comment.
- `src/snapshot.rs` — `SNAPSHOT_VERSION` 9 → 10 + comment.

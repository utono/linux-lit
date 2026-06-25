# Citation-monotonic playback sync + 2H6 timestamp repair

**Date:** 2026-06-25
**Status:** Design approved

## Problem

In `2H6-Amb` Act 1, Scene 4 the Spirit speaks a prophecy (lines 1.4.34–39,
audio ~2468–2493 s). York then re-reads the same prophecy paper aloud
(lines 1.4.67–73). In `lit.db` the **re-read lines were assigned the first
occurrence's timestamps** (2467–2493) instead of their true later audio
(~2620–2663 s, the gap between line 64 @ 2619.74 and line 74 @ 2663.9). Two
distinct lines therefore claim the same audio instant for both media rows
(`media_id` 88 and 275).

`find_line_for_time` (`src/mpv/client.rs`) maps an audio position to a line
purely by `start_time` using `partition_point`. When two lines share a
bracketing `start_time`, it can resolve to the **later** occurrence (the
re-read, ~50 work lines ahead), yanking the cursor forward while the Spirit is
still speaking. This is the premature navigation observed in the screenshots
(`Tell me what fate awaits the Duke of Suffolk?` and `Let him shun castles;`
highlighted out of sequence).

The only existing defense is the `ABERRANT dist>50` guard in
`src/main.rs`'s `CursorSync` handler — a coarse magic number that *skips* the
event rather than choosing the correct line.

## Goals

1. **Robust sync:** the cursor must move through the work's line ordering
   (= citation order; `LineMap.work_to_buffer` is monotonic in
   `(div1, div2, line_in_div)`) the way `j`/`k`/`q`/`g`/`G` do — never jumping
   to a distant duplicate-timestamp line. Robust even if bad timestamps exist.
2. **Repair the data:** remove the impossible timestamps on the 2H6-Amb
   re-read lines so they stop acting as false sync anchors.

Non-goals: re-measuring the re-read's true audio times (a later litdb pass);
`[`/`{` bookmark navigation (already monotonic, unaffected — out of scope).

## Part A — Citation-monotonic line selection (robust fix)

### Where

`find_line_for_time` in `src/mpv/client.rs`. Selection happens **inside the
mpv-client task** (option A1), which already owns the timestamp table. No new
channel message is needed.

### State

The mpv-client task gains a field `last_synced_work_idx: Option<usize>`,
updated every time it emits a `CursorSync(idx)` (the `idx` is already a work
index via `line_id_to_index`). This is the "where the cursor is, in citation
order" anchor.

### Selection rule (forward-only, nearest)

`find_line_for_time` currently picks a single `active` timestamp index by
`partition_point` + the gap-aware early-jump promotion, then maps it through
`line_id_to_index`. The change: when more than one timestamp entry brackets the
effective time (i.e. shares the resolved `start_time`, OR the gap-promotion is
ambiguous), build the **candidate set** of work indices and choose:

- the candidate with **minimum `|work_idx − last_synced_work_idx|`**,
- breaking ties **toward the forward candidate** (the larger index), so normal
  line-by-line progress always advances.

When `last_synced_work_idx` is `None` (first sync after load/seek), fall back to
the candidate nearest the timestamp's own position (current behavior — the
`partition_point` winner).

This selection:

- **Rejects the duplicate glitch:** the far re-read line is never "nearest" to
  the Spirit's line, so the cursor stays on the first occurrence.
- **Honors real backward seeks:** when the user seeks MPV back (`o`/`O`/Left or
  scrubbing), those keybinds already set 86400 s sync suppression and re-seek;
  on resume `last_synced_work_idx` may be stale, but "nearest in absolute
  work-index distance" lands the cursor where the audio actually is, because the
  near candidate beats the far one.

### Retire the magic guard

The `ABERRANT dist>50` check in `src/main.rs` is no longer load-bearing once
selection is citation-nearest. Keep a wide sanity clamp (e.g. reject a single
event that would move the cursor more than the whole work — defensive only),
but remove the `dist>50` early-`continue`. The principled choice replaces it.

### Tie/edge cases

- **No duplicates (normal case):** candidate set has one element; behavior is
  identical to today. Zero risk to existing works.
- **Gap-aware promotion** (`SYNC_GAP_PREROLL`) still applies; it operates on the
  chosen candidate, not before it.
- **Untimestamped next line:** unchanged — `pending_advance` in `main.rs` still
  carries the cursor across gaps.

## Part B — Repair 2H6-Amb data (B1: null the bad timestamps)

NULL out the impossible timestamps on the re-read lines for **both** media rows.
Affected `line_mapping_id`s (verified): 1175128, 1175130, 1175131, 1175132,
1175133, 1175134 (line_in_div 67, 69, 70, 71, 72, 73). Line 68 ("Suffolk?") and
65–66 (Latin) already have no timestamp.

Done as a small SQL migration in `~/utono/litdb`, **not** in linux-lit code:

```sql
DELETE FROM line_timestamps
WHERE line_mapping_id IN (1175128,1175130,1175131,1175132,1175133,1175134);
```

(Or set start/end NULL if a row must persist for another column — but these rows
exist only to hold the timestamp, so DELETE is clean.)

After repair, sync has no false anchor through the re-read; the lines display
normally and `pending_advance` carries the cursor across them. Part A makes sync
correct even if such data reappears, so B is belt-and-suspenders.

This must be applied to the live `~/utono/litdb/data/lit.db`. The snapshot cache
is `.txt`-mtime + db-fingerprint guarded, so it invalidates automatically on the
next launch.

## Testing

### Unit (pure logic, `cargo test --bins`)

In `src/mpv/client.rs` tests for `find_line_for_time`:

- **Duplicate-timestamp, cursor near first:** two line ids share `start=2484`,
  `last_synced_work_idx` near the first occurrence → asserts the **first**
  (nearer) work index is returned, not the far duplicate.
- **Backward seek:** `last_synced_work_idx` far ahead, audio time brackets an
  earlier line → asserts the near earlier candidate is chosen.
- **No-duplicate regression:** existing `test_find_line_for_time*` cases must
  still pass unchanged (single-candidate path).

### Visual (user-run, per CLAUDE.md "do not run the app")

Reproduction for the user to eyeball:

1. Open `2H6-Amb`, navigate to Act 1, Scene 4 (the conjuring scene).
2. Start playback (Tab) from before the Spirit's prophecy
   ("By the eternal God…", ~2427 s).
3. Watch the highlight cross the Spirit's prophecy (lines 34–39).
4. **Expected:** the cursor advances one line at a time through the Spirit's
   lines and does **not** jump forward to the re-read prophecy
   (`Tell me what fate awaits…` / `Let him shun castles;`).

## Files

- `src/mpv/client.rs` — `find_line_for_time` selection + `last_synced_work_idx`
  state + unit tests.
- `src/main.rs` — remove the `ABERRANT dist>50` early-continue; keep a wide
  sanity clamp.
- `~/utono/litdb` — SQL migration nulling the 6 re-read timestamps (separate
  repo, separate commit).

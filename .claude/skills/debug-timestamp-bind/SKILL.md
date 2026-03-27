---
name: debug-timestamp-bind
description: Use when the u (set start time) or . (set chapter) keybind fails silently — no timestamp written to lit.db, no sign column update. Accepts a screenshot argument showing the highlighted line.
---

# Debug Timestamp Bind

Diagnose why `u` (set_start_time) or `.` (set_chapter) failed to write to lit.db.

## Diagnostic Steps

Run these in order, stopping at the first failure found:

### 1. Read the log

```bash
cat ~/utono/linux-lit/linux-lit.log
```

Look for lines after the `KEY: name=u` or `KEY: name=period` entry:
- `TS: set_start_time failed: no media_id` — no media connected
- `TS: set_start_time failed: no work line for buffer line N` — line mapping failure
- `TS: set start_time=...` — success (problem is elsewhere)
- No TS line at all — key wasn't routed to the handler

### 2. If "no media_id"

Verify media is associated with the work in lit.db:

```sql
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT mf.id, mf.path FROM media_files mf
   JOIN work_media_associations wma ON wma.media_id = mf.id
   WHERE wma.work_abbrev = '<ABBREV>'"
```

If no rows, the work has no media association. If rows exist, check that MPV connected (look for `MPV: connected to` in the log).

### 3. If "no work line for buffer line N"

This means `buffer_to_work[N]` is `None` — the text file line didn't match any DB line during `build_line_map`.

**Identify the line from the screenshot.** Extract a distinctive phrase from the highlighted line.

**Check the line exists in lit.db:**

```sql
sqlite3 ~/utono/litdb/data/lit.db \
  "SELECT id, canonical_text, normalized_text FROM line_mapping
   WHERE work_abbrev = '<ABBREV>'
   AND canonical_text LIKE '%distinctive phrase%'"
```

**Compare normalizations.** The match fails when `normalize(text_file_line) != db_normalized_text`. Common causes:
- **Bracketed stage directions** in text file not in DB: `[To Fool.]`, `[Aside.]`, `[Exit.]`
- **Different punctuation or spelling** between text file edition and DB edition
- **Line splitting differences** — text file wraps differently than DB canonical_text

**Test the normalization:**

```bash
# In the running app's context, check what normalize() produces
cargo test -- text_file_map::tests --nocapture
```

Or add a temporary test case with the problematic line to verify the fix.

### 4. Fix the normalization

Fixes go in `src/text_file_map.rs` in the `normalize()` function or `strip_brackets()`. After fixing, rebuild and verify the LINEMAP match percentage improves:

```bash
cargo build
# Run the app, check log for: LINEMAP: matched X/Y work lines (Z%)
```

### 5. Verify the fix

After rebuilding, launch the app, navigate to the same line, press `u` or `.`, and check:
- Log shows `TS: set start_time=...` or `TS: set chapter start_time=...`
- Sign column dot appears on the line
- `sqlite3 ~/utono/litdb/data/lit.db "SELECT * FROM line_timestamps WHERE line_mapping_id = <ID>"` shows the row

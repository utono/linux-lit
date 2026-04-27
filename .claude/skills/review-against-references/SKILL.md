---
name: review-against-references
description: Use when reviewing a linux-lit subsystem against the reference codebases at ~/Documents/repos/linux-lit/ to surface bugs, missing edge cases, or design improvements drawn from how foliate, lue, bk, openreader, html5-audio-read-along, or transcript-tracer-js solve the same problem. Accepts a subsystem argument like "pagination", "audio-sync", "bookmarks", "selection-tools", "navigation", or "ingest".
argument-hint: <subsystem>
---

# Review Against References

Compare a linux-lit subsystem to the reference codebases at `~/Documents/repos/linux-lit/` and produce a focused list of **refactors that bring linux-lit's structure into alignment with the references** so future reference lookups translate more directly.

This is a **review skill**, not an implementation skill. It outputs a written review and stops. Implementation requires a separate explicit request.

## Goal: leverage, not just parity

The point of this review is not "find bugs the reference handles." It is: **make linux-lit shaped enough like the reference that next time you open the reference for guidance, the translation is mechanical instead of conceptual.**

That changes the orientation of every finding:

- Prefer recommending a **rename, extraction, or reshape** that mirrors the reference's vocabulary and module boundaries — even when linux-lit's current code is *correct*.
- A finding that says "linux-lit is correct but its function is named/shaped differently from foliate's `getVisibleRange`, so future cross-reference is harder" is **valid and welcome**.
- Bugs and edge cases still belong in the review, but frame the fix as "adopt the reference's shape so this class of bug becomes structurally hard to reintroduce," not "patch this one site."

If a finding cannot be phrased as "refactor linux-lit toward the reference's pattern," it probably belongs under "Out of scope" or in a different review.

## When to use

Invoke when the user asks to:

- "Review pagination against the references"
- "What can we learn from foliate about X"
- "Compare our audio sync to lue"
- "Find improvements for the bookmark schema"

Do not invoke for general bug debugging — use `superpowers:systematic-debugging` for that. This skill is for *comparative* review.

## Subsystem → reference map

The reference index lives in `~/utono/linux-lit/CLAUDE.md` under "Reference Codebases". Use this map; do not invent new pairings.

| Subsystem | Linux-lit source | Primary references | Secondary |
|---|---|---|---|
| pagination | `src/input/navigation.rs`, `src/app.rs` (display logic) | `foliate-js/paginator.js` | `bk/src/view.rs` |
| audio-sync | `src/mpv/`, sync handlers in `src/app.rs` | `lue/lue/timing_calculator.py`, `lue/lue/audio.py` | `html5-audio-read-along/read-along.js`, `openreader/src/hooks/audio/` |
| timestamps / sync data | `src/db/queries.rs` (timestamp tables), `src/mpv/` | `openreader/src/hooks/audio/`, `transcript-tracer-js/transcript-tracer.js` | `lue/lue/timing_calculator.py` |
| bookmarks / annotations | `src/db/queries.rs` (bookmarks table), `src/ui/bookmark_picker.rs` | `foliate/src/annotations.js` | `lue/lue/progress_manager.py` |
| location addressing | `src/db/line_types.rs`, `line_mapping.id` usage | `foliate-js/epubcfi.js` | — |
| selection-tools / vocab popup | `src/ui/vocab_popup.rs`, `src/concordance.rs` | `foliate/src/selection-tools.js`, `foliate/src/selection-tools/*.html` | — |
| navigation / keymap | `src/input/keymap.rs`, `src/input/navigation.rs` | `bk/src/main.rs`, `bk/src/view.rs` | `lue/lue/input_handler.py` |
| theming | `src/theme.rs` | `foliate/src/themes.js` | — |
| ingest / format support | `src/db/queries.rs`, `src/text_file_map.rs` | `lue/lue/content_parser.py`, `bk/src/epub.rs` | `foliate-js/epub.js` |

If the requested subsystem is not in the table, ask the user to pick one rather than guessing.

## Process

Work the steps in order. Do not skip ahead.

### 1. Confirm scope

- Confirm the subsystem name and the linux-lit files in scope. If ambiguous (e.g., "review the reader"), ask which subsystem before proceeding.
- Confirm the reference repos for that subsystem exist at `~/Documents/repos/linux-lit/`. If missing, stop and ask the user to re-clone.

### 2. Read the linux-lit side first

Read the linux-lit source files for the subsystem **end-to-end** before touching the references. You need a clear mental model of what linux-lit currently does, including its known quirks, before comparing.

Take notes on:

- **Algorithm shape** — the core control flow.
- **Data model** — DB tables, structs, enums involved.
- **Known edge cases** — search the file for `// fix`, `// HACK`, recent commit messages mentioning fixes in this area (`git log --oneline -- <files>`).
- **Open questions** — anything that looks suspicious or incomplete.

### 3. Read the reference side

For each primary reference, read the named file end-to-end. These files are deliberately small (most under 1500 lines). Do not grep first — read.

Take parallel notes on:

- **How the reference solves the same problem.**
- **Edge cases the reference handles** that linux-lit's notes did not mention.
- **Data model differences** (CFI vs `line_mapping.id`, JSON vs SQLite, etc.).
- **Patterns** (state machine shape, dispatch order, separation of concerns).

For secondary references, only read the relevant section, not the whole file.

### 4. Identify candidate findings

Every finding must propose a **concrete refactor that aligns linux-lit's shape with the reference's shape**. The finding type tells the reader what kind of leverage that alignment unlocks. A finding is one of:

- **Pattern alignment** — linux-lit and the reference solve the same problem differently. Recommend reshaping linux-lit toward the reference's pattern (function name, call graph, event vs. polling, single source of truth, etc.) so the reference becomes directly readable as guidance. State the rename/extraction/reshape, and what cross-reference it makes mechanical afterward.
- **Bug suspect** — the reference handles a case linux-lit appears to mishandle. State the reproducer hypothesis **and** the structural change (matching the reference's shape) that makes the bug class hard to reintroduce. Don't recommend a point patch.
- **Missing edge case** — the reference handles a case linux-lit doesn't appear to consider. State what would trigger it in linux-lit, and the refactor that closes the gap by adopting the reference's handling shape.
- **Schema gap** — the reference's data model carries information linux-lit's does not, and the gap blocks future leverage. State the schema change and which reference reads-across it enables.

Default to **pattern alignment** framing whenever the reference and linux-lit diverge in shape. Use the other types when the divergence is also load-bearing for a bug, edge case, or feature.

Do **not** include:

- Stylistic preferences with no leverage payoff (no future cross-reference enabled, no bug class closed).
- Features that would change linux-lit's identity (e.g., "foliate uses a WebView, you should too"). Substrate stays Rust + GTK4 + SQLite + MPV.
- Findings based on what the reference does *better in its own context* but doesn't translate (e.g., CSS multi-column tricks for a GTK TextView).
- Renames purely for taste — every alignment must name a future scenario where the matched vocabulary saves work.

### 5. Write the review

Write the review to `docs/reviews/YYYY-MM-DD-<subsystem>-vs-references.md` (use Brisbane date: `TZ='Australia/Brisbane' date +%Y-%m-%d`). Structure:

```markdown
# <Subsystem> Review vs Reference Codebases

**Date:** YYYY-MM-DD
**Linux-lit files reviewed:** <paths with line counts>
**References consulted:** <repo/file with line counts>

## Summary

<2-3 sentences: how linux-lit's shape compares to the reference's shape overall, the headline alignment win, and what cross-reference becomes mechanical after these refactors.>

## Findings

### F1. <Short title> [pattern-alignment | bug-suspect | missing-edge-case | schema-gap]

**Reference shape:** `<repo>/<file>:<line range>` — <what the reference does, named in the reference's own vocabulary (function names, event names, module boundaries)>

**Linux-lit shape:** `<file>:<line range>` — <what linux-lit currently does, named in linux-lit's vocabulary>

**Refactor toward reference:** <the concrete rename/extraction/reshape — e.g., "extract `visible_range(top) -> VisibleRange { last_fit, height, count }` mirroring `paginator.js#getVisibleRange`; replace the four ad-hoc summing loops with calls to it">

**Leverage unlocked:** <what becomes mechanical afterward — e.g., "future foliate paginator reads translate line-for-line; descender / speaker-trim fixes land in one place">

**Risk if ignored:** <concrete consequence — bug class that stays open, or future cross-reference work that stays manual>

**Effort:** S | M | L (rough; S = under an hour, M = a day, L = multi-day)

---

### F2. ...

## Out of scope

<Things you noticed but excluded, with one-line reason each. Keeps future-you from re-investigating them.>

## Suggested next step

<One concrete next action: implement F1, gather more data on F3, etc. Do not implement here.>
```

Each finding must name a specific reference file and line range, and must propose a refactor that brings linux-lit's shape closer to the reference's shape. "foliate handles this better" with no path is not a finding. "Linux-lit is correct but its function should be named/shaped like the reference's so future cross-reference is mechanical" *is* a valid finding — record it.

### 6. Cap the review

- **Findings cap:** 10. If you have more than 10, pick the highest-leverage 10 and list the rest under "Out of scope" with a one-line reason. Order findings by **leverage unlocked** (how much future cross-reference work each refactor makes mechanical), with risk as a secondary sort (open bug class > silent edge case > pure structural alignment) and effort as the tiebreak (S before L).
- **Length cap:** 1200 words for the body (Findings section). Stay tight per finding even with more headroom — concision is still the point.

### 7. Stop

Output to the user:

> "Review written to `<path>`. <N> findings, <bug-suspect-count> bug suspects, <design-improvement-count> design improvements. Want me to dig deeper on any specific finding, or write an implementation plan for one?"

Do **not** start fixing anything. Do not invoke `writing-plans` or `superpowers:writing-plans` automatically. Wait for the user.

## Anti-patterns

- **Reading the reference first.** You'll anchor on the reference's design and frame linux-lit as deficient. Read linux-lit first.
- **Generic "consider adopting X" findings.** Every finding must be tied to a specific file and line range in both sides, and must propose a concrete refactor (rename, extraction, reshape).
- **Patch-level fixes for bug findings.** If the reference's shape would have prevented the bug class, recommend the structural alignment, not the point patch. A finding that says "fix this if-branch" without "...by adopting `<reference function/pattern>`" is half-done.
- **Recommending the reference's stack.** linux-lit is Rust + GTK4 + SQLite + MPV. Don't recommend WebViews, JSON-per-book, curses TUIs, or whisper.cpp because the reference uses them. Translate the *vocabulary, algorithm, or schema*, not the substrate.
- **Renames without leverage.** Don't rename functions just because the reference's name reads better. Every alignment must name a specific future cross-reference scenario it makes mechanical.
- **Bundling unrelated subsystems.** One review = one subsystem. If audio-sync review surfaces a bookmarks issue, note it under "Out of scope" and let the user request a separate review.
- **Implementing during review.** The output is a markdown file. Tools used: Read, Bash (for `git log`/`wc`), Write. No Edit on linux-lit source. No `cargo build`.

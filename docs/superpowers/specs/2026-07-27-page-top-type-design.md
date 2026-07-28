# `PageTop`: make an unpaired page position unrepresentable — design

_2026-07-27. Status: spec for review. NOT yet implemented._

## Why

Five pagination bugs shipped and were fixed in a single day (2026-07-27). Every
one was the same mistake: **code set the page's LINE and forgot its row
OFFSET**, or computed a page geometrically instead of reading the pinned table.

- journal-picker Escape landed on the cursor line, not `(42, 603)`
- startup resume recomputed `current_line - 1`, off-grid by construction
- cross-work jump landing forced `page_top_line = buf_idx`, no offset
- centering landing used `current_line - lpp/2`
- `hide_translations` never assigned `page_top_offset` at all

The troubleshooting ledgers now run to 3,037 lines across two files, with 21
numbered failure modes in the clip checklist alone. That is not a documentation
problem; it is the architecture asking to be changed.

## Root cause, precisely

`AppState` declares the page position as TWO independent public fields, six
lines apart:

```rust
pub page_top_line: usize,     // mod.rs:286
pub page_top_offset: i32,     // mod.rs:292
```

They must always change together — a prose page top is `(line, row-offset px)`
and the offset is load-bearing (603px in the reported bug). Nothing enforces
that. 29 sites assign `page_top_line`; only 12 set the offset within two lines.
**The remaining ~17 are the standing bug surface**, and each new call site is a
fresh chance to reintroduce the same defect.

This is a make-illegal-states-unrepresentable problem. The fix is a type.

## Design

### The type

```rust
/// A page position: the buffer line the page starts at, plus the pixel offset
/// INTO that line's first display row. The offset is non-zero only on a pinned
/// prose grid, where a page may begin mid-paragraph. The two halves are
/// meaningless apart — that is why they are one value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PageTop {
    line: usize,
    offset: i32,
}

impl PageTop {
    /// A page starting at the top of `line`. The ONLY way to build a PageTop
    /// without an offset — named so that dropping an offset is a visible,
    /// deliberate act at the call site rather than an omission.
    pub fn at_line_start(line: usize) -> Self { Self { line, offset: 0 } }

    /// A page position read from a pinned table (or otherwise known-complete).
    pub fn new(line: usize, offset: i32) -> Self { Self { line, offset } }

    pub fn line(self) -> usize { self.line }
    pub fn offset(self) -> i32 { self.offset }
}
```

Fields PRIVATE. `AppState` holds `pub page_top: PageTop` in place of the two
fields.

The win is not the struct — it is that `set_page_instant(state, top)` can no
longer be handed a bare line. The journal-Escape bug becomes a compile error.

### Why `at_line_start` rather than `From<usize>`

An implicit conversion would silently restore exactly the bug being removed. A
named constructor keeps every offset-dropping site greppable:

```
rg "at_line_start" src/
```

That grep is the permanent audit the ledgers currently do by prose.

### Scope of change (measured, not estimated)

- 29 real assignments to `page_top_line`; 12 already paired (mechanical),
  ~17 need a decision about the offset (the risk surface).
- `scroll.rs` holds 11 of the assignments behind its own 35-function API —
  internal plumbing, not scattered callers.
- `page_back_stack: Vec<(usize, i32)>` is ALREADY the pair as an anonymous
  tuple. It becomes `Vec<PageTop>` — strictly better, no semantic change.
- **Nothing is serialized.** `rg` over `snapshot.rs` and `config.rs` finds no
  `page_top_*`, so there is NO on-disk compatibility problem and no snapshot
  version bump.

### Deliberately NOT in this change

- Unifying `page_table.rs` / `prose_pages.rs` behind one trait (step 3).
- The `Pinned` vs `Estimated` resolver return type (step 2).
- `is_line_fully_visible`'s prose-table blindness (still latent, documented).

Each is its own branch. This spec is step 1 only, and it is the one that
delivers most of the value.

## Migration plan

Compiler-guided: make the fields private, then fix every error the compiler
reports. It cannot miss a site.

1. Add `PageTop` with tests. No behaviour change.
2. Replace the two `AppState` fields with `pub page_top: PageTop`. Everything
   breaks; the error list IS the work list.
3. Fix mechanically, file by file, smallest first: `translations.rs` (3),
   `highlight.rs` (2), `navigation.rs` (3), `prose_pages.rs` (4),
   `app/mod.rs` (12), `scroll.rs` (12).
4. At each site that would use `at_line_start`, ASK whether a pinned table is
   active there. If yes, it is a latent instance of today's bug — fix it and
   note it in the ledger rather than preserving it.
5. `page_back_stack` → `Vec<PageTop>`.

Step 4 is the point of the exercise. Steps 1–3 are typing.

## Testing

Per CLAUDE.md this is pagination, so the bar is behavioural, not just green.

- **Unit:** `PageTop` construction/accessors; `at_line_start` yields offset 0.
- **Invariant:** the refactor must be BEHAVIOUR-PRESERVING except where step 4
  finds a real bug. The gate is the nav-fuzz on BOTH engines:
  - `run-fuzz.sh --start-work BH-Barrett` (prose, pinned grid)
  - `run-fuzz.sh --start-work <a play>` (two-column, play table)
- **Regression:** the existing suite (1209 tests) must stay green.
- **A/B:** for any site changed under step 4, prove it with the same
  before/after log comparison used for the landing fixes
  (`BOTTOM_CLIP_EXACT` vs `BOTTOM_CLIP_ROWFILL`, `PAINT: first frame`).
- **On-screen:** headless cage run on real production geometry
  (`1920x1236` → `text_view.height = 1098`), plus a real-renderer check by the
  user, since cage is software rendering and can disagree on layout.

## Risk

**High, and the history says so.** The ledger records a previous pagination
refactor causing 169 test failures and a 1-line `JumpEnd` page. `scroll.rs` and
`app/mod.rs` are the hottest paths in the app.

What makes this different from that attempt:

- It is **type-driven and compiler-checked**, not a logic rewrite. The compiler
  enumerates the call sites; nothing is found by reading.
- The pinned tables now EXIST as a real source of truth. The earlier regressions
  happened while pagination was still inferring boundaries from text.
- `canonical_page_top_offset_for` already exists as the chokepoint this
  formalizes — the shape is proven, this makes it mandatory.
- Behaviour is meant to be UNCHANGED, so the fuzz is a true oracle: any
  behavioural diff is a bug in the refactor, not an expected consequence.

Mitigations: one branch in a worktree, staged commits per file, fuzz on both
engines before merge, and step 4 findings called out individually rather than
folded in silently.

**Abort criterion:** if the fuzz shows a behavioural diff that is not a step-4
finding and is not understood within one session, revert the branch rather than
patch forward. The current code is correct today; a half-migrated state is not
worth shipping.

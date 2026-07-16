# Audit-opportunity ledger entry format

Each opportunity in `docs/audit-opportunities.md` is one entry.
Copy this shape. Keep numbering monotonic; never reuse a merged number.

The `## #N — ` and `- **#N** ` prefixes below, and the `## Lessons` /
`Standing exclusions` / `Below the floor` headings, are what the skill's step-1
`rg` index pattern matches. Changing them means updating that pattern too, or
the audit reads a partial index and reuses a merged number.

```markdown
## #N — <short-name> — STATUS

- **Status:** OPEN | SPEC | PLAN | DONE (commit <sha>)
- **Signal:** <duplication count + where, e.g. "13 ListBox pickers repeat an
  identical move_selection tail">
- **Identical part (extracts):** <the byte-identical code that becomes the helper>
- **Variants (stay at call sites):** A — <…>; B — <…>; C — <…>
- **EXCLUDED:** <file> (<why — structurally different>); <file> (<why>)
- **Safe-scope:** yes — behavior-preserving <kind: widget-construction / tail /
  literal> extraction, zero behavior change.
- **Rank inputs:** copies=<n>, drift_risk=<low/med/high>, scope=<tiny/small/med>
```

**When it ships, prune it to the one-line form** (the full block above moves
verbatim to `docs/audit-opportunities-archive.md`):

```markdown
- **#N** <short-name> (<sha>) — <one sentence: what became what, and where>.
```

The header of the ledger file itself:

```markdown
# linux-lit audit opportunities

Numbered, safe-scope, behavior-preserving refactoring opportunities. Produced by
the `assess-maintainability` skill; consumed by the spec→plan→refactor→merge
pipeline. DONE entries stay for numbering continuity — never reuse a number.

Larger, behavior-CHANGING projects (god-struct split, app.rs module carve-up)
are tracked separately at the bottom under "## Larger projects (not safe-scope)"
— they are NOT numbered opportunities.
```

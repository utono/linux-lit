# LitReader UI review

Fill this in **after opening every screenshot in `target/ui/`** — including the
`*_annotated.png` overlays and the `*.suspects.txt` lists. The Stop hook will
not let the turn finish until this references each screenshot and addresses
each suspect by name.

## Run

- Commit / build: <!-- sha or branch -->
- Screenshots reviewed: <!-- list every <name>.png -->

## Per-screenshot observations

### <name>.png
- Window title bar reads: <!-- quote the visible text -->
- Panels / regions visible: <!-- e.g. library sidebar, reading pane, annotation gutter -->
- On-screen text legible and not clipped? <!-- yes/no + detail -->
- Anything visually wrong (overlap, blank area, misaligned margin, wrong colors)?

<!-- repeat per screenshot -->

## Suspects (from *.suspects.txt)

For each flagged widget, say whether the pixels confirm a real bug or it's a
false positive, and why:

- `<role:name>` — <!-- real bug / false positive + reasoning -->

## Verdict

- [ ] UI renders as intended
- [ ] Issues found (list below, file follow-ups)

Notes:

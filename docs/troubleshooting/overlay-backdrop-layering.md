# Overlay backdrop layering — failure modes

Frequency-ordered ledger for "I can see the surface UNDERNEATH an overlay
showing through it". Read this BEFORE changing any overlay's draw code or
visibility toggles.

The recurring shape: an overlay is drawn ON TOP of a surface that was never
hidden, and whatever mattes it out is either translucent or missing. The fix
is never "make the card bigger" — it is to identify what provides the
matting for that specific surface.

## Know which of the two overlay mechanisms you are looking at

linux-lit has TWO unrelated ways of drawing a full-screen overlay, and they
fail differently. Establish which one you have BEFORE debugging.

- **Widget overlays (card + scrim pair).** A `container` GtkBox (the card) plus
  a separate `scrim` GtkBox behind it, both added via `overlay.add_overlay`.
  The gloss/synopsis/echoes overlay (`GlossOverlay`), the journal overlay
  (`JournalOverlay`), and the five per-overlay Ctrl+/ legends
  (`keybinds_legend::KeybindsLegend`) are all this kind.
- **Cairo overlays (one full-bleed DrawingArea).** A single `DrawingArea`
  painting everything — backdrop included — in a `set_draw_func`. The main-card
  Ctrl+/ overlay (`ui/keybinds_overlay.rs`) and the gamepad overlay
  (`ui/gamepad_overlay.rs`) are this kind. They have **no scrim widget at all**,
  so the backdrop they paint IS their matting.

Grep test: if the file has a `scrim` field it is the first kind; if it has
`set_draw_func` and a "full-screen scrim/backdrop" fill it is the second.

## 1. A Cairo overlay's backdrop fill is not fully opaque (fixed 2026-08-01)

**Tell.** The reading card's text ghosts faintly through an overlay that has no
business showing it — visible as speaker names and stray words floating behind
the keycaps. Easy to dismiss as a rendering artifact or a theme quirk.

**Root cause.** `draw_row_screen` in `ui/keybinds_overlay.rs` filled the
backdrop with `set_source_rgba(0.341, 0.322, 0.475, 0.95)`. That trailing
`0.95` is a 5% transparency, and the reader's main card is directly beneath
(no overlay ever hides it — see #3), so 5% of the reading card composited
through. The alpha was almost certainly meant as a "scrim" dimming effect,
which is wrong for a surface that is its own matting.

**Fix.** `set_source_rgb(0.341, 0.322, 0.475)` — fully opaque, no alpha.

**Confirm by PIXEL MEASUREMENT, never by eye.** A 5% bleed is faint enough to
argue about in a screenshot pasted into chat. Sample a band of the backdrop
where text used to ghost and count distinct colors: an opaque fill gives
exactly ONE color, any bleed gives several.

```bash
python3 -c "
from PIL import Image
im = Image.open('/tmp/shot.png').convert('RGB'); w,h = im.size
cols = {}
for y in range(500, 700, 10):
    for x in range(60, w-60, 20):
        p = im.getpixel((x,y)); cols[p] = cols.get(p,0)+1
print('unique colors:', len(cols))   # 1 == opaque, >1 == bleed
"
```

**Sibling not yet fixed.** `ui/gamepad_overlay.rs` has the identical defect at
`const BG: (f64,f64,f64,f64) = (0.341, 0.322, 0.475, 0.95)`. It was left alone
because that overlay is currently unreachable (its `spawn`/`dispatch` are dead
code per the build warnings). If the gamepad overlay is ever revived, drop the
alpha there too.

## 1b. A dimming wash left behind after the thing it dimmed is gone (fixed 2026-08-01)

**Tell.** "The Ctrl+/ bind is changing the root color." The backdrop behind a
legend is a DARKER shade of the reader's root — same hue, visibly deeper. Easy
to misread as a theme bug or as the legend picking a different color.

**Root cause.** `.legend-scrim` was `rgba(0, 0, 0, 0.3)` — a translucent BLACK
WASH, not a root-colored fill. That was correct when the parent overlay stayed
visible behind the legend and wanted dimming. Once the parent card is hidden
(#2, `suspend_for_legend`), the only thing left under the wash is the parent's
opaque root-colored scrim, so the 30% black simply darkened the root:
`(65,123,159)` reader root → `(46,87,109)` under the legend.

**Fix.** `.legend-scrim` → `background-color: transparent`. The layer is kept
rather than deleted; it is still the legend's full-bleed hit-test area.

**General lesson.** When you hide the surface a translucent overlay was
dimming, the wash does not become a no-op — it starts dimming whatever is now
behind it. Audit every alpha layer in the stack after changing what is visible
beneath it.

**Confirm by comparing the ROOT, not the card.** Sample points outside every
card in BOTH states; they must be byte-identical:

```bash
python3 -c "
from PIL import Image
a=Image.open('/tmp/before.png').convert('RGB'); b=Image.open('/tmp/after.png').convert('RGB')
pts=[(8,8),(20,300),(8,700),(1270,700)]
print('MATCH' if all(a.getpixel(p)==b.getpixel(p) for p in pts) else 'MISMATCH')
"
```

## 2. Hiding the scrim along with the card (widget overlays)

**Tell.** You hide a parent overlay so something else can float alone, and the
READING CARD appears behind it instead of the plain root background.

**Root cause.** `scrim_bg` (`theme.rs`) is the live `root_color` **verbatim and
opaque** — it is not a dimming veil, it is the matting that stands in for the
root. Hiding `scrim` alongside `container` therefore does not reveal "the root",
it reveals the reader's main card, which no overlay ever hides (#3).

**Fix.** Hide ONLY `container`; leave `scrim` visible. This is what
`suspend_for_legend` / `restore_after_legend` on `GlossOverlay` and
`JournalOverlay` do.

## 3. Assuming an overlay hides the reader's main card

It does not. `state.text_view` / `state.scrolled_window` stay `visible = true`
for the entire life of every overlay; they are only OCCLUDED by the opaque
scrim. So any hole in the matting — a translucent fill (#1), a hidden scrim
(#2) — shows the reading card, not the root color.

(The one `scrolled_window.set_visible(false)` pair in the tree, at
`app/mod.rs` + `input/highlight.rs`, is work-switch flash prevention and has
nothing to do with overlay layering.)

## 4. Do not reach for `hide()` to temporarily drop an overlay

`GlossOverlay::hide` and `JournalOverlay::hide` are **close funnels**, not
visibility toggles: they stop the loading spinner, reset the ask card via
`ask_host.card().close()`, clear the focus dim, and drop the word underline.
Calling one for a temporary hop (e.g. showing a legend over it) silently
discards session state the user expects to return to.

Use the non-destructive `suspend_for_legend` / `restore_after_legend` pair
instead. `ChatPanel::show`/`hide` are safe as-is — they only flip
`container` visibility and the panel has no scrim (it reflows the layout
rather than matting anything).

**Regression check.** Screenshot before opening, and after closing, the
temporary surface; the two PNGs must be **byte-identical**:

```bash
cmp -s /tmp/before.png /tmp/after.png && echo IDENTICAL || echo STATE LOST
```

## 5. Verifying headlessly

Land directly on the surface — do not try to escape into it:

```bash
./scripts/land-on.sh Ham 1.1 gloss     # or: journal, synopsis, or omit for reader
```

Traps that cost real runs here:

- **A running instance predates your rebuild.** A cage launched before
  `cargo build` still runs the OLD binary, so a fix "does not work". The
  pixel measurement in #1 read 3 distinct colors against a stale instance and
  1 against the rebuilt one, from the same source tree. Relaunch, then measure.
- **The first `wtype` chord after mapping is dropped.** Send `Ctrl+/` twice and
  check the log for `KEY: name=slash … mode=<Expected>` before trusting a
  capture. If two chords both land, the second CLOSED the overlay again.
- **`Ctrl+j` opens the journal PICKER, not the journal overlay.** Press Return
  to select an entry and reach `mode=JournalOverlay`.
- **`pkill -f "cage -- ./target/debug/linux-lit"` also matches the agent's own
  shell** when run in the same command as other work (exit 144 kills the
  pipeline). Run it as its own final step, then confirm with
  `pgrep -f "cage -- \./target/debug/linux-lit" | wc -l`.

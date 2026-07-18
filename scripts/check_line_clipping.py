#!/usr/bin/env python3
"""Detect top/bottom line clipping in LitReader's rendered text pane.

The core invariant: the first and last *visible* text lines must be shown
whole, never cut by the top or bottom edge of the reading pane. Generic extent
checks miss this — a half-clipped line still has nonzero, on-screen extents —
so this measures vertical completeness directly.

Method:
  1. Locate the text pane via AT-SPI (largest accessible exposing the Text
     interface), to get the exact clip boundaries; or use --region / full image.
  2. Crop the screenshot to it and build a row-wise ink projection
     (ink = pixels deviating from the background level, so it's theme-agnostic).
  3. Group ink rows into glyph rows. A glyph row adjacent to a pane edge that
     is shorter than the median row height — or that touches the edge with no
     background margin — is a clipped line.

Writes <shot>.clip.json (measurements) and <shot>_clip.png (annotated bands),
prints PASS/FAIL, and exits:
  0 = no clipping   1 = clipping detected   2 = could not evaluate (fail closed)
"""

import argparse
import json
import sys


def die_uneval(msg):
    sys.stderr.write(f"check_line_clipping: cannot evaluate: {msg}\n")
    sys.exit(2)


try:
    import numpy as np
    from PIL import Image, ImageDraw
except Exception as e:  # noqa: BLE001
    die_uneval(f"missing dep ({e})")


def find_text_region(app_name, timeout):
    try:
        import pyatspi
    except Exception as e:  # noqa: BLE001
        return None, f"pyatspi unavailable ({e})"
    import time

    def largest_text(acc, best):
        try:
            acc.queryText()
            x, y, w, h = acc.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            if w > 0 and h > 0 and (best is None or w * h > best[2] * best[3]):
                best = (int(x), int(y), int(w), int(h))
        except NotImplementedError:
            pass
        for c in acc:
            if c is not None:
                best = largest_text(c, best)
        return best

    deadline = time.time() + timeout
    while time.time() < deadline:
        for app in pyatspi.Registry.getDesktop(0):
            if app and app.name == app_name:
                region = largest_text(app, None)
                if region:
                    return region, None
        time.sleep(0.25)
    return None, f"no Text-interface widget for app '{app_name}'"


DEFAULT_HEIGHT_FRAC = 0.7
DEFAULT_MIN_MARGIN = 1
# A short EDGE row counts as clipped only if it also sits within this many
# background rows of the pane edge. A genuinely clipped line is short *because
# the edge cut it*, so it is flush against the edge; a thin descender tip split
# off by the ink projection can land far above the edge (35px in a real case)
# and must not read as a clip. Sized well above descender-tail gaps (<=3px) yet
# far below a whole body row (~22px), so a truly clipped half-row still trips.
DEFAULT_EDGE_TOL = 8


def decide_clip(first_h, last_h, median_h, min_interior, top_margin,
                bottom_margin, region_h, height_frac=DEFAULT_HEIGHT_FRAC,
                min_margin=DEFAULT_MIN_MARGIN, edge_tol=DEFAULT_EDGE_TOL):
    """Pure top/bottom clip decision from row geometry. Returns a dict with
    `clip_top` / `clip_bottom` (and the `short_*` intermediates, for tests).

    A short edge row is clipped only if it is (a) shorter than the body median
    AND every interior row (so a complete-but-small row like a 0.75-scale
    speaker label is not flagged), AND (b) near the pane edge (so a detached
    descender tip far from the edge is not flagged). A row flush against the
    edge with no background margin is always a clip regardless of height.
    """
    short_top = (
        first_h < height_frac * median_h
        and first_h < min_interior
        and top_margin <= edge_tol
    )
    short_bottom = (
        last_h < height_frac * median_h
        and last_h < min_interior
        and bottom_margin <= edge_tol
    )
    clip_top = short_top or (top_margin < min_margin)
    clip_bottom = short_bottom or (bottom_margin < min_margin)
    return {
        "short_top": short_top,
        "short_bottom": short_bottom,
        "clip_top": clip_top,
        "clip_bottom": clip_bottom,
    }


def glyph_runs(rows_with_ink, merge_gap=2):
    """Group inked rows into glyph rows, merging runs separated by <= merge_gap
    blank rows. A descender tip ('y'/'g'/comma tails) tapers so thin that its
    connecting rows can dip under the 1%-width ink threshold, splitting the tip
    off as a detached 1px "row" — which then reads as a clipped last line (a
    false positive first hit when the paged clip's descender allowance exposed
    the true tips it used to cover). Genuine inter-line gaps are >= ~4px at any
    reading size, so merging <= 2px gaps cannot fuse two real lines or hide a
    real next-line sliver."""
    runs, start = [], None
    for i, v in enumerate(rows_with_ink):
        if v and start is None:
            start = i
        elif not v and start is not None:
            runs.append((start, i - 1))
            start = None
    if start is not None:
        runs.append((start, len(rows_with_ink) - 1))
    merged = []
    for run in runs:
        if merged and run[0] - merged[-1][1] - 1 <= merge_gap:
            merged[-1] = (merged[-1][0], run[1])
        else:
            merged.append(run)
    return merged


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--shot", required=True)
    p.add_argument("--app", default=None, help="accessible app name (for region detection)")
    p.add_argument("--region", default=None, help="explicit x,y,w,h instead of AT-SPI")
    p.add_argument("--delta", type=int, default=35, help="ink threshold vs background")
    p.add_argument("--height-frac", type=float, default=DEFAULT_HEIGHT_FRAC,
                   help="edge row shorter than this fraction of median = clipped")
    p.add_argument("--min-margin", type=int, default=DEFAULT_MIN_MARGIN,
                   help="min background rows expected before first / after last line")
    p.add_argument("--edge-tol", type=int, default=DEFAULT_EDGE_TOL,
                   help="a short edge row is a clip only within this many rows of the edge "
                        "(guards against detached descender tips far from the edge)")
    p.add_argument("--timeout", type=float, default=5.0)
    args = p.parse_args()

    try:
        img = Image.open(args.shot).convert("RGB")
    except Exception as e:  # noqa: BLE001
        die_uneval(f"cannot open {args.shot} ({e})")

    if args.region:
        rx, ry, rw, rh = (int(v) for v in args.region.split(","))
        region_src = "explicit"
    elif args.app:
        region, err = find_text_region(args.app, args.timeout)
        if region is None:
            die_uneval(err)
        rx, ry, rw, rh = region
        region_src = "atspi"
    else:
        rx, ry, rw, rh = 0, 0, img.width, img.height
        region_src = "fullimage"

    # clamp to image bounds
    rx, ry = max(0, rx), max(0, ry)
    rw, rh = min(rw, img.width - rx), min(rh, img.height - ry)
    if rw <= 0 or rh <= 0:
        die_uneval("text region has no area on the screenshot")

    g = np.asarray(img.convert("L"), dtype=np.int16)
    crop = g[ry:ry + rh, rx:rx + rw]
    bg = int(np.median(crop))
    ink = np.abs(crop - bg) > args.delta
    row_ink = ink.sum(axis=1)
    rows_with_ink = row_ink > max(1, int(0.01 * rw))

    runs = glyph_runs(rows_with_ink)
    if not runs:
        die_uneval("no text rows found in region (wrong region or blank pane?)")

    heights = [b - a + 1 for a, b in runs]
    interior = heights[1:-1] if len(heights) >= 3 else heights
    median_h = float(np.median(interior))
    top_margin = runs[0][0]
    bottom_margin = (rh - 1) - runs[-1][1]
    first_h, last_h = heights[0], heights[-1]

    # A short EDGE row is clipped only if it is also shorter than every
    # interior row: a complete-but-small row (a 0.75-scale speaker label at
    # the page top) is legitimately under the body-text median, while a real
    # top/bottom slice is shorter than anything else on the page.
    min_interior = float(min(interior))
    verdict = decide_clip(
        first_h=first_h, last_h=last_h, median_h=median_h,
        min_interior=min_interior, top_margin=top_margin,
        bottom_margin=bottom_margin, region_h=rh,
        height_frac=args.height_frac, min_margin=args.min_margin,
        edge_tol=args.edge_tol,
    )
    clip_top = verdict["clip_top"]
    clip_bottom = verdict["clip_bottom"]

    # single-line panes can't establish a median; report low confidence.
    low_conf = len(heights) < 2

    result = {
        "shot": args.shot,
        "region": [rx, ry, rw, rh],
        "region_source": region_src,
        "glyph_rows": len(runs),
        "median_row_height": median_h,
        "first_row_height": first_h,
        "last_row_height": last_h,
        "top_margin_px": top_margin,
        "bottom_margin_px": bottom_margin,
        "clip_top": bool(clip_top),
        "clip_bottom": bool(clip_bottom),
        "low_confidence": low_conf,
    }
    base = args.shot.rsplit(".", 1)[0]
    with open(f"{base}.clip.json", "w") as f:
        json.dump(result, f, indent=2)

    # annotate
    draw = ImageDraw.Draw(img)
    draw.rectangle([rx, ry, rx + rw - 1, ry + rh - 1], outline=(40, 90, 200), width=1)
    fy = ry + runs[0][0]
    ly = ry + runs[-1][1]
    top_color = (220, 30, 30) if clip_top else (30, 160, 60)
    bot_color = (220, 30, 30) if clip_bottom else (30, 160, 60)
    draw.line([rx, fy, rx + rw, fy], fill=top_color, width=2)
    draw.line([rx, ly, rx + rw, ly], fill=bot_color, width=2)
    draw.text((rx + 3, fy + 2), f"first row h={first_h} (median {median_h:.0f})", fill=top_color)
    draw.text((rx + 3, max(ry, ly - 14)), f"last row h={last_h}", fill=bot_color)
    img.save(f"{base}_clip.png")

    if clip_top or clip_bottom:
        where = []
        if clip_top:
            where.append(f"TOP (first row h={first_h} vs median {median_h:.0f}, margin {top_margin}px)")
        if clip_bottom:
            where.append(f"BOTTOM (last row h={last_h} vs median {median_h:.0f}, margin {bottom_margin}px)")
        print("FAIL: line clipping detected — " + "; ".join(where))
        print(f"see {base}_clip.png")
        sys.exit(1)

    note = " (low confidence: <2 rows)" if low_conf else ""
    print(f"PASS: {len(runs)} rows, no top/bottom clipping{note}")
    sys.exit(0)


if __name__ == "__main__":
    main()

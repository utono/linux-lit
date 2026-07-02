#!/usr/bin/env python3
"""Fail if text ink appears outside an allowed horizontal band of a region.

Guard for the "TextTag left-margin replaces the view margin" bug class, where
a styled block (list item, block quote) escapes the overlay's centered column
and renders at the view's far-left edge, outside the inset panel.

Usage:
    check_ink_outside.py --shot page.png --region x,y,w,h --band x0,x1 \
        [--min-fill 0.0] [--max-outside-cols 2]

--region  the overlay's scrolled-viewport rect (TEST_JOURNAL_VIEWPORT_RECT)
--band    absolute x range text ink may occupy (TEST_JOURNAL_CONTENT_BAND)
--min-fill  optional: fail if the ink's bottom row is above this fraction of
            the region height (catches gross page UNDERFILL on non-last pages)

Ink = pixels notably darker than the region's median background (light themes;
the e2e harness runs the default light theme). Fails closed on missing deps.
"""

import argparse
import sys

import numpy as np
from PIL import Image


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--shot", required=True)
    ap.add_argument("--region", required=True, help="x,y,w,h")
    ap.add_argument("--band", required=True, help="x0,x1 (absolute px)")
    ap.add_argument("--min-fill", type=float, default=0.0)
    ap.add_argument(
        "--max-outside-cols",
        type=int,
        default=2,
        help="tolerated stray ink columns (antialiasing slop)",
    )
    a = ap.parse_args()

    x, y, w, h = (int(v) for v in a.region.split(","))
    bx0, bx1 = (int(v) for v in a.band.split(","))

    img = np.asarray(Image.open(a.shot).convert("L"), dtype=np.int16)
    reg = img[y : y + h, x : x + w]
    if reg.size == 0:
        print(f"FAIL: empty region {a.region} in {a.shot}")
        return 1

    bg = int(np.median(reg))
    ink = reg < (bg - 60)

    ink_cols = np.where(ink.any(axis=0))[0] + x
    outside = ink_cols[(ink_cols < bx0) | (ink_cols > bx1)]
    if len(outside) > a.max_outside_cols:
        print(
            f"FAIL: {len(outside)} ink columns outside band [{bx0},{bx1}] "
            f"(leftmost {outside.min()}, rightmost {outside.max()}) — "
            "text escaped the overlay column"
        )
        return 1

    if a.min_fill > 0.0:
        ink_rows = np.where(ink.any(axis=1))[0]
        if len(ink_rows) == 0:
            print("FAIL: region has no ink at all")
            return 1
        fill = (ink_rows.max() + 1) / float(h)
        if fill < a.min_fill:
            print(
                f"FAIL: ink bottom at {fill:.2f} of region height "
                f"(< {a.min_fill}) — page underfilled"
            )
            return 1

    print("OK: ink within band")
    return 0


if __name__ == "__main__":
    sys.exit(main())

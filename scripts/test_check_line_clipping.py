#!/usr/bin/env python3
"""Unit tests for the pure clip-decision core of check_line_clipping.py.

Run: python3 scripts/test_check_line_clipping.py   (exit 0 = pass)

These cover the descender-sliver false-positive: a thin (1-2px) descender tip
that the ink-projection splits off as a detached run FAR from the pane edge must
NOT read as a clipped last line — a genuinely clipped line is short *because the
edge cut it*, so it sits AT the edge. See docs/troubleshooting/clip-prevention.md.
"""

import sys

from check_line_clipping import decide_clip, DEFAULT_HEIGHT_FRAC, DEFAULT_MIN_MARGIN, DEFAULT_EDGE_TOL


def check(name, got, want):
    status = "ok" if got == want else "FAIL"
    print(f"  [{status}] {name}: got clip_bottom={got}, want {want}")
    return got == want


def main():
    ok = True

    # The real mid.png regression: last row is a 1px descender sliver 35px above
    # the region's bottom edge; median body row is 22px. Must NOT be a clip.
    ok &= check(
        "descender sliver 35px from edge is not a clip",
        decide_clip(
            first_h=20, last_h=1, median_h=22.0, min_interior=12.0,
            top_margin=14, bottom_margin=35, region_h=582,
        )["clip_bottom"],
        False,
    )

    # A genuinely clipped bottom line: short AND flush against the edge.
    ok &= check(
        "short row flush at bottom edge IS a clip",
        decide_clip(
            first_h=20, last_h=6, median_h=22.0, min_interior=18.0,
            top_margin=14, bottom_margin=0, region_h=582,
        )["clip_bottom"],
        True,
    )

    # A full-height last row with a normal (>= min_margin) background gap below
    # is fine — not short, and not flush against the edge.
    ok &= check(
        "full-height last row with normal margin is not a clip",
        decide_clip(
            first_h=20, last_h=22, median_h=22.0, min_interior=18.0,
            top_margin=14, bottom_margin=6, region_h=582,
        )["clip_bottom"],
        False,
    )

    # A row touching the very edge (zero background margin) is a clip regardless
    # of measured height — the pre-existing min_margin edge-touch rule, preserved.
    ok &= check(
        "row flush at edge with zero margin IS a clip",
        decide_clip(
            first_h=20, last_h=22, median_h=22.0, min_interior=18.0,
            top_margin=14, bottom_margin=0, region_h=582,
        )["clip_bottom"],
        True,
    )

    # A short-but-complete row (0.75-scale speaker label) sitting away from the
    # edge is not a clip — same guard, exercised from the small-row side.
    ok &= check(
        "short complete row away from edge is not a clip",
        decide_clip(
            first_h=20, last_h=16, median_h=22.0, min_interior=16.0,
            top_margin=14, bottom_margin=40, region_h=582,
        )["clip_bottom"],
        False,
    )

    # Top edge still works: a short first row flush against the top IS a clip.
    top = decide_clip(
        first_h=3, last_h=22, median_h=22.0, min_interior=18.0,
        top_margin=0, bottom_margin=30, region_h=582,
    )
    ok &= check("short first row flush at top IS a clip (clip_top)", top["clip_top"], True)

    print("PASS" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

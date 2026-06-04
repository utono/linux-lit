#!/usr/bin/env python3
"""Annotate a screenshot with AT-SPI widget geometry, for agent review.

Walks the accessibility tree of the running app, reads each widget's on-screen
extents (the AT-SPI component interface), and:

  * draws labeled bounding boxes onto <shot>_annotated.png
  * writes the tree + extents to <shot>.tree.json
  * writes machine-detected layout anomalies to <shot>.suspects.txt
    (zero/negative size, off-output, fully covering the whole output)

The suspects file is what the Stop hook forces the agent to address by name —
so "examine the UI" becomes "explain each of these flagged regions against
what the pixels actually show".

Needs the a11y bus (run under scripts/e2e-env.sh), python-dogtail/pyatspi, and
python-pillow. Best-effort: exits 0 with a note if a dep/bus is missing, so it
never breaks a capture.
"""

import argparse
import json
import sys

try:
    import pyatspi
    from PIL import Image, ImageDraw
except Exception as e:  # noqa: BLE001
    sys.stderr.write(f"annotate_ui: skipping (missing dep/bus: {e})\n")
    sys.exit(0)


def find_app(name):
    desktop = pyatspi.Registry.getDesktop(0)
    for app in desktop:
        if app and app.name == name:
            return app
    return None


def extents(acc):
    try:
        comp = acc.queryComponent()
    except NotImplementedError:
        return None
    x, y, w, h = comp.getExtents(pyatspi.DESKTOP_COORDS)
    return (int(x), int(y), int(w), int(h))


def walk(acc, img_w, img_h, nodes, suspects, depth=0):
    role = acc.getRoleName()
    name = acc.name or ""
    ext = extents(acc)
    node = {"role": role, "name": name, "extents": ext, "depth": depth}
    nodes.append(node)

    if ext is not None:
        x, y, w, h = ext
        ident = f"{role}:{name}".strip(":") or f"{role}@{x},{y}"
        if w <= 0 or h <= 0:
            suspects.append(f"{ident}\tzero/negative size {w}x{h}")
        elif x + w <= 0 or y + h <= 0 or x >= img_w or y >= img_h:
            suspects.append(f"{ident}\toff-output at {x},{y} {w}x{h}")
        elif w >= img_w and h >= img_h and depth > 1:
            suspects.append(f"{ident}\tcovers entire output {w}x{h}")

    for child in acc:
        if child is not None:
            walk(child, img_w, img_h, nodes, suspects, depth + 1)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--shot", required=True, help="path to the grim PNG")
    p.add_argument("--app", required=True, help="accessible app name")
    p.add_argument("--timeout", type=float, default=5.0)
    args = p.parse_args()

    import time
    deadline = time.time() + args.timeout
    app = None
    while time.time() < deadline and app is None:
        app = find_app(args.app)
        if app is None:
            time.sleep(0.25)
    if app is None:
        sys.stderr.write(f"annotate_ui: app '{args.app}' not on a11y bus; skipping\n")
        sys.exit(0)

    img = Image.open(args.shot).convert("RGB")
    draw = ImageDraw.Draw(img)
    nodes, suspects = [], []
    walk(app, img.width, img.height, nodes, suspects)

    for n in nodes:
        if not n["extents"]:
            continue
        x, y, w, h = n["extents"]
        if w <= 0 or h <= 0:
            continue
        draw.rectangle([x, y, x + w, y + h], outline=(220, 30, 30), width=1)
        label = f"{n['role']}:{n['name']}".strip(":")
        draw.text((x + 2, y + 2), label[:40], fill=(220, 30, 30))

    base = args.shot.rsplit(".", 1)[0]
    img.save(f"{base}_annotated.png")
    with open(f"{base}.tree.json", "w") as f:
        json.dump(nodes, f, indent=2)
    with open(f"{base}.suspects.txt", "w") as f:
        f.write("\n".join(suspects))

    print(f"annotate_ui: {len(nodes)} widgets, {len(suspects)} suspect(s) -> {base}_annotated.png")


if __name__ == "__main__":
    main()

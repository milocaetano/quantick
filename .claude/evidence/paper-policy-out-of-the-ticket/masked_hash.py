"""Hash a capture with the two clock bands masked out.

The bands are not chosen, they are measured: two paused runs of the *same*
`origin/main` build differ in exactly these rows and nowhere else, across all
nine scenes. Everything outside them - the chart, the ticket, the orders, the
brackets, the risk panel, the strategy editor, the toast - is bit-identical
run to run, so it is fair to demand it be bit-identical build to build.

Usage:
    python masked_hash.py <dir> <labelA> <labelB>
"""

import hashlib
import os
import sys

import numpy as np
from PIL import Image

# Measured from mainP1 vs mainP2, all nine scenes. Rows, inclusive.
MASK_BANDS = [(0, 30), (662, 672)]

SCENES = [
    "paper_orders",
    "paper_order_bracket",
    "paper_order_hover",
    "paper_risk",
    "paper_demo",
    "paper_strategy_editor",
    "cmd_preview",
    "paper_ruler_ticks",
    "toast_paper",
]


def masked_hash(path):
    image = np.array(Image.open(path).convert("RGB"))
    for top, bottom in MASK_BANDS:
        image[top : bottom + 1, :, :] = 0
    return hashlib.sha256(image.tobytes()).hexdigest(), image.shape


def main():
    directory, label_a, label_b = sys.argv[1], sys.argv[2], sys.argv[3]
    print(f"mask: rows {MASK_BANDS} blanked, full width\n")
    print(f"{'scene':24s} {'match':>5s}  {label_a} / {label_b}")
    failures = 0
    for scene in SCENES:
        path_a = os.path.join(directory, f"{label_a}-{scene}.png")
        path_b = os.path.join(directory, f"{label_b}-{scene}.png")
        if not (os.path.exists(path_a) and os.path.exists(path_b)):
            print(f"{scene:24s}  MISSING")
            failures += 1
            continue
        hash_a, shape_a = masked_hash(path_a)
        hash_b, shape_b = masked_hash(path_b)
        ok = hash_a == hash_b and shape_a == shape_b
        failures += 0 if ok else 1
        print(f"{scene:24s} {'OK' if ok else 'DIFF':>5s}  {hash_a[:16]} / {hash_b[:16]}")
    print(f"\n{len(SCENES) - failures}/{len(SCENES)} identical outside the mask")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

"""Derive the mask from the control, then compare the branch outside it.

The method, and why it is sound: a pixel that two runs of the SAME
`origin/main` build disagree on cannot be evidence about a branch. So the mask
is the union of exactly those pixels, measured across three control runs, and
never chosen by hand. What is left is the part of the frame that main
reproduces perfectly, and that part is what the branch must match.

The mask's coverage is printed with the result. A mask that swallowed the
screen would prove nothing, and a reader is entitled to see how much it took.
"""

import glob
import hashlib
import os
import sys

import numpy as np
from PIL import Image

SCENES = [
    "paper_orders", "paper_order_bracket", "paper_order_hover", "paper_risk",
    "paper_demo", "paper_strategy_editor", "cmd_preview", "paper_ruler_ticks",
    "toast_paper",
]


def load(directory, label, scene):
    path = os.path.join(directory, f"{label}-{scene}.png")
    if not os.path.exists(path):
        return None
    return np.array(Image.open(path).convert("RGB"), dtype=np.int16)


def main():
    directory = sys.argv[1]
    controls = sys.argv[2].split(",")
    branches = sys.argv[3].split(",")

    print(f"controls: {controls}   branch runs: {branches}\n")
    total_masked = 0
    total_px = 0
    verdicts = []

    for scene in SCENES:
        frames = [load(directory, c, scene) for c in controls]
        frames = [f for f in frames if f is not None]
        if len(frames) < 2:
            verdicts.append((scene, "NO CONTROL", 0.0))
            continue
        shape = frames[0].shape
        if any(f.shape != shape for f in frames):
            verdicts.append((scene, "CONTROL SIZE MISMATCH", 0.0))
            continue

        # The mask: every pixel any two control runs disagree on.
        mask = np.zeros(shape[:2], dtype=bool)
        for i in range(1, len(frames)):
            mask |= (np.abs(frames[i] - frames[0]).sum(axis=2) > 0)

        coverage = 100.0 * mask.mean()
        total_masked += int(mask.sum())
        total_px += mask.size

        # The branch must match the control outside that mask.
        keep = ~mask
        ref = frames[0].copy()
        ref[mask] = 0
        ref_hash = hashlib.sha256(ref.tobytes()).hexdigest()

        ok = True
        detail = ""
        for b in branches:
            bf = load(directory, b, scene)
            if bf is None or bf.shape != shape:
                ok = False
                detail = "branch capture missing or wrong size"
                break
            cand = bf.copy()
            cand[mask] = 0
            if hashlib.sha256(cand.tobytes()).hexdigest() != ref_hash:
                ok = False
                differing = int(((np.abs(bf - frames[0]).sum(axis=2) > 0) & keep).sum())
                detail = f"{differing} px differ outside the mask"
                break
        verdicts.append((scene, "MATCH" if ok else f"DIFF ({detail})", coverage))

    print(f"{'scene':24s} {'mask %':>7s}  branch vs control outside the mask")
    for scene, verdict, coverage in verdicts:
        print(f"{scene:24s} {coverage:6.2f}%  {verdict}")
    matched = sum(1 for _, v, _ in verdicts if v == "MATCH")
    print(f"\n{matched}/{len(SCENES)} scenes identical outside a mask covering "
          f"{100.0 * total_masked / max(total_px, 1):.2f}% of the frame")
    sys.exit(0 if matched == len(SCENES) else 1)


if __name__ == "__main__":
    main()

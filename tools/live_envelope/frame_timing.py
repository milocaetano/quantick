"""Summarise interleaved dense-replay runs from tools/live_envelope/run_replay.ps1.

Usage: python tools/live_envelope/frame_timing.py <log dir>

Reads <dir>/<label>.err.log for the ten labels below, in their run order, and
prints one table row per run plus a per-side summary. The first
APP_HEALTH_SUMMARY line of each run is the load frame and is excluded from the
means; the envelope fields (`worker_*`) are cumulative, so their maximum over
the steady lines includes the load. A base build predates those fields and
shows `n/a`.
"""

import json
import os
import statistics as st
import sys

ORDER = ["base-1", "head-1", "base-2", "head-2", "head-3", "base-3", "head-4", "base-4", "head-5", "base-5"]


def read(path):
    rows, slow = [], 0
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.startswith("{"):
                continue
            event = json.loads(line)
            code = event.get("event_code")
            if code == "APP_HEALTH_SUMMARY":
                rows.append(event)
            elif code == "APP_SLOW_FRAMES":
                slow += 1
    return rows, slow


def main(root):
    sides = {"base": [], "head": []}
    print(
        "| Run | lines | fps min / mean | frame_avg ms | frame_cpu ms mean | frame_cpu median"
        " | worst ms | trades/s mean | worker_deferred max | worker_backlog max | slow-frame lines |"
    )
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for run in ORDER:
        rows, slow = read(os.path.join(root, f"{run}.err.log"))
        steady = rows[1:]
        fps = [r["fps"] for r in steady]
        avg = [r["frame_avg_ms"] for r in steady]
        cpu = [r["frame_cpu_ms"] for r in steady]
        worst = max(r["frame_worst_ms"] for r in steady)
        rate = st.mean(r["trades_per_s"] for r in steady)
        deferred = max(r["worker_deferred"] for r in steady) if "worker_deferred" in steady[0] else "n/a"
        backlog = max(r["worker_backlog"] for r in steady) if "worker_backlog" in steady[0] else "n/a"
        print(
            f"| {run} | {len(steady)} | {min(fps)} / {st.mean(fps):.2f} | {st.mean(avg):.3f}"
            f" | {st.mean(cpu):.3f} | {st.median(cpu):.3f} | {worst:.2f} | {rate:,.0f}"
            f" | {deferred} | {backlog} | {slow} |"
        )
        sides[run.split("-")[0]].append((min(fps), st.mean(avg), st.mean(cpu), worst, slow))
    for name, runs in sides.items():
        cpus = [r[2] for r in runs]
        print(
            f"- **{name}**: {len(runs)} runs; fps min {min(r[0] for r in runs)};"
            f" frame_avg {st.mean(r[1] for r in runs):.3f} ms;"
            f" frame_cpu mean {st.mean(cpus):.3f} ms (per run {', '.join(f'{c:.2f}' for c in cpus)};"
            f" stdev {st.stdev(cpus):.3f}); worst steady frame {max(r[3] for r in runs):.2f} ms;"
            f" APP_SLOW_FRAMES lines {sum(r[4] for r in runs)}"
        )


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ".")

"""Turn the committed run logs into the tables `perf.md` shows.

The performance claim on this branch was wrong three times, and every time the
mechanism was the same: numbers read off a terminal by hand while a log from a
*different* run sat beside them in the repository. So the numbers are not typed
any more. This reads the committed logs and prints the tables; `perf.md` holds
its output, and the two cannot disagree without this script being re-run and
the diff showing it.

    python .claude/evidence/mt5-session-history/summarise_perf.py
"""

import datetime as dt
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

#: The fill is bracketed by these, so "under the load" is a decided range
#: rather than a judgement call about which row to quote.
FILL_OPENS = "MT5_BACKFILL_START"
FILL_CLOSES = ("BRIDGE_OPENING_COMPLETE", "MT5_BACKFILL_END")

#: The app says this once, when the chart first holds bars -- the instant the
#: trader stops looking at an empty canvas, and so the one worth timing.
FIRST_BARS = "MT5_HISTORY_READY"

HEALTH = re.compile(
    r"APP_HEALTH_SUMMARY.*?fps=(\d+) frame_avg_ms=([\d.]+) frame_cpu_ms=([\d.]+) "
    r"frame_worst_ms=([\d.]+)"
)
STAMP = re.compile(r"^(\d{4}-\d\d-\d\dT[\d:.]+Z)")


def stamp_of(line):
    found = STAMP.match(line)
    return found.group(1) if found else None


def instant_of(stamp):
    """The log's own stamp format, as an instant."""
    return dt.datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def rows(path):
    """Every health summary in the log, with the instant it was printed."""
    out = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        found = HEALTH.search(line)
        if found:
            out.append(
                {
                    "at": stamp_of(line),
                    "fps": int(found.group(1)),
                    "avg": float(found.group(2)),
                    "cpu": float(found.group(3)),
                    "worst": float(found.group(4)),
                }
            )
    return out


def fill_window(path):
    """When the opening block started and finished, from the log's own markers."""
    opened = closed = None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        at = stamp_of(line)
        if at is None:
            continue
        if opened is None and FILL_OPENS in line:
            opened = at
        if opened is not None and any(marker in line for marker in FILL_CLOSES):
            closed = at
    return opened, closed


def under_load(path):
    """The health summaries printed while the opening block was arriving.

    One summary past the close is included: it is printed up to two seconds
    after the frames it describes, so the row straddling the end still reports
    the load. Nothing further, which is the whole point -- quoting the frame
    *after* recovery as though it were the load is the error this exists to
    prevent.
    """
    opened, closed = fill_window(path)
    if opened is None:
        return rows(path), None, None
    inside = [r for r in rows(path) if r["at"] and opened <= r["at"]]
    if closed is None:
        return inside, opened, closed
    kept = []
    for row in inside:
        kept.append(row)
        if row["at"] > closed:
            break
    return kept, opened, closed


def count(path, needle):
    return sum(
        1
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines()
        if needle in line
    )


def trades(path):
    """The opening block plus every slice that followed it."""
    text = path.read_text(encoding="utf-8", errors="replace")
    opened = re.search(r"MT5_HISTORY_READY.*?count=(\d+)", text)
    slices = sum(
        int(found.group(1))
        for found in re.finditer(r"MT5_OPENING_PAGE_READY.*?count=(\d+)", text)
    )
    return (int(opened.group(1)) if opened else 0) + slices


def report(name, path):
    load, opened, closed = under_load(path)
    print(f"### {name} — `{path.name}`")
    print()
    print(f"Fill window: `{opened}` to `{closed}`; {len(load)} health summaries inside it.")
    print(f"Trades charted (backfill + slices): **{trades(path):,}**".replace(",", " "))
    print(f"`APP_SLOW_FRAMES` inside the fill: **{slow_in(path, opened, closed)}**")
    first_line, start, ready = first_paint(path)
    if ready and start and first_line:
        after_start = (instant_of(ready) - instant_of(start)).total_seconds()
        after_first = (instant_of(ready) - instant_of(first_line)).total_seconds()
        print(
            f"First bars on the chart: **{after_start:.2f} s** after "
            f"`{FILL_OPENS}`, **{after_first:.2f} s** after this log's first line."
        )
    print()
    print("```")
    for row in load:
        print(
            f"fps={row['fps']:<3} avg={row['avg']:6.2f} cpu={row['cpu']:5.2f} "
            f"worst={row['worst']:7.2f}"
        )
    print("```")
    print()
    if load:
        print(f"- fps floor **{min(r['fps'] for r in load)}**")
        print(f"- frame_avg peak **{max(r['avg'] for r in load):.2f} ms**")
        print(f"- frame_cpu peak **{max(r['cpu'] for r in load):.2f} ms**")
        print(f"- worst single frame **{max(r['worst'] for r in load):.2f} ms**")
    print()


def first_paint(path):
    """The log's first line, the backfill's start, and the chart's first bars.

    Hand-computing this is what put a figure from one log under a sentence
    naming another, so it is read off the log like everything else here.

    The first stamped line is *not* process launch, and the caller says so: it
    is the earliest moment this log can see, which lands after the process has
    started and its subscriber is up. It is the honest anchor available from a
    log file, and calling it "launch" would be the same overstatement this
    directory keeps having to retract.
    """
    first_line = start = ready = None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        at = stamp_of(line)
        if at is None:
            continue
        if first_line is None:
            first_line = at
        if start is None and FILL_OPENS in line:
            start = at
        if ready is None and FIRST_BARS in line:
            ready = at
    return first_line, start, ready


def slow_in(path, opened, closed):
    if opened is None:
        return count(path, "APP_SLOW_FRAMES")
    hits = 0
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        at = stamp_of(line)
        if at and "APP_SLOW_FRAMES" in line and at >= opened and (closed is None or at <= closed):
            hits += 1
    return hits


def main() -> int:
    for name, filename in (
        ("Control — the main checkout's bridge", "perf-control.log"),
        ("Branch — the session, in slices", "perf-branch.log"),
    ):
        path = HERE / filename
        if not path.is_file():
            print(f"missing: {path}", file=sys.stderr)
            return 1
        report(name, path)
    return 0


if __name__ == "__main__":
    sys.exit(main())

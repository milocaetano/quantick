"""Measure the trade rates the live envelope is sized from.

Reads quantick replay recordings (`<dir>/<SYMBOL>/<YYYY-MM-DD>.csv`, the
format `quantick-replay 1`: `date,time,...` rows, `#` header lines) and prints
one row per session:

- prints: rows in the session (one MetaTrader tick each -- the unit the app
  ingests, which already folds several B3 deals into one print);
- span_h: first to last print, in hours;
- mean/s: prints divided by the span in seconds;
- p99/s and max/s: over fixed one-second bins across the span;
- p99/frame: over fixed 1/60 s bins across the span;
- max/frame: the busiest sliding window of 16 ms (one 60 fps frame).

Usage:
    python tools/live_envelope/tape_rates.py ~/Documents/Quantick/replay

Standard library only; deterministic for a given set of files.
"""

import os
import sys


def millis(line):
    """Milliseconds since local midnight from `date,HH:MM:SS.mmm,...`."""
    t = line[11:23]
    return ((int(t[0:2]) * 60 + int(t[3:5])) * 60 + int(t[6:8])) * 1000 + int(t[9:12])


def percentile(sorted_values, fraction):
    if not sorted_values:
        return 0
    index = min(len(sorted_values) - 1, int(fraction * len(sorted_values)))
    return sorted_values[index]


def session(path):
    stamps = []
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.startswith("#") or not line[:1].isdigit():
                continue
            stamps.append(millis(line))
    if not stamps:
        return None
    stamps.sort()
    first, last = stamps[0], stamps[-1]
    span_ms = max(1, last - first)
    seconds = [0] * (span_ms // 1000 + 1)
    frames = [0] * (span_ms * 60 // 1000 + 1)
    for stamp in stamps:
        offset = stamp - first
        seconds[offset // 1000] += 1
        frames[offset * 60 // 1000] += 1
    busiest = 0
    start = 0
    for end, stamp in enumerate(stamps):
        while stamp - stamps[start] > 16:
            start += 1
        busiest = max(busiest, end - start + 1)
    seconds.sort()
    frames.sort()
    return {
        "prints": len(stamps),
        "span_h": span_ms / 3_600_000,
        "mean_s": len(stamps) / (span_ms / 1000),
        "p99_s": percentile(seconds, 0.99),
        "max_s": seconds[-1],
        "p99_frame": percentile(frames, 0.99),
        "max_frame": busiest,
    }


def main(root):
    header = "symbol  session      prints   span_h  mean/s  p99/s  max/s  p99/frame  max/frame"
    print(header)
    rows = []
    for symbol in sorted(os.listdir(root)):
        folder = os.path.join(root, symbol)
        if not os.path.isdir(folder):
            continue
        for name in sorted(os.listdir(folder)):
            if not name.endswith(".csv") or name.endswith(".context.csv"):
                continue
            stats = session(os.path.join(folder, name))
            if stats is None:
                continue
            rows.append((symbol, stats))
            print(
                f"{symbol:<7} {name[:-4]:<10} {stats['prints']:>9,} {stats['span_h']:>7.2f}"
                f" {stats['mean_s']:>7.1f} {stats['p99_s']:>6} {stats['max_s']:>6}"
                f" {stats['p99_frame']:>10} {stats['max_frame']:>10}"
            )
    for symbol in sorted({symbol for symbol, _ in rows}):
        mine = [stats for s, stats in rows if s == symbol]
        print(
            f"{symbol} over {len(mine)} sessions: prints {min(m['prints'] for m in mine):,}"
            f"-{max(m['prints'] for m in mine):,}; span_h max {max(m['span_h'] for m in mine):.2f};"
            f" mean/s {min(m['mean_s'] for m in mine):.1f}-{max(m['mean_s'] for m in mine):.1f};"
            f" p99/s {min(m['p99_s'] for m in mine)}-{max(m['p99_s'] for m in mine)};"
            f" max/s {max(m['max_s'] for m in mine)};"
            f" p99/frame {min(m['p99_frame'] for m in mine)}-{max(m['p99_frame'] for m in mine)};"
            f" max/frame {max(m['max_frame'] for m in mine)}"
        )


if __name__ == "__main__":
    main(os.path.expanduser(sys.argv[1] if len(sys.argv) > 1 else "~/Documents/Quantick/replay"))

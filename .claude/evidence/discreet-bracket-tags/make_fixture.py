"""Write the synthetic replay day the leg-tag captures play.

The tape wobbles +/-40 pt around 130000 so the capture hook's resting
orders (6 bp out) never fill; the context file's minute candles swing
129500..130600 so the time pane's autoscale holds every leg in view.
Usage: python make_fixture.py <out replay folder>
"""

import math
import os
import sys


def snap(price):
    return int(round(price / 5.0)) * 5


def main(root):
    folder = os.path.join(root, "WINV26")
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "2026-09-09.csv"), "w", newline="") as f:
        f.write("# quantick-replay 1\n# symbol=WINV26\n# timezone=-03:00\n")
        f.write("# side_source=bid_ask\nDate,Time,Price,Bid,Ask,Volume,Side\n")
        for i in range(2400):
            ms = i * 500
            h, rem = divmod(ms, 3600000)
            m, rem = divmod(rem, 60000)
            s, milli = divmod(rem, 1000)
            p = snap(130000 + 35 * math.sin(i / 23.0) + 10 * math.sin(i / 3.1))
            side = "B" if (i * 7) % 3 else "S"
            f.write(
                f"2026-09-09,{10 + h:02d}:{m:02d}:{s:02d}.{milli:03d},"
                f"{p},{p - 5},{p},{1 + (i * 13) % 9},{side}\n"
            )
    with open(os.path.join(folder, "2026-09-09.context.csv"), "w", newline="") as f:
        f.write("# quantick-context 1\n# symbol=WINV26\n# timezone=-03:00\n")
        f.write("# interval_ms=60000\n# complete=true\n# source=synthetic capture fixture\n")
        f.write("Date,Time,Open,High,Low,Close,Volume,Trades\n")
        prev = 130000
        for k in range(60):
            c = snap(130050 + 520 * math.sin(k / 4.0))
            hi, lo = max(prev, c) + 40, min(prev, c) - 40
            f.write(f"2026-09-09,09:{k:02d}:00.000,{prev},{hi},{lo},{c},{500 + k * 7},0\n")
            prev = c


if __name__ == "__main__":
    main(sys.argv[1])

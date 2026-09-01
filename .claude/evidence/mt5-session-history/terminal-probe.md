# The opening block, measured against the trader's own terminal

Criteria **A2** and **A6**. The point of this file is that the fix is not
believed, it is measured — against the live MetaTrader 5 this branch was
written next to, on the contract the report was about.

- Terminal: `XPMT5-PRD` (XP), MetaQuotes build, logged in and running.
- Symbol: `WINV26` — B3's mini index, front month, the contract in the report.
- Date: 2026-08-31. B3 traded 09:03:00.233 to 18:31:23.324 that day.
- Tool: [`live_probe.py`](live_probe.py), which builds a real `Session` around
  the real `MetaTrader5` module and calls the shipping code. No socket, no
  fake terminal, no fixture — the same `last_print_before` and `session_ticks`
  the bridge runs on connect.

Run it yourself with the terminal open:

```
python .claude/evidence/mt5-session-history/live_probe.py WINV26
```

## The result

Raw capture: [`terminal-probe-raw.txt`](terminal-probe-raw.txt).

| | before (rolling 720-minute window) | after (the session) |
| --- | --- | --- |
| oldest tick | **14:03:26** | **09:03:00.233** |
| newest tick | 18:31:23.324 | 18:31:23.324 |
| ticks | 386 173 | **1 525 621** |
| stopped because | the clock said so | `session_edge` — the prints ran out |
| terminal calls | 1 | 2 (1 search + 2 windows) |
| time | 125 ms | 344 ms |

**Recovered: 5.01 hours of the session and 1 139 448 prints.**

## Why the "before" column moves between runs

It is the defect, visible. Three captures were taken over the course of an
hour, and the old window's left edge tracked the wall clock the whole way:

| probe run at | old window started at | session lost |
| --- | --- | --- |
| 22:37 | 13:37:41 | 4.58 h |
| 23:02 | 14:02:58 | 5.00 h |
| 23:03 | 14:03:26 | 5.01 h |

The session's own open never moved, because it is a fact about the market. The
window's did, because it was a fact about the clock. Every one of those runs
would have drawn a chart that looked complete. Open at 21:30 and the edge lands
on 09:30, which is the report this branch was opened for.

The "after" column is 09:03:00.233 in all three, which is the whole point: the
answer no longer depends on when the trader happened to open the chart.

## What it cost

The walk is two terminal calls: one window wide enough to hold a session
(`--backfill-minutes`, still 720 by default) and one more to prove the prints
stop before it. 344 ms for a million and a half prints, against 125 ms for the
third of them the old window returned — so the extra 4x of tape costs about
220 ms, once, on connect.

An earlier revision of the walk cost **1 390 ms** because it rebuilt the
terminal's answer as a Python list. `Session.older_than` and
`Session.join_windows` keep the numpy structured array the terminal returns, and
return it untouched in the single-window case that a trading market always hits.
That is the difference between 984 ms and 219 ms of walk on the same data, and
all of it lands on the frame where the trader is waiting for their chart.

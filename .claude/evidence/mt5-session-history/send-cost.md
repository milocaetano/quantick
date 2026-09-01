# What it costs to put one session on the wire

Criterion **G4**, the fourth of the four causes.

`arch-review.md` claimed "62 s → ~11 s to put a session on the wire, 47 s of it
one `sendall` per tick" and **nothing in this directory backed it**. The probe
that produced it, [`send_cost.py`](send_cost.py), was committed; its output was
not. That is the branch's own defect — a number in prose with no artifact
beside it — so the number is replaced here by a run whose output is committed.

```
python .claude/evidence/mt5-session-history/send_cost.py
```

Raw output: [`send-cost-raw.txt`](send-cost-raw.txt).

## The run

WINV26 on the live terminal, 2026-09-01. The session the walk found is
**816 334 ticks** — a different and smaller day than the 1 525 621-tick session
the rest of this directory measures, because the probe runs on whatever session
the tape is in when it runs. Read the ratios, not the seconds, when comparing
against `whole-day.md`.

```
session: 816334 ticks
  build dicts        :   3.25 s
  json.dumps each    :   2.22 s  (110 MB)
  per-tick sendall   :   3.56 s for 200000  ->  14.54 s extrapolated
  one bulk sendall   :   0.03 s for the whole 110 MB

  TODAY  (per-tick)  :  20.01 s to put the session on the wire
  BUFFERED           :   5.50 s
```

| | per-tick `sendall` (main) | buffered (this branch) |
| --- | --- | --- |
| whole session on the wire | **20.01 s** | **5.50 s** |
| the socket's share of it | **14.54 s** (73%) | **0.03 s** |

The syscall was **73% of the cost**, and one bulk write moves the same 110 MB
in 0.03 s. That is the shape the retired figure was describing; the figure
itself was from an uncommitted run and is not recoverable, so it is gone rather
than restated.

## Two things the run also shows

`{"event_code":"BRIDGE_TICK_FLOOR", "earliest_ms":1734933603326,
"checked":"unnecessary"}` — the floor probe **skipped itself**. The claim
(2024-12-23) sits more than 48 hours below the newest print, so there is
nothing a falsification could change; that skip is the 1109 ms → 0 ms saving on
the common path, here on a real terminal rather than in a bench.

And the terminal answered its own oldest-tick question **correctly** this time.
The bogus "19:30 today" floor in [`whole-day.md`](whole-day.md) is intermittent,
which is exactly why the check falsifies the claim instead of trusting it.

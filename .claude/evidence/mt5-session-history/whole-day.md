# A real bridge, a real socket, a real session

Criterion **A2**. The in-process probe in [`terminal-probe.md`](terminal-probe.md)
proves the walk finds the session. This proves the *bridge* delivers it: the
shipping script launched as its own process, dialling a socket, exactly as
`crates/app/src/feed/mt5_bridge.rs` launches it.

```
python .claude/evidence/mt5-session-history/open_timing.py WINV26
```

## Result

```
opening block     : 1525621 ticks, 225 MB
  from            : 09:03:00.233
  to              : 18:31:23.324
  span            : 9.47 h

  connected       :  0.20 s after launch
  hello           :  0.22 s after launch
  backfill_start  :  0.77 s after launch
  first tick      :  0.77 s after launch
  backfill_end    : 11.23 s after launch
```

The whole trading day, from its first print, opened at 23:00 in the evening.

## The bug this run found, which no fixture would have

The first end-to-end run sent **zero ticks** on that same 1.5 M-print day:

```
{"event_code":"BRIDGE_TICK_FLOOR","symbol":"WINV26","earliest_ms":1788204600002}
{"event_code":"BRIDGE_BACKFILL_NO_HISTORY","symbol":"WINV26","searched_calls":1,
 "note":"the terminal holds no ticks for this symbol; sending an empty block"}
```

`1788204600002` is **19:30 that same evening**. The terminal was asked for its
oldest tick — `copy_ticks_from(symbol, 0, 1, COPY_TICKS_ALL)`, which is what
that call is documented to return — and answered with a tick from an hour
earlier, for a symbol whose history it held back to 2024-12-23 and had served
range queries about seconds before.

The floor is what stops every backwards walk, so believing it ended the search
one step in: it looked at 19:03, compared against a floor of 19:30, and
concluded the contract had no history at all. `backfill_start` and
`backfill_end` went out with nothing between them.

**This is the shape of the original complaint.** Not an error, not a crash — an
empty chart that looks exactly like a market nobody traded. It is also
intermittent: three separate in-process probes and two fresh interpreters all
got the correct 2024-12-23 answer from the same terminal minutes apart. A test
against a fake terminal would never have produced it, and neither would any
number of in-process probes.

`Session.earliest_tick_ms` now falsifies the claim instead of trusting it — one
`copy_ticks_range` for the window immediately below, which is decisive because
below a bogus "19:30 today" sits the whole session and below a real floor there
is nothing. The same run, with the check in place:

```
{"event_code":"BRIDGE_TICK_FLOOR_IMPLAUSIBLE","symbol":"WINV26",
 "claimed_ms":1788204600002,"found_below":226666,"action":"ignore_the_floor"}
{"event_code":"BRIDGE_BACKFILL_SESSION","symbol":"WINV26","count":1525621,
 "first_ms":1788166980233,"last_ms":1788201083324,"windows":2,"stopped_on":"session_edge"}
```

Guarded by
`bridge/mt5/tests/test_session_backfill.py::test_a_terminal_that_misreports_its_oldest_tick_is_not_believed`,
with `test_a_real_floor_is_still_honoured` beside it so the check falsifies a
claim rather than discarding the floor.

## Note on the clock

The bridge refused to start at all until given `--utc-offset-s -10800`:

```
{"event_code":"BRIDGE_UTC_OFFSET_UNKNOWN","action":"refuse_to_start",
 "hint":"no fresh tick and nothing cached: run once during market hours, or
  pass --utc-offset-s (B3 brokers use -10800)"}
```

That is correct behaviour, not a defect — the market was shut, so the server
clock could not be measured, and guessing it would put every timestamp on the
chart three hours out. It is recorded here so the flag in `open_timing.py` is
not mistaken for a workaround.

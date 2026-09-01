# The two ends of the tape cannot drift apart

Criterion **A7**. A guard is worth nothing until it has been seen to fail, so
this records it failing.

The test is `crates/app/tests/session_gap_agreement.rs`. It reads
`SESSION_GAP_MS` and `SESSION_WALK_MAX_SPAN_MS` out of
`bridge/mt5/quantick_bridge.py`, reads `SESSION_GAP_MS` and
`MAX_CAMPAIGN_SPAN_MS` out of `crates/app/src/history_reach.rs`, and compares
them. Neither side is imported: `crates/app` builds as a binary with no library
target, so both are read the same way and neither can pass by being the one
that gets linked.

## Green, as shipped

```
running 2 tests
test the_walk_budget_is_derived_from_the_span_it_bounds ... ok
test the_bridge_and_the_app_measure_a_session_the_same_way ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Red, on purpose

`SESSION_GAP_MS` in the bridge was changed from `60 * 60 * 1000` to
`45 * 60 * 1000` and the test run again:

```
thread 'the_bridge_and_the_app_measure_a_session_the_same_way' panicked at
crates\app\tests\session_gap_agreement.rs:162:5:
the MetaTrader bridge and the chart no longer measure a session the same way:
  bridge SESSION_GAP_MS = 2700000 but chart SESSION_GAP_MS = 3600000
    the bridge stops the opening block at a gap this wide, and the app decides
    a load-older campaign reached a session edge at the same one. Different
    values mean the chart opens on a block whose edge the campaign does not
    recognise.
Change both, or neither.

test result: FAILED. 1 passed; 1 failed
```

The 45-minute value was reverted immediately; `git diff` on this branch shows
`SESSION_GAP_MS = 60 * 60 * 1000`.

## Why this guard and not a comment

The repository has already run this experiment. `QuantickBridge.mq5` opens on
30 minutes of history and `quantick_bridge.py` opened on 720, and the two
drifted quietly enough that four branches worked on the symptom without either
number being questioned. Both of those constants already had a comment. What
they did not have was a test.

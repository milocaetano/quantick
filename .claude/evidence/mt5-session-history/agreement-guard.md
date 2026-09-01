# The two ends of the tape cannot drift apart

Criterion **A7**. A guard is worth nothing until it has been seen to fail, so
this records it failing.

The test is `crates/guards/tests/session_gap_agreement.rs`. It reads
`SESSION_GAP_MS` and `SESSION_WALK_MAX_SPAN_MS` out of
`bridge/mt5/quantick_bridge.py`, reads `SESSION_GAP_MS` and
`MAX_CAMPAIGN_SPAN_MS` out of `crates/app/src/history_reach.rs`, and compares
them. Neither side is imported: `crates/app` builds as a binary with no library
target, so both are read the same way and neither can pass by being the one
that gets linked.

## Green, as shipped

`cargo test -p quantick-guards --test session_gap_agreement` — and it lives in
`crates/guards` now, beside the repository's other guards, because `main`
carved that crate precisely so a question like this costs a second rather than
a full app link:

```
running 5 tests
test the_fill_progress_is_on_the_wire_and_is_optional ... ok
test the_walk_budget_is_derived_from_the_span_it_bounds ... ok
test the_shipped_config_default_agrees_with_the_bridge_too ... ok
test the_bridge_and_the_app_measure_a_session_the_same_way ... ok
test the_slice_cap_matches_what_the_feed_will_accept ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

It grew from two guards to five across the review rounds — the shipped config
default, the slice cap against the feed's own per-block cap, and the fill
progress being on the wire *and optional*. Each was added because a review
found a claim nothing held to account.

## Red, on purpose

`SESSION_GAP_MS` in the bridge changed from `60 * 60 * 1000` to
`45 * 60 * 1000`, and the suite re-run:

```
thread 'the_bridge_and_the_app_measure_a_session_the_same_way' panicked at
crates/guards/tests/session_gap_agreement.rs:179:5:
  bridge SESSION_GAP_MS = 2700000 but chart SESSION_GAP_MS = 3600000
Change both, or neither.

thread 'the_shipped_config_default_agrees_with_the_bridge_too' panicked at
crates/guards/tests/session_gap_agreement.rs:224:5

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

**Two** guards catch it, not one: the constants disagree, and so do the bridge
and the shipped `feeds.toml` default. The 45-minute value was reverted
immediately; `git diff` on this branch shows `SESSION_GAP_MS = 60 * 60 * 1000`.

## Why this guard and not a comment

The repository has already run this experiment. `QuantickBridge.mq5` opens on
30 minutes of history and `quantick_bridge.py` opened on 720, and the two
drifted quietly enough that four branches worked on the symptom without either
number being questioned. Both of those constants already had a comment. What
they did not have was a test.

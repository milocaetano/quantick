# Injected breakage: the tests fail without the fix

The replay short-circuit in `crates/app/src/control/gateway/server.rs` was
disabled (`if false && let Some(ticket) = ...`) and the two end-to-end tests
re-run. Both failed; both pass with the line restored, and the restored file
is byte-identical to the reviewed one.

```text
failures:
    app::tests::control_plane_tests::the_same_layout_key_answers_twice_and_acts_once
    app::tests::control_plane_tests::the_same_layout_key_with_different_input_is_refused_as_a_conflict

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 1970 filtered out
```

With the line restored:

```text
test app::tests::control_plane_tests::the_same_layout_key_with_different_input_is_refused_as_a_conflict ... ok
test app::tests::control_plane_tests::the_same_layout_key_answers_twice_and_acts_once ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1970 filtered out
```

## The reservation, separately

The in-flight reservation in `IdempotencyStore::admit` was disabled
(`if false && !ledger.in_flight.insert(...)`) and the race test re-run. It
failed: with no reservation the pipelined retry is dispatched instead of
refused, so the first response is no longer the one waiting.

```text
test app::tests::control_plane_tests::a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on ... FAILED
assertion `left == right` failed
  left: RequestId("first")
 right: RequestId("second")

test result: FAILED. 0 passed; 1 failed
```

The line was restored and the test passes again.

# What is generated cannot drift (A11 — R1)

The judging ask. Three hand-maintained claims are now mechanically compared
against the code, and each demonstration below is a hand edit that reddens
`cargo test -p quantick-guards` — the guard named `generated`, in
`crates/guards/src/generated.rs`, plus two byte-for-byte tests in `crates/app`.

## Guard 1 — a capability documented that is not registered
```
generated guard: 2 finding(s)
docs/control-plane/capability-inventory.md:61: `trade.order.plaice` is documented but no `*_CAPABILITY_ID` constant under crates/app/src/control declares it
crates/app/src/control/trade.rs:71: `trade.order.place` is registered but has no row in docs/control-plane/capability-inventory.md
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
```

## Guard 2 — a hook described that nothing reads

The historical defect, reintroduced: `QUANTICK_DRAWING_MANAGER` for
`QUANTICK_DRAWINGS_MANAGER`.
```
generated guard: 2 finding(s)
crates/app/src/surfaces/drawing_chrome/mod.rs:1142: `QUANTICK_DRAWINGS_MANAGER` is declared as a hook but docs/ui-harness/hook-prose.md never mentions it
docs/ui-harness/hook-prose.md: `QUANTICK_DRAWING_MANAGER` is described but no `declare_hooks!` in crates/app/src declares it — a capture setting it would reach nothing
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s
```

## Guard 3 — a hook read that nothing describes
```
generated guard: 1 finding(s)
crates/app/src/frvp.rs:509: `QUANTICK_INVENTED` is declared as a hook but docs/ui-harness/hook-prose.md never mentions it
test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s
```

## All three reverted, guards green
```
test every_guard_in_the_registry_has_a_test_here ... ok
test no_context_file_grows_past_its_recorded_ceiling ... ok
test the_generated_indexes_match_the_code_they_describe ... ok
test no_tracked_file_grows_past_its_recorded_ceiling ... ok
test sources_are_utf8_without_a_bom_or_mojibake ... ok
test tracked_files_are_written_in_english ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s
```

## And the byte-for-byte tests behind them

The guard compares name sets in a second, with no dependencies. The two tests
below hold the real registries and compare the whole file, which catches what
a set comparison cannot — a reworded prose cell, or a permission added to a
capability that already had a row.
```
test hooks::tests::the_committed_registry_is_what_the_generator_emits ... ok
test control::inventory::tests::the_committed_inventory_is_what_the_generator_emits ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 2195 filtered out; finished in 0.04s
```

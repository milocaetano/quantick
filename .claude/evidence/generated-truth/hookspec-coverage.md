# HookSpec coverage (A6 — R9)

Every hook the application reads declares itself where it is read. The
declaration is one line — `crate::hooks::declare_hooks![…]` — expanding to a
`HOOKS` slice the module owns; `crates/app/src/hooks.rs` collects them.

## The count

| | |
| --- | --- |
| Hooks declared | 126 |
| Hooks described in the prose | 126 |
| Difference | **0** |
| Owning files | 37 |
| Allowlisted `QUANTICK_*` that are not hooks | 3 |

The 129 the first pass found became 126: three were not launch hooks at all.
`QUANTICK_GIT_COMMIT` is read through `option_env!` at compile time, and
`QUANTICK_FAKE_STORE` and `QUANTICK_TEST_STORE_HOME_ENV` live inside their
modules' own `#[cfg(test)]` blocks. Each is on the guard's signed allowlist
with the reason, rather than given a harness row describing a hook that is not
one — the parity guard forced that question to be answered instead of guessed.

## The registration shape — a window hook
```rust
crate::hooks::declare_hooks!["QUANTICK_FRVP_FOLD_BUDGET"];

#[cfg(test)]
```

## The registration shape — a floating surface's own hook
```rust

crate::hooks::declare_hooks!["QUANTICK_AGENT_POPUP"];

#[cfg(test)]
```

## The collector, one line per owning module
```rust
pub(crate) const OWNERS: &[(&str, &[HookSpec])] = &[
    ("crates/app/src/app.rs", crate::app::HOOKS),
    (
        "crates/app/src/bubble_presets.rs",
        crate::bubble_presets::HOOKS,
    ),
    ("crates/app/src/chart_layers.rs", crate::chart_layers::HOOKS),
    ("crates/app/src/config.rs", crate::config::HOOKS),
    (
        "crates/app/src/drawings/presets.rs",
        crate::drawings::presets::HOOKS,
    ),
    // … 37 entries
```

## The tests that keep it honest
```
test hooks::tests::an_unrecognised_hook_is_reported_rather_than_ignored ... ok
test hooks::tests::every_declared_hook_is_accepted ... ok
test hooks::tests::no_hook_is_declared_by_two_modules ... ok
test hooks::tests::variables_outside_the_prefix_are_not_this_guard_s_business ... ok
test hooks::tests::the_committed_registry_is_what_the_generator_emits ... ok
test hooks::tests::every_owner_path_is_a_real_file ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 2191 filtered out; finished in 0.00s
```

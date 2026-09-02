# G1 / G2 — the four checks, and the guards

Each run on its own, never chained: a `&&` or a trailing `|| echo` has
reported success on this repository while one of the four had failed.

| check | exit | result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | no warnings |
| `cargo build --workspace` | 0 | `Finished dev profile in 1m 43s` |
| `cargo test --workspace` | 0 | **2175 passed, 0 failed, 6 ignored** (app), every other crate green |

`cargo check -p quantick-app --all-targets` is also warning-free: no dead
code, no unused import, nothing left behind by the move.

## G1 — English

`cargo test -p quantick-guards` runs the language scan, the encoding check
and the size ratchet:

```
test result: ok. 4 passed; 0 failed
```

The new module is English throughout — identifiers, doc comments, the
module header, every test name and assertion string. No exemption is
claimed anywhere in this branch.

## CI's non-cargo steps

Untouched by this change and not run: `ruff` over `tools/mt5/` and
`bridge/mt5/`, the session-exporter test and the bridge's Python tests.
This branch edits no Python and no shell.

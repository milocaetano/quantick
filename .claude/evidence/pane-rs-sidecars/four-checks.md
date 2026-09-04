# The four checks (G2)

Run in the worktree, each on its own command and each exit code read
separately — never chained behind a `&&` or a `|| echo`, which is how a red
check has printed a green summary here before.

```
$ cargo fmt --all -- --check
FMT_EXIT=0

$ cargo clippy --workspace --all-targets
    Finished `dev` profile [optimized + debuginfo] target(s) in 37.93s
CLIPPY_EXIT=0

$ cargo build --workspace
    Finished `dev` profile [optimized + debuginfo] target(s) in 1m 56s
BUILD_EXIT=0

$ cargo test --workspace
    … 96 test binaries, every one `test result: ok` …
    crates/app: ok. 1894 passed; 0 failed; 4 ignored
TEST_EXIT=0
```

No warning was emitted by clippy or the build. The workspace sets
`warnings = "deny"`, so an unused import left behind by the move would have
failed the build rather than passed quietly — which is why the import lists in
the four new modules are exactly what each file uses.

## The guards

```
$ cargo run -q -p quantick-guards
(no output, exit 0)

$ cargo test -p quantick-guards
ok. 138 passed; 0 failed   (plus 16 + 5 in the integration binaries)
```

The guards' silence covers the size ratchet, the context ratchet, the module
cycle check, the headless rule, the English rule (`language.rs`) and the
generated-file check. The last is what proves the hook registry and the
capability inventory still match the code that generates them.

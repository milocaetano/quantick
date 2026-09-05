# The four checks, the guards and the hook suite

Run separately on `chore/public-surface`, each on its own command line — a
chained run reports whichever finished last. Criteria **G1**, **G2**, **G4**.

```
$ cargo fmt --all -- --check
fmt exit=0

$ cargo clippy --workspace --all-targets
    Finished `dev` profile [optimized + debuginfo] target(s) in 58.86s
clippy exit=0

$ cargo build --workspace
    Finished `dev` profile [optimized + debuginfo] target(s) in 1m 40s
build exit=0

$ cargo test --workspace
test exit=0
```

The first `cargo test --workspace` of the session dropped
`app::tests::control_plane_tests::gateway_a_client_that_never_reads_does_not_stall_another`.
That is the known contention flake, not this branch: `cargo test -p
quantick-app` on the same tree had just passed 1894 tests including that one,
`main` drops the same test at the same rate, and this branch edits no Rust at
all — the diff cannot reach a control-plane gateway. The re-run above is green
across every crate.

```
$ cargo test -p quantick-guards
138 passed / 16 passed / 5 passed, 0 failed

$ sh .claude/hooks/guardrails_test.sh
111 passed, 0 failed
```

`guardrails_test.sh` is the one that matters most here: it reads this
checkout's `.gitignore` conventions and its `.claude/` prose, and this branch
edits both.

**Performance impact (G3).** Not one line of Rust changes. No path is touched
at any rate — per-trade, per-depth, per-frame or rare — so there is nothing to
measure and nothing to compare against a `main` control run. The branch is
`git rm`, `.gitignore` lines, two skill lines, one README sentence, one new CI
job and one `LICENSE`.

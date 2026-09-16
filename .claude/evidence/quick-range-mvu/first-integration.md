# First integration checkpoint (not final-head verification)

Base HEAD: 9ff57501249f51d8f75c21f52094ec3dc3c39af2. MERGE_HEAD:
a6644bc602e203b17bb29a5581548b03c4898fc6. No commit or remote publication.

## Characterization before runtime edits

Borrowed A1R target: C:/src/quantick-worktrees/feat-evidence-resources/target.
`cargo test -p quantick-app quick_range_ -- --nocapture`, session 96733:
exit 1, compile 8m27s, tests 0.23s; 3 failed, 1 passed, 2081 filtered.
The failures were stale slots after rebuild, duplicate-time slot identity
(actual [199,199], expected [80,160]), and persistent selection chrome.
The shipped Fibonacci range test passed. These are expected regressions,
not a baseline claim that the application passed.

## First candidate results

`cargo test -p quantick-chart-interaction --target-dir target`: 13 tests passed.
Ten lifecycle/ordering tests and three exact-reference resolution tests.
The crate has no dependencies, toolkit, transport, or clock.

`cargo check -p quantick-app --all-targets`, session 54464: exit 0,
27.94s, after contract extraction but before the subsequent owner adapters.
Session 70944: same command, exit 0, 24.10s, after initial adapter integration.

`cargo test -p quantick-app quick_range_ -- --nocapture`, session 83567:
exit 0; compilation 2m38s, tests 0.16s. All four passed: the three
characterizations above plus the shipped Fibonacci range test.
During its compilation the next shell forwarding cleanup was started;
this receipt therefore describes the first integrated test artifact only,
not the final formatted tree. A fresh complete verification is required.

## Guard status

No budget or exemption increase. Unformatted first integration measured
UI-free 46917 versus prior budget 46911. After formatting and typed-port
forwarder cleanup: 46966 versus 46911; extension boundary 10824 versus
10802. These are FAILING measurements, not guard passes. Further genuine
ownership/duplication work remains. Headless findings and scan failure
counters were zero. Full checks, contract regeneration, performance,
feature-enabled checks, UI evidence and all independent reviews remain pending.

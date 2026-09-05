# Architecture baseline and validation protocol

Status: foundation evidence at source revision
`8673489664fe8f9dfe5237de90bf85ef2bd28db6`, collected 2026-09-05.
This PR changes documentation only. Structural measurements describe the audited
code; runtime comparisons become acceptance evidence for later refactors.

## Environment and structural evidence

- Windows, `x86_64-pc-windows-msvc`.
- Intel Core i5-12400F, 6 cores / 12 logical processors.
- `rustc 1.98.0 (88d9e12ae 2026-08-18)`, LLVM 22.1.8.
- `cargo 1.98.0 (797e8a9bc 2026-08-05)`; repository `Cargo.lock` unchanged.
- Lockfile SHA256: `02D505E112DCCE244B11627708DA2815C6068402997A5C0D805B0E483D1C6721`.
- Benchmark source SHA256 (Windows checkout of `control_plane_tests.rs`):
  `BA8E22DFAC51FD0C2A9B195A3F170AB95D86C3AB019BB45543203E013F03B0A8`.
- Worktree: `docs/architecture-foundation`, from updated `origin/main` at the
  revision above. Initial `cargo build -p quantick-guards` and
  `cargo check -p quantick-app --all-targets` both exited 0 before edits.

Command: `cargo run -p quantick-guards -- --report`. The initial capture used
the just-built `target/debug/quantick-guards.exe --report` to avoid the active
Cargo build lock; it invokes the same report entry point.

Selected output (production-source scanner measurements, not coupling scores):

```text
crate.lines.app                         109942
crate.lines.total                      175156
crate.lines.app_percent                62
file.largest.crates/app/src/pane.rs     5384
struct.wide.app::ChartPane              34
struct.wide.app::Tab                    62
struct.wide.app::PaperTrading           31
ratchet.size.budget                    45739
ratchet.size.recorded                  45505
ratchet.cycle.measured                 3
headless.findings                      0
scan.unreadable                        0
scan.undecodable                       0
scan.blind                             0
```

`QuantickApp` is below the report's wide-struct threshold; absence is not a
zero-field measurement. The cycle figure records the existing baseline, not a
claim that every module is cycle-free. Count reductions alone do not establish
encapsulation. The audit's field-consumer and lifetime evidence matters more.

## Runtime evidence and its limits

The existing manual test
`app::tests::control_idle_dense_replay_benchmark` in
[control_plane_tests.rs](../../crates/app/src/app/tests/control_plane_tests.rs)
provides a deterministic workload and prints average/p99/worst CPU frame time.
Run it with:

```powershell
cargo test -p quantick-app control_idle_dense_replay_benchmark -- --ignored --nocapture --test-threads=1
```

It uses 8,000 seeded trades, 30 warm-up frames, 600 measured frames and 64 trades
per measured frame. The test does not itself toggle control access or compare
two configurations; paired access-mode scenarios need explicit harness setup.
Treat a run as a fixture observation,
not proof of GPU rendering, multiwindow behavior, memory scaling, remote request
pressure or untrusted-script execution budgets. A documentation-only PR makes
no runtime improvement or regression claim.

Three sequential baseline runs passed in the repository's optimized test profile,
after builds and the workspace suite had finished. No profile override was set
by this mission. These are repeated baseline samples, not base/candidate pairs:

| Sample | CPU average (ms) | CPU p99 (ms) | CPU worst (ms) | Trades/s |
| --- | --- | --- | --- | --- |
| 1 | 1.115009 | 1.802500 | 2.386600 | 57317.916 |
| 2 | 1.108253 | 1.797400 | 2.233600 | 57661.000 |
| 3 | 1.090924 | 1.787000 | 2.070300 | 58572.113 |

The printed `feed_arrival_ms` uses synthetic historical timestamps and is not
interpreted as live latency. These measurements do not establish a production
frame-rate target or a GPU budget.

Before each runtime extraction, capture its relevant scenarios on the same
machine and compiler, alternating base/candidate order. Record full SHA, lockfile
hash, build profile, command/environment, fixture hash and dimensions; record
display/GPU/scale for GUI work. Keep raw outputs in the task's evidence bundle.
Use at least three paired measurements to expose variation, not just the best
sample. Do not run builds or competing benchmarks during timed collection.

| Scenario | Existing evidence source | Required observations |
| --- | --- | --- |
| Deterministic ingestion | `crates/engine/benches/hot_path.rs`, fixed 5,000,000 synthetic trades per builder | ns/trade, throughput, unchanged closed-bar counts; separate from chart performance |
| Idle control | Manual dense replay fixture plus explicit access-mode harness setup | CPU average/p99/worst, control off vs enabled-idle |
| Requested snapshots/commands | `control/registry.rs` capture counters, bounded gateway queue and action logs | Capture duration, response size, queue pressure, longest handler; a dispatch budget cannot preempt a handler |
| Visible and hidden market views | `app/health.rs` `APP_HEALTH_SUMMARY`, lifecycle fixtures | Frame wall/CPU, trade/depth rates, lag, queue length, worker and feed counts; all required sessions remain drained |
| History growth and pane multiplication | Retained `ChartState` tape plus orderflow health counters and process memory | Memory at equal history across 1/2/4 tabs and panes; distinguish intentional copies and leaks |
| Ownership transfer and close | Tab/worker/paper lifecycle tests | No restart/reset/fill on transfer; one explicit close and worker termination on session end |
| Restore/default appearance | Workspace fixtures and UI harness | Same document/semantic state; default visual evidence under equal DPI and font setup |

These are measurement scenarios, not promised capacity limits. Detailed memory,
GPU, minimized-window and request-pressure measurements are not collected by
this docs milestone; F4/F5 must collect relevant evidence before touching their
hot paths. Set any numerical regression threshold before evaluating a candidate,
using base-run spread and the affected interaction; do not invent one after
seeing a regression or describe an unmeasured path as unchanged in speed.

## Existing behavior tests to retain

From `app/tests/workspaces_tests.rs`:

- `a_cockpit_exported_from_the_app_comes_back_when_it_is_opened`
- `opening_a_workspace_replaces_the_tab_strip_instead_of_growing_it`
- `opening_a_file_that_is_not_a_workspace_changes_nothing_on_screen`
- `a_restored_workspace_puts_the_window_back`

From `app/tests/panes_layout_tests.rs`:

- `switching_tabs_preserves_everything_each_one_owns`
- `a_background_tab_keeps_ingesting`
- `closing_a_tab_activates_a_neighbour_and_drops_its_market`
- `closing_a_tab_ends_its_worker_threads`

Also retain workspace-store debounce/blocked-write/exit-flush tests, control
permission and schema fixtures, engine golden fixtures and indicator commit/
preview fixtures. F1-F7 name the additional boundary tests they need; a test
using production code is required, not a behavior-changing test-only substitute.

## Verification record for this foundation PR

Commands run sequentially on this branch:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo clippy --workspace --all-targets` | Exit 0 |
| `cargo build --workspace` | Exit 0 |
| `cargo test --workspace` | Exit 0; summed Rust results: 3,439 passed, 0 failed, 12 ignored |

The workspace suite includes the repository guards (including headless, language,
encoding and ratchets). Authored prose was also read manually; the mission's
Portuguese request is an attributed quotation. Local Markdown link targets and
`git diff --check` were checked successfully. No Rust or test implementation
changed, so the docs edit does not require new implementation-mirroring tests.

Raw logs are retained in the session evidence directory
`%TEMP%/quantick-architecture-foundation/` (`fmt.log`, `clippy.log`, `build.log`,
`test.log`, `dense-benchmark.log`, `dense-benchmark-2.log`,
`dense-benchmark-3.log`, `guards-before.txt`, `environment.txt`). The PR
publishes the results; this local directory is also supplied to the independent
reviewers. It is not a portable artifact or a substitute for CI logs.

Diff boundary: only `docs/architecture/`, `docs/README.md` and the mission archive
are intended changes. No runtime, schemas, manifests, lockfile, guard budgets or
existing local changes are modified. Confirm using `git diff --name-only
origin/main...HEAD` at final review.

Independent final review verdicts and CI are recorded in the PR body after the
final commit, so this file does not claim a review of a commit that does not yet
exist. Issue #314 was created; the Projects update failed because the installed
token lacks `read:project`. The issue and PR are usable without that board update.

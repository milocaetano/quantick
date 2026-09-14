# Validation: pane-local layout strips

Candidate head before mission archival: `2fea2bf9` on top of `eb7bb039`.

## Focused checks

- `cargo test -p quantick-app panes_layout_tests`: PASS, 74 tests.
- `cargo test -p quantick-guards`: PASS, 228 tests across guard targets.
- `git diff --check`: PASS.
- Hook registry regenerated from the application and checked by the generated
  artefact guard.

## Dense frame comparison

The existing ignored `control_idle_dense_replay_benchmark` ran twice on the
clean `main` checkout and twice on the candidate. Every sample used 8,000
initial trades, 30 warm-up frames, 600 measured frames, and 64 new trades per
frame on the same host.

| Revision | Run 1 mean / p99 ms | Run 2 mean / p99 ms | Two-run median mean / p99 ms |
| --- | ---: | ---: | ---: |
| `main` (`eb7bb039`) | 1.117452 / 3.117000 | 1.110512 / 2.294800 | 1.113982 / 2.705900 |
| candidate | 1.059225 / 1.899800 | 1.149794 / 3.064900 | 1.104510 / 2.482350 |

Candidate delta: mean **-0.85%**, p99 **-8.26%**. This is flat or better and
well inside the mission's +10% mean / +15% tail guardrail. Hook-driven live
captures separately held 59-60 fps with zero worker backlog.

## Mandatory loop

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets`: PASS.
- `cargo build --workspace`: PASS.
- `cargo test --workspace`: 2,068 app tests passed, 11 ignored, and two
  unrelated order-flow worker tests failed. Both reproduce identically when
  run alone on a clean `main` checkout at `eb7bb039`:
  `bubbles_project_while_book_capture_stays_off` and
  `the_live_strip_alone_keeps_the_aggression_pipeline_running`. Neither file
  nor its dependencies are changed by this mission. This baseline failure is
  recorded rather than represented as candidate-green; exact-head CI remains
  the final authoritative gate.

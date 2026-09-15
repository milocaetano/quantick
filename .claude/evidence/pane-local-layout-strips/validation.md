# Validation: pane-local layout strips

Runtime commit `c8e9502e` on top of `main` `aaae6f32`. The evidence commit
that contains this report changes no executable source.

## Focused checks

- `cargo test -p quantick-app panes_layout_tests`: PASS, 75 tests.
- `cargo test -p quantick-guards`: PASS, 257 tests across guard targets.
- `cargo test -p quantick-app a_three_chart_canvas_resizes_its_context_rows_up_and_down`: PASS.
  This is the stacked-pane resize test added on the rebased `main`; the pane
  floor now reserves both the chart readability minimum and its local footer.
- `git diff --check`: PASS.

## Dense frame comparison

The existing ignored `control_idle_dense_replay_benchmark` ran twice on the
clean current `main` checkout and twice on the candidate. Every sample used
8,000 initial trades, 30 warm-up frames, 600 measured frames, and 64 new
trades per frame on the same host and target directory.

| Revision | Run 1 mean / p99 ms | Run 2 mean / p99 ms | Two-run median mean / p99 ms |
| --- | ---: | ---: | ---: |
| `main` (`aaae6f32`) | 0.789074 / 1.314100 | 0.921524 / 1.828800 | 0.855299 / 1.571450 |
| candidate | 0.702834 / 1.266400 | 0.998597 / 1.960400 | 0.850716 / 1.613400 |

Candidate delta: mean **-0.54%**, p99 **+2.67%**. This is flat and inside the
mission's +10% mean / +15% tail guardrail. Fresh hook-driven visual captures
also reported zero worker backlog.

## Mandatory loop after rebase

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets`: PASS.
- `cargo build --workspace`: PASS.
- `cargo test --workspace`: 2,064 app tests passed and 11 were ignored; two
  unrelated order-flow worker tests failed:
  `bubbles_project_while_book_capture_stays_off` and
  `the_live_strip_alone_keeps_the_aggression_pipeline_running`.

The first failure was rerun identically on a clean checkout of current `main`
at `aaae6f32` (`left: 0`, `right: 1`). Neither failing test nor its order-flow
pipeline is changed by this mission. This is recorded as a local Windows
baseline failure, not represented as candidate-green; exact-head GitHub CI is
the authoritative final gate.

## Fresh visual and control evidence

The application was rebuilt from `c8e9502e` immediately before capture. The
wide, narrow, rename, delete, and three-pane stacked states were relaunched
through the registered UI hooks with isolated scratch stores and inspected at
native display scale. `control-health.jsonl` records the same build capturing
14 scopes and a screenshot through the control plane, followed by a clean
gateway shutdown.

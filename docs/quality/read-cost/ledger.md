# Read-cost ledger

What each merged pull request asked a reader to hold: the production lines of
the Rust files it changed, plus the production lines of the files those
directly reference. `tools/read_cost/measure.py` is the calculator and owns
what counts; this file is only its record. Direct references, never a
transitive closure.

A pull request carries its own row. Before the merge, on the branch:

```sh
python tools/read_cost/ledger.py record --repo . --pr <number>
```

It measures the same two objects CI measures — the base the pull request was
opened against and its head — so the row is the same one a workflow would have
written afterwards. The read-cost comment on the pull request asks for it when
it is missing, `pr-gate` repeats the ask at `gh pr ready` and `gh pr merge`,
and after the merge `ci.yml`'s `read-cost-ledger` job annotates the run when a
merged pull request left none. Nothing writes here on the repository's behalf:
`main` takes pull requests only, and a bot that could push to it would need a
write credential living in CI.

The first 30 rows were backfilled through the same calculator, over the last
30 merged pull requests at the time — campaign children included, which is why
the `Base` column exists.

`Read cost` is `production_lines`. `Changed` is how many production Rust files
the diff touched. `Top referenced` is the largest file the change pulls in by
reference alone — one it never edits and a reader still has to open. `Date` is
the day the row was written: the merge date for the backfilled rows, the day
the pull request measured itself for the rows added since.

## The ceiling

<!-- read-cost-ceiling:v1 18000 -->

**18,000 production lines.**

The median of the `feat/` and `fix/` rows in the backfill, rounded to the
thousand: 28 feature rows, median 18,242, so 18,000.

It is a warning and never a block. `pr-gate` prints it, with the files the
branch pulls in by reference alone, when a feature branch goes over.
`python tools/read_cost/ledger.py median` recomputes what the rows suggest
today; moving the recorded number is a person's decision, in its own pull
request, because a ceiling that follows the rows is a description rather than
a bound.

| PR | Date | Branch | Base | Read cost | Changed | Top referenced |
| ---: | --- | --- | --- | ---: | ---: | --- |
| 487 | 2026-09-14 | feat/pr-read-cost | campaign/outside-eight | 0 | 0 | — |
| 488 | 2026-09-14 | fix/feed-gap-honesty | campaign/outside-eight | 17251 | 14 | `crates/app/src/state.rs` (663) |
| 489 | 2026-09-15 | fix/resize-stacked-panes | main | 8098 | 3 | `crates/app/src/tab/feed.rs` (642) |
| 493 | 2026-09-15 | feat/chart-context-menu-layers | main | 21105 | 6 | `crates/app/src/replay_view.rs` (845) |
| 494 | 2026-09-15 | feat/sync-main-bars | campaign/outside-eight | 19233 | 10 | `crates/app/src/indicator_worker.rs` (624) |
| 497 | 2026-09-15 | feat/evidence-resources | campaign/outside-eight | 5670 | 4 | `crates/app/src/control/contract.rs` (943) |
| 498 | 2026-09-15 | feat/show-drawing-all-charts-toggle | main | 2171 | 2 | `crates/app/src/surfaces/drawing_chrome/mod.rs` (654) |
| 499 | 2026-09-17 | feat/edit-loop-windows | campaign/outside-eight | 0 | 0 | — |
| 500 | 2026-09-15 | feat/right-drag-fibonacci-actions | main | 35517 | 11 | `crates/app/src/toolbar.rs` (994) |
| 506 | 2026-09-15 | feat/mouse-vertical-line | main | 51075 | 25 | `crates/app/src/replay_get_data.rs` (999) |
| 507 | 2026-09-16 | chore/stop-tracking-agent-evidence | main | 4415 | 4 | `crates/guards/src/generated.rs` (524) |
| 508 | 2026-09-15 | fix/stale-control-descriptors | main | 1048 | 1 | — |
| 510 | 2026-09-15 | fix/context-column-divider-drag | main | 10613 | 5 | `crates/app/src/control/layout.rs` (646) |
| 511 | 2026-09-16 | feat/sync-main-chart-layout | campaign/outside-eight | 59771 | 52 | `crates/app/src/toolbar.rs` (994) |
| 514 | 2026-09-16 | feat/bar-selection-registry | campaign/outside-eight | 49257 | 41 | `crates/app/src/replay_get_data.rs` (999) |
| 515 | 2026-09-16 | feat/layer-policy-registry | campaign/outside-eight | 48977 | 47 | `crates/app/src/orderflow_view/settings.rs` (1075) |
| 516 | 2026-09-16 | fix/paste-without-text-clipboard | main | 2864 | 1 | `crates/app/src/surfaces/drawing_chrome/mod.rs` (652) |
| 519 | 2026-09-16 | feat/mcp-native-chart-image | main | 2096 | 5 | `crates/mcp/src/fake.rs` (196) |
| 522 | 2026-09-17 | feat/capability-admission-owner | campaign/outside-eight | 10142 | 5 | `crates/app/src/control/gateway.rs` (831) |
| 524 | 2026-09-17 | feat/sync-main-current | campaign/outside-eight | 61180 | 40 | `crates/app/src/replay_get_data.rs` (999) |
| 529 | 2026-09-18 | feat/app-owner-landing | campaign/outside-eight | 73843 | 168 | `crates/app/src/replay_get_data.rs` (999) |
| 530 | 2026-09-19 | feat/mvu-app-owners | campaign/outside-eight | 46575 | 71 | `crates/app/src/control/gateway/server.rs` (1204) |
| 531 | 2026-09-18 | feat/declared-stage-registry | campaign/outside-eight | 16839 | 17 | `crates/app/src/replay_view.rs` (845) |
| 532 | 2026-09-19 | feat/headless-app-core | campaign/outside-eight | 56699 | 155 | `crates/app/src/control/gateway/server.rs` (1204) |
| 533 | 2026-09-19 | feat/long-fn-commands | campaign/outside-eight | 42795 | 44 | `crates/app/src/toolbar.rs` (913) |
| 534 | 2026-09-19 | feat/harness-composition-root | campaign/outside-eight | 61967 | 83 | `crates/app/src/control/retry_matrix.rs` (963) |
| 542 | 2026-09-19 | campaign/outside-eight | main | 116263 | 408 | `crates/control-local/src/discovery.rs` (1048) |
| 543 | 2026-09-19 | fix/final-review-repairs | campaign/outside-eight | 33739 | 31 | `crates/app/src/replay_get_data.rs` (996) |
| 544 | 2026-09-19 | feat/architecture-ratchets | main | 4146 | 9 | `crates/guards/src/generated.rs` (524) |
| 547 | 2026-09-19 | feat/fast-draft-ci | main | 0 | 0 | — |
| 550 | 2026-09-19 | feat/read-cost-ledger | main | 0 | 0 | — |
| 553 | 2026-09-20 | perf/ci-under-seven-minutes | main | 0 | 0 | — |
| 554 | 2026-09-20 | feat/capability-family-registries | main | 6129 | 28 | `crates/control-schema/src/evidence.rs` (665) |
| 557 | 2026-09-20 | feat/pr-is-the-context | main | 0 | 0 | — |
| 562 | 2026-09-20 | fix/ship-gate-reads-advisories | main | 0 | 0 | — |
| 569 | 2026-09-21 | feat/mission-cost-harness | campaign/mission-velocity | 0 | 0 | — |
| 571 | 2026-09-21 | docs/mission-cost-baseline | campaign/mission-velocity | 0 | 0 | — |
| 572 | 2026-09-21 | docs/mission-cost-ranking | campaign/mission-velocity | 0 | 0 | — |
| 578 | 2026-09-21 | fix/mission-registry-transcripts | campaign/mission-velocity | 0 | 0 | — |
| 579 | 2026-09-21 | feat/velocity-experiment-protocol | campaign/mission-velocity | 0 | 0 | — |
| 580 | 2026-09-21 | perf/cap-agent-context | campaign/mission-velocity | 0 | 0 | — |
| 589 | 2026-09-21 | fix/poc-plate-price | campaign/mission-velocity | 2740 | 5 | `crates/app/src/drawings/mod.rs` (412) |

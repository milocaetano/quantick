# Mission: extract the order-flow engine into a headless crate `quantick-orderflow`

## Objective

Move the egui-free order-flow engine (`crates/app/src/orderflow_engine.rs` and
`crates/app/src/orderflow/{config,grouping,history,interaction,projection,scale,timeline}.rs`)
out of `crates/app` into a new headless crate `quantick-orderflow` under
`crates/orderflow`, as a pure move: the app keeps rendering, view, worker and
control projections and changes only `use` paths; tests travel with the files;
the crate graph, the `AGENTS.md` map, the `workspace_deps` guard and the size
baseline are updated.

Why it matters: `crates/app` holds 196k lines against 67k in the other sixteen
crates, and the order-flow engine is its largest egui-free island. Its own
`mod.rs` already states the invariant a crate boundary would enforce ("nothing
here depends on egui, a renderer, a wall clock or the network"). Today
`backtest` cannot consume it and the three open performance issues in this code
(#134, #155, #158) can only be benchmarked by building the desktop app. A crate
gives *one engine, three consumers* a chance to hold for order-flow analytics.

**Tier:** `medium`. A pure move of 14,423 lines pays twice in the diff and sits
far past the `small` ceiling, so `delivery-review` runs (completeness pass).
`arch-review` waives nothing. No question in step 3 is *a call that is the
trader's*, so the tier does not rise.

## Request ledger

- **R1** — Create a new crate `crates/orderflow`, package `quantick-orderflow`,
  edition and lints inherited from the workspace; dependencies are
  `quantick-engine` ("depends on `engine` only") plus whatever third-party
  crates the moved files already use, inherited from `[workspace.dependencies]`.
  "No `egui`, `tokio`, `std::time`."
- **R2** — Move the eight files "unchanged except for paths"; inline
  `#[cfg(test)]` modules travel with their files ("tests travel with the files").
- **R3** — `feed_lag_ms` moves with them; `metrics.rs` in app "re-exports or
  calls it so its other callers do not change".
- **R4** — `crates/app/src/orderflow/mod.rs` "disappears or becomes a one-line
  re-export — the mission decides and records why".
- **R5** — "Consumers change `use` paths only." `orderflow_worker.rs`,
  `orderflow_render.rs`, `orderflow_view.rs` and `control/orderflow.rs` stay in
  app: "the app keeps rendering, view, worker and control projections".
- **R6** — Registrations: root `Cargo.toml` members; `crates/app/Cargo.toml`
  dependency; `AGENTS.md` graph (`app --> orderflow`, `orderflow --> engine`)
  and a table row; `workspace_deps.rs` expected edges; size-baseline paths
  renamed; a short `crates/orderflow/README.md` "stating what it owns and that
  `backtest` may consume it".
- **R7** — Tighten the size baseline if any file shrank
  (`cargo run -p quantick-guards -- --tighten`).
- **R8** — Acceptance: "`grep -rnE 'egui|eframe|tokio|SystemTime|Instant::now'
  crates/orderflow/src` returns nothing", the grep quoted as evidence.
- **R9** — Acceptance: `cargo test -p quantick-orderflow` is green, "does not
  build `quantick-app`", and runs as many tests as lived in the eight files
  before the move, both counts as evidence.
- **R10** — Acceptance: "Every consumer's diff is `use`/path lines only",
  evidenced by `git diff origin/main...HEAD --stat` plus one consumer's diff.
- **R11** — Acceptance: `workspace_deps` green with the new edges; `AGENTS.md`
  map and table updated; `cargo test -p quantick-guards` green; size-baseline
  `!budget` unchanged or lower.
- **R12** — Acceptance: the four-check loop green;
  `crates/app/src/app/tests/orderflow_tests.rs` "unchanged except paths".
- **R13** — Purpose: the crate is what lets *one engine, three consumers* hold
  for order-flow analytics and lets the perf issues be benchmarked without the
  desktop app. Judges R1–R12: the result must be consumable from `backtest`
  without building `app`.
- **R14** — Out of scope, deliberately: `backtest` consuming the crate; any
  change to `orderflow_render.rs`, `orderflow_view.rs` or the perf issues;
  anything in `feed/`, `paper_trading.rs`, `harness.rs` or hook declarations;
  adding a `lib.rs` target to `crates/app`.
- **R15** — Respect parallel work: `fix/generated-truth` edits both baselines
  (whoever merges second rebases and re-runs `--tighten`);
  `refactor/pane-tests-out` edits `pane.rs` at its trailing test module, so this
  mission touches `pane.rs` "only at its `use` lines".
- **R16** — Process: read `C:\src\mission-orderflow-crate.md` in full first and
  build this ledger from it; re-check every evidence-ledger claim instead of
  trusting it.
- **R17** — Tier `medium`; `delivery-review` runs; `arch-review` waives nothing.

## Decisions taken by the trader

None. At `medium` a question is asked only where a wrong guess throws work
away; every doubt below is reversible in one edit, so each became an
assumption. The ones the full round would have asked are marked *wanted to
ask*.

## Assumptions

- **S1** *(wanted to ask)* — **The brief's claim of zero clock reads is false.**
  The moved files call `Instant::now()` seven times: once in the `project()`
  convenience wrapper (`orderflow_engine.rs:1062`), which feeds the
  projection-cache cadence and therefore *decides output*; twice as stopwatches
  inside `project_at` (`:1101`, `:1121`) whose results reach only the diagnostic
  counters `last_projection_ms` / `last_live_ms`; twice in tests (`:2094`,
  `:2168`) as the injected `now`; twice in `#[ignore]` bench tests in
  `projection.rs` (`:4824`, `:4899`) printing to stderr. R8's literal grep
  contradicts R2/R5's "pure move, `use` paths only" once the premise is false.
  Reading taken: the one read that decides output leaves the crate — the
  `project()` wrapper is deleted and `orderflow_worker.rs` passes
  `Instant::now()` to the existing clock-injected `project_at`, one line beyond
  a `use` path, declared in A6. The stopwatches stay, on the precedent
  `CLAUDE.md` grants `backtest`'s `main.rs` stopwatch: they measure a
  computation's duration, never a timestamp, and no output depends on them.
  R8 is graded as: `egui|eframe|tokio|SystemTime` empty, and every
  `Instant::now` occurrence enumerated with none deciding an output. Safe to
  assume: reversible in one edit whichever way the trader would have called it.
- **S2** — **The crate depends on `quantick-orderbook` as well as
  `quantick-engine`.** `DepthEvent`, `BookLevel`, `DepthStatus`,
  `DepthResyncReason`, `BookSnapshot` are the engine's input vocabulary (eight
  `use quantick_orderbook` lines across the set). The brief's claim 4 grepped
  for `quantick_engine` and missed them. No alternative exists short of
  duplicating the types, and `orderbook` is a headless leaf, so the edge
  `orderflow --> orderbook` is added beside `orderflow --> engine` in the map
  and in `workspace_deps.rs`. A fact, not a choice.
- **S3** — `tracing` stays as a dependency: the moved engine logs through it,
  it is a facade with no clock or I/O of its own, and the three feeds already
  carry it below `app`. `rust_decimal` and `serde` (config derives) likewise.
- **S4** — **`mod.rs` disappears** (R4). Its facade becomes the crate root
  `lib.rs`; `orderflow_engine.rs` becomes `crates/orderflow/src/engine.rs`,
  reached as `quantick_orderflow::engine`. Why not a one-line re-export in app:
  it would keep a second name (`crate::orderflow`) alive for the same thing,
  and the `use` lines the consumers change anyway are the whole cost of not
  having it. `main.rs` drops `mod orderflow;` and `mod orderflow_engine;`.
- **S5** — `pub(crate)` items in the moved files become `pub`: all 13 left in
  the engine module after S1 (its public surface is the crate's), and the three
  in `projection.rs` the chart reaches (`HeatmapProjection::empty`,
  `normalized_log_intensity`, `normalized_area_size`). A crate boundary forces
  this; it is visibility, not behaviour. Three more things the compiler
  demanded, found while building: `HeatmapProjection::empty` loses its
  `#[cfg(test)]` gate, because the render tests that call it live in `app`,
  which links this crate as a plain dependency; the engine's own tests keep a
  test-module-only `project(request)` shorthand that reads `Instant::now()`
  (twelve call sites, all tests, the same category as the two test reads S1
  already lists); and `toml` joins as a dev-dependency because the config
  round-trip tests parse the TOML the chart persists.
- **S6** — `feed_lag_ms` lands in `crates/orderflow/src/lib.rs` with its test
  `lag_is_observation_minus_event_time` moved from `metrics.rs`; `metrics.rs`
  re-exports it (`pub use quantick_orderflow::feed_lag_ms;`) so `tab.rs` and
  `app.rs` do not change. Conventional placement.
- **S7** — Size baseline: the two entries (`orderflow/projection.rs 1850`,
  `orderflow/config.rs 1523`) change path only; ceilings and `!budget` do not
  move unless `--tighten` lowers them. Context baseline: `AGENTS.md` sits at
  its ceiling exactly (12,825 bytes), so the map row and edges are paid for by
  trimming equal weight from `AGENTS.md` prose, never by a raise.
- **S8** — `CLAUDE.md` names `orderflow` in the headless list and the
  dependency-direction line: `workspace_deps::claude_md_lists_every_crate`
  demands the backticked name of every crate, so this is a registration, not
  a docs change of choice. The `deny.toml` comment counting "seventeen"
  `quantick-*` crates becomes "eighteen".
- **S9** — The performance gate is met by the two `#[ignore]` bench tests
  already in `projection.rs` (`bench_projection_over_a_dense_tape`,
  `bench_the_live_half_under_the_live_lane_pie_preset`), run before the move
  in `quantick-app` and after it in `quantick-orderflow`. A crate boundary is
  the only codegen change a pure move can make, and these two are the paths
  the boundary could affect.
- **S10** — Consumer edits beyond `use` lines are exactly: the one worker line
  from S1, the `metrics.rs` re-export from S6, and the two `mod` deletions from
  S4. Anything else found necessary is a finding, not an assumption.

## Acceptance criteria

- [x] **A1** — `crates/orderflow/Cargo.toml` declares package
      `quantick-orderflow`, inherits edition and lints from the workspace, and
      depends on `quantick-engine`, `quantick-orderbook`, `rust_decimal`,
      `serde`, `tracing` and nothing else, with `toml` as its one
      dev-dependency — no `egui`, `eframe`, `tokio`.
      *Evidence:* the manifest quoted; `cargo tree -p quantick-orderflow -e normal --depth 1`.
      → PR body. *(R1, R13)*
- [x] **A2** — The eight files live under `crates/orderflow/src/` and
      `git diff -M origin/main...HEAD` shows each as a rename whose hunks are
      path lines (`crate::orderflow` → `crate`, `crate::metrics` → `crate`),
      visibility (`pub(crate)` → `pub`) and the S1 wrapper deletion, nothing
      else.
      *Evidence:* `git diff -M --stat` and the per-file rename diffs, which
      live on the branch itself. → PR body (stat and a summary per file). *(R2)*
- [x] **A3** — Every test travelled: the eight files hold 185 `#[test]`
      attributes before and after; `cargo test -p quantick-orderflow` passes and
      runs the same number of tests that `cargo test -p quantick-app -- --list`
      listed under `orderflow::` and `orderflow_engine::` before the move, plus
      the one moved `feed_lag_ms` test; its build never prints
      `Compiling quantick-app`.
      *Evidence:* both `--list` counts and the test summary line.
      → PR body. *(R2, R9)*
- [x] **A4** — `feed_lag_ms` is defined in `quantick_orderflow` and
      `crates/app/src/metrics.rs` re-exports it; `tab.rs` and `app.rs` show no
      diff for it.
      *Evidence:* `grep -rn feed_lag_ms crates/` quoted. → PR body. *(R3)*
- [x] **A5** — `crates/app/src/orderflow/mod.rs` no longer exists, and S4
      records why.
      *Evidence:* `git diff --stat` shows the deletion; S4 above. → PR body. *(R4)*
- [x] **A6** — Every consumer in `app` changes only `use`/path lines, with
      exactly the four declared exceptions of S10; `orderflow_worker.rs`,
      `orderflow_render.rs`, `orderflow_view.rs`, `control/orderflow.rs` remain
      in `crates/app/src`.
      *Evidence:* `git diff origin/main...HEAD --stat` and the full diff of
      `crates/app/src/control/orderflow.rs` and `orderflow_worker.rs` quoted.
      → PR body. *(R5, R10)*
- [x] **A7** — Registrations landed: root `Cargo.toml` lists
      `crates/orderflow`; `crates/app/Cargo.toml` depends on it; `AGENTS.md`
      has edges `app --> orderflow`, `orderflow --> engine`,
      `orderflow --> orderbook` and a table row; `workspace_deps.rs` `ALLOWED`
      has `("orderflow", &["engine", "orderbook"])`; both size-baseline paths
      renamed; `crates/orderflow/README.md` states what it owns and that
      `backtest` may consume it.
      *Evidence:* `cargo test -p quantick-pine --test workspace_deps` green;
      the README quoted. → PR body. *(R6, R11)*
- [x] **A8** — `grep -rnE 'egui|eframe|tokio|SystemTime' crates/orderflow/src`
      returns nothing, and `grep -rnE 'Instant::now' crates/orderflow/src`
      returns only the two stopwatch reads inside `project_at`, the test-only
      `project` shorthand of S5, the two test reads and the two bench reads S1
      enumerates — no production `project()` wrapper.
      *Evidence:* both greps quoted. → PR body. *(R8, R1)*
- [x] **A9** — `cargo test -p quantick-guards` green; `--tighten` run and its
      output recorded; size-baseline `!budget` unchanged or lower; context
      baseline `!budget` unchanged.
      *Evidence:* both outputs quoted. → PR body. *(R7, R11)*
- [x] **A10** — `crates/app/src/app/tests/orderflow_tests.rs` diff is path
      lines only.
      *Evidence:* its `git diff` quoted. → PR body. *(R12)*
- [x] **A11** — Nothing out of scope moved: no diff under `crates/app/src/feed/`,
      `paper_trading.rs`, `harness.rs`, hook declarations, `crates/backtest/`;
      no `crates/app/src/lib.rs`; `orderflow_render.rs`, `orderflow_view.rs` and
      `pane.rs` differ in `use` lines only.
      *Evidence:* `git diff --stat` and the three files' diffs quoted.
      → PR body. *(R14, R15)*

### Injected gates

- [x] **G1** — Every artifact in English (`CLAUDE.md` owns the rule).
      *Evidence:* `arch-review` dimension 8 verdict; `cargo test -p quantick-guards`.
      → arch-review report in the PR body.
- [x] **G2** — Four checks green after rebasing on latest `main`: `cargo fmt
      --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build
      --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* exit codes quoted. → PR body verification boxes.
- [x] **G3** — Performance impact declared and measured. Rates: engine ingest
      per-trade / per-depth-event; projection per-frame (cache-gated); the crate
      boundary is the only codegen change. Evidence that it is flat: the two
      `projection.rs` bench tests' `BENCH … ms_per_frame` lines before (in
      `quantick-app`) and after (in `quantick-orderflow`).
      *Evidence:* both bench outputs. → PR body.
- [x] **G4** — `arch-review` run at `medium` over `git diff origin/main...HEAD`,
      every Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the `arch-review-ok` marker and the report. → PR body.

## Evidence recorded (2026-09-04, before the reviews)

- **A1** — `crates/orderflow/Cargo.toml`: `quantick-engine`, `quantick-orderbook`,
  `rust_decimal`, `serde`, `tracing`; dev-dependency `toml`; `[lints] workspace = true`.
- **A2** — `git diff -M --stat origin/main...HEAD`: nine renames (eight files plus
  `mod.rs` → `lib.rs`); hunks are `crate::orderflow::` → `crate::`, `crate::metrics::`
  → `crate::`, `pub(crate)` → `pub`, the S1 wrapper, the S5 items.
- **A3** — before: `cargo test -p quantick-app -- --list` under `orderflow::` /
  `orderflow_engine::` = 185 (34 in the engine); `#[test]` count 185. After:
  `cargo test -p quantick-orderflow` → `184 passed; 0 failed; 2 ignored` = 186 =
  185 + `lag_is_observation_minus_event_time`; no `Compiling quantick-app` line.
  `quantick-app` went from 2,190 to 2,004 listed tests (−186).
- **A4** — `metrics.rs:102 pub use quantick_orderflow::feed_lag_ms;`; `tab.rs:3267`
  and `app.rs:5251` unchanged.
- **A5** — `crates/app/src/orderflow/mod.rs` renamed to `crates/orderflow/src/lib.rs`
  (git rename detection), S4 records why.
- **A6** — 14 consumers in `crates/app/src`; every hunk a `use`/path line, plus:
  `orderflow_worker.rs:197` `project_at(request, Instant::now())`, `metrics.rs`
  re-export, `main.rs` two `mod` lines, and a rustfmt reflow of the `use crate::{}`
  group in `control/orderflow.rs`.
- **A7** — `cargo test -p quantick-pine --test workspace_deps`: 6 passed.
- **A8** — `grep -rnE 'egui|eframe|tokio|SystemTime' crates/orderflow/src`: two doc
  comment lines saying "no egui", no code. `Instant::now`: `engine.rs:1099`,
  `:1119` (stopwatches), `:1404` (test-module shorthand), `:2101`, `:2175` (tests),
  `projection.rs:4816`, `:4891` (bench tests).
- **A9** — `cargo test -p quantick-guards`: 86 + 6 + 5 passed. `--tighten`: nothing
  to tighten in any ratchet. Size `!budget 61397` unchanged; context `!budget
  231950` unchanged; `AGENTS.md` 12,740 of 12,825; `CLAUDE.md` 9,988 (no entry).
- **A10** — `orderflow_tests.rs`: no diff at all (it reached the engine through
  `super::*`, which still resolves).
- **A11** — no diff under `feed/`, `paper_trading.rs`, `harness.rs`, `crates/backtest`;
  no `crates/app/src/lib.rs`; `orderflow_render.rs`, `orderflow_view.rs`, `pane.rs`
  diffs are `use`/path lines (the four `pane.rs` test-module lines are paths).
- **G2** — `cargo fmt --all -- --check` 0; `cargo clippy --workspace --all-targets` 0;
  `cargo build --workspace` 0; `cargo test --workspace` 0, each on its own.
- **G3** — `bench_projection_over_a_dense_tape`: before 71.3 / 69.6 / 71.4 ms,
  after 62.8 / 63.6 / 62.0 ms. `bench_the_live_half_under_the_live_lane_pie_preset`:
  before 0.375 / 0.520 / 0.567 ms, after 0.383 / 0.381 / 0.324 ms. Flat or better.
- **G1, G4** — the arch-review report, after this file is archived.

## Not applicable, and why

- **Touches anything user-visible** (`ui-harness`, `visual-qa`,
  `trader-ux-review`): no pixel changes. Rendering and view code stay in `app`
  and change `use` lines only; no surface is added or altered.
- **Adds a capability — crate** (`new-extension`): the crate is a move, not a
  new capability docking through a port. The parts of the recipe that do apply
  are held by A7 (registration-only edits) and the PR body (blast radius);
  "port named" and "fake second implementation" have nothing to name or fake.
- **Adds something a trader does**: nothing new is doable.
- **Engine / determinism, test-first**: no behaviour is written; the golden
  tests move unchanged (A3).
- **Docs/skills only**: not a docs change; the full shape pass runs.

## Closing steps

- **C1** — `delivery-review` completeness pass returns PASS (medium tier).
- **C2** — PR open, body naming the tier `medium` beside the four verification
  boxes, with every criterion's evidence.

## The request as received

Quoted verbatim, untranslated, as the one marked attribution `CLAUDE.md`'s
language rule allows: the words are the data the ledger was cut from, and a
translation could drop an ask no reviewer could then find. The trader's
invocation was in English already.

> /mission medium refactor/orderflow-crate — extract the egui-free order-flow
> engine (orderflow_engine.rs and
> crates/app/src/orderflow/{config,grouping,history,interaction,projection,scale,timeline}.rs,
> 14,423 lines) out of crates/app into a new headless crate `quantick-orderflow`
> that depends on `engine` only. A pure move: the app keeps rendering, view,
> worker and control projections and changes only `use` paths; tests travel
> with the files; the crate graph, AGENTS.md map, workspace_deps guard and size
> baseline are updated. Read C:\src\mission-orderflow-crate.md in full before
> anything else and build the request ledger from it.

The brief it points at, `C:\src\mission-orderflow-crate.md`, is not in the
repository; its *Scope*, *Acceptance criteria*, *Out of scope* and *Parallel
work* sections are what R1–R15 restate, each ask carrying the brief's own
words where they decide something.

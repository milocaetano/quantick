# Mission: one bar-spec vocabulary, owned by the engine

**Objective.** Move the single bar-spec vocabulary into `quantick-engine` so the
chart and the backtest build bars from one definition, proven by a parity test,
with the backtest refusing deal-count bars honestly.

**Why.** CLAUDE.md's "one engine, three consumers — never fork bar-building per
consumer" is broken today: `BarSpec` exists twice (`crates/app/src/state.rs:180`
and `crates/backtest/src/bars.rs:22`), the backtest copy lacks `Trades(u64)`, and
its own doc (`bars.rs:7-12`) names the engine as the right home. Three blind
assessments on 2026-09-14 cited this duplicate as a counterexample (quantick-score
SE1, SE3, SE4) and outside-score A6 cites the central `BarSpec` match.

**Tier:** `medium` — a cross-crate move touching the engine's public surface,
two consumers and 38 importing files in `app`; more than a `small` diff, no
money, safety or UI decision.

## Request ledger

- **R1** — One `BarSpec` vocabulary lives in headless `quantick-engine` (enum,
  `parse`, `to_config_string`, `kind`, `clamped`, `build` dispatch, plus the
  `BarKind` it names); `app` and `backtest` import it and the backtest duplicate
  is deleted.
- **R2** — The backtest refuses a `trades:N` spec with a typed, labelled error,
  because recordings carry no deal counter — never by building something else.
- **R3** — Chart and backtest are proven to cut identical bars for every shared
  spec kind from one fixture tape.
- **R4** — Engine code is test-first.
- **R5** — The config string format, the control-plane schemas and every
  existing golden stay byte-identical; the size, cycle, UI-free and context
  ratchets do not grow.
- **R6** — Purpose, which judges the rest: "eu quero melhorar a arquitetura do
  quantick" — the change must remove the forked bar vocabulary from the code,
  not describe it.

## Decisions

None asked: nothing qualified under step 3 that the code or a repo default does
not answer.

## Assumptions

- **S1** — The parity proof is transitive, not one test linking both crates:
  `backtest` may not depend on `app` and nothing may depend on `backtest`
  (CLAUDE.md, *Leaves stay leaves*). So the engine asserts `BarSpec::build` against
  the existing hand-computed goldens, the app asserts `ChartState` against the
  same CSVs, and the backtest asserts the bars its strategy is shown against the
  same CSVs. Safe: every consumer is held to one committed file per kind.
- **S2** — The backtest inherits the chart's parse rules, including the time
  bounds `100ms..=1d`. Before, the backtest accepted any positive interval.
  Safe: one vocabulary is the ask, and no fixture or test uses an interval outside
  the bounds.
- **S3** — The backtest's run summary for a volume or dollar spec prints the
  decimal as the chart does (`volume(5.50)`), not normalized (`volume(5.5)`).
  Safe: a display string only, no golden asserts it, and the chart's form is the
  one a trader reads off a tab.
- **S4** — `run_session` panics with a stated reason if handed a `Trades` spec
  programmatically. The typed refusal sits at the parser, the only entry for a
  spec from outside the crate. Safe: a programmer error, documented under
  `# Panics`, unreachable from the CLI.
- **S5** — `BarKind::default_spec` moves too, so the engine owns the default
  time interval (60 s). `time_header::DEFAULT_INTERVAL_MS` stays as a re-export
  of the engine constant, so no caller changes. Safe: the value is unchanged and
  asserted.

## Acceptance criteria

- [x] **A1** — Exactly one `enum BarSpec` and one `enum BarKind` exist in the
      workspace, in `crates/engine/src/spec.rs`; `crates/backtest/src/bars.rs`
      no longer defines a bar enum or a parser.
      *Evidence:* `git grep -n "enum BarSpec\|enum BarKind" -- crates` prints one
      line each, both under `crates/engine`.
      → PR body, *Evidence*. *(R1, R6)*
- [x] **A2** — `app` and `backtest` compile against the engine's `BarSpec`; the
      app's `crate::state` paths keep working through a re-export.
      *Evidence:* the four checks green.
      → PR body, *Verification loop*. *(R1)*
- [x] **A3** — The engine tests pinning the vocabulary (parse, config round
      trip, refusals, clamping, kind list, dispatch against the goldens) are
      committed before the engine code they test.
      *Evidence:* `git log --reverse --format='%h %s' origin/main..HEAD` shows the
      test commit before the implementation commit, and the test file failed to
      compile at that commit.
      → PR body, *Test-first*. *(R4)*
- [x] **A4** — `BarSpec::build` cuts each existing golden fixture (tick n3,
      volume t5, dollar t500, time 1s, imbalance t8) into exactly its committed
      expected bars.
      *Evidence:* `cargo test -p quantick-engine --test bar_spec` green.
      → PR body, *Evidence*. *(R3, R4)*
- [x] **A5** — The app's `ChartState`, given the same spec strings and fixture
      trades, produces exactly those expected bars.
      *Evidence:* a named `quantick-app` test green, reading the engine's
      fixture files.
      → PR body, *Evidence*. *(R3)*
- [x] **A6** — The backtest's `run_session`, given the same spec strings and
      fixture trades, shows its strategy exactly those expected bars.
      *Evidence:* a named `quantick-backtest` test green, reading the same files.
      → PR body, *Evidence*. *(R3)*
- [x] **A7** — `--bars trades:2000` is refused by the backtest with a typed error
      whose message names the missing deal counter and does not call the kind
      unknown.
      *Evidence:* a named backtest test asserting the error variant and message.
      → PR body, *Evidence*. *(R2)*
- [x] **A8** — Existing goldens, the committed control-plane schemas and the
      config round trip are unchanged.
      *Evidence:* `git diff --stat origin/main...HEAD -- crates/engine/tests/fixtures schemas`
      is empty, and `every_bar_spec_survives_the_config_round_trip` passes.
      → PR body, *Evidence*. *(R5)*
- [x] **A9** — No ratchet grows: size, cycle, UI-free, extension roots and
      context are at or below their `origin/main` values, and any shrink is
      tightened.
      *Evidence:* `cargo run -p quantick-guards -- --report` before and after,
      diffed, and `cargo test -p quantick-guards` green.
      → PR body, *Ratchets*. *(R5, R6)*

### Injected gates

- [ ] **G1** — Every artifact is in English (CLAUDE.md), graded by `arch-review`
      dimension 8 and the language guard.
      → arch-review report on the PR.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace` and `cargo test --workspace` green after rebasing
      on the latest `main`, each run on its own.
      → PR body, *Verification loop*.
- [x] **G3** — Performance impact declared: `BarSpec::build`, `parse` and
      `clamped` run on a spec change, a rebuild or a session start (rare); the
      per-trade `push` path and every builder are untouched, only moved callers.
      → PR body, *Performance*.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, every Blocker
      and Should-fix resolved or deferred in the PR body.
      → arch-review report on the PR.
- [x] **G5** — Engine/determinism territory: fixture-backed tests written first
      (A3); the goldens guard determinism through `golden::assert_golden`.
      → PR body, *Test-first*.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

The G-AI evidence lands on the PR: the ai-review report comment and the
`ai_review_threads.sh list` output in the final verifier's reconciliation.

### Not applicable

- *Touches a hot path*: the builders and `push` are not edited; `build` is
  called on rebuild, not per trade.
- *Touches anything user-visible*: no surface, label or control changes; the
  toolbar reads the same `BarKind` through the re-export.
- *Adds a capability*: no new bar type, feed or layer; the vocabulary moves.
- *Adds something a trader does*: no new action.

## Evidence recorded before review

- **A1** — `git grep -n "enum BarSpec\|enum BarKind" -- crates` prints
  `crates/engine/src/spec.rs:44` and `:153` only.
- **A2, G2** — at `7f46592b`: `cargo fmt --all -- --check` exit 0; `cargo clippy
  --workspace --all-targets` exit 0; `cargo build --workspace` exit 0 (the first
  attempt failed writing a build-script object, `no such file or directory`, with
  665 GB free; the unchanged rerun passed); `cargo test --workspace` exit 0,
  3,888 passed, 0 failed, 19 ignored. Each run on its own.
- **A3, G5** — `799fe374 test(engine): pin one bar-spec vocabulary before it
  moves` precedes `0dd6728b feat(engine): own the bar-spec vocabulary`; at
  `799fe374` the test failed to compile with `E0432: unresolved imports
  quantick_engine::BarKind, quantick_engine::BarSpec, ...`.
- **A4** — `cargo test -p quantick-engine --test bar_spec`: 12 passed, including
  `the_builder_a_spec_names_cuts_each_golden_exactly`.
- **A5** — `state::bar_spec_parity_tests::the_chart_cuts_every_golden_the_engine_pins ... ok`.
- **A6** — `the_backtest_cuts_every_golden_the_chart_cuts` passed in
  `quantick-backtest`'s `harness` suite.
- **A7** — `bars::tests::trades_is_refused_for_the_reading_it_lacks_not_as_unknown`
  asserts `SpecError::NeedsDealCounter(BarSpec::Trades(2000))` and the message.
- **A8** — `git diff --stat origin/main...HEAD -- crates/engine/tests/fixtures schemas`
  is empty; `every_bar_spec_survives_the_config_round_trip` passes in the engine.
- **A9** — guards report before/after: `crate.lines.app` 120748 → 120435,
  `crate.lines.backtest` 2408 → 2307, `crate.lines.engine` 4506 → 4906, total
  197907 → 197893; `ratchet.app-ui-free` 47665 → 47351 (tightened); size,
  cycle, extension-boundary and context budgets unchanged;
  `cargo test -p quantick-guards` green.
- **G3** — rates: `BarSpec::parse`, `clamped` and `build` run on a spec change, a
  rebuild or a session start (rare). No builder and no `push` path is edited;
  the clippy-driven edits replace `.clone()` with a copy of the same value.

## Closing steps

- **C1** — `delivery-review` completeness pass (inline, `medium`) PASS.
- **C2** — The PR is open, non-draft, on `refactor/one-bar-spec`.
- **C3** — `mission_ship_gate.sh mission <pr>` PASS at the final head.

## Verbatim request

> "qual meta facil para bater vamos começar por ela"
>
> "vc nem ta mexendo no codigo, eu quero melhroar a arquitetura do qauantick"
>
> — the trader, 2026-09-14, answered with the objective above: one `BarSpec`
> in the engine, a parity test, and an honest backtest refusal for `trades:N`.

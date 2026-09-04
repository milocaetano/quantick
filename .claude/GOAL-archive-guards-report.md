# Mission: `quantick-guards --report`

## Objective

Add a `--report` mode to `quantick-guards` that prints, deterministically and
with no dependency, the repository health numbers this refactor sprint is being
judged on, as one stable text table with one line per number, so a session can
quote it before and after a merge instead of re-deriving twelve numbers by hand
with `wc`, `grep` and `awk` at open-judgement rates.

**Tier:** `medium`. A new CLI mode with its own measurement code, new tests and
a `CLAUDE.md` line is past the `small` diff ceiling, and the mode's output is a
public artifact the sprint will quote — it earns a completeness pass. It is not
`high`: nothing here is user-visible, nothing touches the engine, and the whole
change lives inside a leaf crate with no dependants.

## Request ledger

| # | Ask | Verbatim fragment where the wording carries it |
| --- | --- | --- |
| R1 | Add `--report` as a third mode beside `--file <path>` and `--tighten`, and update the hand-written usage string. | "a third mode beside the two in ledger #1, with the usage string updated" |
| R2 | Output is plain text, one `label<TAB>value` line per number, in a fixed sort order, with no timestamps and no paths outside the workspace. | "one `label<TAB>value` line per number, sorted by a fixed order, no timestamps, no paths outside the workspace" |
| R3 | Two runs on the same tree are byte-identical, so `diff` between two commits is the report of what a merge changed. | "two runs on the same tree are byte-identical and `diff` between two commits is the report of what a merge changed" |
| R4 | Report production lines per crate (via `size::production_lines`), the workspace total, and `app` as a percentage. | "production lines per crate […] the workspace total, and `app` as a percentage" |
| R5 | Report the eight largest production files. | "the eight largest production files" |
| R6 | Report every `pub struct` in `crates/*/src` with 30 or more fields, by name and field count, where a field is a `name:` line at one indent inside the struct body. | "a field is a `name:` line at one indent inside the struct body" |
| R7 | Report each ratchet's recorded `!budget` against the total it caps. | "both ratchets: recorded `!budget` and the measured total it caps" |
| R8 | Report counts in production source of `#[allow(`, `process::id()` and `#[cfg(test)]` outside `tests/` directories and `tests.rs` files. | "counts in production source of `#[allow(`, `process::id()`, and `#[cfg(test)]` outside `tests/` directories and `tests.rs` files" |
| R9 | Report lines in `crates/app/src` production files that contain no `egui` identifier, as the remaining extraction headroom. | "as the remaining extraction headroom" |
| R10 | Test the struct-field counter and the production-only counting on fixture strings, in `crates/guards/tests/guards.rs`. | "Tests for the struct-field counter and the production-only counting on fixture strings" |
| R11 | Test that `--report` run twice on the real tree produces identical output. | "one test that runs `--report` twice on the real tree and asserts the outputs are identical" |
| R12 | Add one sentence to `CLAUDE.md`'s verification-loop paragraph naming the mode, paid for inside the existing context budget by finding the bytes elsewhere in the same change. | "paid for inside the context budget (find the bytes elsewhere in the same change; the guard will say if you did not)" |
| R13 | Record today's report in this goal file, so the sprint's "before" is in the archive. | "so the sprint's 'before' is in the archive" |
| R14 | `--report` completes in under two seconds on a warm build. | "completes in under two seconds on a warm build" |
| R15 | The report's numbers agree with the brief's hand measurements to the line, or the PR body explains each difference as a rule chosen differently, never a bug. | "or the PR body explains each difference (a rule chosen differently, never a bug)" |
| R16 | `crates/guards/Cargo.toml` `[dependencies]` stays empty. | "stays empty" |
| R17 | Do not ratchet any of the new numbers; no JSON, history or plotting; no change to `size.rs` / `context.rs` behaviour beyond reuse. | "Out of scope, deliberately" |
| R18 | Purpose: a session can quote the same numbers before and after a merge, so "did it improve?" is answered by a number rather than re-derived by hand each mission. | "so that a session can quote it before and after a merge" |

## Decisions taken by the trader

- **D1** — The report iterates the `GUARDS` registry and prints a line per
  ratchet it finds, not only the two the brief named. The tree has three today
  (`size`, `context`, `cycle`); a fourth appears in the report without an edit.
  This widens R7 from "both ratchets" to "every ratchet".
- **D2** — Each ratchet prints three numbers: the signed `!budget`, the
  `recorded` total of its ceilings (which is what the budget actually caps),
  and the `measured` total its files weigh today. The slack between `recorded`
  and `measured` is the debt the sprint is paying down, and showing it is the
  point of diffing two reports.

## Assumptions

- **S1** — `--report` exits 0 whenever it could read the tree, regardless of
  what any guard would find. It is a measurement mode, not a check; making it
  exit non-zero would make it unusable inside a `diff` pipeline. Safe to
  assume: the brief calls it a report, and the existing modes already own the
  pass/fail question.
- **S2** — "Production" throughout means `size::production_source` over the
  files `size`'s own `tracked()` accepts (`crates/**/*.rs`, excluding any
  `tests/` or `target/` path segment), reusing the existing definition rather
  than writing a second one. Safe to assume: R17 asks for reuse, and two
  definitions of "production" drifting apart is the defect this repository
  files against its own code.
- **S3** — "Per crate" means the directory immediately under `crates/`, which
  is how both baselines already spell paths. Conventional default.
- **S4** — The `#[cfg(test)]` count in R8 counts occurrences in tracked files
  that are not under a `tests/` directory and are not named `tests.rs`, over
  the *whole* file text rather than over production-only text — counting
  `#[cfg(test)]` inside production-only text would score zero by construction,
  since stripping exactly those items is what `production_source` does. Safe
  to assume: the ask is meaningless under the other reading.
- **S5** — Struct field counting (R6) is a line-shaped rule, as the brief
  spells it: a `name:` line at one indent inside a `pub struct` body. It is
  not a Rust parser, and the tests pin the rule rather than a parse. Safe to
  assume: the brief states the rule.
- **S6** — Row labels are stable identifiers, and rows whose *set* is
  data-driven (per crate, largest files, wide structs) sort by a fixed key, so
  a crate appearing or a file shrinking moves exactly the lines it should.
  Conventional default for a diffable report.
- **S7** *(wanted to ask; the `medium` question budget was spent on D1 and
  D2)* — The new measurement code lands in a new module
  `crates/guards/src/report.rs` rather than growing `main.rs` or `size.rs`.
  Reading chosen: `CLAUDE.md`'s "a capability docks as a new file plus one
  registration line". Reversible in one move if the trader wants it elsewhere.
- **S8** *(wanted to ask)* — The percentage in R4 is printed as an integer
  percent, computed with integer arithmetic. A fractional percent invites a
  formatting decision that can differ between float paths; an integer is
  byte-stable by construction.

## Acceptance criteria

- [ ] **A1** — `cargo run -q -p quantick-guards -- --report` exits 0 and
      prints a `label<TAB>value` table covering every number in R4–R9.
      *Evidence:* the captured output, quoted in full.
      → the PR body, and the baseline section of this file. *(R1, R2, R4, R5,
      R6, R7, R8, R9)*
- [ ] **A2** — The usage string names three modes and still says they are
      alternatives; `--report` with extra arguments is refused, not ignored.
      *Evidence:* the usage text, and a test asserting the refusal.
      → `crates/guards/tests/guards.rs`, quoted in the PR body. *(R1)*
- [ ] **A3** — Two consecutive `--report` runs on the same tree produce
      byte-identical output. *Evidence:* an empty `diff` between two captured
      runs, plus a test that runs the report twice and asserts equality.
      → `crates/guards/tests/guards.rs`, `diff` output in the PR body.
      *(R3, R11)*
- [ ] **A4** — Every ratchet in `GUARDS` prints its `!budget`, its `recorded`
      total and its `measured` total. *Evidence:* three lines per ratchet in
      the output, for all three ratchets present today.
      → the PR body. *(R7, D1, D2)*
- [ ] **A5** — The struct-field counter and the production-only counting are
      each tested on fixture strings, including the cases the rule turns on: a
      nested type at deeper indent, a `#[cfg(test)]` item above a test module,
      a non-`pub` struct. *Evidence:* named tests, green.
      → `crates/guards/tests/guards.rs`. *(R6, R10)*
- [ ] **A6** — `--report` on a warm build completes in under two seconds.
      *Evidence:* a timed run's wall clock.
      → the PR body. *(R14)*
- [ ] **A7** — Each number in the brief's hand measurements is placed
      side-by-side with the report's number, and every difference is explained
      as a rule chosen differently. *Evidence:* the side-by-side table.
      → the PR body. *(R15)*
- [ ] **A8** — `crates/guards/Cargo.toml` `[dependencies]` is still empty.
      *Evidence:* the file, quoted. → the PR body. *(R16)*
- [ ] **A9** — `CLAUDE.md`'s verification-loop paragraph names `--report` in
      one sentence, and the context ratchet is green without its `!budget`
      being raised. *Evidence:* `cargo test -p quantick-guards` green, and the
      `CLAUDE.md` diff showing the bytes paid for elsewhere.
      → the PR body. *(R12)*
- [ ] **A10** — Today's report is pasted verbatim into this file's baseline
      section before it is archived. *Evidence:* the archived
      `GOAL-archive-guards-report.md`. *(R13, R18)*
- [ ] **A11** — Nothing new is ratcheted, no JSON, history or plotting is
      added, and `size.rs` / `context.rs` behaviour is unchanged beyond new
      reuse. *Evidence:* the diff of those two files carries no behaviour
      change. → the PR body. *(R17)*
- [ ] **G1** — Every artifact in the change is English.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* each command's output. → the PR body.
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate. *Evidence:* the classification. → the PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, with
      every Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the review verdict. → the PR body.

## Not applicable, and why

- **Hot path** — nothing here runs per trade, per depth update or per frame.
  `--report` is a rare, operator-invoked tree walk; A6 bounds it anyway.
- **User-visible surface** — no UI is touched, so `ui-harness`, `visual-qa`
  and `trader-ux-review` have no surface to reach. The mode's only output is
  stdout text.
- **`new-extension`** — this adds a CLI mode to a leaf binary, not a feed, bar
  type, indicator, layer, panel or crate. The registry it docks against is the
  existing `main.rs` mode dispatch and the existing `GUARDS` list.
- **Second operator** — the capability *is* a named call with a readable
  result; there is no mouse path to build an alternative to.
- **Engine / determinism** — no engine code is touched. Determinism is
  nonetheless a first-class ask here (R3), tested by A3.
- **Docs/skills-only waiver** — does not apply. This change ships Rust, so the
  full shape pass is owed despite the one-line `CLAUDE.md` edit.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the tier beside the four verification boxes.

## The sprint baseline

`cargo run -q -p quantick-guards -- --report` on this branch, rebased onto
`origin/main` at `6d3e792` on 2026-09-04. This is the sprint's "before": a
later session runs the same command and diffs the two.

The branch was cut at `d551813` and rebased once, which the report itself
recorded: `site.process_id` read 7 at `d551813` and reads 11 here, because
PR #290 landed production scratch-directory helpers in `guards` and
`replay` in between. That is the mode working — four new sites, named by one
diffed line, with nobody re-deriving anything by hand.

```
crate.lines.app	114392
crate.lines.backtest	2400
crate.lines.control	5184
crate.lines.control-local	1612
crate.lines.engine	3375
crate.lines.feed-binance	2914
crate.lines.feed-hyperliquid	1773
crate.lines.feed-mt5	4594
crate.lines.guards	4393
crate.lines.indicators	3878
crate.lines.mcp	2422
crate.lines.orderbook	1022
crate.lines.orderflow	7701
crate.lines.pine	6123
crate.lines.replay	3787
crate.lines.sim	2279
crate.lines.strategy	1879
crate.lines.trading	1789
crate.lines.total	171517
crate.lines.app_percent	66
file.largest.crates/app/src/app.rs	9232
file.largest.crates/app/src/control/gateway.rs	4142
file.largest.crates/app/src/orderflow_render.rs	3075
file.largest.crates/app/src/orderflow_view.rs	2485
file.largest.crates/app/src/pane.rs	7771
file.largest.crates/app/src/paper_report.rs	3300
file.largest.crates/app/src/paper_trading.rs	6265
file.largest.crates/app/src/tab.rs	4468
struct.wide.app::ChartPane	77
struct.wide.app::PaperTrading	55
struct.wide.app::QuantickApp	56
struct.wide.app::Tab	62
struct.wide.app::ToolRail	32
struct.wide.app::ToolbarModel	31
struct.wide.orderflow::BookEngine	31
struct.wide.orderflow::BubbleStyle	30
struct.wide.orderflow::HeatmapConfig	34
struct.wide.orderflow::OrderflowHealth	32
struct.wide.sim::PerformanceReport	36
ratchet.size.budget	61410
ratchet.size.recorded	61410
ratchet.size.measured	171517
ratchet.context.budget	232885
ratchet.context.recorded	166099
ratchet.context.measured	233820
ratchet.cycle.budget	3
ratchet.cycle.recorded	3
ratchet.cycle.measured	3
site.allow	88
site.process_id	11
site.cfg_test	598
app.lines.without_egui	110701
scan.unreadable	0
scan.undecodable	0
scan.blind	0
```

## The request as received

Quoted in full and verbatim, in the language it was written in, because the
ledger above must be auditable against the original words rather than against
its own paraphrase. Everything else in this file is English.

> /mission medium feat/guards-report — add a `--report` mode to quantick-guards
> that prints, deterministically and with no dependency, the repository health
> numbers this refactor sprint is being judged on: production lines per crate
> and the app's share of the workspace, the largest production files, the
> widest structs, the two ratchet budgets against their measured totals, and
> the counts of `#[allow(`, `process::id()` and `#[cfg(test)]` sites in
> production files. One stable text table, one line per number, so that a
> session can quote it before and after a merge. Read
> C:\src\mission-guards-report.md in full before anything else and build the
> request ledger from it.

The brief that invocation points at is `C:\src\mission-guards-report.md`,
outside the repository. Its scope, acceptance criteria, out-of-scope list and
parallel-work note are the source of R1–R18 above; the fragments quoted in the
ledger's third column are its words.

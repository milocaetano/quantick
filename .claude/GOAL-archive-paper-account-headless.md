# Mission — move the paper account out of the UI crate

**Objective.** Move the paper account — the deciding half of paper trading:
orders, fills, settlement, risk sizing, journal export and report numbers —
out of `crates/app` into a new headless crate `quantick-paper`, so the chart,
the backtest and the bot can drive one account, the headless guard scans it,
and a change to it no longer recompiles the UI crate.

**Why.** Campaign task Q16 of #367 (issue #441, phase 2). Today the account is
2,330 lines in `crates/app/src/paper_account{,/*}.rs`: backtest and the bot
cannot reuse it (breaking "one engine, three consumers" for paper trading),
`crates/guards/src/headless.rs` does not scan it, and any edit recompiles the
whole UI crate.

**Tier:** `high` (set by the campaign; recorded in `mission-tier` before this
file). A new crate, a dependency-graph edge, a money path moved across a crate
boundary, and two byte-pinned goldens that must not move: a full bug pass and
a full delivery review are owed. Not `max`: no new behaviour, and no decision
here is the trader's money or safety — the fill rules, the risk lock and the
journal format are carried over byte for byte.

## Request ledger

Source: issue #441 (quoted verbatim at the end) and the coordinator's
delegation for Q16 (quoted verbatim at the end).

| | Ask |
| --- | --- |
| **R1** | Move the account into a headless crate — *"extend `crates/trading`, or a new `paper` crate under it if `trading`'s role argues against it; record the choice"*. Its content is *"orders, fills, settlement, risk sizing, journal export, report numbers"*. |
| **R2** | Keep the dependency direction one-way: *"`app` -> the crate; nothing back"*. |
| **R3** | Keep the headless rules: *"time is told, not read"*. |
| **R4** | `app` keeps the ticket UI and calls the account through its public API; *"no paper-account logic left in `crates/app` beyond UI and wiring"*. |
| **R5** | The byte-pinned tests (`the_journal_bytes_are_fixed`, `the_report_numbers_are_fixed`) move with the account and keep their hashes — *"same SHA-256"*. |
| **R6** | Prove a second consumer: *"backtest (or `sim`) drives the same account on a fixture tape with a test"*. |
| **R7** | Measure and report the incremental rebuild time for a one-line change in the account, before and after. Coordinator: with no other cargo process if possible; otherwise state the observed load, repeat three times, report the median. |
| **R8** | The crate is listed in `HEADLESS_CRATES` and `cargo test -p quantick-guards` is green. |
| **R9** | Merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch (A5 of the issue). Coordinator: the consolidated PR #449 is already cut and *"the coordinator merges"* — this mission never merges. |
| **R10** | Purpose, and the ask that judges the rest: backtest and the bot can reuse the account, the headless guard scans it, and a change to it stops recompiling the whole UI crate. |
| **R11** | Coordinator constraint: Q17 and Q18 run in parallel — keep edits to `AGENTS.md`'s map, `crates/guards/src/graph.rs`, `HEADLESS_CRATES` and the workspace `Cargo.toml` minimal and line-local; *"Do not touch `crates/app/src/control/`"*. |

## Decisions

None asked. The trader is not reachable from this subagent; every doubt below
is an `S` line, and the ones step 3 would have asked are marked *wanted to ask*.

## Assumptions

- **S1** — **A new crate, `crates/paper` (`quantick-paper`), not `trading`.**
  The account drives `quantick_sim::Simulator` and speaks `sim`'s journal
  format (`quantick_sim::history`), and `sim → trading` already exists, so
  hosting the account in `trading` would need the reverse edge `trading →
  sim`. `trading`'s role — the venue-neutral vocabulary and the port a broker
  implements — also argues against it: the account is a *consumer* of a venue
  (policy, sizing, journal), not a venue. `sim` is rejected for the same
  reason from the other side: it is the fill kernel, and the account is what
  sits on top of one. Edges: `paper → engine, sim`; `app → paper`;
  `backtest → paper`. It is the choice the issue itself names second.
- **S2** — **"Report numbers" means the report's computation, not only
  `paper_account/report.rs`.** That file is a 131-line seam; the numbers the
  report golden pins are cut in `paper_report/window.rs::ensure_report_view`
  over `paper_report/ledger.rs`'s journal reader and `curve.rs`'s equity walk,
  on `paper_calendar`'s date law and `timezone`'s offset. R5 puts that golden
  *in the new crate*, so the pure half of those moves with it: `HistoryRow`,
  `LoadedHistory`, `SourceFilter`, `load_history`, `ReportPeriod`, the equity
  walk and the cut (`ReportView`), plus the civil-date law (`CivilDate`,
  `DateRange`, `civil_utc`, `TzOffset`). The window, the calendar grid, the
  ledger rows and every colour stay in `app`, which re-exports the moved names
  at their old paths. *Wanted to ask*: whether the trader meant only the
  2,330 lines he measured; the wider reading is the one R5 can be met under,
  and it narrows nothing he asked for.
- **S3** — **`TzOffset` moves into `quantick_paper::civil`** and `app`'s
  `timezone` module re-exports it, because the report cut takes one and
  `engine` is documented as never seeing a timezone. If a later crate below
  `app` needs the same offset, lifting `civil` into its own crate is a
  follow-up, not this mission's.
  *Amended in review (AI-review thread PRRT_kwDOTfuoRs6h7rs7, commit
  `a61aa969`):* the follow-up was taken now. Q17's headless control host
  uses `TzOffset` too, so leaving it in `paper` would have handed that crate
  an edge into the paper account for a timezone. `civil` became
  `quantick-civil`, a dependency-free crate below `paper` and `app`, listed in
  `HEADLESS_CRATES` and `CLAUDE.md`'s sentence beside `paper`.
- **S4** — **`app` keeps a host wrapper at `crates/app/src/paper_account.rs`**
  that owns the headless account plus the UI-side state the old struct
  carried: the report window state, the cmd-trading gesture settings and the
  armed chart click, the import folder picker (`rfd`, a thread), the export's
  wall-clock file stamp and writer thread, and the two `QUANTICK_PAPER_*`
  environment hooks (read here, *told* to the account). It dereferences to
  the account, so `control/` — which the coordinator forbids touching — keeps
  calling `account().place_intent(..)` and friends unchanged. Keeping the path
  also keeps the generated hook registry byte-identical. Reversible in one
  follow-up if a reviewer prefers explicit forwarding.
- **S5** — **The report re-read after a journaled close stays eager.** The
  account raises a "journal changed" flag in its outbox; the wrapper's
  `on_trade` and `handle_events` — the paths a close is journaled on — take
  it at once and re-read the open report, exactly as before, and the
  per-frame `settle` takes it as a backstop for any path that reaches the
  account through `DerefMut`.
- **S6** — **The journal golden is driven through the account's public API
  in the new crate**, with the ticket's two offsets handed over as a
  `TicketForm` rather than typed into a text box — the text box is the
  ticket's, and the ticket stays in `app`. The expected bytes, the file name
  and the SHA-256 do not change. The offset-text parsing keeps its own tests
  in `app`.
- **S7** — **A5's merge is the coordinator's.** This mission delivers the PR
  ready, with base exactly `campaign/lean-a-plus`, CI green and every review
  marker current; it never runs `gh pr merge`.
- **S8** — **Log lines keep `target: "quantick::app"` and their event codes**
  after moving, as `quantick-orderflow` already does, so log filters and the
  health summary read the same lines.
- **S9** — **A4 is measured two ways and both are reported**: the edit-test
  loop (`cargo test --no-run` of the crate that owns the account: `app`
  before, `paper` after) and the full UI binary rebuild (`cargo build -p
  quantick-app`) after a one-line edit in the account. The second is the
  honest cost of a change that must still reach the chart; the first is what
  the move buys. Other agents were building on this host throughout; the load
  is recorded beside the numbers, in one fixed format: the count of
  `cargo.exe` and `rustc.exe` processes `tasklist` lists immediately before
  each run.
- **S10** — **The second consumer is `backtest`, not `sim`.** The issue
  allows either, but `paper → sim` is the edge this move needs, so a `sim`
  test driving the account would be the reverse edge `sim → paper`.
  `backtest` already links `sim` and is the headless consumer `CLAUDE.md`
  names, so it can take `paper` as a plain downward edge.

- **S11** — *Recorded in review.* Two trader-visible deltas the move makes
  on purpose, both found by the step-0 bug pass on `a61aa969`: the risk-lock
  refusal sentence loses a run of 18 stray spaces a lost line continuation
  had left inside it ("or                  turn the lock off" becomes "or
  turn the lock off"), and a timeline reset's forced close now raises
  `journal_changed`, so a report held open re-reads after a seek instead of
  omitting that trade until the next close. No golden and no test asserted
  the old text or the stale report; both are fixes of the same kind the
  goldens exist to stop from happening *silently*, which is why they are
  written down here and in the PR body.

S2 is the mission's headline scope decision; the PR body states it first
for the reviewer to ratify, not only here.

## Preflight

The source-first completeness pass (high tier, delivery contract) ran from
fresh context on this map before any code: no uncovered outcome,
constraint or gate. It raised three traceability points, all taken: S10
records why `backtest` rather than `sim`; S2 is surfaced as the headline
decision; S9 fixes the load format so two readers agree on it.

## Acceptance criteria

- [x] **A1** — `crates/app` holds no paper-account logic beyond UI and wiring:
      `crates/app/src/paper_account.rs` is the host wrapper of S4 and nothing
      in `app` places, fills, sizes, journals or cuts the report; the new crate
      is in `HEADLESS_CRATES` and in `CLAUDE.md`'s headless sentence, and
      `cargo test -p quantick-guards` passes.
      *Evidence:* an inventory of what `app`'s `paper_account.rs`,
      `risk_sizing.rs`, `order_strategies.rs`, `paper_report*` and
      `paper_calendar.rs` still hold after the move, and the guards run.
      → PR body, "A1". *(R1, R4, R8)*
- [x] **A2** — `the_journal_bytes_are_fixed` and `the_report_numbers_are_fixed`
      live in `crates/paper` and pass there with their expected text
      byte-identical: journal SHA-256
      `ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6`,
      report golden SHA-256
      `c90b6f970fd53ea2330f9dc6a2c7ad029549dc3d2f794955af5cddc8675cfe25`
      (both computed at base `317772a5`).
      *Evidence:* `cargo test -p quantick-paper` naming both tests, and the
      SHA-256 of both constants recomputed from the moved source.
      → PR body, "A2". *(R5)*
- [x] **A3** — A test in `crates/backtest` drives `quantick_paper::PaperAccount`
      over the replay crate's recorded fixture tape and asserts its closed
      trades equal the backtest harness's own run of the same orders.
      *Evidence:* the named test and its passing output. → PR body, "A3".
      *(R6, R10)*
- [x] **A4** — Incremental rebuild time for a one-line change in the account,
      before and after, three runs each, median reported, load stated.
      *Evidence:* the table of runs. → PR body, "A4". *(R7, R10)*
- [ ] **A5** — The PR's base is exactly `campaign/lean-a-plus`; it is left
      ready for the coordinator's merge.
      *Evidence:* `gh pr view --json baseRefName`. → PR body. *(R9)*
- [x] **A6** — The graph is one-way: `ALLOWED` gives `paper` exactly
      `engine` and `sim` (amended by S3's review note: and `civil`, whose own
      row is empty), `backtest` gains `paper`, nothing gains `app`, and
      the graph guard passes.
      *Evidence:* the `graph.rs` hunk and the guards run. → PR body, "A6".
      *(R2)*
- [x] **A7** — Time is told, not read: `quantick-paper`'s production source
      reads no clock, spawns no thread, names no UI type (the headless guard
      scans it clean, one signed entry for its test-only scratch helper), and
      the export's wall-clock stamp, the writer thread and the folder picker
      live in `app`.
      *Evidence:* the headless guard run and a grep of the crate. → PR body,
      "A7". *(R3)*
- [x] **A8** — `crates/app/src/control/` is untouched, and the edits to
      `AGENTS.md`, `graph.rs`, `headless.rs`'s list and the root `Cargo.toml`
      are line-local hunks.
      *Evidence:* `git diff --stat <base>... -- crates/app/src/control` empty,
      and the hunk list of the four shared files. → PR body, "A8". *(R11)*
- [x] **A9** — Behaviour is unchanged: every test that existed at base still
      passes, in `app` or in the crate it moved to, and no expected value was
      edited to make it pass.
      *Evidence:* test counts per crate at base and at head, and the full
      workspace test run. → PR body, "A9". *(R4, R5)*

### Injected gates

- [ ] **G1** — Every artifact in English (`arch-review` dimension 8, language
      guard). → arch-review report.
- [ ] **G2** — The four checks green after rebasing on the latest
      `origin/campaign/lean-a-plus`, each run on its own. → PR body.
- [x] **G3** — Performance impact declared. The per-trade path
      (`PaperAccount::on_trade` → `handle_events`) moves verbatim; the wrapper
      adds one flag read per print and one cross-crate call. Per-frame:
      `settle` gains the same flag read. Rare: import, export, report re-cut.
      Evidence that the per-trade path is flat: a timed run of the same tape
      through the ticket at base and at head. → PR body, "Performance".
- [ ] **G4** — `arch-review` (step 0 `code-review` at `medium`) with every
      Blocker and Should-fix resolved or deferred in the PR body.
- [x] **G5** — Test-first: the second-consumer test (A3) and the goldens'
      new home are committed before the code that makes them pass.
      → git log.
- [x] **G6** — `new-extension` for the new crate: registration-only edits
      (workspace member, graph row, headless list, map row), defaults
      preserve today's behaviour, blast radius stated in the PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

Evidence destinations for G-AI1…4: the ai-review report comment on the PR,
the `ai_review_threads.sh list` output quoted in the PR body, and the private
`ai-review-complete` projection under the shared key.

### Not applicable

- **Touches anything user-visible** — no surface changes; every string the
  account raises is moved verbatim, and the goldens pin the journal and the
  report. `visual-qa` and `trader-ux-review` would photograph an unchanged
  chart.
- **Adds something a trader does** — no new action; the control plane is
  untouched (R11).

### Evidence recorded before the reviews

Measured on this branch before archiving; the PR body carries the same
numbers with the review verdicts beside them.

- **A2** — SHA-256 recomputed from the moved constants at head: journal
  `ab748594…94d6`, report `c90b6f97…fe25`, both equal to base `317772a5`.
  `cargo test -p quantick-paper`: 59 passed, both goldens among them.
- **A3** — `the_backtest_drives_the_same_paper_account_the_chart_trades_through`
  passes: identical closed trades (a manual close and a target), the same
  open position at the end, and a journal that parses back to them.
- **A4** — one-line string edit in the account, three runs each, median,
  measured with zero other `cargo`/`rustc` processes (`tasklist`):
  before, `cargo build -p quantick-app` 16.2 s and `cargo test -p
  quantick-app --no-run` 30.3 s; after, `cargo test -p quantick-paper
  --no-run` 1.6 s, `cargo build -p quantick-app` 9.3 s, `cargo test -p
  quantick-app --no-run` 12.9 s. A first before-series under a load of 6-10
  processes read 26.4 s and 51.6 s.
- **A9** — 57 `#[test]` functions left `app`, every one present by the same
  name in `quantick-paper` (script over `git show 317772a5` against head);
  app 2076 passed / 10 ignored, whole workspace green.
- **G3** — 200,000-print tape through the ticket, five runs per build, three
  alternating base/head pairs: base medians 11.7 / 14.3 / 11.9 s, head
  12.2 / 14.7 / 11.4 s. Flat within the noise; the run is dominated by the
  per-close journal append, which is unchanged.

## Closing steps

- **C1** — `delivery-review` PASS, from fresh context, last.
- **C2** — The open PR, non-draft, base `campaign/lean-a-plus`, CI green at
  its head.
- **C3** — `sh .claude/hooks/mission_ship_gate.sh ship <pr>` PASS.

## Verbatim request

> Issue #441, by the trader (milocaetano), 2026-09-13:
>
> refactor(trading): move the paper account out of the app crate so backtest and bot reuse it
>
> <!-- campaign-task:milocaetano/quantick#367/Q16 -->
> ## Context
>
> Campaign child of https://github.com/milocaetano/quantick/issues/367 (task key Q16, phase 2). Base: `campaign/lean-a-plus`. Mission tier: high. Raised by the trader on 2026-09-13.
>
> The paper account (the deciding half of paper trading: orders, fills, settlement, risk sizing, journal export, report numbers) lives in the UI crate: `crates/app/src/paper_account.rs` and `paper_account/{export,orders,report,risk}.rs`, 2,330 lines, none of which references egui/eframe (measured at campaign tip `124cdf0d`). Consequences:
> - backtest and the bot cannot reuse it, which breaks for paper trading the "one engine, three consumers" rule the project keeps for bars;
> - `crates/guards/src/headless.rs` (no wall clock, no UI) does not scan it, because it scans headless crates and not `app`;
> - any change to it recompiles the whole UI crate.
>
> ## Scope
>
> - Move the account into a headless crate (extend `crates/trading`, or a new `paper` crate under it if `trading`'s role argues against it; record the choice), keeping the dependency direction one-way (`app` -> the crate; nothing back) and the headless rules (time is told, not read).
> - `app` keeps the ticket UI and calls the account through its public API; byte-pinned tests (`the_journal_bytes_are_fixed`, `the_report_numbers_are_fixed`) move with the account and keep their hashes.
> - Prove a second consumer: backtest (or `sim`) drives the same account on a fixture tape with a test.
>
> ## Acceptance criteria
>
> - [ ] A1: no paper-account logic left in `crates/app` beyond UI and wiring; the crate is listed in `HEADLESS_CRATES` and `cargo test -p quantick-guards` is green.
> - [ ] A2: the byte-pinned journal and report tests pass unchanged in the new crate (same SHA-256).
> - [ ] A3: a backtest/sim test drives the same account (second consumer).
> - [ ] A4: incremental rebuild time for a one-line change in the account measured before/after, reported.
> - [ ] A5: merged into `campaign/lean-a-plus`, after the first consolidated PR is cut, through a PR whose base is exactly that branch.

> The coordinator's delegation for Q16 (campaign #367), 2026-09-13, the
> operative sentences:
>
> Execute campaign task **Q16**, issue https://github.com/milocaetano/quantick/issues/441 (read it in full with `gh issue view 441`), of campaign #367 (https://github.com/milocaetano/quantick/issues/367), phase 2, mission tier **high**, through the `/mission` workflow of this repository (`.claude/skills/mission/SKILL.md` in the worktree). You are a subagent and cannot ask the trader; make routine judgment calls, record them in the goal file and PR body.
>
> Parallel work: Q17 (#442, moving the UI-free control-plane host out of `crates/app` into a headless crate) and Q18 (#443, a ratchet on `app.lines.without_egui`) run at the same time in other worktrees. Keep your edits to `AGENTS.md`'s map, `crates/guards/src/graph.rs`, `HEADLESS_CRATES` and the workspace `Cargo.toml` minimal and line-local so the later rebase is trivial. Do not touch `crates/app/src/control/`.
>
> A4 (incremental rebuild time before/after) must be measured with no other cargo process running, if possible; other agents are building, so state the load you observed and repeat three times, reporting median.
>
> Delivery: draft PR `cd <wt> && gh pr create --draft --base campaign/lean-a-plus ...` [...] Then CI green, `gh pr ready <n>` once, `sh .claude/hooks/mission_ship_gate.sh ship <n>`. **Never merge** (A5 says it merges after the first consolidated PR is cut; the consolidated PR #449 is already cut, and the coordinator merges).

# Mission — take the performance report out of the order ticket

**Objective:** Extract the performance report and the trade ledger out of
`crates/app/src/paper_trading.rs` into their own module, with the report
numbers proven unchanged by a golden test written before the move.

**Tier:** `high`. The work touches the money path's own file — the one that
places orders, projects brackets, sizes risk and writes the trades journal —
and moves roughly 2,550 production lines of it. A wrong cut here does not
merely look wrong; it can change a number the trader reads as their own
result. It earns the full interrogation, the full gate table, a `medium` bug
pass and `delivery-review` in full.

**Why it matters:** `paper_trading.rs` is 9,238 production lines and is the
second god object in this repository — the one that has never been looked at.
`PaperTrading` holds 75 fields and 149 methods, and one `impl` block runs
5,441 lines. Every line of calendar and equity-curve rendering sitting in it
is a line someone auditing "was the stop placed at the right price?" has to
read past. Shrinking this file is making the money path auditable, not
tidying it.

---

## Request ledger

| # | Ask | Criteria |
| --- | --- | --- |
| **R1** | Move the performance report out of `paper_trading.rs` into its own module. | A1, A2 |
| **R2** | Move the trade ledger out of `paper_trading.rs` into the same module. | A1, A2 |
| **R3** | Move the report/ledger fields off `PaperTrading` — every `report_*`, `ledger_*`, `calendar`, `history_cache`, `saved_totals`, `selected_trade`, `collapsed_days`. | A3 |
| **R4** | Move the `REPORT_*` / `CALENDAR_*` / `CURVE_*` / `TILE_*` constant block with them. | A2 |
| **R5** | Follow `paper_calendar.rs`: state the shape in the module's own header, pure below one entry point, plain functions over plain values, testable without a window. | A4 |
| **R6** | The new module must not reach back into `PaperTrading` — *"pass it what it reads, the way `SurfaceEnv` does"*. | A5 |
| **R7** | **The numbers must not move.** Data honesty is non-negotiable — *"a rounding that shifts during a move is the code telling the trader a lie about their own results"*. | A6 |
| **R8** | Prove R7 with a golden test: fixed closed trades in, the full report snapshot out, asserted byte-for-byte, **written before the move and unchanged after**. | A6, A7 |
| **R9** | No behaviour change anywhere else — order entry, brackets, risk sizing, the ruler and the journal stay exactly where and exactly as they are. | A8, A14 |
| **R10** | Touch `app.rs` only at the five harness call sites (`autostart_report`, `autostart_calendar`, `set_ledger_scope`, `autostart_ledger_pages`) and nowhere else; do not restructure it. | A9 |
| **R11** | State the accounting honestly in the PR body: production lines for `paper_trading.rs` before and after, production lines of the new module, and the **net**. | A10 |
| **R12** | Re-run `cargo run -p quantick-guards -- --tighten` immediately before pushing, not earlier — a parallel branch is also moving `!budget`. | A11 |
| **R13** | `paper_trading.rs` drops below 7,000 production lines and stops being the largest file in the workspace. | A12 |
| **R14** | `PaperTrading` loses at least 20 fields; state before and after. | A3 |
| **R15** | `visual-qa` captures the report window, the calendar and the ledger before and after, and they are identical. | A13 |
| **R16** | Stay out of the non-goals: risk sizing, order entry and the cmd/bracket/drag path, and anything in `app.rs` beyond the five calls. | A9, A14 |
| **R18** | *"This is extraction, not design."* The diff is a relocation, not a re-modelling of the report. | A1, A2, A8 |
| **R17** | *(purpose)* Make the money path auditable — what is left in `paper_trading.rs` is order placement, brackets, risk and the journal, not report rendering. | A1, A12, A15 |

---

## Decisions taken by the trader (step 3)

- **D1 — One module, `crates/app/src/paper_report.rs`.** The report and the
  ledger are two surfaces but share `ReportView`, `HistoryRow`,
  `LoadedHistory` and the equity walk; splitting them would force those types
  public across a seam. One file, roughly 2,550 production lines, following
  `paper_calendar.rs`'s header shape.
- **D2 — The golden asserts every computed number**: a fixed set of closed
  trades, through `report_from_history`, to a text dump of every field of
  `PerformanceReport` plus the `EquityWalk` points, asserted byte-for-byte
  against an inline golden. Chosen over asserting `ReportSnapshot` because the
  snapshot omits numbers the report computes, and an omitted number is an
  unguarded one.
- **D3 — One `ReportState` field on `PaperTrading`.** The new module owns the
  struct and every method on it; `PaperTrading` holds it and passes it down.
  75 fields to roughly 52 (loses 24, gains 1).
- **D4 — Keep thin delegators for the `pub(crate)` API, drop the private
  ones.** `app.rs` and `control/gateway.rs` keep calling
  `paper.autostart_report()` and friends, so the five named `app.rs` call
  sites are the only ones that change and `gateway.rs` is untouched. The
  roughly 12 private `draw_*` / `reload_*` / `ensure_*` methods move whole and
  leave no wrapper behind.

---

## Assumptions (step 5)

- **S1 — The free-function render block moves too.** The request names 22
  methods, but `draw_report_tiles`, `draw_tile`, `draw_equity_curve`,
  `draw_hover_card`, `paint_list_row`, `draw_trade_list`, `draw_report_grid`,
  `draw_side_grid`, `draw_exit_reason_grid`, `load_history` and
  `report_from_history` (lines 7059-7987, roughly 930 production lines) are
  report rendering and report arithmetic by any reading, and the sub-7,000
  target is not reachable without them. Measured, not guessed: types (~440)
  plus the `impl` span (~1,080) plus constants (~100) is ~1,620, which lands
  the file at ~7,620.
- **S2 — `TRADE_LIST_COLUMNS`, `REPORT_LIST_ROW_H_PX` and
  `TRADE_LIST_CELL_PAD_PX` move with the trade list they lay out**, though the
  request's constant list names only the four prefixes.
- **S3 — `draw_trades_tab` (the sidebar trades tab) is the ledger** and moves,
  since the request explicitly moves "the ledger's paging, totals and row
  rendering" and that method is where all three are drawn.
- **S4 — The existing report/ledger tests move with the code they cover**, into
  the new module's own `#[cfg(test)] mod tests`. They are the proof that
  behaviour did not change, so they must keep passing across the move
  unedited except for import paths.
- **S5 — `SourceFilter`, `LedgerScope` and `ReportPeriod` keep their
  `pub(crate)` visibility**, re-exported from `paper_trading` where the
  control plane already names them through it, so no caller outside the two
  files learns a new path.
- **S6 — The "before" `visual-qa` capture is taken on this branch's first
  commit** (the golden test, no move yet), not in a separate `origin/main`
  worktree: the first commit is behaviourally `origin/main` for every surface
  in scope, and one worktree is one launch configuration less to get wrong.
- **S7 — *wanted to ask*: whether `PaperTrading`'s method count is itself a
  target.** The request states a field target (at least 20) and a line target
  (under 7,000) but no method target. D4's delegators mean the method count
  falls by roughly the 12 private ones only. Went with: fields and lines are
  the stated measures; the method count is reported honestly in the PR body
  and not optimised for.

---

## Acceptance criteria

### Mission-specific

- [x] **A1** — The performance report and the trade ledger are drawn from
      `crates/app/src/paper_report.rs`, not from `paper_trading.rs`; no
      `draw_report_*`, `draw_ledger_*`, `draw_trade_list`, `draw_equity_curve`
      or `draw_trades_tab` body remains in `paper_trading.rs`.
      *Evidence:* `grep -n` over both files, before and after.
      → `.claude/evidence/report-out-of-the-ticket/move-inventory.md`. *(R1, R2, R17)*
- [x] **A2** — The report/ledger types (`ReportView`, `ReportSnapshot`,
      `ReportWindow`, `EquityWalk`, `ReportPeriod`, `LedgerScope`,
      `LedgerTotals`, `LedgerPage`, `LedgerRow`, `HistoryRow`,
      `LoadedHistory`, `SourceFilter`) and the `REPORT_*` / `CALENDAR_*` /
      `CURVE_*` / `TILE_*` / trade-list constants live in the new module.
      *Evidence:* the same inventory, listing each item's old and new line.
      → `.claude/evidence/report-out-of-the-ticket/move-inventory.md`. *(R1, R2, R4)*
- [x] **A3** — `PaperTrading` holds one `report: paper_report::ReportState`
      field in place of the roughly 24 report/ledger fields, and its field
      count drops by at least 20. Both counts stated.
      *Evidence:* field count before and after, from the struct body.
      → `.claude/evidence/report-out-of-the-ticket/field-count.md`, and the PR body. *(R3, R14)*
- [x] **A4** — `paper_report.rs` opens with a module header in
      `paper_calendar.rs`'s shape: what it owns, why it is separate, and where
      the pure/impure line falls.
      *Evidence:* the header itself, quoted.
      → `crates/app/src/paper_report.rs`, lines 1 to about 30. *(R5)*
- [x] **A5** — Nothing in `paper_report.rs` names `PaperTrading` or takes it as
      a parameter; everything it reads is passed to it.
      *Evidence:* `grep -c 'PaperTrading' crates/app/src/paper_report.rs` is 0
      (bar a doc-comment reference), and the signature list of its public
      entry points.
      → `.claude/evidence/report-out-of-the-ticket/no-reach-back.md`. *(R6)*
- [x] **A6** — A golden test asserts every field of `PerformanceReport` plus
      the `EquityWalk` points, byte-for-byte, over a fixed closed-trade
      fixture, and passes.
      *Evidence:* `cargo test -p quantick-app the_report_numbers_are_fixed`
      output.
      → `.claude/evidence/report-out-of-the-ticket/golden.md`. *(R7, R8)*
- [x] **A7** — That golden was committed **before** the move and its expected
      value is byte-identical after it.
      *Evidence:* `git log -p` over the golden constant showing one commit
      adding it and no commit changing it.
      → `.claude/evidence/report-out-of-the-ticket/golden.md`. *(R8)*
- [x] **A8** — Order entry, brackets, risk sizing, the ruler and the journal
      are unchanged: the diff over `paper_trading.rs` touches those regions
      only where a moved field is now read through `self.report`.
      *Evidence:* `git diff origin/main...HEAD -- crates/app/src/paper_trading.rs`
      reviewed hunk by hunk, with every hunk outside the moved region
      classified.
      → `.claude/evidence/report-out-of-the-ticket/untouched-money-path.md`. *(R9)*
- [x] **A9** — `git diff origin/main...HEAD -- crates/app/src/app.rs` touches
      only the five named harness call sites.
      *Evidence:* the diff itself, quoted in full.
      → `.claude/evidence/report-out-of-the-ticket/app-rs-diff.md`. *(R10, R16)*
- [x] **A10** — The PR body states `paper_trading.rs` production lines before
      and after, `paper_report.rs` production lines, and the net change in
      total production lines.
      *Evidence:* the PR body.
      → the PR. *(R11)*
- [x] **A11** — `cargo run -p quantick-guards -- --tighten` is run in the last
      commit before the push, and `!budget` reflects it.
      *Evidence:* the commit touching `size-baseline.txt` is the last one
      before `git push`; its timestamp and the push order stated.
      → `.claude/evidence/report-out-of-the-ticket/tighten.md`. *(R12)*
- [x] **A12** — `paper_trading.rs` is under 7,000 production lines and is not
      the largest file in the workspace.
      *Evidence:* `size-baseline.txt` after `--tighten`, sorted.
      → `.claude/evidence/report-out-of-the-ticket/sizes.md`. *(R13, R17)*
- [x] **A13** — `visual-qa` captures the report window, the calendar and the
      ledger before and after the move; the pairs are identical.
      *Evidence:* screenshot pairs plus a stated comparison verdict per
      surface.
      → `.claude/evidence/report-out-of-the-ticket/visual-qa/`. *(R15)*
- [x] **A14** — `risk_sizing.rs`, the cmd/bracket/drag path and the order-entry
      methods are untouched by this branch.
      *Evidence:* `git diff --stat origin/main...HEAD` showing which files
      changed at all.
      → `.claude/evidence/report-out-of-the-ticket/untouched-money-path.md`. *(R9, R16)*
- [x] **A15** — What remains in `paper_trading.rs` is stated as a list of the
      concerns it still holds, so the next extraction has a starting point.
      *Evidence:* a closing section of the inventory.
      → `.claude/evidence/report-out-of-the-ticket/move-inventory.md`. *(R17)*

### Injected gates

- [x] **G1** — Every artifact in English, per `CLAUDE.md`.
      *Evidence:* `cargo test -p quantick-guards` (language scan) and
      `arch-review` dimension 8.
      → `.claude/evidence/report-out-of-the-ticket/checks.md`.
- [x] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own.
      *Evidence:* the four commands' output and exit codes.
      → `.claude/evidence/report-out-of-the-ticket/checks.md`.
- [x] **G3** — Performance impact declared: every touched path classified by
      rate (per-trade / per-depth / per-frame / rare).
      *Evidence:* the classification, written before the review.
      → `.claude/evidence/report-out-of-the-ticket/performance.md`, and the PR body.
- [x] **G4** — Touches a per-frame path (the report window and ledger draw
      every frame while open): evidence that rendering is flat or better, not
      a belief.
      *Evidence:* the move is a code relocation with no algorithmic change;
      stated as such with the equity-walk caching left intact, plus an
      `APP_HEALTH_SUMMARY` comparison if any call shape changed.
      → `.claude/evidence/report-out-of-the-ticket/performance.md`.
- [x] **G5** — User-visible: every affected surface reachable by env hook (the
      five already exist and must keep working); `visual-qa` pass with all
      surfaces PASS or defects explicitly accepted.
      *Evidence:* the `visual-qa` report.
      → `.claude/evidence/report-out-of-the-ticket/visual-qa/`.
- [x] **G6** — `arch-review` run over `git diff origin/main...HEAD` with every
      Blocker and Should-fix resolved, or deferred in the PR body with its
      severity.
      *Evidence:* the review verdict and the `arch-review-ok` marker.
      → the review output, and the PR body.

### Not applicable, and why

- **Adds a capability (`new-extension`)** — no. Nothing new docks here; this
  moves existing code behind an existing surface. No port to name, no
  registration, no second implementation to fake.
- **Adds something a trader *does* (`arch-review`'s second operator)** — no
  new action. The five harness hooks and the control-plane reads already
  exist and are preserved unchanged by D4's delegators; the drivability that
  exists today is exactly the drivability that ships.
- **Engine / determinism territory** — not the engine. But the test-first rule
  is honoured anyway and deliberately: R8 requires the golden written before
  the move, which is the same discipline for the same reason.
- **`trader-ux-review`** — no surface changes, so no Blocker can be
  introduced. `visual-qa`'s identical-pairs check (A13) is the stronger claim
  and subsumes it; a UX review of a pixel-identical surface would be grading
  `origin/main`.
- **Docs/skills only** — no. This is a code change and takes the full shape
  pass.

---

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the `high` tier beside the four verification
  boxes, with CI green.

---

---

## Results, as shipped

Every criterion above is ticked, and each one's evidence is at the path it
named. The numbers, in one place:

| | before | after |
| --- | ---: | ---: |
| `paper_trading.rs` production lines | 9,238 | **6,396** |
| `paper_report.rs` production lines | — | **3,300** |
| total production | 9,238 | 9,696 (**+458**) |
| `PaperTrading` fields | 75 | **55** |
| `app.rs` lines changed | — | **0** |
| `PaperTrading` methods | 149 | **138** (135 ship; three are `#[cfg(test)]`) |

- **A6/A7** — the golden's text has the same SHA-256 at the commit that
  wrote it and at HEAD: `c90b6f97…5cfe25`. It was written before the move
  and did not change during it.
- **A13** — the ledger surface is pixel-identical (0 of 179,180). The
  report and the calendar differ by ~150 pixels confined to the chart's
  live CVD legend, and a control run of the *same build against itself*
  differs inside that same band, which is what proves the band is the tape.
- **A9** — `app.rs` was not edited at all. The five call sites the mission
  budgeted for did not need touching, because the wrappers kept their
  names. `main.rs` gained one `mod` line.

### Departures from the plan, stated

1. **S1 held, and further.** The free-function render block moved as
   assumed, and so did the ledger's row painters (`push_by_day`,
   `draw_day_header`, `draw_open_row`, `draw_ledger_row`, `elide_tail` and
   the rest, ~440 lines) which the assumption had not named. Same reading:
   they are the ledger, and the sub-7,000 target needed them.
2. **D4 was applied to its spirit, not its letter.** It said keep
   delegators for the `pub(crate)` API. Five of them —
   `set_day_collapsed`, `toggle_all_days`, `pick_report_dates`,
   `show_report_month`, `report_snapshot` — turned out to have no caller
   outside the report on `origin/main`, so a wrapper for them would have
   been an API with nobody on the other side. They are `ReportState`
   methods now. Nothing outside lost a name it was using, and
   `report_snapshot`'s one production caller (the `PAPER_REPORT_CUT` log)
   moved with it unchanged.
3. **S4 held for the golden and bent for ten tests.** The report's tests
   moved with the report. Ten that drive a real journal on disk stayed with
   the host that writes one, and were rewritten to read through three
   named accessors (`saved_rows_loaded`, `revealed_pages`, `view_rows`)
   rather than through fields — so the split did not cost the
   encapsulation it was for. The golden itself moved unedited.
4. **A new cost the plan did not predict: +425 production lines.** The
   assumption was silent on whether the total would rise. It did, and the
   raise is on the `!budget` line with its reason, not hidden in a signed
   per-file entry. See `sizes.md`.

5. **The review found one behaviour change, and it was real.** `arch-review`
   step 0 and this session's own read landed on the same defect
   independently: `draw_report_window` consumed `ReportResponse.start_import`
   and dropped `ReportResponse.toast`, so a typed period the report refused
   produced silence instead of the acknowledgement `origin/main` gave —
   against R9, in the one place the extraction had genuinely changed
   behaviour. Fixed, and covered by
   `a_refusal_the_report_raises_reaches_the_windows_one_toast`, which fails
   without the fix. The suite had no test on that path before; that gap is
   why the seam could swallow it.
6. **And one performance note, also fixed.** `report_env!` gathers the open
   position, so building it on the per-trade close path cost a
   `position_summary()` per closed trade even with the report shut.
   `ReportState::is_open()` now guards it.

7. **The ledger was one line short, and `delivery-review` said so.** The
   completeness pass derived 22 atomic asks from the trader's own words and
   found 21 in the ledger. The missing one is *"This is extraction, not
   design."* — carried in spirit by R5 and R6, which are the request's
   *positive* design instructions, but never written down as the constraint
   it is. Added above as **R18** and graded COVERED: the moved bodies are
   verbatim and the only new types are the seam R6 requires. The work was
   never at risk; the record was.
8. **S7's commitment was dropped and then kept.** S7 said the method count
   would be "reported honestly in the PR body and not optimised for", and
   the first draft of this file and of the PR body stated no method count at
   all. `delivery-review` caught the drift. It is **149 → 138** — fifteen
   methods left, four arrived (`open_row`, and the three `#[cfg(test)]`
   accessors), so 135 reach the shipped binary. Not optimised for, as
   promised: the eleven surviving wrappers could have been deleted to make
   the number look better, and deleting an API its callers use to improve a
   statistic is the opposite of the point.
9. **Not rebased, then rebased.** The four checks were first run against the
   `origin/main` this branch was cut from, while main had moved 11 commits —
   including a rework of the very guards crate A11 and A12 depend on. G2
   says "after rebasing on latest `main`", so the branch was rebased onto
   `9376ac7` and every check re-run against it. The reworked size ratchet
   accepts these numbers unchanged.

### Found on the way, not fixed here

The size guard ends a `#[cfg(test)]` module at the first line that is
exactly `}` (`crates/guards/src/size.rs:321`). A raw string containing a
column-0 closing brace therefore walks the scan out of the test module —
the first draft of the golden scored 12,367 production lines instead of
9,238. Worked around here by indenting the dump; the guard's own
fragility is untouched and is a PR follow-up.

---

## The request as received

Quoted verbatim and in full, as `mission` step 5 requires: this is the source
`delivery-review` re-derives the asks from, and paraphrasing it would make the
ledger above its own judge. It is reproduced unedited for that reason, not
because any part of it is exempt from the English rule — it is already
English. Received 2026-09-02 from the trader, as the argument to
`/mission high`.

> Take the performance report out of the order ticket.
>
> Running in parallel with a second session working on `app.rs`. Your file is
> `crates/app/src/paper_trading.rs`. Do not restructure `app.rs` — you touch it only
> at the five harness call sites around lines 1555-1620 (`autostart_report`,
> `autostart_calendar`, `set_ledger_scope`, `autostart_ledger_pages`) and nowhere else.
>
> `paper_trading.rs` is the second god object in this repository and the one that has
> never been looked at. `PaperTrading` holds 75 fields and 149 methods; `QuantickApp`,
> after two missions, holds 76 and 144. They are the same size. One `impl PaperTrading`
> block runs 5,441 lines, from 1528 to 6969 — longer than any impl in `app.rs`.
>
> The difference is what this file does: it places orders, projects brackets, sizes risk
> against capital and writes the trades journal. Every line of calendar and equity-curve
> rendering sitting in it is a line someone auditing "was the stop placed at the right
> price?" has to read past. Shrinking this file is not tidiness; it is making the money
> path auditable.
>
> Move the performance report and the trade ledger out. They are 22 of the 149 methods
> (`open_report`, `reload_report`, `ensure_report_view`, `draw_report_window`,
> `draw_report_filters`, `draw_report_calendar`, `report_snapshot`, `pick_report_dates`,
> `show_report_month`, `set_report_list_open`, plus the ledger's paging, totals and row
> rendering), about 24 of the 75 fields (every `report_*`, `ledger_*`, `calendar`,
> `history_cache`, `saved_totals`, `selected_trade`, `collapsed_days`), and the
> `REPORT_*` / `CALENDAR_*` / `CURVE_*` / `TILE_*` constant block. They already own their
> types — `ReportView`, `ReportSnapshot`, `ReportWindow`, `EquityWalk`, `ReportPeriod`,
> `LedgerScope`, `LedgerTotals`, `LedgerPage`, `LedgerRow`, `HistoryRow`, `LoadedHistory`,
> `SourceFilter`. This is extraction, not design.
>
> Follow `paper_calendar.rs`. It took the date law out of this same file and states the
> shape in its own header: pure below one `draw_month` entry point, plain functions over
> plain values, testable without a window. The report reads closed trades and renders; it
> has no reason to be less pure. It must not reach back into `PaperTrading` — pass it what
> it reads, the way `SurfaceEnv` does.
>
> Constraints:
> - **The numbers must not move.** This is where trading performance is computed, and data
>   honesty is a non-negotiable rule in CLAUDE.md — a rounding that shifts during a move is
>   the code telling the trader a lie about their own results. Prove it with a golden test:
>   a fixed set of closed trades in, the full report snapshot out, asserted byte-for-byte,
>   written *before* the move and unchanged after.
> - No behaviour change anywhere else. Order entry, brackets, risk sizing, the ruler and
>   the journal stay exactly where and exactly as they are.
> - State the accounting honestly in the PR body: production lines for `paper_trading.rs`
>   before and after, production lines of the new module, and the **net**. The last
>   extraction moved 854 lines out of a tracked file into an untracked one; `!budget` fell
>   528 while total production rose ~326. That was the right trade, but a reader of the
>   budget alone would not have known it happened.
> - Re-run `cargo run -p quantick-guards -- --tighten` immediately before pushing, not
>   earlier: a parallel branch is also moving `!budget`.
>
> Non-goals: risk sizing (21 methods, and `risk_sizing.rs` already exists — deciding what
> belongs where is its own mission), order entry and the cmd/bracket/drag path (32 methods,
> the hot path, it goes last and slowly), and anything in `app.rs` beyond those five calls.
>
> Acceptance beyond the standard gates:
> - `paper_trading.rs` drops below 7,000 production lines and stops being the largest file
>   in the workspace.
> - `PaperTrading` loses at least 20 fields; state before and after.
> - The golden report test exists and passes unchanged across the move.
> - `visual-qa` captures the report window, the calendar and the ledger before and after
>   and they are identical.

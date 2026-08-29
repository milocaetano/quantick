# GOAL — a chart that reaches into the past on demand

**Mission**: let one press of *load older* reach a trader-chosen span into the
past — a session's open plus a lead into the day before it — and let a chart
that is not cut by time carry the venue's candle history in front of its own
bars, in replay as well as live.

Branch: `claude/grafico-historico-carregamento-sjubpt`

## What is broken today

Three findings, all read out of the code:

1. `Tab::refold_history_prefix` (`crates/app/src/tab.rs:1315`) gives a pane with
   no `time_interval_ms()` an **empty** prefix. A tick / volume / dollar /
   imbalance chart therefore never carries a venue candle, and
   `OlderCandles::NoChartCutByTime` greys the *older candles* control out on it.
   That is the trader's "only the timeframe chart loads, the tick chart does
   not".
2. `Tab::request_older_history` (`crates/app/src/tab.rs:1802`) sends exactly
   **one** `load_older` of `history_step` trades per press. One page of 2 000
   prints is minutes of a liquid contract — "it loads a bit and no further
   back". Nothing continues past a session's open into the day before it.
3. `Tab::request_ohlcv_history` (`crates/app/src/tab.rs:841`) refuses to ask
   while `self.replay.is_some()`, yet `feed::replay` answers `FetchOhlcv` from
   the context file beside the recording. A replay therefore opens with no
   run-up at all, and only picks the context up if the trader presses *load
   older*.

## Acceptance criteria

### Mission-specific

1. [x] **A reach on the load button.** `+ older` gains a reach the trader
       picks: `Page` (one page of trades — today's behaviour, and the default)
       or `Previous session`. It lives in the history caret menu, is remembered
       across restarts, and is reachable without a mouse.
2. [x] **A press with a reach beyond `Page` runs a paging campaign**: one
       request outstanding at a time, continuing until the reach is met, the
       feed withdraws paging (exhausted), or a bounded budget is spent. Never
       unbounded; the loading indicator resolves exactly once per campaign.
3. [x] **The session open is read out of the tape, not assumed from a
       calendar**: the first inter-print gap longer than a named constant is
       the market's last close, and `Previous session` continues a further
       named lead past it. A market that never closes has no gap, and the
       campaign ends on its span cap — documented, not silent.
4. [x] **A chart not cut by time can carry the venue candle prefix**, behind an
       option that is **off by default** so today's behaviour is preserved. The
       status bar keeps naming venue candles apart from built bars, and the
       *older candles* control stops reporting `NoChartCutByTime` when the
       option is on.
5. [x] **A replay installs its context on open**, with no click, and the same
       option puts it in front of a replay's tick chart.
6. [x] **Tests written first** over: the session-gap search, every campaign
       stop condition (reach met / exhausted / budget spent / feed gone), the
       non-time prefix install and trim, and the replay opening fetch.

### Injected gates

7.  [x] **English in every artifact** — code, comments, log and UI strings,
        test names, this file, the commit messages and the PR body.
8.  [x] Four checks green on the latest `main`: `cargo fmt --all -- --check`,
        `cargo clippy --workspace --all-targets -- -D warnings`,
        `cargo build --workspace`, `cargo test --workspace`.
9.  [x] **Performance declared by rate** for every touched path (per-trade /
        per-depth / per-frame / rare) in the plan, not in the review. If a
        per-frame path grows, a measured number — not a belief.
10. [x] If `bridge/mt5/` or `tools/mt5/` is touched: `ruff check --select F`
        over both, plus `python3 tools/mt5/test_export_session.py`.
11. [x] `new-extension`: the reach is a named port with registration-only
        edits, defaults preserve today's behaviour, a second implementation is
        exercised in a test, and the blast radius (added vs. edited files) is
        stated in the PR body.
12. [x] **Drivable without a mouse**: the reach and the venue-lead-in option
        are reachable by a named call and readable back — env hook for the
        harness, and no capability that only a click can reach.
13. [x] `ui-harness` hook for every new/changed surface, added in this change;
        `visual-qa` with all surfaces PASS or defects explicitly accepted;
        `trader-ux-review` with no unresolved Blocker.
14. [x] `arch-review` run over `git diff main...HEAD`, every Blocker and
        Should-fix resolved or deferred in the PR body; **PR opened**.

## Out of scope

- History paging in the MQL5 EA (`bridge/mt5/QuantickBridge.mq5`). It declares
  neither `history_paging` nor `rates` and keeps behaving exactly as today.
- Any change to bar construction in `quantick-engine`.
- A venue calendar or timezone-aware session table. The session open is found
  in the tape.

## Result (2026-08-29)

Delivered in four commits on `origin/main` (`03b9b1c`), as
https://github.com/milocaetano/quantick/pull/253. All four checks green (1883
passed, 0 failed); `language_guard` passes.

Every criterion met, with three deliberate deferrals recorded in the PR body:
the Portuguese branch name (assigned by the session harness, which forbids
pushing elsewhere — worth renaming on merge); the indicator seam over mixed bar
populations, resolved by disclosure rather than by hiding the prefix from a
trade-cut chart's indicators; and the absence of a registered control-plane
*action* for the reach, where the state is readable and the named setter exists
but the capability descriptor is a separate change.

**Criterion 10** was not applicable rather than skipped: nothing under
`bridge/mt5/` or `tools/mt5/` was touched.

**Criterion 13** ran further than expected. The capture workflow in
`ui-harness` is written for Windows, but the app launches under Xvfb with
llvmpipe once `libxkbcommon-x11` is installed, so three states were captured
here: the default (`0+14 bars`, no candle request made), the lead-in on a
replayed tick chart (`240v+0+14 bars`), and one press of `+ older` against a
scripted MetaTrader bridge — eleven pages, `action="reach_met"`, stopping 3.05
hours into the previous session. `trader-ux-review` found two Should-fixes and
both are fixed.

**The evidence that mattered most** was the control run: with the lead-in off
the tab issued zero `FetchOhlcv`, which proves the default is untouched rather
than merely asserting it.

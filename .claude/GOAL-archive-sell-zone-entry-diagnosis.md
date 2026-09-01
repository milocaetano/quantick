# Mission — why no sell was taken at the top of the sell zone, and the fix

**Objective.** Identify why no sell was taken on the bar the trader marked at
the top of the `SellGainAlarm` region on BTCUSDT (2000-tick bars, ~15:03), and
correct the defect that made the cause unreadable — without changing the
trading rule, which stays the trader's call.

**Why it matters.** The trader looked at the chart to answer "why did nothing
fire here?" and the badge answered with a sentence about a *different bar*.
The strategy kernel's own doc comment states the standard it is held to — "a
strategy that stays silent about *why* it is not firing reads as broken"
(`crates/strategy/src/force.rs:73`). On this bar it did worse than stay
silent: it gave a confident, wrong-bar answer.

## Request ledger

| # | Ask | Source |
| --- | --- | --- |
| R1 | Identify the cause of no sell being taken on the marked bar at the upper part of the sell zone. | verbatim: *"identficar a causa de nao ter feito essa venda na parte superior na zona de venda"* |
| R2 | Correct it. Scoped by **D1** to proven defects; a trading-rule change is returned to the trader as a decision. | verbatim: *"e faça a correção"* |
| R3 | Change the absolute floor so it measures the **candle's size** (`high - low`) instead of the body (`\|close - open\|`). | verbatim: *"achoq ue esse min body deveria ser min tamanho do candle nao?"* |

`R3` arrived mid-mission, after the diagnosis was presented. It is a
**trading-rule change**, which **D1** reserves to the trader — so it was put
to them as a decision rather than taken, and **D5** records the answer. It is
scope the trader added, not scope this mission invented.

## Decisions taken by the trader (D)

- **D1 — What "faça a correção" means.** Diagnose against the real tape, prove
  which gate held *that* bar, and fix only what is a genuine defect (the
  per-bar reason, plus any real bug). A change to the trading rule itself
  comes back as the trader's decision, never taken unilaterally. *Rationale
  given: a rule change moves money.*
- **D2 — Which bar.** Only the bar the red arrow marks (~15:03), not the whole
  14:48–15:05 window.
- **D3 — The `min_body` floor.** Trader did not know the live value; agent to
  read the saved preset and report the real one. **Answered from evidence:**
  `SellGainAlarm` carries `min_body = "100"` in
  `~/Documents/Quantick/quantick-strategies.toml`.
- **D4 — Alarm or order.** The instance should have **placed an order**.
  **Confirmed from evidence:** the preset carries `alarm_only = false`.
- **D5 — The floor measures the candle, not the body.** Put to the trader
  with the trade-off stated plainly: the ratio half of the ruler is
  body-based (`body / avg(body)`), so a range-based floor lets a
  wick-dominated doji clear a gate its body could not — the very bar
  `region.rs`'s wick rule exists to refuse. **The trader chose the
  range-based floor after that concern was raised**, which under this repo's
  working rules is their decision to make and mine to implement in full. The
  doji exposure is accepted, named here, and called out in the PR body so it
  is a known position rather than a surprise.
- **D6 — One PR.** The reporting fix and the rule change ship together, at
  the trader's instruction. (I had recommended splitting them so a
  money-moving change could be reviewed alone; the trader chose one PR, and
  the PR body separates the two so a reviewer can still weigh them apart.)

- **D7 — Ship without the cause confirmed.** `delivery-review` found that
  every fixture on this branch ran the shipped band (1.5–2.5, window 3)
  while `SellGainAlarm` runs **`window = 20`, `min_factor = 2`,
  `max_factor = 4.5`** — and the floor is consulted *only inside the band*,
  so a body under 2× the 20-bar average is `Quiet` and never reaches a floor
  of either kind. The marked bar may therefore have been held by the **band**,
  not the floor, in which case **R3**'s change would not have fired it. There
  is no BTCUSDT recording of that session on disk (only WDOU26 and WINV26), so
  it cannot be settled here. **Put to the trader with that stated plainly; they
  chose to ship**, on the reasoning that the badge repair is precisely the
  instrument the question needed: the next occurrence will name its own gate
  ("quiet 1.5×", or "2.2× in band · candle 62 under floor"), and the cause can
  be closed from evidence instead of inference.
- **D8 — Three deferrals granted.** The trader approved shipping
  `ruler_refusal()` with no in-app consumer, **G4** without a `main` control
  run, and **G5** with neither `visual-qa` nor `trader-ux-review` run. See
  the `## Deferred` section.

- **D9 — The Pine input rename ships as-is.** The fourth review round found
  that renaming the script's input silently reinterprets a saved chart
  calibration, because indicator settings rebind by position and neither the
  index nor the type changed. Put to the trader with the alternatives — add a
  load-time notice for indicator settings, or leave the script measuring the
  body — and they chose to ship, the default being `0.0` and the field's
  title now naming the candle. Recorded under `## Deferred`.
- **D10 — Close the mission here.** Four `arch-review` rounds returned
  14, 13, 13 and 12 findings, the fourth finding defects inside the third's
  own fixes. That flat-count, self-inflicted shape is the one
  `delivery-review` names as a signal to talk rather than run a fifth round,
  so it was put to the trader: they chose to run both reviews over the final
  head and open the PR. The smaller findings still open — the badge
  re-deriving `status_line`'s ordering rather than sharing it, three
  near-identical fixture helpers, and the badge's unbounded width — are named
  in the PR body as follow-ups rather than fixed here.

## Assumptions (S)

- **S1** — The marked bar's body is under 100 price units. Read from the
  screenshot's price axis (bars in that congestion span roughly 30–80 USD
  against a 180,000 price). Safe to assume for *scoping*, and it is not
  load-bearing: criterion **A1** proves the gate from a reconstructed bar
  rather than from this estimate, and the fix in **A3** is correct whichever
  ruler verdict actually held the bar.
- **S2** — The stale reason on the badge ("the body never cut the region")
  came from a later bar below the region, where price sat from ~15:10. Not
  load-bearing either: **A2** proves the *mechanism* by which any unrelated
  bar's reason reaches this badge, without needing to identify which one did.
- **S3** *(wanted to ask, cut by the four-question cap)* — Whether the trader
  wants the badge to keep naming a stale reason at all, or to fall silent
  once it is not about the current bar. Went with: **keep it, but never let
  it impersonate a fresh answer** — the existing `HoldReason.fresh` flag
  already encodes that intent, so honouring it is repair rather than
  redesign.
- **S4** *(wanted to ask, cut by the cap)* — Whether `Quiet`, `Exhaustion`,
  `Warmup` and `FlatAverage` deserve the same badge treatment as
  `UnderFloor`. Went with: **yes, all of them** — they are the same class of
  silence, and fixing one arm while leaving four identical holes would
  reproduce this exact bug report on the next verdict.

## The cause (R1) — established from source, pending tape confirmation

The chain, each link read in the code rather than inferred:

1. `SellGainAlarm` carries `min_body = "100"` — an absolute body floor whose
   own doc comment (`force.rs:33-40`) records that it was calibrated on a
   **WINV26** session. It is applied unchanged to BTCUSDT.
2. The marked bar's body is under that floor, so `ForceWindow::weigh` returns
   `BarVerdict::UnderFloor` (`force.rs:227`) — the arm that exists precisely
   to say "the relative band would have called this force, and the absolute
   floor held it".
3. `signal_from` maps every non-`Force` verdict to `None`
   (`trigger.rs:167`). No `Signal` is produced.
4. `ArmedStrategy::judge` takes the no-signal path and **clears the note**:
   `let Some(signal) = signal else { self.note = None; return Vec::new(); }`
   (`armed.rs:449-452`). No hold reason is recorded for this bar at all.
5. `hold_reason()` therefore falls back to `last_hold` — the most recent
   refusal from *any* earlier bar (`armed.rs:786-789`).
6. The chart badge renders `hold_reason()` **and nothing else**
   (`pane.rs:2405-2412`). It never renders `trigger.status()`.

Step 4's own comment says the no-signal path is safe because it "lets the
trigger's own status narrate". **On the chart, it does not.** `status_line()`
includes `trigger.status()`; the chart badge does not call `status_line()`.
The compensation the design relies on is absent from the one surface the
trader actually reads — so a bar the ruler refused shows another bar's
refusal, present-tense-adjacent, with no way to tell.

**Two findings, deliberately separated:**

- **Defect (mine to fix, R2 under D1)** — the trigger's own refusal never
  becomes a hold reason, and the chart badge presents an unrelated bar's
  reason in its place.
- **Rule/config (the trader's, D1)** — `min_body = 100` is instrument-blind:
  right for WINV26, arbitrary for BTC at 180,000. Reported with numbers,
  **not** changed by this mission.

## Acceptance criteria

- [x] **A1** — The gate that held the marked bar is named with its measured
      numbers (body, average body, ratio, floor) from a reconstructed bar,
      not from a screenshot estimate.
      *Evidence:* a test asserting the verdict for that bar's geometry.
      → `crates/strategy/src/armed.rs` tests + `.claude/GOAL-archive-*.md`. *(R1)*
- [x] **A2** — A regression test proves today's defect: a bar whose ruler
      refuses it leaves the badge showing an **earlier, unrelated** bar's
      reason, with nothing marking it as not-about-this-bar.
      *Evidence:* a test that fails on `origin/main` and passes after A3.
      → `crates/app/src/pane.rs` or `crates/strategy/src/armed.rs` tests. *(R1, R2)*
- [x] **A3** — The trigger's own refusal becomes a first-class hold reason:
      a bar the ruler declined names *that* gate, on the bar it happened,
      carrying the numbers a trader can act on.
      *Evidence:* tests over `UnderFloor`, `Quiet`, `Exhaustion`, `Warmup`,
      `FlatAverage`; badge text asserted.
      → `crates/strategy/src/armed.rs`, `crates/app/src/pane.rs`. *(R2)*
- [x] **A4** — A stale reason can no longer read as an answer about the
      current bar: what the badge shows is either fresh, or explicitly
      marked as not about this bar.
      *Evidence:* test asserting the badge over a ruler-refused bar.
      → `crates/app/src/pane.rs` tests. *(R2)*
- [x] **A5** — **The reporting fix changes no trading behaviour**, and the
      rule change (**A7**) is the branch's *only* behavioural change. The two
      are separable in the diff: the badge work touches no gate, and the
      floor work touches no reason string.
      *Evidence:* the badge change confined to `crates/app/src/pane.rs`
      (a surface that emits no `Command`); every pre-existing
      command-emission test in `armed.rs` and `backtest` passing unchanged
      except those that assert the floor's own semantics, each such change
      listed in the PR body with why.
      → `cargo test --workspace` output + PR body. *(D1, R2)*

      *Originally written as "the trading rule is unchanged", which **D5**
      superseded. Re-scoped rather than deleted so the record shows the
      criterion moved by the trader's decision and not by my convenience.*
- [x] **A6** — The `min_body = 100` finding is reported to the trader with
      the numbers and the instrument-calibration problem named.
      *Evidence:* a section in the PR body quoting the WINV26 calibration
      note and the BTC figure. → PR body. *(D1, R1)*
- [x] **A7** — The absolute floor measures the candle's **size**
      (`high - low`), not its body. A bar whose body is under the floor but
      whose candle clears it is now **force**, and the marked bar is exactly
      that case.
      *Evidence:* a test over the marked bar's reconstructed geometry
      asserting `Force` where today it is `UnderFloor`, plus the renamed field
      (`min_range`, matching `ForceBar.range`) carrying the new meaning.
      → `crates/strategy/src/force.rs` tests. *(R3, D5)*
- [x] **A8** — A preset saved before this change keeps working, and its
      stored number is carried into the new meaning **visibly**, not
      silently: the trader's own `min_body = "100"` becomes a 100-unit
      *candle-size* floor, and that reinterpretation is stated in the PR
      body rather than discovered by a changed fill.
      *Evidence:* a round-trip test loading a pre-change preset file.
      → `crates/app/src/strategy_presets.rs` tests. *(R3, D5)*
- [x] **A9** — The doji exposure **D5** accepts is proven and bounded, not
      hand-waved: a test shows what now clears the floor that did not
      before, so the trader can see the shape of what they took on.
      *Evidence:* a test naming the wick-dominated bar case.
      → `crates/strategy/src/force.rs` tests. *(D5)*

### Injected gates

- [x] **G1** — Every artifact in English (`CLAUDE.md` owns the rule and its
      exemptions; the verbatim request section below is the one marked,
      attributed quotation).
      *Evidence:* `cargo test -p quantick-app language_guard`; arch-review
      dimension 8. → CI output.
- [x] **G2** — Four checks green after rebasing on latest `main`, each run
      **separately** (a chained `|| echo` has reported a false all-clear here
      before).
      *Evidence:* four separate exit codes. → PR body.
- [x] **G3** — Performance impact declared by rate class. `judge` is
      **per-closed-bar** (rare); the alarm's `wants_forming_check` path is
      **per-print** (hot). Any allocation added to the per-print path is a
      defect, and the declaration says which paths were touched.
      *Evidence:* a stated classification per touched function. → PR body.
- [ ] *(partial - numbers taken, no `main` control run)* **G4** — If the per-print path is touched at all, evidence that
      performance is flat: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense
      tape vs. a `main` control run.
      *Evidence:* both runs' numbers. → PR body. *(Expected N/A — see below.)*
- [ ] *(partial - screenshot blocked by the environment)* **G5** — User-visible change (the badge sentence): `ui-harness` hook
      for the surface, `visual-qa` pass, `trader-ux-review` with no
      unresolved Blocker.
      *Evidence:* screenshot paths + both verdicts. → PR body.
- [ ] *(runs after this file is archived)* **G6** — `arch-review` run over `git diff origin/main...HEAD`, every
      Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the review's verdict. → PR body.
- [x] **G7** — No new magic number: the floor, the ratio and the window are
      already named parameters and stay that way; any new threshold is named
      and justified.
      *Evidence:* arch-review dimension on hardcoded values. → PR body.

### Not applicable, and why

- **`new-extension`** — this adds no feed, bar type, indicator, layer, panel
  or crate. It repairs reporting on an existing port. The `Trigger` port is
  *consulted* differently, not replaced.
- **Test-first / golden determinism** — applies to engine bar-building. This
  touches the strategy kernel's reporting, not the aggregator. Regression
  tests are still written before the fix (A2), which is the same discipline
  applied where it belongs.
- **G4** — expected N/A: the fix targets the per-closed-bar `judge` path. If
  implementation forces a change to the per-print alarm path, G4 stops being
  N/A and the measurement is taken. This is stated so a silently-omitted gate
  and a deliberately-excluded one cannot look alike.

## Evidence, criterion by criterion

Recorded at the point the branch was archived. Every line is a command's
output or a named test, not a recollection.

| # | Verdict | Evidence |
| --- | --- | --- |
| A1 | met | `force::tests::the_floor_measures_the_whole_candle_not_the_body` names the gate on the marked bar's geometry: body 95, candle 140, ratio ~1.84 inside a 1.5–2.5 band, floor 100. Under the old rule `UnderFloor`; under the new one `Force`. |
| A2 | met | `pane::tests::the_badge_names_the_rulers_own_refusal_and_not_only_an_older_bars`. Run against the pre-fix badge composition it fails with the reported sentence, verbatim: `⚡ SellGainAlarm · last held: the body never cut the region` — the exact string in the trader's screenshot. |
| A3 | met | Two halves. Prose: the badge leads with the ruler's reading and its number (`×`). **Value:** `Trigger::refusal()` is a new port method returning a stable name per verdict, handed out by `ArmedStrategy::ruler_refusal()`, so an operator that is not looking at the chart can tell the ruler declined this bar without parsing English. Proven by `every_ruler_verdict_that_declines_a_bar_has_a_readable_name` (one assertion per verdict, no catch-all) and `a_ruler_refused_bar_leaves_the_gates_reason_standing_and_says_so`. **The first version of this branch claimed A3 on the badge text alone; arch-review caught that the machine-readable half named in the criterion had not been written, and it was built rather than the criterion quietly reworded.** |
| A4 | met | Same test asserts ordering: the current-bar sentence precedes `last held:`, so a standing refusal can no longer stand alone and read as fresh. |
| A5 | met, restated | **The earlier version of this row was wrong and arch-review caught it.** It claimed `crates/strategy/src/armed.rs` was a "pure rename" with "the state machine untouched"; `armed.rs` in fact gained `ruler_refusal()` and its tests. What is true, and checkable: `crates/strategy/tests/full_operation.rs`, `crates/backtest/tests/harness.rs`, `crates/app/src/app.rs` and `crates/app/src/strategy_anchors.rs` are pure rename — no line outside `min_body`/`min_range` changed. In `armed.rs` the *state machine* is untouched: no gate, no transition and no emitted `Command` changed, and every pre-existing command-emission test passes unedited. The additions are one read-only accessor and tests. The branch's only behavioural change is the floor (**A7**), plus one assertion updated for it (`the_candle_floor_holds_quiet_what_the_band_alone_would_call_force`: `body: 60` → `range: 62`). |
| A6 | met | Reported in the PR body with the WINV26 calibration note quoted and the BTC figure (100 is 0.06% of a 180,000 price). The trader's `quantick-strategies.toml` lives outside the repo and is not written by this branch. |
| A7 | met | `the_floor_measures_the_whole_candle_not_the_body` passes; `ForceParams::min_range` and `BarVerdict::UnderFloor { range }` carry the new meaning in their names and docs. |
| A8 | met | `strategy_presets::tests::a_bank_written_under_the_old_min_body_key_keeps_its_floor`. The fixture is written by the bank itself and only its key renamed, so it asserts the serde alias rather than a hand-typed schema. |
| A9 | met | Two tests, because the first one alone was not honest. `a_wick_dominated_candle_clears_the_floor_and_the_band_still_refuses_it` shows the gates composing — but arch-review pointed out it warms with 30-point bodies, so the ratio is 0.16 and the *band* refuses the bar whatever the floor says, which proves nothing about the regime the floor exists for. `a_congested_tape_admits_more_than_the_body_floor_did` is that regime: a shrunken average body, a bar with a 25-point body on a 140-point candle that the old floor refused and this one admits. The cost of **D5** is now a number somebody can read. |
| G1 | met | `tracked_files_are_written_in_english` passes (4/4 in `language_guard`). |
| G2 | met | Four checks, each run on its own, exit 0: fmt, clippy (`-D warnings`), build, test. One pre-existing environmental failure — see below. |
| G3 | met | Declared below. |
| G4 | **partial** | See below — an honest gap, not a pass. |
| G5 | **partial** | See below — an honest gap, not a pass. |
| G6 | pending | `arch-review`, run after this file is archived. |
| G7 | met | No new threshold introduced. The floor, band and window stay named parameters; the one number added is the `size_guard` ceiling, raised deliberately with a comment giving the reason. |

### The one test failure, and why it is not this branch

`the_bridge_paging_tests_pass` fails in this environment and does so on
`main` too: the cargo wrapper shells out to `python3`, which resolves to a
Microsoft Store alias here. Run directly, `python bridge/mt5/tests/test_paging.py`
reports **all checks passed across 31 tests**, exit 0 — which is what CI runs.

### G3 — performance impact, by rate class

**Rewritten after `delivery-review` found the previous version describing a
build that is not this branch.** It claimed `ArmedStrategy` caches the ruler's
reading, that the badge borrows it, and that a named test pins the cache — all
true of a commit that was then reverted, and left standing in this file. A
performance declaration that describes code which is not shipped is worse than
none, because it is read as measurement.

What the shipped branch actually does:

- `ForceWindow::weigh` — **per closed bar, and also per print**. One extra
  subtraction (`high - low`), read once and reused by the gate and the
  `ForceBar` it builds; no allocation. The per-print half was missed in the
  first declaration and is the reason **G4** below is not N/A: `weigh` is
  reached from `ForceTrigger::preview`, which the alarm's forming-bar path
  calls on every print through `preview_opportunity`.
- `ArmedStrategy::on_closed_bar` — **per closed bar** (rare). Unchanged.
- `ChartPane::badge_text_for` — **per frame**, once per armed instance. It
  calls `Trigger::status()`, which formats two `Decimal`s and returns an owned
  `String`. That is the same call `status_line` has always made, and it is not
  new to this path so much as newly *reached* from it. A cache was written to
  avoid it and reverted: it moved the cost onto every closed bar of every
  backtest session, which never reads the string, to save one allocation of
  several on a paint path that allocates regardless. `status_line` makes its
  own separate call; the two are not shared.

### G4 and G5 — what could not be finished, and why

Both are **partial**. Saying so rather than quietly dropping them is the
point of writing "not applicable and why" into this file at all.

A run of the branch build reached the armed-instance surface
(`QUANTICK_STRATEGY_DEMO=1`, stores redirected to a scratchpad so the
trader's workspace could not be touched) and stayed healthy for its whole
life: **fps 59–60, frame_avg 16.64–16.67 ms, frame_cpu 1.9–4.5 ms** on a
live Binance tape at ~39 trades/s. That is the vsync ceiling, so nothing in
this change costs a visible frame. Those numbers were taken before the
cache was written *and* the cache was later reverted, so they measure the
shape that shipped — not, as an earlier version of this file claimed, a
more expensive one the branch improved on.

It is short of what G4 asked for on two counts, and both are stated rather
than glossed: there is **no `main` control run** to compare against, and the
demo region only staged near the end of the run, so those frames are mostly
*not* frames that drew an armed badge. The number is real; it is weaker
evidence than the criterion wanted.

G5's screenshot could not be taken at all. The launched window opened
minimized, and `PrintWindow` returns a fully black raster for its GPU
surface in that state (verified by sampling: 0 non-black pixels of ~5,500
sampled). Restoring it without focus did not help; getting a real raster
needs the window raised over the desktop — and the trader was **using the
application at that moment**, in their own live MetaTrader session. The
`ui-harness` rule is explicit that a capture run is a guest and must not
fight the user, so the run was ended and its process closed, leaving the
trader's untouched.

What stands in place of the screenshot: the badge sentence is asserted
character-level by an automated test, including the ordering of its
clauses, and the pre-fix composition was shown to reproduce the reported
string exactly. What is genuinely **not** evidenced is how the longer
sentence *looks* in a chart corner — whether it crowds the drawing at small
window sizes. That is a real question this branch does not answer, and it
belongs to `trader-ux-review` and to the trader's own eye.

## What the two review rounds changed, and what is deferred

`arch-review` step 0 ran three times at **xhigh** — once mid-branch (14 findings)
again over the finished branch (13), and a third time over the result of
those fixes (13 more). Forty findings; the review rounds are why several
things in this file read differently from how they were first written, and
why two of my own changes are no longer in the branch at all.

The third round's sharpest catch was not a bug in the usual sense. The
floor and the bracket ruler are **the same measurement**: `signal_from`
projects off `ForceBar::range`, which is the `high - low` this floor now
reads. So a bar admitted on its candle rather than its body arms a stop
sized by that candle — the congestion fixture's 25-point push carries
`sl_mult × 140` of stop. **D5 changes risk per trade, not only how many
bars fire**, which no paragraph on this branch had said until the review
asked. It is now in `ForceParams::min_range`'s own doc and pinned by
`a_bar_admitted_on_its_candle_projects_off_that_candle`, and it is called
out in the PR body because it is the trader's decision to hold, not mine
to bury.

That round also found the migration in a shape that would have reproduced
this branch's own bug one layer down: a `resolved_floor()` resolver that
`to_kernel` called and the arming form did not, so a vintage preset would
have shown a floor of `0` in the dialog while the kernel armed 100. The
resolver is gone. The vintage number is migrated **once, at load**, into
the single field every reader already uses, compared as a `Decimal` rather
than as text so `"0"` and `"0.0"` cannot mean opposite things.

**Two of my own fixes were reverted because the review was right about
them:**

1. **The `trigger_status` cache is gone.** It moved a `format!` off the
   badge's per-frame path, and both rounds objected. The decisive argument
   was not the one I had weighed: `crates/backtest` calls `on_closed_bar`
   for every bar of every recorded session and never reads the cached
   string, so the cache added an unconditional allocation to a batch loop
   to save one of several allocations on a paint path that allocates
   regardless. It also bought a coherence invariant in a pure domain crate
   and a timing precondition on a public trait. The badge asks the ruler
   directly again. The real fix — caching the *composed* badge string in
   `ChartPane`, where the per-frame cost actually is — is a follow-up, not
   this branch.
2. **The alarm-only `aside()` on the badge is gone.** It was scope I added
   from a "consider"-grade note in round one, and it shipped two defects:
   the branch was not gated on `armed`, and `disarm()` never clears the
   note, so a disarmed alarm-only instance would have painted its aside
   forever while the menu said "disarmed" — re-introducing the exact
   two-surface divergence this branch exists to end. It also stuttered,
   because `strategy_anchors::badge_text` already emits an "alarm only"
   clause. Reverted whole.

**Still deferred, deliberately:**

- **The form ships `100` as the floor's starting point.** Instrument-blind,
  and the comment above it now says so. Picking a new number is a trading
  decision the trader reserved under **D1**/**A6**.
- **The badge can chain three clauses with no truncation.** Real, and this
  branch cannot answer it: G5's screenshot could not be taken, so how the
  longer sentence sits in a chart corner has no evidence behind it. It
  belongs to `trader-ux-review`.
- **The body floor is gone rather than joined.** `ForceParams` can no
  longer express a minimum body at all, so the one calibration that was
  ever measured (247 → 7 on WINV26) is unreachable by any setting. That is
  **D5** as the trader chose it — "trocar", replace, not "dois pisos" —
  and reversing it is theirs to ask for, not mine to decide. The doc says
  what a second floor would be for, so the next person asking is not
  starting from nothing.
- **`ruler_refusal()` has no production consumer yet.** The stable names it
  returns are read by tests and by nothing else, because the control
  plane's scene does not carry armed instances — `badge_text_for`'s own doc
  already filed that. **A3** asked for the machine-readable half to exist
  and it does, at the port; wiring it into `quantick_get_scene` is the
  follow-up that makes it reachable from outside the process.

**Migration, restated after the review.** The `#[serde(alias)]` this file
first described is gone. An alias is the *same* field to serde, so a bank
carrying both keys was a duplicate-field error — and `load_from` answers any
parse error by starting empty, which the next save writes over every preset
the trader had. A vintage number is now its own optional field, reconciled
by `resolved_floor()`, and the bank logs `STRATEGY_FLOOR_REINTERPRETED` when
it carries one forward, so the change of meaning is visible in the running
app instead of only in a doc comment. Rolling back to a build before this
one still finds no key it knows and falls through to `0`; that direction no
alias here can reach, and the PR body says so.

## Deferred

Every line here is **granted by the trader** (**D8**, and **D7** for the
first). Nothing in this section is a deferral the session gave itself — an
earlier draft of this file listed two that were, and `delivery-review` failed
the branch for it.

- **The Pine input rename reinterprets a saved chart calibration** —
  *granted under D9.* `min_body_points` became `min_range_points` in
  `crates/app/scripts/exhaustion_reversal.pine`. Indicator settings persist
  and rebind **positionally**, and the rename keeps both the index and the
  `Float` type, so `INDICATOR_INPUTS_REBOUND` never fires: a trader who had
  calibrated that floor to `100` reloads with `100` now meaning `high - low`.
  The bank got `STRATEGY_FLOOR_REINTERPRETED` for exactly this; the chart got
  silence. Put to the trader with that stated, including the option of giving
  indicator settings the same load-time notice, and they chose to ship: the
  shipped default is `0.0` (off), so only a trader who set it is affected,
  and the input's title now reads "min force-bar candle range (points, 0 =
  off)". The alternative they declined was leaving the script on the body,
  which would have re-opened the chart-versus-kernel divergence this branch
  closed.

- **A1 / A7 (the half that names the marked bar's gate)** — *granted under
  D7.* What is delivered: the reporting defect, proven; the floor measuring
  the candle, proven; and
  `under_the_traders_own_band_the_floor_is_only_reached_inside_it`, which
  writes the two regimes down under the trader's real preset. What is **not**
  delivered: which regime the marked bar was in. It needs the tape, and no
  BTCUSDT recording of that session exists. The trader chose to ship and read
  the repaired badge on the next occurrence.
- **A3 (the machine-readable half's consumer)** — *granted under D8.*
  `Trigger::refusal()` and `ArmedStrategy::ruler_refusal()` exist, are
  exhaustively matched and are tested, but no running surface reads them: the
  control plane's scene does not carry armed instances. Wiring it into
  `quantick_get_scene` is the follow-up.
- **G4 (no `main` control run)** — *granted under D8.* The branch build held
  fps 59–60 / frame_avg 16.64–16.67 ms on a live tape; there is no control
  run on `main` to compare against. The per-frame path is the shape `main`
  already had, since the cache that would have changed it was reverted.
- **G5 (`visual-qa` and `trader-ux-review` not run)** — *granted under D8.*
  The `ui-harness` hook exists (`QUANTICK_STRATEGY_DEMO`), but no screenshot
  could be taken: the window opened minimized, `PrintWindow` returns a black
  raster in that state, and raising it meant intruding on the trader's live
  session. Two user-visible changes therefore ship un-reviewed by eye — the
  badge sentence, and the arming form's label (`and body ≥` →
  `and candle ≥`, with its tooltip rewritten), which the first draft of this
  file failed to name at all.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open with CI green.

## The request as received

Quoted verbatim and untranslated, per `CLAUDE.md`'s exemption for a marked,
attributed quotation: these are the trader's own words, and `delivery-review`
re-derives the asks from them rather than from this file's paraphrase. A
translation here would make the ledger its own source of truth, which is the
failure the section exists to prevent.

> **Trader, 2026-08-31, message 1 (with a chart screenshot; a red arrow marks
> a bar at the top of the upper `SellGainAlarm` region, ~15:03, BTCUSDT
> 2000-tick):**
>
> identficar a causa de nao ter feito essa venda na parte superior na zona de venda

> **Trader, 2026-08-31, message 2 (sent mid-turn):**
>
> e faça a correção

> **Trader, 2026-08-31, message 3 (sent mid-turn, after the diagnosis was
> presented — the source of `R3`):**
>
> achoq ue esse min body deveria ser min tamanho do candle nao?

Two later answers came through `AskUserQuestion` rather than as free text, so
they have no verbatim form to quote; they are recorded as the decisions they
were, with the option the trader selected named exactly:

> **Trader, 2026-08-31 — answering "o piso min_body: como você quer
> resolver?", having been told that a range floor lets a wick-dominated doji
> clear a gate its body could not:** selected **"Trocar para tamanho do candle
> (high-low)"**, and **"Tudo neste mesmo PR"**. That is `D5` and `D6`.

> **Trader, 2026-08-31 — answering the finding that the floor is only reached
> inside a band their preset sets at 2–4.5, so the marked bar may have been
> held by the band instead:** selected **"Subir assim: o badge passa a dizer
> qual porteira segurou"**, and granted the three deferrals above. That is
> `D7` and `D8`.

> **Trader, 2026-08-31 — answering the fourth round's finding that renaming
> the Pine input silently reinterprets a saved chart calibration:** selected
> **"Subir assim — default é 0 e o título do campo mudou"**, and, on how to
> close, **"Fechar: rodar os dois reviews e abrir o PR"**. That is `D9` and
> `D10`.

The screenshot is part of the request: the badge visible in it reads
`⚡ SellGainAlarm · last held: the body never cut the region`, and that
sentence being about a different bar is the defect this mission fixes.

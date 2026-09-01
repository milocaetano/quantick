# GOAL — the MetaTrader chart opens on the whole session

**Mission**: make the MetaTrader tick backfill anchor on the *trading session*
instead of the wall clock, so opening the mini index chart at any hour shows
the day from its first print — and deliver it progressively, so the chart is
usable while the morning fills in behind it.

Branch: `feat/mt5-session-history` ·
worktree `../quantick-worktrees/feat-mt5-session-history`

## Why this matters

The trader opened WINV26 today and the chart began at 09:30. It is the fourth
attempt at this problem. The reason it keeps surviving a PR is that the
symptom ("not enough history") has two independent causes, and each previous
mission fixed one of them.

Measured against the trader's own running terminal on 2026-08-31 at 22:10 local
(`XPMT5-PRD`, WINV26), before any change:

| Fact | Value |
| --- | --- |
| Session held by the terminal | 09:03:00.233 → 18:31:23.324 |
| Trade ticks in that session | 1 525 621 |
| Time for the terminal to return the whole day | 0.12 s |
| `copy_ticks_range(now − 720 min, now)` returns | oldest **13:10:32** — 4 h of the session missing |
| `backfill_max_ticks` default | 1 000 000 — **525 621 oldest ticks dropped** even with a correct window |

1. **The window is anchored on the clock.** `Bridge.backfill`
   (`bridge/mt5/quantick_bridge.py:543`) asks for `now − backfill_minutes` with
   a 720-minute default. Its own docstring says "The trading day, so the chart
   opens on the session rather than on a sliver of it", and
   `bridge/mt5/README.md` documents the default as "720 — a whole B3 session".
   Neither is true: a rolling 12-hour window covers a 09:00 session only while
   the clock reads between roughly 18:25 and 21:00. Open at 22:10 and it starts
   at 13:10; open at 21:30 and it starts at 09:30, which is the report.
2. **The cap amputates what the window did reach.** A B3 day of the mini index
   is now over 1.5 M prints against a 1 000 000 ceiling that keeps the newest.
   Fixing the window alone still opens the chart mid-morning.

The app already knows how to find a session boundary without a calendar —
`crate::history_reach::SESSION_GAP_MS` reads it off the tape as a print gap —
and the bridge does not use that idea at all. Two surfaces that should agree.

## Decisions taken by the trader (2026-08-31)

- **D1 — The opening block is the whole session.** Anchored on the session's
  first print, not on the clock. Outside trading hours it is the last session
  the terminal holds, whole. Chosen over "today + N previous sessions" and over
  a session-anchored window measured in hours.
- **D2 — The chart opens immediately and fills backwards.** The newest slice
  paints first; older slices land behind it with progress shown. Chosen over
  waiting 15–30 s for one complete block, given the measured 157 MB / ~7 s
  serialize cost of a full WIN day.
- **D3 — The Python bridge only.** `quantick_bridge.py` is what the app
  autostarts, so it is the real path. `QuantickBridge.mq5` keeps its 30-minute
  `InpBackfillMinutes` and is left declaring honestly what it delivers.
- **D4 — One `+ older` press reaches a trader-chosen time span.** The tick step
  is derived from that span rather than being the fixed 2 000 trades that is
  meaningless against a 1.5 M-print day.

## Request ledger

| # | Ask | Where it is discharged |
| --- | --- | --- |
| R1 | Fix MetaTrader tick-history loading definitively — verbatim *"resolver de vez"*, meaning the class of defect goes, not this instance of it | A1, A5, A7 |
| R2 | The defect observed today goes: the mini index chart opened and did not load the past, verbatim *"Parou em 9h30"* | A1, A2 |
| R3 | Opening the day on a **tick** chart must show what happened earlier in that day | A2, A4 |
| R4 | Data that exists in the terminal must reach the chart instead of being left behind by a ceiling — verbatim *"Se existe esses dados no metatarder, pq não colocar?"* | A3, A5 |
| R5 | Purpose, and the ask that judges the others: several PRs already tried this and none solved it, so this one is proven against the real terminal rather than against fixtures alone | A6, A7 |

## Assumptions

- **S1** — A session boundary is *observed*, never tabled. The bridge finds the
  session's first print by walking back over a print gap, the same idea
  `SESSION_GAP_MS` already encodes in the app. Safe to assume rather than ask:
  a hardcoded 09:00 is a venue calendar, which `arch-review` rejects as a magic
  number and which would be wrong for the Tickmill CFDs on the same bridge.
- **S2** — Progressive delivery extends the existing `backfill_start` /
  `backfill_end` markers rather than replacing them, so the feed's "history is
  done" contract and an unchanged EA both keep working. Safe: the protocol is
  versioned and additive, and `PROTOCOL.md` is its owner.
- **S3** — D3 leaves the EA's behaviour alone; "declaring honestly" is
  discharged in prose (`bridge/mt5/README.md` and the `InpBackfillMinutes`
  comment) rather than by changing EA code, which D3 puts out of scope.
- **S4** — *wanted to ask, cap reached.* The default span for D4's reach. Taken
  as **2 hours**, configurable, with dead time crossed rather than counted — a
  press that lands in an overnight gap continues to the previous session's
  close instead of returning empty. Reversible in one edit; recorded here
  because the "crossed, not counted" half is a design choice the trader did not
  make.
- **S5** — *wanted to ask, cap reached.* Whether the 1 M tick cap should be
  raised or removed. Taken as: it stops being a *span* limiter (the chosen
  session is delivered whole) and survives only as a memory bound far above one
  session, with any cut it does make said on the chart rather than only in the
  log. Follows from R4; asking would have spent a slot on a number the trader
  had already answered in principle.

## Acceptance criteria

### Mission-specific

- [x] **A1** — The Python bridge's opening backfill starts at the session's
      first print, found by walking back over a print gap, for any clock time
      the bridge is started at. *Evidence:* unit tests over synthetic tick
      sequences (an overnight gap, a weekend, a market that never closes, a
      terminal holding less than one session) asserting the chosen `from`
      instant. → `bridge/mt5/tests/test_session_backfill.py`. *(R1, R2)*
      → **MET.** `bridge/mt5/tests/test_session_backfill.py` — 17 checks over an evening open, a mid-session open, a pre-open, a weekend, two sessions on disk, a never-closing market, a young contract, an unheld symbol, ordering, the cursor, the cap, and a terminal misreporting its own floor.

- [x] **A2** — Against the live terminal, a bridge started outside trading
      hours delivers WINV26 from 09:03, not 13:10 and not 09:30. *Evidence:*
      captured bridge stderr plus the app's `MT5_HISTORY_READY` line naming the
      oldest instant, before and after. →
      `.claude/evidence/mt5-session-history/whole-day.md`. *(R2, R3)*
      → **MET.** [`.claude/evidence/mt5-session-history/whole-day.md`](.claude/evidence/mt5-session-history/whole-day.md) — a real bridge process onto a real socket delivering 1 525 621 ticks from **09:03:00.233**, and [`.claude/evidence/mt5-session-history/in-the-app.md`](.claude/evidence/mt5-session-history/in-the-app.md) — the desktop app holding 30 510 bars of it.

- [x] **A3** — No ceiling silently drops what the terminal holds for the
      session. A cut that does happen is visible on the chart, not only in the
      log. *Evidence:* a test asserting truncation raises a user-visible
      notice, plus a live run over the 1 525 621-tick day reporting zero
      dropped. → `.claude/evidence/mt5-session-history/no-silent-cut.md`. *(R4)*
      → **MET.** [`.claude/evidence/mt5-session-history/no-silent-cut.md`](.claude/evidence/mt5-session-history/no-silent-cut.md) — the cap raised to 4 M against a measured 1.53 M day, and a cut now reported through `bridge_log` as an on-chart `Attention` naming `+ older`.

- [x] **A4** — The chart paints before the session has finished loading: newest
      slice first, older slices behind it, progress visible. *Evidence:*
      protocol tests for the sliced block, plus a `visual-qa` screenshot series
      showing a populated chart with the fill still running. →
      `.claude/evidence/mt5-session-history/progressive/`. *(R3)*
      → **MET.** Protocol tests in `crates/feed-mt5` and slice tests in `test_session_backfill.py`; the chart caught mid-fill at `7999+0 bars` with six slices still to come in [`.claude/evidence/mt5-session-history/progressive/mt5-mid-fill.png`](.claude/evidence/mt5-session-history/progressive/mt5-mid-fill.png), against `30510+0 bars` complete. The on-screen countdown is the approved deferral below.

- [x] **A5** — One `+ older` press reaches a trader-chosen time span, as a new
      `HistoryReach` variant with its control; the existing reaches keep
      today's behaviour. *Evidence:* campaign stop-condition unit tests for the
      span reach, and a screenshot of the control. →
      `crates/app/src/history_reach.rs` tests +
      `.claude/evidence/mt5-session-history/older-span.png`. *(R1, R4)*
      → **MET.** `HistoryReach::Span` with campaign tests over traded time, a night crossed not counted, the early exit and the unreachable-span stop; the control in `.claude/evidence/mt5-session-history/shots/older-span-menu.png`, absent under another reach in `.claude/evidence/mt5-session-history/shots/reach-previous-session-menu.png`.

- [x] **A6** — The fix is proven against the trader's own terminal, not only
      against fixtures: a recorded before/after probe with the numbers carried
      into the PR body. →
      `.claude/evidence/mt5-session-history/terminal-probe.md`. *(R5)*
      → **MET.** [`.claude/evidence/mt5-session-history/terminal-probe.md`](.claude/evidence/mt5-session-history/terminal-probe.md) — before/after against `XPMT5-PRD`, with the runnable probe beside it and the drift of the old window recorded across three runs.

- [x] **A7** — The bridge and the app cannot drift about where a session
      starts: one owner for the gap threshold, guarded by a test that fails if
      the Python and Rust values diverge. *Evidence:* the guard test, run red
      once by changing one side. → `crates/app/tests/` guard +
      `.claude/evidence/mt5-session-history/agreement-guard.md`. *(R1, R5)*

### Injected gates
      → **MET.** [`.claude/evidence/mt5-session-history/agreement-guard.md`](.claude/evidence/mt5-session-history/agreement-guard.md) — `crates/app/tests/session_gap_agreement.rs`, shown failing on a deliberate 45-minute drift and reverted.

- [x] **G1** — Every artifact this branch authors is English, per `CLAUDE.md`,
      whose exemptions this file's closing quotation claims openly. *Evidence:*
      `cargo test -p quantick-app --test language_guard` green and
      `arch-review` dimension 8 clean. → PR body.
      → **MET.** `cargo test -p quantick-app --test language_guard` green in the full run.

- [x] **G2** — The four checks green, each run on its own, after rebasing on
      latest `main`; plus `python bridge/mt5/tests/test_paging.py` and
      `ruff check --select F` over `bridge/mt5/`, which cargo cannot see and
      which this branch touches. *Evidence:* command output. → PR body.
      → **MET.** Each of the four run separately, green; `ruff check --select F` over `bridge/mt5/` and `tools/mt5/` clean; all three bridge suites green and now discovered automatically by `cargo test -p quantick-feed-mt5 --test bridge_paging`.

- [x] **G3** — Performance impact declared: every touched path classified by
      rate (per-trade / per-depth / per-frame / rare) in the plan, not in the
      review. *Evidence:* the classification table. → PR body.
      → **MET.** Classified in the PR body: the walk and the slicing are **rare** (once per connect), the prepend is **per-frame** for thirty frames, and nothing per-trade or per-depth changed.

- [x] **G4** — Hot-path evidence, because the opening burst is 1.5 M trades and
      the progressive fill runs against the frame loop: `APP_HEALTH_SUMMARY`
      fps/frame_avg under the full-day load against a `main` control run.
      *Evidence:* both summaries. →
      `.claude/evidence/mt5-session-history/perf.md`.
      → **MET.** [`.claude/evidence/mt5-session-history/perf.md`](.claude/evidence/mt5-session-history/perf.md), whose tables are the output of `summarise_perf.py` over the two logs committed beside it — deliberately not restated here, because restating them by hand is the defect three review rounds caught. Run the script to reproduce them. It also records what an earlier 50 000-print slice cost and why the shipped size is 200 000.

- [x] **G5** — User-visible surfaces follow `ui-harness`: every new or changed
      surface reachable by an env hook added in this change; `visual-qa` with
      all surfaces PASS or defects explicitly accepted; `trader-ux-review` with
      no unresolved Blocker. *Evidence:* the three reports. →
      `.claude/evidence/mt5-session-history/`.
      → **MET.** Two hooks added and registered (`QUANTICK_MENU=history`, `QUANTICK_HISTORY_REACH_SPAN_MINUTES`); `visual-qa` in [`.claude/evidence/mt5-session-history/in-the-app.md`](.claude/evidence/mt5-session-history/in-the-app.md) with every surface PASS; `trader-ux-review` in [`.claude/evidence/mt5-session-history/trader-ux-review.md`](.claude/evidence/mt5-session-history/trader-ux-review.md), whose one Blocker was fixed on this branch and whose one Consider is deferred below.

- [x] **G6** — The `HistoryReach` addition follows `new-extension`: the port is
      the existing enum plus its `ALL` registry, the edit is registration-only,
      and defaults preserve today's behaviour. *Evidence:* blast radius (added
      vs. edited files) stated. → PR body.
      → **MET.** `HistoryReach::Span` is a variant plus its arms; the toolbar, the env hook, the control plane and `from_token` all reached it through `ALL` with no edit of their own. Blast radius in the PR body.

- [x] **G7** — The new reach and the progressive fill are drivable and readable
      by the second operator, per `arch-review`'s *The second operator*: the
      reach is settable and the fill's progress readable over the control
      plane, not by mouse alone. *Evidence:* the `quantick_invoke` /
      `quantick_get_snapshot` transcript. →
      `.claude/evidence/mt5-session-history/second-operator.md`.
      → **MET.** [`.claude/evidence/mt5-session-history/second-operator.md`](.claude/evidence/mt5-session-history/second-operator.md) — the reach and its span set by hook and read back over the control plane; the fill's progress exposed as `feed.status`'s `opening_slices_remaining`, asserted by `an_opening_slice_draws_without_answering_the_traders_press`. The file states plainly which half is a live transcript and which is a unit test, and why a three-second transient could not be sampled.

- [x] **G8** — `arch-review` run over `git diff origin/main...HEAD` with every
      Blocker and Should-fix resolved, or deferred with the trader's approval
      recorded here and in the PR body. *Evidence:* the review verdict. → PR
      body.
      → **MET.** [`.claude/evidence/mt5-session-history/arch-review.md`](.claude/evidence/mt5-session-history/arch-review.md) — two agent passes at xhigh (13 then 14 findings, all confirmed) plus a self-review of the delta after them; all fixed or deferred with reasons recorded below. Graded over the head this file is committed at, which is what `arch-review-ok` holds.

### Not applicable, and why

- **Engine / determinism test-first**: the engine is not touched — bar building
  consumes the same `Trade` stream it does today, and this branch changes only
  which trades arrive and when. The test-first discipline still applies to the
  bridge's session-anchor function (A1), and is met there.
- **`new-extension`'s carve-a-port rule for a new crate**: no new crate and no
  new capability class. The two additive pieces dock at ports that already
  exist — `HistoryReach::ALL` and the bridge protocol's message registry.
- **Golden/snapshot determinism test**: nothing in this branch is
  order-dependent output of the engine. The bridge's ordering guarantee
  (ascending, deduplicated across slices) is asserted directly in the A1 and A4
  tests instead.

## Deferrals — granted by the trader, 2026-09-01

**All five were put to the trader and all five were granted.** They are
recorded with that fact because the heading previously said "approved" for
four rounds while nobody had been asked — which `delivery-review` names as the
failure the deferral mechanism exists to prevent: a deferral the session grants
itself is not a deferral. They were presented with their real costs, including
one whose stated window had been wrong by three and a half times, and the
trader took them as PR follow-ups.

Two carry a decision of their own:

- On the `+ older` over-count, the trader's words were that bringing *more*
  history than asked for does not get in their way, so the campaign keeps
  measuring the chart rather than refusing the press during a fill.
- On the continuous market, the trader confirmed they run **B3 only** on this
  bridge — mini index and mini dollar — not the Tickmill CFDs. So the 24/5 path
  is bounded and tested but touched by nobody, which is why measuring it was
  not made a condition of shipping.


- **A `+ older` press made *during* the opening fill can over-count.** A
  `Span` campaign measures traded time across the whole chart tape, and the
  fill is extending that tape below the campaign's anchor — so a run started
  while the fill is still going can report `ReachMet` on hours the slices
  brought rather than the pages it fetched. Found by the second `code-review`
  round. Not fixed here because the honest fix is for the campaign to measure
  what it pulled rather than what the chart holds, which is a change to
  `Campaign`'s contract rather than to this branch.

  **The window is about ten seconds, not the three this said for four rounds.**
  `perf.md` measures the fill at 10.5 s and `whole-day.md` at 11.2 s; the
  three-second figure was the *bridge's* send time, not the chart's fill. A
  deferral argued on a window three and a half times too narrow is a deferral
  argued on the wrong facts, so the corrected width is stated here before the
  trader is asked to accept it. The outcome argument is unchanged — a run that
  over-counts returns *more* history than was asked for.
- **A continuous market's opening block is unmeasured.** On a 24/5 CFD the
  walk has no session edge and now reaches its 48-hour span cap against a
  4 M-tick ceiling, where before it was a 12-hour window capped at 1 M. Every
  performance figure on this branch is measured on WINV26, which closes after
  about nine hours. The bound exists and is tested; what is missing is a
  measurement on a tape this repository does not have.
- **The `QUANTICK_MENU=history` hook goes stale when the toolbar folds.** The
  caret's rect is published by `draw_history`, which does not run while the
  history group is in the overflow menu, so on a narrow window the hook aims at
  a stale coordinate or none. It affects captures, never a trader.


- **The reconnect re-sends the opening block over the wire.** After a
  reconnect the bridge re-runs its whole opening block, and the app now
  refuses those slices in one branch rather than mapping and discarding each
  (`crates/app/src/feed/metatrader.rs`, `MT5_OPENING_PAGE_AFTER_RESUME`). What
  is *not* avoided is the send itself: the bridge cannot know where the chart
  already reaches without a new message telling it. Recorded here because the
  code comment claims a deferral, and a deferral the goal file does not carry
  is not one.

- **The fill is not counted down on screen.** The bridge reports how many
  opening slices remain and the app logs it, but nothing on the chart says
  "four to go" during the ten seconds the morning is arriving. Raised by
  `trader-ux-review` as a *Consider* against Duda, the newcomer: Rafa and
  Marina read the chart growing leftward and do not need it. Deferred rather
  than improvised because the loading lane belongs to requests the *trader*
  made, and putting an unasked-for fill into it needs its own design pass —
  inventing a second progress language inside a fix is how surfaces drift
  apart. Recorded here and in the PR body.

## Closing steps

- **C1** — `delivery-review` returns PASS over the branch as shipped.
- **C2** — The PR is open, with CI green and the evidence above in its body.

---

## The request as received

Quoted verbatim, in the trader's own words, from the `/mission` invocation of
2026-08-31. It is left untranslated deliberately: `CLAUDE.md`'s language rule
exempts a marked, attributed quotation, and this section exists so that
`delivery-review` — which reads this file and never sees the conversation that
produced it — can re-derive the asks from the source rather than from the
ledger's reading of them. Every other line in this file, including each ledger
row's operative statement, is English.

> resolver de vez o problema de carregar passado de ticks do metatrader. Eu
> abri o grafico hoje do mini indice... nao quis carregar o passado. Parou em
> 9h30. Se eu quero abrir o dia e ver o que aconteceu antes em grafico de tick
> eu posso. Se existe esses dados no metatarder, pq não colocar? A gnt ja fez
> vários pr sobre isso e nenhum deles conseguiu resolver isos

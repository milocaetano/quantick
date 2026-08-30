# Mission: a bracket may be a ladder, and how far is decided before the click

Give an entry's protection more than one rung — parts that close in sequence,
each with its own stop and target — plus the two things a trader wants before
the order exists: a named ladder they keep, and a wheel that walks a
symmetric 1:1 distance out from the aim.

Branch: `feat/pending-order-brackets`
Worktree: `../quantick-worktrees/feat-pending-order-brackets`

## What changed mid-mission, and why the scope is narrower than it started

The session began against `origin/main` at 13f9670 and ran long. While it did,
two upstream pull requests landed on the same ground:

- `crates/sim` was split into `crates/sim` + `crates/trading` behind a
  `TradingVenue` port (`OrderIntent`, `VenueEvent`, `BracketTarget`).
- **Bracketing a resting order shipped upstream**: `BracketTarget::Order(id)`,
  `venue.amend_bracket`, `PaperControl::CreateLeg { owner, leg }` and the drag
  handles on a pending order's line.
- **The `trade.*` capability family shipped upstream**: `trade.order.place`,
  `trade.order.bracket`, `trade.order.cancel`, behind a `trader` profile that
  nothing hands out.

Two of the original seven criteria were therefore already done, and better
than this branch had done them. The trader's call (2026-08-29) was to rebase
and keep only what is genuinely new. The pre-rebase work is preserved at the
tag `pre-rebase/pending-order-brackets` and is not being shipped.

The process lesson, recorded rather than excused: a long session must re-fetch
`origin/main` periodically, not only at the start.

## Decisions taken with the trader

- **One PR, everything that is still new.** Not staged.
- **A ladder's rungs are real working orders.** Each part becomes an OCO pair
  on the reducing side — individually draggable, cancellable, listed among
  working orders. A plain bracket stays what it was: it arms the position's
  own pair, which is what the port reports and what every venue models, so
  the single-pair trader's chart is untouched.
- **Ticks only** for strategy offsets. Currency and percentage rows need a
  tick value this workspace does not have.
- **A strategy applies to every entry kind**, market included.
- **`<None>` changes nothing.** With no strategy the order rests bare.

## Acceptance criteria

1. **Partial exits in the domain, written test-first.** `Bracket` becomes an
   ordered list of parts, bounded at four; on the fill each part becomes an
   OCO pair whose legs are ordinary working orders; a part's take profit
   filling cancels that part's stop and leaves the others open. Fixture and
   expected fills written before the implementation, golden determinism test
   passing. — **met**: `crates/sim/tests/exit_ladder.rs`, 7 tests.
2. **Named exit strategies.** Rows of (share %, gain ticks, loss ticks),
   edited in a window, selectable in the ticket, persisted app-wide through
   the paper-state sidecar; rows must sum to 100%; a selection naming a
   missing strategy selects nothing. — **met**:
   `crates/app/src/order_strategies.rs`, 9 tests plus 3 wiring tests.
3. **Projected before the click.** With the cmd modifier held the selected
   ladder paints alongside the entry preview, every rung. One function
   resolves it for the projection and the placement, proven by a test that
   compares the two brackets. — **met**:
   `the_strategys_ladder_is_both_projected_and_placed`.
4. **The ruler.** Wheel plus modifier walks stop and target out
   symmetrically, one tick per notch, stated in points and ticks; the chart
   neither pans nor zooms on those frames; a selected strategy stands the
   ruler down. — **met**: 5 tests.
5. **Drivable without a hand.** `trade.strategy.select` and `trade.ruler.set`
   join the upstream `trade.*` family; every result in the family reports
   which ladder is armed and where the ruler stands; the named ruler and the
   wheel are proven to agree. — **met**.
6. **The spec is updated.** `docs/ux/paper-trading.md` sections 4, 9b and 10.
   — **met**.

### Injected gates

- **English throughout** — `language_guard` passes; prose, branch name and
  commit messages read by hand.
- **Four checks green** on the rebased branch: fmt, clippy `-D warnings`,
  build, test (2040 app tests; the `bridge_paging` failure is this machine's
  `python3` Store alias, and the 21 bridge tests pass under real `python`).
- **Performance declared and measured.** Per-trade path, 200k prints,
  optimized test profile: `main` plain bracket ~6.2 ms, this branch's plain
  bracket ~7.7 ms, two-part ladder ~26 ms — 31 / 38 / 131 ns per print. The
  measurement ships as an ignored test beside the fixtures.
- **`arch-review` run** with step 0's bug pass; findings resolved or deferred
  in the PR body.
- **PR opened.** Merging is the trader's call.

### Reviewed

- **`arch-review` step 0** ran the bundled `code-review` at `xhigh` and
  returned **15 findings**, three of them proven with probe tests the
  reviewer ran and deleted. Every one is addressed in `7271112`; the three
  proven domain holes (a reversal leaving the dead position's legs armed over
  the new one, averaging in leaving part of the position unstopped, and
  `SetBracket` leaving a ladder armed beside a new pair) now have regression
  tests of their own in `crates/sim/tests/exit_ladder.rs`.
- **A visual pass** was run on the editor, the one genuinely new window. It
  found three defects no test could — the editor never appeared under its own
  launch hook, opened onto an empty pane, and opened beneath the indicator
  legend — all fixed in `309f691`, with the fixed surface captured at fps 60 /
  frame_avg 16.7 ms under a live tape.

### Not done, and named as such

- **`trader-ux-review` was not run**, and the visual pass covered only the
  editor. The chart-side surfaces this adds — a laddered order's rungs, the
  projected ladder under a held modifier, the ruler's chips — are
  unphotographed, and no trader's eye has been over the flow as a whole. That
  is the remaining gap in this mission's gate list and the first thing to
  close before merge.

# Mission — volume profile parity between venue candles and live tape

## Objective

A fixed-range volume profile drawn over **backfilled venue candles**
(`approximated from OHLC`) reports roughly **half** the volume per minute that a
neighbouring profile over **live tape** reports on the same 1-minute BTCUSDT
chart — `vol 985.63` over 85 bars (~11.6 BTC/min) against `vol 1.72K` over a
comparable span (~21.5 BTC/min). Find out why, and close the gap wherever
quantick is the one losing volume.

The two totals must agree because they describe the same quantity: base-asset
volume traded in a minute. The only honest reason for them to differ is that
the market actually traded differently in the two spans.

## Scope rule from the user

- If **quantick** is mishandling the data — the kline→`Bar` mapping, the
  OHLC approximation, the footprint ladder, the profile merge, or the label —
  **fix it**.
- If the **venue candle data itself** is what disagrees (i.e. quantick reports
  faithfully what the venue said), **report it and do not "correct" it**.
  Data honesty: never silently patch a venue number into agreement.

## Acceptance criteria

### Mission-specific

1. The cause of the ~2x gap is named with `file:line` evidence, not a theory:
   either a defect in a named code path or a proven property of the data.
2. A test pins the invariant that was violated. Engine/determinism territory,
   so **test-first**: fixture + expected output written before the fix.
3. Volume conservation is proven end to end for the candle path — a candle's
   `buy_volume + sell_volume` survives `BarFootprint::approximated` and
   `VolumeProfile::merge` unchanged, and equals the venue's reported total
   traded volume for that interval in the same unit as `Trade.quantity`.
4. Volume conservation is proven for the tape path — a bar's ladder totals
   equal that bar's `buy_volume + sell_volume`, including when the level cap
   forces a regroup.
5. Re-measured in the running app: the two profiles over comparable spans no
   longer differ by a factor the market cannot explain (or, if the market does
   explain it, that is stated with the venue's own numbers as evidence).
6. Anything that stays inexact is **labeled**, never smoothed over.

### Standard gates (code change)

- Four checks green after rebasing on latest `main`:
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo build --workspace`, `cargo test --workspace`.
- Performance impact declared per touched path by rate
  (per-trade / per-depth / per-frame / rare).
- `arch-review` run over `git diff main...HEAD`, every Blocker and Should-fix
  resolved or explicitly deferred in the PR body.
- PR opened. Merging is not part of the mission.

### Standard gates (user-visible surface)

Only if the fix changes what is drawn or labeled:

- `ui-harness` hook exists for every changed surface.
- `visual-qa` pass, all surfaces PASS or defects explicitly accepted.
- `trader-ux-review` with no unresolved Blocker.

## Not in scope

- Changing what the venue reports, or reconciling quantick to a third-party
  chart's numbers.
- Redesigning the profile tool.

## Branch

`fix/profile-volume-parity`, worktree
`../quantick-worktrees/fix-profile-volume-parity`.

# Mission

Size the footprint's rows — and the liquidity map's capture bucket — from the
tape's own price grid when no feed states an instrument price step, so a chart
without depth stops drawing ladders on a grouping finer than the instrument can
trade at.

## Background

`crates/app/src/orderflow_engine.rs`'s `capture_base` already states the rule:

> The instrument's own tick size, when the feed states one, is the finest bucket
> that can ever hold liquidity — finer rows are permanently empty and paint the
> map with stripes (B3's mini index moves in 5-point steps, so a 2-point bucket
> wastes over half the rows).

The rule is right; the input never arrives. `price_step` is produced only inside
the **depth** path (`feed-mt5/src/depth.rs`), and `auto_base` is only consulted
on a `DepthEvent::Snapshot`. A market replay reports no depth at all, and a feed
without L2 reports none either, so the grouping stays at
`footprint_series::default_group()` — `0.01` — which is finer than every real
tick size. On WIN, whose tick is 5, that is a ladder of which 4 in every 5 rows
can never hold a print.

That is what the trader reported as "the VAL is always at the low of the
candle": `VolumeProfile::value_area` was being handed a ladder that is 99.8%
empty buckets and asked to recover a price grid the app had thrown away. Five
review rounds on the value area confirmed that recovering it there is not
solvable — every distance rule tried either silences a legitimately sparser side
or admits a remote cluster. The branch `fix/value-area-pins-val-to-low` holds
that work and its findings; it is not going to a PR.

The tape carries the grid exactly. Measured on the trader's own recordings:
every one of 200 000 WINV26 prints is a multiple of 5, and every WDOU26 print is
a multiple of 0.5.

## Acceptance criteria

1. A deterministic price-grid detector lives in `engine` — trades in, step out —
   with no wall clock, no randomness and no iteration-order dependence, written
   test-first from fixtures.
2. It reports nothing until it has grounds to: one print, or prints all at one
   price, name no grid. What it reports divides every price difference it has
   seen.
3. It is honest about a tape that contradicts it: a print off the grid narrows
   the answer rather than being snapped onto it, and the narrowing is a normal
   result, not an error.
4. Where a feed *does* state a price step, that still wins — this adds a
   fallback, it never overrides the venue.
5. The detected step reaches the footprint ladder and the liquidity map's
   capture bucket through the **existing** `capture_base` / `set_footprint_group`
   paths, never a parallel one, so the two surfaces keep agreeing on rows.
6. Regrouping is bounded: the answer only ever narrows, so it settles, and the
   number of refolds one session can pay for is stated and tested.
7. On the trader's recorded WINV26 session the chart's status line reads rows 5,
   not rows 0.01 — and `value_area` on that ladder is `main`'s own answer, with
   no change to `VolumeProfile`.

## Injected gates

- Every artifact in English.
- Four checks green after rebasing on latest `main`.
- Engine/determinism territory: fixture + expected output before the code;
  golden tests guard determinism.
- Performance impact declared: the detector runs **per trade**, which is the
  hot path — it must allocate nothing and do integer work only. A refold is
  rare and is charged for separately.
- Touches a user-visible surface (the ladder's row width, the map's grid):
  `ui-harness` hook for the state, `visual-qa` pass.
- `arch-review` run, every Blocker/Should-fix resolved or deferred in the PR
  body.
- PR opened. Merging is not part of the mission.

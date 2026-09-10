# Discreet SL/TP tags — five-agent panel and trader grading

Five panelists (Opus, read-only) looked at the same *before* captures and the
code, each from one seat. Two trader seats then graded the *after* captures
against `trader-ux-review`'s severity scale.

## The seats

1. Chart-UI visual designer.
2. Order-flow scalper (WIN / BTC, right edge of the chart).
3. Risk-first prop-desk trader.
4. Interaction engineer who owns the paint/press contract.
5. Trading-platform benchmarker (NT8, TradingView, MT5, ATAS, Quantower).

## Where they agreed (what shipped)

| Question | Decision | Seats |
| --- | --- | --- |
| Rest text | the leg word and its signed points; no price (the gutter chip carries it), no unit | 1, 2, 3 (5 proposed `SL 1`, and accepted `SL -77` as the one-number form) |
| ✕ at rest | none — it opens with the row, like the order tag's | all five; 2, 3 and 5 called an always-on ✕ beside the axis a misclick hazard |
| Open state | today's full statement plus ✕ when the pointer is on the leg's row; R:R while dragging | 1, 2, 4, 5 |
| Size | same 20 px / mono 11 as the order pill — width was the complaint, not height | 1, 2 |
| Pending vs live | a pending leg's pill is a **ghost** (dark fill, leg-coloured outline and ink); the dashed line alone is not enough | 1, 2, 3, 5 |
| Order id | kept on a pending leg at rest (`#1 SL -195`): proximity lies — one order's target sat beside the other's line in the before capture | 3 |
| Paint/press | one per-frame `OpenTag` value keyed by order *or* leg; `control_at` offers `ClearLeg` only while that value says the ✕ is painted | 1, 3, 4 (all three named the ungated press a hard blocker) |
| Ladder rungs, aim preview | out of this change | 1, 4 |

## Before / after, measured

Deterministic tape: `make_fixture.py` (a synthetic WINV26 day plus minute
context), played by `capture_legs.ps1` through the offline feed in
`offline.toml`; same tape, same hooks, both builds.

| Scene | Before | After |
| --- | --- | --- |
| Two bracketed resting orders, four leg tags (left pane) | 247–249 px each, `#1 SL 129727 -195 pts · on fill ×` | 82–84 px each, `#1 SL -195` ghost |
| Open long, live stop | 157 px, `SL 129750 -270 pts ×` | 59 px, `SL -270` solid |
| Hover (hook forces every tag open) | — | the full statement, `· on fill` and ✕ return |

Widths were measured by pixel on the tag's top edge, where the dashed line
does not run.

## Trader grading of the after captures

- **Scalper — PASS, no Blocker.** "The tags stopped covering the chart … I
  can tell pending from live immediately (ghost pill with `#` versus solid)."
- **Risk trader — PASS, no Blocker.** "A pending stop can't be mistaken for
  live protection at rest … The invisible-delete blocker I raised is fixed."

Should-fix items the graders raised, none caused by this change. They are
deferred in the PR body:

- Tags at prices a few pixels apart overlap. The before captures show the
  same thing, and the open form is worse.
- The resting order pill hides its id, so a leg's `#1` has nothing to match
  at rest. The shipped order-pill rule (id waits for the pointer) is pinned
  by its own test; changing it is a separate decision.
- The number is price distance × quantity, still labelled `pts` in the open
  form (`signed_points`).
- Nothing marks a stop above or below the visible range, and the HUD card
  shows no SL.
- The engineer found a laddered *position*'s rung tag painted over its
  working order's open ✕.

Consider items the graders raised against this change:

- The ✕ can appear under a resting cursor when autoscale moves a leg onto the
  pointer's row. This is less exposure than the always-on ✕ it replaces.
- A newcomer learns that "ghost means not armed" only on hover.

Shrink the in-plot tag on stop-loss / take-profit lines into a discreet pill in the resting order tag's grammar, so the labels stop covering the chart.

**Tier:** `medium`, raised from `small`. The leg painter moved out of `paper_trading.rs`, which sat exactly at its size ceiling, into `paper_trading/leg_tag.rs`. That move pushed the diff past `small`'s 300-line exemption.

## What changed

| | Before | After |
| --- | --- | --- |
| Live leg on a position, at rest | `SL 129750 -270 pts ×` (157 px) | `SL -270`, solid pill (59 px) |
| Leg on an unfilled order, at rest | `#1 SL 129727 -195 pts · on fill ×` (248 px) | `#1 SL -195`, **ghost** pill: dark fill with outline and ink in the leg's colour (83 px) |
| Pointer on the leg's row | same as at rest | the full statement plus ✕, as today |
| Dragging a leg | full + R:R, no ✕ | unchanged |

- **Price:** it stays on the gutter chip. At rest the tag shows the leg word, its signed points, and on a pending leg its order id.
- **The ✕ follows the order tag's contract.** `OpenTag` is now keyed by order or leg (`TagKey`). `fill_open_legs` computes it in the same input pass. `control_at` offers `ClearLeg` only while that one value says the ✕ is painted, so it cannot be pressed while invisible.
- **The capture hook:** `QUANTICK_PAPER_ORDER_HOVER` now also opens the leg tags, and the registry was regenerated from `hook-prose.md`.

## How the design was chosen

This follows the trader's request: five panelists read the same before captures, then the two trader seats graded the after captures. The panel was:

- a chart-UI designer,
- an order-flow scalper,
- a risk-first prop trader,
- a paint/press interaction engineer,
- a platform-conventions benchmarker.

The synthesis and both verdicts are in `.claude/evidence/discreet-bracket-tags/panel.md`.

- Scalper: **PASS, no Blocker.**
- Risk trader: **PASS, no Blocker.** The pending vs live read is honest, and the ✕ can no longer be pressed while invisible.

## Evidence

- **Tests** (`crates/app/src/paper_trading/tests/mod.rs`):
  - `a_bracket_leg_tag_is_a_pill_until_the_pointer_reaches_it`: rest text, no ✕ at rest; the open text with `on fill` and ✕; only the row under the pointer opens.
  - `a_position_leg_rests_as_the_leg_and_its_points`
  - `a_leg_offers_its_clear_exactly_while_it_paints_one`: sweeps the ✕ column and asserts painted == pressable, including an edge-clamped TP.
  - `a_working_orders_legs_are_draggable_and_clearable`: now asserts there is no clear at rest.
- **Captures:** a deterministic same-tape pair per scene, so both builds saw identical input.
  - Tape: `make_fixture.py` (a synthetic WINV26 day plus minute context), played by `capture_legs.ps1` through `offline.toml`.
  - Tag widths, measured by pixel: 248 → 83 px for order legs, 157 → 59 px for a position's stop.
  - The PNGs are not committed (the repository keeps none); they were handed to the trader in the session.
- **Visual check** on those captures:
  - The tags stay inside the pane. The live before-capture showed them spilling past the narrow pane's left edge.
  - The ghost ink is legible on both leg colours.
  - The open form is unchanged.

## Performance

Per frame and per visible leg:
- **Paint:** at rest the tag formats a shorter string.
- **Input:** `fill_open_legs` adds one row-hit test per existing leg, pushed into the already reused `open_tags` buffer.

No new allocation class, and no per-trade or per-depth path.

## Deferred

None of these is caused by this change. Each is worth its own issue.

- Ladder rungs and the cmd aim preview keep their current tags. This narrows the request, and is reversible: the rung press path carries the defect below.
- Tags at prices a few pixels apart overlap, including an order's tag with a leg's. `control_at` checks orders' ✕ before legs' even though legs paint on top. The overlap is visible in the before captures too; the open form makes it worse.
- The resting order pill hides its id (a shipped, test-pinned rule), so a pending leg's `#1` has nothing to match at rest.
- The leg number is price distance × quantity but still reads `pts` (`signed_points`).
- Nothing marks a stop that sits above or below the visible range, and the HUD card shows no SL.
- A laddered position's rung tag paints over its working order's open ✕.

## Verification

- **Local:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace` and `cargo test -p quantick-guards` are all green on this head.
- **CI:** final-head CI is the gate.

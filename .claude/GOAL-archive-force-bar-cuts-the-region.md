# Mission

Fix the rectangle strategy so a force bar trades a drawn region only when its
**body** — open and close, never the wicks — actually cuts that region.

## The bug, in the trader's words

> A barra de força precisa cruzar, e não só cruzar as sombras. Cruzar é
> abertura e fechamento dela.

(Quoted verbatim, per the English rule's attributed-quotation exemption. In
English: the force bar has to cross, and the wicks crossing is not crossing —
crossing is its open and its close.)

Today `ArmedStrategy::on_closed_bar` decides the entry from the trigger bar's
**close alone**. For a sell instance any force bar closing below the region's
low rests a retest limit at that low — including a bar that opened below the
low too and therefore never touched the region. That is the reported symptom:
a force bar that never cut the region still leaves a limit order sitting in
it.

## The rule this mission installs

For a **sell** instance on a region `[low, high]`, judged on the trigger bar's
open `o` and close `c`:

| Geometry | Action |
| --- | --- |
| `c` inside `[low, high]` (whatever `o` did) | market sell |
| `c < low` and `o >= low` — the body cut the lower edge | retest limit at `low` (under `BreakPolicy::RetestLimit`) |
| `c < low` and `o < low` — the body never crossed the edge | nothing, gate named on the badge |
| `c > high` — closed away, above the region | nothing |

A **buy** instance is the mirror image around `high`.

## Acceptance criteria

1. The region/cut test judges the trigger bar's body (open **and** close). A
   close inside fires at market whatever the open did; a close beyond the near
   edge rests the retest limit only when the open sat on the region's side of
   that edge; every other geometry holds fire.
2. A force bar whose body lies wholly beyond the near edge places **no**
   order, and names its own gate on the badge (no bare "armed" over a bar
   that did nothing).
3. A close beyond the *opposite* edge still never fires — kept under test.
4. The rule has exactly one home in the kernel, consumed identically by the
   chart and the backtest. No second copy in `app`.
5. Test-first, as a pure domain crate demands: fixture bars and expected
   commands written before the code, covering all four geometries on both
   sides.
6. Performance impact declared: the strategy kernel runs **per closed bar**
   (rare), not per trade, per depth update or per frame. The change adds two
   `Decimal` comparisons to a path that already does several.
7. Every artifact in English.
8. Four checks green after rebasing on latest `main`; `arch-review` run with
   every Blocker/Should-fix resolved or deferred in the PR body; PR opened.

## Explicitly not in scope

- The retest bracket is projected off the trigger bar's close, not off the
  edge the order actually rests at. That is a separate design question and is
  reported in the PR body, not changed here.
- No new UI surface and no layout change: the only user-visible delta is one
  more status-line note in a badge that already renders arbitrary notes, so
  `visual-qa` / `ui-harness` add nothing to grade.

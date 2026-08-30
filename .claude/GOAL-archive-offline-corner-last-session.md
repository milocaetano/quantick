# Mission

A MetaTrader chart opens showing the most recent session the terminal holds
even when nothing is trading, and any feed problem is reported by a discreet
offline chip in the chart's bottom-right corner that opens the recovery popup
when clicked — never by a card across the chart, and never by discarding data
already on screen.

## Why

The trader opened WINV26 before B3's open on 2026-08-29 and got an empty chart
under a full-width error card. Their words: *"ficar sem ver dado algum é pior
do que ficar sem conectar"* — seeing no data at all is worse than not being
connected — and *"se conseguiu trazer algo, deixa lá os dados"*. Reviewing the
last session is how they prepare; today the chart is only useful while a venue
is streaming.

Two independent causes, both in scope:

1. **The opening backfill is a window anchored on `now`.** `quantick_bridge.py`
   asks `copy_ticks_range(now − 720 min, now)`; the Expert Advisor asks 30 min.
   Fourteen hours after B3 closes, both return nothing and the chart opens
   empty even though the terminal holds the whole session on disk.
2. **A feed problem is told at chart scale.** `notice_card` draws a 420 px card
   with two buttons whenever a pane is empty (PR #257). That was right when the
   card was the only way out; it is wrong as the resting state of a chart the
   trader opens outside market hours every day.

Not to be confused with PR #258 (`feat/replay-day-before`), which joins the
previous day in front of a *recorded* session. This mission is the live path:
the chart the trader opens on `metatrader-b3` / `WINV26`.

## Acceptance criteria

Mission-specific:

1. **The chart opens populated outside market hours.** With B3 closed and the
   terminal running, opening the `metatrader-b3` / `WINV26` tab paints bars
   from the most recent session held, not an empty pane. The opening backfill
   reaches back to the last session it can find instead of a fixed window
   anchored on `now`, bounded and logged; an empty result stays empty rather
   than inventing anything.
2. **No feed condition draws a chart-scale card by itself any more.** The
   resting report is a compact chip in the chart's bottom-right corner: a
   coloured dot plus one word (`offline`), sized so it never covers bars. Every
   feed problem lands there — including the provider-named reasons that used to
   set `may_cover_bars`.
3. **The chip is the door to the recovery popup.** Clicking it opens the popup
   carrying the headline, the reason and the same two acts as PR #257
   (`Reconnect` primary-by-decision, `Reload` beside it, with what Reload costs
   stated). The popup opens on a click and on nothing else.
4. **Data already on screen survives every offline path.** A silent tape, a
   reconnect and a repeated failure never clear bars, drawings, indicators or
   an open paper position. Only the trader's own `Reload` rebuilds a timeline,
   and it still says so before it runs.
5. **The offline state is readable and drivable without a mouse.** The chip and
   the popup's two controls appear in `quantick_get_scene`, each linked to the
   capability that operates it, and the chip carries the rectangle it was drawn
   at. Opening the popup stays a gesture rather than a call: an agent acts
   through `feed.reconnect` / `feed.reload` directly, and `notify.popup`
   already exists for making the trader read something — a second door would be
   a second way to do a solved thing.
6. **The bridge's reach is proven headlessly.** New tests in `bridge/mt5/tests/`
   cover the widened opening backfill: a market closed for hours, a weekend, a
   symbol with no history at all, and the bound that stops the walk.

Injected gates:

7. **English everywhere** — every artifact in this branch (CLAUDE.md owns the
   rule and its exemptions).
8. **Four checks green** on latest `main`: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace`. Plus the two CI checks
   the workspace cannot see, because this branch touches Python:
   `ruff check --select F` over `tools/mt5/` and `bridge/mt5/`, and
   `python tools/mt5/test_export_session.py`; `python bridge/mt5/tests/test_paging.py`
   too, since the backfill walk lives beside it.
9. **Performance declared by rate.** Every touched path classified
   (per-trade / per-depth / per-frame / rare) in the plan, not the review. The
   chip draws per frame, so: `APP_HEALTH_SUMMARY` fps and frame_avg under a
   dense tape against a `main` control run, numbers in the PR body.
10. **UI gates.** `ui-harness` hook for every new surface (the chip, the popup,
    the empty-outside-hours open), registered in the same change;
    `visual-qa` with all surfaces PASS or defects explicitly accepted;
    `trader-ux-review` with no unresolved Blocker.
11. **`arch-review`** run over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred with its reason in the PR body.
12. **PR opened** with the evidence in its body. Merging is the trader's call.

## Out of scope

- Market Replay's own day-before join (PR #258).
- Persisting bars across launches.
- A venue session calendar. Session boundaries stay observed from the tape's
  own gaps, as `history_reach::SESSION_GAP_MS` already does.

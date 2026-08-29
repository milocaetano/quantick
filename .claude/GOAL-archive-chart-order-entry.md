# Mission

Make order entry a first-class gesture on **every visible chart pane** and
route it through a new venue-neutral **`quantick-trading`** port: holding the
buy/sell modifier aims and places on whichever pane the pointer is over, the
aim offers an explicit limit-or-stop choice instead of only inferring one,
and a *working* order carries draggable `SL`/`TP` handles whose legs arm
automatically the moment it fills.

Branch: `feat/chart-order-entry`; worktree
`../quantick-worktrees/feat-chart-order-entry`.

## Where this starts from (verified in code, not assumed)

- Every pane of a tab already **paints** the paper layer
  (`Tab::draw_canvas` calls `paper.draw_layer` per pane), but
  `PaneChrome::paper_owns_input = side == focused` gates the aim and the
  press to the focused pane only (`pane.rs:4395`, `pane.rs:5935`).
- `compute_cmd_preview` (`paper_trading.rs:2365`) **infers** the entry kind
  from the mark — above it a buy stops in, below it a buy waits at a limit.
  There is no way to say otherwise.
- Bracket handles exist **only on the open position's entry line**
  (`paper_trading.rs:1847-1892`); a pending order has none.
- `quantick_sim::Command::ModifyOrder { id, price }` moves a resting order's
  price only. `SetBracket` targets the open position. **No command amends a
  working order's bracket** — though `Order::bracket` already exists and is
  already applied on fill.
- `PaperTrading` holds a concrete `quantick_sim::Simulator` and issues
  `sim::Command` from ~15 call sites; there is no venue abstraction.
- The control registry (`app/src/control/registry.rs`) has **no trading
  action family at all** — nothing a second operator could call.

Scope decisions taken with the trader (2026-08-29): "any chart" means every
visible pane of the current tab (the account stays one per tab, which is what
a tab already means — not shared across tabs); the port lives in a new
`quantick-trading` domain crate, not an app-side module.

## Acceptance criteria

1. **A venue port, and paper is one implementation of it.** New workspace
   crate `trading` (package `quantick-trading`) owning venue-neutral order
   intents (`OrderIntent` with side, kind, price, quantity, bracket,
   cancel-at, flat-only), bracket amendments, the working-order/position
   views a UI needs, and the `TradingVenue` trait. `PaperVenue` wraps
   `quantick_sim::Simulator`. Dependency direction `app` → `trading` →
   `sim` / `engine`, no reverse edge; `trading` is a pure domain crate (no
   UI, no network, no async, no wall clock). **Test: a fake second venue
   implements the trait and the chart's order-entry path drives it**, so
   nothing paper-specific leaked into the port.

2. **Order entry works on every visible pane.** The buy/sell modifier paints
   the aim and places the order on whichever pane the pointer is over — no
   focus click first. The existing claimant precedence holds per pane,
   unchanged: a drawing's handle, the canvas chrome, this module's own
   furniture (✕, bracket handle, any grabbable line) and the layer switch
   all still outrank the aim. Test: the aim computes on an unfocused pane,
   and a press there places the order.

3. **The trader chooses limit or stop.** *Revised during the work, and the
   revision is the finding.* The original wording asked for an override that
   yields "the opposite `EntryKind` at the same price". That order cannot
   exist: `validate_limit_side` and `validate_stop_side`
   (`crates/sim/src/simulator.rs`) leave exactly **one** resting kind valid at
   any price — a buy limit at or above the market fills immediately, a buy
   stop at or below it triggers immediately — so an override producing the
   other kind would produce a guaranteed rejection, and an aim advertising it
   would break this module's own rule that the label never promises an order
   the press will not make.

   Shipped instead, and more useful: a stated kind (`auto` / `limit` / `stop`,
   remembered in the sidecar, chosen in the Trading tab) that is honoured
   where it is valid and **stands the aim down** where it is not. The case it
   serves is real — the mark moves, so under `auto` the same click at the same
   level places a stop now and a limit two ticks later. Test: at one price,
   `auto` yields the valid kind while the *other* stated kind yields nothing,
   and the press places nothing where the aim shows nothing.

4. **A working order carries its own brackets, set by hand on the chart.**
   Hovering a pending order's line or tag reveals labelled `SL`/`TP`
   handles; dragging from the line (or from a handle) sets that leg, in the
   same drag grammar the position line already uses; both legs paint as
   their own lines tied to the order, and they arm on the fill. Backed by a
   new `quantick_sim::Command::SetOrderBracket { id, stop_loss,
   take_profit }` validated against the order's *own* price the way
   `SetBracket` is validated against the position's, with rejections
   surfaced verbatim as toasts. Test: bracket set on a resting limit →
   tape fills the limit → the position opens already protected.

5. **Drivable without a mouse.** A `trade.*` action family in the control
   registry — place (with an explicit kind, never an inferred one), amend a
   working order's bracket, cancel — each with a readable result, listed by
   `quantick_describe`/`quantick_search_capabilities`. Test: an action
   places an order and a read-back reports it.

6. **English everywhere** — every artifact, per `CLAUDE.md`.

7. **Four checks green** after rebasing on latest `main`:
   `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -D
   warnings`, `cargo build --workspace`, `cargo test --workspace`.

8. **Performance declared and measured.** The aim now computes per visible
   pane instead of once — a per-frame path. Classify every touched path by
   rate in the plan, and prove it with `APP_HEALTH_SUMMARY` fps/frame_avg
   under a dense tape against a `main` control run, numbers in the PR body.

9. **UX proven, not asserted** — the trader asked for this explicitly.
   `ui-harness` hooks for every new/changed surface, added in this change;
   `visual-qa` with all surfaces PASS or defects explicitly accepted;
   `trader-ux-review` with no unresolved Blocker.

10. **`arch-review` run** over `git diff main...HEAD` with every
    Blocker/Should-fix resolved, or deferred and named in the PR body;
    **PR opened**. Merging is not part of the mission.

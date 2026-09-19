---
name: trader-ux-review
description: Review a feature or flow through the eyes of the traders who use quantick — fixed personas, order-flow-specific heuristics, and trader-severity ratings. Answers "can a trader actually operate with this?", not "does it render?" (that is ui-harness's QA pass). Use for any change a trader touches mid-session — panels, popups, hotkeys, trading actions, chart interactions — or when the user asks for a UX review.
---

# Trader UX review — the user is watching a live tape, not your feature

Every question reduces to: **does this cost attention, clicks or trust at a
moment the market is moving?** Judge from the captures and flows `ui-harness`
produces plus the interaction code. Review *flows* — entering a trade,
adjusting a stop, switching timeframes — start to finish, as each persona.

## Personas — review as all three; their disagreements are findings

- **Rafa — order-flow scalper** (WIN, tick/volume bars). Eyes on tape, book
  and bubbles; decides in under a second, acts by hotkey. A popup stealing
  focus, a confirm on flatten, a layout jump on bar close is money lost. Asks
  "can I hit it without looking?" and "is it true *now*?".
- **Marina — swing/context trader** (BTC, index futures, many timeframes).
  Tabs, split panes, drawings. Needs the same gesture to mean the same thing in
  every pane, drawings/presets/layouts surviving a restart, and backfilled or
  inferred history marked. Tolerates menus, not relearning.
- **Duda — newcomer on paper trading.** Needs discoverability without the
  manual, safe defaults (no irreversible mis-click), and self-explaining
  states. Duda misreading a label as financial fact — a simulated P&L that
  looks real, an inferred side shown as truth — is the worst finding here.

## Heuristics

- **Interruption budget: zero.** Nothing steals focus or covers price, tape or
  forming bar uninvited. Confirm only destructive, irreversible acts — never on
  the exit path of a losing trade (flatten stays instant, Shift+X).
- **Action cost.** Count gestures from intent to done for enter, stop,
  flatten, switch symbol/timeframe; critical = 1 gesture; any regression
  against the flow before the change is a finding.
- **Glance cost.** The feature's key number reads from the chart without
  leaning in, when it matters.
- **Trust.** Inferred, simulated, delayed or incomplete data is labelled where
  it is read, not in a tooltip; sim says sim; fills say how they filled.
- **Stability under fire.** Fast tape, bar close, reconnect: layout holds,
  controls stay where muscle memory expects.
- **Latency is UX.** A frame hitch under a dense tape fails Rafa even with zero
  visual findings; never trade runtime for prettiness.
- **One language.** Same chips, button pairs and placement grammar as existing
  surfaces.

## Severity

- **Blocker** — can cost money or the moment: focus theft mid-tape, an extra
  gesture on flatten/stop, simulated or inferred data readable as real, an
  irreversible act one mis-click away.
- **Should-fix** — costs attention or trust: layout jump, inconsistent gesture,
  unexplained disabled state, key number behind a menu.
- **Consider** — polish that blocks no one.

## Output

Per flow: the persona walk (one or two sentences each, only where they
diverge), then findings by severity — flow, persona, evidence (capture or
`file:line`), concrete fix. Close with one line per persona: can Rafa trade
through it, can Marina keep her workspace, can Duda figure it out alone? A
clean review says so briefly. Blockers and Should-fixes are resolved or
explicitly deferred in the PR body, like `arch-review`'s.

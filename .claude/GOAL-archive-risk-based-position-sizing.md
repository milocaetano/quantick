# Mission

Size the entry from a risk budget: give quantick a trader-declared per-instrument
money model, an initial paper-trading capital, and a fixed-risk mode that derives
the order quantity **before the entry is placed**, from the stop the trader put on
that entry.

## Scope, in the trader's own framing

This is **entry risk**: deciding *how many contracts to send* at the moment of
entry. It is not stop/target management after a position is open.

**In scope** - the entry carries its stop *already*, and the size follows from it.
Two ways the trader predefines that stop, both named by the trader, and a third
that already exists on the same funnel:
- **A saved strategy** (`order_strategies.rs`, the ticket's Strategy row): the
  named exit ladder predefines stop and target in ticks before the click.
- **The mouse wheel** (`step_ruler`): rolling walks the projected stop out from
  the aim, one instrument step per notch.
- The ticket's plain stop offset, the third source `aim_bracket` already resolves.

All three converge on `aim_bracket` (`crates/app/src/paper_trading.rs:3634`), so
sizing docks there once and every path is served by one code path, never three.

- The refusal when the budget cannot be honoured at that stop.

**Out of scope, deliberately, and named in the PR body**
- Re-sizing anything after entry. An open position's contracts are already bought;
  dragging its stop changes risk, not size.
- Re-sizing a resting order when its stop is dragged.
- Netting the entry's budget against exposure a previous entry already carries.
  The budget is the risk of *this entry*. **Explicitly deferred by the trader:**
  locking further entries once already in the operation is a later session.

### The shape that deferral must leave room for

The trader's stated direction is a **pair** of ceilings: a *max positioned risk*
for the account alongside a *risk per trade*, so a trader can enter small and
scale in without the total ever passing the maximum. This mission builds the
second of that pair, and the hard part of both: the lot arithmetic.

One naming consequence, taken now so the future half is additive rather than a
rename: the setting is **risk per trade**, in the type, the config key, the
control-plane action and the UI string. A bare "risk" would have to be renamed
the day the position ceiling lands.

## Why now

The workspace refuses currency on purpose, in three tracked places
(`crates/sim/src/lib.rs`, `crates/app/src/order_strategies.rs`,
`docs/ux/paper-trading.md`), and `docs/ux/paper-trading.md:38-40` names the
instrument table as "future work". This mission is that future work.

## Trader decisions on record

1. **Money is trader-declared per symbol.** Two fields (point value, size step),
   persisted per symbol in the paper-state sidecar beside `ruler_steps`. No bridge
   or protocol change in this branch; venue auto-fill is a named follow-up.
2. **The budget is binding, and the lock is on by default.** With fixed risk on,
   there is no entry whose risk exceeds the fixed value. Where the minimum tradable
   size already exceeds the budget at that stop, the entry is refused and a *small,
   discreet* line asks the trader to raise the risk - not an amber block, not a
   toast, not a modal. A named toggle deactivates the lock.
3. **The budget is per entry**, measured from that entry's own stop.

## Acceptance criteria

### The kernel
- [ ] `crates/trading/src/money.rs`: instrument spec (point value, size step, min
      size, optional max, provenance) plus `size_for_risk` and `risk_at`. Pure,
      deterministic, `rust_decimal`. Placement justified: `signed_points` lives in
      `trading`, and a broker adapter must not link the paper simulator to learn a
      lot step.
- [ ] Every refusal named, each carrying `advice()`, following `LadderError`.
- [ ] Quantity rounds **down only**. `risk <= budget` proved by re-multiplication
      (never by trusting 28-digit division), asserted as a sweep invariant.
- [ ] `risk_at(size_for_risk(x)) == size_for_risk(x).risk` - one arithmetic, so the
      readout and the sizer can never disagree.
- [ ] **A saved strategy's ladder sizes correctly.** A `Bracket` may carry up to
      four `ExitPart`s with their own quantities and stops, so the entry's risk is
      the share-weighted stop distance. Rung rounding is not linear
      (`OrderStrategy::resolve` gives the last rung the remainder), so: weight ->
      size -> **re-resolve the ladder at the sized quantity** -> report the risk of
      the re-resolved ladder. Never report the requested budget as if it were the
      risk.
- [ ] Test-first: the fixture table is written and failing before the implementation,
      and includes the trader's clamp scenario by name, plus a ladder whose rung
      rounding moves the real risk away from the weighted mean.

### The money model
- [ ] Trader-declared per symbol; provenance labelled on the surface.
- [ ] **Never** inferred from `paper_trading.rs`'s `tick_size()` (decimal-scale
      heuristic; yields 1 for WIN$N whose real step is 5) nor from price magnitude
      nor from the symbol name. Guarded by a test.
- [ ] Unknown money disables fixed risk *with its reason*; it never falls back to a
      quantity of 1 and never guesses.

### Capital
- [ ] Capital is a **map keyed by currency**, not a scalar. No FX conversion
      anywhere; nothing is ever summed across currencies.
- [ ] Percent risk resolves against the declared initial capital, and the UI says it
      does not follow the session's result.
- [ ] The capital is a typed constant, not a running balance.
- [ ] Default for a trader who sets nothing: every existing screen unchanged, points
      everywhere, no sidecar version bump.

### The surface
- [ ] The existing Shift+wheel ruler (`step_ruler`) derives the quantity: one gesture
      moves the entry's stop and its size together; the chart chip carries the money.
- [ ] Selecting a saved strategy in the ticket derives the quantity the same way,
      from the ladder it predefines - proved by a test that both paths size one
      fixture entry identically.
- [ ] `qty_text` stays the single source of truth - the mode writes into it and
      disables the field with a reason. No second quantity field.
- [ ] The over-budget notice is small and discreet (the ticket's support-line
      convention), never a block of amber in the middle of the ticket.
- [ ] The app half lands in a **new** `crates/app/src/risk_sizing.rs`:
      `paper_trading.rs` is at its 8,861-line ceiling (`crates/app/tests/size_guard.rs:86`).
- [ ] `step_quantity`'s hard-coded step of 1 is fixed for fractional-size instruments.

### Honesty debts settled in the same change
- [ ] The three tracked "points, never currency" promises are amended, not left
      contradicting the feature.
- [ ] The journal stamps `# money_per_point` / `# currency` as additive header lines.
      `FORMAT_VERSION` unchanged; files without them still parse; stamped per file,
      never derived at read time.
- [ ] The report and the ledger stay in points in this branch.

### Standard gates
- [ ] **English** in every tracked artifact.
- [ ] Four checks green, each run separately, after rebasing on latest `main`.
- [ ] **Performance declared**: the readout recomputes per frame while the ruler
      moves - classify it, and show fps/frame_avg against a `main` control run.
- [ ] **Second operator**: `trade.risk.set` and `trade.instrument.set_money`
      registered beside `trade.ruler.set`; `session.paper` gains `risk_state` and
      `risk_refusal`, both produced by the *same* function that renders the on-screen
      sentence, proved by a test rather than by reading the diff.
- [ ] **ui-harness hook** `QUANTICK_PAPER_RISK` added in the same change; runs pin
      `QUANTICK_PAPER_STATE` at a scratch file so no launch overwrites the real setup.
- [ ] **visual-qa** all surfaces PASS or defects explicitly accepted; the clamped
      state captured from a cold launch with no hand.
- [ ] **trader-ux-review** with no unresolved Blocker.
- [ ] **new-extension**: port named, registration-only edits, blast radius (added vs
      edited) stated in the PR body.
- [ ] **arch-review** run, every Blocker/Should-fix resolved or deferred in the body.
- [ ] PR opened. Merging is not part of this mission.

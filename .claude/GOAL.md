# Mission — a trade mark paints only where the tape on screen proves its fill

**Objective**: closed-trade marks appear on the chart only when the pane's
series actually covers the fill's instant. A trade whose entry/exit moment the
tape on screen has not reached (or has already dropped) paints nothing —
instead of being clamped onto the first or the newest bar, where the marks
pile up at the start of a replay and clutter the chart.

Branch: `fix/trade-marks-off-series`
Worktree: `../quantick-worktrees/fix-trade-marks-off-series`

## The bug

`PaneState::slot_at_time` clamps out-of-range instants — its own tests say so
(`crates/app/src/state.rs:949-950`: "past the end: the newest", "before the
start: the oldest"). `trade_paint::endpoints`
(`crates/app/src/trade_paint.rs:170`) takes that clamped slot at face value,
so a fill the pane's bars do not cover still gets an x. After a replay rewind
(bars reset, `sim.closed_trades()` kept) every earlier round trip resolves to
an edge bar and the marks accumulate there — the reported "ícones já no início
do dia".

The honest bound is the tape the pane holds: the first bar's `open_time` on
one side, the newest bar's `close_time` (last print in the forming bar) on the
other. Outside it, the chart has no proof of the fill and draws nothing —
the same reasoning the module header already gives for not painting trades
loaded from earlier sessions.

## Acceptance criteria

### Mission-specific

- [ ] **A1** — a strict time→slot lookup on the pane answers `None` for an
      instant before the oldest bar's `open_time` and `None` for one after the
      newest bar's `close_time`, and the covering slot otherwise. Unit tests
      cover both edges, both boundaries (exactly on `open_time` / on
      `close_time`) and an instant inside.
- [ ] **A2** — `trade_paint` uses it: a trade whose entry *or* exit is off the
      tape paints no mark, no connector and no cap notice. Test asserts an
      empty shape set for a before-the-tape trade and for an after-the-tape
      one.
- [ ] **A3** — no regression for a covered trade: entry triangle, exit diamond
      and connector land on the same slots as today. Test pins the marks of a
      fully covered round trip.
- [ ] **A4** — the hover tooltip cannot reach an off-tape trade (it is not in
      the mark list at all). Test hovers the pixel an off-tape mark used to
      occupy and asserts no tooltip.
- [ ] **A5** — the replay case is proven end to end: with bars whose window
      does not cover them, a set of closed trades produces zero marks; as the
      series grows past each fill's instant, that trade's marks appear (and
      only then). Test walks a growing series.
- [ ] **A6** — `TRADE_PAINT_LIMIT` still counts only what is paintable: the
      "N of M shown" notice reports trades withheld by the cap, never trades
      the tape does not cover.

### Standard gates (from `/mission`)

- [ ] **G1** — four checks green on the rebased branch: `cargo fmt --all --
      --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`.
- [ ] **G2** — performance impact declared: every touched path classified by
      rate. Expected: per-frame (chart paint) only, and the change is two
      integer comparisons on a lookup that already runs per trade per frame —
      no new allocation, strictly fewer shapes emitted.
- [ ] **G3** — `ui-harness`: the off-tape state is reachable by env hook, added
      in this change (a `QUANTICK_PAPER_DEMO` mode that seeds trades outside
      the pane's tape), so the before/after is capturable without a human.
- [ ] **G4** — `visual-qa` pass over the trade-paint surface: covered trades
      painted, off-tape trades absent, no marks stacked on an edge bar. All
      surfaces PASS or defects explicitly accepted.
- [ ] **G5** — `trader-ux-review` with no unresolved Blocker (does a trader
      lose information they needed? the ledger still holds every trade).
- [ ] **G6** — `arch-review` run over `git diff main...HEAD`, every Blocker and
      Should-fix resolved or deferred in the PR body.
- [ ] **G7** — PR opened with the evidence in its body. Merging is not part of
      this mission.

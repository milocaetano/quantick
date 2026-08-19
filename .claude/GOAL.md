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
start: the oldest"). `trade_paint::endpoints` took that clamped slot at face
value, so a fill the pane's bars do not cover still got an x.
`Simulator::reset` keeps completed trades on purpose ("they happened"), so a
replay seek rebuilds the bars *under* the round trips and every one of them
resolved to an edge bar — the reported "ícones já no início do dia".

The honest bound is the tape the pane holds: the oldest bar's open on one
side, the newest bar's `close_time` (last print) on the other.

## Acceptance criteria — all met

### Mission-specific

- [x] **A1** — `ChartPane::covering_slot_at_time` answers `None` outside the
      window and the covering slot inside it.
      `pane::tests::a_covering_slot_answers_only_inside_the_tape` covers both
      edges, both exact boundaries and an instant inside;
      `an_empty_pane_covers_no_instant_at_all`,
      `a_venue_prefix_covers_its_own_instants` and
      `the_clamping_lookup_still_clamps` pin the rest.
- [x] **A2** — `trade_paint::tests::a_trade_the_tape_does_not_cover_paints_nothing`
      asserts no polygon for four cases: older, newer, entry-off/exit-on,
      entry-on/exit-off.
- [x] **A3** — `a_covered_round_trip_lands_on_its_own_bars` pins the entry
      apex at (60, 203) and the exit diamond at (100, 145.5): unchanged
      geometry for a covered trade.
- [x] **A4** — `an_off_tape_trade_cannot_be_hovered_either` hovers the pixel
      a clamped mark used to occupy and gets no tooltip.
- [x] **A5** — `pane::tests::a_fill_ahead_of_the_tape_waits_for_it` walks a
      growing series; `app::tests::a_rebuilt_timeline_does_not_stack_old_marks_on_its_edge`
      does the whole replay-seek end to end and **fails without the wiring**
      (2 marks painted vs 0 with it — measured by reverting the one line).
- [x] **A6** — `the_cap_counts_only_what_the_tape_covers` and
      `the_notice_carries_the_cap_and_the_tape_together`: the cap's count and
      the off-the-bars count are separate and both said out loud.

### Standard gates

- [x] **G1** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo build --workspace` and `cargo test
      --workspace` all exit 0 (1474 app tests, 62 binaries).
- [x] **G2** — per-frame paint path. The window is derived **once per draw**
      (`covered_window`), not per fill, so the per-trade cost is two integer
      comparisons and an off-tape trade now short-circuits before the binary
      search it used to run. Measured under a 1 000 prints/s replay, four
      45 s runs alternating binaries: branch `frame_cpu_ms` 2.76–2.92,
      `main` 1.96–2.58, fps pinned at 59.0 in every run that shared a
      compositor rate. The same-binary spread (0.63 ms) is wider than the
      cross-binary one (0.34 ms) — no measurable difference.
- [x] **G3** — `QUANTICK_REPLAY_RESTART_AFTER=<n>` presses the transport's
      own Restart once the session has closed n round trips, registered in
      `.claude/skills/ui-harness/SKILL.md`. Two tests: it waits without a
      recording, and it fires exactly once with one.
- [x] **G4** — visual-qa over the real app (off-screen capture, `fps=59`,
      `whitesamples=0`): marks on their own bars; after the seek the round
      trip still ahead of the tape paints nothing and the corner says
      "trade paint: 1 off the bars on screen"; with its entry covered but not
      its exit it still paints nothing; once the tape passes the exit both
      round trips are on the chart and the notice is gone.
- [x] **G5** — trader-ux-review: one Should-fix found and fixed (the notice
      said "tape", which is also the name of a visible canvas surface — it
      now says "bars on screen", and so does the toast). One Consider
      deferred (selecting an off-tape row in the ledger emphasises nothing).
- [x] **G6** — arch-review with step 0 (`code-review` at high, 13 findings):
      9 acted on, 4 refuted or deferred with reasons in the PR body.
- [x] **G7** — PR opened.

# Mission: trade-history context — dated, named, paginated ledger; calendar-filtered report with the trades behind the curve

- **Branch**: `feat/trade-history-context`
- **Worktree**: `C:\src\quantick-worktrees\feat-trade-history-context` (every write happens there, never in the main checkout)
- **Cut from**: `origin/main` @ 89ffafdb
- **Done** = PR open with green CI and evidence in its body. Merging is NEVER part of the mission (another agent merges; wait for Camilo's word — memory `merge-por-outro-agente`).

## Objective (user's request, condensed)

The trade history is too raw to reason about. The sidebar ledger (`TRADES · SIM`) shows a
time with no date and, outside "All symbols", no instrument, and it dumps every saved
session at once with no way to walk back through them. The performance report
(`Simulated performance`) can only be sliced by anchor-relative pills (Today/7d/30d/90d/All
plus a typed span) — there is no way to say "show me 12 August", or "between the 5th and the
9th", and no way to see *which* trades produced the equity curve on screen. The user asked
for something complete and professional: no missing information.

## Scope (what ships)

1. **Sidebar ledger rows carry their identity**: date and symbol on every row, alongside
   today's time / duration / exit reason / points — legible at the panel's real width
   (~330 px), not truncated into ambiguity.
2. **Sidebar walks back through history**: earlier sessions load a bounded first page with a
   control to reveal older trades, stating how many remain rather than implying "that is all".
3. **Report gains a calendar**: a month grid where days that hold trades are highlighted;
   click one day to filter to it, click a second to make a range. The selection drives the
   report and is stated in words. The existing anchor-relative pills stay (a replayed
   session's trades may be years old; "7d back from the newest saved trade" is still the
   right default) — the calendar range is an additional, explicit filter that takes over
   when set and is cleared back to the pills in one click.
4. **Report shows the trades behind the curve**: the filtered trade list, in the report
   window itself, in closing order, with date, symbol, side, quantity, entry → exit, points
   and exit reason — so a curve is never an anonymous shape.

Out of scope (state, do not build): editing or annotating trades, currency conversion
(the workspace knows no per-instrument tick value), any change to how trades are journaled
to disk beyond what filtering needs.

## Standing decisions

- **Calendar coexists with the pills** rather than replacing them (rationale above). Setting
  a calendar range shows the pills as inactive; clearing it returns to the pill selection.
  Defaults preserve today's behaviour exactly: with no range set, the report renders exactly
  what it renders today.
- **Days-with-trades index is derived from what is loaded**, cached per history load, never
  recomputed per frame.
- **Dates are civil dates in the chart's display timezone**, using the existing `civil_utc` /
  `TzOffset` math the ledger and the "Today" pill already share — one date law, not two.
- **App launches for visual-qa**: require Camilo's authorization per memory
  `no-agent-app-launches`; ask before the first launch, then follow the protocol
  (scratchpad `QUANTICK_UI_STATE` / `QUANTICK_TRADES_DIR` / `QUANTICK_PAPER_STATE`, kill every
  instance when done).

## Acceptance criteria (evidence required for each)

### Mission-specific

1. **Dated, named rows**: every ledger row (this session and earlier) states the trade's
   civil date and its symbol together with the existing time, duration, exit reason and
   points; nothing today's row shows is dropped, and nothing is truncated at the panel's real
   width. *Evidence: unit test over the row's composed text + visual-qa capture of the dock
   at ~330 px.*
2. **Paginated earlier sessions**: the ledger renders a bounded first page of saved history
   and a control that reveals the next page, naming how many trades remain unshown; the count
   is honest (it matches what is loaded, and says so when the folder holds more).
   *Evidence: unit test over the page/remaining arithmetic + capture before/after the click.*
3. **Calendar filter**: the report opens a month grid; days holding trades are visually
   distinct from days that do not; one click selects a day, a second click makes a range, and
   a clear control returns to the pills. *Evidence: unit tests over the day index and the
   range state machine (first click, second click before/after the first, clear) + visual-qa
   captures of single-day and range states.*
4. **Range drives the report honestly**: the tiles, the equity curve and the grids all read
   the same filtered set; the support line states the selected range in words; an empty
   selection says so out loud and names the control that would widen it (never a silently
   blank report). *Evidence: unit test that a range filter and the rendered view agree +
   capture of the empty-range refusal.*
5. **The trades behind the curve**: the report window lists the filtered trades with date,
   symbol, side, quantity, entry → exit, points and exit reason, in closing order, scrolling
   inside the window without making the window itself un-resizable (the "stuck huge" bug the
   grids already guard against). *Evidence: visual-qa capture at the report's minimum size
   and at a large size.*
6. **Date math is tested, not assumed**: civil-day bounds, range inclusivity at both ends, and
   the display-timezone offset are unit-tested — including a day that straddles UTC midnight
   in a negative offset (UTC-03:00, the user's own timezone). *Evidence: tests green.*

### Injected gates — any code change

7. **Four checks green** after rebasing on the latest `main`: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`,
   `cargo test --workspace`. *Evidence: command output.*
8. **Performance impact declared** — every touched path classified by rate (per-trade /
   per-depth / per-frame / rare) in the plan, before the review. *Evidence: the table in the
   PR body.*
9. **arch-review run** over `git diff main...HEAD` with every Blocker and Should-fix resolved,
   or deliberately deferred and named in the PR body. *Evidence: review verdict.*
10. **PR opened** — the mission is not done before the PR exists; merging is never part of it.

### Injected gates — hot path (per-frame)

11. **Performance flat or better**: the ledger rows and the report window draw per frame, and
    this change adds a date string, a symbol, a calendar grid and a trade list to that work.
    Evidence that frame cost did not regress: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense
    tape with the dock and report open, against a `main` control run — or a bench over a
    fixture if launches are not authorized. Numbers in the PR body.

### Injected gates — user-visible

12. **ui-harness hooks**: every new/changed surface reachable from a fresh launch with zero
    clicks — the calendar open with a known selection, the paginated ledger past its first
    page, the report's trade list. Hooks added in this same change and documented in the
    skill's registry.
13. **visual-qa pass**: the state matrix captured and read against the defect checklist —
    every surface PASS, or a defect explicitly accepted in the PR body.
14. **trader-ux-review**: no unresolved Blocker.

### Injected gates — adds a capability

15. **Additive shape**: the calendar/date-range filter docks as its own module with
    registration-only edits at the call sites; defaults preserve today's behaviour; blast
    radius (files added vs. edited) stated in the PR body.

# Mission — download a replay session from MetaTrader, in the app

**Objective.** Inside the app, a trader picks a symbol and a day, downloads
that day's tape plus N previous sessions of M1 context from MetaTrader, and
presses play with no empty white to the left — pulling more history with the
`Load older` gesture the app already has.

Branch: `feat/replay-download-mt5` · worktree:
`../quantick-worktrees/feat-replay-download-mt5`

## Decisions taken up front

- **Downloader**: a one-shot process (`tools/mt5/export_session.py`) launched
  by the supervisor the app already owns (`crates/app/src/feed/mt5_bridge.rs`).
  It does not share the live tape's socket and does not need the EA attached to
  a chart, so a 60 MB download can never take down a live session.
- **Context**: M1 downloaded once, resampled in-app for 1m/2m/5m/15m via the
  existing `crates/app/src/resample.rs`. The trader never picks a timeframe at
  download time — only how many previous sessions to bring.
- **Scope**: one PR.

## Acceptance criteria

### Mission-specific

1. **One door.** Downloading lives as a `Get data` tab inside the existing
   Market Replay browser (`crates/app/src/replay_view.rs`). No new menu entry:
   for the trader, downloading and replaying are the same task.
2. **Three inputs, one button.** Symbol read from the MT5 Market Watch; day
   picked from a calendar with dataless days disabled; context as a
   previous-sessions stepper. Estimated tick count and size shown *before* the
   click.
3. **Never blocks a frame.** The download runs off the UI thread, is
   cancellable, and reports progress in ticks/MB. The app stays usable
   throughout.
4. **Lands in the format that already exists.** Tape written as a
   `quantick-replay` CSV (`crates/replay/src/format.rs`); the downloaded
   session then appears in the session list marked as on disk and is never
   re-downloaded silently.
5. **No empty white.** On play, the context sits left of the playhead behind an
   amber divider, reusing `REPLAY_ACCENT` so "this is not tape" reads the same
   way it already does for the backfill/live divider.
6. **Data honesty at the point of reading.** The context stretch is labelled as
   broker candles; footprint, bubbles and delta are off across it and say why.
   A delta of zero must never be readable as "no aggression happened".
7. **Four timeframes, one download.** 1m/2m/5m/15m all come from the M1 series
   through `resample.rs`. No per-timeframe fetch.
8. **The gesture that already exists.** `Load older` in replay pulls further
   sessions at the current timeframe's granularity without pausing playback.
   On tick/volume bars it loads candles and says that it did.
9. **Failures are actionable.** Terminal closed, symbol unknown, day with no
   data: each says what to change, never a raw MT5 error code.

### Standard gates (injected by the mission skill)

- Four checks green after rebasing on latest `main`: `cargo fmt --all --
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`.
- **Performance impact declared** by rate for every touched path: the context
  render is per-frame; the download is rare. Classified in the plan, measured
  before the PR — `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape against
  a `main` control run, numbers in the PR body.
- **`new-extension`**: the download source docks as a named port with a fake
  second implementation under test; edits to existing files are
  registration-only; defaults leave today's live chart untouched; blast radius
  (added vs. edited files) stated in the PR body.
- **Test-first in domain crates** (`replay`, resample, format): fixture and
  expected output written before the code; determinism guarded by a golden test.
- **`ui-harness`**: every new surface — the `Get data` tab, the in-flight
  download state, the context stretch on the chart — reachable by env hook,
  added in this same change.
- **`visual-qa`**: all surfaces PASS or defects explicitly accepted.
- **`trader-ux-review`**: no unresolved Blocker. Criterion 6 is the one flagged
  in the design consultation — treat it as a Blocker until proven shipped.
- **`arch-review`**: every Blocker and Should-fix resolved, or deferred in
  writing in the PR body.
- **PR opened** with green CI and the evidence in its body. Merging is not part
  of this mission.

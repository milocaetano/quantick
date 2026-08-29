# Mission

A `+ older` press must either put older history on the chart the trader
selected, or say on screen — once, quietly, without a dismiss — that there is
none to fetch. The *previous session* reach must never end in silence.

## Why this is reopened

The reach shipped without its outcome ever reaching the trader's eyes.

- `Tab::advance_history_campaign` (`crates/app/src/tab.rs`) settles every run
  into a `tracing::info!` line and nothing else. `CampaignEnd` already names
  all seven endings — `ReachMet`, `Exhausted`, `NothingComingBack`,
  `NothingCharted`, `PagesSpent`, `PrintsPulled`, `SpanCovered` — and six of
  them mean *the chart did not move*. On screen those six look identical to a
  press that did nothing at all.
- `crates/app/src/feed/replay.rs` declares `history_paging:
  request.session.context.is_some()` — a fact about *candles* standing in for
  the *trade* capability, directly under a comment that reads "A recording
  pages *candles*, never trades". So a recording puts the reach chips, the
  page-size box and a live `+ older` button in front of the trader, then
  answers every request with `HistoryPrepended(Vec::new())`. The run idles its
  three pages and stops. That is the facade, exactly as reported.
- The context the trader *does* hold on a recording (2819 one-minute candles
  in the reported session) can only reach a tick chart through the
  `venue_lead_in` checkbox in a settings menu. The press they made can never
  surface it and never says so.

## Acceptance criteria

1. **A settled run speaks when the chart did not move.** Every `CampaignEnd`
   carries a trader-facing sentence; a run that ends without having moved the
   chart's oldest print raises it in the existing transient lane. A run that
   met its reach stays quiet — the chart is its own answer.
2. **The single-page reach is held to the same honesty.** It is the default
   reach and today it is equally silent: a press that brings nothing back says
   so.
3. **A recording declares what it can serve.** `history_paging` on a replay
   reflects older *trades*, of which a recording has none, so the trade half of
   the history menu is not offered over one. The "pick up context downloaded
   since this session opened" behaviour moves onto the candle path
   (`FetchOhlcv`), which is the request that actually answers with candles, and
   is proven still to work by a test.
4. **The warning points at the way forward when there is one.** On a recording
   holding context, the message names the candle reach rather than only
   refusing.
5. **Subtle, not modal.** One line in a lane the trader is already watching,
   self-clearing, no dismiss, no error card.
6. **Readable without a mouse.** The settled reason is exposed on the control
   plane beside `history_trades`, so an operator can ask what the last reach
   reached; the new surface registers a `ui-harness` hook.
7. **Tests written before the fix**, covering: each silent ending now leaves a
   message; `ReachMet` leaves none; a recording offers candles and not the
   trade reach; the replay context re-read survives the move.

## Injected gates

- **English** in every tracked artifact (`CLAUDE.md` owns the rule).
- **Four checks green** after rebasing on latest `main`:
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo build --workspace`, `cargo test --workspace`.
- **Performance impact declared**: campaign settle is *rare* (once per run);
  the lane draw is *per-frame* and must stay a borrow of an `Option`, painting
  only while a message lives. No new per-trade or per-depth work.
- **`arch-review`** run over `git diff main...HEAD`, every Blocker and
  Should-fix resolved or deferred in the PR body.
- **User-visible**: `ui-harness` hook for the new surface in the same change;
  `visual-qa` with all surfaces PASS or defects explicitly accepted;
  `trader-ux-review` with no unresolved Blocker.
- **PR opened.** Merging is not part of the mission.

## Verification

| # | Criterion | Evidence |
| --- | --- | --- |
| 1 | A settled run speaks when the chart did not move | `CampaignEnd::notice` gives all six silent endings a sentence; `ReachMet` returns `None`. `a_run_that_reaches_nothing_says_so_where_the_trader_is_looking`, `a_run_that_meets_its_reach_says_nothing`, `every_ending_but_the_one_that_worked_has_something_to_say` — pass |
| 2 | The single-page reach held to the same honesty | `a_single_page_press_that_brings_nothing_back_says_so`, `a_single_page_press_that_lands_prints_says_nothing` — pass. `Tab::empty_page_verdict` reads the same two facts `Campaign::advance` reads first, so the two paths cannot disagree |
| 3 | A recording declares what it can serve | `history_paging: false` in `feed/replay.rs`; `a_recording_never_claims_to_page_trades` covers both with- and without-context sessions. The context re-read moved to `FetchOhlcv`, gated on the file's modified time; `the_candle_reach_picks_up_a_run_up_that_grew_since_the_session_opened` and `the_candle_reach_never_moves_the_playhead` — pass |
| 4 | The warning points at the way forward | `HistoryPagingOff` reads the candle entry's own `OlderCandles` state, so the refusal never ends at a control that is itself greyed out; on a tick chart it names the venue lead-in. `a_refusal_never_points_at_a_control_that_is_also_off` — pass |
| 5 | Subtle, not modal | One muted line in the existing loading lane, no spinner, no dismiss. Screenshots: `note-nothing-coming-back.png` (replay, note up, `+ older` greyed), `note-control-none.png` (same launch without the hook, no note), `note-live.png` (Binance). All captured at 60 fps |
| 6 | Readable without a mouse | `history_reach_note` and `history_reach_running` on the observer feed-status snapshot, both `#[serde(default)]`-compatible with v1; `QUANTICK_HISTORY_NOTE=<ending>` registered in `ui-harness` |
| 7 | Tests written before the fix | Yes — the tests were written and run red first. 1936 app tests pass; 36 test binaries green |

### Gates

- **English** — `language_guard` passes; branch, commits and prose read and English throughout.
- **Four checks** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` all clean. The one failure is `the_bridge_paging_tests_pass`, environmental (`python3` resolves to the Windows Store alias here); `python bridge/mt5/tests/test_paging.py` passes all 21 checks and the bridge is untouched by this branch.
- **Performance** — settle is *rare* (once per reply); `expire_history_note` is *per-frame* over a `Copy` `Option` and one duration compare; the lane's note allocates one galley per frame while visible, as its sibling rows already do. `reload_context` went from every candle request to only a changed file. No per-trade or per-depth work added.
- **arch-review** — step 0 (`code-review`) ran at `xhigh`, 14 findings, 12 confirmed and fixed, 1 already fixed in flight, 1 refuted. Shape pass found the bool-pair refusal and fixed it.
- **User-visible** — `ui-harness` hook added; `visual-qa` PASS on three surfaces; `trader-ux-review` found one Should-fix (a refusal inviting the retry burst `MAX_IDLE_PAGES` exists to prevent) and it is fixed.

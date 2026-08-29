# Mission

An armed strategy on a price-band rectangle must fire on every bar its band
covers — and say so on the chart when it cannot — and duplicating the band must
carry its armed strategy with it.

The investigation that produced this mission is kept in
`.claude/GOAL-investigation-notes.md`: the trader's 2026-08-28 WINV26 session
replayed tick for tick, proving the bar at ~16:02:50 (O 177885 / C 177775, body
110) was `FORCE Sell` on both rulers and `ClosedInside` the band — a valid
`Opportunity::Market` that produced neither an order nor an alarm, with the
account flat and the instance armed before the bar closed.

## Acceptance criteria

1. **The silent hold is reproduced before it is fixed.** A test through the
   real chart path (the `pane` + `tab` sweep, not the kernel alone) arms an
   instance on a band, closes a qualifying bar, and asserts no command comes
   out — failing before the fix, passing after.
2. **A region never expires in silence.** Whatever the span rule ends up being,
   a bar the band cannot judge is named on the **badge**, in words, on the
   chart — not only in `status_line()` behind the drawing's context menu.
   `off_series` gets the clause `hidden` already has; today it pauses the bot
   mute.
3. **The held-fire reason outlives the next quiet bar.** `note` is currently
   cleared by every closed bar carrying no signal, so the reason for the bar
   that mattered is gone one bar later. The last *decided* reason stays until
   another decision replaces it.
4. **One order per region, and nothing outside the region blocks it.** The
   trader's ruling. The account-wide `account_flat` flag stops deciding for
   every armed region on the chart; each region owns its one order and its own
   alarm. Stated consequence, carried into the PR body: two regions may then
   hold positions at once, and `quantick-sim` nets them into one — that is the
   accepted trade, not an oversight.

   **Boundary, stated rather than glossed:** the rule is applied to the
   *strategy's* entry gate, which is the thing that silences a setup. It is
   **not** applied to `quantick-sim`'s own fill model: a resting limit that
   would fill into an open position still stands down
   (`CancelReason::AccountOccupied` -> `DisarmReason::AccountOccupied`).
   Lifting that means giving the simulator per-region accounts instead of one
   net position — a different mission, and one that touches the conservative
   tape-based fill model the crate is built on.

   It is also worth the trader knowing: this gate was **not** what silenced
   the 2026-08-28 setup. The account was flat. The rule is being applied
   because it is the shape they want, not because it was the cause.
5. **A duplicated band carries its strategy.** `Ctrl+D` over a band with an
   armed instance yields a copy carrying its own instance: same preset, same
   trigger params, **fresh** state — `ArmedState::Armed`, no inherited order
   id, no inherited alarm cooldown or preview mark — warmed on the same series.
   `Drawings::duplicate_selected` stays ignorant of strategies; the pane docks
   the instance onto the new id.
6. **Test-first in the kernel.** Any `quantick-strategy` change lands fixture +
   expected output before the code, and the determinism golden test still
   guards it.
7. **Every artifact in English.**
8. **Performance impact declared** — every touched path classified by rate
   (per-trade / per-depth / per-frame / rare). The strategy sweep and
   `strategy_region` are per-trade; the badge is per-frame; duplication is rare.
9. **User-visible surfaces**: `ui-harness` hook added in the same change for
   every new/changed surface, `visual-qa` pass, `trader-ux-review` with no
   unresolved Blocker.
10. **Four checks green** after rebasing on latest `main`:
    `cargo fmt --all -- --check`,
    `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo build --workspace`, `cargo test --workspace`.
11. **`arch-review`** run over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred in the PR body.
12. **PR opened.** Merging is not part of the mission.

---

# Investigation record

Find why the `SellGainAlarm` instance armed on the WINV26 red region placed no
order and sounded no alarm on 2026-08-28, name the gate that held it, explain
what changed since it last fired, and fix whatever the diagnosis shows is a
defect rather than a setting.

## Evidence gathered before the work started

- The running app (PID 23356, built from `cd6c764` at 15:46) shows the badge
  `⚡ SellGainAlarm` with no state clause — the instance is `Armed`, watching,
  alarm mark `Quiet`.
- `quantick-strategies.toml` (written 16:09 today): `SellGainAlarm` is
  side=sell, window 20, min_factor 2, max_factor 4.5, min_body 100,
  rearm auto, on_break retest_limit, alarm on / on_close / once_per_bar /
  short-beep cut at 1 s. The preset compiles — both sound tokens exist and
  every bound is honoured.
- `indicators-state.toml`: the painted `force_bar.pine` ruler runs
  window 20, min 2.0, **max 4.0**, **no body floor**, bear colour pure red.
  The armed ruler runs max 4.5 **and a 100-point floor**. The two rulers on
  one chart are not the same ruler.
- Paper journal for today: a short fired at 10:12:45 (178685) and stopped at
  10:13:33, so the instance was firing earlier in the session.
- `badge_text` never renders the instance's `note`. The reason a bar was held
  lives only in `status_line()`, which is reachable through the drawing's
  context menu — the trader has to go looking for it.

## Acceptance criteria

1. The gate that held the bar is named from evidence — a deterministic replay
   of the 2026-08-28 WINV26 tape through the trader's exact preset and region,
   printing the verdict per closed bar, not from reading the source.
2. The answer to "why did it fire before and not now" is stated with the
   change (config or commit) that made the difference.
3. Any defect the diagnosis exposes is fixed, with a test that fails before
   the fix and passes after.
4. Every artifact in English.
5. Performance impact declared: every touched path classified by rate
   (per-trade / per-depth / per-frame / rare).
6. Four checks green after rebasing on latest `main`:
   `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
   warnings`, `cargo build --workspace`, `cargo test --workspace`.
7. `arch-review` run over `git diff main...HEAD`, every Blocker and Should-fix
   resolved or deferred in the PR body.
8. If a user-visible surface changes: `ui-harness` hook added in the same
   change, `visual-qa` pass, `trader-ux-review` with no unresolved Blocker.
9. PR opened. Merging is not part of the mission.

## Findings (deterministic replay, 2026-08-28 WINV26, 1,399,146 ticks)

Bar cuts aligned to the chart by voting the flow pane's x-axis label times
against every possible tick(2000) offset: offsets 1964-1968 reproduce all
eight labels, offset 0 reproduces three, every other offset none.

The red band (≈177,626-177,822) was met by exactly six bars all afternoon:

| bar | close | O -> C | body | armed ruler (2..4.5, floor 100) | painted ruler (2..4, no floor) | body vs region |
|---|---|---|---|---|---|---|
| 632 | 15:54:30 | 177470 -> 177635 | 165 | exhaustion Buy 4.52x | exhaustion Buy 4.52x | closed inside |
| 633 | 15:55:00 | 177640 -> 177590 | 50 | quiet 1.30x | quiet 1.30x | cut through |
| 637 | 16:00:30 | 177570 -> 177645 | 75 | **under floor 2.17x** | **FORCE Buy 2.17x** | closed inside |
| 638 | 16:00:54 | 177640 -> 177785 | 145 | FORCE **Buy** 3.60x | FORCE Buy 3.60x | closed inside |
| 639 | 16:01:20 | 177790 -> 177740 | 50 | quiet 1.23x | quiet 1.23x | closed inside |
| 641 | 16:02:31 | 177810 -> 177815 | 5 | quiet 0.11x | quiet 0.11x | closed inside |

1. **No sell-side force bar met that band all afternoon.** Every bar that
   closed inside it pushed up. A sell instance holds on "opposite side" —
   correct behaviour, and the direct answer to "it closed inside the zone".
2. **The two rulers on that one chart disagree.** Bar 637 is painted as a
   force bar and refused by the armed instance for the 100-point floor.
   Bars 675 (2.50x, body 95) and 684 (2.13x, body 80) are the same story.
3. **Why it fired before and not now**: the band is relative, the floor is
   absolute. The 20-bar average body was ~66 points this morning (bar 260,
   10:43:33, fired a sell inside this same band) and ~35 points this
   afternoon. A 100-point floor is 1.5x the morning average — below the
   band's own 2.0x edge, so it never bound. Against a 35-point average it
   is 2.9x — above that edge. Everything between 2.0x and 2.9x is now
   painted force and refused by the bot. That dead zone opened when the
   tape went quiet, not when the code changed.
4. **The reason is destroyed before the trader can read it.** `note` is
   cleared on every closed bar that carries no signal, and `badge_text`
   never renders it at all — it lives only in `status_line()`, behind the
   drawing's context menu. The bar that mattered (16:00:54, "opposite
   side") had its reason erased by the next quiet bar.

## Correction, after the trader's marked screenshot

The region is **mutable and moved constantly** — the trader repositions it
through the session. The band captured live at 17:25 (≈179,005-179,106) is not
the band that was on screen when the setup happened (≈177,626-177,822), and the
chart had also been re-opened in between, which re-phases the tick(2000) cuts.
So the axis-label alignment recovered from the 17:25 capture describes the
*re-cut* chart, not the series the armed instance was actually judging.

Scanning all 2000 alignments for a painted sell force bar meeting the band this
afternoon: 48 of 2000 hit, clustering at 16:01:30 and 16:02:45-16:02:50. The
group that matches the marked candle's geometry — body opening above the band's
top and closing inside it — is 16:02:50:

    O 177885  H 177890  L 177775  C 177775   body 110
    painted FORCE Sell 2.03-2.25x | armed FORCE Sell 2.03-2.25x | ClosedInside

Both rulers call it force, it clears the 100-point floor, and the region test
returns `ClosedInside` -> `Opportunity::Market`. **The kernel wanted to place a
market sell.**

### What actually held it

`PaperTrading::is_flat()` (`crates/app/src/paper_trading.rs:1425`) is false
whenever a *working order* exists, not only an open position:

    self.sim.position().is_none() && self.sim.orders().is_empty() && self.sim.queued().is_empty()

The screenshot shows a resting **BUY limit 1** tag — `kind_word(EntryKind::Limit)`,
so a working order, not a position. `ArmedStrategy::on_closed_bar` therefore
reached `if !account_flat { note = "trigger held: account not flat" }` and
returned no command. That is the note the trader read on the badge.

**Why it fired before and not now**: both buy presets now carry
`on_break = "retest_limit"` (`BuyGainAlarm`, `Buygain`). A retest limit rests
indefinitely. While one rests, **every sell instance on the chart is muted** —
one bot's parked order silences another bot's setup. In the 2026-08-18 backup
every buy preset used `on_break = "ignore"`: nothing rested, the account was
flat between trades, and the sell fired.

### The alarm

The alarm deliberately ignores the account gate, so it should have sounded.
`quantick-strategies.toml` was written at **16:09**, six minutes after the bar —
if `SellGainAlarm` was created then, the instance armed at 16:02:50 was
`SellGain`, whose `alarm = false`. The alternative is that it sounded and was
missed: `short-beep` cut to `alarm_play_secs = 1`. Not separable from the
evidence on disk.

### Still true, and adjacent

- The band is relative, the floor absolute. The 16:01:30 alignments show the
  same shape with body 85-95: painted as force, refused as `under-floor`. The
  marked bar cleared 100 by ten points. Afternoon average body ~35 points makes
  the 100-point floor equivalent to 2.9x — above the band's own 2.0x edge, so
  everything in 2.0x..2.9x is painted and refused.
- Nothing records where the region *was* when a bar closed. The trader moves it
  all session; the app cannot answer "why didn't it fire at 16:02" afterwards,
  and neither can a reviewer.

## Retraction: there was no resting order

The `BUY limit 1` tag in the trader's screenshot is the **aim preview**, not a
working order. Two tag builders exist and they read differently:

- preview (`paper_trading.rs:2005-2011`): `side_word_upper` + `kind_word` +
  quantity -> `BUY limit 1` — painted while the modifier is held over a price,
  "while it paints, the click is already live", no order exists.
- resting order: `BUY LMT 1` — uppercase, abbreviated, pinned by
  `a_resting_order_tag_is_a_pill_until_the_pointer_reaches_it`
  (`paper_trading.rs:8875`).

The screenshot shows the lowercase spelled-out form. **The account was flat.**
The `account_flat` gate did not hold this bar, and the "one bot's resting limit
mutes the other" story, while a real hazard, is not what happened today.

## What the tape still proves, and what is left

Proven: at ~16:02:50 a bar with O 177885 / C 177775, body 110, was `FORCE Sell`
on **both** rulers and returned `ClosedInside` against the band — a valid
`Opportunity::Market`. The geometry, the side, the band and the floor all
passed.

With the account flat, only three gates silence the order *and* the alarm
together:

1. `region_active == false` — the rectangle's span does not cover the closing
   bar's slot (`extend_right` off with the right anchor behind the tape), or
   `strategy_region` returned `None` (`off_series`, `foreign_market`, hidden).
   Note that `paint_strategy_badge` appends "region hidden — paused" only for
   `hidden`; an `off_series` region pauses the bot with **no word on the
   badge**.
2. `signal.side != params.side` — not this bar, it was a sell.
3. No instance existed yet: `ArmedStrategy::warm` replays the warmup bars with
   the gates shut by design, so arming after the fact never fires on a bar that
   already closed.

Only the live app can separate these. None is distinguishable from the tape.

## The trader's ruling on the design

Stated directly: **one order per region, and nothing may block the order or the
alarm.** The global `account_flat` gate — one account-wide flag deciding for
every armed region on the chart — is not the shape they want. Each region owns
its one order and its own alarm.

# Strategy anchors — the semi-automatic operation

The division of labour is the product's thesis, and the operational
document demands it: intensity signals (force bars, elephants) never fire
alone on WIN — index arbitrage prints them mechanically, blind to any
level. Context is what a human reads off the chart; the trigger inside
that context is a reaction-time race a human always loses. So: **the
human draws the region, names it, and arms it; the machine watches every
closed bar and pulls the trigger** — in paper trading, through the exact
funnel manual orders use.

One brain, two consumers: the `quantick-strategy` kernel the chart arms
here is the same one `quantick-backtest --strategy force-region` runs
headless over recorded sessions. Never fork strategy logic per consumer.

## The loop, as the trader lives it

1. Draw a **rectangle** over the congestion region (price band × a span
   of the tape; stretch it past the newest bar to keep it live into the
   future). Right-click it, name it if you like ("congestão 108k").
2. Right-click → **Add strategy…** — the arming dialog: pick a bank
   preset or shape one (side, quantity, the force band, the projection
   multipliers, re-arm policy), optionally **Save preset**, then **Arm**.
3. The drawing wears a badge from that moment: `⚡ preset · state`. The
   trigger warms itself on the bars already on screen, so armed means
   armed now.
4. On every **closed** bar: is it a force bar of the armed side, closing
   inside the region, with the account flat? Then a market order with its
   projected bracket goes to the simulator, filling on the next print —
   like the hand would, minus the seconds the hand needs. A bar that
   instead **cut clean through** the region may rest a limit at the edge
   it cut, if the instance was armed with that option — see **Region**
   below for what counts as a cut.
5. The operation runs to its bracket (or to a manual close — the human
   always outranks the bot). One-shot instances are then `done`; auto
   instances re-arm and hunt the next qualifying bar.

## Semantics (the kernel's contract)

- **Trigger — force bar**: body between `min_factor`× and `max_factor`×
  the simple average of the last `window` bodies, the judged bar
  included, exactly the shipped `force_bar.pine` ruler (defaults 1.5,
  2.5, 20) — **plus an absolute floor on the candle's range in points**
  (`min_range`, form default 100; `0` disables). The floor measures
  `high - low`, wicks included, while the band above measures the body:
  the two gates object to different things, one to a bar that is small
  next to its neighbours and one to a bar that is small in price.
  (It measured the *body* until the branch that renamed it; a bank saved
  then still loads, and its number is read as a range floor from that
  point on.) The relative band alone is honest on time
  candles but promiscuous on activity-cut bars: measured on a live WINV26
  session **against the body floor**, the bare band called 247 of 1,355
  volume bars "force" (a 55-point body at 1.62× fired a real paper
  entry); the 100-point body floor
  left 7. **That figure bounded the body
  floor, not this one**: a range floor admits every bar a body floor of
  the same number admitted and more, since `high - low >= |close - open|`
  always. Nobody has re-measured the range floor on that session, so the
  honest statement is "looser than the gate that produced 7, by an
  unmeasured amount". An elephant has a size, not only a ratio. A body above the band
  is **exhaustion and does not fire** — too big to chase, by design.
  Dojis have no side. The window must be full; warmup is a badge state,
  not a silent zero.
- **Region**: the rectangle's price band, inclusive of its edges, with
  the bar's slot inside the rectangle's span of the tape. A rectangle
  that ended in the past is an expired region — stretch it to re-live it.

  The band is judged against the trigger bar's **body — its open and its
  close. The wicks are not consulted**: a shadow poking into the band is
  the level being probed and refused, not cut. Reading the bar's open `o`
  and close `c` against a band `[low, high]`, for a **sell** instance:

  | Geometry | What happens |
  | --- | --- |
  | `c` inside `[low, high]`, whatever `o` did | market sell |
  | `c < low` and `o >= low` — the body cut the lower edge | a limit rests at `low` if **on-break** is `retest_limit` *and* the projected legs clear that edge; otherwise the bar is reported as a cut the option declined |
  | `c < low` and `o < low` — the body finished past an edge it never crossed | nothing |
  | `c > high` — closed away, above the band | nothing |

  A **buy** instance is the same rule mirrored around `high`. The kernel
  answers this as `Region::body_cut` (`crates/strategy/src/region.rs`),
  one definition for the chart and the backtest both.

- **On break** (`on_break`, default `ignore`): what a bar that *cut* the
  region does. `retest_limit` rests a limit at the edge the body cut —
  the price the tape must revisit for the retest — bracketed off the
  trigger bar and cancelling itself if the bar's projected target trades
  before the return. The order also stands down at its own fill moment if
  a position is open by then: a bot never trades against a hand. `ignore`
  holds fire and says so on the instance's status line.

  Two cases the option cannot deliver, both narrated on the status line
  rather than silently:

  - **The legs do not clear the edge.** The bracket is projected off the
    trigger bar's close, but the entry prices at the *edge*. A leg landing
    on the wrong side of that edge would be dropped at fill time, and an
    entry is never armed unprotected — so the instance refuses the cut
    instead ("trigger held: the retest bracket does not clear the edge"). A
    tight `sl_mult` on a bar that closed just past the edge is the usual
    way to meet this.
  - **No take-profit leg means no expiry.** The cancel-at level *is* the
    projected target, so with `tp_mult` at `0` the order carries none and
    rests until it fills or the instance is disarmed. The status line says
    "until filled or disarmed" rather than "cancels at target" — believe
    the badge over the checkbox.
- **Projection**: measured on the trigger bar's **full range, wicks
  included** — the body decides *whether* to trade, the range decides
  *how wide*. TP = close + `tp_mult` × range(trigger bar) in the
  trade's favour; SL = close − `sl_mult` × range against it (defaults
  1.0 / 1.0; `0` = no leg). A promised leg that cannot be priced holds
  fire — never fires unprotected.
- **Flat gate**: the whole account must be flat, manual positions
  included. A bot never trades against the human's open operation, and
  netting means at most one live operation per chart.
- **Re-arm**: `one_shot` (default — each arming is at most one
  operation; re-arming is a human gesture on the menu) or `auto`.

## Every stop is named

An instance never goes quiet silently. The badge states:

| State | Colour | Meaning |
| --- | --- | --- |
| `armed` | accent | watching; the per-drawing menu narrates the trigger ("warmup 7/20", "quiet 0.8×") |
| `fired` | amber | entry command queued, waiting for the next print |
| `in position` | buy-green | entry filled; the bracket (or the human) will end it |
| `done` | muted | one-shot completed |
| `timeline reset` / `bar spec changed` / `market changed` / `entry rejected` / `entry cancelled` / `protection dropped — closed` / `disarmed` | faint | why it stopped watching |

The sweeps behind those reasons: a replay seek or session reset, a bar
spec change (the body average means something else under another cut —
re-arming after either resets the ruler and re-warms honestly), a symbol
switch (the region belongs to the market that left), a simulator
rejection (the reason is the curriculum), a manual flatten sweeping the
pending entry, and a protective leg the simulator dropped at fill time —
the market outran the level, so the instance closes the position at the
next print (the exit the leg would have taken, late but honest) and
stops. Deleting the drawing removes its instance outright — a live
position keeps its bracket and becomes the human's. **Hiding** the
drawing pauses the bot instead: the badge stays painted (it is the one
mark that never hides) and says `region hidden — paused`; showing the
drawing resumes it. A drawing that lost its footing on the series, or
that belongs to another market, pauses it the same way and says which
(`region off its series — paused`, `region on another market — paused`).

A region whose **drawn span** no longer reaches the next bar to close
pauses it too, and this one the trader reaches by accident: the band is
a rectangle they move all session, and a tape that walks past its right
edge — or a drag back over history — leaves it covering no future bar.
The badge says `region ended — stretch it right`. It is a hold and not a
disarm: the instance stays armed, its alarm keeps listening, and the
moment the band covers the future again it fires on the next bar with no
button pressed. Turning on **extend right** is the standing answer.

The badge also carries the last refusal itself — `opposite side`,
`the body never cut the region`, `account not flat` — and keeps it after
the bar it happened on, because a trader reads the chart *after* the
move and a single quiet bar used to erase the reason. The badge also
counts the queued entry as occupying the account, which is what stops
two instances co-triggered by one bar from stacking orders: the
simulator models one netted position carrying one bracket, so a second
fill would silently replace the first instance's stop.

**Duplicating** a band (Ctrl+D) carries its bot: the copy is armed from
the same preset, with a fresh ruler, a fresh alarm and no inherited
state. A band whose bot was stopped — disarmed, or a spent one shot —
copies as a plain rectangle; the copy does not resurrect what the trader
stopped. When the copy cannot be armed (it is hidden, off its series, or
on another market) the status line says so rather than leaving a silent
band that looks armed.

## The bank (`quantick-strategies.toml`)

Named presets, versioned, in the durable cockpit home
(`QUANTICK_STRATEGY_PRESETS` overrides the path). A preset is the
**declarative** half of an instance:

```toml
version = 1

[presets."BF venda 1x1"]
trigger = "force_bar"
side = "sell"
quantity = "1"
window = 20
min_factor = "1.5"
max_factor = "2.5"
tp_mult = "1.0"
sl_mult = "1.0"
rearm = "one_shot"
```

Decimals travel as strings and parse exactly; a row this build cannot
faithfully execute (unknown trigger, unparsable field) is refused whole,
never approximated. A future trigger — the operational document's BEI —
is a new `trigger` token and a new kernel arm; the state machine, the
menu, the bank and the badge never change.

**This is deliberately the future NL surface.** "Venda em BF vendedora
em qualquer região do quadrado *congestão 108k*" compiles to: a preset
(this table), a drawing reference (drawings now have stable ids and user
names), and one `arm(preset, drawing, side)` call — exactly what the
menu emits today. The AI layer, when it comes, writes rows and calls
arm; it never touches the runtime.

## Recorded decisions

- **Paper only.** There is no real order route in the workspace (the MT5
  bridge is feed-only), and this feature does not add one. Real routing
  is a future goal with its own safety design.
- **Armed instances do not persist** across restarts or sessions — the
  same rule drawings and open positions follow: redrawing the morning's
  regions is ritual de leitura, and re-arming is part of it.
- **Rectangles only** carry strategies today: the one shape whose two
  anchors honestly bound a price region. Other shapes can dock later by
  widening the menu gate, not the kernel.
- **The human outranks the bot, always**: manual close/flatten simply
  works (the instance sees the account go flat and completes), disarming
  an in-position instance hands the operation — position, bracket — to
  the human, and a manual flatten that sweeps a pending bot entry
  disarms it (`entry cancelled`; the bot does not insist).
- **Measurement before conviction**: this tool exists to *measure* the
  hybrid setup (human region + mechanical trigger) in replay and paper.
  The harness's `force-region` strategy measures the mechanical half over
  recorded sessions; neither is a claim about edge.

## Validation hooks (ui-harness)

`QUANTICK_STRATEGY_DEMO=1` (rectangle + armed instance, badge on),
`QUANTICK_STRATEGY_DEMO=popup` (the arming dialog over it),
`QUANTICK_STRATEGY_PRESETS=<path>` (relocate the bank). Pair the demo
with `QUANTICK_CONTEXT_MENU=chart` to land the scripted right-click on
the rectangle and photograph the per-drawing menu.

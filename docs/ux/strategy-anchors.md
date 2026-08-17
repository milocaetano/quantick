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
   like the hand would, minus the seconds the hand needs.
5. The operation runs to its bracket (or to a manual close — the human
   always outranks the bot). One-shot instances are then `done`; auto
   instances re-arm and hunt the next qualifying bar.

## Semantics (the kernel's contract)

- **Trigger — force bar**: body between `min_factor`× and `max_factor`×
  the simple average of the last `window` bodies, the judged bar
  included, exactly the shipped `force_bar.pine` ruler (defaults 1.5,
  2.5, 20). A body above the band is **exhaustion and does not fire** —
  too big to chase, by design. Dojis have no side. The window must be
  full; warmup is a badge state, not a silent zero.
- **Region**: the rectangle's price band, inclusive of its edges; the
  bar's *close* must sit inside it, and the bar's slot inside the
  rectangle's span of the tape. A rectangle that ended in the past is an
  expired region — stretch it to re-live it.
- **Projection**: TP = close + `tp_mult` × range(trigger bar) in the
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
drawing resumes it. The badge also counts the queued entry as occupying
the account, which is what stops two instances co-triggered by one bar
from stacking orders.

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

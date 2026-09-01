# The second operator

Criterion **G7**: can something that is not holding the mouse set what this
branch adds, read back what it did, and discover that it exists?

## Act — both halves of the reach, with no mouse

```
QUANTICK_HISTORY_REACH=span
QUANTICK_HISTORY_REACH_SPAN_MINUTES=240
```

Both go through the same functions the menu calls — `set_history_reach` and
`set_history_reach_span_minutes` (`crates/app/src/app.rs`) — never a parallel
path, and the second clamps to the campaign's own span cap so a hook cannot
promise a reach no press can reach.

## Read — what the application says it did

`quantick_get_snapshot` over `workspace.summary`, through `quantick-mcp` on
the running instance, with nothing but those two environment variables set:

```
history_reach              = span
history_reach_span_minutes = 240
```

The transcript, with the raw answer under it, is
[`second-operator-reach-readback.txt`](second-operator-reach-readback.txt).
(An earlier revision of this file cited `second-operator-reads.txt` for these
two lines; that file holds the *fill-progress* polling below and never held
them. `delivery-review` caught the mis-citation.)

## Read — the opening fill's progress

`feed.status` gained `opening_slices_remaining`
(`crates/app/src/control/feed.rs`), fed from `Tab::opening_slices_remaining`,
which the tab sets from each slice's `remaining` and clears when the fill ends.
It is absent when nothing is filling — so an operator can tell *"this chart is
still arriving"* from *"this is all there is"*, which `history_trade_count`
alone cannot say: it rises with no denominator.

**What is proven, and how.** Two halves, tested separately because they fail
separately:

- The tab's reader is asserted by
  `an_opening_slice_draws_without_answering_the_traders_press`
  (`crates/app/src/tab.rs`), which drives a real slice through `drain_feed` and
  checks `opening_slices_remaining() == Some(4)`.
- The wire contract is asserted by `the_fill_progress_is_on_the_wire_and_is_optional`
  (`crates/app/tests/session_gap_agreement.rs`), which checks the shipped
  feed-status schema carries the field **and does not require it** — absent is
  the steady state, so a required field would make every idle snapshot fail its
  own schema.

What neither covers is the one-line copy between them in
`crates/app/src/control/feed.rs`. That is review-covered rather than
test-covered, and saying so is cheaper than implying otherwise.

**What is not proven, and why — stated rather than implied.** I tried five
times to catch the value live over the control plane and could not. The fill is
now shorter than one client round trip: with the slice at its default the whole
session lands in about three seconds, and spawning `quantick-mcp` and getting
an answer costs more than that, so every read returned the settled state
(`history_trade_count = 1 525 571`, field absent — correct, and not the
transient I was after). Reducing the slice to 20 000 to lengthen the window did
not help; the fill still finished first. The reads are in
[`second-operator-reads.txt`](second-operator-reads.txt) exactly as they came
back.

So the claim here is the narrow one: the field is wired end to end and unit
tested, and a live sample of a three-second transient is beyond this client.
The corroborating artifact that the state exists at all is
[`progressive/mt5-mid-fill.png`](progressive/mt5-mid-fill.png) — the chart
caught mid-fill at `7999+0 bars` with six slices still to come, against
`30510+0 bars` when complete.

## Discover — the capability announces itself

- `HistoryReach::ALL` backs the menu, the hook and `from_token` alike, and
  `every_reach_is_reachable_by_its_token_and_says_what_it_does` fails for a
  reach added without a token, a label or a hover.
- `ScriptedMenu::ALL` does the same for `QUANTICK_MENU`, which was a string
  equality against one literal before this branch.
- Both new hooks are in the `ui-harness` registry, which is where an operator
  looks for what exists.

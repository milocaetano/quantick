# Trader UX review — MT5 session history

Two trader-facing changes: the chart opens on the whole session, filled in
progressively; and a third `+ older` reach, *by time*.

## Flow 1 — opening the mini index in the morning

**Rafa (scalper).** Presses nothing. The chart has bars in about a second
instead of eleven, and the morning arrives behind him while the tape runs.
Frame cost measured under the real 1.5 M-print load: the floor is 54 fps for
about eight seconds against the control's 58, with no `APP_SLOW_FRAMES` on
either side ([`perf.md`](perf.md)). Rafa reads flow from motion, so that dip is
the one thing here he would feel — it is at open, before he is reading, and it
recovers to 59. An earlier slice size made it 43 fps with three slow-frame
warnings, which would have been a FAIL of this review.

**Marina (context).** Gets what she actually asked for: the day from 09:03 on
a tick chart, 30 510 bars, rather than a window whose left edge moved with the
clock. The bar count is the thing she checks and it is on the status bar.

**Duda (newcomer).** Nothing to learn. The one state she could misread — a
session larger than the cap — now says so on the chart with the way to the
rest, instead of only in a log ([`no-silent-cut.md`](no-silent-cut.md)).

### Finding 1 — **Blocker, fixed in this branch**

*Flow:* pressing `+ older` while the opening session is still filling in.
*Persona:* Rafa and Marina both.

`Tab::drain_feed` ended the loading indicator and advanced the history
campaign on **every** prepended block (`crates/app/src/tab.rs:3031`, `:3053`).
The opening slices arrived as `FeedEvent::HistoryPrepended`, so a press made
during the fill would have had its spinner stopped by the next slice — and its
campaign handed up to thirty pages it never fetched, spending the run's budget
or declaring it finished on tape it did not pull.

This is precisely what the protocol's `opening` flag exists to prevent, and it
was prevented at the feed's pager and then lost one layer up: the flag was
flattened into the ordinary reply before it reached the tab's own request
state. The pager was right and the chart was not.

*Fix, applied:* `FeedEvent::OpeningPrepended`, drawn and counted exactly like a
reply and settling nothing. Guarded by
`an_opening_slice_draws_without_answering_the_traders_press`, with
`a_page_the_trader_asked_for_still_answers_the_press` beside it so the reply
path cannot quietly stop working instead.

## Flow 2 — reaching further back with *by time*

**Rafa.** Does not use it; his reach is one page and that is unchanged. The
default reach is untouched, so nothing he presses behaves differently.

**Marina.** The reason it exists. Two thousand prints against a contract
printing 1 525 621 a day is a couple of minutes per click; "two more hours"
is a unit she thinks in. The hover states the part that would otherwise
surprise her — nights and weekends are crossed, not counted — at the control
rather than in a manual.

**Duda.** The chips read as words (`one page`, `previous session`, `by time`),
and the box is labelled `hours of tape per press` and prints `3 h`, not
`180`. She can tell what a press will do before making it.

### Finding 2 — **Consider**

*Flow:* watching the session fill in.
*Persona:* Duda.

The bridge reports how many slices are still to come and the app logs it
(`MT5_OPENING_PAGE_READY … remaining`), but nothing on screen counts it down.
Rafa and Marina do not need it — the chart visibly grows leftward — but a
newcomer cannot tell "still loading" from "this is all there is" for those ten
seconds.

*Not fixed here.* The honest place for it is the loading lane, which belongs to
requests the trader made; putting an unasked-for fill in it needs its own
design pass, and inventing one inside a bug-fix branch is how a second
progress language gets born. Deferred to the PR body.

## Verdicts

- **Rafa can trade through it.** No new gesture and no focus theft. The one
  cost he could feel is an eight-second dip to 54 fps while four times the
  session arrives — at open, before he is reading, and clear of the app's own
  slow-frame threshold.
- **Marina keeps her workspace.** The reach and its span are persisted, the two
  existing reaches are unchanged, and the config seed is documented.
- **Duda can figure it out alone.** Words not numbers, a hover that states the
  surprising half, and a cut that explains itself — with one gap she would
  feel (Finding 2) and nobody else would.

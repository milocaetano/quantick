# The review round budget

Ledger entry **L3**, issue [#575](https://github.com/milocaetano/quantick/issues/575),
campaign [#563](https://github.com/milocaetano/quantick/issues/563). Graded under
[the experiment protocol](experiment-protocol.md), proof form `removal`,
mechanism variable `removed_context_requests`.

This file holds the measurement and the reasoning. It is deliberately outside
the context ratchet's tracked set — the instruction trees pay only for the
rule. Four instruction files link here as the justification for what they
state, so a session can follow the link and load this file on demand. The
point of keeping it out of the trees is that none of them has to: the rule
reads and decides without it, and only a reader asking *why* pays for this.

## 1. What the ranking asked for, and what the evidence now supports

[`ranking.md`](ranking.md) ranked three removals third among the campaign's
sinks. This mission takes a reading on each and **ships two of the three**.

| #566 proposed | This mission | Why |
| --- | --- | --- |
| `delivery-review` exempt at `medium` | **refused** | its catch rate was never measured, and the first mission that could record a refusal recorded one |
| `ai-review` capped at one round | **capped at two passes** | one reading at the draft head, one at the final head; the third and later passes are re-stamps, not readings |
| a round budget on `arch-review` | **capped at two passes**, step 0 untouched | same measurement, same shape |

### The refusal, stated plainly

`ranking.md` priced `delivery-review` as *"undefined — 57 runs, 0 gating
catches"* and named it removable at `medium`. Four lines above that row the same
document explains why the zero is there: the durable report *"is published only
once the round has converged"*. A refusal that was raised and repaired inside a
round therefore never reached the public record. **The 57-run zero is not a
catch rate of zero; it is an unmeasured catch rate**, and the row should have
said so.

The instrument changed at [#580](https://github.com/milocaetano/quantick/pull/580):
phase two dispatches `delivery-review` as its own context which answers
`PASS|FAIL` to a caller, so a refusal is now visible whether or not it is later
repaired. The very first mission measured that way returned **FAIL on a real
unmet criterion, correctly**, and the repair it forced was the largest single
repair context on that mission — 188 requests, 21,034,914 tokens. One
counterexample does not make `delivery-review` cheap, but it does destroy the
only measurement that said it was removable.

Decision **D8** says a reduction chosen on feel rather than on measurement is
still a failure. Removing `delivery-review` on a number the ranking's own prose
says cannot be read would be exactly that, so it stays at `medium`. This is the
smaller number the dispatch asked for rather than the modelled one.

## 2. What a third pass actually was

Durable `quantick-review-report:v1` comments, counted per pull request:

| PR | `arch-review` | `ai-review` | `delivery-review` | passes a two-pass budget removes |
| --- | ---: | ---: | ---: | --- |
| 569 | 1 | 1 | 1 | none |
| 571 | 3 | 3 | 1 | `arch#3`, `ai#3` |
| 572 | 4 | 4 | 1 | `arch#3`, `arch#4`, `ai#3`, `ai#4` |
| 578 | 3 | 1 | 1 | `arch#3` |
| 579 | 1 | 1 | 1 | none |
| 580 | 3 | 3 | 1 | `arch#3`, `ai#3` |

Read the pattern rather than the counts. On #580 both third passes ran as
contexts whose whole brief was *"refresh the projection at the branch's new
head"* — they exist because a repair moved the head and invalidated an
otherwise current marker, not because anyone thought the diff needed a third
reading. On #578 the third `arch-review` pass follows the `delivery-review`
report by two minutes, for the same reason. On #571 the campaign's own summary
records round three's findings as Considers.

So the third and later passes are **projection refreshes forced by a moved
head**. A two-pass budget does not remove a reading: every diff still gets one
full reading at the draft head and one at the final head, per review. What it
removes is the extra refresh caused by repairing in more than one wave.

## 3. The references

A `removal` borrows the length of what it removed from missions that still ran
it, over at least `MIN_REFERENCE_N = 3` of them, priced at the **smallest**.

Each removed pass is measured **inside the reviewing context itself**, windowed
by that review's own durable report timestamps — or, where the pass ran as its
own dispatched context (#580), by that context's whole length. Repairs the pass
provoked are *not* counted, which understates what the budget removes and is
the conservative direction.

| Reference | Total requests | Removed passes | `removed_requests` | `removed_billable` |
| --- | ---: | ---: | ---: | ---: |
| PR 571 | 884 | 2 | 31 | 7,147,085 |
| PR 572 | 2,838 | 4 | 89 | 18,549,602 |
| **PR 578** | **317** | **1** | **21** | **7,027,952** |
| PR 580 | 1,604 | 2 | 115 | 8,275,890 |

`total_requests` is the mission's whole attributed context set — its own
contexts plus every agent it dispatched — because a removal that took a
dispatched context out could not otherwise be seen in the total at all.

**#569 and #579 are not references.** Neither ran a third pass of anything, so
neither has a removed context to measure; they are already in the post-lever
state and cannot price what they never ran. Including them would put a zero
into `min(removed_requests)`, and a zero pre-lever value is `void` under
protocol section 5 rather than generous.

### The unit, stated

"Priced at the smallest" is per **reference mission**, not per removed pass.
The unit is not chosen here: `experiment.py`'s `smallest_reference` selects
`min(references, key=removed_requests)`, and `removed_requests` is a
per-mission field of the reference record — the same field this table's fourth
column fills. `removed_rows` then divides it by `removed_contexts` to price
the counterfactual, so the per-pass figure is derived from the per-mission one
rather than competing with it.

Read per removed pass instead, the ordering changes: **#571 is smaller at 15.5
requests** against #578's 21, with #572 at 22.3 and #580 at 57.5. That is a
stricter number in one axis, and it is recorded here rather than hidden. It is
not the one the protocol's schema carries, and this document does not switch
to it — a lever cannot pick the unit it is graded in. The `removed_requests`
values above and in `experiments.json` stand as measured.

**The smallest reference is PR 578, at 21 requests over 1 removed context.**
That is what the counterfactual is priced at, and #578 is also the reference
with the smallest total, so the ordinal half of K1 is judged against **317
requests**.

### The confound, named

#571 and #572 ran before L1 (the 80-request context cap) shipped, so their
totals carry L1's saving as well as L3's. Protocol section 8 serialises the two
levers for exactly this reason. Two facts keep it honest here: the smallest
reference on both axes is #578, whose own review passes ran inline and are
unaffected by L1's context arithmetic; and #580, the one fully post-L1
reference, is reported beside the protocol's own answer so the stricter reading
is visible.

## 4. The rule that ships

At `medium` and below, `arch-review` and `ai-review` each get **two passes per
head-freezing round**: one at the draft head, one at the final head after the
last repair.

The unit matters, because `mission_ship_gate.sh` reads each marker at the
exact head. Red CI after the second pass, a rebase onto a moved campaign tip
or an in-branch delta fix all move the head and invalidate a marker that was
current. The pass that re-stamps it is a **refresh**, not a new round, and it
is outside the budget — a budget that forbade it would forbid the gate.

1. Repairs are consolidated. Every repair batch completes before the second
   pass, so the second pass is the refresh and there is no third.
2. `delivery-review` runs **before** those second passes, so a gap it names is
   repaired inside the same batch window instead of invalidating two markers
   that were already refreshed. This is the sequencing that produced #580's
   second pair of refreshes.
3. Anything the second pass raises that is not a Blocker becomes a delta
   follow-up issue under the delivery contract.
4. A Blocker at the second pass is repaired and re-read. It opens a new round,
   with its own two passes, and the pull request records it. A budget that
   could suppress a Blocker would be a blindfold, not a budget.
5. **Step 0 is outside the budget.** The rule's single owner is the step 0
   section of `arch-review`'s own skill, which states that it runs in every
   dispatch at every tier; this line records why nothing here touches it — 86
   findings over 66 rounds is the best yield in the chain.
6. `ai_review_threads.sh list` still gates on zero open threads, unchanged.
7. Green CI and `cargo run -p quantick-guards -- --report` are untouched.

`high` and `max` keep an unbounded round count: the tiers exist to buy depth on
the work that warrants it, and the measurement that justifies this budget was
taken at `medium`.

## 5. The escaped-defect risk being accepted

**Named exactly.** A defect introduced by a repair batch and visible only to a
reader who re-read the whole diff a third time. Under the budget that reader
does not run.

**What bounds it.** Every diff still gets two full readings per review, the
second at the final head, so a repair is never unread — it is read once instead
of twice. The Blocker exception in rule 4 keeps the severity that matters
outside the budget. Step 0, the chain's only measured catcher, runs at every
pass. The zero-open-threads gate, `mission_ship_gate.sh`'s literal
reconciliation and full-head CI are all unchanged.

**What does not bound it.** Nothing in the measurement proves a third pass has
never caught anything — only that the passes this campaign can see were
re-stamps, and that the one round-three finding anyone recorded was a Consider.
`ranking.md`'s `delivery-review` row is the standing warning against reading an
unmeasured rate as a zero, and it applies to this row too: the honest claim is
*not measured to catch*, not *proven not to catch*.

**The ledger records it.** Protocol section 6's escaped-defect ledger is the
place any defect later traced to work shipped under this budget is written, in
L3's entry, whatever the verdict was.

## 6. The reading

Filled in at the draft pull request from this mission's own record. See
`experiments.json`, entry `L3`.

# Where the development loop's tokens and wall clock actually go

Campaign [#563](https://github.com/milocaetano/quantick/issues/563), task T3,
issue [#566](https://github.com/milocaetano/quantick/issues/566). It follows
[#564](https://github.com/milocaetano/quantick/issues/564)'s harness and
[#565](https://github.com/milocaetano/quantick/issues/565)'s baseline and uses
their data; it does not re-derive them.

Method: [`docs/quality/mission-cost/method.md`](../mission-cost/method.md).
Two facts about this report's standing with respect to that method, stated
before any number:

- **This is not a method verdict.** The method grades a *mission*, and #565
  proved this repository cannot place a session on a mission — 99.65% of the
  period's tokens were unplaced against a registered ceiling of 20%. This
  report therefore ranks in a unit that needs no attribution, the way
  `opening_frame.py` did, and every document behind it is stamped
  `registered_comparison: false`.
- **The privacy boundary is unchanged.** Everything read from the transcripts
  went through `tools/mission_cost/transcripts.py`, which returns five values
  per line and drops the rest inside the reader. No transcript content was
  read, quoted or committed (D3).

Decisions carried in and not reopened: **D7** — tokens are the primary axis,
wall clock is the secondary one, parallelism is not a lever. **D8** — process
ceremony is priced, not protected; green CI and
`cargo run -p quantick-guards -- --report` are not negotiable.

## The unit, and why the ranking works at all

One transcript file is one **agent context**: everything one agent held while
it worked, whether it was a main thread or a dispatched subagent. It needs no
attribution to a branch, so it is countable where a mission is not.

What a context costs is not proportional to the work it does. Every request
re-reads the whole context, so the standing prompt grows as the agent works and
each new turn is billed against everything the agent has already seen. Fitting
`billable = frame·N + slope·N²` separately over each population — 560 contexts
and 48,772 requests in all — gives R² 0.966 for subagents against 0.809 for the
linear model, and R² 0.994 against 0.883 for main threads. **The second term is not a refinement; it is the
majority of the bill.**

The quadratic coefficient is what that costs, stated per turn: 2 × 530.98 =
about **1,062 tokens added to the standing prompt by each turn, paid again by
every turn after it**. The raw curve corroborates it — a subagent request
carries a mean of 42,178 cache-read tokens across the first ten requests of its
context, against a period-wide mean of 180,922.

**Two populations, two laws, and the ranking needs both.** A main thread carries
a larger standing frame and a shallower slope than a dispatched agent. Nothing
below compares one population's cost against the other's law without saying so:
the generated lever table carries a *Cost law* column for exactly that reason,
after this report's first version got it wrong and #572's architecture review
caught it.

## Figures

Everything load-bearing below is generated from the committed JSON beside this
file, not retyped out of it. Regenerate with:

```sh
python tools/mission_cost/shape.py table --block figures \
  --shape docs/quality/velocity/shape.json \
  --ceremony docs/quality/velocity/ceremony.json
python tools/mission_cost/shape.py table --block levers \
  --shape docs/quality/velocity/shape.json
```

`test_shape.ReportFigures` fails if either generated block and that JSON ever
disagree. Both are regenerated, never edited by hand.

<!-- shape-figures:v1 -->

| Figure | Value |
| --- | ---: |
| Agent contexts read | 560 |
| Requests | 48,772 |
| Billable tokens | 9,461,418,611 |
| Subagent contexts | 520 (39,911 requests, 7,220,773,840 tokens) |
| Subagent cost law, tokens | 64,224·N + 530.98·N² (R² 0.9664, linear-only R² 0.8089) |
| Subagent standing frame / accumulation | 35.4% / 64.6% |
| Main contexts | 40 (8,861 requests, 2,240,644,771 tokens) |
| Main cost law, tokens | 150,062·N + 233.14·N² (R² 0.9937, linear-only R² 0.8826) |
| Main standing frame / accumulation | 56.8% / 43.2% |
| **All contexts, standing frame / accumulation** | **40.6% / 59.4%** |
| Mean cache reads per request over a subagent context's first 10 requests | 42,178 |
| Subagent contexts over 60 requests | 196 of 520, holding 91.3% |
| Subagent contexts over 100 requests | 106 of 520, holding 80.4% |
| Subagent contexts over 150 requests | 58 of 520, holding 69.7% |
| Top 10% of subagent contexts (52) hold | 68.6% |
| Top 25% of subagent contexts (130) hold | 84.5% |
| Top 50% of subagent contexts (260) hold | 95.5% |
| Missions read for ceremony | 87 |
| arch-review / ai-review / delivery-review durable reports | 112 / 114 / 57 |
| step-0 findings over reported rounds | 86 over 66 (35 empty) |
| AI-review threads, of which open | 75 / 1 |
| Commits before / after the pull request existed | 248 / 334 (57.4% repair) |
| Pull requests whose ceremony facts arrived short | not recorded — this reading predates the check, so completeness is unknown |

<!-- end shape-figures:v1 -->

Window `2026-09-11T15:16:05Z .. 2026-09-21T06:20:00Z`, closed at the instant
checkpoint 4 dispatched this task. Digest
`c5e51be429b2e852bdb2c2422e98e5a00e96737e903ea002312328c50b9d3135`.

**The window cuts requests, not files.** The transcript directory is live and
append-only, so a context still running when a reading is taken keeps growing
afterwards; keeping or dropping whole files either way makes the reading
unrepeatable. Counting only the requests inside the window makes it stable, and
two runs of the command above over this window are byte-identical even while a
session is open.

The totals agree with #565's independently taken baseline to within one point
on the main-thread share — 23.7% here against 23.6% there — which is the
cross-check that this reading and that one saw the same machine. The token
totals differ by 2.1%, which is the extra window this reading carries.

## The ranking — tokens

Ordered by measured share of the period's 9.46 B billable tokens. The phase
column answers #566's second criterion; "both" means the sink is a property of
how any context is run, main thread or subagent alike.

| # | Sink | Share of tokens | Phase | Thread | What would have to change |
| --- | --- | ---: | --- | --- | --- |
| 1 | **Context accumulation** — every request re-reading everything its own agent already did | **59.4%** (5.70 B) | all phases | both; 82% of it subagent | Cap an agent context and hand off to a fresh one. Capping at 80 requests, each piece running as a fresh dispatch on the subagent law and 8 requests charged per *extra* context, models at **−45.9%**. |
| 2 | **The standing frame** — system prompt, tool definitions, skill catalogue, always-loaded instructions, paid on every request | **40.6%** (3.89 B) | all phases | both; 56.8% of main-thread cost | Make the frame smaller. Median opening frame is **58,237 tokens** (p25 57,004, p75 59,978, n = 40). Of that, the installed skill catalogue is ~47 KB of frontmatter across **158 skills, 12 of them this repository's** — about 11,800 tokens. `CLAUDE.md` + `AGENTS.md` are 13,062 bytes, ~3,300 tokens. Taking 10,000 tokens off every request is **−5.1%**. |
| 3 | **Phase two — review rounds and the repairs they cause** | **75.1% and 81.8%** of the two missions measured exactly | reviews, repair, CI watch | subagent (campaign children) or main thread (trader-launched) | Fewer rounds. 20% fewer requests overall models at **−29.5%** alone and **−62.0%** stacked on ranks 1 and 2. |
| 4 | **Phase one — ledger, questions, grounding, implementation, draft PR** | **24.9% and 18.2%** of the same two missions | mission setup, implementation | main thread for a trader-launched mission; already subagent for a campaign child | See *Candidate #570* below. It is the cheapest quarter of a mission because it runs at the bottom of the accumulation curve. |
| 5 | **Campaign coordination** — checkpoints, charter edits, board writes, child briefs | **9.5%** on top of the children it dispatched | coordination | main thread | The coordinator context spent 20,175,810 tokens over 152 requests while dispatching three children whose own cost was 212,730,874. It is real but it is not where the money is. |
| 6 | **Guard ratchets and hook gates** | **not measurable against 9.46 B** | any code change | neither | Nothing. See *Guards and hooks*. |
| 7 | **CI** | **0 tokens** | CI watch | neither | Nothing on this axis. CI costs wall clock only. |

Ranks 1 and 2 are the same 9.46 B split two ways, not two different pots: rank
1 is the part of the bill that scales with how long an agent runs and rank 2 is
the part that does not. Rank 3 and rank 4 are that same money again, split by
*when* it was spent. The ranking is deliberately presented twice because the
two cuts imply different fixes, and #566 asks for both.

### Rank 1 in detail: the concentration

Long contexts are not the common case; they are the expensive case. The
length bands and the concentration rows are in the generated figures block
above: **196 of 520 subagent contexts run past 60 requests and hold 91.3% of
subagent tokens; 58 run past 150 and hold 69.7%.** The median subagent context
is 42 requests and the median main thread is 137.5.

Divide the subagent law by `N` and the same fact reads as a price per request:
at 20 requests the average request costs **74,843** tokens, at 80 **106,702**,
at 160 **149,180**, at 320 **234,137** and at 450 **303,164**. An agent that
works four times as long does not cost four times as much; it costs about
eleven times as much. Those five numbers are
`(64,223.5 × N + 530.98 × N²) / N` at the coefficients the figures block
publishes, so a reader can check them without rerunning anything.

### The counterfactual, and how the 60% is reached

Arithmetic over the fitted coefficients, not an experiment — turning it into
something a real mission proves is [#567](https://github.com/milocaetano/quantick/issues/567)'s
job, not this one's. Generated from the same committed JSON as the figures
block, and every row states the cost law it used.

<!-- shape-levers:v1 -->

| Levers applied | Cost law | Modelled tokens | Change |
| --- | --- | ---: | ---: |
| Baseline (modelled) | each population's own | 9,585,006,964 | — |
| **L1** — cap every context at 80 requests, handing off to a fresh dispatch | subagent | 5,181,488,395 | **-45.9%** |
| **L1+L2** — the same, and 10,000 tokens off the standing frame | subagent | 4,663,288,395 | **-51.3%** |
| **L1+L3** — the same as L1, and 20% fewer requests | subagent | 4,050,176,840 | **-57.7%** |
| **L1+L2+L3** — all three together | subagent | 3,638,000,840 | **-62.0%** |
| **L2** — 10,000 tokens off the standing frame, no cap | each population's own | 9,097,286,964 | **-5.1%** |
| **L3** — 20% fewer requests, no cap | each population's own | 6,757,272,499 | **-29.5%** |

Handoff charged at 8 requests per *extra* context, once per handoff rather than once per piece.

<!-- end shape-levers:v1 -->

Three assumptions are stated in that table rather than buried: the cap is 80
requests, the frame trim is 10,000 tokens per request, and L3 supposes 20%
fewer requests. They live in `shape.py` as `POLICY_CAP`, `FRAME_TRIM_TOKENS`
and `REQUEST_SCALE`, so a reader who disagrees with one can redo the whole
table with another number.

**Why the capped rows use the subagent law.** Capping a context *means* handing
off to a fresh dispatch, and a fresh dispatch is a subagent by construction:
frame 64,224 rather than the main thread's 150,062. Pricing the capped rows any
other way would model a policy nobody is proposing. The rows that do not cap —
L2 and L3 — keep each population on its own law, because trimming the frame or
asking for fewer requests does not move where an agent runs.

The campaign's *about 60%* is reachable, and it needs all three levers. No
single one of them gets there, and the largest one is structural rather than
editorial: it is a rule about how long an agent is allowed to run, not a
smaller file.

Within the subagent population alone — no law substitution, the view
`shape.json` carries per population — caps between 40 and 120 requests are all
within four points of each other (−45.9%, −44.6%, −42.4%, −37.5%), so the exact
number is not delicate. Below 40 the handoff charge starts eating the saving.

## The ranking — wall clock

The secondary axis (D7). Nothing here overrides the token ranking, and where
the two conflict the token ranking wins.

| # | Sink | Share | Costs tokens? |
| --- | --- | ---: | --- |
| 1 | **Nobody working** — the span between missions, overnight, between a child finishing and its coordinator acting | **70% of the period's span** | no |
| 2 | **Agent time** | 142.6 h of agent-seconds over a 228.1 h span, at 2.05× concurrency, so **30% of the span** is the loop actually running | yes — this is the token ranking |
| 3 | **CI** | `ci_wall_seconds` median **1,506 s** per mission over a median of 3 runs — **12.7%** of the median `pr_open_seconds` of 11,832 | no |
| 4 | **Coordinator turnaround** | On #569 the mission agent stopped at 01:51:07Z and the merge landed at 03:12:41Z: **81 minutes, 71% of that pull request's open life**, with nothing running | no |

### Agent time and waiting time, separated

#566's third criterion. Period scale. These four are #565's registered figures,
quoted rather than re-derived so that the campaign carries one wall-clock
reading and not two; this reading's own agent-seconds over its own window come
to 146.7 h, which corroborates them.

| | |
| --- | ---: |
| Span, first to last request | 228.1 h |
| Agent-seconds (concurrency counted twice, the quantity the loop asks for) | 142.6 h |
| Distinct elapsed time (the union; concurrency counted once) | 69.5 h |
| **Waiting — span minus elapsed** | **158.6 h, 70%** |

Per mission, on the two this report can attribute exactly:

| | #569 | #571 |
| --- | ---: | ---: |
| Agent active | 55.8 min | 76.2 min |
| Pull request open | 114.8 min | 124.8 min |
| Agent share of the open life | **48.6%** | **61.1%** |

So roughly half to three fifths of a mission's open life is an agent working,
about an eighth is CI, and the rest is turnaround. **A token fix does not
shorten the waiting, and shortening the waiting saves no tokens.** That is the
whole reason #566 asked for the two axes separately.

## Phase attribution

Two missions in this campaign can be attributed **exactly**, without the window
inference the method warns about in E1: #569 and #571 each ran as a single
dispatched agent context under one coordinator session, and their durable
review reports timestamp the phase boundaries from the public record.

Each window below is *the round plus the repairs it caused*, because the
repairs were committed inside it — the commit timestamps on both branches make
that unambiguous, and separating a review from the work it provoked would be an
arbitrary cut rather than a measurement.

### #569 `feat/mission-cost-harness` — 365 requests, 91,229,177 tokens

| Window | Requests | Tokens | Share | Wall |
| --- | ---: | ---: | ---: | ---: |
| Steps 1–7, ledger to draft pull request | 154 | 22,696,389 | **24.9%** | 23.9 min |
| Draft PR → arch-review report, incl. step 0 and its repairs | 135 | 38,564,504 | **42.3%** | 19.6 min |
| arch-review report → ai-review report | 1 | 365,991 | 0.4% | 0.2 min |
| ai-review → delivery-review, incl. its repairs | 22 | 8,311,845 | 9.1% | 3.9 min |
| delivery-review → mission completion, incl. CI watch and readiness | 42 | 16,790,089 | 18.4% | 9.5 min |
| After completion | 11 | 4,500,359 | 4.9% | — |
| **Phase one / phase two** | | | **24.9% / 75.1%** | |

### #571 `docs/mission-cost-baseline` — 449 requests, 121,501,697 tokens

| Window | Requests | Tokens | Share | Wall |
| --- | ---: | ---: | ---: | ---: |
| Steps 1–7, ledger to draft pull request | 152 | 22,107,024 | **18.2%** | 53.1 min |
| Draft PR → arch round 1 (includes post-draft implementation) | 178 | 51,764,751 | **42.6%** | 45.1 min |
| → ai round 1 | 12 | 4,235,468 | 3.5% | 18.4 min |
| → arch round 2 and its repairs | 57 | 21,814,359 | 18.0% | 16.2 min |
| → ai round 2 | 12 | 4,982,946 | 4.1% | 10.0 min |
| → ai round 3 | 11 | 4,696,530 | 3.9% | 8.6 min |
| → arch round 3 | 8 | 3,473,132 | 2.9% | 9.8 min |
| → delivery-review | 8 | 3,519,404 | 2.9% | 4.4 min |
| → mission completion | 5 | 2,221,308 | 1.8% | 10.2 min |
| After completion | 6 | 2,686,775 | 2.2% | — |
| **Phase one / phase two** | | | **18.2% / 81.8%** | |

Two things this pair settles.

**A mission costs about 106 M tokens.** #569 and #571 together cost
212,730,874 over two missions, 106.4 M each. #565's upper bracket — the
period's tokens divided by 87 missions — was 106,558,954. Two independent
routes to the same number within 0.2%, one of them exact. **The bracket was not
loose; it was the answer.** This is also the first per-mission token figure in
the campaign that is not a bracket, and it is 220 times larger than the figure
the campaign has been recording for itself in checkpoints (#569 "413,719
subagent tokens"), which counts uncached input plus output only — 217,599 of
#569's 91,229,177 — and therefore omits the 95% of the bill that is cache
reads. **Checkpoint 4's `campaign_own_cost_so_far` understates the campaign's
own spend by roughly two orders of magnitude.**

**Phase two is where a mission's money goes**, at 75.1% and 81.8%. Not because
review is inherently expensive, but because review runs at the *top* of the
accumulation curve: #569's first 154 requests cost 147,379 each and its last
211 cost 324,800 each, for the same agent doing comparable amounts of work.

## Pricing the chain

D8's core question, and the honest answer starts with a limitation.

**The durable review reports cannot show what the chain caught.** Across 87
missions the published record holds 112 arch-review, 57 delivery-review and 54
mission-completion reports, and **every one of them carries `verdict=PASS`**,
plus 114 ai-review reports all carrying `verdict=COMPLETE`. Not one durable
report in the window records a refusal. That is not evidence the chain is
useless; it is a consequence of publishing the report only once the round has
converged. The catches have to be counted from evidence created at the moment
of the finding, which leaves three sources: the step-0 counts each arch-review
report states about itself, the AI-review threads GitHub timestamps when they
are opened, and the commits a branch made *after* its pull request already
existed, which are repairs by construction.

| Ceremony | Runs | What it caught | Token cost, measured | Cost per catch |
| --- | ---: | --- | ---: | ---: |
| **step 0**, the bug pass inside `arch-review` | 66 rounds that stated a count | **86 findings**; 35 rounds found nothing | inside the arch window; not separable | best in the chain, ~1.3 findings per round that reports |
| **`arch-review`** shape pass | 112 reports | not machine-countable; ≥ 39 findings by marker, the rest in prose | #569 one window of 38,564,504 (42.3%); #571 rounds 2 and 3 at 21,814,359 and 3,473,132 | **~3.5–38 M per round** |
| **`ai-review`** | 114 reports | **75 threads over 87 missions** (0.86 per mission, median 0), 1 still open, no Blocker marker in any report | #571's three rounds at 4,235,468 + 4,982,946 + 4,696,530 = **13,914,944, 11.5% of the mission** | **~14 M per round, ~12–16 M per catch** |
| **`delivery-review`** | 57 reports | **zero refusals on the published record** | #569 **8,311,845 (9.1%)**, #571 **3,519,404 (2.9%)** | **undefined — 57 runs, 0 gating catches** |
| **repair rounds** | 334 repair commits against 248 implementation commits | this is the *effect* of the chain, not a separate catcher | the windows above | **57.4% of all commits on this repository are made after the pull request exists** |
| **guard ratchets and hooks** | every write and every Bash call | unmeasured; the hooks keep no log | `cargo run -p quantick-guards -- --report` runs in **6.1 s** and the hooks are silent on pass, so **~0 tokens** | not a sink at any price |

### What this says, ceremony by ceremony

**step 0 stays.** 86 findings in 66 reported rounds is the highest yield in the
chain by a wide margin, and 35 empty rounds is the right shape for a bug pass —
a pass that always finds something is not calibrated. It is also the ceremony
`small` and `medium` share, so it is not what the tier ladder is buying.

**`ai-review` earns its place but not its round count.** 75 threads across 87
missions is a real catch rate, and one thread still open is a real gate. What
the measurement cannot justify is *three rounds*: #571's rounds 2 and 3 cost
9,679,476 tokens between them, 8.0% of that mission, and the campaign's own
summary records that round's findings as Considers. One round plus thread
closure keeps the catch and drops most of the cost.

**`delivery-review` is named removable at `medium`.** 57 runs, no refusal ever
published, 8.3 M and 3.5 M tokens on the two missions measured exactly, and it
is the one ceremony `medium` has that `small` does not. *Escaped-defect risk,
stated:* delivery-review is the only reader that reconciles the shipped work
against the retained request line by line, so removing it raises the chance a
mission ships having quietly dropped an ask. Two things bound that risk. The
final verifier, `mission_ship_gate.sh`, already performs the literal
reconciliation against *What done means* at completion and is not being
touched. And the failure mode it guards is a missing requirement, which is
visible to the trader on the pull request, rather than a defect that reaches
the chart. The recommendation is therefore to give `medium` the same exemption
`small` already has, keeping it at `high` and `max` where diffs are large
enough that a dropped ask is hard to see.

**Campaign coordination is cheap and stays.** 20,175,810 tokens over 152
requests against 212,730,874 of children is 9.5%, and it is what keeps the
children from being scheduled ahead of their evidence (D4). The one change
worth making is bookkeeping rather than ceremony: checkpoints should record the
billable figure, not the uncached-plus-output figure, for the reason given
above.

**Guards and hooks are not a sink and must not be cut.** They cost 6.1 s to
run, they emit nothing into a context when they pass, and the PreToolUse and
PostToolUse entries in `.claude/settings.json` have 10-second timeouts and are
silent on success. Against 9.47 B tokens their cost does not register. C8 asks
which parts of the process earn their cost; these earn it trivially, because
they have almost none. Removing them would save nothing measurable and would
give up the size, context and architecture ratchets in exchange.

## Tiers

Is `medium` earning its extra ceremony over `small` on this repository's real
diffs? The tiers differ in the interrogation budget, the criteria table, the
bug-pass level and whether `delivery-review` runs at all.

| Tier | Missions | Churn (median) | AI threads per mission | step-0 findings per mission | Repair commits per mission | delivery-review runs |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `small` | 15 | 285 | 0.13 | 0.80 | 1.27 | 0 |
| `medium` | 22 | 1,476 | 1.45 | 1.23 | 4.18 | 23 |
| `high` | 15 | 3,240 | 0.73 | 2.33 | 4.73 | 12 |
| unrecorded | 35 | 1,977 | 0.86 | 0.34 | 4.34 | 22 |

Normalised by the thing the tier is supposed to track — how much changed:

| Tier | AI threads per 1,000 changed lines | step-0 findings per 1,000 changed lines |
| --- | ---: | ---: |
| `small` | 0.47 | 2.81 |
| `medium` | 0.99 | 0.83 |
| `high` | **0.23** | 0.72 |

**The ladder does not buy catches.** `high` runs the deepest ceremony on the
largest diffs and finds the fewest findings per line changed, on either
counter. `small`, whose ceremony is the thinnest, has the best step-0 yield per
line. The gap between `small` and `medium` in raw counts is almost entirely a
gap in diff size: five times the churn, eleven times the AI threads, three
times the repair commits.

Two honest confounds, because n is 15 to 22 per tier and this is not a
randomised assignment. A tier is chosen by the agent that runs the mission, so
`high` may be chosen precisely for work being done carefully, and careful work
has fewer defects to find. And `unrecorded` is 35 missions of the same
population, mostly older, which weakens any trend read across the ladder.

What survives both confounds is the narrower claim, and it is the one D8 asked
for: **the single ceremony that `medium` adds over `small` is
`delivery-review`, and `delivery-review` has 57 runs and no recorded catch.**
That is a measurement, not a feeling.

## Candidate #570 — priced, and closed

[#570](https://github.com/milocaetano/quantick/issues/570) proposes extending
#557's PR-as-context rule backwards to cover steps 1 through 7, which
[#557](https://github.com/milocaetano/quantick/pull/557) left in the main
thread. #566's first comment asks for it to be scheduled on a number or closed
unimplemented. Here is the number.

**Phase one is 24.9% and 18.2% of the two missions measured exactly, and it is
the cheapest part of them.** #569's first 154 requests averaged 147,379 tokens
against the mission's own average of 249,943, because phase one runs at the
bottom of the accumulation curve. Moving the cheapest quarter of a mission is
the smallest available move.

**#565's weakening of its premise holds.** The main thread is 23.7% of the
period's tokens on 18.2% of its requests. Even abolishing main-thread cost
entirely — not moving it, abolishing it — caps the saving at 23.7%, and phase
one is a fraction of what a main thread does, the rest being coordination,
conversation and work that is not a mission at all.

**A tighter bound.** Main-thread requests cost 252,866 each and subagent
requests 180,922. If every main-thread request had been issued from a
subagent-grade context, the period would have saved 8,861 × 71,944 = 638 M
tokens, **6.7% of 9.46 B** — and that is the ceiling for moving *all*
main-thread work, of which #570 targets one phase.

**And it is already subsumed.** A campaign child runs phase one in a fresh
dispatched context today: #569 and #571 both did, which is why their phase-one
requests are cheap. The remaining case is a mission the trader launches inside
a session that has already run long — and that case is exactly rank 1, which
fixes it for phase one and phase two together rather than for phase one alone.

**Verdict: close #570 unimplemented, superseded by the rank 1 fix child.** Its
argument was right about direction and wrong about size, and the structural fix
that covers it is worth more than the phase-specific one.

## The trade-off frontier

D7 asks for the frontier to be named with numbers wherever the axes oppose,
rather than for whichever reads better.

| Lever | Tokens | Wall clock | Verdict under D7 |
| --- | ---: | --- | --- |
| Cap the context and hand off (L1) | **−45.9%** | **worse.** 8 requests per extra context, ~9% more requests on a 365-request mission, and each handoff serialises: the new context cannot start until the old one has written its brief | Take it. The token axis wins. |
| Trim the standing frame (L2) | −5.1% at −10,000/request | neutral | Take it. It is free on the second axis and it composes: after L1 the frame is the larger term. |
| Fewer review and repair rounds (L3) | −29.5% alone, −62.0% stacked | **better** — fewer rounds is less CI and less turnaround | Take it. The only lever that wins on both axes, as #566's second comment predicted. |
| More concurrency | slightly **worse** — each concurrent agent loads the pull request independently | better | **Forbidden by D7.** Not proposed. |
| Faster CI | 0 | better, bounded by 12.7% of `pr_open` | Not a token lever; #552 and #553 already took most of it, and #565 showed runs per mission rose to meet it. |
| Cut the coordinator's turnaround | 0 | better — 71% of #569's open life | Not a token lever. Listed so it is not mistaken for one. |

## Unmeasured

Sinks this instrument cannot see, named rather than omitted (#566's fifth
criterion).

1. **Which agent ran which ceremony**, for any mission but this campaign's two.
   Naming a subagent's job means reading the dispatch that created it, which is
   transcript content and forbidden by D3. The two missions here were resolved
   from public report timestamps instead, and that trick only works when one
   mission owns one context.
2. **`arch-review`'s shape-pass findings.** The durable report publishes the
   converged state, so a finding raised and fixed inside a round leaves no
   countable trace. The marker count of 39 is a floor, not a measurement.
3. **The composition of the 58,237-token standing frame** beyond this
   repository's own 13,062 bytes and the ~47 KB skill catalogue. Tool
   definitions, MCP schemas and the system prompt are host-side, and nothing in
   the five fields separates them.
4. **What the hook gates caught.** `guardrails.sh` keeps no log of a denial, so
   the deny side of the PreToolUse gates has no count at all. Its cost is ~0
   either way, which is why this does not block the verdict on them.
5. **Codex sessions.** `$mission` under Codex writes nothing to the Claude
   projects directory. Any mission run there is invisible to every number in
   this campaign, and the outside-eight campaign is known to have used it.
6. **Compaction.** A compacted or resumed context reads as one long context,
   and the five fields cannot tell that from an agent that simply endured. The
   fitted slope is therefore a *lower* bound wherever compaction happened.
7. **Retries and abandoned turns** are inside `usage` by construction (E7).
   They are the right quantity for a cost campaign and the wrong one for a
   productivity claim; this report makes no productivity claim.
8. **Money.** The four token kinds are priced differently and the harness
   applies none (E8). "Billable tokens" orders the sinks; it does not cost them
   in currency, and a fix that trades cache reads for output would look free
   here and would not be.
9. **The trader's own reading and thinking time**, which is inside the 70%
   this report calls waiting and is not idle at all.
10. **Tool-result sidecar files** an agent writes and re-reads. Their re-read
    cost is inside the accumulation term and cannot be separated from ordinary
    growth.

## Prose drift

#565 deferred a mechanical check of a report's prose figures against its
committed JSON, and routed it here. That drift class recurred three times on
#571's branch, always in prose and never in a committed file, and each
occurrence cost a review round — between 3.5 M and 22 M tokens by the figures
above.

**Decision: build the generator, not the checker.** A checker parses prose and
goes quiet the day the prose is reshaped, which is exactly the "stale parser is
worse than none" failure the routing warned about. A generator inverts that: if
the document and the data disagree, the generator's test fails loudly and names
the command that fixes it.

It is not filed as a fix child, because it is small enough to discharge in
place and C8 asks for proportion. It is implemented here: this report's
*Figures* and *Levers* blocks are both written by `shape.py table` out of
`shape.json` and `ceremony.json`, and `test_shape.ReportFigures` fails if
either ever disagrees with them.

**It earned its place immediately, and the evidence is this report's own first
version.** The counterfactual table was hand-written beside the document rather
than generated out of it, and one row silently priced a capped population on
the subagent law while the baseline it was compared against used each
population's own — a 6-point error in the campaign's headline number, in the
row fix child #573 is scheduled on. #572's architecture review caught it as a
Blocker. The durable repair was not a sharper reader: it was to move that table
inside a generated block and give every row a *Cost law* column, which is what
`shape-levers:v1` is.

Numbers that are still *not* in either block — the phase tables, which come
from per-window sums over two named transcripts — remain hand-written. That is
the residual risk, stated rather than hidden, and it is smaller than what was
repaired because those sums are per-mission evidence rather than the headline
lever.

## Fix children

Created from the top of this ranking under D4. None of them is implemented
here.

| Rank | Child | What it does | Modelled saving |
| --- | --- | --- | ---: |
| 1 | [**#573**](https://github.com/milocaetano/quantick/issues/573) | Cap an agent context and hand off to a fresh one; supersedes #570 | −45.9% |
| 2 | [**#574**](https://github.com/milocaetano/quantick/issues/574) | Shrink the standing frame, starting with a skill catalogue in which 146 of 158 skills belong to other projects | −5.1% at −10,000/request |
| 3 | [**#575**](https://github.com/milocaetano/quantick/issues/575) | Cut the round count: one `ai-review` round plus thread closure, `delivery-review` exempt at `medium`, step 0 untouched | −29.5% alone |
| — | [**#576**](https://github.com/milocaetano/quantick/issues/576) | Let the registry name a transcript, not only a session, so the other three can be graded at all | enables the rest |

#576 is not a saving; it is the reason the others can be measured. The registry's
unit is the session, and all three of this campaign's children are subagents of
**one** coordinator session, so declaring a session id — D5's remedy — would
make each child claim its siblings. The exact unit is the transcript file, and
the schema cannot hold one. That is why this mission did not edit
`missions.json`: the only entry it could have written would have been wrong.

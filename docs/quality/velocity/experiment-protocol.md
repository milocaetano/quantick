# The velocity experiment protocol

Registered for campaign [#563](https://github.com/milocaetano/quantick/issues/563),
task [#567](https://github.com/milocaetano/quantick/issues/567). It is the rule
every change to the development loop is graded against before it is allowed to
stay on `campaign/mission-velocity`, and it is written before any of the three
ranked levers — [#573](https://github.com/milocaetano/quantick/issues/573),
[#574](https://github.com/milocaetano/quantick/issues/574),
[#575](https://github.com/milocaetano/quantick/issues/575) — has taken a
reading, for the same reason `method.md` was written before the first baseline:
a rule chosen after the numbers are in can always be moved until the answer
comes out.

`docs/quality/mission-cost/method.md` owns measurement. This document owns
**judgement**: what a reading has to be before it counts, and what happens to a
change whose reading never arrives.

**Where this file lives, and why it is not under `docs/workflow/`.** That tree
is inside the context ratchet's tracked set, so every byte there is re-read on
every turn of every session — the exact quantity this campaign exists to cut.
A protocol that cost 15 kB of standing frame would be spending the saving it
protects. It lives beside the campaign's other evidence, and the instruction
trees pay only for pointers.

## 1. What this decides, and what it refuses to decide

**Decides:** whether one change to the development loop reduced what a mission
costs in billable tokens, by enough to be worth keeping, without breaking the
two things the campaign will not trade.

**Refuses:** whether the campaign reached its target. That is
[#568](https://github.com/milocaetano/quantick/issues/568)'s job, measured end
to end on a real mission. A `proven` verdict here is a *component*, not a
result, and components **multiply** — three levers at −45.9%, −5.1% and −29.5%
stack to −62.0%, not to −80.5%. Anyone adding them has misread this document.

**Also refuses:** any claim about quality. The harness measures cost (method
E11). Section 6 states the floor quality has to clear; it does not pretend to
grade it.

## 2. The unit, and why it is not the mission total

The campaign's decision D9: **the unit of cost is the agent context, not the
mission.** A context's billable cost is quadratic in its turn count, because
every request re-reads everything the context has already accumulated:

| Population | Law | R² | Linear-only R² |
| --- | --- | ---: | ---: |
| Subagent | `64,224·N + 530.98·N²` | 0.9664 | 0.8089 |
| Main thread | `150,062·N + 233.14·N²` | 0.9937 | 0.8826 |

Fitted in [#566](https://github.com/milocaetano/quantick/issues/566) over 560
contexts and 48,772 requests, published in `docs/quality/velocity/ranking.md`
and generated from `docs/quality/velocity/shape.json`. Every counterfactual
below is priced on these coefficients **as fitted**. Re-fitting the law on the
graded mission and then pricing the lever against the re-fit is circular, and it
is refused: the law is an input to grading, never an output of it.

A mission's cost is the sum over the contexts it owns. Its **shape** — how many
contexts, how long each one ran, what the standing frame cost per request — is
what a lever moves. The total is what the shape produces.

## 3. The small-n problem, and how this protocol gets out of it

The registry holds 87 missions before the campaign and will hold a handful
after it. The registered group rule (`method.md` section 5) needs
`MIN_GROUP_N = 5` in each group; the campaign will never have five *after*
missions per lever, so a group comparison returns `inconclusive` for every
lever, forever. Lowering that threshold does not help: missions are not
interchangeable, and the dispersion says so out loud — `pr_open_seconds` runs
from a p25 of 2,194 s to a p75 of 26,020 s and a maximum of 185,749 s over the
same 87 missions. Two mission totals differing by 30% is an ordinary Tuesday.

So this protocol does not compare mission totals. It **substitutes mechanism
for sample**:

> Measure the thing the lever changed, exactly, on the mission it ran on, and
> convert that movement into tokens through a law fitted before the lever
> existed.

A mechanism reading does not need replication the way an outcome reading does,
because it is not an estimate of a population parameter — it is a measurement
of a quantity the lever directly controls, on the mission that ran it. n = 1 is
enough for a mechanism reading. n = 1 is never enough for an outcome reading,
and this protocol never takes one.

### The three admissible proof forms

| Form | The lever… | The saving is measured as |
| --- | --- | --- |
| `reshape` | rearranges the same requests across a different number of contexts | the mission's own observed context vector, priced on the registered law, against the same requests arranged as before |
| `frame` | cuts the standing tokens every request in a context pays | the measured fall in the opening frame, times the mission's own request count, through the registered law |
| `removal` | deletes a set of agent contexts from the loop | the mission's observed vector with the removed contexts **added back at the length they ran to in reference missions**, priced on the registered law |

All three end in the same place — a list of contexts priced on the registered
law — and that uniformity is deliberate. The lever is graded on the shape it
changed, never on a comparison between two mission totals.

`reshape` and `frame` are exact at n = 1: both counterfactuals are built
entirely from the graded mission's own numbers and borrow nothing.

`removal` is the hard one, and it is the one a round-cut needs, because the
length of what was removed has to come from missions that still ran it. So:

- it needs `MIN_REFERENCE_N = 3` reference missions with the removed contexts
  measured, each recording how many contexts were removed and how many requests
  they held;
- it is priced on the **smallest** of them, not the median and not the mean.
  The range matters: `delivery-review` measured 9.1% of one mission and 2.9% of
  another — a factor of three — and a lever that cuts it has to beat the 2.9%;
- and its `K1` carries a second, ordinal condition: **the graded mission's total
  request count must fall below every reference mission's.** Ordinal, because an
  ordering survives the dispersion that a magnitude comparison drowns in. This
  is the condition that rejects a round-cut whose work simply reappeared
  somewhere else: the repair turns land in the same total, and the total does
  not fall.

### The fourth form, refused by name

`totals` — "this mission cost less than the last one, therefore the change
worked" — is **not an admissible proof form.** `experiment.py` refuses a ledger
entry that declares it, and no verdict path can reach `proven` without one of
the three forms above. This is the refusal that stops the campaign certifying
noise, and it is executable rather than advisory.

### The conflict of interest, stated

#575 proposes cutting review rounds, and #567 — this document — is the rule
#575 is judged by. Both are this campaign's work. The protections written
against that, deliberately, are the ones that bite a round-cut hardest:

- A round-cut is a `removal`, so what it removed is measured over at least three
  reference missions and priced at the **smallest** of them.
- Its declared mechanism variable is **the requests the removed contexts held**,
  never the round count. Cutting a round that cost 2% of a mission and calling
  it a 30% saving fails on the numbers, whatever the round count did.
- `K1` requires the graded mission's total request count to fall below every
  reference mission's. A cut that merely moves work — fewer review rounds, more
  repair rounds — is `refuted` there, because the repair turns land in the same
  total.
- A `removal` that is also a reduction in review depth carries section 6's
  honesty triple or the verdict is `void`, not merely unproven.

## 4. Registered constants

Registered here and in `docs/quality/mission-cost/method.md` section 10, before
any reading is taken under them. Changing one invalidates every verdict taken
under the old value; the change is its own pull request and names what it
invalidates, exactly as `method.md` requires of its own thresholds.

| Constant | Value | What it is for |
| --- | ---: | --- |
| `MIN_MECHANISM_CHANGE` | 0.10 | the declared variable must have moved by at least a tenth of its pre-lever value, or the lever did not do what it said |
| `MIN_MODELLED_SAVING` | 0.03 | a modelled saving smaller than this is inside the model's own error and is not worth a probation |
| `MAX_MODEL_RESIDUAL` | 0.25 | if the law mispredicts the graded mission's own total by more than this, the law does not describe this mission and its counterfactual is not evidence |
| `MIN_REFERENCE_N` | 3 | reference missions a `removal` needs before its share is transferable |
| `PROBATION_MISSIONS` | 1 | graded missions a lever gets before a verdict is due |
| `MAX_UNPROVEN_EXTENSIONS` | 1 | how many times an `unproven` verdict may buy one more mission |

`MIN_GROUP_N`, `MIN_RELATIVE_CHANGE` and `MAX_UNPLACED_SHARE` are **not
touched.** They govern a different comparison — group medians over the registry
— and every reading already taken under them stands.

## 5. The decision rule

A lever is `pending` from the moment it is registered. A verdict is computed
from one graded mission's reading, and **both keys must turn**:

- **K1 — the mechanism moved.** The declared variable moved in the declared
  direction by at least `MIN_MECHANISM_CHANGE` of its pre-lever value, measured
  on the graded mission's own record. A lever whose variable did not move did
  not cause anything, whatever the total says. For a `removal` the pre-lever
  value is **derived from the references, never read from the reading** — a
  lever that could write its own starting point could make any ending point
  look like a fall.
- **K2 — the model describes this mission.** The registered law predicts the
  graded mission's measured billable total within `MAX_MODEL_RESIDUAL`, the
  modelled saving is at least `MIN_MODELLED_SAVING`, and the measured total is
  at or below the counterfactual. A law that cannot predict the mission it is
  grading cannot price a counterfactual for it.

| Verdict | Condition |
| --- | --- |
| `void` | the quality floor failed, the honesty triple is missing, the reading rests on window-inferred attribution, or the declared form is `totals`. **Nothing may be concluded from a void reading, in either direction.** |
| `refuted` | K1 fails, or the measured total exceeds the counterfactual |
| `unproven` | K1 holds and K2 does not — typically a residual past `MAX_MODEL_RESIDUAL`, or a `removal` with fewer than `MIN_REFERENCE_N` references |
| `proven` | K1, K2, the quality floor and the honesty requirement all hold |

**Attribution must be exact.** A reading whose transcripts were assigned by
`method.md` section 2 rule 3 — window inference — is `void`. Only `declared`
attribution carries a verdict, which is what makes section 9 load-bearing
rather than tidy.

**The graded mission is a real mission** (charter C4): it delivers real work,
opens a pull request, passes the ordinary review chain and merges. A synthetic
run is not a reading. This is also what stands in for a work-invariance check:
a `reshape` lever that quietly delivered less would not have shipped the work.

**What `proven` means, said plainly.** *The mechanism moved as the lever
declared, and on a law fitted before the lever existed that movement is worth at
least the stated share of this mission's bill.* It does not mean the next
mission will be that much cheaper, and it does not mean the campaign is. Those
are #568's questions.

## 6. The quality floor

Charter C5 as amended by decision **D8**. Two tiers, and the difference between
them is the whole point.

**Non-negotiable — a failure here is `void`, and the reading is unusable:**

- **Green CI at the graded mission's final head.** Every registered check.
- **`cargo run -p quantick-guards -- --report` holds or improves** against the
  mission's base. Byte-identical, or a difference each of whose lines is an
  improvement or is explained.

**Priced, not protected.** Review depth and process ceremony are levers, not
fixtures. Reducing them is authorized. What the campaign owes instead is
honesty about the trade, and this protocol makes that owing mechanical: a lever
whose entry declares `reduces_review: true` carries all three of

- `removes` — what stops happening, named exactly;
- `risk` — what it raises in escaped-defect risk, stated as a risk and not as
  a reassurance;
- `justified_by` — the measurement that says it was worth it, cited to a
  committed report.

A missing or empty field is `void`, not `unproven`. A reduction chosen on feel
rather than on measurement is still a failure — that is what the third field
exists to catch, and an entry citing nothing measurable does not have one.

**Reported, and never a pass condition.** Findings per review — `arch-review`
rounds and their step-0 catches, `ai-review` threads, the `delivery-review`
verdict — is recorded on every graded mission so the trade stays visible.
A lever does not fail for raising it or pass for lowering it. It is the number
that lets the trader see, later, what the speed cost him.

**The escaped-defect ledger.** Any defect later traced to work delivered under
a lever is recorded against that lever in its entry. It does not retroactively
change a verdict — a proven saving was still measured — but it is the only
honest record of what the reduction bought and what it cost, and #568 reads it.

## 7. The revert rule

Charter C4: a change that does not prove a saving comes out. Operatively:

1. A lever merges into `campaign/mission-velocity` with verdict `pending` and a
   probation of `PROBATION_MISSIONS` graded missions, recorded in
   `docs/quality/velocity/experiments.json` in the same pull request.
2. When its probation mission completes, the verdict is computed with
   `python tools/mission_cost/experiment.py grade --id <lever>` and written into
   the ledger.
3. **`refuted` or `void` → reverted.** The coordinator reverts the lever's
   commits from the campaign branch by `git revert`, before dispatching the
   next child, and records the revert commit in the ledger entry and in the
   next checkpoint's decision log. A `void` verdict is reverted like a refuted
   one: a change that cannot be graded is not a change that has earned a place.
4. **`unproven` → one extension.** `MAX_UNPROVEN_EXTENSIONS = 1`, so a lever
   gets exactly one more graded mission. A second `unproven` is reverted.
5. **Ungraded at #568 → reverted.** A lever whose probation has not produced a
   verdict by the time the campaign reaches its verification mission is
   reverted before that mission runs. #568's number must not contain a lever
   nobody graded; that is precisely the failure —
   [#556](https://github.com/milocaetano/quantick/issues/556) optimizing a
   frame no model reads — that the campaign exists to stop repeating.
6. Reverting is the coordinator's action. It is not optional, not deferred to
   the trader, and not satisfied by an issue promising to look again.

`experiment.py verify` refuses a ledger in which a lever's probation has lapsed
with no verdict and no recorded revert, so the state cannot be left ambiguous
by being left alone.

## 8. Concurrency, and how concurrent levers are disentangled

Blanket serialization would cost the campaign a mission per lever and is more
than the confounding risk requires. The rule is **disjointness of the mechanism
variable**, because attribution here is to the variable, not to a time window:

| Variable | Moved by | Disjoint from |
| --- | --- | --- |
| `contexts` | a cap-and-handoff lever (#573) | `frame` |
| `frame` | a standing-prompt trim (#574) | `contexts`, `requests` |
| `requests` | a round or phase cut (#575) | `frame` |

- **Disjoint variables may be in probation at the same time**, on different
  graded missions. `frame` is disjoint from both others: trimming the standing
  prompt changes the per-request cost and not the request structure.
- **`contexts` and `requests` are not disjoint** — both enter `N` — and are
  **serialized**. The second lever's pre-lever value is the first lever's
  post-lever state, and its counterfactual is built on that state. Grading it
  against the original baseline would double-count the first lever's saving.
- **Two levers may never be graded on the same mission.** One reading, one
  lever; otherwise neither counterfactual is clean.
- **Stacked claims multiply.** `(1 − a)(1 − b)`, never `a + b`. #566's −62.0%
  stack is a product and is quoted as one.
- While an experiment mission runs, the coordinator dispatches no sibling whose
  context spans it. That keeps the confounding out and, not coincidentally,
  keeps section 9's transcript claim uncontested.

`experiment.py verify` refuses a ledger with two open probations on the same
variable, or two levers naming the same graded branch.

## 9. What a mission records about itself, and when

Decision **D12**, settled here.

The problem, from #576: a mission may now declare its own transcript, and
nothing helps it learn which one it is. #576 recovered its own by sampling file
sizes twice and taking the one that grew — a procedure that needs two live
samples, is ambiguous if two siblings write between them, and cannot be run
once the mission is over. #573 makes it worse: a handed-off context cannot
recover its predecessor's id.

**The `cwd` route stays closed.** Reading `cwd` or `sessionId` off a transcript
line would resolve this outright and is exactly what #564's second acceptance
criterion forbids and what error mode E2 pays for openly. It is not taken here.
It is not the only workable answer, so there is nothing to refer back to the
trader.

### The decision

**A mission records a claim on its own contexts, at three named moments, and
resolves that claim to exact transcript paths before its pull request is ready.
The claim is made of quantities the mission already has: a session id from its
own environment, and two instants of its own work.**

| Moment | What is written | Where |
| --- | --- | --- |
| **At step 6 of `mission`** — worktree armed, before the first edit | `branch`, and a context claim `{session, role, started_at, ended_at: null}`. `session` is `CLAUDE_CODE_SESSION_ID` from the environment — not a transcript read, so E2's boundary is untouched. `role` is `subagent` or `main`, which the mission knows about itself. `started_at` is the instant the line is written. | the mission's own record in `docs/quality/mission-cost/missions.json`, committed on its branch |
| **At every context handoff** | the closing context's `ended_at`, then a fresh claim for the new context | the same record, appended, in the commit the handoff makes |
| **At step 8.1 of `mission`** — the draft pull request | the last claim's `ended_at`, then the resolved `transcripts` paths | the same record, in the commit that publishes the mission summary |

### How a claim resolves, exactly

```sh
python tools/mission_cost/measure.py identify \
  --session <uuid> --role subagent \
  --from <started_at> --to <ended_at>
```

Two rules turn the claim into paths:

- **Containment finds the mission's own context.** It needs no tolerance
  constant: the mission issued a request at `started_at` and another at
  `ended_at`, so both instants lie inside its own transcript's span by
  construction.
- **Being contained finds the mission's descendants** — the reviewers and
  helpers it dispatched, whose whole lives sit inside its window and whose cost
  is the mission's own. The registry's `transcripts` field gets both.

`role` is what keeps the answer from always being contested, and it is not a
detail: the coordinator's main thread spans every child it dispatched, so
without it every campaign child would find two containing transcripts and none
would be gradeable. A mission knows which of the two it is.

Two containing transcripts of the mission's own kind is real overlap — a
sibling that ran across the whole of this mission's life. `identify` then names
them all and refuses to choose: the mission is **contested**, and a contested
mission is `void` under section 5 rather than silently wrong. Section 8's
no-spanning-sibling rule is what keeps that rare.

This resolves after the fact, needs no live sampling, survives handoffs because
each context carries its own claim, and reads nothing but the five values
`method.md` section 3 already allows.

**Proved on this mission.** #567 ran as a dispatched agent under the campaign
coordinator's session and resolved its own transcript with the command above:
one containing context, `contested: false`, and the coordinator's main thread
correctly excluded by `role`. The procedure #576 had to improvise — two live
size samples, minutes apart — was not needed and was not used.

### The residual honesty

- The tail after `ended_at` — the final report, the handback — is outside the
  claim and is under-counted. Recorded as **E14** in `method.md`, not papered
  over. It is a handful of requests against a mission's hundreds.
- **E12 still stands.** Nothing verifies that a resolved path is the mission's
  work; the defence is that the mission wrote its own claim while it ran, and
  now that the claim is three timestamped lines rather than a remembered file
  name, a later reader can at least check the claim against the pull request's
  own commit times.
- This changes no rule in `method.md` section 2. `identify` produces the paths
  a mission writes into the existing `transcripts` field, which #576 added.
  Nothing is inferred and no new assignment rule exists to get wrong.

**What this protocol does not do:** fold the three moments into
`.claude/skills/mission/SKILL.md`. That file is inside the context ratchet, and
adding the instruction there is a cost paid on every turn of every session,
including the great majority that are not experiments. For campaign children
the dispatch carries it and the coordinator's checkpoint holds the state, which
is where the requirement is enforceable today. Folding it into the skill is a
follow-up whose context-budget cost is paid deliberately or not at all.

## 10. What this protocol costs

D8 is explicit that the loop's own ceremony is priced, and a protocol is
ceremony. So:

- **Per graded lever:** one `experiment.py grade` run and one ledger commit.
  The reading itself is the harness run #564 already built, over transcripts
  that exist anyway. No extra mission, no extra review round, no extra CI job
  beyond one offline `verify` over a file of a few kilobytes.
- **`PROBATION_MISSIONS = 1`** is the single decision that keeps this cheap,
  and it follows from section 3 rather than from wishing: a mechanism reading
  does not average, so demanding a second mission would buy nothing and cost a
  mission. The protocol asks for replication in exactly one place — a
  `removal`'s reference share — because that is the one place a number is
  borrowed from missions other than the graded one.
- **The standing instruction cost is a pointer.** Nothing is added to
  `CLAUDE.md`, `AGENTS.md` or any skill; the context ratchet's tracked weight
  does not move.

Against what it protects: a mission costs about **106,365,437 billable
tokens** on the two exactly measured points (#569 at 91,229,177; #576 at
65,967,727 over 297 requests), and the three ranked levers model at −62.0%
stacked. A protocol whose whole enforcement is one arithmetic command and one
committed JSON file is not what will decide whether that number is reached.
Landing an unproven lever is.

## 11. Where the pieces are

| Piece | Path |
| --- | --- |
| This protocol | `docs/quality/velocity/experiment-protocol.md` |
| The ledger — pre-registrations, readings, verdicts | `docs/quality/velocity/experiments.json` |
| The grader and the ledger checker | `tools/mission_cost/experiment.py` |
| The transcript claim resolver | `python tools/mission_cost/measure.py identify` |
| Measurement, the laws, the registry | `docs/quality/mission-cost/method.md`, `tools/mission_cost/` |
| The baseline and the ranking | `docs/quality/velocity/baseline.md`, `ranking.md` |
| CI enforcement | `.github/workflows/ci.yml`, the `experiment.py verify` step |
| Campaign state, per lever | the checkpoint on #563 |

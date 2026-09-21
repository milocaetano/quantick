# The context cap

Why one agent context stops at 80 requests and hands off, where the number came
from, and what the rule costs. The operative rule is three paragraphs in
[`.claude/skills/mission/SKILL.md`](../../../.claude/skills/mission/SKILL.md);
this file is the arithmetic behind it and is read only by someone changing the
number.

Registered as ledger entry **L1** of campaign
[#563](https://github.com/milocaetano/quantick/issues/563), issue
[#573](https://github.com/milocaetano/quantick/issues/573), form `reshape`,
under [the experiment protocol](experiment-protocol.md).

**Where this file lives, and why it is not under `docs/workflow/`.** That tree
is inside the context ratchet, so every byte there is paid on every turn of
every session. A 9 kB explanation of a three-paragraph rule would spend the
saving it exists to defend. Same reasoning, same place, as
`experiment-protocol.md`.

## The shape the cap is aimed at

A context's bill is not proportional to the work it does. Fitted in
[#566](https://github.com/milocaetano/quantick/issues/566) over 560 contexts
and 48,772 requests, and published in [the ranking](ranking.md):

| Population | Law | R² | Linear-only R² |
| --- | --- | ---: | ---: |
| Subagent | `64,224·N + 530.98·N²` | 0.9664 | 0.8089 |
| Main thread | `150,062·N + 233.14·N²` | 0.9937 | 0.8826 |

The quadratic term is the majority of the bill, because every request re-reads
everything its own context has already accumulated. Each turn adds about 1,062
tokens that every later turn pays again, so a context that runs four times as
long costs about eleven times as much: at the subagent law the average request
costs 74,843 tokens at 20 requests and 303,164 at 450.

It is concentrated, which is what makes it worth a rule at all — 58 of 520
subagent contexts ran past 150 requests and held 69.7% of all subagent tokens,
while the median subagent context is 42 requests.

## Where 80 came from, and why it is not 40

Two tables, and confusing them is easy, so both are stated.

**Within the subagent population**, capping every context and charging 8
requests per *extra* context for re-grounding
(`populations.subagent.counterfactual` in [`shape.json`](shape.json)):

| Cap | 40 | 60 | **80** | 120 | 150 | 200 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Modelled change | −45.9% | −44.6% | **−42.4%** | −37.5% | −34.1% | −29.2% |

**Across all tokens**, capping both populations and pricing every capped piece
on the subagent law — which is what capping *means*, since a fresh dispatch is
a subagent by construction (`policy.levers`, lever `L1`): **−45.9% at a cap of
80.**

So 40 is the arithmetic optimum of the first table and 80 gives up 3.5 points
to it. That trade is taken deliberately:

- **Handoffs are not free.** The counterfactual charges 8 requests per extra
  context, and the real charge is whatever it costs the next context to
  re-ground. Below 40 that charge starts eating the saving, and the model
  cannot see the part of it that is wall clock.
- **Every handoff serialises.** The next context cannot start until the
  previous one has written its brief. On a 365-request mission a cap of 80 is
  roughly 9% more requests, and they are not free in time. Decision D7 says the
  token axis wins where the two conflict, which is why the cap exists at all;
  it does not say wall clock is worthless.
- **Every handoff loses continuity.** A fresh context cannot remember what its
  predecessor noticed and did not write down. Halving the number of handoffs
  halves the number of times that can bite.
- **The curve is flat here.** 40 to 120 spans 8.4 points of a 45.9-point
  saving. The number is not delicate, which is the real finding: pick a
  boundary the work already has rather than the optimum of a smooth curve.

**To change it**, start with the copy agents obey: the literal **80** in the
context-cap rule of `.claude/skills/mission/SKILL.md`, which is the number a
mission actually reads. Then move `POLICY_CAP` in `tools/mission_cost/shape.py`,
rerun `python tools/mission_cost/shape.py report`, and regenerate the tables in
[the ranking](ranking.md). `FRAME_TRIM_TOKENS` and `REQUEST_SCALE` beside it are
the other two levers' knobs. The cap is also a registered constant of ledger
entry L1: changing it invalidates any verdict taken under the old value, and
that is its own pull request under the protocol's section 4.

## Why the boundaries are structural and not counted

**A context cannot observe its own request count from the inside.** There is no
call that answers "how many requests have I made", and a rule written as "stop
at request 80" with no way to see request 80 is prose, not a rule.

So the rule names boundaries the work already has, and they fall near the cap
rather than on it:

| Phase | What it holds |
| --- | --- |
| Grounding | `mission` steps 1 to 6: the ledger, the questions, the criteria, `GOAL.md`, the worktree |
| Implementation | step 7 to the draft pull request at step 8.1 |
| Each review, the CI watch, each repair batch | step 8.2 onward, already a fresh dispatch per piece since [#557](https://github.com/milocaetano/quantick/issues/557) |

Implementation is the one that can run long, and it is also the one with the
most obvious seams: a commit, a crate, a slice of the ledger. The rule says to
split it there rather than let it run.

For the cases where a context wants the number,

```sh
python tools/mission_cost/measure.py contexts --since <the claim's started_at>
```

reads `CLAUDE_CODE_SESSION_ID` from the environment and prints the request count
of the context that is running it, against the cap. `--since` is what makes the
answer exact: the context's own `started_at` names it by containment, and the
report says `resolved: true`. Without it the command can only offer the newest
writer in the session, which a sibling agent often is; that answer carries
`resolution: newest_write` and `resolved: false`, because a guess must not read
as a measurement. Two contexts containing the instant are `contested`, with
nothing named. It reads `usage` fields and
timestamps only, which is the same boundary the rest of the harness keeps
(method error mode E2); it never reads transcript content.

## What the rule costs

**The context ratchet.** The operative rule is in `mission/SKILL.md`, which is
tracked by the ratchet and therefore paid on every turn of every session that
loads the skill. Priced against what it buys:

| | |
| --- | ---: |
| Bytes added to the tracked instruction trees, net | under 1,000 |
| Tokens that is, per request, at roughly 4 bytes per token | ~250 |
| Over a 300-request mission | ~75,000 tokens |
| A mission costs, on the measured average | 106,365,437 billable tokens |
| The modelled saving | ~45% of that, ~48,000,000 tokens |

The instruction costs about 0.16% of what it saves. The reasoning, the curve and
this pricing table stay out of the tracked trees precisely so that the ratio
stays that way; the `!budget` line does not move, because the new bytes are paid
for in the same change by prose taken out of `docs/campaign/workflow.md` and by
shortening the step 8 paragraph this rule makes partly redundant.

**Wall clock and continuity** are the two real costs, stated above and stated
again in the ranking's own trade table, which calls the wall clock *worse* and
takes the lever anyway on D7.

**No hook.** A `PostToolUse` hook could count tool calls per session, but tool
calls are not requests, and the hook would fire on every call of every session
for the benefit of the few that are missions. The cap's enforcement is the
reading the protocol takes on the graded mission, plus `measure.py contexts` for
a context that wants to check itself.

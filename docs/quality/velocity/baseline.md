# Mission-cost baseline, and a verdict on seven merged optimizations

Campaign [#563](https://github.com/milocaetano/quantick/issues/563), task
[#565](https://github.com/milocaetano/quantick/issues/565). Measured with the
harness of [#564](https://github.com/milocaetano/quantick/issues/564) under
[`docs/quality/mission-cost/method.md`](../mission-cost/method.md), which was
registered before any number here was read.

## The short answer

Over 9.5 days, 87 merged missions and 47,788 model requests, the loop spent
**9.27 billion billable tokens**. **Cache reads are 95.0% of that** and carry
the headline: the cost is the standing context each request drags, multiplied
by the number of requests, and **subagents make 82.5% of the requests and spend
76.4% of the tokens**.

**None of the seven merged optimizations can be given a token verdict.** Not
one. The method's attribution places 0.35% of the period's tokens on a mission
and leaves 99.65% unplaced, so every comparison returns `cannot_be_attributed`
— the correct answer, and the one the thresholds were fixed in advance to
force. That is not a measurement failure to be worked around: sessions in this
repository routinely run for days across dozens of branches, so there is no
honest way to charge a session to a mission after the fact.

What *can* be measured without attribution says this:

- The **CI critical path really did fall** — median 1,164 s in the regime before
  #552 to **401 s** after #553, and below the 505 s that preceded the whole
  slowdown. That is #552 and #553 doing what they said.
- But **CI time per mission went up, not down**: across #547's pivot, median
  CI runs per mission **2.5 → 7** and median CI wall clock per mission
  **1,376 s → 2,956 s**. The runs got faster and there are nearly three times
  as many of them.
- The **session-opening frame did not shrink** after #546 halved the
  always-loaded instructions: median 58,146 → 59,489 tokens, which is
  `no_reduction` under the registered rule. The arithmetic explains why: what
  #546 removed from the frame a Claude Code session actually loads is about
  1,000 bytes, roughly 0.15% of a request.
- **#556's own self-report is right.** Its 63,973 bytes never reached a model.
  The opening frame does not move, and a drop of that size would have been six
  interquartile ranges — unmissable even at n = 3.

The measured population-level trend across the whole optimization window is an
**8.7% fall** in tokens per merged mission — *below* the registered 10%
threshold, computed from a ratio that has no dispersion, over an after-period
of 29 hours. It is not a result. It is reported because hiding it would be
worse.

## How to reproduce every number here

`python` on this machine is 3.12; `python3` is a broken shim.

```sh
# the baseline report: docs/quality/velocity/baseline-report.json
python tools/mission_cost/measure.py report --repo . \
  --as-of 2026-09-21T04:00:00Z \
  --transcripts ~/.claude/projects/C--src-quantick \
  --transcripts ~/.claude/projects/C--src-quantick-worktrees-fix-control-review-followups \
  --out docs/quality/velocity/baseline-report.json

# one verdict: the registry regrouped around that pull request's merge instant
python tools/mission_cost/group_registry.py \
  --registry docs/quality/mission-cost/missions.json \
  --pivot 2026-09-20T01:43:13Z \
  --out docs/quality/velocity/registries/pr-546.json
python tools/mission_cost/measure.py compare --repo . --no-gh \
  --registry docs/quality/velocity/registries/pr-546.json \
  --metric billable_tokens \
  --transcripts ~/.claude/projects/C--src-quantick \
  --transcripts ~/.claude/projects/C--src-quantick-worktrees-fix-control-review-followups

# the session-opening frame: not a method verdict, and it says so
python tools/mission_cost/opening_frame.py \
  --transcripts ~/.claude/projects/C--src-quantick \
  --since 2026-09-11T15:16:00+00:00 --pivot 2026-09-20T01:43:13+00:00
```

The CI figures come from one `gh` call, recorded in
[`ci-runs.json`](ci-runs.json) with its command and the instant it was taken.

**The transcript directory is live and append-only.** A later run reads a
larger directory and produces different totals, which is why every committed
artifact carries `inputs.digest`, and each one carries its own. The baseline report's is
`sha256:3797ff91...`, the four frame readings share
`sha256:fc97b464...`, and the verdict files carry theirs. They were taken hours
apart from one growing directory, which is why the frame readings cover 40
sessions where the baseline report covers 39: the session that ran this
measurement is itself in the data, and grew while it ran. A figure is quoted
from the file that carries it and nowhere else.

## What the instrument saw

| | |
| --- | --- |
| Transcript roots | `C--src-quantick`, `C--src-quantick-worktrees-fix-control-review-followups` |
| Transcripts read | 654 |
| Sessions | 49 |
| Lines that failed to parse | 0 |
| `usage` lines with no timestamp | 0 |
| Out-of-order lines | 0 |
| Files skipped | none |

The directory holds sessions from 2026-09-04T02:06:12Z onward, with **no
session at all between 2026-09-06T00:26Z and 2026-09-11T15:16Z**. That is error
mode E6 — the host prunes — and it is why this baseline uses only the
contiguous later period:

**Coverage period: 2026-09-11T15:16:05Z → 2026-09-21T03:21:13Z**, 821,069 s
(9.50 days), 39 sessions.

### The population, as a rule rather than a choice

`docs/quality/mission-cost/missions.json` holds **87 missions**, selected
mechanically so that no mission is in or out because of how it looked:

> Every pull request merged into `main` or into a campaign branch whose head
> branch begins `feat/`, `fix/`, `docs/`, `perf/`, `refactor/`, `chore/` or
> `test/`, and whose first commit — GitHub's own commit list for that pull
> request, section 1's default `started_at` — falls at or after
> 2026-09-11T15:16:00Z.

The prefix test keeps the unit "one branch, one pull request, one piece of
work": it excludes the `sync/*` pull requests, which are mechanical base moves
with no mission behind them, and the `campaign/*` integration pull requests,
which are the landing of dozens of missions at once and would be counted twice.
Two candidates fell outside the coverage test and are not in the registry: #360
(first commit 2026-09-10T20:19) and #306 (2026-09-04T03:42), both of which
began work in the pruned gap.

Every record carries an explicit `started_at` and `ended_at` rather than
leaving them null, so `gh` resolves nothing, the reading repeats offline, and a
rebase cannot shrink a window behind the campaign's back (method E3). Every
record's `sessions` list is empty; the next section says why.

### Two corrections to what T1 expected

1. **No session here is subagent-only.** T1's hand-back warned that 107 session
   directories against 49 root transcripts meant sessions observable through
   their subagents alone. In fact only 27 of those directories hold a subagent
   transcript, and **every one of those 27 also has a root transcript**. The
   other 80 hold `tool-results/` and metadata, not transcripts. Nothing in this
   reading is flagged `partial_main_thread`, and E5 did not bite.
2. **Only one worktree has its own projects directory.** T1 warned that a run
   passing a single root under-counts every mission worked in a worktree.
   Correct in principle, immaterial here: 48 of 49 sessions were started from
   the main checkout, and the single worktree directory
   (`fix-control-review-followups`) holds one session and no root transcript.
   Both roots are passed anyway, because the next campaign child may not be so
   lucky.

## The baseline: tokens

Tokens are the headline axis (D7). The four kinds are reported separately
because they are priced differently and tell different stories; the derived
`billable_tokens` is their sum and is used only for ordering.

### The whole coverage period

| Kind | Tokens | Share of billable |
| --- | ---: | ---: |
| `cache_read_input_tokens` | 8,811,465,614 | 95.05% |
| `cache_creation_input_tokens` | 441,790,961 | 4.77% |
| `output_tokens` | 17,113,881 | 0.18% |
| `input_tokens` (uncached) | 258,509 | 0.0028% |
| **`billable_tokens`** | **9,270,628,965** | |
| `requests` | 47,788 | |

**Cache reads carry the headline.** They are 95.2% of everything on the input
side and 95.0% of the billable total. Uncached input is 0.0028% of input — all
but three thousandths of one percent of what the loop sends is a cached prefix
being re-read. A change that does not move the cached prefix, or does not move
the number of requests that re-read it, cannot move this number, whatever else
it improves.

Per request: **193,995 billable**, 184,387 of them cache reads, 358 output.

### Main thread against subagents

Measured over the 31 sessions the attribution grouped as this period's shared
bucket — 99.5% of the period's tokens.

| | Billable tokens | Share | Requests | Share | Per request |
| --- | ---: | ---: | ---: | ---: | ---: |
| Main thread | 2,177,155,738 | 23.6% | 8,274 | 17.5% | 263,132 |
| Subagents | 7,043,595,331 | 76.4% | 39,028 | 82.5% | 180,475 |

A main-thread request carries 46% more context than a subagent request, and
there are 4.7 subagent requests for every main-thread one. Which of those two
facts costs more is #566's question, not this report's.

### What a mission costs in tokens — a bracket, not a number

This is the figure #565 asked for, and it is the one the data will not give.

- **Lower bound, 0 tokens.** Of 87 registered missions, the method's attribution
  placed a session on **three**: #366 (21,834,639 billable, 201 requests), #529
  (2,559,517, 38 requests) and #553 (14,028,772, 128 requests). The other 84
  report zero. Those three are accidents of the calendar — a session that
  happened to overlap exactly one mission's window — not measurements of a
  typical mission.
- **Upper bound, 106,558,954 tokens.** Divide the period's whole billable total
  by the 87 missions merged in it: 106.6 M billable, of which 101.3 M cache
  reads, 197 k output, 549 requests per mission. It is an upper bound because
  the numerator contains everything the machine did — campaign coordination,
  trading sessions, exploration, abandoned work, reviews of other branches and
  this measurement mission itself — not only the 87 missions.
- **It has no dispersion.** A ratio of two sums has no quartiles. The nearest
  thing with a distribution is the *session*, which is not a mission:

| Session billable tokens (n = 37 entries covering 39 sessions) | |
| --- | ---: |
| min | 250,245 |
| p25 | 8,739,823 |
| median | 26,294,723 |
| p75 | 106,602,810 |
| max | 4,940,480,709 |

The maximum is one session holding 53% of the period's tokens. Any statistic
over a distribution this skewed that is not a median is misleading, and even
the median describes sessions, not missions.

**So the honest baseline statement for tokens is: a mission costs somewhere
between nothing measurable and 106.6 M billable tokens, and the method cannot
narrow it further from these inputs.** The next section says why, and what
would fix it.

## Why no session is declared

D5 instructed this mission to declare session ids in the registry rather than
lean on windows. **No retrospective session can honestly be declared**, for two
independent reasons, and both are worth more to the campaign than a declaration
would have been.

1. **Nothing inside the privacy boundary says which branch a session worked
   on.** The fields that would — `cwd` and `gitBranch` — are deliberately not
   read (method E2, campaign D6), and the method's section 6 says a registry
   entry is written by a person and never inferred from transcript content, so
   reading them to build the registry would be the same breach through a side
   door. The one path-derived signal, the projects directory name, resolves
   exactly one session out of 49.
2. **Even with the branch, a session is the wrong unit here.** Sessions in this
   repository are long and multi-mission: the largest ran from 2026-09-11T22:30
   to 2026-09-14T15:42 across a window in which 45 pull requests merged. The
   method's section 2 never splits a session, for good reason — timestamps
   cannot divide it fairly. Declaring that session for any one mission would
   charge that mission three days of other people's work. This is error mode
   E4, and it is not fixed by fixing E2.

**What this means for the trader's open call on E2.** Reversing D6 and reading
`gitBranch` would *not*, on its own, make this baseline usable. It would make
attribution exact only if the method also stopped treating the session as
indivisible and started charging each individual request to the branch it named
— a different method, not a relaxed threshold, and one that would have to be
registered before the next reading. That is the call to put to the trader: not
"may the harness read `gitBranch`", but "should the unit of attribution become
the request rather than the session". This mission does not make it.

**The cheap fix needs no decision at all.** A mission that records its own
session id while it runs is exact by declaration, costs nothing, and breaks no
boundary. Every child of this campaign from here on should do it. Nothing
retrospective can.

### The unplaced share, measured

| | Billable tokens | Share of measured |
| --- | ---: | ---: |
| Placed on a mission | 38,422,928 | 0.35% |
| Shared between two or more missions' windows | 9,220,751,069 | 84.18% |
| Unassigned to any window | 1,694,316,234 | 15.47% |
| **Unplaced (shared + unassigned)** | **10,915,067,303** | **99.65%** |

`MAX_UNPLACED_SHARE` is 0.20. The measured share is 0.9965. Every comparison
over these inputs returns `cannot_be_attributed`, by the rule as written.

## The baseline: wall clock

Secondary to tokens (D7), but exact where tokens are not: a CI run and a pull
request belong to one branch by construction, so `gh`-derived timings need no
attribution at all.

### How the wall-clock tables are grouped

Every per-pull-request wall-clock table below is grouped by **the same rule as
the token verdicts**, from the same committed registries: the seven files under
[`registries/`](registries/), each produced by `group_registry.py` at that pull
request's merge instant. A mission whose window straddles the pivot joins
neither group, and so does the mission that delivered the change, whose window
ends *at* the pivot. That is why #547's before-group is 74 and not 75.

Those seven files are derived: each is exactly
`group_registry.py --registry docs/quality/mission-cost/missions.json --pivot
<merge instant>`, and they cost 168 KB in a repository separately working to
shrink its clone. They are committed anyway, because a reader reconciling a
table should not have to re-run a tool to see what it was grouped by. If the
trade goes the other way later, the seven merge instants — one at the head of
each per-pull-request section below — are enough to regenerate all of them, and
to the minute: the seven files come back byte-identical even when the pivot is
given without its seconds.

The figures are `dispersion.summary` over the `delivery` values already in
[`baseline-report.json`](baseline-report.json), which is three lines a reader
can run against the committed files:

```python
report = json.load(open("docs/quality/velocity/baseline-report.json"))
registry = json.load(open("docs/quality/velocity/registries/pr-547.json"))
by_branch = {entry["branch"]: entry for entry in report["missions"]}
for side in ("before", "after"):
    values = [by_branch[m["branch"]]["delivery"]["ci_runs"]
              for m in registry["missions"] if m["group"] == side]
    print(side, dispersion.summary(values))
```

An earlier draft of this report grouped these four tables by comparing the
instants as strings, which put each delivering mission in `before` instead of
neither and moved #547's `ci_runs` median from 2.5 to 3. The architecture
review of #571 caught it; the tool now refuses to compare an instant's
spelling, and the figures below are the corrected ones. No verdict changed.

### Per mission, from `gh` (n = 87, every registered mission)

| Metric | min | p25 | median | p75 | max | mean |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `pr_open_seconds` | 592 | 2,194 | **11,832** | 26,020 | 185,749 | 20,433 |
| `ci_wall_seconds` | 410 | 556 | **1,506** | 2,620 | 20,645 | 2,441 |
| `ci_seconds` | 410 | 556 | **1,541** | 2,951 | 36,292 | 3,159 |
| `ci_runs` | 1 | 1 | **3** | 5.5 | 30 | 4.09 |

The median mission is open for **3 h 17 m** and burns **25 m** of CI wall clock
across **3** runs. The p75 mission is open 7 h 14 m. The maximum, 51 h, is a
campaign child left open across a sync.

`pr_open_seconds` is not agent time: it starts when the draft opens and ends at
the merge, and includes review, CI, waiting for a base to move and the trader
being asleep.

### Agent time over the whole period

| Quantity | Seconds | Hours | Per registered mission |
| --- | ---: | ---: | ---: |
| `agent_seconds` (concurrency counted twice, on purpose) | 513,368 | 142.6 | 5,901 s = 1 h 38 m |
| `elapsed_seconds` (union; concurrency counted once) | 250,066 | 69.5 | 2,874 s = 48 m |
| `span_seconds` (outer bound, idle included) | 821,069 | 228.1 | — |

Both per-mission columns are upper bounds for the same reason the token divisor
is: the numerator holds everything, not only the 87 missions. The ratio that
does not depend on the divisor is the interesting one: **agent time is 2.05× the
elapsed time**, so the loop runs about two agents wide on average, and the loop
was active for 30% of the 228-hour span.

## The seven verdicts

Verdict vocabulary is the method's: `reduction`, `no_reduction`, `directional`,
`inconclusive`, `cannot_be_attributed`. `directional` is explicitly not a
saving.

| PR | What it claimed | Token verdict | Wall-clock verdict | Disagree? |
| --- | --- | --- | --- | --- |
| [#546](https://github.com/milocaetano/quantick/pull/546) | halve the always-loaded agent instructions | `cannot_be_attributed`; opening frame `no_reduction` | `no_reduction` | no |
| [#547](https://github.com/milocaetano/quantick/pull/547) | fast affected-crates job on drafts | `cannot_be_attributed` | `no_reduction` (runs per mission 2.5 → 7) | no |
| [#550](https://github.com/milocaetano/quantick/pull/550) | per-PR read cost visible and bounded | `cannot_be_attributed` | `no_reduction` | no |
| [#552](https://github.com/milocaetano/quantick/pull/552) | full Linux verdict, 43 min to parallel jobs | `cannot_be_attributed` | **per run: reduced. Per mission: `no_reduction`** | **yes** |
| [#553](https://github.com/milocaetano/quantick/pull/553) | full CI critical path under 7 minutes | `cannot_be_attributed` | **per run: reduced. Per mission: `no_reduction`** | **yes** |
| [#556](https://github.com/milocaetano/quantick/pull/556) | stop publishing MCP output schemas | `cannot_be_attributed`; opening frame `inconclusive` | `no_reduction` | no |
| [#557](https://github.com/milocaetano/quantick/pull/557) | phase two into fresh PR-only contexts | `cannot_be_attributed` | `inconclusive` (n = 3) | no |

Answering #565's three-way question directly: **all seven are "cannot
attribute" on the token axis**, which is the axis the trader named as the one
that hurts. On the wall-clock axis two of them — #552 and #553 — demonstrably
reduced the time a single CI run takes, and none of the seven reduced the CI
time a mission pays in total.

### Why every token verdict is the same

The evidence files are [`verdicts/`](verdicts/). Each was produced by
regrouping the registry around that pull request's merge instant, then running
the registered comparison. Example, #546 on `billable_tokens`:

```json
{ "verdict": "cannot_be_attributed",
  "reason": "shared and unassigned tokens exceed 20% of a group's measured tokens",
  "before": { "n": 76, "median": 0.0, "p25": 0.0, "p75": 0.0, "max": 21834639.0 },
  "after":  { "n": 10, "median": 0.0, "p25": 0.0, "p75": 0.0, "max": 14028772.0 } }
```

Both group medians are literally zero, because 84 of 87 missions were charged
nothing. The verdict would be worthless even if the unplaced gate let it
through.

**A note on the gate, for #566.** `MAX_UNPLACED_SHARE` is computed from
billable tokens *whatever metric was asked for*, so a comparison of
`ci_wall_seconds` — a quantity with no unplaced bucket at all, since a CI run
belongs to exactly one branch — is refused by a token-attribution guard. That
is why the wall-clock column above is computed from the same
`dispersion.compare` applied to the `gh` figures in
[`baseline-report.json`](baseline-report.json) rather than read off a
`measure.py compare` run, and why it is labelled as a separate axis rather than
presented as a method verdict. Changing the gate is not this mission's call.
Only `billable_tokens` and `elapsed_seconds` verdict files are committed;
`cache_read_input_tokens` and `output_tokens` were also run and returned the
same verdict for the same reason, since the gate does not depend on the metric.

### #546 — halve the always-loaded agent instructions

Merged 2026-09-20T01:43:13Z.

**It did what it said to the files.** At the merge commit `6bab4d91`, measured
against its first parent:

| | Before | After | Change |
| --- | ---: | ---: | ---: |
| `CLAUDE.md` | 9,547 | 8,539 | −1,008 B |
| `AGENTS.md` | 13,563 | 4,523 | −9,040 B |
| both | 23,110 | 13,062 | **−43.5%** |
| every `.claude/skills/*/SKILL.md` | 105,273 | 63,137 | **−40.0%** |

**It did not show up in what a session opens with.**
[`frames/pr-546.json`](frames/pr-546.json), the first request of every session
in the coverage period:

| | n | p25 | median | p75 |
| --- | ---: | ---: | ---: | ---: |
| before | 32 | 56,383 | **58,146** | 59,547 |
| after | 8 | 58,308 | **59,489** | 60,959 |

Verdict `no_reduction`: the after median did not fall. It rose 2.3%.

**The arithmetic says it could not have shown up.** In a Claude Code session
only `CLAUDE.md` is loaded up front — the frame this measuring session itself
opened with carried `CLAUDE.md` and the memory file and did not carry
`AGENTS.md`, which Codex loads instead. So the always-loaded cut in a Claude
Code session is 1,008 bytes, call it 250–340 tokens at 3–4 bytes per token, or
**0.14–0.18% of the 184,387 cache-read tokens a median request carries**. Even
counting `AGENTS.md`, 10,048 bytes is 2,500–3,350 tokens, **1.4–1.8% per
request** — still an order of magnitude under `MIN_RELATIVE_CHANGE`.

The 42 KB cut from the `SKILL.md` files is a different mechanism: a skill body
enters the context when the skill is invoked and is then re-read by every
subsequent request in that session, so it compounds where the always-loaded
frame does not. This report does not size that effect; sizing it is #566's job.

**Verdict.** Token axis: `cannot_be_attributed` by the method;
`no_reduction` on the opening frame, which is corroboration and not a method
verdict. Wall clock: `no_reduction`. The change is real on disk and too small
to find in the bill.

### #547 — fast affected-crates job on drafts

Merged 2026-09-19T21:42:46Z.

Per mission, from `gh`, grouped by
[`registries/pr-547.json`](registries/pr-547.json):

| Metric | before (n = 74) | after (n = 11) | |
| --- | ---: | ---: | --- |
| `ci_runs` median | 2.5 | **7** | +180% |
| `ci_wall_seconds` median | 1,376 | **2,956** | +115% |
| `ci_seconds` median | 1,440 | **3,723** | +159% |
| `pr_open_seconds` median | 10,839 | 13,206 | +22% |

`no_reduction` on all four.

**And the fast path was barely used.** A draft push under #547 produces a CI
run whose only job is `fast`, which finishes in well under two minutes. In the
29 hours and 85 CI runs after #547 merged, exactly **two** runs were that
short, both the first push on a CI-tuning branch. Before the merge, the short
runs were #547's own branch testing itself. The mechanism that would have saved
time is present and, in this window, almost never taken — consistent with
readiness requiring a full verdict at the exact head, so a branch that will be
made ready runs full CI anyway.

**Confounding is severe here and the number should not be read as #547's
fault.** The 11 missions after the pivot are the CI-tuning missions themselves
plus this campaign's children, which re-run CI by their nature, and #547's own
follow-up `78c2c37f` ("always run full CI when a draft becomes ready") adds a
run by design. n = 11 against n = 74 across 29 hours.

**Verdict.** Token axis: `cannot_be_attributed`. Wall clock: `no_reduction`,
with the caveat above.

### #550 — per-PR read cost visible and bounded

Merged 2026-09-20T03:40:45Z. Per mission, `ci_wall_seconds` median 1,389 →
1,819 and `ci_runs` median 3 → 7: `no_reduction`, n = 78 before and 7 after.

**It did not claim a reduction and was never going to produce one.** #550 adds
a measurement — a sticky comment, a ledger row, a bounded `pr-gate` advisory.
Its cost is additive: 35 of the 87 registered missions carry a read-cost row —
most of them from #550's own backfill of the last 30 merged pull requests —
each mission now pays one bounded measurement on `gh pr create|ready|merge`,
and every agent that reads the pull request reads one more comment. Grading
it as if it were an optimization would be a category error; the honest entry is
that there is no reduction to attribute, and that whether the visibility it
buys is worth its own cost is a question for #566.

**Verdict.** Token axis: `cannot_be_attributed`. Wall clock: `no_reduction`,
and none was claimed.

### #552 and #553 — the CI critical path

Merged 2026-09-20T01:55:27Z and 2026-09-20T10:12:23Z. Taken together, because
they are eight hours apart and no window separates them.

**Per run, they worked.** Duration of every completed `CI` workflow run on a
non-campaign branch, from [`ci-runs.json`](ci-runs.json):

| Regime | n | min | p25 | median | p75 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| R0 — 09-11 to 09-16, before the slowdown | 230 | 388 | 471 | **505** | 527 | 836 |
| R1 — 09-17 to #552 merging | 102 | 40 | 811 | **1,164** | 2,117 | 3,201 |
| R2 — between #552 and #553 | 21 | 51 | 387 | **597** | 627 | 1,064 |
| R3 — after #553 | 39 | 271 | 384 | **401** | 411 | 1,018 |

R1 → R3 is a **65.5%** fall in the median, with the after median far below the
before lower quartile: `reduction` under the registered rule, and by a wide
margin. Against R0 — the regime before CI ever slowed down — the fall is
**20.6%** (505 → 401), which also clears `MIN_RELATIVE_CHANGE`. #553's own body
claimed "6m 10s to 6m 35s"; the measured median across 39 subsequent runs is
6m 41s, with p75 at 6m 51s. The claim holds, slightly optimistically.

**Per mission, they did not.** Around #553's pivot, `ci_wall_seconds` median
1,412.5 → 1,794 (n = 80 / n = 6) and `ci_runs` median 3 → 6: `no_reduction` on
both. A run is **2.9×** shorter — the R1 → R3 median ratio from the regime
table above, 1,164 ÷ 401 — and a mission triggers twice as many of them. The
other reading of the same pair is the CI wall clock a mission pays *per run*:
471 s before, 299 s after, a 36% fall that the doubled run count more than
spends.

**This is the one place where the two axes disagree, and it is the finding.**
A CI run got much cheaper; a mission's total CI bill did not, because the
number of runs a mission triggers rose at the same time. Whether the extra runs
are caused by the draft/ready split (#547), by the CI-tuning missions
iterating on CI itself, or by the campaign's children being unusual, this
window cannot separate — n after is 6.

**Confounding.** R1's slowdown is not CI's fault and was not caused by any of
the seven: it begins on 09-17, on branches based on `campaign/outside-eight`,
and coincides with that campaign's code volume landing (#542 merged
2026-09-19T20:12Z, +89,520 / −51,083 across 607 files). Crediting #552 and #553
with the whole R1 → R3 fall would be crediting them for undoing a regression
they did not cause. The R0 → R3 figure, 20.6%, is the conservative one and the
one to carry forward.

**Verdict.** Token axis: `cannot_be_attributed` for both; neither change
touches a prompt. Wall clock: **`reduction` per CI run** (65.5% against R1,
20.6% against R0), **`no_reduction` per mission**.

### #556 — stop publishing MCP output schemas

Merged 2026-09-20T15:41:43Z.

#556's body states that it optimized a frame the model never reads: 63,973
bytes of output schemas left the `tools/list` frame (88,876 → 24,903 bytes),
and the Messages API tool definition has no output-schema field, so a client
drops them before building a request.

**The measurement agrees with the self-report.**
[`frames/pr-556.json`](frames/pr-556.json):

| | n | p25 | median | p75 | range |
| --- | ---: | ---: | ---: | ---: | --- |
| before | 36 | 56,559 | 58,219 | 59,800 | 43,203 – 66,558 |
| after | 4 | 58,305 | 59,731 | 60,959 | 57,618 – 61,052 |

The registered rule returns `inconclusive`: n = 4 after, and the minimum is 5.
But the effect size matters more than the count here. 63,973 bytes is roughly
16,000–21,000 tokens. Had those schemas been reaching the model, the opening
frame would have fallen by about a quarter — six times the interquartile range
of 3,241 tokens — and all four post-merge sessions would sit near 40,000
rather than near 59,000. None does; the lowest is 57,618. **A drop of the size
#556 would have caused is excluded by the data even at n = 4; a drop of zero is
what is observed.**

This is agreement, not disagreement: #556 was right about itself, and right to
put it in writing. Its value is the 26,000-byte ceiling test it leaves behind,
which guards the input schemas that *do* reach a model.

**Verdict.** Token axis: `cannot_be_attributed` by the method; `inconclusive`
on the opening frame, with the effect-size argument above showing the direction
is zero rather than unknown. Wall clock: `no_reduction`; none was claimed.

### #557 — dispatch phase two into fresh PR-only contexts

Merged 2026-09-20T21:45:30Z, **5 hours and 36 minutes before the transcripts
end**. Three missions started after it; one session opened after it.

Every comparison is `inconclusive` on n or `cannot_be_attributed` on
attribution. Nothing else is true yet.

What can be said is what #557 would have to move if it works: it shifts phase-
two work out of the main thread and into fresh subagent contexts. The baseline
shows a main-thread request carrying 263,132 billable tokens against a
subagent's 180,475, and subagents already making 82.5% of requests. So the
change trades a smaller number of expensive requests for a larger number of
cheaper ones plus a cache write per new context, and whether that is a saving
depends on a ratio this window cannot measure.

**The instrument cannot grade it as it stands.** The main-thread/subagent split
is reported per mission and per bucket, never per period, so there is no way to
ask "did the main-thread share fall after 2026-09-20T21:45". That is an
instrument gap, recorded here for #566 rather than fixed here.

**Verdict.** Token axis: `cannot_be_attributed`. Wall clock: `inconclusive`
(n = 3). Re-measure when a week of transcripts exists under the new rule.

## What else moved in each window

Every one of the seven merged inside a 24-hour stretch, into a trunk that was
also absorbing a large campaign landing. The windows do not separate. Merged to
`main` between 2026-09-19T20:00Z and 2026-09-21T03:20Z, in order:

| Merged | PR | Branch | What it touched |
| --- | --- | --- | --- |
| 09-19T20:12 | #542 | `campaign/outside-eight` | +89,520 / −51,083 over 607 files — the biggest single change in the period |
| 09-19T20:49 | #544 | `feat/architecture-ratchets` | new guards |
| 09-19T21:42 | **#547** | `feat/fast-draft-ci` | CI events, `pr-gate`, hooks |
| 09-20T00:08 | #549 | `fix/guards-after-outside-eight` | guard ratchets |
| 09-20T01:43 | **#546** | `docs/halve-agent-instructions` | `CLAUDE.md`, `AGENTS.md`, every `SKILL.md` |
| 09-20T01:55 | **#552** | `feat/ci-under-fifteen-minutes` | `ci.yml`, hooks, CI tools |
| 09-20T03:40 | **#550** | `feat/read-cost-ledger` | `pr-gate`, CI, ledger |
| 09-20T03:41 | #551 | `feat/analyst-authority-tier` | control plane |
| 09-20T10:12 | **#553** | `perf/ci-under-seven-minutes` | `ci.yml`, debug info, job split |
| 09-20T15:41 | **#556** | `fix/mcp-trim-tool-list` | MCP server frame |
| 09-20T19:06 | #554 | `feat/capability-family-registries` | control plane |
| 09-20T21:45 | **#557** | `feat/pr-is-the-context` | `mission` skill step 8 |
| 09-21T00:26 | #562 | `fix/ship-gate-reads-advisories` | ship gate |
| 09-21T00:50 | #558 | `fix/risk-lock-refusal-spacing` | product code |
| 09-21T03:12 | #569 | `feat/mission-cost-harness` | this campaign's instrument |

Four of the seven touch `ci.yml`, `pr-gate` or the hooks within nine hours of
one another. Three touch what an agent reads. #542 changes the amount of code
CI compiles in the same window in which #552 and #553 change how it compiles.
**No pair of these windows is separable from the others**, and the method's
`cannot_be_attributed` rule would have fired on the overlap test alone even if
the attribution had been perfect.

One data-quality note worth a reader's caution: **#552's pull request body does
not describe #552's diff.** Its title and its ten changed files are the CI
work; its embedded mission summary and "What changed" section describe a
composition-root mission on `crates/app/src/launch.rs`. The verdict above is
graded against the diff, not the body.

## The one population-level trend, and why it is not a result

Splitting the coverage period at #547's merge — the first of the seven — and
dividing each side's whole token spend by the missions whose windows fall
wholly on that side:

| | before (74 missions, 25 session entries) | after (11 missions, 12 session entries) | change |
| --- | ---: | ---: | ---: |
| billable per mission | 110,312,968 | 100,679,029 | **−8.7%** |
| cache reads per mission | 104,515,630 | 97,937,180 | −6.3% |
| output per mission | 182,568 | 327,622 | **+79.5%** |
| requests per mission | 566 | 539 | −4.7% |
| billable per request | 195,039 | 186,631 | −4.3% |
| cache reads per request | 184,789 | 181,549 | −1.8% |

**This is not a result, for four reasons, each sufficient on its own.**

1. −8.7% is below `MIN_RELATIVE_CHANGE`, which was registered at 10% before any
   of this was read.
2. A ratio of two sums has no dispersion, so the registered rule cannot be
   applied to it at all — there is no n, no quartile and no way to say whether
   −8.7% is signal.
3. The after side is 29 hours long and contains 12 session entries, several of
   them the CI-tuning and measurement missions themselves.
4. The −1.8% fall in cache reads per request is the same order as the 1.4–1.8%
   #546 could arithmetically have delivered, which makes it a coincidence
   worth noticing and nothing more.

The +79.5% in output tokens per mission is the largest move in the table and
runs the wrong way. It is reported because it is there, not because this
report can explain it.

## Where this stops

- **No verdict here carries statistical significance, and none is claimed.**
  The largest after-group is 11 missions and the smallest is 3; the registered
  minimum is 5. No p-value, confidence interval or hypothesis test appears in
  this report, because n and the skew do not support one.
- **The only reduction this report asserts is per CI run**, for #552 and #553
  together, and it is asserted as a median shift over 39 runs against 102, with
  the confounder named and the conservative figure (20.6% against R0) offered
  as the one to carry.
- **The token axis produced no verdict for any pull request.** That is the
  campaign's central finding so far: the loop's dominant cost was, for these
  seven changes, unmeasurable after the fact.
- **Nothing here says whether the work was any good.** Method E11; #563's C5
  owns quality.
- **Nothing here ranks the cost sinks or proposes an experiment.** Those are
  #566 and #567.
- **These readings expire.** The transcript directory is pruned by the host and
  grows while it is read. Re-running any command in this report on a later day
  gives different totals and a different digest; that is the input moving, not
  the harness. It happened twice during this mission: an earlier run of the
  #546 opening frame, two sessions before the committed one, gave n = 7 after
  and a median of 58,795 where the committed reading gives n = 8 and 59,489 --
  the same `no_reduction` verdict, and further from a reduction, not nearer.
  The committed files are the readings as taken, each at the digest it carries.

## Baseline figures for the charter

```text
Coverage: 2026-09-11T15:16:05Z .. 2026-09-21T03:21:13Z (9.50 days)
Population: 87 merged missions; 39 sessions; 47,788 requests
Tokens (period total): billable 9,270,628,965
  cache_read 8,811,465,614 (95.0%) | cache_creation 441,790,961
  output 17,113,881 | uncached input 258,509
Per request: billable 193,995 | cache_read 184,387 | output 358
Main thread 23.6% of billable / 17.5% of requests; subagents 76.4% / 82.5%
Tokens per mission: not measurable. Bracketed between 0 (attribution placed
  0.35% of tokens) and 106,558,954 billable (period total / 87 missions).
  Unplaced share 99.65% against a MAX_UNPLACED_SHARE of 0.20.
Wall clock per mission (gh, n=87, exact):
  pr_open_seconds  median 11,832  p25 2,194  p75 26,020
  ci_wall_seconds  median  1,506  p25   556  p75  2,620
  ci_runs          median      3  p25     1  p75    5.5
Agent time (period): agent 513,368 s | elapsed 250,066 s | span 821,069 s
  = 2.05x concurrency; 30% of the span active
Verdicts on #546 #547 #550 #552 #553 #556 #557: cannot_be_attributed on the
  token axis, all seven. Wall clock: reduction per CI run for #552+#553
  (median 1,164 s -> 401 s in regime; 505 s -> 401 s against the pre-slowdown
  regime), no_reduction per mission for all seven -- at #547's pivot, median
  ci_runs 2.5 -> 7 (n 74/11) and ci_wall_seconds 1,376 -> 2,956.
```

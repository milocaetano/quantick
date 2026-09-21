# Mission cost

What a mission cost to deliver: tokens by kind, main thread apart from
subagents, and wall clock — from the three sources that already exist and none
of which was read by a committed tool before this one.

[`docs/quality/mission-cost/method.md`](../../docs/quality/mission-cost/method.md)
is the specification. It was committed before the first reading was taken, on
purpose: a threshold chosen after seeing the data can always be moved until a
result appears. Where this harness and that document disagree, the document is
right and the harness is the bug.

## What is not in the repository

**The session transcripts are the trader's own conversations. They are not in
this repository, they are not reproducible in CI, and nothing here commits
them.** They live on the trader's machine, under the Claude projects directory
for the repository the session was started in.

So:

- The harness runs **locally**, by hand. No workflow runs it, and no workflow
  can: the input does not exist on a runner.
- Only **derived aggregates** are ever committed — counts, sums, quartiles and
  verdicts. Never a line, never a quotation, never a summary of what was said.
- The reader takes **five values** per line: the timestamp and the four token
  counters. `Record` has exactly five fields, so a sixth cannot reach a report
  by accident, and `test_transcripts.py` plants bait strings in the fixture and
  asserts that none of them survives.
- The fixture under `fixtures/` is **synthetic**, written by hand for these
  tests. Nothing in it came from a real session.

The one CI step this tool has runs its own offline suite against that fixture.

## Running it

One command:

```sh
python tools/mission_cost/measure.py report --repo .
```

It writes canonical JSON to standard output: sorted keys, ASCII, one trailing
newline. Two runs over the same input set are byte identical, and
`inputs.digest` fingerprints that input set so a changed number can be traced
to changed inputs rather than to a changed harness.

Useful flags:

| Flag | What it does |
| --- | --- |
| `--transcripts DIR` | a transcript root; **repeat it** when the missions ran from worktrees, which have their own projects directories |
| `--registry FILE` | the mission registry (default `docs/quality/mission-cost/missions.json`) |
| `--as-of INSTANT` | bound an open pull request and an unfinished window, so an open mission still reports a duration |
| `--no-gh` | transcripts and the registry only; no `gh`, no `git`, no network |
| `--out FILE` | write there instead of standard output |

And to grade one metric across the registry's two groups:

```sh
python tools/mission_cost/measure.py compare --repo . --metric billable_tokens
```

`shape.py` answers the question a mission comparison cannot: what one **agent
context** costs as it lengthens, and what the review chain around it caught. It
has three subcommands.

```sh
# the cost shape, from local transcripts; the window cuts requests, not files
python tools/mission_cost/shape.py measure --repo . \
  --since 2026-09-11T15:16:05Z --until 2026-09-21T06:20:00Z \
  --out docs/quality/velocity/shape.json

# what the review chain produced, from `gh`, over the registry's pull requests
python tools/mission_cost/shape.py ceremony --out docs/quality/velocity/ceremony.json

# the report's generated blocks, offline, out of those two committed documents
python tools/mission_cost/shape.py table --block figures \
  --shape docs/quality/velocity/shape.json \
  --ceremony docs/quality/velocity/ceremony.json
python tools/mission_cost/shape.py table --block levers \
  --shape docs/quality/velocity/shape.json
```

`table` is the answer to a drift class rather than a convenience. A report that
retypes a figure out of a file that already holds it goes stale silently, and a
checker over prose goes stale the same way; a **generator** does not, because
`test_shape.ReportFigures` fails the moment
`docs/quality/velocity/ranking.md`'s blocks stop matching the JSON. Every row
of the lever table carries the cost law it was priced on, because the first
version of that table priced one row on a law its own baseline did not use.
`--block` takes its choices from the `BLOCKS` registry, so a third generated
block is one entry there rather than five edits in five places.

The policy assumptions the lever table is priced on are **assumptions, not
measurements**, so each is a flag on `measure` rather than a source edit, and
the `policy` block records what the run used:

| Flag | Default | Domain | What it supposes |
| --- | --- | --- | --- |
| `--cap` | 80 | at least 1 | requests after which a context hands off to a fresh dispatch |
| `--handoff` | 8 | not negative | requests a handed-off context spends re-reading its brief |
| `--frame-trim` | 10000 | not negative, and below the smallest *usable* fitted frame | tokens cut from the standing frame |
| `--request-scale` | 0.8 | greater than 0 | fraction of today's requests that remain |

Outside those domains the command **refuses by name**, the way it refuses a
missing transcript directory. A flag added so an operator can vary a stated
assumption must not also let them publish a document that cannot mean anything:
`--cap 0` is a division by zero, `--cap -5` prices as no cap at all and reports
a plausible number that means nothing, and a trim above the fitted frame prices
a request at less than nothing. The `--frame-trim` bound is the one that needs
the fit, so it is checked after it rather than by `argparse`, and a *degenerate*
law does not bound it — refusing there would blame the flag for the fit.

What each document says about itself, because a number nobody can grade is
worse than no number. The rule throughout is that **the judgement travels with
the number**: every surface rendered off a graded value carries the same
verdict, so the JSON and the report can never disagree about whether a figure
is real. `verdict()` is the one owner of the three states a graded block can be
in — **graded**, **degenerate** and **ungraded** — and a document carrying no
`validity` field is the third, never the first: it still renders, but no block
claims it was graded and passed. `test_shape.OneVerdictRule` asserts that over
every block in the registry, because the two blocks drifted apart on exactly
this question once.

- **`cost_law.validity`.** The fit is an unconstrained least squares, so a
  narrow population — a short window, one campaign's contexts, a main-only run
  — can put its minimum at a negative coefficient, which is arithmetic rather
  than a measurement. Every law carries `usable` and, when it is not,
  `degenerate_because`. The figures block labels the law row, the shares row
  and the **All contexts** headline that sums them. The committed fixture is
  one of those populations on purpose, and `test_shape.Command` pins it.
- **`policy.validity`.** The same question one surface on, and it has to be
  asked separately: a lever row is arithmetic *over* a law, so it can fail when
  the law is sound (`negative_modelled_tokens`) and it inherits the law's
  defect when it is not (`priced_on_a_degenerate_law`, with
  `degenerate_populations` naming which). `lever_table` leads with **Not a
  reading** and marks both numeric columns rather than printing the L1 figure —
  the one #573 is scheduled on — as a measurement.
- **`totals.truncated_pulls` and each pull's `truncated`.** Every `gh`
  connection is asked for its `totalCount` and the document records what it did
  not see. Above zero, every ceremony total is a floor rather than a number, and
  the figures block says so. Three claims are kept apart there and never
  collapsed: a recorded zero, a recorded shortfall, and an **absent** field,
  which says only that completeness is unknown to the document.
  `ceremony` also stamps an `inputs` block — the registry it read, the pull
  requests in it and a digest over their facts — for the reason `measure` and
  `opening_frame.py` do. The committed `ceremony.json` predates those fields
  and is the reading taken at the time; the next run writes them.

`--metric` takes a token kind, `billable_tokens`, `main_thread.<kind>`,
`subagents.<kind>`, `agent_seconds`, `elapsed_seconds`, `span_seconds`,
`pr_open_seconds`, `ci_seconds`, `ci_wall_seconds` or `ci_runs`.

`python` on the trader's machine is 3.12; `python3` is a broken shim there. CI
runs on `python3`, which is the real interpreter on a runner.

## What it reads

| Source | What is taken |
| --- | --- |
| session transcripts | `timestamp` and the four `message.usage` token counters, nothing else |
| `gh pr view` | created, merged and closed instants, head, base and state |
| `gh run list` | completed runs' start and end on the mission's branch |
| `docs/quality/read-cost/ledger.md` | that pull request's recorded read cost, through `tools/read_cost/ledger.py`'s own parser rather than a second one |

## The registry

`docs/quality/mission-cost/missions.json` says which branch and pull request a
mission is, and may declare what it owns outright — whole sessions, individual
transcripts, or both:

```json
{
  "branch": "fix/example",
  "pr": 123,
  "sessions": [],
  "transcripts": ["3fa85f64-5717-4562-b3fc-2c963f66afa6/subagents/agent-abc.jsonl"],
  "started_at": null,
  "ended_at": null,
  "group": null,
  "note": "why this mission is in this group"
}
```

Both are exact and both report method `declared`. `transcripts` exists because
the session stopped being the thing a mission owns: missions are dispatched as
agents under one coordinator session, so siblings share a session directory,
and a mission that caps a context and hands off owns several transcripts.
Declaring the session would charge a mission with its siblings' cost. A path is
spelled the way the harness addresses it — `<uuid>.jsonl` or
`<uuid>/subagents/<name>.jsonl`, relative to a transcript root and never
carrying the root's own name.

Two missions claiming one transcript, one claiming a transcript whose session
another claims, and a path that more than one of the run's transcript roots
holds are each a registry error and the run is refused. A declared transcript
*no* root holds is a `declared_transcript_missing` note instead, because the
host prunes transcripts and a committed registry outlives them.

What nobody declared is placed by its timestamps, and a session two missions
could claim goes to the shared bucket with both names on it — never divided,
because timestamps cannot divide it fairly. The undeclared remainder of a
partly declared session is placed the same way, on its own timestamps, so a
coordinator's main thread stays out of its children's totals.

The report always prints the shared and unassigned buckets with their full
totals. A report that could not place half its tokens says so; that is the
point of the bucket.

## The modules

| File | What it owns |
| --- | --- |
| `transcripts.py` | the privacy boundary: discovery, the five-field record, the 300-second idle bound, and the one rule for reading an instant |
| | `locate` walks and classifies without opening a file; `discover` is `locate` plus the fold. A caller with its own fold — `shape.py`'s windowed one — takes `locate`, so no transcript is parsed twice |
| `canonical.py` | section 7's byte contract: sorted keys, ASCII, LF, one trailing newline |
| `attribution.py` | the registry, the three assignment rules, and the main/subagent split |
| `dispersion.py` | quartiles, and the registered rule for when a fall counts as a reduction |
| `delivery.py` | `gh` timings and the read-cost row, every call through an injectable runner |
| `measure.py` | the command, the report and the comparison |
| `group_registry.py` | one registry, regrouped `before`/`after` around an instant |
| `opening_frame.py` | how big the prompt is on a session's first request |
| `shape.py` | what one agent context costs as it lengthens, what the review chain caught, and the report blocks generated from both |

`group_registry.py` exists because the method gives each mission one `group`
field, so grading seven merged changes needs seven groupings of one population.
It reads and writes registries and never touches a transcript.

`opening_frame.py` answers the question a mission comparison cannot when no
session can be placed on a mission: how big is the standing frame a session
opens with. It needs no attribution, it reuses the registered thresholds
unchanged, and it marks its own output `registered_comparison: false` because
it is post-hoc — corroboration beside a method verdict, never one. Like
`measure.py`, it stamps every document with the `inputs.digest` of the openings
it read.

Two rules the package keeps to one owner each, both with a test that fails if a
second copy appears (`test_canonical.OneOwner`): the byte contract in
`canonical.py`, and `transcripts.require_instant`, which every command that
puts a mission on one side of a pivot goes through. `Z` sorts after `+`, so one
moment spelled two legal ways must never be compared as text.

## Grading a change

Measuring what a mission cost is one question; deciding whether a change to the
loop earned its place is another, with its own thresholds and its own
refusals. [`docs/quality/velocity/experiment-protocol.md`](../../docs/quality/velocity/experiment-protocol.md)
is that rule and `docs/quality/mission-cost/method.md` section 10 registers its
constants. `experiment.py` executes it over the committed ledger,
`docs/quality/velocity/experiments.json`:

```sh
python tools/mission_cost/experiment.py verify
python tools/mission_cost/experiment.py grade --id L1
```

`verify` reads no transcript at all -- it is arithmetic over the ledger -- so
CI runs it on every pull request, which is the only part of the protocol a
later session cannot decline to read. `test_experiment.VerifyStaysOffline`
asserts that property rather than trusting it, because this module loads
`shape.py`, which loads the module that shells out to `gh`. A refusal carries
a stable `code` beside its sentence, the way `shape.py` publishes its
degenerate-law names, so a caller can act on *which* refusal fired. `grade` prices each lever's
counterfactual through `shape.modelled` on the coefficients `shape.json`
already publishes, never on a law re-fitted over the mission being graded.

A verdict may only rest on transcripts a mission declared about itself. What a
mission records, and when, is section 9 of the protocol; this resolves the
record it wrote into the exact paths the registry wants:

```sh
python tools/mission_cost/measure.py identify \
  --session "$CLAUDE_CODE_SESSION_ID" --role subagent \
  --from 2026-09-21T10:06:00Z --to 2026-09-21T10:20:00Z
```

Containment finds the mission's own context; being contained finds the agents
it dispatched. `--role` is not decoration: a coordinator's main thread spans
every child it dispatched, so without it every campaign child would look
contested and none would be gradeable.

## Tests

```sh
python -m unittest discover -s tools/mission_cost -p 'test_*.py'
```

Offline, every one of them: the `gh` calls answer from a table and no test
reads a transcript outside `fixtures/`. Two read committed repository data
rather than the fixture — `test_group_registry.CommittedRegistries` re-derives
each of `docs/quality/velocity/registries/pr-*.json` from
`docs/quality/mission-cost/missions.json` and fails if a copy has gone stale,
which is what keeps a published reconciliation honest when the population
grows. They skip when those files are absent.

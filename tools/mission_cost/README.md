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
mission is, and may declare its sessions outright. A declared session is exact.
A session nobody declared is placed by its timestamps, and a session two
missions could claim goes to the shared bucket with both names on it — never
divided, because timestamps cannot divide it fairly.

The report always prints the shared and unassigned buckets with their full
totals. A report that could not place half its tokens says so; that is the
point of the bucket.

## The modules

| File | What it owns |
| --- | --- |
| `transcripts.py` | the privacy boundary: discovery, the five-field record, the 300-second idle bound |
| `attribution.py` | the registry, the two assignment rules, and the main/subagent split |
| `dispersion.py` | quartiles, and the registered rule for when a fall counts as a reduction |
| `delivery.py` | `gh` timings and the read-cost row, every call through an injectable runner |
| `measure.py` | the command, the report and the comparison |
| `group_registry.py` | one registry, regrouped `before`/`after` around an instant |
| `opening_frame.py` | how big the prompt is on a session's first request |

`group_registry.py` exists because the method gives each mission one `group`
field, so grading seven merged changes needs seven groupings of one population.
It reads and writes registries and never touches a transcript.

`opening_frame.py` answers the question a mission comparison cannot when no
session can be placed on a mission: how big is the standing frame a session
opens with. It needs no attribution, it reuses the registered thresholds
unchanged, and it marks its own output `registered_comparison: false` because
it is post-hoc — corroboration beside a method verdict, never one.

## Tests

```sh
python -m unittest discover -s tools/mission_cost -p 'test_*.py'
```

Offline, every one of them: the fixture is the only input and the `gh` calls
answer from a table.

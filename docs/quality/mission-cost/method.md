# Mission-cost method

Registered for campaign [#563](https://github.com/milocaetano/quantick/issues/563),
task [#564](https://github.com/milocaetano/quantick/issues/564).

This document is the method. It is committed before any commit in this campaign
reports a number read from the session transcripts, because a method chosen
after the data has been seen proves nothing: every threshold below can be moved
to make a result come out, and the only defence is that the thresholds were
fixed first. `tools/mission_cost/` implements it; where the implementation and
this document disagree, this document is the specification and the
implementation is the bug.

Changing a number here after a reading has been taken invalidates every reading
that used the old one. A change is its own pull request, states which readings
it invalidates, and those readings are taken again.

## 1. What a mission is

**One branch and one pull request.** A mission is identified by its branch name,
which is unique on this repository, and carries the pull request opened from it.
A branch with no pull request is not a mission and is not measured. A pull
request whose branch produced several pull requests is a registry error, not a
measurement the harness resolves.

A mission's identity is the registry record described in section 6: branch, pull
request number, base ref, head SHA, and the work window `[started_at, ended_at)`.

The **work window** is, by default:

- `started_at` — the committer date of the earliest commit reachable from the
  pull request's head but not from its merge base with its base ref, read from
  GitHub's own commit list for that pull request: once the branch is merged its
  head is an ancestor of the base ref and a local merge base collapses onto it;
- `ended_at` — `mergedAt` if the pull request merged, else `closedAt` if it
  closed, else the report's `--as-of` instant.

Both may be overridden in the registry. The default is a convenience; the
override is the authority.

## 2. How a transcript is assigned to a mission

The unit of assignment is a **session**, identified by the session UUID that
names the transcript file or its directory. A session is never split.

Two rules, applied in order:

1. **Declared.** A registry record may list `sessions: ["<uuid>", …]`. Every
   listed session — its root `<uuid>.jsonl` and every
   `<uuid>/subagents/agent-*.jsonl` beneath it — is assigned to that mission.
   Assignment method `declared`. A session declared by two missions is a
   registry error and the harness refuses the run.
2. **Window-inferred.** A session that no record declares is a *candidate* for
   every mission whose work window overlaps the session's own
   `[first timestamp, last timestamp]` interval by any amount. Then:
   - exactly one candidate → assigned to it, method `window`;
   - more than one candidate → assigned to none, counted in the **shared**
     bucket, with the candidate missions named;
   - no candidate → counted in the **unassigned** bucket.

The shared and unassigned buckets are reported with their full token totals, so
a reader can always see how much of the measured cost the attribution failed to
place. A report that places half its tokens is not a better report than one that
says so.

**Subagents inherit.** A subagent transcript is assigned by its parent session
directory. Where a session directory has no root transcript, the directory name
is still the session UUID and the same two rules apply to the subagent files'
own timestamps.

## 3. Which `usage` fields count

The reader takes, from each transcript line, exactly five values and discards
the rest of the line before it returns:

| Taken | From |
| --- | --- |
| `timestamp` | the line's top level, ISO-8601 with a `Z` suffix |
| `input_tokens` | `message.usage.input_tokens` |
| `cache_creation_input_tokens` | `message.usage.cache_creation_input_tokens` |
| `cache_read_input_tokens` | `message.usage.cache_read_input_tokens` |
| `output_tokens` | `message.usage.output_tokens` |

A line without a `message.usage` object is counted only as a line seen; it
contributes no tokens and no timestamp. A missing counter inside a present
`usage` object reads as `0`.

**The four kinds are reported separately and are never silently summed.** They
are priced differently and they tell different stories: a cache read is the
standing cost of the context a turn carries, a cache write is what changing that
context costs, uncached input is what the turn newly said, and output is what
the model produced. A change that trades one for another is invisible in a
single total. A derived `billable_tokens` — the sum of all four — is reported
as one ordering number alongside them, never instead of them.

`requests` is the count of lines carrying a `usage` object.

**Everything else is discarded, including the rest of `usage`.** The transcripts
also carry `output_tokens_details`, `server_tool_use`, `service_tier`,
`cache_creation`, `inference_geo`, `iterations` and `speed` inside `usage`, and
`cwd`, `gitBranch`, `sessionId`, `model`, `message.content` and much more
outside it. None of them is read into a record. The reader's return type has
five fields, so nothing else can reach a report by accident.

**The harness reports token counts, not money.** Applying prices would date the
evidence and would hide which of the four kinds moved.

## 4. How wall clock is bounded

Timestamps say when a request was issued. They do not say how long a person
waited, and a transcript that sat open overnight would otherwise report the
night as work. So:

- **`IDLE_GAP_SECONDS = 300`.** Within one transcript, order the timestamps and
  take each consecutive pair. A gap of at most 300 seconds contributes an
  interval of that length. A longer gap contributes nothing: the session is
  treated as having stopped and restarted.
- **`agent_seconds`** — the sum of those interval lengths across every
  transcript assigned to the mission. A subagent running beside the main thread
  is counted twice here, deliberately: this is agent-time, the quantity that
  scales with what the loop asks for.
- **`elapsed_seconds`** — the length of the *union* of those intervals across
  the mission. Concurrency is counted once. This is the honest answer to "how
  long did it take".
- **`span_seconds`** — last timestamp minus first timestamp across the mission,
  idle included. It is the outer bound and it is not a cost.

Delivery wall clock comes from `gh`, not from transcripts:

- **`pr_open_seconds`** — `mergedAt` (else `closedAt`, else `--as-of`) minus
  `createdAt`.
- **`ci_seconds`** — summed over the completed workflow runs on the mission's
  branch: `updatedAt` minus `startedAt`. The branch rather than one head SHA,
  because a mission pays for every run it triggered, including the ones on the
  heads it pushed over.
- **`ci_wall_seconds`** — the union of those same run intervals, so parallel
  workflows on one head count once.
- **`ci_runs`** — how many.

Read cost comes from `docs/quality/read-cost/ledger.md`, through
`tools/read_cost/ledger.py`: the mission's row, or `null` when it has none. It
is a covariate that says how much source the change asked a reader to hold, not
a cost this method measures.

## 5. What counts as a reduction

The comparison unit is the mission. A metric is any one of the per-mission
numbers above. Two groups, `before` and `after`, each a set of missions named in
the registry.

Dispersion is reported for every group: `n`, `min`, `p25`, `median`, `p75`,
`max` and `mean`. Percentiles are the inclusive, linearly interpolated
definition — Python's `statistics.quantiles(..., method="inclusive")` — so the
number does not depend on the implementation.

Registered thresholds:

- **`MIN_GROUP_N = 5`.** Fewer than five missions in either group and the
  verdict is `inconclusive`, whatever the medians say.
- **`MIN_RELATIVE_CHANGE = 0.10`.** A median that moved by less than a tenth of
  the before-median is noise at this n.
- **`MAX_UNPLACED_SHARE = 0.20`.** If a group's shared plus unassigned tokens
  exceed a fifth of the tokens measured in that group's window, the group's
  attribution is too weak to carry a claim.

The verdict, computed in this order:

| Verdict | Condition |
| --- | --- |
| `cannot_be_attributed` | either group breaches `MAX_UNPLACED_SHARE`, or the two windows overlap in time |
| `inconclusive` | either group has `n < MIN_GROUP_N` |
| `reduction` | `median_after < p25_before` **and** `(median_before − median_after) / median_before ≥ MIN_RELATIVE_CHANGE` |
| `no_reduction` | `median_after ≥ median_before` |
| `directional` | the median fell, but not past `p25_before` or not by `MIN_RELATIVE_CHANGE` |

`directional` is not a saving. The campaign may report it as a signal worth
another window; it may not count it towards the target.

The reported reduction size, when the verdict is `reduction`, is
`(median_before − median_after) / median_before`, stated with both group
medians and both `n` beside it. A percentage without its `n` and its dispersion
is not a result this method produces.

## 6. The registry

`docs/quality/mission-cost/missions.json`, schema 1:

```json
{
  "schema": 1,
  "missions": [
    {
      "branch": "feat/example",
      "pr": 123,
      "sessions": ["8f14e45f-ceea-467a-9d1a-000000000000"],
      "started_at": null,
      "ended_at": null,
      "group": "before",
      "note": "why this mission is in this group"
    }
  ]
}
```

`sessions` may be empty, and then section 2's window rule applies. `started_at`
and `ended_at` are `null` unless the default window is wrong. `group` is
`"before"`, `"after"` or `null`. Every field is written by a person or by a
mission recording itself; nothing infers a registry entry from transcript
content.

## 7. Determinism

One documented command, canonical JSON out: keys sorted, ASCII, newline at end,
no value derived from the clock except those read from the inputs and an
explicit `--as-of`. The report carries an `inputs.digest`: a SHA-256 over the
sorted list of transcript relative paths, each with its record count and its
four counter totals. Two runs over the same input set produce byte-identical
output, and a changed digest says the inputs moved rather than the harness.

## 8. Known error modes

Stated here rather than discovered later, because every one of them bounds what
a reading from this harness is allowed to claim.

- **E1 — Window inference cannot separate concurrent missions.** This repository
  routinely carries twenty or more live worktrees. A session overlapping two
  windows is placed in the shared bucket and leaves both missions' totals. Only
  declared sessions are exact, and the shared bucket's size is the honest
  measure of how much this hurt.
- **E2 — The exact fix exists and is deliberately not used.** `cwd` and
  `gitBranch` are present on most transcript lines and would resolve E1 outright.
  The reader does not touch them: #564's second acceptance criterion limits it to
  `usage` fields and timestamps, and a privacy boundary that bends for a
  convenient field is not a boundary. E1 is the price, paid openly.
- **E3 — Rebase rewrites committer dates.** A default `started_at` taken from
  the first commit can therefore start later than the work did, shrinking the
  window and pushing early sessions into the unassigned bucket. The registry's
  explicit `started_at` is the remedy.
- **E4 — Sessions and missions are not one-to-one.** One session can serve
  several missions and one mission can span several sessions. The session is the
  unit, so a session that served two missions is shared, never divided. There is
  no fair way to divide it from timestamps alone.
- **E5 — Main-thread transcripts can be missing.** Subagent directories outnumber
  root transcripts on this machine. A mission whose main-thread transcript is
  absent reports subagent cost only, and the report marks it
  `partial_main_thread` rather than reporting a small number as if it were whole.
- **E6 — Transcripts are pruned by the host.** A mission older than the
  retention window reads low rather than absent. The report carries
  `first_timestamp_seen` so a reader can tell a pruned window from a cheap one.
- **E7 — `usage` is what was billed, not what was useful.** Retries, discarded
  turns and abandoned branches of work are all in it. That is the right quantity
  for a cost campaign and the wrong one for a productivity claim.
- **E8 — Token counts are not money and not time.** The four kinds carry
  different prices and the harness applies none.
- **E9 — The 300-second idle bound is a convention.** It is not a measurement of
  attention. A different bound gives different `elapsed_seconds`; the number is
  registered here so that at least it is the same convention across every
  reading.
- **E10 — CI timings include queue time.** `gh` reports a run's start and end on
  shared runners, and queue depth is outside this repository's control. A CI
  comparison across windows with different runner contention is not a like-for-like
  comparison, and the campaign's own measurements of CI contention already show
  this varies.
- **E11 — The harness measures cost, never quality.** Nothing here says whether
  the work was any good. #563's C5 counter-metric exists for that reason and is
  not this method's business.

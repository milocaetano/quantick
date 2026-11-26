# Read-cost ledger

What each merged pull request asked a reader to hold: the production lines of
the Rust files it changed, plus the production lines of the files those
directly reference. `tools/read_cost/measure.py` is the calculator and owns
what counts; this directory is only its record. Direct references, never a
transitive closure.

One file per merged pull request, `<pr>.md`, holding that pull request's row
under the table heading. Two pull requests that each record a row therefore
never touch the same path, and a maintenance PR never conflicts with another.
This README holds the prose and the ceiling; no generated row is ever written
here. Readers take rows only from files named by a pull-request number, in
number order.

Rows are written by the tool from merged `origin/main` history, never by the
feature PR. Fetch main, then generate a batch for a separate maintenance PR:

```sh
git fetch origin
python tools/read_cost/ledger.py backfill --repo . --limit 30
```

`--limit` bounds the number of first-parent main commits inspected; raise it
to cover a longer gap. Only PRs whose merge commit is in that history and
whose target was main are recorded. The calculator uses each merged PR's
recorded base/head, so squash merges retain the full change. Existing row
files and this README, with its ceiling, are never rewritten; reruns do not
duplicate rows. Lookup or measurement failures exit nonzero without writing
a partial batch.

On main pushes, `ci.yml`'s existing `read-cost-ledger` job publishes the
updated directory as `read-cost-ledger-<sha>`. Retain the artifact through a
maintenance PR when needed; CI never pushes to main. Missing ledger rows
produce no PR reminder or readiness check and require no post-review commit.

The first 30 rows were backfilled through the same calculator, over the last
30 merged pull requests at the time — campaign children included, which is why
the `Base` column exists.

`Read cost` is `production_lines`. `Changed` is how many production Rust files
the diff touched. `Top referenced` is the largest file the change pulls in by
reference alone — one it never edits and a reader still has to open. `Date` is
the merge date for generated rows; historical branch-written rows retain
the date they were originally measured.

## The ceiling

<!-- read-cost-ceiling:v1 18000 -->

**18,000 production lines.**

The median of the `feat/` and `fix/` rows in the backfill, rounded to the
thousand: 28 feature rows, median 18,242, so 18,000.

It is a warning and never a block. `pr-gate` prints it, with the files the
branch pulls in by reference alone, when a feature branch goes over.
`python tools/read_cost/ledger.py median` recomputes what the rows suggest
today; moving the recorded number is a person's decision, in its own pull
request, because a ceiling that follows the rows is a description rather than
a bound.

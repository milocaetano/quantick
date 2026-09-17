# Incremental package-test evidence

This tool measures editing cost. It does not assign either Quantick score.
The frozen [production lexer](../outside_score/measure.py) selects the actual
three largest crates at an exact clean commit, sorting by descending production
lines and then crate name. Within each crate, the largest production `src/`
module is the representative touched file (path breaks a tie). Test files and
test-only items are excluded by that same lexer. No historical crate list is
used as a fallback.

## Protocol

Use Python 3.11 or newer. Run from a clean checkout containing the committed tool. The source checkout,
new detached benchmark worktree, new target root and new evidence directory
must be disjoint. All three new paths must not exist; nothing is cleaned or
deleted. The exact 40-character source commit is **H**. A later candidate or
fixed-budget commit is **E**, not a newly measured runtime.

```powershell
python tools/edit_loop/measure.py run --repo . --sha FULL_COMMIT_SHA `
  --worktree C:/bench/run-001-worktree `
  --target C:/bench/run-001-targets `
  --output C:/bench/run-001-evidence `
  --budgets tools/edit_loop/budgets.json
```

Use a host leased for this experiment, not a trader's active build target.
The workflow uses the same command on a GitHub-hosted Windows runner. Each
selected crate gets a new compile target, one exact-command warm-up, one
no-touch control, then **five** mtime-only touches followed by the full
`cargo test -p PACKAGE`. Every sample must prove that package recompiled and
tests actually ran successfully. All five durations remain visible; the
summary reports their median, minimum and maximum, not the best selected run.
These are end-to-end Cargo process wall times including package tests, not
Cargo's compile-summary time and not cold-build costs. The two-second pause
before each touch and compiler-contention observations are outside that time.

The pinned toolchain and committed test/dev profiles are unchanged. Parallel
build jobs are explicitly two. The Cargo registry/download cache remains
shared and is labelled; the crate compile targets start empty. Global or
ancestor Cargo configuration may contain `build.jobs`, which the explicit
environment overrides; any other configuration is rejected until its effect
has reviewed support. Compiler/profile/cache environment overrides are also
rejected. `QUANTICK_BUBBLES` is normalized only for these processes to the
tracked fixture; other `QUANTICK_*` overrides are refused. No trader config,
credential, token or complete environment dump enters the report.

Allowlisted OS, CPU/memory/disk, runner image and toolchain identities are
retained. Compiler/cache processes are observed before and after each Cargo
invocation; observed contention fails the series. These boundary snapshots
are **not** proof of exclusive physical hardware or absence of transient
background work. Hosted VM variability is part of the declared host class.

The canonical guard report is captured separately in an untimed setup target.
Its production physical-line counts include lines the frozen lexer excludes,
such as comments and blanks. They are a cross-check, not an interchangeable
ranking or a second score. The selected crates must exist in those counts.

## Fixed budgets and failure evidence

`budgets.json` starts explicitly **uncalibrated**. The initial hosted run
collects all valid samples and then fails the budget gate; it is calibration
evidence, never a passing scheduled check. Download its entire artifact and
inspect it against the source commit available in Git:

```powershell
python tools/edit_loop/measure.py propose C:/bench/run-001-evidence/report.json --repo .
```

The proposal does not modify budgets. The reviewed provisional rule is
`ceil(1.25 * max(all five baseline samples) + 5 seconds)` per selected crate:
25% operational runner headroom plus five seconds, rounded upward. Confirm
this rationale against the actual retained series, then commit the fixed
proposal through review. Do not recalibrate from each checked run, discard
slow samples, relax the rule to hide a failure, or substitute smaller crates.
The same workflow must then pass against that committed baseline.

```powershell
python tools/edit_loop/measure.py check C:/bench/run-002-evidence/report.json --repo .
```

`check` reads budgets from the checkout's committed HEAD, not an arbitrary
replacement JSON file. A host-class, pinned toolchain, jobs, root profile,
top-three or representative-source change requires explicit reviewed
recalibration. The fixed baseline also binds the runner/frozen-lexer protocol
hash, so an executable measurement change cannot silently reuse old limits.
Missing, invalid, nonfinite, failed, timed-out, uncompiled,
unrestored or over-budget data fails. Proposal/check also require measurement
age under 30 days and validate raw file hashes, the complete required input
hash map, commands, touch sequence and raw test/compile observations. They
recompute the entire ranking at H in a newly owned temporary materialization
of validated Git blobs. They execute none of that archived code; export
omissions/substitutions, links and escaping archive paths are refused.

The manifest hashes every artifact including the report. Each crate preserves
original source bytes, saved metadata/recovery state, warm-up/control/all
completed samples, stdout/stderr, exit/timeout status and process timings.
Partial series retain their failure and recovery records. The workflow's
always-upload step retains failures and itself fails if artifacts are absent.
The weekly schedule can execute only after the workflow reaches the default
branch; a candidate PR run is not evidence that cron already ran.

Keep raw measurements, manifests, recovery records and one-run reports outside
Git, in temporary directories and CI artifacts. The PR and linked issue carry
the concise result, exact source SHA, UTC date, host/protocol identity, hashes
and artifact links. Preserve original failed artifacts when a repair produces
a newer run; an artifact's age or failure is not permission to rewrite it.
Only reusable runner/checker code, fixtures, protocol documentation and the
reviewed fixed budget contract belong in source control. The budget contract
retains the minimal calibration identity, rationale and values needed to
enforce it; it is not a renamed raw-evidence archive. This follows the
[delivery contract](../../docs/workflow/delivery.md#keep-execution-evidence-out-of-git).

## Source safety and interruption

Source bytes must equal H before touching. Symbolic/hard links, junctions, escapes and
unexpected replacement are refused. An already-open regular-file handle
changes only mtime; finally verifies bytes, file identity and permissions,
then restores atime and mtime. Unexpected edits are never overwritten.

On Windows, a waiting helper is assigned to a no-breakaway, kill-on-close job
before it can start Cargo. The supervisor terminates that job and requires
zero active processes before restoration. A successful Cargo parent exit
with surviving children is a failed sample, not a passing shortcut. The
helper's startup/ownership setup is outside the Cargo stopwatch.

Raw process records also retain bounded Windows owned-job snapshots before
termination and after accounting reaches zero: PID, verified membership,
image, creation/exit FILETIME and process-handle wait status. For completed
commands these read-only diagnostics follow Cargo's stopped stopwatch; a
timeout remains a failure, never an inferred duration. Observations never
replace the result or accounting/orphan decision. A vanished/recycled PID remains explicitly
unresolved; it is never a PID-based kill target. Incomplete diagnostic queries
fail closed. No process name is an exemption for surviving descendants.

On Linux, the helper remains a session leader and subreaper after Cargo exits,
so orphaned descendants do not disappear merely because their parent exited.
Cleanup targets only that owned group. If descendants are still active when
cleanup starts, POSIX escape races prevent a whole-tree proof: the tool
conservatively refuses restoration even after best-effort group termination.
Unexpected session/group escape or a compiler of unknown ownership observed
after the sample also refuses restoration. No unknown process is killed to
manufacture quiescence. These cases retain recovery evidence and fail.

A hard-killed runner cannot execute `finally`. A remaining `running` process
record or `touched`/`restore_refused` recovery state is incomplete evidence,
not a resumable or passing measurement. Do not overwrite its source or reuse
its targets to manufacture a full series. Preserve the isolated worktree and
original/recovery records; inspect the recorded owned PID and descendants
before any manual metadata recovery. Start a new measured series with new
paths after the cause is resolved. This tool never deletes those artifacts,
stops an unrelated process or claims a killed run was restored.

## Offline regression suite

```sh
python -m unittest discover -s tools/edit_loop -p 'test_*.py'
ruff check --select F tools/edit_loop/
```

Fixtures exercise selection, isolation, Windows metadata handles, concurrent
edits, partial series, owned subprocess timeout, raw-evidence corruption,
budget drift and workflow ordering. Their fake Cargo outputs are test inputs,
never timing evidence. Actual pinned-Cargo recompile proof comes from the
retained five-sample experiment.

References: [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html)
and [GitHub schedule events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule).

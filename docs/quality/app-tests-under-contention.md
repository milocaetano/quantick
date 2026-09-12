# App tests under CPU contention

The Linux CI job runs the `quantick-app` test binary a second time, as many
concurrent copies sharing a few cores, right after `Test`. The step is
`App tests under CPU contention` in `.github/workflows/ci.yml`; the harness is
`tools/ci/contention.sh`. Issue #407 added it (campaign #367, child Q9).

## Why

`Test` runs every test once on an idle runner. A test that assumes a worker
thread keeps up with the thread asserting on it passes there, merges, and goes
red later on an unrelated pull request. Four such tests (#361, #364, #385,
#403) arrived that way in sixteen hours. Each fix reproduced its failure the
same way: many copies of the test binary sharing one or two cores. This step
runs that harness in the pull request that adds the test.

## What it does

1. Finds the binary with `cargo test --workspace --no-run
   --message-format=json`: the `Test` step's own invocation. After that step
   it compiles nothing. A single-package selection (`-p quantick-app`) could
   resolve different features and rebuild.
2. Reads `tools/ci/contention-known-issues.txt` and turns each line into a
   `--skip <name>` under `--exact`.
3. Starts 24 copies at once, each under `taskset -c 0-3` and a 360-second
   `timeout`, from the package root (where cargo runs a test binary).
4. Fails if any copy exits non-zero, times out, or ends without a passing
   summary. For each failed copy it prints the copy number, the failing test
   names, and libtest's failure report. Then it prints a count of copies per
   failing test and one GitHub annotation per test. The next step uploads
   every copy's log as the `contention-logs` artifact.

Each copy keeps libtest's default `--test-threads`. The default is the size of
the affinity mask, so each copy runs 4 tests at once: 96 test threads, plus
their worker threads, on 4 vCPUs.

Reproduce on Linux from the repository root, after a `cargo test --workspace`:

```sh
sh tools/ci/contention.sh                      # 24 copies on 4 cores
CONTENTION_COPIES=12 CONTENTION_CORES=2 sh tools/ci/contention.sh
```

Other knobs: `CONTENTION_TIMEOUT` and `CONTENTION_LOGS`, plus
`CONTENTION_KNOWN_ISSUES` to point at another skip list. Exit status is 0 when
every copy passed, 1 when any copy failed, and 2 when the harness could not run.

## Choosing 24 copies on 4 cores

All figures come from the 4-vCPU `ubuntu-latest` runner, on throwaway
push-only branches (`tmp/q9-proof-*`, deleted afterwards). *Before* is
`f36cc022`, the parent of #404's fix, where #403's
`the_trade_paint_layer_switch_stops_the_marks` is still unfixed. *Tip* is the
campaign tip `52e35a1b`. The two trees differ in the app only in that test.

| Copies × cores | Runs (before / tip) | #403's test caught before its fix | Copies' wall time |
| --- | --- | --- | --- |
| 20 × 1 (#404's harness, 1 test thread per copy) | 1 / 1 | 0 of 1 | 329–450 s |
| 8 × 2 | 1 / 1 | 0 of 1 | 96–131 s |
| 12 × 2 | 1 / 1 | 0 of 1 | 145–198 s |
| 16 × 3 | 1 / 1 | 0 of 1 | 201–224 s |
| 24 × 3 | 2 / 2 | 0 of 2 | 305–337 s |
| 16 × 4 | 2 / 2 | 0 of 2 | 100–199 s |
| 20 × 4 | 3 / 3 | 1 of 3 | 125–234 s |
| **24 × 4** | **9 / 9** | **3 of 9** | **154–300 s** |

So the harness needs every copy to be parallel as well as outnumbered. One
test thread per copy (20 × 1) never caught it, and neither did 2 or 3 cores.
With 4 cores, 24 copies caught it in 3 of 9 runs and 20 copies in 1 of 3,
about the same rate on samples this small.
`CONTENTION_CORES` is 4 because that is all the runner has, so there `taskset`
only states the mask. On a larger machine the copies are still held to 4
cores. Over eighteen runs, each whole step at 24 × 4 took 2 min 34 s to 5 min 0 s:
at the five-minute target, not comfortably under it. 20 × 4 took 2 min 6 s
to 3 min 54 s. The step keeps 24 because more copies is more contention on
every run, and because the extra minute stayed within the target in all
eighteen; if the target tightens, 20 is the measured fallback.
`timeout-minutes: 8` caps the step.

It catches a load-sensitive test often, not always: about one run in three
for #403's. One green run is not proof that a test is immune to load. The step
raises the odds that such a test fails in its own pull request, where before it
only failed later, on an unrelated one, and every run that does fail names it.

The proof runs (the skip-list logic, #403's test never skipped):

- Before, https://github.com/milocaetano/quantick/actions/runs/34710577586:
  red in 1 of 3 jobs at 24 × 4. It named
  `app::tests::layers_tests::the_trade_paint_layer_switch_stops_the_marks`
  at `layers_tests.rs:565`, the CI assertion #403 recorded. It was also red in
  1 of 3 jobs at 20 × 4.
- Tip, https://github.com/milocaetano/quantick/actions/runs/34710578514: that
  test passed in every job. One job failed on a test found by the step itself,
  now #413 (below).
- The committed script with its CI defaults, four jobs each:
  before, https://github.com/milocaetano/quantick/actions/runs/34712034171,
  and tip, https://github.com/milocaetano/quantick/actions/runs/34712035367.
  #403's test did not fail in these four, and each revision had one job red on
  a further new test, now #415 and #416. The failing job's
  `contention-logs-2` artifact holds all 24 copies' logs.

## The skip list

Calibration found seven more load-sensitive tests. Their sources are identical
at the campaign tip. Fixing them is outside #407, so the copies skip them.
**`Test` still runs every one**, so nothing leaves the ordinary suite. Each
line in `tools/ci/contention-known-issues.txt` is an exact name and its issue.
The harness refuses a line without an issue, and a name the binary no longer
has. The PR that fixes a test removes its line.

At landing the list held:

| Test | Issue | Reproduced at |
| --- | --- | --- |
| `control_plane_tests::gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed` (`:935`, observer-read revisions) | #408 | 24 × 4 and 16 × 4 |
| `control_plane_tests::attaching_a_script_and_detaching_it_leaves_the_pane_as_it_was` (`:3135`) | #409 | 16 × 4 |
| `control_plane_tests::a_keyed_call_that_expired_before_the_application_saw_it_leaves_its_key_free` (`:5435`) | #410 | 24 × 4, and locally on Windows |
| `control_plane_tests::gateway_rejects_a_duplicate_request_id_while_a_wait_is_parked` (`:2702`) | #411 | 24 × 4, and locally on Windows |
| `screenshot_evidence_tests::synthetic_fractional_geometry_round_trips_original_png_and_clipped_regions` (`:92`) | #413 | 24 × 4 |
| `control_plane_tests::an_operator_cannot_detach_the_traders_own_indicator` (`:2910`) | #415 | 24 × 4 |
| `control_plane_tests::a_bundle_with_a_screenshot_maps_every_named_control_to_a_region_of_the_image` (`:4231`) | #416 | 24 × 4 |

The file is the current list; this table is only the landing record.

## When the step goes red

- **A test your PR added or changed:** it probably assumes an ordering that
  load breaks. Make it force the ordering (a hold, a flush, or a wait for the
  state it asserts), as #382, #400 and #404 did. Do not add it to the skip
  list.
- **A test your PR did not touch:** the step has found another test from
  before this step existed. File an issue with the copy's output and tell the
  reviewer. The skip-list line citing that issue is a reviewed change, never a
  way to turn one run green.
- **A copy timed out:** the log shows tests libtest reported as running for
  over 60 seconds. Treat it like a failure.

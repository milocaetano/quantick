# F1 indicator operation dependencies

Pre-edit inventory at `a808b2d87b36d73041027e4d20c053544b454a96`,
branch `feat/indicator-operation-owner`, campaign #330 child #316.

## Inputs and origins inspected before editing

| Input | Origin and use |
| --- | --- |
| Script name/source | UI `add_script_indicator` reads `ScriptLibrary`; control `attach_script` deserializes and compiles before mutation. |
| Operator provenance | Control resolves recorded author before current actor; UI library passes human. Consent, dispatch, schema validation and journaling stay in control adapters. |
| Tab/side | Focus resolved by the app adapter; legend removal captures the pane where the gesture occurred. |
| Indicator views | Target `ChartPane.indicators` allocates local slot IDs and removes the render-side slot. |
| Worker command sender | Target `ChartPane.indicator_worker` receives Add/Remove; compile/replay remains worker-side. |
| Slot kind | `IndicatorState.slot_kinds` records `(TabSlot, SavedKind)` for persistence and layout order. |
| Operator ownership | `IndicatorState.operator_slots` is a set of complete `TabSlot` addresses, excluding human layout entries from remote removal. |
| File watches | `IndicatorState.script_files` is populated by UI library loading/restoration; removal drops only the target's watch. |
| Pending hidden/style | Layout installation queues values until the worker creates views; removal clears only the target's queued entries. |
| Layout book and membership | `mirror_add/remove` derives origin layout position, edits the saved entry, enumerates other panes on that layout and materializes/removes matching slots. |
| Script library during mirroring | Mirrored scripts are resolved by saved name, including the existing missing-source error slot. Source transport is not redesigned. |
| Save intent | `mirror_add/remove` and then `note_indicator_edit_at` both stamp the layout store today; the store coalesces them into one dirty bit and debounce clock. |

Other callers: native add calls `mirror_add` then `note_indicator_edit_at`;
layout materialization uses silent operations; UI legend removal and control
operator detach converge through `remove_indicator_at`. Slot cleanup is duplicated
in `remove_indicator_silently`. No other caller uses `mirror_remove`.

The adapter keeps focus, library/filesystem handling, layout orchestration and
the final save stamp. The extracted operation only receives the target's
indicator host and the five slot-bookkeeping collections above. Library,
settings, polling clock, presets, app, workspace, feed, paper and rendering
state are excluded.

## Compatibility inventory

Numeric slot IDs restart for each pane. Existing `TabSlot` carries tab, side and
slot, and must remain intact through mutation/cleanup. v1 detach accepts only
the local slot number: it picks the first operator-owned matching entry in
`slot_kinds` insertion order, refuses human-only matches and reports absent
numbers as false. Two operator panes with the same number are therefore
ambiguous. This is an existing limitation, not a new guarantee of exact-target
wire addressing. A follow-up can introduce a versioned owner-qualified detach
input with explicit stale-target behavior while retaining v1 resolution.

Control attach precompiles and refuses invalid Pine without mutation. UI library
attach deliberately reserves a slot and reports worker compilation failure on
it, so users can repair/reload the file. The campaign explicitly requires both
behaviors preserved. No compile policy or script execution budget changes.

## Rate and measurement plan

Attach/detach, ownership lookup, mirroring and save marking are rare command
paths. No added per-frame, per-trade or per-depth work. Existing synchronous
control compilation and asynchronous worker compilation/queueing stay unchanged;
the optional attach-latency measurement is not triggered. A bounded gateway
queue does not bound one handler's execution time.

Use the unchanged `control_idle_dense_replay_benchmark`: 8,000 seeded trades,
30 warm-up frames, 600 measured frames, 64 trades/frame. Preserve the baseline
test executable before editing, build the candidate, then alternate three
same-host pairs. Builds and competing benchmarks do not run during collection.
Inspect a median CPU-average increase above 10% rather than declaring a pass;
report all samples and tail variance. This fixture does not toggle access modes,
measure GPU rendering, request load, untrusted-script budgets or attach latency.

Raw command logs and frozen baseline executable:
`%TEMP%/quantick-f1-indicator-operation-owner/`.

## Delivered boundary and regression evidence

`app/indicator_operations.rs` introduces `IndicatorSlots`, which borrows exactly
five collections from the existing `IndicatorState`. `IndicatorHost` supplies
only `add(IndicatorSource)` and `remove(SlotId)`, using the existing ChartPane
allocation and worker command path. `ScriptAttachment` returns the full target
and an optional human layout entry. A fake host runs the same production
attachment, legacy target resolution and cleanup with three colliding slot 0s.

The old mutation methods had unrestricted `&mut QuantickApp`. The extracted
operations cannot access focus, settings, polling, script library, workspace,
paper trading or feeds. The app adapter still orchestrates human mirroring;
this is a dependency reduction for mutation, not a complete layout/session
ownership redesign. Immutable `IndicatorViews::all()` remains available for F2.
No root field, action-registry change, control DTO or domain-crate edge was added.

Both UI library and control attachment retain `attach_script_indicator` as their
adapter into the extracted implementation. Human removal and silent mirror
removal reuse its complete-target cleanup. Human mirroring still happens before
origin removal. The redundant intermediate dirty stamps in `mirror_add/remove`
are removed; their callers already stamp at operation completion, retaining
the same debounce policy and one pending save. `LayoutStore::mark_changed`
increments no public revision. Native add also retains its final existing stamp.

Five focused regressions pass in `focused-tests-fixed.log`:

- `fake_hosts_exercise_production_ownership_and_complete_target_cleanup` checks
  human refusal, absent IDs, same-side/different-tab and same-tab/different-side
  collisions, v1 insertion order, every cleanup collection and missing-host cleanup.
- `human_script_operations_mirror_two_panes_and_save_once` drives the toolbar
  library action and removes from the nonfocused pane, checking both workers,
  layout content and one pending/consumed save intent for each mutation.
- `operator_script_operations_preserve_human_mirrors_and_saved_content` drives
  registered agent actions, preserves both human panes and the complete saved
  layout book, and checks refused human removal produces no save intent.
- `invalid_control_script_leaves_both_panes_layout_and_save_intent_unchanged`
  checks human-origin and agent-origin registered calls with existing content.
- `invalid_library_script_retains_mirrored_error_slots_for_repair` loads a broken
  real file through the toolbar, retaining both error slots, file watches and
  its human layout entry.

Existing control ownership/catalogue/schema tests remain in place. No file under
`schemas/`, no wire DTO and no fixture content changed. This extraction does not
claim to solve first-match v1 ambiguity, source portability or script preemption.

## Same-host measurements

Environment: Windows `x86_64-pc-windows-msvc`, Intel Core i7-11800H, 8 cores /
16 logical processors, rustc 1.98.0 (`88d9e12ae`), cargo 1.98.0 (`797e8a9bc`).
The repository optimized test profile was used without an override. This is
a different host from the historical F0 numbers; only the pairs below compare
control and candidate. `environment.txt` records compiler details and full hashes.
Lockfile SHA256 is
`02D505E112DCCE244B11627708DA2815C6068402997A5C0D805B0E483D1C6721`;
unchanged benchmark source SHA256 is
`BA8E22DFAC51FD0C2A9B195A3F170AB95D86C3AB019BB45543203E013F03B0A8`.
The baseline test executable was built at the full base SHA above before source
edits; measured candidate source is the runtime/test content at
`2c50fcd810ae9858300c92c0a4a33e73edbdd8d9`.
`benchmark-binaries.txt` records both executable hashes. The PR records the
candidate commit SHA, avoiding a self-referential commit hash in this document.

Frozen test executable SHA256 values:

- Baseline: `E4DDDDD40FEF41E4FFB774FD9A9F3189DAE9AE1F5816C3127C2D87A54442188E`.
- Candidate: `A08282D70A638F12B293E191B7D1C137138BB3A7F71552A46F391E9B3310CD31`.

Build command: `cargo test -p quantick-app control_idle_dense_replay_benchmark --no-run`.
Each frozen executable runs `control_idle_dense_replay_benchmark --ignored
--nocapture --test-threads=1` from the same worktree. Pair 1 runs baseline then
candidate; pair 2 reverses that order; pair 3 returns to baseline then candidate.
The initial baseline observation (1.406533 ms average) is retained separately
and is not counted as an extra pair. No builds ran during timed collection.

| Pair | Variant | CPU average ms | CPU p99 ms | CPU worst ms | Trades/s |
| --- | --- | --- | --- | --- | --- |
| 1 | Baseline | 1.396238 | 2.385300 | 3.002600 | 45782.611 |
| 1 | Candidate | 1.401550 | 2.508900 | 2.772500 | 45609.292 |
| 2 | Candidate | 1.335180 | 2.302100 | 4.326500 | 47873.041 |
| 2 | Baseline | 1.317612 | 2.393500 | 3.601500 | 48507.743 |
| 3 | Baseline | 1.396554 | 2.491700 | 5.351400 | 45774.010 |
| 3 | Candidate | 1.515662 | 2.670400 | 3.509500 | 42173.828 |

Median CPU average changes from 1.396238 to 1.401550 ms (+0.38%). Individual
pair averages change by +0.38%, +1.33% and +8.53%; tails vary in both directions.
The predefined 10% median investigation threshold is not crossed. These are
fixture observations with host noise, not an improvement claim or proof about
GPU/access-mode/request-pressure performance. The synthetic arrival timestamps
are not interpreted as live latency. Compilation and queueing did not change.

## Structural and verification record

Guard report: app production lines 109944 -> 110013; module-cycle measurement
remains 3 (the existing baseline), headless findings 0, blind scans 0. More lines
were added than removed; the benefit is restricted dependencies and shared
cleanup, not a line-count reduction. Actual production edits are `app.rs` (one
module declaration), `app/indicator_manager.rs`, `app/layout_wiring.rs` and new
`app/indicator_operations.rs`. Tests add one registration and one focused file;
this dossier and the mission archive complete the diff.

Worktree arming passed before edits: guards build and app all-target check.
Every edit batch passed repository guards. One E0624 repair kept focus resolution
inside the existing adapter instead of widening a private layout helper. One
test-fixture repair selected the actual nonfocused pane rather than assuming
the split fixture starts focused on Flow. Both original failure logs are retained.

The mandatory ordered verification logs are `fmt.log`, `clippy.log`, `build.log`
and `test.log` in
`C:/Users/camil/AppData/Local/Temp/quantick-f1-indicator-operation-owner/`.
The ordered verification loop passed with these results:

| Command | Exit | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | Formatting matches. |
| `cargo clippy --workspace --all-targets` | 0 | Workspace and all targets pass. |
| `cargo build --workspace` | 0 | All workspace crates build. |
| `cargo test --workspace` | 0 | 3458 passed, 0 failed, 12 ignored across Rust test summaries; app 1908 passed, 4 ignored. |

The first full test attempt failed two unchanged app tests:
`observer_core_capture_stays_within_the_ui_budget` measured best median 264 us
against 250 us, and `gateway_a_client_that_never_reads_does_not_stall_another`
received a non-success response in its live-client assertion. The original
`test-initial-failure.log` is retained. Individual frozen baseline/candidate
probes passed both tests (capture best medians 75/80 us); the frozen baseline
full app suite also passed (1903 passed, 4 ignored). A baseline defect was not
demonstrated. The first unchanged full ordered-loop retry passed at default
concurrency. No budget, timeout, assertion or concurrency setting was changed.
This is a recorded transient result, not evidence to waive exact-head CI.

The final archive/dossier update receives the same ordered verification before
commit; no runtime source changes followed the measurements. Independent
AI/architecture/delivery reviews and exact-diff markers belong to the coordinator
after this implementation/mission archive commit; this dossier does not claim
a review of its own branch. The PR base is explicitly `campaign/architecture-a`.

## Phase-two save assertion repair

PR #332 [AI-review thread](https://github.com/milocaetano/quantick/pull/332#discussion_r3945209724)
identified a timing-sensitive operation test: after real worker flushing,
`take_save(Instant::now())` can correctly return `Write` when compilation or a
scheduler pause outlasts the 1,000 ms debounce. Repair attempt 1 checks
`is_dirty()`, consumes one `take_flush() == Write`, verifies the cleared flag,
and requires a second flush to return `Wait`. These checks read no clock, so
worker delay cannot change their save decision. No sleep or clock injection is
needed. The existing explicit-clock tests
`a_change_inside_the_debounce_window_is_not_yet_asked_for` and
`a_change_that_has_settled_is_asked_for_once` retain the before/at-boundary
debounce evidence; the existing exit-flush test retains immediate flush evidence.

Only this operation-test helper and this dossier change after the measured
candidate commit. Production source, runtime policy, performance fixture,
budgets, timeouts and concurrency settings are identical to that commit. The
paired measurements above are retained as evidence for equivalent production
source, not described as a benchmark of the repaired test executable.

Repair verification uses new logs in the same evidence directory:
`fix-guards.log`, `fix-focused-tests.log`, `fix-layout-store-tests.log`, and the
ordered full loop `fix-fmt.log`, `fix-clippy.log`, `fix-build.log`, `fix-test.log`.
`fix-verification-exits.txt` records the full-loop exit codes;
`fix-source-equivalence.txt` records the changed-file and unchanged-source audit.
Earlier failure logs and retry history remain intact. Fresh CI and independent
reviews of the repair commit remain the coordinator's responsibility.

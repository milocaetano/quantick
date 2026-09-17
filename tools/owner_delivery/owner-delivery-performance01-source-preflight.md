# Owner delivery performance 01: source preflight

Final bounded verdict: suffix02 and proposal02 resolve PF-01/PF-02 below. The corrected suffix follows the real clicked release-frame path and is suitable for final export/identity review; there are zero open concrete suffix findings. This is a bounded read-only source preflight, not a benchmark result, score, build/materialization grant or sampling release. The combined candidate commit and its exports do not yet exist in this review.

## Inputs inspected

- `owner-delivery-performance01-proposal.md` and `frame-performance-preflight-independent-review.md`.
- `owner-action-measurement-suffix01.rs`, independently verified SHA256 `bd862c0a4f9f8a3fa7243ff9f9c5c77b769143f20657db3ab71f2387ce7b6e11`.
- Actual baseline `80da32426e6e2f7a2a962ab8054d986897461b6d` test helpers, drawing vocabulary and allocator through git show, plus current I2 worktree source for comparison. The current worktree is not represented as the eventual combined candidate.

No source edits, builds, tests, samples, native operations, remote operations or score actions were performed. This external report is the only new preflight artifact.

## Required correction before freezing the suffix

PF-01: The suffix's emitted output contains `symbol` and a Flow label, but omits the feed/venue. The proposal promises market identity; a symbol alone is not that identity. Add actual `app.active_tab().feed_id` beside the actual symbol in the output. `feed_id` exists with the same public field name at baseline and current source. A literal Flow label is justified here by reading/asserting directly from `flow_pane.drawings`, rather than inferring the pane from a user selection. No new production API is needed. Root must freeze and hash the corrected suffix; the reviewed suffix01 hash must not be mislabeled as the corrected bytes.

PF-02 (claim precision): `work_meter::realloc_copy_bytes` and `largest_realloc_copy` count the smaller old/new allocation sizes. They are possible copy-byte upper bounds, not measurements of bytes the system allocator actually copied; a growth in place can copy nothing. The JSON field names are the existing honest API and may stay. Clarify proposal/report wording that calls these actual copy counts. Allocation observations are only the calling test/UI thread; they do not count asynchronous worker allocations. The existing timing test binary also uses this allocator, so separate timing/allocation invocations do not remove observer cost. The proposal already acknowledges the latter correctly.

No other concrete source/API incompatibility was found in this inspection. Compilation has deliberately not been attempted, so compatibility is a source judgment, not a compile receipt.

## Real action and observation boundaries

The baseline helpers implement secondary press, move, secondary release; an existing settling frame populates the floating action rectangle. The suffix then reads the registered enabled control, checks the Flow collection is empty, sends the real primary press frame, resolves the current rectangle again, and prepares the release event vector. This matches `click_quick_range_action` rather than directly invoking a control handler or the new conversion plan. Profile, Retracement and Projection tool IDs and 2/2/3 anchor expectations match the baseline tests.

Both branches enter the same `run_frame_with_events` helper for the primary release. That helper constructs RawInput and calls ctx.run/app.draw_frame, so the metric includes the complete release frame, not isolated conversion dispatch. Timing stops before metric JSON creation and semantic assertions; FullOutput remains alive until after output recording. Allocation mode resets the largest counter and snapshots the same thread before the same helper, then snapshots immediately afterward. The prepared event vector lies outside both observed intervals. JSON, printing, fixture teardown and FullOutput destruction lie outside them. No measured code chooses a faster substitute path based on the branch.

Every observation checks exactly one drawing, expected tool and anchor count, fractional bar locations, Human attribution, cleared quick range, no refusal toast and Projection's repeated final anchor. It records all actual bar/price bit patterns and timestamps. Tool output is emitted from the expected constant only after equality with the actual drawing tool is asserted. Omitted fresh drawing IDs are a justified exclusion. The external analysis must compare these semantic records, including the corrected feed identity, across baseline/candidate without normalizing anchor geometry or timestamps.

## Helpers and workload preservation

The baseline app_with_history uses a new per-app scratch home, 200 deterministic trades, Tick1, live-strip off, and the shared 1400x900 test window. Current helpers preserve these semantics, but are not byte-identical: test_app/app_with_history now delegate through default AppLaunch helpers and new_with_workspace. Any final report claiming unchanged helper bytes would be wrong. Baseline constructor hook reads and candidate default launch must be compared under the same sanitized scenario environment. Existing scratch isolation remains per app; do not replace it with a shared measurement home.

The inspected work_meter and session_length_tests source are unchanged from baseline. Dense still uses the existing 8000-trade, 30-warmup, 600-frame, 64-trades/frame fixture. The long session fixture exercises actual CVD/lane worker frames, so it is relevant to I2's batching/publication/retired-fold path as well as ordinary app frames. It does not cover arbitrary Pine programs, source compile/reload latency, all indicator families, all feature-enabled scenarios, GPU latency or native/control correctness. Dense alone does not prove indicator-session or clicked conversion costs. These coverage limits must remain explicit instead of being converted into an empty benchmark pass.

## Fixed counts and execution stops

Each action invocation has 10 checked warmups plus 100 retained observations for each of three actions: exactly 330 emitted records, including warmups. Six invocations per mode yield 1980 records (180 warmup and 1800 retained), with actions compared separately. Timing and allocation modes each use B1,C1 / C2,B2 / B3,C3. Six dense invocations retain all existing summaries. Long session uses one complete baseline and one complete candidate invocation. There is no adaptive stopping, trimming, best-of rerun or revised threshold.

The proposal preserves the dense greater-than-10% median CPU-average investigation trigger without treating lower shifts as automatically accepted. Action has no invented numeric allowance; consistent adverse movement or uncertain tails stays unresolved. The within-session 1.50 timing ratio is not a candidate allowance. Literal allocation/fold/lane/session budgets remain unchanged. Runtime assertion failure, overlap, missing records or failed budgets stops collection and preserves the first failure and preceding observations; preparation compile failure is not a timing sample.

## Final identity checks still required

Before materialization/build release, bind the reviewed clean combined commit, actual baseline commit, corrected suffix hash, both exported source manifests, and precise append-only overlay manifests. Recompare the named helper semantics and source dependencies on those exports; verify every product byte outside the suffix matches its commit, no helper/assertion/budget edits, and all frozen measurement definitions retain their pinned blobs. Local crate additions mean Cargo.lock need not be byte-identical; external package versions and effective compiler settings must be reconciled explicitly.

Before collection, preserve both executable/PDB identities and the full build settings/toolchain/features/profile. Exact default optimized test and release-long binaries must be distinguished. Capture the sanitized nonsecret environment and idle process proof before each pair; no compiler, competing benchmark or native automation may overlap. The external runner/parser must enforce exact selectors, --ignored/--nocapture/--test-threads=1, 330 action records per invocation, action/mode/index/warmup uniqueness, semantic equality, finite order and stop conditions. Those runner/parser artifacts have not been supplied or reviewed here. Root's finite execution release remains mandatory; this report cannot substitute for it.

## Bounded delta closure: suffix02 / proposal02

Independently verified `owner-action-measurement-suffix02.rs` SHA256 `e34b57b032e8d88cb68b85606b7f479b97446db5f5cb93595a053dff9013340a`. A direct source diff shows exactly one logical change from suffix01: actual `app.active_tab().feed_id` is emitted beside symbol. It occurs in the post-observation JSON record and does not change the timer/tally interval, fixture, checks, count or ordering. PF-01 is resolved. Original suffix01 and the observation above remain traceable.

Read `owner-delivery-performance02-proposal.md`: it references suffix02, requires feed_id-plus-symbol comparison, explicitly describes realloc copy metrics as upper bounds, and requires semantic helper/default-AppLaunch/scratch comparison rather than assumed wrapper byte equality. PF-02 is resolved by the explicit refinement; use that precise upper-bound language in results. No other protocol change loosens the fixed budgets, finite counts or stop conditions.

All final export/commit/helper/environment/executable/runner identity checks listed above remain required. No exports, executable, PDB, parser or collection result were reviewed. This closure authorizes no materialization, build or sample; root retains the next finite release decision.

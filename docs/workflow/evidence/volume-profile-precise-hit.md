# Precise Volume Profile pointer capture

Mission: `.claude/GOAL-archive-volume-profile-precise-hit.md`, high tier.
Base: `origin/main` at `d3d4b23d3741ec82204b2c0f2c77b23a98908ac4`.
Raw records: [evidence directory](../../../.claude/evidence/volume-profile-precise-hit).

## Behavior and regression proof

The previous hit test accepted the whole profile-width strip across its price
extent. It therefore intercepted empty buckets and space beyond short rows.
The profile now shares its projected rows and silhouette segments between paint
and selection. Filled rows use their individual widths; outline interiors pass
through. POC, value-area and range lines retain a maximum three-logical-pixel
pointer tolerance. Labels remain informational.

The generic handle shortcut also needed a tool policy: otherwise an invisible
profile handle could still intercept a pointer that missed the precise body.
The profile exposes handle targets only while selected, bounded by the painted
circle/ring plus the same tolerance. Other tools retain their existing default.
This necessary adjustment implements R1/R2 without changing their gestures.

The tests in `drawings/fixed_range_profile/tests/precise_hit.rs` cover row width,
missing buckets, line visibility, empty cache, outline-only interiors and
staircases, mixed filled/outlined regions, reversed anchors, developing right
edges, chart clipping, inverted scales, minimum-pixel row height and invisible
handle targets. `before-tests.log` records three failing geometry regressions
on main before the repair. The early gesture fixture also needed its deferred
bar specification applied before feeding trades; that fixture failure is not
claimed as proof of the product defect.

`app/tests/profile_pointer_tests.rs` sends real egui pointer events through a
200-bar fixture. A complete press/drag/release in empty profile space pans the
chart, preserves both profile anchors and leaves it unselected; a subsequent
empty click clears selection. A painted-row drag moves both anchors and shows
the move cursor; a visible-handle drag changes only the endpoint and shows the
resize cursor. Price round trips use a 0.00001 tolerance for screen precision.

## Validation identity

Windows, Rust 1.98.0 (`88d9e12ae`, 2026-08-18), Cargo 1.98.0, workspace test
profile (optimized with debug information), unchanged `Cargo.lock`.
Validation runs in `C:/src/quantick-worktrees/fix-volume-profile-precise-hit`.
The ordered command logs are `fmt.log`, `clippy.log`, `build.log` and
`workspace-tests.log`. Source hashes and the verified tree are recorded in
`validation-identity.json` before evidence-only archival.

The successful test environment explicitly sets `QUANTICK_BUBBLES` to this
worktree's `config/bubbles.toml` and `RUST_TEST_THREADS=2`. The inherited host
variable pointed at a personal preset: two existing order-flow projection tests
failed even alone. `isolated-orderflow.log` preserves that failure;
`isolated-orderflow-stock-config.log` records all 23 order-flow view tests passing
with the repository preset. No order-flow code or personal configuration was
changed. `first-workspace-tests.log` and `personal-preset-workspace-tests.log`
preserve the earlier failed workspace runs. `build-locked-qa.log` preserves a
Windows executable lock resolved by closing this task's QA window.

## Performance

Only per-frame geometry/paint/hit paths and gesture handling change; no
per-trade, per-depth or session-fold code changes. The existing capped profile
ladder supplies the rows. Screen-to-price bounds select a BTreeMap range before
projection, including overlapping one-pixel rows. Silhouette traversal uses the
same clipped paint geometry; hit traversal allocates no segment buffer.
The cached POC supplies the maximum row volume for both paint passes.

`precise_profile_frame_benchmark` runs 2,048 deterministic rows, 200 egui frames
per batch, six batches, both paint passes and one pointer test per frame. The
same fixture ran against main before implementation (`baseline.log`). The first
repair still scanned every row for pointer Y and measured slower
(`full-scan-benchmark.log`); that result prompted the bounded lookup above.
Final measurements are recorded in `after-benchmark.log`: median of all six
batches is 351.44 microseconds/frame versus main's 415.42, a 15.4% reduction.
The full-scan intermediate median was 461.94 microseconds/frame. These runs
share the same host and fixture; other worktrees may compile concurrently, so
the numbers describe this measurement rather than a general speed guarantee.
This is a CPU fixture comparison, not a claim about GPU or live-feed latency.

## Visual QA

Existing `QUANTICK_FRVP_DEMO`, `QUANTICK_FRVP_DEMO_SELECT`, `QUANTICK_POINTER`,
`QUANTICK_INVERTED` and replay hooks reach the affected states. All stores and
the replay fixture are isolated under `target/profile-qa`; the user's running
windows and configuration are untouched. Two synthetic replay days each contain
6,000 trades at 10 ms intervals; repeating prices are 60000, 60000, 60000, 60020,
60050, 60080, 60100, 60090, 60060, 60030, quantities cycle 1 through 7. The chart
uses tick:10, candle width 14, one day of backfill and paused playback for stable
capture. Selected captures use the shipped bubble preset and dense footprint;
restored captures hide footprint, bubbles and tape in the isolated layer store
to expose individual profile rows. Restored layouts are unselected.

Each capture first discovers its own process with `quantick_describe`, reads
scene and diagnostics, then captures chart, health, cursor and scene controls
with a screenshot. Exported chunks and the reconstructed bundle were SHA-256
verified. The `*-bundle-metadata.json` records retain capture revision, evidence
ID, scene, health and the PNG hash; `*-quantick_get_diagnostics.json` retains the
separate diagnostics. PNGs stay local under the evidence directory according to
the repository screenshot policy. A private descriptor copy only points the
client at this task's own process; no other application was controlled.

| Affected surface/state | Verdict and evidence |
| --- | --- |
| Profile selected, dense chart, 1400 x 900 logical points | PASS: `selected-normal.png`; painted rows, range/VA/POC lines, both handles and selected context bar remain visible. 59.99 fps, 16.67 ms wall frame, 3.47 ms CPU. |
| Profile selected, dense chart, 1000 x 700 | PASS for profile geometry/handles: `selected-narrow.png`; reduced width preserves the same targets. 58.30 fps, 17.15 ms wall, 4.94 ms CPU. |
| Profile restored/unselected, 1400 x 900 | PASS: `restored-normal.png`; visible row widths and gaps agree with the profile, no handles or selected context bar. 59.98 fps, 16.67 ms wall, 1.65 ms CPU. |
| Profile restored/unselected, inverted scale, 1000 x 700 | PASS: `restored-inverted.png`; histogram, POC and value-area lines reverse with the price axis, gap space remains visible. 59.99 fps, 16.67 ms wall, 1.18 ms CPU. |
| Final executable, restored/unselected, 1400 x 900 | PASS: `restored-final.png`, capture revision 8, evidence `YCu0mHIvs5f1evlhyFQBHw`; bounded row lookup preserves the rendered shape. 60.12 fps, 16.63 ms wall, 2.29 ms CPU after startup settled. |

Scope limits: this is an input-geometry repair; captions, toolbars, disabled
controls and popup placement are unchanged. The dense narrow screenshot also
shows crowded pre-existing chart/footer captions, outside the changed profile
geometry; the table does not certify those captions. No new popup or disabled
state is introduced. Empty-cache and transparent-outline pointer ownership are
proved by headless geometry assertions, not claimed as screenshot observations.
Paused captures prove stable geometry, not live motion. The bundle explicitly
reports that pixels precede projections by one drain and that some controls lack
bounds; it is not represented as complete same-frame coverage.

Integrity: the changed rows, lines and handles stay inside the canvas clipping;
readability/consistency: existing colors, text sizes and stroke styles remain;
occlusion: selection introduces no new popup or canvas region; state honesty:
the profile status continues to label inferred side and the contributing range.
The final row-range optimization preserves the projected geometry; regression
assertions validate that evidence reuse after the optimization.
The final executable was also captured after that optimization. Its initial
startup sample included a one-second loading frame while the workspace compiled;
the settled capture above and its diagnostics are the visual result, not that
startup measurement.

## Trader UX review

Flow: pan through a profile, intentionally move it, then resize it.

- Rafa: an empty-space drag pans in one gesture without catching a nearby
  profile; a deliberate row/line grab still moves it. No added dialog or focus
  interruption. The CPU fixture and recorded frame diagnostics cover latency.
- Marina: row/line selection still exposes handles; a body drag translates the
  range, a handle drag changes its endpoint, and a restored layout keeps the
  profile geometry. The inverted-axis fixture checks consistent gestures.
- Duda: the cursor distinguishes move, resize and empty chart; visible handles
  explain the resize target. A missed profile grab affects chart navigation,
  and there is no new preference to find or irreversible action.

No Blocker or Should-fix in the changed flows. Rafa can navigate through the
profile; Marina retains explicit move/resize and layout restoration; Duda can
use the visible shape and cursor to identify the target. Evidence is the
pointer-event tests plus the selected/unselected captures above.

## Review and delivery destinations

The current PR receives the independent architecture report (medium direct bug
pass plus all shape dimensions), six-dimension AI report and full independent
delivery report. Those gates, final-head CI and the final mission reconciliation
are pending until their canonical producers publish their receipts. The user
alone merges to main.

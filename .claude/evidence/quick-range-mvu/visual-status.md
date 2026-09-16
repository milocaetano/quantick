# Visual/UX status — partial current-source evidence

## Independent cell review, 2026-09-15

Read-only reviewer visual_final_cells inspected source, pixels, annotation
responses, scenes and diagnostics at 1b4bad7e. No new launches, code or
score assessment. Raw artifacts remain outside Git under
C:/src/quantick-worktrees/outside-eight-coordination/mvu-visual/.

| Cell / case | Verdict and evidence |
| --- | --- |
| profile-shared-operation-1 | Bounded PASS: admitted v2 profile creates drawing 1 on flow pane 0, slots 54–69; selected handles/context bar appear and zero quick_range.* scene controls remain. Evidence ThLm3gRwvsh74BWNxN7a4g. This is shared annotation operation plus selection clearing, not a captured mouse conversion gesture. |
| foreign-pane-selection-1 | Bounded PASS: profile belongs to time pane 1, slots 4–5 while flow remains focused; selected time-pane handles/context bar appear, no temporary range toolbar remains. Evidence 81YIvp15tIzhvaiSkHm7wA. |
| profile-shared-operation-1/capture-motion | Observed motion PASS: trades 163→381, flow bars 83→87, time bars 6→7, revision 167→385; identical canvas rectangles. FPS 60.000484→59.999969. Evidence IHh6lMWaT_bw5F2AAJBv5g. Actual capture interval 6.850s, not the requested approximately 1.2s: that cadence is unproven. |
| ready-narrow-1 target actions | PASS geometry: three available actions at physical x=585/633/681, y=397, 48x48, within 1500x1140 image. Evidence tMGR6uvJO3oodnlG_0xCHA. |
| ready-narrow-1 status bar | FAIL, MVU-V1, Should-fix: overlapping counts/provenance/performance at approximately physical x=790–1000, y=1110–1130. No deferral accepted. |
| profile-shared-operation-1 selected context bar | FAIL, MVU-V2, Should-fix: profile icons overlap heatmap legend around physical x=938–1520, y=172–204 in both captures. No deferral accepted. These widgets have no recorded scene bounds. |

The source paths behind MVU-V1 (statusbar.rs, app/health.rs, app/frame.rs,
theme.rs) are unchanged against pinned main a6644bc6. The same-row drawing
has no reserved width between provenance/content and right-hand machinery;
the fixture's long venue name consumes that budget. For MVU-V2, unchanged
context-bar placement reserves chart/drawing/live-lane geometry but not the
legend. Both are strong baseline mechanism evidence, not matched baseline
pixel reproduction. Neither observed FAIL is waived or called fixed.

The bundles omit interaction.selection and analysis.drawings. The visible
clearing proof uses the action response, handles and zero quick_range scene
controls, not direct model-state fields. One-drain projection skew, missing
non-target bounds and original-pixel/DPI limits remain in every verifier report.

Earlier root-inspected cells also remain in the same scratch root:
active-normal-1 (held ruler, no action buttons), future-normal-1 (honest future
anchors, all three actions, zero live appends), chart-menu-narrow-1 (expanded
menu and unavailable heatmap reason), empty-ticket-narrow-1 (SIM guidance,
disabled buy/sell, no orders), layouts-stacked-rename-1 (three distinct
pane-local strips, addressed rename), replay-fixture-normal-1 (34-print
repository replay, no invented closed bars or depth), and isolated-fib-
retracement-1 / isolated-fib-projection-1 (one real registered annotation each).
The empty/all-drawing-demo Fibonacci attempts were not accepted as clean
target proof; all original failures and cluttered captures are retained.

Persona status: Rafa has observed advancing tape and stable layout, but no
unconditional performance verdict. Marina's selected-profile controls fail
legibility and full sharing/restart/gesture proof is not complete. Duda sees
explicit SIM/inferred/unavailable labels, but MVU-V1 obscures status details
and MVU-V2 obscures controls. This is not an overall trader-UX PASS.

Remaining: repair/reproduce MVU-V1/V2; complete the still-missing stale range,
indicator-band selection, exact default dense preset, all-charts toggle and
stacked-resize interaction cells plus prescribed motion cadence. Existing
headless/integration tests complement but do not impersonate those captures.
Independent full PR reviews, CI and final verifier remain mandatory.

## First current-source capture and retained history

Current source: 1b4bad7ebb257d0c0a016db6994338592b367bda.
On 2026-09-15 at 06:25 UTC, root captured the ready range at 1440x900
logical points / 2160x1350 pixels with a synthetic F1TEST feed on isolated
port 19501. Owned PID 41300 was closed afterward; trader PID 2552 was left
untouched. All writable stores for that launch point to its scratch case.
No desktop input was injected. Desktop idle before launch was 2388422ms.

Scratch evidence: outside-eight-coordination/mvu-visual/ready-normal-2/capture/
(screenshot.png, bundle.json, manifest.json, scene-before.json,
diagnostics-before.json, verifier-report.json and protocol receipts).
Evidence ID ftPPfYVwp4NMWwXuuRl9Gg; revision 3; PNG SHA-256
84233b8358dbf2c4cc070f43fed0192310256971311a19bda7792501399a4a21.
Integrity/source verifier: PASS, with its explicit one-drain projection skew,
21 missing non-target bounds and original-pixel/DPI limitations retained.
This verifier result is not a complete visual/UX PASS.

Structured ready-state observation: quick_range.fixed_range_profile,
quick_range.fib_retracement and quick_range.fib_projection are available,
32x32 logical-point bounds at x=544/576/608, y=318. Pixels show the matching
three-button range toolbar clear of the tape and forming bar; the ruler and
both pane-local layout footers are visible. No clipping of those targets was
observed. Frame health: 59.469887 FPS, CPU average 4.363824ms, CPU worst
23.6068ms. Synthetic historical timestamps truthfully display stale/offline;
depth is unavailable/connecting, not fabricated. This is neither real-market
evidence nor a paired performance result. Dense marks and clipped unrelated
footer text still need baseline comparison before a whole-surface verdict.

The complete matrix and trader persona flow review remain pending. No score
or PR readiness is claimed. Earlier capture failures and the original desktop
pause below remain retained, not replaced with success.

## Earlier desktop pause

The ui-harness skill's guest-on-desktop rule was checked before any launch.
GetLastInputInfo succeeded and reported16ms idle: active desktop input.
No validation application was launched, no input injected, and no trader
window, settings or paper journal changed. Visual QA and trader UX have no
PASS verdict. Headless frame tests do not substitute for these reviews.

Remaining capture matrix: ordinary default, held/ready/future range,
stale/unavailable range, all three conversion actions, persistent selection
on another pane/indicator band, tape lane versus empty future chart space,
context menu over live data, narrow1000px and normal1400px, pane layout/rename
footer, all-charts contextbar toggle, stacked-pane resize, ticket/replay.
Use the feature-enabled fresh executable, controlled dense fixture/preset,
isolated stores and explicit PID; read scene/diagnostics before pixels.
Capture permission/desktop availability must be re-evaluated before resuming.

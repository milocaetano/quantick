# F1 visual QA: preliminary findings, not final acceptance

Reviewer: campaign coordinator, not the F1 implementation author. Skills: ui-harness, visual-qa, trader-ux-review. Desktop warning guidance was limited to textual/pattern redundancy, readable contrast and compact-label overflow; no financial sampling or UI redesign advice applied.

Runtime: isolated copy of F1 executable SHA256 8322C178258A0FE1A72B59B393BF78DC8266DDD3A6B321704B5F5AA91F42135C, built from source tree c8365c0503e7001a7d11c14ad65ed3d2ac85f3b9 before later additive catalog regeneration. The build reports application_commit unknown; executable hash and source-tree evidence provide provenance, not that runtime field. This is not an outside-score or quantick-score assessment.

Fixture: synthetic F1TEST MT5 socket on 127.0.0.1:19474, explicit fixture configuration and tracked bubble preset, isolated cockpit stores and paper journal. No order was submitted, broker launched or user session changed. The normal window was 1440x900 logical points at display scale 1.5. The owned process was PID32840. It was closed cleanly after recent mouse input halted driving, and its original runtime descriptor disappeared. The private copy of that owned descriptor was deleted; no token was printed or retained in evidence.

Evidence directory: C:/src/quantick/.git/worktrees/fix-feed-gap-honesty/evidence/runtime/qa-normal/.

## Observations

- Empty state: empty-state.ndjson diagnostics reports about 59.99fps, no tape and book connecting. empty-dpi.png shows explicit fixture not-connected/offline state and connecting placeholder. Full-window geometry is intact. The earlier empty.png was cropped by DPI-unaware capture coordinates and is NOT visual acceptance evidence; capture-owned-window.ps1 was corrected to request per-monitor DPI awareness.
- Short-gap structural proof: short-state.ndjson reports one tape gap from1789398407400 to1789398407500, duration100ms; feed_integrity anomalies1, missing_messages3, non_monotonic0, unknown_loss0. Source state says connected. The deliberately old synthetic timestamps truthfully render stale/offline attention rather than fresh market data.
- Motion: short-motion1.png and short-motion2.png were captured in the same shell call with a1200ms interval plus process-start overhead. Retained live count advances651 to715, forming state and bubbles advance, layout remains stable. Structured diagnostics reports60.012783fps and about5.54ms mean frame CPU for this fixture. This is not a claim about every dense real-market case.
- Capture manifest 3UbS5pYLSIfjI5y00b4MTw: digest sha256:c7a23fb963658d68ed2bcaa78f5cc14f528b464f1e73f76c74ed50343a059a3b, capture revision11, seven chunks. Image metadata2160x1350 physical pixels, pane.0.canvas and feed_status.chip within_image true. Coverage explicitly incomplete, including screenshot.state_skew (pixels precede projections by one drain) and missing chrome bounds. Only the manifest was retained; the in-memory bundle expired on closing the owned app, so no downloadable canonical bundle is claimed.
- Initial MCP discovery timed out after30s in a runtime directory containing96 entries; the cause was not attributed to F1. A private ACL-protected directory containing only the verified owned descriptor made discovery and pinning deterministic. The first evidence request used an unregistered ui.scene scope and correctly failed; corrected scene.controls request succeeded. Neither failed request is passing evidence.

## Open findings

F1-V1 (Should-fix): draw_feed_gaps calls stall::spoken_ms, which floors100ms to0seconds and renders "0 s gap" for the newly retained short-gap range. This is a new reachable-range presentation defect, not a request to change global stall formatting. Add dedicated accurate gap formatting with subsecond/equal-time tests.

F1-V2 (Should-fix): the existing top-center book-sync loading overlay partly covers the new gap caption in the flow fixture. Keep the dashed seam and its caption associated while placing the caption outside existing loading/footer/time-axis regions. Confirm normal and narrow geometry, with actual paint-shape tests and fresh screenshots. The unsupported-book loading behavior itself predates this diff and is not silently classified as an F1 regression.

Rafa can see uninterrupted tape motion but needs a readable gap warning; Marina needs the short gap accurately named in history; Duda must not infer no missing data from a rounded-zero caption. No persona receives a full-flow PASS yet.

## Pending matrix

Exact short-gap caption after repair; equal-timestamp gap; reconnect gap; narrow window; normal no-gap control; affected menu/disabled-state reconciliation. New captures stopped when GetLastInputInfo reported active mouse use. Continue independent code/CI work and resume captures only when the desktop is idle. Do not request manual trader validation or label unobserved states PASS.

## Caption-repair attempt, 2026-09-14 20:03-20:06 UTC

Executable quantick-app-caption.exe SHA256 8CD1D369A9A855B24AF5E7335505D916F4AE80766BB64E92364502C6B7560EBC, built at tree73f4e4f6e58b9e41dfab8093a7c909ad32597c48 before the R1-base rebase. Isolated PID2896, instance CWzSetd9-C8A0KopLwb_yw, normal1440x900 fixture. Fresh empty.png captured at2182x1406 after12.593s idle; describe/short-state structured replies succeeded and remain in runtime/qa-normal-caption1. GetLastInputInfo refused the subsequent short-motion1.png capture on recent user activity, so that file does not exist and this attempt does not close F1-V1/V2. No caption screenshot or full matrix PASS is claimed. The owned app closed cleanly; only the verified owned synthetic sender25648 was stopped and the private descriptor copy deleted. Other instances and user stores were untouched. The in-memory evidence bundle was not downloaded before closure; only its manifest is retained.

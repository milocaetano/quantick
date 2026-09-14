# F1 independent visual and trader-flow review

Reviewer: campaign coordinator, not the F1 implementation author. Date: 2026-09-14. Skills used: ui-harness, visual-qa, trader-ux-review; UI styling guidance was limited to readable, redundant warning text and compact-label overflow. This is a bounded affected-flow review, not an outside-score or quantick-score assessment.

## Identity and evidence reuse

The successful candidate matrix ran executable SHA256 `8D9A1A30299B269CBCD4E9E7A7EF24A5D92AD6518A94A77E2A536C70DAEEC3FC`, built from combined campaign-base source tree `a8cebfcffb26dc8a384bcc840895c8b2854c17e2` at F1 commit `4c394f63dc8fba5e68e221f3d680feb2e20bb8b3`. Base: `05bc95ecfa36339be75411b251edf67cffa79992`.

Current clean F1 commit `5ad75391c50ce855c576ab6eb2c8bdf96fe764ef` has final tree `248d5824834a6dad9a4ee2253750680f7f7166c4`. Its validated runtime tree `5dd1f20b76e3d5076375b49ab98776b4496994ac` changes only six renderer lines after the matrix input: documenting/naming `GAP_CAPTION_BOTTOM_CLEARANCE_PX = 3.0 * SEAM_LABEL_PT` and replacing the identical inline expression. The source reviewer independently inspected that complete delta and found geometry, fonts, colors, state and behavior unchanged. Remaining delta is mission/evidence documentation. The ordered final runtime validation reports 3910 passed, zero failed, 21 ignored. These older-binary pixels are explicitly reused for the nonsemantic constant repair; they are not relabelled as a new execution. A final-executable optional smoke attempt was refused by the idle guard before launch and is not evidence.

The popup control ran R1 executable SHA256 `81AED0A4AF304C992F8D5237D5DBAC6ED6C3980FDBF7FD803A750241FA7C5DBB`, whose runtime inputs match the campaign base. Its source is R1 runtime `f23ecd52757697df5a572d08cbb9cf988660ab4b`, tree `000af3d138925af15efdd3070840ccf6d5629fb4`; the campaign merge adds no runtime delta. This is an actual control run, not a historical screenshot asserted equivalent.

Paths below are relative to this report's directory. Files are byte-for-byte copies of owned fixture outputs under the private F1 worktree's `evidence/runtime/`. The evidence manifest records hashes. No descriptor, bearer token, executable, user trading store or unrelated window is included.

## Fixture and capture discipline

Synthetic F1TEST MT5 socket on 127.0.0.1:19474; isolated config, layouts, symbols, journal and app state; tracked bubble preset. Four thousand historical ticks followed by moving synthetic prints. No order, real broker or market connection was operated. Normal viewport 1440x900 logical points; narrow 1000x760; scale 1.5. Full outer-window PNGs include chrome, whereas MCP screenshot metadata describes client pixels.

The launcher checked at least 30 seconds of desktop inactivity; each PrintWindow capture checked at least one second. Only verified owned app/sender processes were closed, and private owned descriptor copies were deleted. Normal and narrow candidate captures were inspected directly. Two-frame pairs are separated by approximately 1.2 seconds plus capture overhead, not an exact benchmark interval.

MCP describe preceded reads. Snapshots, diagnostics, semantic scene and capture manifests were collected using an observer with explicit evidence/screenshot scopes. No mutation was sent through MCP. The source intentionally supplies no depth; the book-loading placeholder is existing behavior. Historical fixture timestamps remain honestly marked stale/offline. Popup cells use the existing explicit feed-popup/stall fixture hooks; these are UI test states, not measured real outages.

## Observed matrix

| Cell and files | Structured evidence | Direct pixel result |
| --- | --- | --- |
| Empty normal: `qa-normal-combined-short3/empty.png`, `empty-state.ndjson` | No live tape before sender launch | Empty/connecting state; order controls dimmed; intact viewport. Startup FPS is not a dense-case performance result. |
| Short normal: `qa-normal-combined-short3/short-motion{1,2}.png`, `short-state.ndjson` | Gap 100 ms, bounds 1789398407400 to 1789398407500; one anomaly, three missing messages, zero non-monotonic/unknown-loss counts; 59.99411 fps, 3.617675 ms mean frame CPU | Live count 136 to 178. Second image has readable `100 ms gap` in the lower chart band, clear of top loader and footer. First image has no caption yet: existing renderer anchors to closed bars; it does not prove instantaneous marking. |
| Equal-time narrow: `qa-narrow-combined-equal1/equal-motion{1,2}.png`, `equal-state.ndjson` | Both bounds 1789398407400; duration 0 ms; one anomaly/three missing messages; 59.994209 fps, 3.276571 ms CPU | Count 133 to 175; `0 ms gap` remains readable and associated with the seam, despite equal timestamps. No invented positive duration. |
| Reconnect normal: `qa-normal-combined-reconnect1/reconnect-motion{1,2}.png`, `reconnect-state.ndjson` | 10200 ms gap, bounds 1789398407400 to 1789398417600; one anomaly, zero known missing messages, one unknown-loss event; 59.999577 fps, 3.296053 ms CPU | Count 129 to 172. Caption `10.200 s gap` becomes visible with the closed-bar anchor in the later frame; no assertion that the unknown loss equals a known number of lost trades. |
| Recovery popup: `qa-normal-combined-popup1/short-motion{1,2}.png`, `short-state.ndjson` | 100 ms/three missing messages; 60.02092 fps, 3.459204 ms CPU; feed chip selected, reconnect/reload controls present | Count 137 to 181. Later caption remains visible left of popup. Existing Reload/Reconnect labels and the warning that reload closes paper positions and disarms strategies remain readable. No extra popup introduced by F1. |
| No-gap narrow control: `qa-narrow-combined-normal1/normal-motion{1,2}.png`, `normal-state.ndjson` | Optional `tape_gaps` and `feed_integrity` fields absent, not fabricated zero-valued fields; 60.045322 fps, 3.00239 ms CPU | Count 127 to 169, stable layout; no yellow gap seam or caption. |
| Campaign-base popup: `qa-normal-control-popup1/short-motion2.png`, `short-state.ndjson` | Actual pre-F1 runtime control | Same popup placement and wording as candidate. The old runtime has no short-gap warning, reproducing the original omission. Existing popup can cover part of the live-price area in both versions; F1 does not create that overlap. |

The narrow footer has existing text crowding; this review does not claim globally overlap-free UI. The new caption is clear in the observed narrow state. Popup geometry and footer source are not changed by this patch. The top loader remains misleading for an intentionally depth-free source but predates this diff; no new absence-of-depth claim is made.

Semantic scene cross-check in the popup cell: `feed_status.chip` is selected; recovery controls are available in the UI, which is not a claim about MCP permissions. `toolbar.layers.heatmap` is unavailable with reason `source_captures_no_order_book`; its saved visibility remains selected. The disabled control therefore has a readable coded explanation, and the tape/depth distinction is retained.

The normal short-gap manifest is capture `JBWIdGmWu7J1_Uz3Xwpi7w`, revision 8, seven chunks, digest `sha256:7fff910118731a3b04b268386bfad24e91243b5e1491cd42103f0b4e82f6a143`. Client image metadata is 2160x1350; canvas rectangle (90,132,1770,1080) and feed chip (1995,1224,96,33) both report within-image. Coverage explicitly includes screenshot/projection drain skew and missing chrome bounds. These manifests were retained, not their ephemeral in-memory bundle payloads: no downloadable/openable canonical bundle or exact screenshot-to-projection frame synchronization is claimed. Raw PNGs are separate later captures.

## Finding disposition

- F1-V1, should-fix: FIXED, first repair batch. The original `0 s gap` for a 100 ms interval is retained in `preliminary.md` and `qa-normal/short-motion2.png`. Accurate dedicated gap formatting now agrees with structured 0/100/10200 ms evidence; coarse stall wording is untouched.
- F1-V2, should-fix: FIXED, same repair batch. The gap caption now uses the lower chart band. Normal, narrow, loading and popup pixels show it clear of the relevant chrome. Renderer tests additionally inspect actual text-shape bounds at normal and minimum geometry.
- F1-AR1: source reviewer closed the named-constant repair at final runtime tree; unchanged geometry permits the explicit evidence reuse above. Its source-review follow-up is separate from this visual verdict.

No new unresolved affected-flow finding from this matrix. Earlier invalid DPI crop, 96-instance discovery timeout, incorrect scene scope, recent-input capture refusals and the interrupted launch/capture experiments remain failed/incomplete attempts in `preliminary.md` and the private raw logs. They are not passing cells.

## Trader personas and regression scope

Rafa: PASS for the changed warning flow. Tape counts, forming state and bubbles keep advancing around 60 fps in this fixture; no added gesture, focus move or modal is required to disclose loss. The intentionally opened existing recovery popup is unchanged.

Marina: PASS for the changed history warning. A precise short/equal/reconnect interval stays attached to a visible seam once a closed-bar anchor exists; lower placement preserves the chart workspace. The UI does not fabricate backfilled prints or silently call a gap complete.

Duda: PASS for the changed warning semantics. `gap` plus explicit millisecond/second units does not rely only on yellow. Equal time is not presented as no loss. Unknown reconnect loss is not represented as a known missing-trade count. Stale/inferred labels and the existing reload-cost disclosure remain intact.

This bounded visual review does not exercise order entry or replay execution anew, place trades, claim all layouts/markets are fast, or discharge campaign score gates. Their regression obligations remain in the source tests, complete verification loop, exact-head CI and eventual independent assessment. The synthetic dense-tape fixture has no depth and is not a general order-book performance benchmark.

VISUAL-QA: PASS for the affected F1 warning flow, with identities, reuse and limitations above.
TRADER-UX-REVIEW: PASS for the affected F1 warning flow; no new open findings.

# Visual QA — temporary right-drag range

Scope: flow-chart canvas, ruler preview/readout, floating drawing chrome, the
forming-bar/live lane, the persistent FRVP drawing, tool rail, and chart state
with no data. Every image below came from the final executable identified in
`ui-harness.md`.

| Surface/state | Verdict | Evidence and observation |
| --- | --- | --- |
| Drag active, normal window | PASS | `active-final-a.png` (`5BB2F2D7…6BF5`): ruler fill, diagonal, `+26.82 pts`, `58 bars`, and elapsed time are legible; no premature action bar or persistent drawing is present. |
| Released, normal window | PASS | `ready-final-a.png` (`4DE468C8…BF8`): the same ruler remains and exactly one FRVP icon appears in shared floating chrome, outside the live lane. |
| Converted | PASS | `converted-final-a.png` (`EE8D4ED3…E5D5`): the ruler is gone; a persistent histogram with VAH, POC, and VAL plus normal drawing chrome occupies the selected interval. |
| Dismissed | PASS | `dismissed-final-a.png` (`A602D00F…1FC0`): neither transient ruler nor quick action remains and the chart geometry is unchanged. |
| Empty data | PASS | `empty-final-a.png` (`0AAE4DB2…F7CE`): the hook waits instead of inventing time anchors; the existing connection status honestly explains the empty chart. |
| Narrow 1000×700 window | PASS | `ready-narrow-final-a.png` (`D285CE45…D9B9`): preview, readout, and action remain wholly visible; tool rail and live lane are not clipped or covered. |
| Dense tape | PASS | `ready-dense-final-a.png` (`1EBAAC36…F167`): the blue selection and teal readout remain distinguishable over dense buy/sell bubbles; action chrome stays opaque and readable. |
| Live motion | PASS | `ready-motion-a.png` (`A3FFEEB5…950D`) and `ready-motion-b.png` (`801AC276…6E8D`), 1.2 seconds apart: different hashes and advancing tape/bar content with stable chrome placement and no layout jump. |
| Health | PASS | Final dense run: 59 FPS, 16.668 ms frame average, 9.600 ms frame CPU; no `APP_SLOW_FRAMES`, panic, or error line. Paired base evidence is in `performance.md`. |
| Structured live scene | BLOCKED | The pinned MCP adapter initialized but did not answer `quantick_describe` within 15 seconds. No live evidence ID is claimed. The frame-level scene test PASS verifies stable control ID, capability ID, bounds, and availability; see `targeted-tests.txt` and `ui-harness.md`. |

Defect checklist verdict: no clipping, overlap, off-window action, unreadable
number, price/live-lane occlusion, unexplained disabled state, inconsistent
widget grammar, layout jump, or attributable frame regression was found.
The live-adapter observation limitation is accepted for this PR because the
same semantic snapshot builder is exercised in the end-to-end app frame test;
it does not hide a product UI defect.

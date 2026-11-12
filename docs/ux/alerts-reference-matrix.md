# Desktop Alerts reference matrix

References are the 14 unchanged local files under `docs/alarm/`. The independent
visual audit is recorded in [issue 947](https://github.com/milocaetano/quantick/issues/947#issuecomment-6812991415).
Static reference pixels establish visible controls, not runtime behavior.

| Reference | Visible requirement | Desktop source / named test | Classification and evidence limit |
| --- | --- | --- | --- |
| image1.png | Selected trendline Add Alert | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Supported; tooltip alone does not prove interaction. |
| image2.png | Permanent right Alerts rail | `dock.rs`; `alerts_right_rail_entry` | Added to existing far-right strip; hidden with explicit Hide panels. |
| image3.png | Trendline Add Alert entry | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Same entry as image1, not a separate feature. |
| image4.png | Create, conditions, trigger, expiry, message, notifications | `surfaces/alerts/editor.rs`; `alerts_reference_workflows` | Supported; desktop tabs replace summary links. Unopened source/help menus are unproven. |
| image5.png | Five trendline operators | `alerts/operators.rs`; `alerts_reference_workflows` and alerts evaluator suite | Crossing/up/down, greater/less supported. |
| image6.png | Add value condition | `surfaces/alerts/editor.rs`; `alerts_reference_workflows` | Inline condition list replaces separate Add screen. Unopened target choices not inferred. |
| image7.png | Crossing variants, collapsed Show more | `alerts/operators.rs`; alerts evaluator suite | Visible variants supported; hidden choices unproven. |
| image8.png | Four trigger frequencies | `alerts/book_predicate.rs`; alerts evaluator suite | Once, per bar, close, minute supported; tests establish runtime semantics. |
| image10.png | Five expiry choices | `alerts/spec.rs`, `surfaces/alerts/editor.rs`; alerts contracts suite | Open-ended/day/week/month/custom supported; unopened calendar appearance unproven. |
| image11.png | Sidebar, search, sorting, active row, drawing bells | `surfaces/alerts/manager.rs`, `drawing_chrome/context_bar.rs`; `alerts_parity_surface_harness` | Added docked manager and selected-drawing bell/count. Bell placement follows selected drawing bounds; reference placement semantics unproven. Unselected drawings do not receive markers. Newest-first exists; unopened sort/ellipsis contents unproven. |
| image12.png | Row Pause/Edit/Delete, editor Save/Delete | `surfaces/alerts/manager.rs`, `editor.rs`; `alerts_reference_workflows` | Row Delete added, reuses existing explicit confirmation; reference does not establish confirmation behavior. |
| image13.png | Parallel-channel Add Alert | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Supported. |
| image14.png | Four channel operators | `alerts/operators.rs`; alerts evaluator suite | Enter/exit/inside/outside supported; unseen boundary selector not inferred. |
| image15.png | Six rectangle operators | `alerts/operators.rs`; alerts evaluator suite | Enter/exit/inside/outside/greater/less supported; precise geometry semantics require tests. |

The same open request is reachable through `alerts.open` and the rail. The
`dock.alerts` scene control reports selection; its bounds remain explicitly
unrecorded, consistent with other dock controls. Opening is idempotent and
requires cockpit authority. All definition decisions, cadence and expiry remain
in the headless alerts crate. Drawing attachment inspection is allocation-free
and bounded by definition and condition limits; price observation is unchanged.

Native capture and independent tester results must be attached to the delivery
revision before this matrix is used as completed visual evidence. Deterministic
paint/input tests are not native screenshots, audio-device verification, or OS
focus evidence. No cloud, mobile, arbitrary Pine, or hidden reference menus are
claimed. History timestamps are explicitly labeled Received UTC and agent schema documents
`utc_ms` as receipt/evaluation time, not replay market time. Source identity includes
live feed or replay recording. Issue #894 remains open for separate market-time
storage/display and replay-speed/seek tests; this change corrects the ambiguous
label without changing cadence, expiry, persisted field meaning, or old documents.

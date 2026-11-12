# Desktop Alerts reference matrix

References are the 14 unchanged local files under `docs/alarm/`. The independent
visual audit is recorded in [issue 947](https://github.com/milocaetano/quantick/issues/947#issuecomment-6812991415).
Static reference pixels establish visible controls, not runtime behavior.

| Reference | Visible requirement | Desktop source / named test | Classification and evidence limit | Native capture IDs |
| --- | --- | --- | --- | --- |
| image1.png | Selected trendline Add Alert | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Supported; tooltip alone does not prove interaction. | `sparse-dark-normal-manager/populated.png` |
| image2.png | Permanent right Alerts rail | `dock.rs`; `alerts_right_rail_entry` | Added to existing far-right strip; hidden with explicit Hide panels. | `matrix-dark-normal-manager`, `matrix-light-narrow-manager` |
| image3.png | Trendline Add Alert entry | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Same entry as image1, not a separate feature. | `sparse-dark-normal-manager/populated.png` |
| image4.png | Create, conditions, trigger, expiry, message, notifications | `surfaces/alerts/editor.rs`; `alerts_reference_workflows` | Supported; desktop tabs replace summary links. Unopened source/help menus are unproven. | `matrix-light-normal-editor`, `matrix-light-normal-editor-message`, `matrix-light-normal-editor-notifications` |
| image5.png | Five trendline operators | `alerts/operators.rs`; `alerts_reference_workflows` and alerts evaluator suite | Crossing/up/down, greater/less supported. | `matrix-light-normal-editor` (closed selector; MCP validates five operators) |
| image6.png | Add value condition | `surfaces/alerts/editor.rs`; `alerts_reference_workflows` | Inline condition list replaces separate Add screen. Unopened target choices not inferred. | `matrix-dark-narrow-editor` |
| image7.png | Crossing variants, collapsed Show more | `alerts/operators.rs`; alerts evaluator suite | Visible variants supported; hidden choices unproven. | `matrix-light-narrow-editor` (closed selector) |
| image8.png | Four trigger frequencies | `alerts/book_predicate.rs`; alerts evaluator suite | Once, per bar, close, minute supported; tests establish runtime semantics. | `matrix-dark-normal-editor`, `c5-granted/channel.png` (closed selector) |
| image10.png | Five expiry choices | `alerts/spec.rs`, `surfaces/alerts/editor.rs`; alerts contracts suite | Open-ended/day/week/month/custom supported; unopened calendar appearance unproven. | `matrix-light-short-editor` (closed selector) |
| image11.png | Sidebar, search, sorting, active row, drawing bells | `surfaces/alerts/manager.rs`, `drawing_chrome/context_bar.rs`; `alerts_parity_surface_harness` | Added docked manager and selected-drawing bell/count. Bell placement follows selected drawing bounds; reference placement semantics unproven. Unselected drawings do not receive markers. Newest-first exists; unopened sort/ellipsis contents unproven. | `sparse-dark-normal-manager/populated.png`; all manager matrix cells |
| image12.png | Row Pause/Edit/Delete, editor Save/Delete | `surfaces/alerts/manager.rs`, `editor.rs`; `alerts_reference_workflows` | Row Delete added, reuses existing explicit confirmation; reference does not establish confirmation behavior. | `matrix-light-narrow-manager`, `c5-granted/channel.png` (row actions; confirmation unobserved) |
| image13.png | Parallel-channel Add Alert | `drawing_chrome/context_bar.rs`; `alert_target_binding_end_to_end` | Supported. | `c5-granted/channel.png` |
| image14.png | Four channel operators | `alerts/operators.rs`; alerts evaluator suite | Enter/exit/inside/outside supported; unseen boundary selector not inferred. | `c5-granted/channel.png`, `matrix-light-normal-editor` (MCP validates four operators) |
| image15.png | Six rectangle operators | `alerts/operators.rs`; alerts evaluator suite | Enter/exit/inside/outside/greater/less supported; precise geometry semantics require tests. | `matrix-light-normal-editor` (shell only; MCP validates six operators) |

The same open request is reachable through `alerts.open` and the rail. The
`dock.alerts` scene control reports selection; its bounds remain explicitly
unrecorded, consistent with other dock controls. Opening is idempotent and
requires cockpit authority. All definition decisions, cadence and expiry remain
in the headless alerts crate. Drawing attachment inspection is allocation-free
and bounded by definition and condition limits; price observation is unchanged.

Independent native/agent results are summarized in the [delivery evidence](https://github.com/milocaetano/quantick/issues/947#issuecomment-6815306973).
Capture IDs above resolve within the retained external `native/` evidence directory;
folder IDs mean `capture.png`. Raw captures and structured responses remain outside Git.
All 30 combinations of manager, Conditions, Message, Notifications and History;
dark/light; and normal 1400x900, narrow 1000x760, short 1000x480 logical windows
passed independent native layout inspection with rail scene agreement. This is
layout evidence, not proof of clicking every visible control.

The tester used the copied app built from `611f75d9` (SHA256
`59CEB1A0FDA8A947F10BE152D4D58C7FF3CFFA276AEE25F0DD59C5686BEE1AF7`)
and final MCP from `d05cc234` (SHA256
`A0BFB247DDA34B23D2BB8EAE153F4BED89977AD225D8E11052B4681A4E7021A6`).
Only MCP inputs changed between those commits; app evidence is reused explicitly.
The app reports no embedded commit; build provenance and matching copied hashes
establish attribution. Actual shipped-MCP C5 passed all opt-in/host-grant combinations,
including denied-call data retention and successful deletion/history-clear retries
and readback. CRUD lifecycle, stale-revision refusal, exact replay source and
Delivered history passed. Four cadences configured successfully; full timing and
geometric firing semantics remain deterministic-suite evidence. Past-UTC expiry
was observed; unopened expiry choices remain contract-test evidence.

Four malformed/unavailable storage captures show truthful recovery guidance and
structured errors; correcting the same path and restarting restored data. The
matched manager-off/on replay fixture (20x, tick:1, 700 bubble budget, footprint
off, same two definitions) sustained 59-60 fps in both cases across eight matched
samples each; mean frame time was 16.66757/16.66758 ms. This establishes no observed
manager-induced slowdown for that fixture, not all presets or a comparison to main.

Supported CUA was attempted but its native pipe was unavailable (OS error 2), with
no apps/browsers returned. Native rail clicks, dock switching, Delete/Backspace,
keyboard focus, saved-editor confirmation clicks and dropdown expansion therefore
remain unobserved. Deterministic input tests cover those transitions separately.
Audible output and popup focus are unverified; delivery receipts do not prove them.
No cloud, mobile, arbitrary Pine, or hidden reference menus are
claimed. History timestamps are explicitly labeled Received UTC and agent schema documents
`utc_ms` as receipt/evaluation time, not replay market time. Source identity includes
live feed or replay recording. Issue #894 remains open for separate market-time
storage/display and replay-speed/seek tests; this change corrects the ambiguous
label without changing cadence, expiry, persisted field meaning, or old documents.

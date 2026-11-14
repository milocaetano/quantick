# Simulated trader review of alerts

Issue #860, 2026-11-10. This is independent simulated persona feedback using
the trader-ux-review personas, not market interviews or observed trader input.
The reviewer did not implement this surface. Source review and native rendered
captures are evidence of different things; neither proves native interaction.

## Evidence boundary

Inspected native captures under `.artifacts/alerts-ui-native`: light-normal
Conditions, dark-short Conditions, light-short Notifications, dark-short
Manager, and dark-normal History (`*-corrected/capture.png`). These identify
binary `5675196FB888ABDF5260DCD014FB14A33E0D9D7418F3DE7F724113D92E1D95DD`
as documented in the designer report. The manager capture predates the Rearm
correction. Reviewed current editor, manager, draft, domain operator and
mutation sources and the published alert schema. Native input was unavailable
after the documented retries; this reviewer did not attempt a helper or UI
Automation fallback. No mouse, keyboard, scrolling or focus result is claimed.

## Persona walks

**Create a price or drawing alert.** Rafa needs an exact threshold and a clear
bar-close versus intrabar choice without watching another panel. The captured
`100.0485` survives both value and summary, and the source includes Replay,
symbol and tick aggregation. The short editor retains Cancel/Create while its
body clips into a scroll region; reaching later controls still needs an input
test. Marina needs the chosen drawing to remain the same after changing panes:
the draft freezes its context, and the API schema carries durable composite
identity rather than a bare drawing number. This is a source/contract finding,
not a restart demonstration. Duda benefits from the fixed All conditions label,
its simultaneous-observation explanation, and the plain trigger descriptions.

**Choose delivery and keep trading.** Rafa can leave the default local toast,
with popup off and no sound, avoiding an unnecessary audible interruption.
Notifications visibly explains that monitoring requires this workspace open.
Duda therefore receives an honest local-monitoring limit. Source inspection
finds existing shared delivery integration; actual notification focus behavior
and sound preview remain unobserved. The independent dense-replay report found
59-60 FPS across its samples, with shared-host attribution limits; that result
does not establish manager-overlay or notification-burst performance.

**Manage, recover and audit.** Marina can distinguish source and aggregation,
Paused and Completed rows, and explicit suspension reasons in current source.
Rafa needs a completed once-only rule's action to mean Rearm: the old capture
still has the pre-correction action, while current source dispatches Rearm.
Duda can read the captured history's event price, UTC time, message, delivery
status and definition revision. A deleted rule retains event evidence in the
domain, but that does not restore an accidentally deleted live definition.

## Findings

- **Blocker, delete flow, all personas:** at initial review,
  `crates/app/src/surfaces/alerts/editor.rs` immediately emitted Delete from
  the edit footer, and `crates/alerts/src/book_lifecycle.rs` removed the
  definition. No confirmation or undo was present. A misplaced click beside
  Cancel permanently stops that monitoring. Concrete fix: an explicit inline
  confirm/cancel step before issuing the persistent command. Reported to the
  implementation owner. Follow-up source inspection finds an inline question,
  Keep alert with initial focus, and a separate Confirm delete command. This
  resolves the one-click defect in source; native keyboard/click verification
  and final rendered confirmation remain pending.
- **Should-fix, rectangle condition, Marina/Duda:** the dropdown offers
  Above upper boundary / Below lower boundary, but its selected label uses
  Greater Than / Less Than and the domain summary uses debug operator tokens.
  The boundary meaning disappears after selection. Keep the boundary wording
  visible for a selected rectangle comparison. Source-based finding; no
  rectangle screenshot was inspected. Follow-up source inspection finds one
  `operator_label` helper used by both selection and menu, preserving the
  boundary wording. The selected-label defect is resolved in source. The
  generic domain summary remains technical text, a polish consideration while
  the explicit selected condition supplies its meaning.
- **Acceptance gap, native interaction:** C11 creation/edit/pause/delete,
  invalid-input recovery, keyboard traversal, Escape nesting, scroll access,
  target Locate and notification focus remain unobserved. The unavailable
  input service is an evidence limitation, not a rendered defect or a pass.

Rafa: the captured layout and precision support monitoring; native focus and
interaction safety are not yet established.

Marina: source and durable-target contracts support her workflow; actual
workspace recovery and drawing interaction need their separate evidence.

Duda: the visible labels explain cadence and local scope; source corrections
address delete safety and rectangle wording, pending native interaction proof.

## Final artifact follow-up

Rechecked three actual captures from commit
`485a77bb802255c0dc420137e39997ac7fda1051`, immutable binary SHA256
`457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`:
`dark-short-toolbar-clearance-485a77bb`, `light-normal-editor-485a77bb`, and
`dark-short-storage-error-settled-485a77bb`, each `capture.png` under
`.artifacts/alerts-ui-native`. The repaired short toolbar leaves the inspector's
Lock/Delete controls clear and keeps Create alert visible to its left. Exact
price and summary still agree in the normal editor. The storage-error screen
plainly says unavailable rather than falsely empty: useful to all personas who
must know whether their alerts can be trusted.

**Consider, short storage-error view, Duda:** the lower toast obscures the end
of the draft-retention explanation. The unavailable heading and repair/restart
guidance remain visible, so this is a readability issue rather than a claim of
successful monitoring. Keep the final explanatory line above the toast lane
when this error state is next polished. No input behavior is inferred.

The final admitted consumer independently established safe same-connection
retries, stale-definition refusal, explicit removal authority, history retained
after definition deletion and cleared only by its separate command. See
`alerts-agent-consumer.md` for exact provenance and the MCP scope limitation.
This supports Marina's audit needs but does not demonstrate drawing reopening.
The final captures still do not render rectangle target switching or delete
confirmation. The source fixes above stand; C11 remains an explicit external
native-input evidence gap, and no persona receives an unconditional interaction
pass from this static assessment.

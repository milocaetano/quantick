# Local alert interface implementation

Issue: #860, parent #857. The screenshot source references remain in the maintainer's `docs/alarm` directory; raw native captures stay outside Git.

## Ownership

`alerts` owns definitions, predicates, cadence, expiry, status, immutable history, editor validation and the observation adapter. The adapter receives clocks, source identity, series identity and borrowed drawing facts. It caches only referenced targets, preserves committed-close identity, elects one source owner for mirrors and settles idle lifecycle checks after the complete pane census.

`drawings::alerts` converts the existing drawing geometry and verifies mirror provenance through the durable drawing key. Cosmetic revisions do not change semantic geometry revisions. `stores::alerts` persists before command success or notification delivery. The existing local admission layer remains the authority for capability scopes, persistent effects and idempotency.

The native residual accesses `ChartState`, `ReplayLink`, native time, `LayoutAdapter`, tab selection and existing popup/toast/audio surfaces. Editor and manager painting live in the registered alert surface. No predicate, cadence or expiration decisions live in the app. Frame delivery combines strategy and generic alert cues through the existing distinct-cue batch policy. The sink exposes only aggregate failure; every sound-bearing event is conservatively marked failed when any cue in that batch fails. Silent events retain their own delivery outcome.

## Review matrix

The independent review must cover the following behaviors on the final revision. This checklist is a plan, not a claim of completed acceptance.

- Value and trendline: crossing, crossing up/down, greater/less. Channel and rectangle: entering, exiting, inside/outside. Rectangle also supports above-upper and below-lower comparisons. ALL conditions only.
- Four cadences, every actual committed close, equal-time bars, and post-close trades sharing the correct bar budget.
- Never, end-of-day, week, calendar-month and custom expiry, with absolute timestamp and display timezone; no hidden historical notification after hydration, reconnect, seek or resume.
- Semantic geometry edits, cosmetic edits, idle removal/restoration, duplicate identities, known mirrors, owner disappearance and healthy mirror takeover.
- Global/menu, selected-drawing and manager creation; fixed source draft, subview preservation, validation, locate, pause/resume, edit/delete and immutable history.
- Light/dark, normal/short viewport, keyboard navigation and Escape nesting, reachable fixed footer, truthful status and non-focus-stealing notifications.
- Published schema/operator discovery by an independent consumer, actual admitted transport, authorization, persistent effects, retries and single journal receipts.
- Separate base/current dense-tape native measurements, active Monitoring readback, no-fire allocations, multiple-close and geometry/mirror fixtures.

Native captures, actual mouse/keyboard operations, socket calls, unit pointer injection and host-port fixtures must be reported separately. Unit rendering tests do not establish native visual or interaction acceptance. Final revision, review and CI evidence belong in the PR.

## Native residual and acceptance limits

The final source snapshot adds 1,423 production app lines over 93,463 (94,886
measured), 315 UI-free app lines over 18,396 (18,711), and 21 lines at the
QuantickApp extension root over 1,873 (1,894; extension total 2,517). The
branch-only sidecars exactly match these measurements. The native adapters
translate ChartState/ReplayLink/drawing selection, own Instant conversion,
route persistent commands and shared audio, and paint/input through egui;
source census, mirror election, geometry revision/cache policy and alert
predicates remain in headless owners. Independent HIGH review must verify
that boundary and the small harness-instruction compression.

Delete and history clearing advertise the dedicated sensitive
`cockpit.alert.delete` permission and irreversible alert-removal effect.
Payload `expected_revision` protects an alert definition; it is distinct from
the host envelope revision facility, which this implementation does not claim
to support for these actions. Delete requires explicit confirmation in the
editor. Completed rows offer Rearm; expired rows direct the user to edit
expiration. Rectangle comparisons retain their boundary labels. Automatic
drawing-toolbar placement accounts for the full action footprint and prefers
candidates clear of drawing bounds. Measured legend and open floating-inspector
rectangles stay clear of the toolbar, preserving inspector title dragging.
Manually parked coordinates are preserved through draw-time repairs.

C11 remains incomplete: the supported Windows input service was unavailable
(OS error 2) after documented recovery. No native mouse, keyboard, scrolling,
Escape or focus claim is made. Native captures, admitted socket operations and
unit pointer injection are separately identified in the companion reports.
When supported input becomes available, the remaining checklist is: create
from global/drawing/manager paths; edit invalid and valid fields; traverse and
scroll all short-window controls by keyboard; preserve drafts through subviews;
Pause/Resume/Rearm; cancel and confirm Delete with focus/Escape; Locate missing
and restored targets; inspect expiry/history; fire notifications without focus
loss. Run in both themes and normal/short viewports on the final immutable
harness. This evidence gap prevents acceptance READY even when code checks
pass.

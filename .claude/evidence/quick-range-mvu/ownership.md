# MVU1 ownership ledger — implementation checkpoint, not a delivery verdict

Comparison input: reviewed campaign 9ff57501249f51d8f75c21f52094ec3dc3c39af2
merged locally with pinned main a6644bc602e203b17bb29a5581548b03c4898fc6.
The combined merge is committed at 1b4bad7ebb257d0c0a016db6994338592b367bda,
with both source SHAs as parents. No root field-removal or score claim is made.

## Actual state and decisions

| Responsibility | Before | Current owner |
| --- | --- | --- |
| Direct QuantickApp quick-range fields | None | None: zero removed |
| Idle/pressed/selected state | app drawing_chrome::QuickRange.state | chart-interaction::quick_range::QuickRangeModel.state |
| Press owner/position/anchor | app State::Pressed, toolkit/domain chart coordinates | headless RangeContext/[f32;2]/Anchor |
| Selected owner/two anchors/readiness | app Selection.owner/anchors/ready | headless RangeView.context/anchors/Phase |
| Gesture eligibility/threshold/replacement/release | pane checks plus app QuickRange methods | headless Command update, GestureEligibility and GestureArea |
| Rewrite/layout/tab/pane/selection reconciliation | incomplete app lifetime checks | headless Event handling; stale view cannot paint or convert |
| Conversion success/refusal | app apply_quick_range branches dismiss/retain | correlated Completed/Refused model events; shell executes ExplainRefusal |
| v2/Fib anchor resolution | app annotate::place_chart timestamp/future loop | chart-interaction::annotation::resolve through read-only Series |
| Annotation DTO/schema/policy descriptors | app control::annotate | control::annotation; original schema names and capability versions retained |
| Ruler style/payload, rectangles and drawing paint | app | app view adapter: intentionally retained |
| Admission, actor, journal, idempotency and insertion | app/control-host | existing admitted action and one shared store installer: intentionally retained |

The app adapter still caches PaneSide for addressing, but stable pane ID,
layout and revision are validated before operation execution. PaneSide alone
is not identity. The model is private behind scoped methods, not a mutable
state hatch. The broad application does not own its transition rules.

## Ports, order and coupling

Command is Press/Drag/Release/Dismiss/Convert(Action). Event is Reconcile,
ActiveTab/PaneRemoved/Selection/Completed/Refused. Effect is Place(request)
or ExplainRefusal. Transition is fixed-size and contains at most one effect.
Actions remain Profile/Retracement/Projection. These are feature-scoped types,
not a dispatcher for unrelated chart/feed/trading responsibilities.

The frame adapter supplies current identity/revision before input and ruler
projection. Floating chrome reconciles the active tab's bounded pane set
(MAX_CANVAS_PANES=4) before presenting actions. It does not walk all session
tabs, trades or drawings. Persistent selection reconciliation precedes action
chrome. Exact-reference validation precedes every drawing-store insertion.
Focused model tests exercise same-update rewrite refusal before view/effect,
foreign owners, replacement and late completion. Further final integration
and UI ordering evidence remains required.

Cross-owner reads retained: pane stable identity/layout/revision and coordinate
projection; active pane references for removal/reorder; selected drawing ID;
saved ruler look; registered drawing-tool metadata. The headless model reads
none of these owners directly. The Series port exposes only six read-only
facts; no drawing-store callback can mutate while anchors are being validated.

Workspace edge delta: app -> chart-interaction, +1 (report 29 -> 30).
chart-interaction has no dependencies. No control -> app/core reverse edge.
Control now inherits the existing workspace rust_decimal dependency for the
same annotation wire-number conversion law; this is not a new workspace edge.

## Measured versus not measured

The following counts describe the initial implementation checkpoint. Current
v4 inputs (including the two independently found fractional-coordinate repairs)
pass the unchanged UI-free ceiling at 46907/46911, 302 below the merged input.
The ordered workspace run has 2075 passing app tests and 14 headless owner
tests. The feature-enabled run has 2076 passing app tests. See
ordered-validation-v4.md and feature-validation-current.md. Historical counts
below are retained as history, not current-head claims.

Merged input UI-free production count: 47209. Candidate checkpoint: 46911,
298 lower, meeting the prior 46911 ceiling without the incoming annotation
file exemption. Root production count: 10217 -> 10215 after rendering-pass
selection moves to the view owner; the ratchet was tightened by two, not
raised. This tiny root line delta is not the evidence of domain extraction:
the named state/decisions above are. No protected root field shape changed.

13 tests currently run with only chart-interaction; four original lifecycle
behaviors were re-expressed there and extended. The original demo test remains
in the feature-enabled app adapter because it constructs a registered ruler.
Two annotation contract tests run in control independently of app. Raw final
file/line totals, frozen measurements and performance verdict are pending.

## Final implementation-input checkpoint

Before archival/evidence files, the extraction above the staged merged input
edits29 existing files (757 added/970 removed lines) and adds8 crate/source
files totaling1582 physical lines, including tests and Cargo metadata.
This is an owner-carving migration, not a registration-only new feature:
the existing pane, floating chrome, control adapter and root effect applier
must stop owning the migrated decisions. Subsequent headless consumers have
the narrower change cost demonstrated by extension-probe.md.

Canonical guard report at these inputs: graph30edges, headless0findings,
cycle3(existing ceiling3), UI-free46911/46911, extension10800/10800,
context295401/295668. Unreadable/undecodable/blind/failed scans are all zero.
The full local four-command loop passes; app2073tests and headless13tests
pass. These facts do not constitute a review verdict or score.

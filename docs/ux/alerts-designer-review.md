# Alerts designer review

Issue: [#860](https://github.com/milocaetano/quantick/issues/860).
Date: 2026-11-10. This is implementation-designer participation, not an
independent review or an observed trader usability study.

## Design and evidence boundary

The 560-point floating surface uses the existing theme, widgets and icons.
The editor keeps its source, tabs and action footer outside the scrolling
body. Conditions, Message and Notifications share one draft. Operator
choices, validation and summaries come from the headless alerts owner;
the surface emits commands for persistence rather than mutating the book.
The manager keeps tabs and search above its scrolling rows. Named actions
pair familiar icons with text; they do not require recognizing an icon alone.

Native captures were made with the repository's PID-filtered PrintWindow
script. Environment hooks opened the existing surfaces and selected their
subviews. The synthetic ALERTTEST replay and every persisted store were
isolated under `.artifacts/alerts-ui-native`; no user store was copied or
changed. The manager/history fixtures were seeded through admitted control
capabilities. A native evaluator event at price `99.0191` was read back and
persisted before those captures.

These are rendered screenshots, not mouse/keyboard workflow proof.
Computer Use reported that its native pipe was unavailable (OS error 2)
after the documented retry/reset. Native input attempts stopped. No helper
input process, custom protocol or UI Automation fallback was used. Unit
egui input tests and admitted API tests remain separate evidence classes.

## Captured snapshot

The immutable `bin/quantick-alerts-final.exe` artifact has SHA256
`5675196FB888ABDF5260DCD014FB14A33E0D9D7418F3DE7F724113D92E1D95DD`,
modified at `2026-11-10T09:14:41.7329783Z`. Its build owner identified staged
tree `ec325b8f4e2ef747e007a6bb862508772e57cd1b`.

All 20 cells were captured and inspected: dark/light themes, normal
1200x800 and short 1000x360 logical viewports, and each of `editor`,
`editor-message`, `editor-notifications`, `manager`, `history`.
Actual captures at display scale 1.5 are 1800x1200 and 1500x540 pixels.

Local evidence paths are
`.artifacts/alerts-ui-native/{theme}-{size}-{mode}-corrected/capture.png`.
Each directory also contains `launch.json`, `health.json` and logs.
All saved health samples were 59-60 fps. This meets the capture-health gate;
compilation ran concurrently, so these samples are not a matched performance
comparison. Raw artifacts stay outside Git. All captured processes closed.

## Findings

| Surface | Rendered verdict | Evidence and practical limit |
| --- | --- | --- |
| Conditions | PASS layout/readability | Exact `100.0485` is visible in both value and summary in light-normal. Short view retains source/tabs/footer; later controls are inside the scrolling body. Scrolling and dropdown interaction were not exercised. |
| Message | PASS layout/readability | Multiline input, its label, summary and footer fit all four cells. Text entry and keyboard navigation were not exercised. |
| Notifications | PASS layout/readability | Local popup/toast choices, sound selector, monitoring-scope explanation and footer are visible in all four cells. Sound selection/preview was not exercised. |
| Manager | Layout PASS; action correction pending recapture | Source, names, state and actions are readable. Short view clips later rows within its scroll region while tabs/search remain visible. The captured completed row misleadingly offers Pause; the owner is replacing it with Rearm. |
| History | PASS layout/readability | Exact event price, UTC event time, message, definition revision and sequence are visible in all four cells. Seed/readback is `dark-normal-manager-final/control-seed-readback.json`. |

Two earlier rendering defects were corrected and recaptured: the value
widget rounded `100.0485` to `100.049` while the summary retained full
precision; and the recording's full filesystem path dominated the header.
The corrected widget preserves exact price text. Editor and manager now
show the replay filename, symbol and aggregation; full identity remains in
a tooltip. Tooltip activation itself remains untested.

The centered popup covers the chart's central area, as a floating editor
does, but the latest price marker stays visible on the right in these
captures. Short views preserve the action footer. Bell/plus Create is
readable although the two symbols are somewhat redundant; there is no
icon-only ambiguity in the named actions.

## Still unobserved

Native mouse and keyboard workflows (C11), focus traversal and restoration,
Escape, scrolling, selection dropdowns, invalid-input correction, live
creation through the footer, delete confirmation, drawing selection/Locate
and sound preview remain unobserved by this designer. These limitations
are not waived by screenshot, unit or control evidence. The earlier
snapshot's manager acceptance was superseded by the refresh below. Earlier
editor captures do not cover the later delete-confirmation or
rectangle-boundary label changes.

## Refresh at bc7ac17f

Commit `bc7ac17fc6d5779a2672642555aa108e86f6da62`, tree
`1a269998d2be9f12262076b7826b30bf2506a776`, was captured with immutable
`bin/quantick-alerts-bc7ac17f.exe`, SHA256
`535CC26A11BAF80BCE2C5A66B6C883014AC13D5261BBD71283515349598ED0CC`.
The build mapping is `.artifacts/validation/harness-bc7ac17f.json`.
Earlier 20-cell captures retain their original snapshot attribution;
they are not relabelled as this newer binary.

Twelve new native captures were inspected. Every combination of dark/light
and normal/short contains three scenes below. Saved health is 59-60 fps
throughout, a rendering gate only. Each scene has isolated stores and
`launch.json`, `health.json`, logs and `capture.png`. All processes closed.

| Scene/path suffix | Verdict | Observation |
| --- | --- | --- |
| `completed-bc7ac17f` | PASS rendered state, all four cells | Completed alert exposes Rearm and Edit, replacing the misleading Pause action. Both are visible in short windows. |
| `expired-bc7ac17f` | PASS rendered state, all four cells | Expired alert exposes a disabled Expired action beside Edit. Hover explanation and editing were not exercised. |
| `toolbar-clearance-bc7ac17f`, normal | PASS observed clearance, both themes | Context toolbar including Create alert is clear of the visible rectangle handles and floating inspector. |
| `toolbar-clearance-bc7ac17f`, short | FAIL control overlap, both themes | Toolbar crosses the inspector's Lock drawing row and upper body. Inspector title remains clear, but controls overlap. |

Paths are `.artifacts/alerts-ui-native/{theme}-{size}-{suffix}/capture.png`.
For example, the short dark overlap is
`dark-short-toolbar-clearance-bc7ac17f/capture.png`: inspector begins near
pixel (431,143); the toolbar crosses it around pixels (431,208)-(711,270).
The light short capture shows the same defect. The crowded pane leaves no
obvious free candidate for the whole toolbar, but bounded placement alone
does not establish usable controls. This finding was sent to the delivery
owner; no source change was made by the designer.

Manager fixtures are single-definition derivatives of the earlier isolated
persisted completed alert. The expired derivative clears completion and
sets an already elapsed cutoff. They are fixture-driven renders, not fresh
UI transitions. Toolbar fixtures use the existing gallery/select/inspector
hooks with rectangle selected and inspector position `288,294` in logical
points (clamped on short screens). A shortened 1,800-print synthetic replay
at 100x reaches the 22-drawing gallery threshold and stops with the handles
still visible. Final toolbar fixtures use fresh store directories; the
earlier exploratory `toolbar-bc7ac17f` directories are not acceptance evidence.

The published alerts hook has no mode for editing an existing definition,
opening delete confirmation, or choosing a rectangle target/operator.
Those changed editor states therefore remain unrendered in this pass.
Source/unit validation is not native rendering or input proof. Native
input recovery was not retried; C11 remains blocked as described above.

## Repair recapture at 485a77bb

Commit `485a77bb802255c0dc420137e39997ac7fda1051`, tree
`43a4ed82a0025b6b17b6c4d6fb8f36a3b6f2e0e5`, was recaptured using
`bin/quantick-alerts-485a77bb.exe`, SHA256
`457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`.
The exact build/source mapping is
`.artifacts/validation/harness-485a77bb.json`. Earlier evidence retains its
original binary attribution.

Fourteen native images were captured and inspected with isolated stores:
four toolbar cells, four default editor cells, four malformed-storage cells,
and two later short-window storage captures. Paths below are relative to
`.artifacts/alerts-ui-native`, each ending in `/capture.png`; adjacent launch
metadata, health and logs preserve the run. All processes closed afterward.

| Path pattern | Rendered verdict |
| --- | --- |
| `{theme}-{size}-toolbar-clearance-485a77bb` | PASS all four cells. Normal windows retain the full bar clear of the inspector and visible rectangle handles. Short windows show only the existing Create alert button to the inspector's left, leaving Lock drawing, Delete drawing and the upper inspector body unobscured. The previous H03 overlap is fixed in these actual pixels. |
| `{theme}-{size}-editor-485a77bb` | PASS default price-editor layout in all four cells. Exact `100.0485` remains readable; normal summary agrees, and short view retains header/footer. This does not exercise target switching or delete confirmation. |
| `{theme}-normal-storage-error-485a77bb` | PASS error honesty and readability. Malformed isolated storage produces an explicit unavailable heading, parse explanation and repair/restart guidance, with no false empty manager or successful editor. |
| `{theme}-short-storage-error-485a77bb` and `{theme}-short-storage-error-settled-485a77bb` | Error honesty PASS; minor visual occlusion remains. The storage-error toast covers the final draft-retention line near the window bottom, including after 12 seconds. Unavailable heading, error and file/permissions/restart guidance remain visible. |

The malformed fixture contains `malformed = [` in its isolated alerts file;
it is intentional invalid input, never a user store. The short toast overlap
was reported to the owner rather than silently labelled a full visual pass.
Settled short captures and all other accepted cells record 59-60 fps; the
initial light-short error capture recorded 57 fps. These are capture-health
checks, not performance comparisons.

No mouse/keyboard, target-switch, confirmation or tooltip behavior is inferred
from these renders. Existing hooks still cannot open the changed rectangle
operator or delete-confirmation states. C11's supported-helper gap remains
unresolved; no native-input retries or fallback automation were attempted.

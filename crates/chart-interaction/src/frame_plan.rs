//! Registered stages of one application frame, with the dependencies that
//! make their order load-bearing.
//!
//! The declaration order is the frame: the application's frame loop walks
//! [`FramePlan::stages`] and runs one stage adapter per entry. A dependency
//! is declared only where the code relies on it — a hazard a test pins, or a
//! panel whose place in the layout is its declaration order — so the
//! independent stages stay free to move and the dependent ones cannot.
//!
//! Harness-only stages exist only where their hooks do. The plan is a macro
//! the application expands with one `cfg` predicate per harness family —
//! `scenario`, `control` and `scripted` (scenario or drawing) — so a release
//! build's `FrameStage` names no harness stage and its plan has none of
//! their edges; the constant validation holds for every build.
//!
//! What it replaced: the frame's order used to be the statement order of one
//! 734-line root method, where 49 lines start an `if`, `match`, `for` or
//! `while` or hold an `else` to decide what ran where. That method is now 8 lines with no branch; the order is 31 stages and 31
//! declared edges with every harness, 24 and 26 without, pinned by the tests
//! beside this file. Adding an
//! independent stage costs one `Name after []` line here and one executor
//! arm in the application: no other declaration moves, and the constant
//! validation covers it the moment it compiles.

#[cfg(test)]
mod tests;

/// Declare `FrameStage` and `FramePlan` in the calling module, each harness
/// stage and edge compiled where its family's predicate holds there.
#[macro_export]
macro_rules! frame_stages {
    (scenario: $scenario:meta, control: $control:meta, scripted: $scripted:meta $(,)?) => {
        $crate::declare_stages! {
            pub enum FrameStage {
                /// The interval since the previous frame, for the health readout.
                RecordFrameTime after [],
                /// Every tab takes in what its feed sent, on screen or not.
                DrainSources after [],
                /// History notes past their linger leave.
                ExpireHistoryNotes after [],
                /// The harness re-raises a note it finds absent. After the expiry:
                /// running first would let a note expire after the hook looked,
                /// drawing one frame with an empty lane.
                HistoryNoteHook when ($scenario) after [ExpireHistoryNotes],
                /// The harness asks for local agent access.
                EnableControlAccess when ($control) after [],
                /// A control trace beside a replayed session re-injects its actions.
                ReplayTrace after [],
                /// The harness mark. After the trace: a mark taken before its
                /// recording's trace loads is read back from the sidecar as a
                /// recorded action and re-injected as a second, replayed mark.
                TakeMark when ($control) after [ReplayTrace],
                /// The harness writes its annotations.
                AnnotateHooks when ($control) after [],
                /// The harness captures an evidence bundle. After the annotations:
                /// the bundle describes the window an assistant already wrote on.
                EvidenceHook when ($control) after [AnnotateHooks],
                /// The control gateway services this frame's requests. After the
                /// evidence hook, which must capture before the gateway drains.
                GatewayService after [EvidenceHook when ($control)],
                /// Scripted views, demos and pending history requests.
                ScenarioHooks when ($scripted) after [],
                /// The harness's window startup state, once.
                WindowStartupHook when ($scenario) after [],
                /// The health summary and workspace upkeep. After the drain: the
                /// summary reports the trades it counted.
                WindowHousekeeping after [DrainSources],
                /// Keyboard claims first, then the menu, control and tool bars.
                TopChrome after [],
                /// Indicator requests serviced, then the indicator dialogs drawn.
                IndicatorSurfaces after [],
                /// Pane requests to arm a strategy, handed to the arming dialog.
                StrategyPopupRequests after [],
                /// The registered surfaces and what they asked for. After the
                /// toolbar that toggles them, after the indicator dialog that sets
                /// the preview the environment reads, and after the popup requests
                /// that open the arming dialog.
                Surfaces after [TopChrome, IndicatorSurfaces, StrategyPopupRequests],
                /// Script reloads, pending indicator state and chart layers.
                IndicatorMaintenance after [],
                /// The status line, and the frame's one stall judgement.
                StatusLine after [],
                /// The layout delete confirmation.
                LayoutDialogs after [],
                /// The replay browser and transport. After the status line: bottom
                /// panels stack outside-in, so the transport sits directly above it.
                ReplayBrowser after [StatusLine],
                /// The edge-docked drawing rail, inside the top and bottom chrome.
                DrawingRail after [TopChrome, ReplayBrowser],
                /// The right dock and what it asked for, inside the same chrome.
                Dock after [TopChrome, ReplayBrowser],
                /// The pinned drawing inspector: a right panel, so after the dock,
                /// which keeps the outer edge. After the scenario hooks: a drawing
                /// demo selects what it placed, and the panel shows that selection
                /// on the frame it was made rather than the next.
                PinnedInspector after [Dock, ScenarioHooks when ($scripted)],
                /// A changed market respawns its feed and pending layouts settle.
                /// After the surfaces, whose market request this frame applies.
                FeedAndLayoutSwitches after [Surfaces],
                /// Waits owned by other components mirrored onto the loading overlay.
                /// After the replay browser whose load it reflects.
                MirrorLoadingWaits after [ReplayBrowser],
                /// The chart, its overlays and the feed corner: whatever the panels
                /// left. After every panel, which the chart pays for, after the stall
                /// judgement the corner reads, and after the switches and waits it
                /// paints.
                Canvas after [
                    TopChrome,
                    StatusLine,
                    ReplayBrowser,
                    DrawingRail,
                    Dock,
                    PinnedInspector,
                    FeedAndLayoutSwitches,
                    MirrorLoadingWaits,
                ],
                /// Floating drawing controls, in front of the opaque canvas.
                DrawingChrome after [Canvas],
                /// Cancels for disarmed bots and pending alarms. After every menu
                /// that can disarm one this frame.
                StrategyCleanup after [Surfaces, Canvas, DrawingChrome],
                /// The frame tail: the feed notice's action, paper settlement and
                /// report, the popup state. After the canvas that answered them, and
                /// after the drain: a replay seek arrives there as a reset that
                /// closes the paper position, and the report projects that close on
                /// the frame it happened.
                Tail after [Canvas, DrainSources],
                /// Keep polling the feed about sixty times a second.
                RequestRepaint after [],
            }
        }

        /// The canonical traversal the frame loop executes.
        pub struct FramePlan;

        impl FramePlan {
            pub fn stages() -> impl ExactSizeIterator<Item = FrameStage> + Clone {
                FrameStage::canonical()
            }
        }
    };
}

//! Registered stages of one application frame, with the dependencies that
//! make their order load-bearing and the scope each one runs in.
//!
//! The declaration order is the frame: the application's frame loop walks
//! [`FramePlan`] scope by scope and runs one stage adapter per entry. A
//! dependency is declared only where the code relies on it — a hazard a test
//! pins, or a panel whose place in the layout is its declaration order — so
//! the independent stages stay free to move and the dependent ones cannot.
//!
//! Every stage declares one [`StageScope`]: once per tick before any window
//! paints, once for each presented native window, or once per tick after the
//! last window. The declaration is grouped by scope, so the order a one-window
//! tick runs is the declaration order itself, and a constant assertion
//! refuses a stage declared out of its group. Every stage also declares its
//! [`HomeVisibility`]: whether it runs while Home stands in for the chart.
//! The executor reads both; no adapter asks either question again.
//!
//! Harness-only stages exist only where their hooks do. The plan is a macro
//! the application expands with one `cfg` predicate per harness family —
//! `scenario`, `control` and `scripted` (scenario or drawing) — so a release
//! build's `FrameStage` names no harness stage and its plan has none of
//! their edges; the constant validation holds for every build.
//!
//! What it replaced: the frame's order used to be the statement order of one
//! 734-line root method, where 49 lines start an `if`, `match`, `for` or
//! `while` or hold an `else` to decide what ran where. Then it was three
//! loops over the plan, each with its own `match` deciding which half of a
//! stage it ran, and Home asked in four adapters. Now the order is 41 stages
//! and 46 declared edges with every harness, 34 and 41 without, each with its
//! scope and Home visibility, pinned by the tests beside this file. Adding an
//! independent stage costs one `Name after []` line here, one scope and one
//! Home entry, and one executor arm in the application: no other declaration
//! moves, and the constant validation covers it the moment it compiles.

#[cfg(test)]
mod tests;

/// Where a frame stage runs within one tick of the native window set.
///
/// The declaration order of the scopes is the order a tick runs them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StageScope {
    /// Once per tick, before any window paints: sources, the gateway, the
    /// workers and the market state every window reads.
    Tick,
    /// Once for each presented window, with that window bound: input, paint
    /// and the consequences of what was clicked there.
    Window,
    /// Once per tick, after every window painted: the tabs no window shows.
    Settle,
}

impl StageScope {
    /// Whether a pass over this scope ends when its window closes under it.
    pub const fn ends_with_its_window(self) -> bool {
        matches!(self, Self::Window)
    }
}

/// Whether a frame stage runs while Home stands in for the chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeVisibility {
    /// Runs on Home and over a market alike.
    Always,
    /// Acts on the market Home hides, so it stands down while Home is up.
    Market,
    /// Home's own stand-in for a market stage: it runs only on Home.
    Home,
}

impl HomeVisibility {
    /// Whether a stage with this visibility runs, given whether Home is up.
    pub const fn runs(self, home: bool) -> bool {
        match self {
            Self::Always => true,
            Self::Market => !home,
            Self::Home => home,
        }
    }
}

/// Declare `FrameStage` and `FramePlan` in the calling module, each harness
/// stage and edge compiled where its family's predicate holds there.
#[macro_export]
macro_rules! frame_stages {
    (scenario: $scenario:meta, control: $control:meta, scripted: $scripted:meta $(,)?) => {
        $crate::declare_stages! {
            pub enum FrameStage {
                // Once per tick, before any window paints.
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
                /// The health summary and workspace upkeep. After the drain: the
                /// summary reports the trades it counted.
                WindowHousekeeping after [DrainSources],
                /// Hook and pane requests that open or replace an indicator dialog.
                IndicatorSurfaces after [],
                /// Pane requests to arm a strategy, handed to the arming dialog.
                StrategyPopupRequests after [],
                /// Script reloads, pending indicator state and chart layers.
                IndicatorMaintenance after [],
                /// Pending layouts settle, a frame after the click that armed
                /// them, so the frame carrying the change paints its overlay
                /// first; then every tab's layout and workspace upkeep.
                LayoutSwitches after [],
                /// Waits owned by other components mirrored onto the loading
                /// overlay. Once per tick, before the windows: it reflects the
                /// load a replay browser started on the tick before.
                MirrorLoadingWaits after [],
                /// Keep polling the feed about sixty times a second.
                RequestRepaint after [],
                // Once for each presented window.
                /// The harness's window startup state, once.
                WindowStartupHook when ($scenario) after [],
                /// Keyboard claims first: the indicator picker's, then the tab
                /// keys, which may enter or leave Home.
                TabKeys after [],
                /// Rail and drawing shortcuts, after the tab keys settled
                /// whether a market is on screen.
                MarketKeys after [TabKeys],
                /// The menu and control bars. After the rail shortcuts, which
                /// read the keyboard before any widget holds it.
                MenuBar after [MarketKeys],
                /// No toolbar on Home: the market underneath still settles, and
                /// a picker the menus opened still answers. Before the toolbar
                /// and unable to bring the market back, so one of the two runs.
                HomeToolbar after [MenuBar],
                /// The toolbar, under the menu bar.
                Toolbar after [MenuBar, HomeToolbar],
                /// The indicator dialogs, after the requests that open them, so
                /// a double click on a pane or a curve opens the dialog on the
                /// same frame the gesture happened.
                IndicatorDialogs after [IndicatorSurfaces],
                /// The registered surfaces and what they asked for. After the
                /// toolbar that toggles them, after the indicator dialog that sets
                /// the preview the environment reads and after the request that
                /// replaces it, and after the popup requests that open the arming
                /// dialog.
                Surfaces after [
                    Toolbar,
                    IndicatorSurfaces,
                    IndicatorDialogs,
                    StrategyPopupRequests,
                ],
                /// The status line, and the frame's one stall judgement.
                StatusLine after [],
                /// The layout delete confirmation.
                LayoutDialogs after [],
                /// The replay browser and transport. After the status line: bottom
                /// panels stack outside-in, so the transport sits directly above it.
                ReplayBrowser after [StatusLine],
                /// The edge-docked drawing rail, inside the top and bottom chrome.
                DrawingRail after [Toolbar, ReplayBrowser],
                /// The right dock and what it asked for, inside the same chrome.
                Dock after [Toolbar, ReplayBrowser],
                /// The pinned drawing inspector: a right panel, so after the dock,
                /// which keeps the outer edge. After the scenario hooks: a drawing
                /// demo selects what it placed, and the panel shows that selection
                /// on the frame it was made rather than the next.
                PinnedInspector after [Dock, ScenarioHooks when ($scripted)],
                /// A changed market respawns its feed and the active tab applies
                /// its spec. After the surfaces, whose market request this window
                /// applies.
                FeedSwitch after [Surfaces],
                /// Home's page where the chart would be: whatever the panels
                /// left. After the chrome Home keeps, which it pays for.
                HomeCanvas after [Toolbar, StatusLine, ReplayBrowser, FeedSwitch],
                /// The chart, its overlays and the feed corner: whatever the panels
                /// left. After every panel, which the chart pays for, after the stall
                /// judgement the corner reads, and after the switches and waits it
                /// paints. After Home's page, which cannot bring the market back,
                /// so one of the two runs.
                Canvas after [
                    HomeCanvas,
                    Toolbar,
                    StatusLine,
                    ReplayBrowser,
                    DrawingRail,
                    Dock,
                    PinnedInspector,
                    LayoutSwitches,
                    FeedSwitch,
                    MirrorLoadingWaits,
                ],
                /// Floating drawing controls, in front of the opaque canvas.
                DrawingChrome after [Canvas],
                /// Cancels for the shown tab's disarmed bots. After every menu
                /// that can disarm one this frame.
                StrategyCleanup after [Surfaces, Canvas, DrawingChrome],
                /// The frame tail: the feed notice's action, the shown account's
                /// settlement and report, the popup state. After the canvas that
                /// answered them, and after the drain: a replay seek arrives there
                /// as a reset that closes the paper position, and the report
                /// projects that close on the frame it happened.
                Tail after [Canvas, DrainSources],
                // Once per tick, after every window.
                /// Cancels for the disarmed bots of the tabs no window shows: a
                /// menu click and a tab switch can land on the same frame, and a
                /// hidden tab's cancel must not sit stranded until it is looked at.
                BackgroundStrategyCleanup after [],
                /// Alarms every tab raised this tick, after the cancels that can
                /// silence one.
                PlayAlerts after [StrategyCleanup, BackgroundStrategyCleanup],
                /// The accounts no window shows settle, after the shown ones did.
                BackgroundPaperPanels after [Tail],
            }
        }

        /// The canonical traversal the frame loop executes.
        pub struct FramePlan;

        impl FramePlan {
            pub fn stages() -> impl ExactSizeIterator<Item = FrameStage> + Clone {
                FrameStage::canonical()
            }

            /// The stages one pass over `scope` runs, in declaration order.
            pub fn stages_in(
                scope: $crate::frame_plan::StageScope,
            ) -> impl Iterator<Item = FrameStage> + Clone {
                Self::stages().filter(move |stage| stage.scope() == scope)
            }
        }

        impl FrameStage {
            /// Where this stage runs within one tick of the window set.
            pub const fn scope(self) -> $crate::frame_plan::StageScope {
                use $crate::frame_plan::StageScope::{Settle, Tick, Window};
                match self {
                    Self::RecordFrameTime
                    | Self::DrainSources
                    | Self::ExpireHistoryNotes
                    | Self::ReplayTrace
                    | Self::GatewayService
                    | Self::WindowHousekeeping
                    | Self::IndicatorSurfaces
                    | Self::StrategyPopupRequests
                    | Self::IndicatorMaintenance
                    | Self::LayoutSwitches
                    | Self::MirrorLoadingWaits
                    | Self::RequestRepaint => Tick,
                    #[cfg($scenario)]
                    Self::HistoryNoteHook => Tick,
                    #[cfg($control)]
                    Self::EnableControlAccess
                    | Self::TakeMark
                    | Self::AnnotateHooks
                    | Self::EvidenceHook => Tick,
                    #[cfg($scripted)]
                    Self::ScenarioHooks => Tick,
                    #[cfg($scenario)]
                    Self::WindowStartupHook => Window,
                    Self::TabKeys
                    | Self::MarketKeys
                    | Self::MenuBar
                    | Self::HomeToolbar
                    | Self::Toolbar
                    | Self::IndicatorDialogs
                    | Self::Surfaces
                    | Self::StatusLine
                    | Self::LayoutDialogs
                    | Self::ReplayBrowser
                    | Self::DrawingRail
                    | Self::Dock
                    | Self::PinnedInspector
                    | Self::FeedSwitch
                    | Self::HomeCanvas
                    | Self::Canvas
                    | Self::DrawingChrome
                    | Self::StrategyCleanup
                    | Self::Tail => Window,
                    Self::BackgroundStrategyCleanup
                    | Self::PlayAlerts
                    | Self::BackgroundPaperPanels => Settle,
                }
            }

            /// Whether this stage runs while Home stands in for the chart.
            pub const fn home(self) -> $crate::frame_plan::HomeVisibility {
                use $crate::frame_plan::HomeVisibility::{Always, Home, Market};
                match self {
                    Self::MarketKeys
                    | Self::Toolbar
                    | Self::DrawingRail
                    | Self::Dock
                    | Self::PinnedInspector
                    | Self::Canvas
                    | Self::DrawingChrome => Market,
                    Self::HomeToolbar | Self::HomeCanvas => Home,
                    _ => Always,
                }
            }

            /// Rendering/input adapters depend on native placement; process work does not.
            pub const fn needs_presentation(self) -> bool {
                match self {
                    Self::TabKeys
                    | Self::MarketKeys
                    | Self::MenuBar
                    | Self::HomeToolbar
                    | Self::Toolbar
                    | Self::IndicatorDialogs
                    | Self::Surfaces
                    | Self::StatusLine
                    | Self::LayoutDialogs
                    | Self::ReplayBrowser
                    | Self::DrawingRail
                    | Self::Dock
                    | Self::PinnedInspector
                    | Self::HomeCanvas
                    | Self::Canvas
                    | Self::DrawingChrome => true,
                    #[cfg($scenario)]
                    Self::WindowStartupHook => true,
                    #[cfg($control)]
                    Self::EvidenceHook => true,
                    _ => false,
                }
            }

            /// Whether `order` keeps every scope's stages together, in the
            /// order a tick runs the scopes.
            pub const fn is_grouped_by_scope(order: &[Self]) -> bool {
                let mut index = 1;
                while index < order.len() {
                    if (order[index].scope() as u8) < (order[index - 1].scope() as u8) {
                        return false;
                    }
                    index += 1;
                }
                true
            }
        }

        const _: () = assert!(FrameStage::is_grouped_by_scope(&FrameStage::ORDER));
    };
}

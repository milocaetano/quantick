//! Toolbar state and availability, independent of painting and host effects.
use quantick_sources::candles::OlderCandles;
use quantick_sources::config::FeedCapabilities;
use quantick_sources::history_reach::HistoryReach;
use quantick_sources::history_run::RunStatus;
use quantick_workspace::canvas::{LayoutMarks, PaneTree};

/// A side effect the toolbar asks the app to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Switch the canvas to a named arrangement from the layout registry.
    ///
    /// Carries the registry entry rather than a copy of its shape, so the
    /// toolbar never holds a second opinion about what a layout contains.
    SetLayout(&'static quantick_workspace::canvas::LayoutPreset),
    /// Load back to a target: the main click's, or one the menu named.
    LoadHistory(HistoryReach),
    /// Stop the history run in flight and keep what arrived.
    CancelHistory,
    /// Open the palette in switch mode on this tab (the market button).
    SwitchMarket,
    /// What the REC control beside the symbol asked for.
    DealRecording(quantick_replay::recorder::DealRecordingAction),
    /// Fetch and prepend one more span of older venue candles.
    ///
    /// The trade twin above and this one are two different records — a page of
    /// prints and a span of buckets — served by two different venue endpoints
    /// under two different capabilities. A feed can page one and not the
    /// other, so they are two actions rather than one with a mode.
    LoadOlderCandles,
    /// Show or hide the dock.
    ToggleDock,
    /// Open or close the window menu, hung from the toolbar's `≡`.
    ToggleWindowMenu,
    /// Open or close the layout popover, hung from the capsule's miniature.
    ToggleLayoutPopover,
    /// Open or close the ADD TO CANVAS menu, hung from the +.
    ToggleAddMenu,
    /// Open or close the workspace menu, hung from the capsule's name.
    ToggleWorkspaceMenu,
    /// Save the tab's edits to its workspace (the floppy, Ctrl+S).
    SaveWorkspace,
    /// Add a native indicator, by its catalog id
    /// (`quantick_indicators::native`). One variant for every native there
    /// will ever be — the menu is drawn from the catalog, so the toolbar
    /// never learns which natives exist.
    AddNative(&'static str),
    /// Flip an indicator's render-side eye toggle (no recompute).
    ToggleIndicatorHidden(u64),
    /// Remove an indicator.
    RemoveIndicator(u64),
    /// Load a script from the library, by index into `ToolbarModel::scripts`.
    AddScriptIndicator(usize),
    /// Open the settings dialog of an indicator.
    OpenIndicatorSettings(u64),
    /// Simulated market buy at the Trading tab's quantity (paper trading).
    PaperBuy,
    /// Simulated market sell at the Trading tab's quantity (paper trading).
    PaperSell,
    /// Exit the open simulated position at the next print.
    PaperClose,
    /// Turn the open simulated position around at the next print.
    PaperReverse,
}

pub use ToolbarAction as Action;

#[derive(Debug, Clone, PartialEq)]
pub struct Capsule {
    pub tree: PaneTree,
    pub workspace: String,
    pub unsaved: bool,
    pub marks: LayoutMarks,
    pub menu_open: bool,
    pub popover_open: bool,
    pub add_open: bool,
    pub door_open: bool,
    pub door_hover: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaySource {
    pub label: String,
    pub hover: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryButton {
    pub main: HistoryReach,
    pub status: RunStatus,
    pub tz: quantick_civil::TzOffset,
}
impl HistoryButton {
    pub fn action(self) -> Action {
        if self.status.running() {
            Action::CancelHistory
        } else {
            Action::LoadHistory(self.main)
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub capsule: Capsule,
    pub market: (char, String),
    pub replay: Option<ReplaySource>,
    pub deal_recording: Option<quantick_replay::recorder::RecordingView>,
    pub focused_bars: quantick_engine::bar_registry::BarConfiguration,
    pub history: HistoryButton,
    pub history_candles: usize,
    pub older_candles: OlderCandles,
    pub capabilities: FeedCapabilities,
    pub dock_visible: bool,
    pub paper: quantick_paper::desk::TradeBar,
}
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
}
pub enum Intent {
    Refresh(Box<Snapshot>),
    Actions(Vec<Action>),
}
#[derive(Debug, PartialEq)]
pub enum Effect {
    Action(Action),
    SetLayoutPopover(bool),
    SetAddMenu(bool),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Vec<Effect>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Refresh(snapshot) => {
                if state.snapshot.as_ref() != Some(&*snapshot) {
                    state.snapshot = Some(*snapshot);
                }
                Effects::default()
            }
            Intent::Actions(actions) => {
                let Some(snapshot) = state.snapshot.as_mut() else {
                    return Effects::default();
                };
                Effects(
                    actions
                        .into_iter()
                        .filter_map(|action| {
                            if !snapshot.available(action) {
                                return None;
                            }
                            Some(snapshot.capsule.transition(action))
                        })
                        .collect(),
                )
            }
        }
    }
}
impl Snapshot {
    pub fn available(&self, action: Action) -> bool {
        match action {
            Action::LoadHistory(_) => {
                self.capabilities.history_paging && !self.history.status.running()
            }
            Action::CancelHistory => self.history.status.running(),
            Action::LoadOlderCandles => self.older_candles.is_available(),
            Action::PaperBuy | Action::PaperSell => self.paper.blocked.is_none(),
            Action::PaperReverse => self.paper.blocked.is_none() && self.paper.close.is_some(),
            Action::PaperClose => self.paper.close.is_some(),
            _ => true,
        }
    }
    pub fn history_menu_reachable(&self) -> bool {
        self.capabilities.history_paging || self.capabilities.ohlcv_history
    }
}

impl Capsule {
    /// Resolve capsule toggles against the current cached state.
    pub fn transition(&mut self, action: Action) -> Effect {
        match action {
            Action::ToggleLayoutPopover => {
                self.popover_open ^= true;
                Effect::SetLayoutPopover(self.popover_open)
            }
            Action::ToggleAddMenu => {
                self.add_open ^= true;
                Effect::SetAddMenu(self.add_open)
            }
            action => Effect::Action(action),
        }
    }

    /// A selected layout or an outside press ends the layout picker session.
    pub fn settle_popover(&mut self, outside: bool, actions: &[Action]) {
        self.popover_open &= !outside
            && !actions
                .iter()
                .any(|action| matches!(action, Action::SetLayout(_)));
    }
    pub fn marks(&self) -> (&str, bool, bool) {
        (&self.workspace, self.unsaved, self.marks.layout_unsaved)
    }
}

/// One answer for the pane headers and source controls.
pub fn available_bar_inputs(
    capabilities: FeedCapabilities,
    recording: Option<&quantick_replay::recorder::RecordingView>,
) -> quantick_engine::bar_selection::BarInputAvailability {
    quantick_engine::bar_selection::BarInputAvailability {
        traded_volume: capabilities.traded_volume,
        deal_counter: capabilities.deal_counter,
        deal_count: recording
            .is_some_and(quantick_replay::recorder::RecordingView::deal_count_available),
    }
}
impl HistoryButton {
    pub fn label(self, clock: &str, cancel: &str) -> String {
        let at = |ms| quantick_civil::fmt_weekday_minute(ms, self.tz);
        match self.status {
            RunStatus::Idle => format!("{clock} History: {}", self.main.label()),
            RunStatus::Queued(reach) => format!("Queued {}\u{2026} {cancel}", reach.label()),
            RunStatus::Loading(progress) | RunStatus::Paused(progress) => format!(
                "Loading {}\u{2026} back to {} {cancel}",
                progress.reach.label(),
                at(progress.oldest_ms)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_toggles_and_dismissal_use_cached_state() {
        let preset = &quantick_workspace::canvas::LAYOUT_PRESETS[0];
        let mut capsule = Capsule {
            tree: preset.tree.clone(),
            workspace: "Default".into(),
            unsaved: false,
            marks: LayoutMarks::default(),
            menu_open: false,
            popover_open: false,
            add_open: false,
            door_open: false,
            door_hover: String::new(),
        };
        assert_eq!(
            capsule.transition(Action::ToggleLayoutPopover),
            Effect::SetLayoutPopover(true)
        );
        capsule.settle_popover(false, &[]);
        assert!(capsule.popover_open);
        capsule.settle_popover(true, &[]);
        assert!(!capsule.popover_open);
        capsule.transition(Action::ToggleLayoutPopover);
        capsule.settle_popover(false, &[Action::SetLayout(preset)]);
        assert!(!capsule.popover_open);
        assert_eq!(
            capsule.transition(Action::ToggleAddMenu),
            Effect::SetAddMenu(true)
        );
        assert_eq!(
            capsule.transition(Action::ToggleAddMenu),
            Effect::SetAddMenu(false)
        );
        assert_eq!(
            capsule.transition(Action::SaveWorkspace),
            Effect::Action(Action::SaveWorkspace)
        );
    }
    #[test]
    fn the_history_button_names_its_target_and_cancels_a_run() {
        use quantick_sources::history_reach::ReachProgress;
        let tz = quantick_civil::TzOffset::new(-180);
        let idle = HistoryButton {
            main: HistoryReach::Sessions(1),
            status: RunStatus::Idle,
            tz,
        };
        assert!(idle.label("clock", "cancel").ends_with("Yesterday"));
        // Thursday 2026-10-01 13:14 UTC is 10:14 at UTC-03:00.
        let loading = HistoryButton {
            status: RunStatus::Loading(ReachProgress {
                reach: HistoryReach::Sessions(5),
                oldest_ms: 1_790_860_440_000,
                sessions_reached: 2,
                traded_ms: 0,
                prints_pulled: 0,
                pages: 3,
            }),
            ..idle
        };
        let label = loading.label("clock", "cancel");
        assert!(
            label.starts_with("Loading 5 days\u{2026} back to Thu 10:14"),
            "{label}"
        );
        let queued = HistoryButton {
            status: RunStatus::Queued(HistoryReach::Hours(2)),
            ..idle
        };
        assert!(queued.label("clock", "cancel").starts_with("Queued +2 h"));
        assert!(
            quantick_session::HistoryPagingOff::NothingToPage
                .hover()
                .contains("only streams forward"),
            "and a feed that cannot page gets the other reason, not the run's"
        );
    }

    #[test]
    fn the_history_button_loads_its_target_and_cancels_queued_or_running_work() {
        let button = HistoryButton {
            main: HistoryReach::Sessions(1),
            status: RunStatus::Idle,
            tz: quantick_civil::TzOffset::new(-180),
        };
        assert_eq!(
            button.action(),
            Action::LoadHistory(HistoryReach::Sessions(1))
        );
        let queued = HistoryButton {
            status: RunStatus::Queued(HistoryReach::Hours(2)),
            ..button
        };
        assert_eq!(queued.action(), Action::CancelHistory);
        let running = HistoryButton {
            status: RunStatus::Loading(quantick_sources::history_reach::ReachProgress {
                reach: HistoryReach::Sessions(5),
                oldest_ms: 1_790_860_440_000,
                sessions_reached: 2,
                traded_ms: 0,
                prints_pulled: 0,
                pages: 3,
            }),
            ..button
        };
        assert_eq!(running.action(), Action::CancelHistory);
    }
}

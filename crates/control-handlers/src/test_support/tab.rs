#![cfg(test)]

use super::*;

/// One fake tab: its market, what its feed and history do when asked, and
/// its panes, the first on the flow side.
pub(crate) struct FakeTab {
    pub id: u64,
    pub symbol: String,
    pub feed_id: String,
    /// Whether a recovery finds a feed session to respawn.
    pub respawns: bool,
    /// Every recovery asked of the tab, `true` for a reconnect.
    pub recoveries: Vec<Recovery>,
    pub history_paging: bool,
    /// The chart is still filling, so a press waits.
    pub filling: bool,
    /// A started run has nothing to page and ends within the press.
    pub start_finishes: bool,
    pub history: RunStatus,
    pub main_reach: HistoryReach,
    pub panes: Vec<FakePane>,
    /// The tab plays a recorded session.
    pub replaying: bool,
    /// The deal recorder's view; `None` on a feed with no counter.
    pub recording: Option<RecordingView>,
    /// What the recorder refuses to start or load with, when it refuses.
    pub recorder_refusal: Option<DealRecordingError>,
    /// Every recorder act the tab took, by name.
    pub recorder_acts: Vec<String>,
    pub auto_roll: AutoRoll,
    /// What the last History act on the tab did.
    pub last_history: Option<HistoryOutcome>,
}

impl FakeTab {
    pub fn new(id: u64, symbol: &str) -> Self {
        Self {
            id,
            symbol: symbol.to_owned(),
            feed_id: "fake".to_owned(),
            respawns: true,
            recoveries: Vec::new(),
            history_paging: true,
            filling: false,
            start_finishes: false,
            history: RunStatus::Idle,
            main_reach: HistoryReach::Hours(1),
            panes: Vec::new(),
            replaying: false,
            recording: None,
            recorder_refusal: None,
            recorder_acts: Vec::new(),
            auto_roll: AutoRoll::default(),
            last_history: None,
        }
    }

    /// The recorder's act `act`, unless it refuses.
    fn record_act(&mut self, act: String) -> Result<(), DealRecordingError> {
        if let Some(refusal) = self.recorder_refusal.clone() {
            return Err(refusal);
        }
        self.recorder_acts.push(act);
        Ok(())
    }

    /// A press judged the way the session judges one: refused while a run
    /// pages, queued while the chart fills, begun otherwise.
    fn press(&mut self, reach: HistoryReach) -> Press {
        let press = match self.history {
            RunStatus::Loading(_) | RunStatus::Paused(_) => return Press::AlreadyRunning,
            _ if self.filling => {
                self.history = RunStatus::Queued(reach);
                Press::Queued
            }
            _ => {
                self.history = if self.start_finishes {
                    RunStatus::Idle
                } else {
                    loading(reach)
                };
                Press::Start
            }
        };
        self.main_reach = reach;
        press
    }

    fn cancel(&mut self) -> Cancelled {
        let cancelled = match self.history {
            RunStatus::Queued(reach) => Cancelled::Queued(reach),
            _ => Cancelled::Nothing,
        };
        self.history = RunStatus::Idle;
        cancelled
    }

    fn snapshot(&self, outcome: Option<HistoryOutcome>) -> SessionSnapshot {
        let (last_press, last_cancel) = match outcome {
            Some(HistoryOutcome::Pressed(press)) => (Some(press), None),
            Some(HistoryOutcome::Cancelled(cancelled)) => (None, Some(cancelled)),
            None => (None, None),
        };
        SessionSnapshot {
            run: self.history,
            main_reach: self.main_reach,
            last_press,
            last_cancel,
            note: None,
            candles_held: 0,
            candles_pending: false,
            older_exhausted: false,
            venue_lead_in: false,
            progressive: true,
        }
    }
}

/// The main-click default a fake window opens with.
pub(crate) const FAKE_DEFAULT_REACH: HistoryReach = HistoryReach::Sessions(9);

/// One fake feed: its id, the symbols it offers and the contract each of
/// those rolls onto.
pub(crate) type FakeFeed<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

/// A config over `feeds`.
pub(crate) fn fake_config(feeds: &[FakeFeed<'_>]) -> AppConfig {
    let feeds: Vec<serde_json::Value> = feeds
        .iter()
        .map(|(id, symbols, rolls)| {
            let rolls: std::collections::BTreeMap<_, _> = rolls.iter().copied().collect();
            serde_json::json!({
                "id": id,
                "name": id,
                "provider": "binance",
                "symbols": symbols,
                "contract_rolls": rolls,
            })
        })
        .collect();
    let config =
        serde_json::json!({ "default_feed": "fake", "default_symbol": "FAKE", "feeds": feeds });
    serde_json::from_value(config).expect("a valid config")
}

/// The fake tab's market acts over its plain fields.
impl MarketTab for FakeTab {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: self.feed_id.clone(),
            symbol: self.symbol.clone(),
            replay: self.replaying,
        }
    }
    fn switch_market(&mut self, _config: &AppConfig, feed_id: &str, symbol: &str) {
        (self.feed_id, self.symbol) = (feed_id.to_owned(), symbol.to_owned());
    }
    fn paper_open(&self) -> bool {
        false
    }
    fn unlisted(&self) -> Option<String> {
        None
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        &mut self.auto_roll
    }
    fn recover(&mut self, _config: &AppConfig, recovery: Recovery) -> RecoveredFeed {
        self.recoveries.push(recovery);
        RecoveredFeed {
            symbol: self.symbol.clone(),
            respawned: self.respawns,
        }
    }
    fn pages_history(&self, _config: &AppConfig) -> bool {
        self.history_paging
    }
    fn history(&mut self, _config: &AppConfig, act: HistoryAct) -> HistoryOutcome {
        let outcome = match act {
            HistoryAct::Press(reach) => HistoryOutcome::Pressed(self.press(reach)),
            HistoryAct::Cancel => HistoryOutcome::Cancelled(self.cancel()),
        };
        self.last_history = Some(outcome);
        outcome
    }
    fn session(&self) -> SessionSnapshot {
        self.snapshot(self.last_history)
    }
}

impl DealRecorder for FakeTab {
    fn replaying(&self) -> bool {
        self.replaying
    }
    fn recorded_symbol(&self) -> &str {
        &self.symbol
    }
    fn recorder_view(&self) -> Option<RecordingView> {
        self.recording.clone()
    }
    fn load_recorded_day(&mut self, index: usize) -> Result<(), DealRecordingError> {
        self.record_act(format!("load {index}"))
    }
    fn start_recording(&mut self) -> Result<(), DealRecordingError> {
        self.record_act("start".to_owned())
    }
    fn stop_recording(&mut self) {
        self.recorder_acts.push("stop".to_owned());
    }
}

/// The fake window lends its own tabs, beside the config it lends a market
/// call and its rail's magnet.
impl OpenTabs for FakeWindow {
    type Tab = FakeTab;
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn open_tab(&self, index: usize) -> Option<&FakeTab> {
        self.tabs.get(index)
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn rail_magnet(&self) -> bool {
        self.rail_magnet
    }
}

/// A fake tab's panes carry no analysis.
impl AnalysisTab for FakeTab {
    fn analysis_panes(&self) -> Vec<AnalysisPaneEntry<'_>> {
        Vec::new()
    }
}

impl RailMagnetPort for FakeWindow {
    fn switch_rail_magnet(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.rail_magnet, on) != on
    }
}

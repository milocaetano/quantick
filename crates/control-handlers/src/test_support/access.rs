use super::*;

/// The fixed time a fake access tells a handler it runs at.
pub(crate) const FAKE_NOW_UNIX_MS: i64 = 100;

/// A fake access: a fixed number of notifications it allows, then the wait
/// it reports, and every event it journaled.
pub(crate) struct FakeAccess {
    pub budget: usize,
    pub retry_after: Duration,
    pub events: Vec<NewEvent>,
    /// The actor and client a replayed run recorded, when the call replays one.
    pub replayed: Option<(ActorKind, String)>,
}

impl FakeAccess {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            retry_after: Duration::from_millis(1_500),
            events: Vec::new(),
            replayed: None,
        }
    }
}

impl Default for FakeAccess {
    fn default() -> Self {
        Self::with_budget(0)
    }
}

impl JournalAccess for FakeAccess {
    fn record_event(&mut self, event: NewEvent) -> WireU64 {
        self.events.push(event);
        WireU64::new(self.events.len() as u64)
    }
    fn now_unix_ms(&self) -> i64 {
        FAKE_NOW_UNIX_MS
    }
}

impl AnnotateAccess for FakeAccess {
    fn recorded_actor(&self) -> Option<(ActorKind, &str)> {
        self.replayed
            .as_ref()
            .map(|(kind, client)| (*kind, client.as_str()))
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}

impl NotifyAccess for FakeAccess {
    fn allow_notification(&mut self, _actor: &ActorContext) -> Result<(), Duration> {
        if self.budget == 0 {
            return Err(self.retry_after);
        }
        self.budget -= 1;
        Ok(())
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}

/// A run paging toward `reach`, nothing pulled yet.
pub(crate) fn loading(reach: HistoryReach) -> RunStatus {
    RunStatus::Loading(ReachProgress {
        reach,
        oldest_ms: 0,
        sessions_reached: 0,
        traded_ms: 0,
        prints_pulled: 0,
        pages: 0,
    })
}

/// A buy print at `price`, the `id`th second of the epoch.
pub(crate) fn trade(id: u64, price: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: i64::try_from(id).expect("small id") * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// A window lending its tabs, by id in screen order, beside the config and
/// the rail's magnet: the fake host of every scope that walks [`OpenTabs`].
pub(crate) struct TabsWindow<T> {
    pub tabs: Vec<(u64, T)>,
    pub magnet: bool,
    pub config: AppConfig,
}

impl<T> TabsWindow<T> {
    pub fn new(tabs: Vec<(u64, T)>) -> Self {
        Self {
            tabs,
            magnet: false,
            config: fake_config(&[]),
        }
    }
}

impl<T> TabDirectory for TabsWindow<T> {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|(id, _)| *id == tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tabs[index].0
    }
}

impl<T> OpenTabs for TabsWindow<T> {
    type Tab = T;
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn open_tab(&self, index: usize) -> Option<&T> {
        self.tabs.get(index).map(|(_, tab)| tab)
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn rail_magnet(&self) -> bool {
        self.magnet
    }
}

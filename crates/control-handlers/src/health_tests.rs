//! The health projection over a fake window: what a tab's readings become on
//! the wire, and what the revision key ignores.
use super::*;

/// One tab's readings as plain fields a test sets.
#[derive(Default)]
struct FakeTab {
    integrity: Option<FeedIntegritySnapshot>,
    loading: LoadingTracker,
    arrival_latency_ms: Option<i64>,
    split: Option<FeedLatency>,
    gate: Option<Desync>,
    indicators: IndicatorViews,
}

impl HealthTab for FakeTab {
    fn feed_integrity(&self) -> FeedIntegritySnapshot {
        self.integrity.clone().unwrap_or(FeedIntegritySnapshot {
            anomalies: WireU64::new(0),
            missing_messages: WireU64::new(0),
            unknown_loss: WireU64::new(0),
            non_monotonic: WireU64::new(0),
        })
    }
    fn loading(&self) -> &LoadingTracker {
        &self.loading
    }
    fn arrival_latency_ms(&self) -> Option<i64> {
        self.arrival_latency_ms
    }
    fn latency_split(&self) -> Option<FeedLatency> {
        self.split
    }
    fn intake_lag(&self) -> Option<IntakeLagSnapshot> {
        None
    }
    fn trade_gate(&self) -> Option<&Desync> {
        self.gate.as_ref()
    }
    fn visit_panes(&self, visit: &mut dyn FnMut(&mut dyn Iterator<Item = PaneHealthRead<'_>>)) {
        visit(&mut std::iter::once(PaneHealthRead {
            pane_id: 7,
            side: PaneSideDto::Flow,
            indicators: &self.indicators,
            orderflow: None,
        }));
    }
}

#[derive(Default)]
struct FakeWindow {
    frame: FrameReading,
    tabs: Vec<FakeTab>,
}

impl HealthScopePort for FakeWindow {
    const HIGH_LAG_MS: i64 = 5_000;
    fn frame(&self) -> FrameReading {
        self.frame
    }
    fn visit_health_tabs(&self, visit: &mut TabVisit<'_>) {
        let tabs = self.tabs.iter().enumerate();
        visit(&mut tabs.map(|(index, tab)| (index as u64 + 1, tab as &dyn HealthTab)));
    }
}

fn one_tab(tab: FakeTab) -> FakeWindow {
    FakeWindow {
        tabs: vec![tab],
        ..FakeWindow::default()
    }
}

fn split(arrival_lag_ms: i64, hop: &'static str) -> FeedLatency {
    FeedLatency {
        arrival_lag_ms,
        source_lag_ms: Some(40),
        source_lag_peak_ms: Some(90),
        transport_lag_ms: Some(arrival_lag_ms - 40),
        hop: Some(hop),
        prints: 12,
    }
}

#[test]
fn a_window_that_measured_nothing_reports_no_frame_metrics() {
    let snapshot = snapshot(&one_tab(FakeTab::default()));
    assert_eq!(snapshot.frame.wall_average_ms, None);
    assert_eq!(snapshot.tabs.len(), 1);
    assert_eq!(snapshot.tabs[0].tab_id.get(), 1);
    let pane = &snapshot.tabs[0].panes[0];
    assert_eq!((pane.pane_id.get(), pane.indicator_count.get()), (7, 0));
    assert_eq!(pane.orderflow, None);
}

#[test]
fn integrity_is_reported_only_once_an_anomaly_was_counted() {
    let mut tab = FakeTab::default();
    assert_eq!(tab_health(1, &tab).feed_integrity, None);
    let counted = FeedIntegritySnapshot {
        anomalies: WireU64::new(2),
        missing_messages: WireU64::new(5),
        unknown_loss: WireU64::new(1),
        non_monotonic: WireU64::new(0),
    };
    tab.integrity = Some(counted.clone());
    assert_eq!(tab_health(1, &tab).feed_integrity, Some(counted));
}

#[test]
fn only_the_work_in_flight_is_listed_in_task_order() {
    let mut tab = FakeTab::default();
    tab.loading.begin(LoadingTask::VenueHistory);
    tab.loading.begin(LoadingTask::History);
    tab.loading.begin(LoadingTask::History);
    let listed: Vec<_> = tab_health(1, &tab)
        .active_loading_tasks
        .into_iter()
        .map(|task| (task.task, task.operation_count.get()))
        .collect();
    assert_eq!(
        listed,
        [("history".to_owned(), 2), ("venue_history".to_owned(), 1)]
    );
}

#[test]
fn a_tab_with_nothing_measured_has_no_tape_and_a_split_fills_every_hop() {
    let mut tab = FakeTab::default();
    assert_eq!(tape_health(&tab), None);
    tab.arrival_latency_ms = Some(300);
    let alone = tape_health(&tab).expect("an arrival alone is a reading");
    assert_eq!(alone.feed_arrival_latency_ms, None);
    assert_eq!(alone.sampled_prints.get(), 0);
    tab.split = Some(split(250, "bridge"));
    let tape = tape_health(&tab).expect("a split tape");
    assert_eq!(tape.arrival_latency_ms, Some(300));
    assert_eq!(tape.feed_arrival_latency_ms, Some(250));
    assert_eq!(tape.source_latency_ms, Some(40));
    assert_eq!(tape.source_latency_peak_ms, Some(90));
    assert_eq!(tape.transport_latency_ms, Some(210));
    assert_eq!(tape.dominant_hop.as_deref(), Some("bridge"));
    assert_eq!(tape.sampled_prints.get(), 12);
}

#[test]
fn the_revision_ignores_frame_cost_and_exact_lag_and_moves_when_the_tape_turns_late() {
    let mut window = one_tab(FakeTab {
        arrival_latency_ms: Some(300),
        ..FakeTab::default()
    });
    let before = revision(&window);
    window.frame.wall_average_ms = Some(16.5);
    window.tabs[0].arrival_latency_ms = Some(900);
    assert!(
        revision(&window) == before,
        "frame and lag move every frame"
    );
    window.tabs[0].arrival_latency_ms = Some(FakeWindow::HIGH_LAG_MS + 1);
    assert!(revision(&window) != before, "the tape turned late");
}

fn paused(tape_age_ms: i64) -> Desync {
    use quantick_sources::sync::{LinkState, SyncLimits, SyncReading};
    Desync {
        reading: SyncReading::TapeAge,
        link: LinkState::Up,
        latency_ms: None,
        latency_hop: None,
        tape_age_ms: Some(tape_age_ms),
        limits: SyncLimits::default(),
        notice: None,
    }
}

#[test]
fn a_paused_tab_reports_its_cause_and_the_revision_moves_on_its_edges_only() {
    let mut window = one_tab(FakeTab::default());
    assert_eq!(snapshot(&window).tabs[0].trade_gate, None, "entries open");
    let open = revision(&window);
    window.tabs[0].gate = Some(paused(6_000));
    let gate = snapshot(&window).tabs[0]
        .trade_gate
        .clone()
        .expect("a paused tab says why");
    assert_eq!(
        (gate.reading.as_str(), gate.link.as_str()),
        ("tape_age", "up")
    );
    assert_eq!(
        (gate.tape_age_ms, gate.max_tape_age_ms, gate.max_latency_ms),
        (Some(6_000), 5_000, 500)
    );
    assert!(gate.sentence.contains("6.0 s old"), "{}", gate.sentence);
    let entered = revision(&window);
    assert!(entered != open, "the pause began");
    window.tabs[0].gate = Some(paused(7_500));
    assert!(revision(&window) == entered, "an ageing tape is not news");
}

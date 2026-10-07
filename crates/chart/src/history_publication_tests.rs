use super::*;
use quantick_engine::{BarSpec, Side};
use rust_decimal::Decimal;

#[derive(Default)]
struct ManualRunner {
    job: Option<(HistoryRebuild, Arc<AtomicBool>)>,
    result: Option<ChartState>,
}

impl ManualRunner {
    fn complete(&mut self) {
        let (job, cancelled) = self.job.take().expect("scheduled recut");
        self.result = job.run(|| cancelled.load(Ordering::Relaxed));
    }
}

impl HistoryRunner for ManualRunner {
    fn start(&mut self, job: HistoryRebuild, cancelled: Arc<AtomicBool>) -> bool {
        self.job = Some((job, cancelled));
        true
    }
    fn finished(&mut self) -> Result<Option<ChartState>, HistoryRunnerStopped> {
        Ok(self.result.take())
    }
    fn retire(&mut self, _state: ChartState) {}
}

fn trade(id: u64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: id as i64 * 1000,
        price: Decimal::from(100 + id % 5),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn displayed() -> ChartState {
    let mut state = ChartState::new(BarSpec::Tick(3));
    state.ingest_backfill(&(20..30).map(trade).collect::<Vec<_>>());
    state
}

#[test]
fn multi_day_pages_coalesce_and_live_catchup_is_bounded_before_publication() {
    let mut state = displayed();
    let mut publication = HistoryPublication::<ManualRunner>::default();
    publication.enqueue(Arc::new((10..20).map(trade).collect()));
    assert!(!publication.poll(&state));
    publication.enqueue(Arc::new((1..10).map(trade).collect()));
    assert_eq!(
        state.trades().len(),
        10,
        "the readable chart stays in place"
    );
    publication.runner.complete();
    assert!(
        !publication.poll(&state),
        "queued pages join the next complete recut"
    );
    publication.runner.complete();
    for id in 30..5030 {
        state.ingest_live(&trade(id));
    }
    assert!(!publication.poll(&state));
    assert_eq!(
        publication.active.as_ref().unwrap().caught_up,
        10 + LIVE_CATCHUP_TRADES
    );
    assert!(publication.take_ready(&state).is_none());
    assert!(publication.poll(&state));
    let candidate = publication.take_ready(&state).unwrap();
    state.prepend_history(&(1..20).map(trade).collect::<Vec<_>>());
    assert_eq!(candidate.bars(), state.bars());
    assert_eq!(candidate.partial(), state.partial());
    let candidate_tape: Vec<_> = candidate.trades().iter().collect();
    let expected_tape: Vec<_> = state.trades().iter().collect();
    assert_eq!(candidate_tape, expected_tape);
    assert_eq!(candidate.backfill_boundary(), state.backfill_boundary());
}

#[test]
fn a_spec_change_restarts_pending_pages_and_source_cancel_rejects_late_results() {
    let mut state = displayed();
    let mut publication = HistoryPublication::<ManualRunner>::default();
    publication.enqueue(Arc::new((1..20).map(trade).collect()));
    publication.poll(&state);
    let old_cancel = Arc::clone(&publication.active.as_ref().unwrap().cancelled);
    state.set_spec(BarSpec::Time(5000));
    assert!(!publication.poll(&state));
    assert!(old_cancel.load(Ordering::Relaxed));
    publication.runner.complete();
    assert!(publication.poll(&state));
    assert_eq!(publication.take_ready(&state).unwrap().spec(), state.spec());
    publication.enqueue(Arc::new(vec![trade(0)]));
    publication.poll(&state);
    let cancelled = Arc::clone(&publication.active.as_ref().unwrap().cancelled);
    publication.cancel();
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(!publication.pending());
    let empty = ChartState::new(BarSpec::Tick(3));
    assert!(publication.poll(&empty));
    assert!(publication.take_ready(&state).is_none());
}

#[derive(Default)]
struct RefusedRunner;
impl HistoryRunner for RefusedRunner {
    fn start(&mut self, _job: HistoryRebuild, _cancelled: Arc<AtomicBool>) -> bool {
        false
    }
    fn finished(&mut self) -> Result<Option<ChartState>, HistoryRunnerStopped> {
        Err(HistoryRunnerStopped)
    }
    fn retire(&mut self, _state: ChartState) {}
}

#[test]
fn repeated_worker_failures_stop_and_a_retry_keeps_all_accepted_pages() {
    let state = displayed();
    let mut publication = HistoryPublication::<RefusedRunner>::default();
    publication.enqueue(Arc::new((1..20).map(trade).collect()));
    publication.poll(&state);
    publication.poll(&state);
    assert!(publication.failed());
    assert!(!publication.pending());
    assert_eq!(publication.queued[0].len(), 19);
    for _ in 0..10 {
        assert!(publication.poll(&state));
    }
    assert_eq!(
        publication.failures, 2,
        "the stopped worker cannot spin on every frame"
    );
    assert!(publication.retry());
    assert!(publication.pending());
    assert_eq!(
        publication.queued[0].len(),
        19,
        "retry does not fetch or drop accepted history"
    );
}

#[test]
fn a_held_publication_keeps_every_page_and_recuts_once_on_release() {
    let state = displayed();
    let mut publication = HistoryPublication::<ManualRunner>::default();
    publication.hold(true);
    publication.enqueue(Arc::new((10..20).map(trade).collect()));
    assert!(!publication.poll(&state), "nothing is published while held");
    assert!(publication.runner.job.is_none(), "and no recut starts");
    publication.enqueue(Arc::new((1..10).map(trade).collect()));
    assert!(!publication.poll(&state));
    assert!(publication.pending(), "the pages are kept, not dropped");
    publication.hold(false);
    assert!(!publication.poll(&state));
    publication.runner.complete();
    assert!(publication.poll(&state));
    let candidate = publication
        .take_ready(&state)
        .expect("one recut of both pages");
    assert_eq!(candidate.trades().len(), 29);
    assert!(publication.runner.job.is_none(), "exactly one recut ran");
}

/// A pane built while pages wait gets every one of them: the page a recut is
/// working on and the pages held behind it, oldest arrival first.
#[test]
fn the_unpublished_pages_are_every_page_not_yet_on_the_display() {
    let state = displayed();
    let mut publication = HistoryPublication::<ManualRunner>::default();
    assert!(publication.unpublished_pages().is_empty());
    publication.enqueue(Arc::new((10..20).map(trade).collect()));
    publication.poll(&state);
    assert!(publication.runner.job.is_some(), "a recut is working on it");
    publication.hold(true);
    publication.enqueue(Arc::new((1..10).map(trade).collect()));
    let lengths: Vec<usize> = publication
        .unpublished_pages()
        .iter()
        .map(|page| page.len())
        .collect();
    assert_eq!(lengths, [10, 9]);
    publication.hold(false);
    publication.runner.complete();
    while !publication.poll(&state) {
        if publication.runner.job.is_some() {
            publication.runner.complete();
        }
    }
    publication.take_ready(&state).expect("both pages published");
    assert!(
        publication.unpublished_pages().is_empty(),
        "a published page is no longer owed to anyone"
    );
}
